//! core::apache — Apache httpd as a per-site OVERRIDE server (TODO "Deferred
//! services").
//!
//! httpd runs ONLY as a backend on an internal loopback HTTP port — NEVER the
//! edge (same contract as `core::frankenphp`): the edge Caddy keeps :443 + TLS
//! + Host routing and reverse-proxies to this backend. Unlike FrankenPHP,
//! Apache has no embedded PHP: `.php` is handed to the site's PHP version's
//! SHARED php-fpm pool via `mod_proxy_fcgi` — the same pools nginx sites use,
//! so per-version PHP settings apply identically.
//!
//! Why Apache at all: `.htaccess`. The generated vhost sets
//! `AllowOverride All`, so plugin/theme `.htaccess` rules behave like on
//! classic WP hosting — the thing the shared nginx can't emulate.
//!
//! The conf is generated per site from the BUNDLE tree (`ServerRoot` = the
//! resolved `httpd-<ver>` cache dir; `LoadModule` paths resolve against it —
//! only the modules the conf loads are bundled, and mod_ssl/mod_http2 are
//! deliberately not among them). Compiled-in default paths inside the bottle
//! are `@@HOMEBREW_PREFIX@@` placeholders and must never be trusted — every
//! path the server touches (pidfile, runtime dir, logs, mime map) is explicit.

use crate::core::services::RewriteMode;
use crate::error::Result;
use crate::platform::traits::Platform;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

/// Base loopback port for per-site Apache backends. Distinct from FrankenPHP's
/// 8200–8299 so a site switching server kinds can never collide with itself.
pub const APACHE_BASE_PORT: u16 = 8300;

/// Deterministic loopback backend port for an Apache-override site, in
/// `8300..8400` (same FNV-1a scheme as `frankenphp::site_port`).
pub fn site_port(domain: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in domain.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    APACHE_BASE_PORT + (h % 100) as u16
}

/// `httpd` inside the bundle tree.
pub fn httpd_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/httpd")
}

/// The modules the generated conf loads, in load order. mpm_event + unixd
/// first (the MPM must exist before anything else initializes), proxy before
/// proxy_fcgi (module dependency).
const MODULES: &[(&str, &str)] = &[
    ("mpm_event_module", "mod_mpm_event.so"),
    ("unixd_module", "mod_unixd.so"),
    ("authz_core_module", "mod_authz_core.so"),
    ("dir_module", "mod_dir.so"),
    ("mime_module", "mod_mime.so"),
    ("env_module", "mod_env.so"),
    ("log_config_module", "mod_log_config.so"),
    ("rewrite_module", "mod_rewrite.so"),
    ("proxy_module", "mod_proxy.so"),
    ("proxy_fcgi_module", "mod_proxy_fcgi.so"),
];

/// Per-mode request routing. Single + subdomain-multisite need only the WP
/// front-controller fallback (mod_dir's `FallbackResource`); subdirectory
/// multisite mirrors WordPress's canonical network `.htaccess` rules in
/// server context (leading slashes; existence checks via DOCUMENT_ROOT —
/// `%{REQUEST_FILENAME}` isn't mapped yet at this phase). A site's own
/// `.htaccess` (AllowOverride All) still applies per-directory afterwards.
/// `RewriteEngine On` is emitted unconditionally by [`generate_config`] (the
/// HTTPS map needs it), so the modes only add their rules.
fn routing(mode: RewriteMode) -> &'static str {
    match mode {
        RewriteMode::Single | RewriteMode::SubdomainMultisite => {
            "FallbackResource /index.php\n"
        }
        RewriteMode::SubdirectoryMultisite => {
            "RewriteRule ^/index\\.php$ - [L]\n\
             RewriteRule ^/([_0-9a-zA-Z-]+/)?wp-admin$ /$1wp-admin/ [R=301,L]\n\
             RewriteCond %{DOCUMENT_ROOT}%{REQUEST_URI} -f [OR]\n\
             RewriteCond %{DOCUMENT_ROOT}%{REQUEST_URI} -d\n\
             RewriteRule ^ - [L]\n\
             RewriteRule ^/([_0-9a-zA-Z-]+/)?(wp-(content|admin|includes).*) /$2 [L]\n\
             RewriteRule ^/([_0-9a-zA-Z-]+/)?(.*\\.php)$ /$2 [L]\n\
             RewriteRule . /index.php [L]\n"
        }
    }
}

