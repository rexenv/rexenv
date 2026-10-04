//! core::openlitespeed — OpenLiteSpeed as a per-site OVERRIDE server (macOS + Linux).
//!
//! Same contract as `core::apache`: a loopback-only HTTP backend behind the edge Caddy (which
//! keeps :443, TLS and Host routing), with NO PHP of its own — `.php` goes to the site's SHARED
//! php-fpm pool over FastCGI (`extProcessor … type fcgi … autoStart 0`), so per-version PHP
//! settings and the mail catch apply exactly as on nginx sites. Why OpenLiteSpeed at all: the
//! LSCache module (what a WordPress developer's LiteSpeed host runs) plus `.htaccess`.
//!
//! The binary is rexenv's own build (`rexenv/runtimes`, six patches; `docs/PLAN-openlitespeed.md`).
//! It is shared by every OLS site from the binary cache; each site gets its OWN server root
//! under app-data (`LSWS_HOME`), because OpenLiteSpeed reads `<root>/conf/httpd_config.conf`
//! and writes `cachedata/`, `tmp/`, `autoupdate/` beside it. Its runtime files (pid, swap,
//! status) go to `<root>/run` through `LSWS_TMP_DIR` (patch 0003) — never the machine-wide
//! `/tmp/lshttpd`, which would make a second site refuse to start ("already running").
//!
//! What the generated config must say, and why (each line measured, PLAN §3.1 / §4):
//! - `disableWebAdmin 1` — no admin console, no listener of its own besides the site's.
//! - `noRemoteFetch 1` (patch 0004) — no release check to openlitespeed.org, no quic.cloud.
//! - `user <uid>` — refused at start without a resolvable user, even as non-root.
//! - `fileAccessControl` masks `000` — the defaults deny files a developer's editor left 0644.
//! - `tuning { shmDefaultDir … }` — read from `tuning`, not server level.
//! - upstream's WHOLE default `module cache` block — a short block parses and never caches.
//! - every value unquoted is fine with spaces (app-data paths contain them; measured).

use crate::core::services::RewriteMode;
use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::Child;

/// Base loopback port for per-site OpenLiteSpeed backends. Disjoint from FrankenPHP's
/// 8200–8299 and Apache's 8300–8399, so a site switching server kinds never collides
/// with its own previous backend.
pub const OPENLITESPEED_BASE_PORT: u16 = 8400;

/// Deterministic port in `8400..8500` (the FNV-1a scheme the other override kinds use) — only
/// the pre-allocation fallback; every site records its real port at create/switch (B20 §4).
pub fn site_port(domain: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in domain.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    OPENLITESPEED_BASE_PORT + (h % 100) as u16
}

/// The server binary inside the cached tree (flat: see the `binaries::manifest` arm).
pub fn server_bin(basedir: &Path) -> PathBuf {
    basedir.join("openlitespeed")
}

/// The MIME map shipped beside the binary.
pub fn mime_path(basedir: &Path) -> PathBuf {
    basedir.join("mime.properties")
}

/// The site's own server root (`LSWS_HOME`).
pub fn server_root(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("openlitespeed").join(domain))
}

/// The generated config — at the one path OpenLiteSpeed reads under its server root.
pub fn config_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(server_root(platform, domain)?.join("conf").join("httpd_config.conf"))
}

/// stdout+stderr of the server process.
pub fn log_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform.paths().log_dir()?.join(format!("openlitespeed-{domain}-stdout.log")))
}

/// The server's own error log (named in the config).
pub fn error_log_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform.paths().log_dir()?.join(format!("openlitespeed-{domain}-error.log")))
}

/// The server's access log (named in the config — OpenLiteSpeed logs an ERROR without one).
pub fn access_log_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform.paths().log_dir()?.join(format!("openlitespeed-{domain}-access.log")))
}

/// Every file a site's OpenLiteSpeed leaves outside its server root — the delete/rename sweeps
/// remove these together with the root, from ONE list (the Apache error log outlived every
/// delete for a month because its sweep was a hand-kept list beside the config).
pub fn log_paths(platform: &dyn Platform, domain: &str) -> Result<Vec<PathBuf>> {
    Ok(vec![
        log_path(platform, domain)?,
        error_log_path(platform, domain)?,
        access_log_path(platform, domain)?,
    ])
}

