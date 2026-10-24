//! core::apache — Apache httpd as a per-site OVERRIDE server (shipped from the
//! "Deferred services" plan; evidence in docs/archive/SHIPPED-2026-07.md).
//!
//! httpd runs ONLY as a backend on an internal loopback HTTP port — NEVER the
//! edge (same contract as `core::frankenphp`): the edge Caddy keeps :443,
//! TLS, and Host routing, and reverse-proxies to this backend. Unlike FrankenPHP,
//! Apache has no embedded PHP: `.php` is handed to the site's PHP version's
//! SHARED php-fpm pool via `mod_proxy_fcgi` — the same pools nginx sites use,
//! so per-version PHP settings apply identically.
//!
//! Why Apache at all: `.htaccess`. The generated vhost sets
//! `AllowOverride All`, so plugin/theme `.htaccess` rules behave like on
//! classic WP hosting — the thing the shared nginx can't emulate.
//!
//! The conf is generated per site from the resolved `httpd-<ver>` cache dir
//! (`ServerRoot`; `LoadModule` paths resolve against it). On macOS that dir is
//! the merged Homebrew BUNDLE (only the modules the conf loads are bundled, and
//! mod_ssl/mod_http2 are deliberately not among them; compiled-in default paths
//! are `@@HOMEBREW_PREFIX@@` placeholders and must never be trusted); on
//! Windows it is Apache Lounge's zip tree (`bin/httpd.exe`, `modules/`,
//! `conf/mime.types`; the MPM is mpm_winnt, compiled in). The five places the
//! two trees differ are [`Layout`]'s fields, keyed by OS the way `binaries.rs`
//! keys its catalog — ONE conf text with those holes (docs/archive/PLAN-windows-servers.md
//! D4). Every path the server touches (pidfile, runtime dir, logs, mime map) is explicit.

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

/// Where the pieces of an httpd tree live on `os` — the per-OS VALUES the one conf text
/// is rendered with (docs/archive/PLAN-windows-servers.md D4). `core/` names no OS by `cfg`; it
/// takes `os` as data, as `binaries::shape_of_on` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The server binary, relative to the tree (`bin/httpd` · `bin/httpd.exe`).
    pub bin: &'static str,
    /// Where `LoadModule` finds the `.so` files (the bottle's `lib/httpd/modules` · the
    /// zip's `modules`).
    pub modules_dir: &'static str,
    /// The real mime map (the bottle stages `etc/` under `.bottle/`; the zip has `conf/`).
    pub mime_types: &'static str,
    /// Whether the MPM is a loadable module: mpm_event + unixd on POSIX hosts; on Windows
    /// mpm_winnt is compiled into httpd.exe and `mod_unixd` does not exist.
    pub loadable_mpm: bool,
    /// What follows `proxy:fcgi://127.0.0.1:<port>` in the handler, and the directive that
    /// fixes `SCRIPT_FILENAME` after it. mod_proxy_fcgi appends the script's filesystem path
    /// to that URL; on Windows the path starts with a drive letter, so `…:9783C:/Users/…`
    /// parses as host `127.0.0.1:9783c` ("DNS lookup failure for: 127.0.0.1:9783c", measured
    /// on the Win11 VM, 24 Oct 2026). A trailing slash keeps the port whole, and then the
    /// FPM-style fixup hands PHP `/C:/Users/…` — "No input file specified" — so
    /// `SCRIPT_FILENAME` is rebuilt from `DOCUMENT_ROOT` + the request's `SCRIPT_NAME`
    /// (`FallbackResource` has already made that `/index.php` for a pretty URL). Both
    /// measured 200 with `HTTPS=on` through php-cgi 8.3.36; empty on POSIX hosts.
    pub fcgi_url_suffix: &'static str,
    pub fcgi_fixup: &'static str,
}

