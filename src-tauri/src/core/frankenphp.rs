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

/// The `php_server` block: always starts with `env HTTPS {https_on}` — the
/// map's output (see [`generate_config`]) — so PHP sees `HTTPS=on` exactly when
/// the edge forwarded an https request; the site's own env vars (§1.6) follow
/// as `env NAME "value"` lines at `depth` tabs. Values escaped for the
/// double-quoted Caddyfile string (`site_env::escape_value`); `{`/`}`/`$`/
/// control chars were rejected at validation, and `HTTPS` is a reserved name
/// there, so a user var can never clash with this line.
fn php_server(depth: usize, env: &[(String, String)]) -> String {
    let t = "\t".repeat(depth);
    let lines: String = env
        .iter()
        .map(|(name, value)| {
            format!("{t}\tenv {name} \"{}\"\n", crate::core::site_env::escape_value(value))
        })
        .collect();
    format!("{t}php_server {{\n{t}\tenv HTTPS {{https_on}}\n{lines}{t}}}\n")
}

/// Dot-segment guard: 404 for any path with a dot-leading segment (`/.git/…`,
/// `/x/.env`), except the root `/.well-known/` subtree (ACME/plugin probes).
/// Docroots carry `.git` (git-cloned plugins) and tunnels make a served docroot
/// PUBLIC. Two matchers because Go's RE2 has no lookahead (can't mirror the
/// nginx template's `(?!well-known)`): root-level dot except well-known, plus
/// any NESTED dot-segment — the nested rule also denies `/.well-known/.hidden`,
/// keeping parity with the nginx/Apache templates. At site level Caddy's
/// canonical directive order runs `respond` before `php_server`; inside a
/// `route` block order is literal, so the caller puts the guard FIRST.
fn dotfile_guard(depth: usize) -> String {
    let t = "\t".repeat(depth);
    format!(
        "{t}@dot_root {{\n\
         {t}\tpath_regexp ^/\\.\n\
         {t}\tnot path_regexp ^/\\.well-known(/|$)\n\
         {t}}}\n\
         {t}respond @dot_root 404\n\
         {t}@dot_nested path_regexp ^/.+/\\.\n\
         {t}respond @dot_nested 404\n"
    )
}

/// The site block body for a rewrite mode. Single/subdomain use the high-level
/// `php_server` (its built-in `try_files … /index.php` is the single-site rule);
/// subdirectory multisite adds WordPress's network path rewrites.
fn site_body(mode: RewriteMode, env: &[(String, String)]) -> String {
    match mode {
        RewriteMode::Single | RewriteMode::SubdomainMultisite => {
            format!("{}{}", dotfile_guard(1), php_server(1, env))
        }
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
            format!(
                "\troute {{\n\
                 {guard}\
                 \t\t@wpadmin {{\n\
                 \t\t\tnot file\n\
                 \t\t\tpath_regexp ^(/[^/]+)?/wp-admin$\n\
                 \t\t}}\n\
                 \t\tredir @wpadmin {{path}}/ permanent\n\
                 \t\t@wpstrip {{\n\
                 \t\t\tnot file\n\
                 \t\t\tpath_regexp wpstrip ^(/[^/]+)?(/wp-.*)$\n\
                 \t\t}}\n\
                 \t\trewrite @wpstrip {{http.regexp.wpstrip.2}}\n\
                 \t\t@phpstrip {{\n\
                 \t\t\tnot file\n\
                 \t\t\tpath_regexp phpstrip ^(/[^/]+)?(/.*\\.php)$\n\
                 \t\t}}\n\
                 \t\trewrite @phpstrip {{http.regexp.phpstrip.2}}\n\
                 {php_server}\
                 \t}}\n",
                guard = dotfile_guard(2),
                php_server = php_server(2, env),
            )
        }
    }
}