/// Remove a site's whole server root (config, run dir, LSCache storage) — best-effort.
///
/// A recursive delete of a DERIVED path, so the path is checked before it is used: its last
/// component must be exactly this domain, under `<app-data>/openlitespeed/`. An empty or odd
/// domain would otherwise widen the delete to every site's root (the 24 Jul 2026 example that
/// removed a whole Sites folder through `docroot.parent()` is the reason this is spelled out).
/// (ledger #776)
pub fn remove_server_root(platform: &dyn Platform, domain: &str) {
    let Ok(root) = server_root(platform, domain) else { return };
    if is_this_sites_root(&root, domain) {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// The check [`remove_server_root`] makes before a recursive delete — pure, so it is tested.
fn is_this_sites_root(root: &Path, domain: &str) -> bool {
    if domain.is_empty() || domain.contains('/') || domain.contains('\\') || domain.starts_with('.') {
        return false;
    }
    let parent_ok = root.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "openlitespeed");
    parent_ok && root.file_name().is_some_and(|n| n == domain)
}

/// The quote character an env value can be wrapped in, or `None` when it holds both.
///
/// OpenLiteSpeed reads `E='NAME:value'` up to the next matching quote with no escape for it
/// (`RewriteRule::parseOneFlag`), so a value carrying `'` AND `"` cannot be written at all.
fn env_quote(value: &str) -> Option<char> {
    match (value.contains('\''), value.contains('"')) {
        (false, _) => Some('\''),
        (true, false) => Some('"'),
        (true, true) => None,
    }
}

/// Refuse what OpenLiteSpeed cannot carry — on top of `site_env::validate`, which already
/// rejected what NO server can (`$`, braces, control characters). Called where a value is
/// SAVED for an OpenLiteSpeed site and where a site is SWITCHED to OpenLiteSpeed, so the
/// generator never meets one.
pub fn check_env(pairs: &[(String, String)]) -> Result<()> {
    for (name, value) in pairs {
        if env_quote(value).is_none() {
            return Err(crate::error::Error::Other(format!(
                "env var '{name}': OpenLiteSpeed cannot pass a value that contains both ' and \" \
                 (its rewrite flags have no escape for the quote) — drop one of them, or use \
                 another web server for this site"
            )));
        }
    }
    Ok(())
}

/// `RewriteRule .* - [E='NAME:value']` — how a per-site variable reaches PHP: OpenLiteSpeed
/// passes rewrite-set env to a FastCGI app as request params (measured: `getenv()`/`$_SERVER`
/// in the pool see it, per REQUEST, the shared pool untouched — nginx's `fastcgi_param` class).
/// Inside the quotes `\` and `%` are the substitution engine's (`%1` is a back-reference, a
/// bare `%` is a parse error that drops the WHOLE rules block), so both are escaped; the
/// quote is the one the value does not contain. `None` for a value [`check_env`] refuses.
/// (ledger #779)
fn env_rule(name: &str, value: &str) -> Option<String> {
    let q = env_quote(value)?;
    let escaped = value.replace('\\', "\\\\").replace('%', "\\%");
    Some(format!("RewriteRule .* - [E={q}{name}:{escaped}{q}]\n"))
}

/// The routing for each multisite mode — applied ONLY when the docroot has no `.htaccess`.
///
/// OpenLiteSpeed runs the vhost's rules BEFORE the directory's `.htaccess`, so a front
/// controller here rewrote every missing path to `/index.php` first and a plugin's own
/// `.htaccess` redirect (`RewriteRule ^old$ /new [R=301]`) never saw its path (measured,
/// 4 Oct 2026 — Apache has no such problem: its `FallbackResource` is a handler fallback,
/// not a rewrite). So a site with a root `.htaccess` routes itself — WordPress, Laravel and
/// every LiteSpeed host work that way — and these rules are the fallback for a site
/// without one. Single and subdomain: a path that is neither file nor directory reaches
/// `/index.php` with its original `REQUEST_URI`. Subdirectory: Apache's exact rules — the
/// vhost-level engine takes the same server-context text. (ledger #778)
fn routing(mode: RewriteMode) -> &'static str {
    match mode {
        RewriteMode::Single | RewriteMode::SubdomainMultisite => {
            "RewriteCond %{DOCUMENT_ROOT}%{REQUEST_URI} !-f\n\
             RewriteCond %{DOCUMENT_ROOT}%{REQUEST_URI} !-d\n\
             RewriteRule . /index.php [L]\n"
        }
        RewriteMode::SubdirectoryMultisite => super::apache::SUBDIRECTORY_MULTISITE_RULES,
    }
}

