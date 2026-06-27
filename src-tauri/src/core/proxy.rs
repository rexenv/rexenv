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

/// One site's TLS termination + upstream.
#[derive(Debug, Clone)]
pub struct SiteRoute {
    /// Host served, e.g. `mysite.test`.
    pub host: String,
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
/// internal issuer/ACME; auto HTTP→HTTPS redirects are disabled so Caddy only
/// does what we configure.
pub fn generate_caddyfile(cfg: &CaddyConfig) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("\tauto_https disable_redirects\n");
    s.push_str(&format!("\thttp_port {}\n", cfg.http_port));
    s.push_str(&format!("\thttps_port {}\n", cfg.https_port));
    s.push_str("}\n");
    for r in &cfg.routes {
        s.push('\n');
        s.push_str(&format!("https://{} {{\n", r.host));
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
                upstream: "127.0.0.1:9999".into(),
                cert_path: "/c/cert.pem".into(),
                key_path: "/c/key.pem".into(),
            }],
        }
    }

    #[test]
    fn caddyfile_uses_explicit_tls_not_internal_issuer() {
        let f = generate_caddyfile(&sample());
        assert!(f.contains("auto_https disable_redirects"));
        assert!(f.contains("http_port 8080"));
        assert!(f.contains("https_port 8443"));
        assert!(f.contains("https://proxytest.test {"));
        assert!(f.contains("tls \"/c/cert.pem\" \"/c/key.pem\""));
        assert!(f.contains("reverse_proxy 127.0.0.1:9999"));
        // Never falls back to Caddy's internal CA.
        assert!(!f.to_lowercase().contains("internal"));
    }

    #[test]
    fn caddyfile_renders_each_route() {
        let mut cfg = sample();
        cfg.routes.push(SiteRoute {
            host: "two.test".into(),
            upstream: "127.0.0.1:9001".into(),
            cert_path: "/c/2.pem".into(),
            key_path: "/c/2.key".into(),
        });
        let f = generate_caddyfile(&cfg);
        assert!(f.contains("https://proxytest.test {"));
        assert!(f.contains("https://two.test {"));
        assert_eq!(f.matches("reverse_proxy").count(), 2);
    }
}
