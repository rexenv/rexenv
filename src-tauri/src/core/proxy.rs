//! core::proxy — edge router (Caddy) config generation + supervision (task 4.2).
//!
//! Caddy terminates TLS for every `*.test` site using the per-site certs issued
//! by our local CA (3.2) via explicit `tls <cert> <key>` directives. Caddy's
//! automatic HTTPS (ACME + its own internal CA) is therefore never used — the
//! chain the browser sees is signed by the CA trusted in 3.4. Caddy proxies to
//! the shared Nginx (one upstream per site by Host).

use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::Child;

pub const CADDYFILE: &str = "Caddyfile";
pub const DEFAULT_HTTP_PORT: u16 = 80;
pub const DEFAULT_HTTPS_PORT: u16 = 443;
/// Caddy's admin endpoint port (its default). A single fixed port so a leftover
/// edge can be found + stopped deterministically — see [`recover_stale_edge`].
pub const CADDY_ADMIN_PORT: u16 = 2019;

/// One site's TLS termination + upstream.
#[derive(Debug, Clone)]
pub struct SiteRoute {
    /// Host served, e.g. `mysite.test`.
    pub host: String,
    /// Also match `*.host` (subdomain multisite, §10.2). The wildcard sub-site
    /// hosts share this site's backend + wildcard cert; a more-specific exact
    /// host (any other site) still wins, so it can't overshadow `other.test`.
    pub wildcard: bool,
    /// Upstream `host:port` Caddy proxies to (the shared Nginx).
    pub upstream: String,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Edge-router configuration.
#[derive(Debug, Clone)]
pub struct CaddyConfig {
    pub http_port: u16,
    pub https_port: u16,
    pub routes: Vec<SiteRoute>,
}

impl Default for CaddyConfig {
    fn default() -> Self {
        Self {
            http_port: DEFAULT_HTTP_PORT,
            https_port: DEFAULT_HTTPS_PORT,
            routes: Vec::new(),
        }
    }
}

/// Render the Caddyfile. Explicit per-site `tls` means Caddy never invokes its
/// internal issuer/ACME (it uses the loaded local-CA certs). Auto-HTTPS is left
/// on so Caddy also binds `http_port` and 308-redirects HTTP→HTTPS for every
/// site host — `http://site.test` lands on `https://site.test`.
pub fn generate_caddyfile(cfg: &CaddyConfig) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("\thttp_port {}\n", cfg.http_port));
    s.push_str(&format!("\thttps_port {}\n", cfg.https_port));
    s.push_str("}\n");
    for r in &cfg.routes {
        s.push('\n');
        // Subdomain multisite serves the apex plus every sub-site host; Caddy
        // matches the most-specific site address first, so the exact hosts of
        // other sites are never shadowed by this `*.host` matcher.
        let site_addr = if r.wildcard {
            format!("https://{host}, https://*.{host}", host = r.host)
        } else {
            format!("https://{}", r.host)
        };
        s.push_str(&format!("{site_addr} {{\n"));
        // Quote paths: app-data paths contain spaces ("Application Support").
        s.push_str(&format!(
            "\ttls \"{}\" \"{}\"\n",
            r.cert_path.display(),
            r.key_path.display()
        ));
        s.push_str(&format!("\treverse_proxy {}\n", r.upstream));
        s.push_str("}\n");
    }
    s
}

/// Write the Caddyfile under the platform config dir; returns its path.
pub fn write_caddyfile(platform: &dyn Platform, cfg: &CaddyConfig) -> Result<PathBuf> {
    let dir = platform.paths().config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(CADDYFILE);
    std::fs::write(&path, generate_caddyfile(cfg))?;
    Ok(path)
}

/// Start Caddy with the given Caddyfile via `ProcessSupervisor`; returns the
/// child handle (track its pid to `stop`).
pub fn start(platform: &dyn Platform, caddy_bin: &Path, caddyfile: &Path) -> Result<Child> {
    let args = vec![
        "run".to_string(),
        "--config".to_string(),
        caddyfile.display().to_string(),
        "--adapter".to_string(),
        "caddyfile".to_string(),
    ];
    // Caddy logs (JSON) to stderr — capture it to a per-service log file.
    let log = platform.paths().log_dir()?.join("caddy-stdout.log");
    platform.supervisor().spawn_logged(caddy_bin, &args, &log)
}

/// Stop a running Caddy by pid (for the non-privileged `start`).
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// Single-quote a path for the /bin/sh command (app-data paths contain spaces).
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.display())
}

/// Start Caddy on privileged ports (:80/:443) as root via `PrivilegeManager`
/// (one admin prompt). `caddy start` backgrounds the server and returns once
/// it's up, so the prompt doesn't block. Afterwards, [`reload`] and [`stop_admin`]
/// drive it through Caddy's localhost admin API — no further prompts.
pub fn start_privileged(platform: &dyn Platform, caddy_bin: &Path, caddyfile: &Path) -> Result<()> {
    let cmd = format!(
        "{} start --config {} --adapter caddyfile",
        sh_quote(caddy_bin),
        sh_quote(caddyfile)
    );
    platform.privileges().run_privileged(&cmd)?;
    Ok(())
}

