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
/// rexenv drives Caddy's admin API over a **unix socket** (not the default
/// unauthenticated TCP `:2019`) so the ROOT edge exposes no local-TCP control
/// surface — any local process reaching `:2019` could otherwise POST config to a
/// root Caddy = arbitrary file read/write as root (task 2.3 / H5). The socket lives
/// in our config dir with owner-only (0600) perms; for the privileged edge it is
/// chown'd to the invoking user so reload/stop stay promptless (see
/// [`start_privileged`]). A fixed path so a leftover edge is found deterministically.
pub const ADMIN_SOCKET_FILE: &str = "caddy-admin.sock";

/// Path of Caddy's admin unix socket under the platform config dir.
pub fn admin_socket_path(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().config_dir()?.join(ADMIN_SOCKET_FILE))
}

/// Caddy admin address for the CLI `--address` / Caddyfile `admin` directive.
fn admin_address(sock: &Path) -> String {
    format!("unix//{}", sock.display())
}

/// Whether OUR edge's admin socket is accepting connections (liveness). The socket
/// FILE persists after a crash, so we actually connect rather than stat the path.
pub fn admin_alive(platform: &dyn Platform) -> bool {
    match admin_socket_path(platform) {
        Ok(sock) => std::os::unix::net::UnixStream::connect(&sock).is_ok(),
        Err(_) => false,
    }
}

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
    /// Admin API unix socket to bind (`Some` in production via [`admin_socket_path`]).
    /// `None` omits the `admin` directive (Caddy's default TCP admin) — tests only.
    pub admin_socket: Option<PathBuf>,
}