/// Render the per-site httpd.conf: loopback listener, the site's docroot with
/// `.htaccess` enabled, `.php` → the site's php-fpm pool over FastCGI, and the
/// WP routing for its multisite mode. The edge terminates TLS, so the backend
/// maps Caddy's `X-Forwarded-Proto: https` to the `HTTPS` env var via
/// mod_rewrite (mod_proxy_fcgi forwards it as a FastCGI param — the nginx
/// vhosts' `fastcgi_param HTTPS $rexenv_https` equivalent); without it
/// WordPress `is_ssl()` is false: http:// asset URLs (mixed content) and a
/// broken wp-admin login. mod_setenvif isn't bundled, hence mod_rewrite. `env` (§1.6, validated by
/// `site_env::validate`) becomes `SetEnv` lines — mod_env adds them to the
/// request's subprocess env, which mod_proxy_fcgi forwards as FastCGI params
/// (the same per-REQUEST delivery as nginx's `fastcgi_param` lines; the shared
/// pool is untouched). All paths quoted (app-data paths contain spaces).
///
/// Dot-segment guard: mod_alias isn't bundled (no `RedirectMatch`), so the
/// deny is a mod_rewrite `[R=404]` placed BEFORE the WP routing — subdirectory
/// multisite's `-f/-d` passthrough (`RewriteRule ^ - [L]`) would otherwise
/// L-stop first and serve an existing `.git/config`. Root `/.well-known/`
/// exempt (ACME/app probes); this also covers `.htaccess` itself, which the
/// generated conf never protected (no stock `<Files ".ht*">` block here).
#[allow(clippy::too_many_arguments)] // flat mirror of the site's serving inputs
pub fn generate_config(
    basedir: &Path,
    docroot: &Path,
    domain: &str,
    port: u16,
    fpm_port: u16,
    mode: RewriteMode,
    env: &[(String, String)],
    run_dir: &Path,
    log_dir: &Path,
) -> String {
    let modules: String = MODULES
        .iter()
        .map(|(name, file)| format!("LoadModule {name} \"lib/httpd/modules/{file}\"\n"))
        .collect();
    let set_env: String = env
        .iter()
        .map(|(name, value)| {
            format!("SetEnv {name} \"{}\"\n", crate::core::site_env::escape_value(value))
        })
        .collect();
    format!(
        "ServerRoot \"{basedir}\"\n\
         Listen 127.0.0.1:{port}\n\
         {modules}\
         ServerName {domain}\n\
         PidFile \"{run}/apache-{domain}.pid\"\n\
         DefaultRuntimeDir \"{run}\"\n\
         ErrorLog \"{logs}/apache-{domain}-error.log\"\n\
         LogLevel warn\n\
         TypesConfig \"{basedir}/.bottle/etc/httpd/mime.types\"\n\
         DocumentRoot \"{docroot}\"\n\
         DirectoryIndex index.php index.html\n\
         <Directory \"{docroot}\">\n\
         \tOptions FollowSymLinks\n\
         \tAllowOverride All\n\
         \tRequire all granted\n\
         </Directory>\n\
         <FilesMatch \"\\.php$\">\n\
         \tSetHandler \"proxy:fcgi://127.0.0.1:{fpm_port}\"\n\
         </FilesMatch>\n\
         {set_env}\
         RewriteEngine On\n\
         RewriteCond %{{HTTP:X-Forwarded-Proto}} =https\n\
         RewriteRule .* - [E=HTTPS:on]\n\
         RewriteRule \"(^|/)\\.(?!well-known(/|$))\" - [R=404,L]\n\
         {routing}",
        basedir = basedir.display(),
        run = run_dir.display(),
        logs = log_dir.display(),
        docroot = docroot.display(),
        routing = routing(mode),
    )
}

/// Per-site httpd conf path (named per site so several backends coexist).
pub fn config_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform
        .paths()
        .config_dir()?
        .join(format!("apache-{domain}.conf")))
}

/// Per-site Apache backend log (stdout+stderr of the master; the conf's
/// `ErrorLog` is a separate, richer file).
pub fn log_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform
        .paths()
        .log_dir()?
        .join(format!("apache-{domain}-stdout.log")))
}