/// Everything [`generate_config`] needs, named — the list is long and positional arguments of
/// the same type are how a docroot ends up where a log dir belongs.
pub struct ConfigInput<'a> {
    pub basedir: &'a Path,
    pub server_root: &'a Path,
    pub docroot: &'a Path,
    pub log_dir: &'a Path,
    pub domain: &'a str,
    pub port: u16,
    pub fpm_port: u16,
    pub mode: RewriteMode,
    pub env: &'a [(String, String)],
    /// `ProcessSupervisor::service_account` — the uid to name in `user`.
    pub account: &'a str,
    /// [`htaccess_fingerprint`] of the docroot's root `.htaccess` as the server will load it,
    /// `None` when there is none.
    pub htaccess: Option<&'a str>,
}

/// The docroot's root `.htaccess` as this server will see it: size + mtime, `None` if absent.
///
/// OpenLiteSpeed reads a directory's `.htaccess` ONCE, on the directory's first request, and
/// never again — neither a file created later nor an edit to a loaded one is seen without a
/// restart (measured on the macOS 15 VM, 4 Oct 2026: two minutes of polling, 404 then 301
/// only after `rex site restart`). WordPress writes its rules there on the first permalink
/// save; the LiteSpeed Cache plugin rewrites them on every settings save. So the fingerprint
/// is rendered INTO the config: an edit makes the desired config differ from the one the
/// running server loaded, and the ServiceManager's config diff — at reconcile and on every
/// watchdog tick — restarts it, the way an OpenLiteSpeed admin issues a graceful restart.
/// The root file only: subdirectory `.htaccess` files are read on first access too, and an
/// edit to one needs `rex site restart` (stated in docs/ARCHITECTURE.md). (ledger #783)
pub fn htaccess_fingerprint(docroot: &Path) -> Option<String> {
    htaccess_fingerprint_now(docroot)
}

/// The `.htaccess` line of a rendered config — so a restart can say whether THAT is what
/// changed, rather than claiming it for every difference.
pub fn htaccess_line(config: &str) -> Option<&str> {
    config.lines().find(|l| l.starts_with("# root .htaccess at start:"))
}

fn htaccess_fingerprint_now(docroot: &Path) -> Option<String> {
    let meta = std::fs::metadata(docroot.join(".htaccess")).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some(format!("{}-{mtime}", meta.len()))
}

