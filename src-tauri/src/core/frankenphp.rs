//! core::frankenphp — FrankenPHP as a per-site OVERRIDE server (Phase 2 §2).
//!
//! FrankenPHP is Caddy + an embedded PHP runtime. Here it runs ONLY as a backend
//! on an internal loopback HTTP port — NEVER the edge. The Phase-1 edge Caddy keeps
//! :443 + TLS + Host routing and reverse-proxies to this backend. So FrankenPHP's
//! own features that would clash with the edge are DISABLED in its config:
//!   - `auto_https off` — never bind :443 or issue/serve certs (TLS is the edge's job);
//!   - `admin off` — never grab the :2019 admin port the edge owns (and so multiple
//!     per-site backends don't fight over it);
//!   - `default_bind 127.0.0.1` — listen on loopback only.
//! PHP is served by FrankenPHP's embedded runtime (`php_server`), NOT the §1
//! php-fpm pools — so an override site's PHP version is the FrankenPHP build's.

use crate::core::services::RewriteMode;
use crate::platform::traits::Platform;
use crate::error::Result;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

/// Base loopback port for per-site FrankenPHP backends (the override range).
pub const FRANKENPHP_BASE_PORT: u16 = 8200;

/// Deterministic loopback backend port for a FrankenPHP-override site, in the
/// override range `8200..8300`. Stable for a given domain so the edge route and
/// the running backend agree without a side channel. (FNV-1a hash; §4's allocator
/// will replace this with recorded, collision-free ports.)
pub fn site_port(domain: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in domain.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    FRANKENPHP_BASE_PORT + (h % 100) as u16
}

/// The site block body for a rewrite mode. Single/subdomain use the high-level
/// `php_server` (its built-in `try_files … /index.php` is the single-site rule);
/// subdirectory multisite prepends WordPress's network path rewrites. Only the
/// Single path is exercised in Phase 2; the slots exist so Phase 3 needs no refactor.
fn site_body(mode: RewriteMode) -> String {
    match mode {
        RewriteMode::Single | RewriteMode::SubdomainMultisite => "\tphp_server\n".to_string(),
        RewriteMode::SubdirectoryMultisite => {
            // WordPress subdirectory-multisite: strip the leading /<site>/ segment
            // from wp-admin/wp-includes/wp-content and *.php, then serve via PHP.
            "\t@wpsubdir path_regexp wpsubdir ^(/[^/]+)?(/wp-(content|admin|includes)/.*)$\n\
             \trewrite @wpsubdir {http.regexp.wpsubdir.2}\n\
             \t@wpsubdirphp path_regexp wpsubphp ^(/[^/]+)?(/.*\\.php)$\n\
             \trewrite @wpsubdirphp {http.regexp.wpsubph.2}\n\
             \tphp_server\n"
                .to_string()
        }
    }
}

/// Render a FrankenPHP Caddyfile serving ONE site's docroot via embedded PHP on
/// an internal loopback HTTP port. Auto-HTTPS + admin are disabled (it's a backend
/// behind the edge). Matches any Host on the port (the single edge route targets it).
pub fn generate_config(docroot: &Path, port: u16, mode: RewriteMode) -> String {
    format!(
        "{{\n\
         \tauto_https off\n\
         \tadmin off\n\
         \tdefault_bind 127.0.0.1\n\
         \tfrankenphp\n\
         }}\n\
         \n\
         :{port} {{\n\
         \troot * \"{root}\"\n\
         {body}\
         }}\n",
        root = docroot.display(),
        body = site_body(mode),
    )
}

/// Write the FrankenPHP config for `domain` (on `port`) under the config dir.
/// Returns the config path. The file is named per site so several backends coexist.
pub fn write_config(
    platform: &dyn Platform,
    domain: &str,
    docroot: &Path,
    port: u16,
    mode: RewriteMode,
) -> Result<PathBuf> {
    let config_dir = platform.paths().config_dir()?;
    std::fs::create_dir_all(&config_dir)?;
    let conf = config_dir.join(format!("frankenphp-{domain}.Caddyfile"));
    std::fs::write(&conf, generate_config(docroot, port, mode))?;
    Ok(conf)
}

/// Start a FrankenPHP backend (foreground) with the given config, via
/// `ProcessSupervisor`. stdout/stderr go to a per-site log.
pub fn start(
    platform: &dyn Platform,
    frankenphp_bin: &Path,
    domain: &str,
    conf: &Path,
) -> Result<Child> {
    let args = vec![
        "run".to_string(),
        "--config".to_string(),
        conf.display().to_string(),
        "--adapter".to_string(),
        "caddyfile".to_string(),
    ];
    let log = platform
        .paths()
        .log_dir()?
        .join(format!("frankenphp-{domain}-stdout.log"));
    platform.supervisor().spawn_logged(frankenphp_bin, &args, &log)
}

/// Stop a running FrankenPHP backend by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// True if a FrankenPHP backend is accepting connections on its loopback port.
pub fn running(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_a_loopback_backend_with_no_edge_features() {
        let cfg = generate_config(Path::new("/Sites/fp/public"), 8200, RewriteMode::Single);
        // Backend, not edge: no auto-HTTPS, no admin endpoint, loopback only.
        assert!(cfg.contains("auto_https off"));
        assert!(cfg.contains("admin off"));
        assert!(cfg.contains("default_bind 127.0.0.1"));
        // Embedded PHP runtime + the site on its internal port.
        assert!(cfg.contains("frankenphp"));
        assert!(cfg.contains(":8200 {"));
        assert!(cfg.contains("root * \"/Sites/fp/public\""));
        assert!(cfg.contains("php_server"));
        // Never terminates TLS itself.
        assert!(!cfg.contains("tls "));
    }

    #[test]
    fn rewrite_slots_differ_by_mode() {
        let single = generate_config(Path::new("/d"), 8200, RewriteMode::Single);
        let subdir = generate_config(Path::new("/d"), 8200, RewriteMode::SubdirectoryMultisite);
        // Single has no WP network rewrites; subdirectory multisite does.
        assert!(!single.contains("wpsubdir"));
        assert!(subdir.contains("wpsubdir"));
        // Subdomain routes like single (plain php_server).
        let sub = generate_config(Path::new("/d"), 8200, RewriteMode::SubdomainMultisite);
        assert!(sub.contains("php_server"));
        assert!(!sub.contains("wpsubdir"));
    }

    #[test]
    fn running_false_on_closed_port() {
        assert!(!running(9));
    }
}
