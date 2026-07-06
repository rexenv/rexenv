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
//!
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
/// subdirectory multisite adds WordPress's network path rewrites.
fn site_body(mode: RewriteMode) -> String {
    match mode {
        RewriteMode::Single | RewriteMode::SubdomainMultisite => "\tphp_server\n".to_string(),
        RewriteMode::SubdirectoryMultisite => {
            // WordPress subdirectory-multisite: faithfully mirror the proven nginx
            // rules (`services::rewrite_block`) as Caddy directives. A `route` block
            // preserves this exact order (Caddy would otherwise auto-sort directives),
            // and each rule is guarded by `not file` — Caddy's equivalent of nginx's
            // `if (!-e $request_filename)`:
            //   1. redirect `/<site>/wp-admin` → `…/wp-admin/`
            //   2. strip the `/<site>` prefix before `/wp-*`  (capture group 2)
            //   3. strip the `/<site>` prefix before `*.php`  (capture group 2)
            // The two rewrites share Caddy's rewrite group, so only the first match
            // fires (like nginx's `last`). Validated with `frankenphp adapt`/`validate`.
            "\troute {\n\
             \t\t@wpadmin {\n\
             \t\t\tnot file\n\
             \t\t\tpath_regexp ^(/[^/]+)?/wp-admin$\n\
             \t\t}\n\
             \t\tredir @wpadmin {path}/ permanent\n\
             \t\t@wpstrip {\n\
             \t\t\tnot file\n\
             \t\t\tpath_regexp wpstrip ^(/[^/]+)?(/wp-.*)$\n\
             \t\t}\n\
             \t\trewrite @wpstrip {http.regexp.wpstrip.2}\n\
             \t\t@phpstrip {\n\
             \t\t\tnot file\n\
             \t\t\tpath_regexp phpstrip ^(/[^/]+)?(/.*\\.php)$\n\
             \t\t}\n\
             \t\trewrite @phpstrip {http.regexp.phpstrip.2}\n\
             \t\tphp_server\n\
             \t}\n"
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

/// Per-site FrankenPHP config path (named per site so several backends coexist).
pub fn config_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform
        .paths()
        .config_dir()?
        .join(format!("frankenphp-{domain}.Caddyfile")))
}

/// Per-site FrankenPHP backend log (stdout+stderr).
pub fn log_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform
        .paths()
        .log_dir()?
        .join(format!("frankenphp-{domain}-stdout.log")))
}

/// Write the FrankenPHP config for `domain` (on `port`) under the config dir.
/// Returns the config path.
pub fn write_config(
    platform: &dyn Platform,
    domain: &str,
    docroot: &Path,
    port: u16,
    mode: RewriteMode,
) -> Result<PathBuf> {
    let conf = config_path(platform, domain)?;
    if let Some(dir) = conf.parent() {
        std::fs::create_dir_all(dir)?;
    }
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
    let log = log_path(platform, domain)?;
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
    fn subdirectory_multisite_mirrors_the_nginx_network_rewrites() {
        let single = generate_config(Path::new("/d"), 8200, RewriteMode::Single);
        let subdir = generate_config(Path::new("/d"), 8200, RewriteMode::SubdirectoryMultisite);
        let sub = generate_config(Path::new("/d"), 8200, RewriteMode::SubdomainMultisite);

        // Single + subdomain route like a single site: plain php_server, no rewrites.
        for cfg in [&single, &sub] {
            assert!(cfg.contains("php_server"));
            assert!(!cfg.contains("route {"));
            assert!(!cfg.contains("rewrite "));
            assert!(!cfg.contains("redir "));
        }

        // Subdirectory multisite adds WordPress's three network rules, ordered inside
        // a `route` block (so Caddy can't reorder them) and each guarded by `not file`.
        assert!(subdir.contains("route {"));
        assert!(subdir.contains("redir @wpadmin {path}/ permanent"));
        assert!(subdir.contains("not file"));
        assert!(subdir.contains("php_server"));
        // Each placeholder name MUST match its `path_regexp` name and reference group 2.
        // The old code shipped `{http.regexp.wpsubph.2}` for a regexp named `wpsubphp`
        // (a typo) → the placeholder resolved empty → every sub-site `.php` broke.
        assert!(subdir.contains("path_regexp wpstrip ^(/[^/]+)?(/wp-.*)$"));
        assert!(subdir.contains("rewrite @wpstrip {http.regexp.wpstrip.2}"));
        assert!(subdir.contains("path_regexp phpstrip ^(/[^/]+)?(/.*\\.php)$"));
        assert!(subdir.contains("rewrite @phpstrip {http.regexp.phpstrip.2}"));
        // The stale, buggy matcher names are gone.
        assert!(!subdir.contains("wpsubph"));
        assert!(!subdir.contains("wpsubdir"));
    }

    #[test]
    fn running_false_on_closed_port() {
        assert!(!running(9));
    }
}