/// Render the site's `httpd_config.conf`: one loopback listener, no admin, no remote fetch,
/// `.php` to the site's pool, the vhost INLINE (one file, so the reconcile's config diff sees
/// every serving input), `.htaccess` honoured, the edge's `X-Forwarded-Proto: https` mapped to
/// `HTTPS=on` (WordPress `is_ssl()`), dotfiles 404 before any routing (root `/.well-known/`
/// exempt), and the LSCache module on with storage under the server root. (ledger #780)
pub fn generate_config(c: &ConfigInput) -> String {
    let root = c.server_root.display();
    let logs = c.log_dir.display();
    let domain = c.domain;
    let env: String = c.env.iter().filter_map(|(n, v)| env_rule(n, v)).collect();
    // With a root .htaccess the site routes itself and the vhost steps aside; without one the
    // vhost's own rules route it. Decided HERE, from what the server will load at start — a
    // live `-f` test would step aside the moment WordPress writes the file, while the running
    // server still ignores it (OpenLiteSpeed never re-reads it), and every pretty URL 404s
    // until a restart. That is what the first running-app smoke on the macOS 15 VM found.
    let (fingerprint, routing) = match c.htaccess {
        Some(fp) => (format!("# root .htaccess at start: {fp}\n"), "RewriteRule ^ - [L]\n"),
        None => ("# root .htaccess at start: none\n".to_string(), routing(c.mode)),
    };
    format!(
        "# Generated by rexenv — rewritten on every start; edits are lost.\n\
         {fingerprint}\
         serverName                 {domain}\n\
         user                       {account}\n\
         autoRestart                0\n\
         httpdWorkers               1\n\
         disableWebAdmin            1\n\
         noRemoteFetch              1\n\
         showVersionNumber          0\n\
         swappingDir                {root}/swap\n\
         mime                       {mime}\n\
         indexFiles                 index.php, index.html\n\
         errorlog {logs}/openlitespeed-{domain}-error.log {{\n\
         \x20 logLevel                 WARN\n\
         \x20 rollingSize              10M\n\
         }}\n\
         accessLog {logs}/openlitespeed-{domain}-access.log {{\n\
         \x20 rollingSize              10M\n\
         \x20 keepDays                 7\n\
         }}\n\
         fileAccessControl {{\n\
         \x20 followSymbolLink         1\n\
         \x20 checkSymbolLink          0\n\
         \x20 requiredPermissionMask   000\n\
         \x20 restrictedPermissionMask 000\n\
         }}\n\
         tuning {{\n\
         \x20 shmDefaultDir            {root}/shm\n\
         \x20 quicEnable               0\n\
         \x20 quicShmDir               {root}/shm\n\
         \x20 maxReqBodySize           2047M\n\
         \x20 maxDynRespSize           2047M\n\
         }}\n\
         extProcessor pool {{\n\
         \x20 type                     fcgi\n\
         \x20 address                  127.0.0.1:{fpm_port}\n\
         \x20 maxConns                 10\n\
         \x20 initTimeout              60\n\
         \x20 retryTimeout             0\n\
         \x20 persistConn              1\n\
         \x20 respBuffer               0\n\
         \x20 autoStart                0\n\
         }}\n\
         scriptHandler {{\n\
         \x20 add fcgi:pool            php\n\
         }}\n\
         virtualHost site {{\n\
         \x20 vhRoot                   {docroot}/\n\
         \x20 allowSymbolLink          1\n\
         \x20 enableScript             1\n\
         \x20 restrained               0\n\
         \x20 docRoot                  $VH_ROOT/\n\
         \x20 index {{\n\
         \x20   useServer              0\n\
         \x20   indexFiles             index.php, index.html\n\
         \x20   autoIndex              0\n\
         \x20 }}\n\
         \x20 rewrite {{\n\
         \x20   enable                 1\n\
         \x20   autoLoadHtaccess       1\n\
         \x20   rules                  <<<END_rules\n\
         RewriteCond %{{HTTP:X-Forwarded-Proto}} =https\n\
         RewriteRule .* - [E=HTTPS:on]\n\
         {env}\
         RewriteRule (^|/)\\.(?!well-known(/|$)) - [R=404,L]\n\
         {routing}\
         \x20   END_rules\n\
         \x20 }}\n\
         }}\n\
         listener Default {{\n\
         \x20 address                  127.0.0.1:{port}\n\
         \x20 secure                   0\n\
         \x20 map                      site *\n\
         }}\n\
         module cache {{\n\
         \x20 ls_enabled               1\n\
         \x20 storagepath              {root}/lscache\n\
         \x20 checkPrivateCache        1\n\
         \x20 checkPublicCache         1\n\
         \x20 maxCacheObjSize          10000000\n\
         \x20 maxStaleAge              200\n\
         \x20 qsCache                  1\n\
         \x20 reqCookieCache           1\n\
         \x20 respCookieCache          1\n\
         \x20 ignoreReqCacheCtrl       1\n\
         \x20 ignoreRespCacheCtrl      0\n\
         \x20 enableCache              0\n\
         \x20 expireInSeconds          3600\n\
         \x20 enablePrivateCache       0\n\
         \x20 privateExpireInSeconds   3600\n\
         }}\n",
        account = c.account,
        mime = mime_path(c.basedir).display(),
        docroot = c.docroot.display(),
        fpm_port = c.fpm_port,
        port = c.port,
    )
}

/// The server root's fixed subdirectories — made before every spawn. `share/autoindex` must
/// exist even with auto-index off (an ERROR and a failing config test without it).
fn ensure_tree(root: &Path) -> Result<()> {
    for d in ["conf", "run", "shm", "swap", "lscache", "share/autoindex"] {
        std::fs::create_dir_all(root.join(d))?;
    }
    Ok(())
}

/// Write the site's config (and its tree); returns the config path.
pub fn write_config(platform: &dyn Platform, domain: &str, content: &str) -> Result<PathBuf> {
    let root = server_root(platform, domain)?;
    ensure_tree(&root)?;
    std::fs::create_dir_all(platform.paths().log_dir()?)?;
    let conf = config_path(platform, domain)?;
    std::fs::write(&conf, content)?;
    Ok(conf)
}