/// The runtime dir (pidfile, mutexes) under app-data.
pub fn run_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("run"))
}

/// Render the desired conf for a site from platform paths (the reconcile's
/// config-diff input — must be byte-identical to what [`write_config`] writes).
#[allow(clippy::too_many_arguments)]
pub fn desired_config(
    platform: &dyn Platform,
    basedir: &Path,
    docroot: &Path,
    domain: &str,
    port: u16,
    fpm_port: u16,
    mode: RewriteMode,
    env: &[(String, String)],
) -> Result<String> {
    Ok(generate_config(
        basedir,
        docroot,
        domain,
        port,
        fpm_port,
        mode,
        env,
        &run_dir(platform)?,
        &platform.paths().log_dir()?,
    ))
}

/// Write the httpd conf for `domain` under the config dir; returns its path.
#[allow(clippy::too_many_arguments)]
pub fn write_config(
    platform: &dyn Platform,
    basedir: &Path,
    domain: &str,
    docroot: &Path,
    port: u16,
    fpm_port: u16,
    mode: RewriteMode,
    env: &[(String, String)],
) -> Result<PathBuf> {
    let conf = config_path(platform, domain)?;
    if let Some(dir) = conf.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::create_dir_all(run_dir(platform)?)?;
    std::fs::create_dir_all(platform.paths().log_dir()?)?;
    let content =
        desired_config(platform, basedir, docroot, domain, port, fpm_port, mode, env)?;
    std::fs::write(&conf, content)?;
    Ok(conf)
}

/// Start an Apache backend (foreground master) with the given conf via
/// `ProcessSupervisor`. The `-f` conf path carries the app-data dir on the
/// cmdline — the adoption ownership marker, like redis's `--dir`.
pub fn start(platform: &dyn Platform, basedir: &Path, domain: &str, conf: &Path) -> Result<Child> {
    let args = vec![
        "-f".to_string(),
        conf.display().to_string(),
        "-DFOREGROUND".to_string(),
    ];
    let log = log_path(platform, domain)?;
    platform.supervisor().spawn_logged(&httpd_bin(basedir), &args, &log)
}