/// Render a FrankenPHP Caddyfile serving ONE site's docroot via embedded PHP on
/// an internal loopback HTTP port. Auto-HTTPS + admin are disabled (it's a backend
/// behind the edge). Matches any Host on the port (the single edge route targets it).
/// `env` (§1.6, validated by `site_env::validate`) becomes `env` lines in the
/// `php_server` block — per-site is natural here (one backend per site).
///
/// The edge terminates TLS, so the backend maps Caddy's `X-Forwarded-Proto:
/// https` to `HTTPS=on` for PHP (a `map` feeding `php_server`'s `env HTTPS` —
/// the same contract as the nginx vhosts' `fastcgi_param HTTPS $rexenv_https`
/// and core::apache's mod_rewrite `E=HTTPS:on`); without it WordPress
/// `is_ssl()` is false behind the edge (http:// asset URLs, broken wp-admin
/// login). `trusted_proxies` does NOT do this — live-verified on FrankenPHP
/// 1.12.4: with it, `HTTPS` stayed empty. A request without the header maps to
/// `""`, which `is_ssl()` treats as false.
pub fn generate_config(
    docroot: &Path,
    port: u16,
    mode: RewriteMode,
    env: &[(String, String)],
) -> String {
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
         \tmap {{header.X-Forwarded-Proto}} {{https_on}} {{\n\
         \t\thttps on\n\
         \t\tdefault \"\"\n\
         \t}}\n\
         {body}\
         }}\n",
        root = docroot.display(),
        body = site_body(mode, env),
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
    env: &[(String, String)],
) -> Result<PathBuf> {
    let conf = config_path(platform, domain)?;
    if let Some(dir) = conf.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&conf, generate_config(docroot, port, mode, env))?;
    Ok(conf)
}

/// Start a FrankenPHP backend (foreground) with the given config, via
/// `ProcessSupervisor`. stdout/stderr go to a per-site log. `env` (§1.6) is set
/// as REAL process environment — per-site by construction (one process per
/// site) — because FrankenPHP's SAPI `getenv()` reads only the process environ
/// (the config `env` lines cover `$_SERVER`; process env covers
/// `getenv()`/`$_ENV`). Live-verified: with config-lines only, the probe showed
/// getenv()=false, $_ENV=null.
pub fn start(
    platform: &dyn Platform,
    frankenphp_bin: &Path,
    domain: &str,
    conf: &Path,
    env: &[(String, String)],
) -> Result<Child> {
    let args = vec![
        "run".to_string(),
        "--config".to_string(),
        conf.display().to_string(),
        "--adapter".to_string(),
        "caddyfile".to_string(),
    ];
    let log = log_path(platform, domain)?;
    platform.supervisor().spawn_logged_env(frankenphp_bin, &args, &log, env)
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
        let cfg = generate_config(Path::new("/Sites/fp/public"), 8200, RewriteMode::Single, &[]);
        // Backend, not edge: no auto-HTTPS, no admin endpoint, loopback only.
        assert!(cfg.contains("auto_https off"));
        assert!(cfg.contains("admin off"));
        assert!(cfg.contains("default_bind 127.0.0.1"));
        // Embedded PHP runtime + the site on its internal port.
        assert!(cfg.contains("frankenphp"));
        assert!(cfg.contains(":8200 {"));
        assert!(cfg.contains("root * \"/Sites/fp/public\""));
        assert!(cfg.contains("php_server"));
        // Edge-terminated TLS reaches PHP as HTTPS=on (WordPress is_ssl()):
        // X-Forwarded-Proto maps to the env line, empty when absent.
        assert!(cfg.contains("map {header.X-Forwarded-Proto} {https_on} {"));
        assert!(cfg.contains("\t\thttps on"));
        assert!(cfg.contains("\t\tdefault \"\""));
        assert!(cfg.contains("env HTTPS {https_on}"));
        // Never terminates TLS itself.
        assert!(!cfg.contains("tls "));
    }

    #[test]
    fn env_vars_render_as_a_php_server_block_and_empty_env_is_byte_stable() {
        let env = vec![("API_URL".into(), "https://x.test".into()), ("Q".into(), "say \"hi\"".into())];
        let single = generate_config(Path::new("/d"), 8200, RewriteMode::Single, &env);
        assert!(single.contains("php_server {"), "got: {single}");
        assert!(single.contains("env API_URL \"https://x.test\""));
        assert!(single.contains("env Q \"say \\\"hi\\\"\""));

        // Subdirectory multisite keeps its route rules AND gets the env block.
        let subdir = generate_config(Path::new("/d"), 8200, RewriteMode::SubdirectoryMultisite, &env);
        assert!(subdir.contains("rewrite @wpstrip"));
        assert!(subdir.contains("env API_URL \"https://x.test\""));

        // No env → the php_server block still carries the HTTPS map line (and
        // nothing else), and user env lines come AFTER it.
        let bare = generate_config(Path::new("/d"), 8200, RewriteMode::Single, &[]);
        assert!(bare.contains("\tphp_server {\n\t\tenv HTTPS {https_on}\n\t}\n"));
        assert!(
            single.find("env HTTPS {https_on}").unwrap()
                < single.find("env API_URL").unwrap()
        );
    }

    #[test]
    fn dotfile_guard_denies_dot_segments_except_root_well_known() {
        // Root dot-segment (minus /.well-known/) + any nested dot-segment →
        // 404. Two matchers because RE2 has no lookahead; the nested rule also
        // denies /.well-known/.hidden (parity with nginx/Apache).
        let single = generate_config(Path::new("/d"), 8200, RewriteMode::Single, &[]);
        assert!(single.contains("respond @dot_root 404"), "got: {single}");
        assert!(single.contains("not path_regexp ^/\\.well-known(/|$)"));
        assert!(single.contains("@dot_nested path_regexp ^/.+/\\."));
        assert!(single.contains("respond @dot_nested 404"));
        // Subdirectory multisite: inside a route block order is LITERAL — the
        // guard must precede the WP rewrites and php_server.
        let subdir =
            generate_config(Path::new("/d"), 8200, RewriteMode::SubdirectoryMultisite, &[]);
        let guard = subdir.find("respond @dot_root 404").unwrap();
        assert!(guard > subdir.find("route {").unwrap());
        assert!(guard < subdir.find("@wpadmin").unwrap());
        assert!(guard < subdir.find("php_server").unwrap());
    }

    #[test]
    fn subdirectory_multisite_mirrors_the_nginx_network_rewrites() {
        let single = generate_config(Path::new("/d"), 8200, RewriteMode::Single, &[]);
        let subdir = generate_config(Path::new("/d"), 8200, RewriteMode::SubdirectoryMultisite, &[]);
        let sub = generate_config(Path::new("/d"), 8200, RewriteMode::SubdomainMultisite, &[]);

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