/// Start the server in the foreground (`-n`: no daemonising, crash guard kept). `LSWS_HOME`
/// names the site's server root, `LSWS_TMP_DIR` its runtime dir.
///
/// `-n`, not `-d`: OpenLiteSpeed's event loop stops itself the moment its PARENT changes
/// (`EventDispatcher`: `getppid() != s_ppid`). Under `-d` that loop runs in the process rexenv
/// spawned, so quitting the app — even SIGKILLing the parent — stopped every OpenLiteSpeed
/// site about a second later (measured on the macOS 15 VM, 4 Oct 2026), breaking "services
/// outlive the app". Under `-n` the spawned process is the crash-guard MAIN, which forks the
/// worker that runs the loop; the worker's parent is that main, which outlives the app, and
/// SIGTERM to the main stops the worker and `lscgid` with it. (ledger #784)
///
/// The app-data dir is passed TWICE after `-n`, and both are load-bearing: it is the
/// ownership marker `owned_master` looks for on the command line, and OpenLiteSpeed
/// overwrites `argv[0]` with "openlitespeed (lshttpd - main)" — the NUL it writes splits the
/// old path in two, which macOS `ps` counts as an extra argument and then drops the LAST one
/// (argc is unchanged). Linux `/proc/<pid>/cmdline` shows both. The worker's title loses the
/// marker entirely, so ownership resolves to the main — the pid rexenv holds. Measured 4 Oct
/// 2026. `getopt` stops at the first operand, so the server never reads them. (ledger #781;
/// the runtime-files half — `LSWS_TMP_DIR`, nothing under `/tmp/lshttpd` — is #782)
pub fn start(platform: &dyn Platform, basedir: &Path, domain: &str, env: &[(String, String)]) -> Result<Child> {
    let root = server_root(platform, domain)?;
    let marker = platform.paths().app_data_dir()?.display().to_string();
    let args = vec!["-n".to_string(), marker.clone(), marker];
    let mut spawn_env: Vec<(String, String)> = env.to_vec();
    spawn_env.push(("LSWS_HOME".into(), format!("{}/", root.display())));
    spawn_env.push(("LSWS_TMP_DIR".into(), root.join("run").display().to_string()));
    let log = log_path(platform, domain)?;
    platform.supervisor().spawn_logged_env(&server_bin(basedir), &args, &log, &spawn_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: RewriteMode, env: &[(String, String)]) -> String {
        generate_config(&ConfigInput {
            basedir: Path::new("/data bin/openlitespeed-1.9.3"),
            server_root: Path::new("/data/openlitespeed/ol.rex"),
            docroot: Path::new("/Sites/ol site/public"),
            log_dir: Path::new("/data/logs"),
            domain: "ol.rex",
            port: 8400,
            fpm_port: 9783,
            mode,
            env,
            account: "501",
            htaccess: None,
        })
    }

    fn cfg_with_htaccess(mode: RewriteMode) -> String {
        generate_config(&ConfigInput {
            basedir: Path::new("/b"),
            server_root: Path::new("/r"),
            docroot: Path::new("/d"),
            log_dir: Path::new("/l"),
            domain: "ol.rex",
            port: 8400,
            fpm_port: 9783,
            mode,
            env: &[],
            account: "501",
            htaccess: Some("523-1791100000000000000"),
        })
    }

    #[test]
    fn ports_stay_in_their_own_range() {
        for domain in ["a.rex", "verylongdomainname.rex", "x.test"] {
            let p = site_port(domain);
            assert!((8400..8500).contains(&p), "{domain} → {p}");
            assert_eq!(
                p - OPENLITESPEED_BASE_PORT,
                crate::core::apache::site_port(domain) - crate::core::apache::APACHE_BASE_PORT
            );
        }
    }

    #[test]
    fn config_is_a_loopback_backend_with_no_admin_and_no_phone_home() {
        let c = cfg(RewriteMode::Single, &[]);
        assert!(c.contains("address                  127.0.0.1:8400"));
        assert_eq!(c.matches("listener ").count(), 1, "one listener — the site's");
        assert!(c.contains("disableWebAdmin            1"));
        assert!(c.contains("noRemoteFetch              1"));
        assert!(c.contains("secure                   0"));
        assert!(!c.contains("443"));
        assert!(c.contains("address                  127.0.0.1:9783"), "PHP goes to the site's pool");
        assert!(c.contains("autoStart                0"), "OLS never spawns PHP of its own");
        assert!(c.contains("user                       501"));
        assert!(c.contains("mime                       /data bin/openlitespeed-1.9.3/mime.properties"));
        assert!(c.contains("vhRoot                   /Sites/ol site/public/"));
        assert!(c.contains("storagepath              /data/openlitespeed/ol.rex/lscache"));
        assert!(c.contains("maxCacheObjSize          10000000"), "the cache block must be whole");
        assert!(c.contains("autoLoadHtaccess       1"));
        // shmDefaultDir is a `tuning` key — at server level OLS ignores it.
        let tuning = c.find("tuning {").unwrap();
        assert!(c[tuning..].find("shmDefaultDir").unwrap() < c[tuning..].find('}').unwrap());
    }

    #[test]
    fn https_map_and_dotfile_deny_come_before_routing() {
        for mode in [RewriteMode::Single, RewriteMode::SubdomainMultisite, RewriteMode::SubdirectoryMultisite] {
            let c = cfg(mode, &[("API".into(), "x".into())]);
            let https = c.find("[E=HTTPS:on]").unwrap();
            let env = c.find("[E='API:x']").unwrap();
            let deny = c.find("[R=404,L]").unwrap();
            let route = c.find("/index.php [L]").unwrap();
            assert!(https < env && env < deny && deny < route, "{mode:?}");
            assert!(c.contains("# root .htaccess at start: none"));
            // With a root .htaccess at start: the dotfile deny still comes first (a site's own
            // rules cannot re-expose `.git`), then the vhost steps aside — no fallback at all,
            // so the site's rules see the raw path — and the fingerprint is in the config, so
            // an edit to the file is a config change (ledger #783).
            let own = cfg_with_htaccess(mode);
            let deny = own.find("[R=404,L]").unwrap();
            let aside = own.find("RewriteRule ^ - [L]").unwrap();
            assert!(deny < aside, "{mode:?}");
            assert!(!own.contains("/index.php [L]"), "{mode:?}: no vhost front controller beside a site's .htaccess");
            assert!(own.contains("# root .htaccess at start: 523-1791100000000000000"));
            assert!(c.contains("RewriteCond %{HTTP:X-Forwarded-Proto} =https"));
        }
        let sub = cfg(RewriteMode::SubdirectoryMultisite, &[]);
        assert_ne!(cfg_with_htaccess(RewriteMode::Single), cfg(RewriteMode::Single, &[]));
        assert!(sub.contains(crate::core::apache::SUBDIRECTORY_MULTISITE_RULES));
        assert!(!cfg(RewriteMode::Single, &[]).contains("wp-admin$"));
    }

    #[test]
    fn the_fingerprint_moves_with_the_file() {
        let dir = std::env::temp_dir().join(format!("rexenv-ols-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(htaccess_fingerprint(&dir), None);
        std::fs::write(dir.join(".htaccess"), "RewriteEngine On\n").unwrap();
        let a = htaccess_fingerprint(&dir).expect("present");
        std::fs::write(dir.join(".htaccess"), "RewriteEngine On\nRewriteRule ^x$ /y [R=301,L]\n").unwrap();
        let b = htaccess_fingerprint(&dir).expect("present");
        assert_ne!(a, b, "an edit must change the fingerprint");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_sites_own_root_is_ever_deleted() {
        let ok = Path::new("/data/openlitespeed/a.rex");
        assert!(is_this_sites_root(ok, "a.rex"));
        // Every shape that would widen the delete.
        assert!(!is_this_sites_root(Path::new("/data/openlitespeed"), ""));
        assert!(!is_this_sites_root(Path::new("/data/openlitespeed/.."), ".."));
        assert!(!is_this_sites_root(Path::new("/data/openlitespeed/a/b"), "a/b"));
        assert!(!is_this_sites_root(Path::new("/data/other/a.rex"), "a.rex"));
        assert!(!is_this_sites_root(ok, "b.rex"));
    }

    #[test]
    fn env_values_are_quoted_and_escaped_the_way_ols_parses_them() {
        assert_eq!(env_rule("A", "say \"hi\", ok] [x]").unwrap(), "RewriteRule .* - [E='A:say \"hi\", ok] [x]']\n");
        assert_eq!(env_rule("B", "it's").unwrap(), "RewriteRule .* - [E=\"B:it's\"]\n");
        assert_eq!(env_rule("C", "C:\\p 50%1").unwrap(), "RewriteRule .* - [E='C:C:\\\\p 50\\%1']\n");
        assert_eq!(env_rule("D", "").unwrap(), "RewriteRule .* - [E='D:']\n");
        assert!(env_rule("E", "'\"").is_none());
        assert!(check_env(&[("E".into(), "a'b\"c".into())]).is_err());
        assert!(check_env(&[("E".into(), "a'b".into()), ("F".into(), "x\"y".into())]).is_ok());
        assert!(!cfg(RewriteMode::Single, &[]).contains("[E='"), "no env → no env rules");
    }
}