/// Stop a running Apache backend by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// True if an Apache backend is accepting connections on its loopback port.
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

    fn cfg(mode: RewriteMode, env: &[(String, String)]) -> String {
        generate_config(
            Path::new("/cache/httpd-2.4.68"),
            Path::new("/Sites/ap site/public"),
            "ap.rex",
            8300,
            9783,
            mode,
            env,
            Path::new("/data/run"),
            Path::new("/data/logs"),
        )
    }

    #[test]
    fn ports_stay_in_the_apache_override_range_and_differ_from_frankenphp() {
        for domain in ["a.rex", "verylongdomainname.rex", "x.test"] {
            let p = site_port(domain);
            assert!((8300..8400).contains(&p), "{domain} → {p}");
            // Same hash, different base: an Apache backend can never collide
            // with the SAME site's FrankenPHP port.
            assert_eq!(
                p - APACHE_BASE_PORT,
                crate::core::frankenphp::site_port(domain)
                    - crate::core::frankenphp::FRANKENPHP_BASE_PORT
            );
        }
        assert_eq!(site_port("a.rex"), site_port("a.rex"));
    }

    #[test]
    fn config_is_a_loopback_backend_with_htaccess_and_fpm_handoff() {
        let c = cfg(RewriteMode::Single, &[]);
        // Loopback listener only — never the edge, never TLS.
        assert!(c.contains("Listen 127.0.0.1:8300"));
        assert!(!c.contains("mod_ssl"));
        assert!(!c.contains("443"));
        // .php → the site's SHARED php-fpm pool (not an embedded PHP).
        assert!(c.contains("SetHandler \"proxy:fcgi://127.0.0.1:9783\""));
        // Edge-terminated TLS reaches PHP as HTTPS=on (WordPress is_ssl()).
        assert!(c.contains("RewriteCond %{HTTP:X-Forwarded-Proto} =https"));
        assert!(c.contains("RewriteRule .* - [E=HTTPS:on]"));
        // The point of Apache: .htaccess honored.
        assert!(c.contains("AllowOverride All"));
        // Placeholder-free explicit paths, quoted (spaces in app-data paths).
        assert!(c.contains("DocumentRoot \"/Sites/ap site/public\""));
        assert!(c.contains("PidFile \"/data/run/apache-ap.rex.pid\""));
        assert!(c.contains("TypesConfig \"/cache/httpd-2.4.68/.bottle/etc/httpd/mime.types\""));
        assert!(!c.contains("@@HOMEBREW"));
        // Every LoadModule is in the bundle's include list — a conf loading an
        // unbundled module would fail at spawn on a fresh cache.
        use crate::core::binaries;
        use crate::platform::traits::Arch;
        let bundle =
            binaries::bundle_manifest("httpd", binaries::HTTPD_VERSION, "macos", Arch::Arm64)
                .expect("httpd bundle pinned");
        let includes = bundle.parts[0].include;
        for (_, file) in MODULES {
            let rel = format!("lib/httpd/modules/{file}");
            assert!(includes.iter().any(|i| *i == rel), "{rel} not bundled");
        }
    }

    #[test]
    fn routing_matches_the_rewrite_mode() {
        // Single + subdomain: front-controller fallback; the only rewrite
        // rules are the HTTPS map's + the dotfile deny (before the routing).
        for mode in [RewriteMode::Single, RewriteMode::SubdomainMultisite] {
            let c = cfg(mode, &[]);
            assert!(c.contains("FallbackResource /index.php"));
            assert_eq!(c.matches("RewriteEngine On").count(), 1);
            assert_eq!(c.matches("RewriteRule").count(), 2);
        }
        // Subdirectory multisite mirrors WP's canonical network rules, after
        // the HTTPS map (env rules must precede the [L] short-circuits).
        let c = cfg(RewriteMode::SubdirectoryMultisite, &[]);
        assert_eq!(c.matches("RewriteEngine On").count(), 1);
        assert!(c.find("[E=HTTPS:on]").unwrap() < c.find("wp-admin$").unwrap());
        assert!(c.contains("wp-admin$ /$1wp-admin/ [R=301,L]"));
        assert!(c.contains("RewriteCond %{DOCUMENT_ROOT}%{REQUEST_URI} -f [OR]"));
        assert!(c.contains("(wp-(content|admin|includes).*) /$2 [L]"));
        assert!(c.contains("(.*\\.php)$ /$2 [L]"));
        assert!(c.contains("RewriteRule . /index.php [L]"));
        assert!(!c.contains("FallbackResource"));
    }

    #[test]
    fn dotfile_paths_return_404_before_wp_routing() {
        // `.git`/`.env` inside a served docroot must 404 (root /.well-known/
        // exempt). The deny must precede the mode routing: subdirectory
        // multisite's `-f/-d` passthrough (`RewriteRule ^ - [L]`) would
        // otherwise L-stop first and serve an existing `.git/config`.
        for mode in [
            RewriteMode::Single,
            RewriteMode::SubdomainMultisite,
            RewriteMode::SubdirectoryMultisite,
        ] {
            let c = cfg(mode, &[]);
            assert!(
                c.contains("RewriteRule \"(^|/)\\.(?!well-known(/|$))\" - [R=404,L]"),
                "got: {c}"
            );
            let deny = c.find("[R=404,L]").unwrap();
            let routing = match mode {
                RewriteMode::SubdirectoryMultisite => {
                    c.find("RewriteCond %{DOCUMENT_ROOT}").unwrap()
                }
                _ => c.find("FallbackResource").unwrap(),
            };
            assert!(deny < routing, "deny must precede routing");
        }
    }

    #[test]
    fn env_vars_render_as_setenv_and_empty_env_is_byte_stable() {
        let env = vec![
            ("API_URL".into(), "https://x.test".into()),
            ("Q".into(), "say \"hi\"".into()),
        ];
        let c = cfg(RewriteMode::Single, &env);
        assert!(c.contains("SetEnv API_URL \"https://x.test\""));
        assert!(c.contains("SetEnv Q \"say \\\"hi\\\"\""));
        // No env → no SetEnv lines at all (reconcile's config diff stays quiet).
        assert!(!cfg(RewriteMode::Single, &[]).contains("SetEnv"));
    }

    #[test]
    fn running_false_on_closed_port() {
        assert!(!running(9));
    }
}