impl Layout {
    /// The layout `os` resolves.
    pub const fn for_os(os: &str) -> Layout {
        // `const fn` cannot compare &str with `==`; byte-compare the one OS that differs.
        if matches!(os.as_bytes(), b"windows") {
            Layout {
                bin: "bin/httpd.exe",
                modules_dir: "modules",
                mime_types: "conf/mime.types",
                loadable_mpm: false,
                fcgi_url_suffix: "/",
                fcgi_fixup: "ProxyFCGISetEnvIf \"true\" SCRIPT_FILENAME \"%{DOCUMENT_ROOT}%{reqenv:SCRIPT_NAME}\"\n",
            }
        } else {
            Layout {
                bin: "bin/httpd",
                modules_dir: "lib/httpd/modules",
                mime_types: ".bottle/etc/httpd/mime.types",
                loadable_mpm: true,
                fcgi_url_suffix: "",
                fcgi_fixup: "",
            }
        }
    }
}

/// A path as a generated server conf spells it on `os`: forward slashes on Windows
/// (httpd and Caddy both accept `C:/…`, and a backslash inside a quoted Caddyfile string
/// is an escape), the path as-is elsewhere. Shared with `core::frankenphp`.
pub fn path_for_conf(os: &str, path: &Path) -> String {
    let s = path.display().to_string();
    if os == "windows" {
        s.replace('\\', "/")
    } else {
        s
    }
}

/// The layout for THIS host.
pub fn layout() -> Layout {
    Layout::for_os(std::env::consts::OS)
}

/// The server binary inside the resolved tree, for this host.
pub fn httpd_bin(basedir: &Path) -> PathBuf {
    basedir.join(layout().bin)
}

/// The MPM and its helper, loaded FIRST where they are modules (the MPM must exist before
/// anything else initializes) — POSIX hosts only (`Layout::loadable_mpm`).
const MPM_MODULES: &[(&str, &str)] = &[
    ("mpm_event_module", "mod_mpm_event.so"),
    ("unixd_module", "mod_unixd.so"),
];

/// The modules the generated conf loads on every OS, in load order: proxy before
/// proxy_fcgi (module dependency). The Windows mirror gate asserts this exact set exists
/// in Apache Lounge's zip (`rexenv/runtimes` `httpd-windows.yml`).
const MODULES: &[(&str, &str)] = &[
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
        RewriteMode::SubdirectoryMultisite => SUBDIRECTORY_MULTISITE_RULES,
    }
}