impl Default for CaddyConfig {
    fn default() -> Self {
        Self {
            http_port: DEFAULT_HTTP_PORT,
            https_port: DEFAULT_HTTPS_PORT,
            routes: Vec::new(),
            admin_socket: None,
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
    if let Some(sock) = &cfg.admin_socket {
        // Bind the admin API to a unix socket instead of the default TCP :2019 (H5).
        // Quote the whole token — app-data paths contain spaces; `|0600` sets
        // owner-only perms (Caddy 2.7+; the privileged edge additionally chowns it).
        s.push_str(&format!("\tadmin \"unix//{}|0600\"\n", sock.display()));
    }
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
    let sock = admin_socket_path(platform)?;
    let appdata = platform.paths().app_data_dir()?;
    // Start the edge as root (binds :80/:443), THEN hand its admin unix socket to the
    // invoking user with 0600 perms so rexenv can drive reload/stop with NO further
    // prompt while no other local process (or user) can reach it (task 2.3 / H5). The
    // uid comes from our app-data dir's owner (the invoking user), so it works for any
    // account. `&&` keeps a failed `caddy start` a failure (the chown group's exit
    // code must not mask it); the chown/chmod are best-effort (`|| true`).
    let cmd = format!(
        "{caddy} start --config {cfg} --adapter caddyfile && {{ \
         for i in 1 2 3 4 5 6 7 8 9 10; do [ -S {sock} ] && break; sleep 0.2; done ; \
         chown $(stat -f %u {appdata}) {sock} 2>/dev/null || true ; \
         chmod 600 {sock} 2>/dev/null || true ; }}",
        caddy = sh_quote(caddy_bin),
        cfg = sh_quote(caddyfile),
        sock = sh_quote(&sock),
        appdata = sh_quote(&appdata),
    );
    platform.privileges().run_privileged(&cmd)?;
    Ok(())
}

/// Reload Caddy's config via its admin API (no privilege needed even though
/// Caddy may run as root). Run after writing a new Caddyfile (e.g. site added).
pub fn reload(platform: &dyn Platform, caddy_bin: &Path, caddyfile: &Path) -> Result<()> {
    let sock = admin_socket_path(platform)?;
    let args = vec![
        "reload".to_string(),
        "--config".to_string(),
        caddyfile.display().to_string(),
        "--adapter".to_string(),
        "caddyfile".to_string(),
        // Reach the admin over our unix socket, not the (now absent) TCP :2019.
        "--address".to_string(),
        admin_address(&sock),
    ];
    wait_ok(platform.supervisor().spawn(caddy_bin, &args)?, "caddy reload")
}

/// Stop Caddy via its admin API (no privilege) over our unix socket.
pub fn stop_admin(platform: &dyn Platform, caddy_bin: &Path) -> Result<()> {
    let sock = admin_socket_path(platform)?;
    wait_ok(
        platform.supervisor().spawn(
            caddy_bin,
            &["stop".to_string(), "--address".to_string(), admin_address(&sock)],
        )?,
        "caddy stop",
    )
}

/// Recover from a LEFTOVER edge before starting ours (Phase 2 §7.3).
///
/// A Caddy orphaned by a prior run (commonly a **root** edge from a `:443` start)
/// keeps holding `:443` and its admin socket. rexenv's admin socket is chown'd to
/// the invoking user, so we can stop even a root edge without privilege: detect a
/// live leftover on our socket and stop it so a fresh start isn't blocked.
///
/// No-op when our admin socket has no live listener. Returns an actionable error if
/// the leftover edge can't be stopped. Ownership-gated: because we probe/stop OUR
/// private socket (never the default TCP `:2019`), a developer's own Caddy on `:2019`
/// is never touched by launch (task 2.4 / M1).
pub fn recover_stale_edge(platform: &dyn Platform, caddy_bin: &Path) -> Result<()> {
    if !admin_alive(platform) {
        return Ok(());
    }
    log::warn!(
        "rexenv: a leftover rexenv Caddy edge is live on its admin socket; \
         stopping it via the admin API so startup isn't blocked"
    );
    // Best-effort: `caddy stop` POSTs to the admin socket; judge success by the
    // socket actually going quiet below.
    let _ = stop_admin(platform, caddy_bin);
    for _ in 0..10 {
        if !admin_alive(platform) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err(crate::error::Error::Other(
        "the leftover Caddy edge would not stop via its admin socket. Stop it and retry \
         (e.g. `caddy stop`, or kill the leftover caddy process)."
            .to_string(),
    ))
}

/// Stop the Caddy edge via its admin API (our unix socket) and wait for it to go
/// quiet. Works on a **root** edge (the socket is chown'd to us, so no privilege
/// needed) and on a stray edge we never tracked (a leftover from a prior run). No-op
/// if our socket has no live listener. This is what `stop_all` uses so "Stop all"
/// reliably frees `:443`/`:80`.
///
/// Ownership-gated (task 2.4 / M1): both steps act ONLY on rexenv's own edge — the
/// admin stop targets our private unix socket (a foreign Caddy on the default TCP
/// `:2019` is invisible to us), and the reap matches our own caddy binary path. A
/// developer's own Caddy is never stopped by launch or Stop-all.
pub fn stop_edge(platform: &dyn Platform, caddy_bin: &Path) -> Result<()> {
    // 1) Graceful: if our edge's admin socket is live, stop via the API + wait for it.
    if admin_alive(platform) {
        let _ = stop_admin(platform, caddy_bin);
        for _ in 0..10 {
            if !admin_alive(platform) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // 2) Reap any Caddy still running OUR binary — including a wedged, listener-less
    //    edge the admin API can't reach (e.g. a crashed start that lost its sockets).
    //    Best-effort: killing a ROOT edge needs privilege, so a root remnant may
    //    survive — it holds no ports, and we log it rather than fail.
    let marker = caddy_bin.display().to_string();
    for pid in platform.supervisor().owned_pids(&marker) {
        let _ = platform.supervisor().stop(pid);
    }
    let survivors = platform.supervisor().owned_pids(&marker);
    if !survivors.is_empty() {
        log::warn!(
            "rexenv: {} Caddy process(es) using our binary could not be reaped \
             (likely a root edge — needs privilege): {survivors:?}",
            survivors.len()
        );
    }

    // 3) If our admin socket is somehow STILL live, surface it.
    if admin_alive(platform) {
        return Err(crate::error::Error::Other(
            "the Caddy edge would not stop via its admin socket after `caddy stop`.".to_string(),
        ));
    }
    Ok(())
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
            admin_socket: None,
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
    fn admin_binds_unix_socket_not_tcp() {
        // No admin_socket → no admin directive emitted (tests only).
        let mut cfg = sample();
        assert!(!generate_caddyfile(&cfg).contains("admin "));
        // With a socket → admin bound to a quoted unix socket with 0600 perms, and
        // NOT the default unauthenticated TCP :2019 (task 2.3 / H5). The quoting also
        // covers app-data paths that contain spaces ("Application Support").
        cfg.admin_socket = Some("/x/app data/config/caddy-admin.sock".into());
        let f = generate_caddyfile(&cfg);
        assert!(
            f.contains("admin \"unix///x/app data/config/caddy-admin.sock|0600\""),
            "caddyfile:\n{f}"
        );
        assert!(!f.contains("2019"));
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