/// Reload Caddy's config via its admin API (no privilege needed even though
/// Caddy may run as root). Run after writing a new Caddyfile (e.g. site added).
pub fn reload(platform: &dyn Platform, caddy_bin: &Path, caddyfile: &Path) -> Result<()> {
    let args = vec![
        "reload".to_string(),
        "--config".to_string(),
        caddyfile.display().to_string(),
        "--adapter".to_string(),
        "caddyfile".to_string(),
    ];
    wait_ok(platform.supervisor().spawn(caddy_bin, &args)?, "caddy reload")
}

/// Stop Caddy via its admin API (no privilege).
pub fn stop_admin(platform: &dyn Platform, caddy_bin: &Path) -> Result<()> {
    wait_ok(
        platform.supervisor().spawn(caddy_bin, &["stop".to_string()])?,
        "caddy stop",
    )
}

/// True if Caddy's admin endpoint port is currently bound (i.e. some Caddy — or
/// another process — is already there).
pub fn admin_in_use() -> bool {
    crate::core::ports::is_listening(CADDY_ADMIN_PORT)
}

/// Recover from a LEFTOVER edge before starting ours (Phase 2 §7.3).
///
/// A Caddy orphaned by a prior run (commonly a **root** edge from a `:443` start)
/// keeps its admin endpoint on `:2019`, which makes a fresh `caddy run` die with
/// `bind: address already in use`. Caddy's admin API lets any local user stop it
/// without privilege (even a root edge stops itself on request), so here we detect
/// that case and stop the stale edge so startup isn't blocked.
///
/// No-op when `:2019` is free. Returns an actionable error if the port is held and
/// can't be freed (e.g. a non-Caddy process is squatting on it).
pub fn recover_stale_edge(platform: &dyn Platform, caddy_bin: &Path) -> Result<()> {
    if !admin_in_use() {
        return Ok(());
    }
    log::warn!(
        "rexenv: a leftover Caddy is holding admin port :{CADDY_ADMIN_PORT}; \
         stopping it via the admin API so startup isn't blocked"
    );
    // Best-effort: `caddy stop` POSTs to the admin endpoint; ignore its exit code
    // (we judge success by the port actually freeing below).
    let _ = stop_admin(platform, caddy_bin);
    for _ in 0..10 {
        if !admin_in_use() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err(crate::error::Error::Other(format!(
        "Caddy admin port :{CADDY_ADMIN_PORT} is held and could not be freed — a non-Caddy \
         process may be using it, or a stale edge won't stop. Free it and retry (e.g. \
         `caddy stop`, or kill the process listening on :{CADDY_ADMIN_PORT})."
    )))
}

fn wait_ok(mut child: Child, what: &str) -> Result<()> {
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(crate::error::Error::Other(format!(
            "{what} failed (exit {:?})",
            status.code()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> CaddyConfig {
        CaddyConfig {
            http_port: 8080,
            https_port: 8443,
            routes: vec![SiteRoute {
                host: "proxytest.test".into(),
                wildcard: false,
                upstream: "127.0.0.1:9999".into(),
                cert_path: "/c/cert.pem".into(),
                key_path: "/c/key.pem".into(),
            }],
        }
    }

    #[test]
    fn caddyfile_uses_explicit_tls_not_internal_issuer() {
        let f = generate_caddyfile(&sample());
        // Auto-HTTPS left on (no `disable_redirects`) so Caddy opens :80 and
        // redirects HTTP→HTTPS; explicit per-site tls still bypasses the issuer.
        assert!(!f.contains("disable_redirects"));
        assert!(f.contains("http_port 8080"));
        assert!(f.contains("https_port 8443"));
        assert!(f.contains("https://proxytest.test {"));
        assert!(f.contains("tls \"/c/cert.pem\" \"/c/key.pem\""));
        assert!(f.contains("reverse_proxy 127.0.0.1:9999"));
        // Never falls back to Caddy's internal CA.
        assert!(!f.to_lowercase().contains("internal"));
    }

    #[test]
    fn admin_port_is_caddy_default() {
        // Fixed so a leftover edge is found + stopped deterministically (§7.3).
        assert_eq!(CADDY_ADMIN_PORT, 2019);
    }

    #[test]
    fn caddyfile_renders_each_route() {
        let mut cfg = sample();
        cfg.routes.push(SiteRoute {
            host: "two.test".into(),
            wildcard: false,
            upstream: "127.0.0.1:9001".into(),
            cert_path: "/c/2.pem".into(),
            key_path: "/c/2.key".into(),
        });
        let f = generate_caddyfile(&cfg);
        assert!(f.contains("https://proxytest.test {"));
        assert!(f.contains("https://two.test {"));
        assert_eq!(f.matches("reverse_proxy").count(), 2);
    }

    #[test]
    fn subdomain_multisite_route_matches_wildcard_without_shadowing_others() {
        let mut cfg = sample();
        // A subdomain-multisite site: apex + wildcard sub-site hosts.
        cfg.routes.push(SiteRoute {
            host: "mysite.test".into(),
            wildcard: true,
            upstream: "127.0.0.1:8088".into(),
            cert_path: "/c/m.pem".into(),
            key_path: "/c/m.key".into(),
        });
        let f = generate_caddyfile(&cfg);
        // The site address carries both the apex and the wildcard sub-site host.
        assert!(f.contains("https://mysite.test, https://*.mysite.test {"));
        // The wildcard is scoped to mysite.test — a plain site keeps its own exact
        // (non-wildcard) address, so it is never shadowed by `*.mysite.test`.
        assert!(f.contains("https://proxytest.test {"));
        assert!(!f.contains("*.proxytest.test"));
    }
}