/// WordPress's canonical subdirectory-network rules in SERVER context (leading slashes;
/// existence checked through `DOCUMENT_ROOT` because `REQUEST_FILENAME` is not mapped yet
/// there). Shared with `core::openlitespeed`, whose vhost-level rewrite engine takes this
/// exact text — measured 4 Oct 2026: `/site2/wp-admin` 301s to `/site2/wp-admin/`,
/// `/site2/wp-content/…` and `/site2/wp-login.php` resolve to the root files, pretty URLs
/// reach `index.php`.
pub(crate) const SUBDIRECTORY_MULTISITE_RULES: &str = "RewriteRule ^/index\\.php$ - [L]\n\
             RewriteRule ^/([_0-9a-zA-Z-]+/)?wp-admin$ /$1wp-admin/ [R=301,L]\n\
             RewriteCond %{DOCUMENT_ROOT}%{REQUEST_URI} -f [OR]\n\
             RewriteCond %{DOCUMENT_ROOT}%{REQUEST_URI} -d\n\
             RewriteRule ^ - [L]\n\
             RewriteRule ^/([_0-9a-zA-Z-]+/)?(wp-(content|admin|includes).*) /$2 [L]\n\
             RewriteRule ^/([_0-9a-zA-Z-]+/)?(.*\\.php)$ /$2 [L]\n\
             RewriteRule . /index.php [L]\n";

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
/// The stock `<Directory "/">` stanza (AllowOverride None, Require all denied) comes
/// BEFORE the docroot's: without it httpd walks every `.htaccess` from the filesystem root
/// down to the docroot, and a stray one above the site — measured on the Dell (24 Oct 2026),
/// a `C:\.htaccess` with `RewriteEngine On` — answered every request with 500 ("RewriteEngine
/// not allowed here"). A `.htaccess` in the user's home or Sites folder would do the same on
/// any OS; only the docroot's tree may override.
///
/// Dot-segment guard: mod_alias isn't bundled (no `RedirectMatch`), so the
/// deny is a mod_rewrite `[R=404]` placed BEFORE the WP routing — subdirectory
/// multisite's `-f/-d` passthrough (`RewriteRule ^ - [L]`) would otherwise
/// L-stop first and serve an existing `.git/config`. Root `/.well-known/`
/// exempt (ACME/app probes); this also covers `.htaccess` itself, which the
/// generated conf never protected (no stock `<Files ".ht*">` block here).
#[allow(clippy::too_many_arguments)] // flat mirror of the site's serving inputs
pub fn generate_config(
    os: &str,
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
    let layout = Layout::for_os(os);
    let modules: String = MPM_MODULES
        .iter()
        .filter(|_| layout.loadable_mpm)
        .chain(MODULES.iter())
        .map(|(name, file)| format!("LoadModule {name} \"{}/{file}\"\n", layout.modules_dir))
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
         TypesConfig \"{basedir}/{mime}\"\n\
         DocumentRoot \"{docroot}\"\n\
         DirectoryIndex index.php index.html\n\
         <Directory \"/\">\n\
         \tAllowOverride None\n\
         \tRequire all denied\n\
         </Directory>\n\
         <Directory \"{docroot}\">\n\
         \tOptions FollowSymLinks\n\
         \tAllowOverride All\n\
         \tRequire all granted\n\
         </Directory>\n\
         <FilesMatch \"\\.php$\">\n\
         \tSetHandler \"proxy:fcgi://127.0.0.1:{fpm_port}{fcgi_suffix}\"\n\
         </FilesMatch>\n\
         {fcgi_fixup}\
         {set_env}\
         RewriteEngine On\n\
         RewriteCond %{{HTTP:X-Forwarded-Proto}} =https\n\
         RewriteRule .* - [E=HTTPS:on]\n\
         RewriteRule \"(^|/)\\.(?!well-known(/|$))\" - [R=404,L]\n\
         {routing}",
        basedir = path_for_conf(os, basedir),
        mime = layout.mime_types,
        fcgi_suffix = layout.fcgi_url_suffix,
        fcgi_fixup = layout.fcgi_fixup,
        run = path_for_conf(os, run_dir),
        logs = path_for_conf(os, log_dir),
        docroot = path_for_conf(os, docroot),
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

/// The conf's `ErrorLog` — the richer per-site log httpd itself writes. It
/// outlived every delete and rename until 18 Sep 2026 (clean-VM smoke: the
/// `.conf` and `-stdout.log` went, `-error.log` stayed) because the sweeps
/// were a hand-kept list beside a conf that names three files.
pub fn error_log_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform
        .paths()
        .log_dir()?
        .join(format!("apache-{domain}-error.log")))
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
        std::env::consts::OS,
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
        cfg_on("macos", mode, env)
    }

    fn cfg_on(os: &str, mode: RewriteMode, env: &[(String, String)]) -> String {
        generate_config(
            os,
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

    /// docs/archive/PLAN-windows-servers.md D4 — **one conf text, five per-OS holes.** The macOS
    /// render is the bottle's (loadable mpm_event + unixd, `lib/httpd/modules`, the
    /// `.bottle/etc` mime map); the Windows render is Apache Lounge's tree (no MPM lines —
    /// mpm_winnt is compiled in and `mod_unixd` does not exist there — `modules/`,
    /// `conf/mime.types`), and everything else is byte-identical: the same listener, the
    /// same fcgi hand-off to the pool port, the same `.htaccess` and HTTPS mapping.
    #[test]
    fn the_windows_render_differs_from_the_mac_one_only_in_the_layout_holes() {
        let mac = cfg_on("macos", RewriteMode::Single, &[]);
        let win = cfg_on("windows", RewriteMode::Single, &[]);
        assert!(mac.contains("LoadModule mpm_event_module \"lib/httpd/modules/mod_mpm_event.so\""));
        assert!(mac.contains("LoadModule unixd_module \"lib/httpd/modules/mod_unixd.so\""));
        assert!(mac.contains("TypesConfig \"/cache/httpd-2.4.68/.bottle/etc/httpd/mime.types\""));
        assert!(!win.contains("mpm_event") && !win.contains("unixd"), "mpm_winnt is compiled in:\n{win}");
        assert!(win.contains("LoadModule proxy_fcgi_module \"modules/mod_proxy_fcgi.so\""));
        assert!(win.contains("TypesConfig \"/cache/httpd-2.4.68/conf/mime.types\""));
        assert!(!win.contains("lib/httpd") && !win.contains(".bottle"), "a bottle path leaked into the Windows conf");
        // The fcgi hand-off: the bare URL on macOS; the trailing slash and the SCRIPT_FILENAME
        // rebuild on Windows (the drive-letter trap in `Layout`'s doc).
        assert!(mac.contains("SetHandler \"proxy:fcgi://127.0.0.1:9783\"\n") && !mac.contains("ProxyFCGISetEnvIf"));
        assert!(win.contains("SetHandler \"proxy:fcgi://127.0.0.1:9783/\"\n"), "{win}");
        assert!(win.contains("ProxyFCGISetEnvIf \"true\" SCRIPT_FILENAME \"%{DOCUMENT_ROOT}%{reqenv:SCRIPT_NAME}\"\n"), "{win}");
        // Everything that is not a layout hole is the same text.
        let strip = |s: &str| -> Vec<String> {
            s.lines()
                .filter(|l| !l.starts_with("LoadModule") && !l.starts_with("TypesConfig") && !l.contains("SetHandler") && !l.starts_with("ProxyFCGISetEnvIf"))
                .map(str::to_string)
                .collect()
        };
        assert_eq!(strip(&mac), strip(&win));
        // The module SET Windows loads is exactly what the mirror gate asserts exists in the zip.
        let win_modules: Vec<&str> = win.lines().filter(|l| l.starts_with("LoadModule")).collect();
        assert_eq!(win_modules.len(), MODULES.len());
        assert_eq!(Layout::for_os("windows").bin, "bin/httpd.exe");
        assert_eq!(Layout::for_os("linux").bin, "bin/httpd", "every POSIX host reads the bottle layout");
        // Windows paths are spelled with forward slashes everywhere the conf names one.
        let win_paths = generate_config(
            "windows",
            Path::new("C:\\Users\\Linkon Miyan\\AppData\\Local\\rexenv\\rexenv\\data\\bin\\httpd-2.4.69"),
            Path::new("C:\\Users\\Linkon Miyan\\Sites\\ap site\\public"),
            "ap.rex", 8300, 9783, RewriteMode::Single, &[],
            Path::new("C:\\data\\run"), Path::new("C:\\data\\logs"),
        );
        assert!(win_paths.contains("ServerRoot \"C:/Users/Linkon Miyan/AppData/Local/rexenv/rexenv/data/bin/httpd-2.4.69\""), "{win_paths}");
        assert!(win_paths.contains("DocumentRoot \"C:/Users/Linkon Miyan/Sites/ap site/public\""), "{win_paths}");
        assert!(win_paths.contains("PidFile \"C:/data/run/apache-ap.rex.pid\""), "{win_paths}");
        assert_eq!(path_for_conf("macos", Path::new("/a b/c")), "/a b/c");
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
        // The point of Apache: .htaccess honored — in the DOCROOT's tree only; above it
        // nothing is read (the Dell's `C:\.htaccess`, 24 Oct 2026).
        assert!(c.contains("AllowOverride All"));
        let root = c.find("<Directory \"/\">\n\tAllowOverride None\n\tRequire all denied\n</Directory>").expect("the root stanza");
        let doc = c.find("<Directory \"/Sites/ap site/public\">").expect("the docroot stanza");
        assert!(root < doc, "the root stanza must come first so the docroot's overrides it");
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
            binaries::bundle_manifest("httpd", binaries::pins().httpd, "macos", Arch::Arm64)
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
