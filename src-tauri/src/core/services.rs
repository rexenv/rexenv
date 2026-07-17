//! core::services — service supervision (Phase 1: PHP-FPM pools).
//!
//! One PHP-FPM master per PHP version (not per site) — all sites on a version
//! share one pool, listening on a loopback TCP port for FastCGI. Nginx (6.2)
//! connects to this port. We run php-fpm as the current user (no root), so the
//! pool needs no `user`/`group` directive.

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

/// Loopback FastCGI port for the PHP 8.3 pool (one pool per PHP version).
pub const PHP_FPM_PORT: u16 = 9783;

/// Render a php-fpm config: one foreground `[global]` master + one `[www]` pool
/// on `127.0.0.1:<port>`. The pool generator is per-version so more versions can
/// be added later, each on its own port. When `sendmail_path` is `Some`, the pool
/// pins `php_admin_value[sendmail_path]` so every site's PHP `mail()` is routed
/// through that shim (Mailpit, §2.2) — sites can't override it (`_admin_`).
/// `settings` are the user's whitelisted, pre-validated per-version ini values
/// (`core::php::SETTINGS`), written as overridable `php_value[key]` lines so
/// WordPress can still `ini_set()` at runtime.
pub fn generate_fpm_config(
    port: u16,
    pid_file: &Path,
    log_file: &Path,
    sendmail_path: Option<&str>,
    settings: &[(String, String)],
) -> String {
    // Pool sizing guards the "every site hangs while the UI shows running" spiral:
    // ALL default sites share this one pool, so N wall-clock-stuck workers (heavy
    // plugin imports, loopback self-requests) starve every site at once — the port
    // still accepts, so no probe sees it. `request_terminate_timeout` (wall clock —
    // PHP's own max_execution_time only counts CPU time on unix) recycles a stuck
    // worker after 5min; `pm.max_requests` recycles leaky workers.
    //
    // Wrap the value in DOUBLE quotes: PHP's ini parser strips the outer quotes
    // but preserves the inner single-quoted binary path verbatim, so the shim's
    // space-containing path survives to `sh -c`. (Bare single quotes get eaten by
    // the ini parser, leaving an unquoted path that `sh` splits on the space.)
    let sendmail = sendmail_path
        .map(|p| format!("php_admin_value[sendmail_path] = \"{p}\"\n"))
        .unwrap_or_default();
    let values: String = settings
        .iter()
        .map(|(k, v)| format!("php_value[{k}] = {v}\n"))
        .collect();
    // The wall-clock recycle must never undercut the user's max_execution_time,
    // or a legitimately long request (big import) dies at 300s and the setting
    // silently lies. Floor stays 300s: a max_execution_time of 0 (unlimited CPU
    // time) still gets the 300s wall-clock guard — surfaced as a cap in the UI.
    let terminate = settings
        .iter()
        .find(|(k, _)| k == "max_execution_time")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .map_or(300, |secs| secs.max(300));
    format!(
        "[global]\n\
         pid = {pid}\n\
         error_log = {log}\n\
         daemonize = no\n\
         \n\
         [www]\n\
         listen = 127.0.0.1:{port}\n\
         pm = dynamic\n\
         pm.max_children = 10\n\
         pm.start_servers = 2\n\
         pm.min_spare_servers = 1\n\
         pm.max_spare_servers = 3\n\
         pm.max_requests = 500\n\
         request_terminate_timeout = {terminate}s\n\
         catch_workers_output = yes\n\
         {sendmail}\
         {values}",
        pid = pid_file.display(),
        log = log_file.display(),
    )
}

/// Write the php-fpm config for `version` (on `port`) under the config dir,
/// creating the run/log dirs. `sendmail_path` (when set) routes the pool's PHP
/// `mail()` to Mailpit (§2.2). Returns the config path.
pub fn write_fpm_config(
    platform: &dyn Platform,
    version: &str,
    port: u16,
    sendmail_path: Option<&str>,
    settings: &[(String, String)],
) -> Result<PathBuf> {
    write_fpm_config_named(platform, version, port, sendmail_path, settings, "conf")
}

/// Like [`write_fpm_config`] but to a `.conf.candidate` file the running pool
/// never reads — the `php-fpm -t` gate for a settings change validates THIS file
/// first, so a value PHP rejects never reaches the real config or a restart.
pub fn write_fpm_config_candidate(
    platform: &dyn Platform,
    version: &str,
    port: u16,
    settings: &[(String, String)],
) -> Result<PathBuf> {
    // No sendmail line: the shim path lives behind the services lock and the
    // fixed-format line can't be invalidated by user settings — the gate is
    // about the user's values.
    write_fpm_config_named(platform, version, port, None, settings, "conf.candidate")
}

fn write_fpm_config_named(
    platform: &dyn Platform,
    version: &str,
    port: u16,
    sendmail_path: Option<&str>,
    settings: &[(String, String)],
    ext: &str,
) -> Result<PathBuf> {
    let config_dir = platform.paths().config_dir()?;
    let log_dir = platform.paths().log_dir()?;
    let run_dir = platform.paths().app_data_dir()?.join("run");
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&log_dir)?;
    std::fs::create_dir_all(&run_dir)?;

    let conf = config_dir.join(format!("php-fpm-{version}.{ext}"));
    let pid = run_dir.join(format!("php-fpm-{version}.pid"));
    let log = log_dir.join(format!("php-fpm-{version}.log"));
    std::fs::write(&conf, generate_fpm_config(port, &pid, &log, sendmail_path, settings))?;
    Ok(conf)
}

/// Start the php-fpm master in the foreground (`-F`) with the given config, via
/// `ProcessSupervisor`. Returns the child handle (track its pid to `stop`).
pub fn start_fpm(platform: &dyn Platform, php_fpm_bin: &Path, conf: &Path) -> Result<Child> {
    let args = vec![
        "-F".to_string(),
        "-y".to_string(),
        conf.display().to_string(),
    ];
    let log = platform.paths().log_dir()?.join("php-fpm-stdout.log");
    platform.supervisor().spawn_logged(php_fpm_bin, &args, &log)
}

/// The `-d` ini pairs that turn a pool into a DEBUG pool: load the minor's
/// pinned `xdebug.so` and put it in step-debugging mode. Command-line `-d`
/// applies to every worker (verified live: `php-fpm -d zend_extension=… -m`
/// lists the module). `xdebug.mode=debug,develop` per the §8.2 spec; client
/// host/port stay at Xdebug's defaults (127.0.0.1:9003 — what IDEs listen on)
/// and activation stays on-trigger (XDEBUG_SESSION cookie/param), so an idle
/// debug pool doesn't stall requests hunting for an absent IDE.
fn xdebug_args(xdebug_so: &Path) -> Vec<String> {
    vec![
        "-d".to_string(),
        format!("zend_extension={}", xdebug_so.display()),
        "-d".to_string(),
        "xdebug.mode=debug,develop".to_string(),
    ]
}

/// Start a DEBUG php-fpm master: [`start_fpm`] plus the Xdebug `-d` overrides.
pub fn start_fpm_xdebug(
    platform: &dyn Platform,
    php_fpm_bin: &Path,
    conf: &Path,
    xdebug_so: &Path,
) -> Result<Child> {
    let mut args = vec![
        "-F".to_string(),
        "-y".to_string(),
        conf.display().to_string(),
    ];
    args.extend(xdebug_args(xdebug_so));
    let log = platform.paths().log_dir()?.join("php-fpm-stdout.log");
    platform.supervisor().spawn_logged(php_fpm_bin, &args, &log)
}

/// GATE: prove this php-fpm binary actually loads `xdebug_so` before any debug
/// pool spawns. PHP treats a failed `zend_extension` as a WARNING and starts
/// anyway (verified: a symbol-mismatched .so prints "Failed loading …" and the
/// process continues) — so without this probe, a bad artifact would serve the
/// toggled site with Xdebug silently missing. Runs `php-fpm -d zend_extension
/// -m` logged to a probe file and requires the module list to contain
/// `xdebug`; the probe log is quoted in the error so the dlopen failure is
/// visible verbatim.
pub fn assert_fpm_loads_xdebug(
    platform: &dyn Platform,
    php_fpm_bin: &Path,
    xdebug_so: &Path,
) -> Result<()> {
    let log_dir = platform.paths().log_dir()?;
    std::fs::create_dir_all(&log_dir)?;
    let probe_log = log_dir.join("xdebug-probe.log");
    let _ = std::fs::remove_file(&probe_log);
    let mut args = xdebug_args(xdebug_so);
    args.push("-m".to_string());
    let mut child = platform
        .supervisor()
        .spawn_logged(php_fpm_bin, &args, &probe_log)?;
    let status = child.wait()?;
    let out = std::fs::read_to_string(&probe_log).unwrap_or_default();
    let loaded = status.success() && out.lines().any(|l| l.trim().eq_ignore_ascii_case("xdebug"));
    if loaded {
        Ok(())
    } else {
        let detail: String = out
            .lines()
            .filter(|l| l.contains("Failed loading") || l.contains("Warning"))
            .collect::<Vec<_>>()
            .join("; ");
        Err(Error::Other(format!(
            "this PHP build did not load xdebug.so ({}) — Xdebug stays off rather than \
             silently missing{}{}",
            xdebug_so.display(),
            if detail.is_empty() { "" } else { ": " },
            detail
        )))
    }
}

/// Validate a php-fpm config without starting it (`php-fpm -t -y <conf>`).
pub fn test_fpm_config(platform: &dyn Platform, php_fpm_bin: &Path, conf: &Path) -> Result<()> {
    let args = vec![
        "-t".to_string(),
        "-y".to_string(),
        conf.display().to_string(),
    ];
    let mut child = platform.supervisor().spawn(php_fpm_bin, &args)?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "php-fpm config test failed (exit {:?})",
            status.code()
        )))
    }
}

/// Stop a running php-fpm by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// Service status the services/UI layer reads: a pool is `running` iff its
/// loopback FastCGI port accepts a connection.
pub fn fpm_running(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

// ── Shared Nginx ────────────────────────────────────────────────────────────
//
// One shared nginx process listens on an internal loopback HTTP port (plain
// HTTP — TLS is Caddy's job) and dispatches to a per-site `server` block by
// `server_name`. Each block roots at the site's docroot and proxies `.php` to
// the shared php-fpm pool. Caddy proxies all `*.test` to this port.

/// Internal loopback HTTP port the shared nginx listens on (Caddy's upstream).
pub const NGINX_HTTP_PORT: u16 = 18088;

/// WordPress rewrite mode for a site. Phase 1 exercises `Single`; the multisite
/// slots exist so Phase 3 needs no refactor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteMode {
    Single,
    SubdomainMultisite,
    SubdirectoryMultisite,
}

/// One site's nginx server block.
#[derive(Debug, Clone)]
pub struct NginxSite {
    pub domain: String,
    pub docroot: PathBuf,
    pub php_fpm_port: u16,
    pub rewrite: RewriteMode,
    /// Per-server `client_max_body_size` in BYTES, mirroring the site's PHP
    /// version's max(upload_max_filesize, post_max_size) — set together so nginx
    /// never 413s an upload PHP would accept (the "raised upload_max_filesize
    /// but uploads still fail" lie). `None` ⇒ the http-level default applies.
    pub body_limit: Option<u64>,
    /// Per-site user env vars (§1.6), VALIDATED by `site_env::validate` —
    /// emitted as `fastcgi_param` lines so they ride the request (shared pools
    /// untouched; getenv() + $_SERVER, not $_ENV).
    pub env: Vec<(String, String)>,
}

/// Full shared-nginx configuration.
#[derive(Debug, Clone)]
pub struct NginxConfig {
    pub http_port: u16,
    pub pid: PathBuf,
    pub error_log: PathBuf,
    pub access_log: PathBuf,
    /// Directory for nginx's writable temp paths (created on start).
    pub temp_root: PathBuf,
    pub sites: Vec<NginxSite>,
}

/// The `location /` (and any rewrite) block for a site's mode. Single uses plain
/// try_files; subdirectory multisite adds WordPress's network rewrite rules.
fn rewrite_block(mode: RewriteMode) -> String {
    match mode {
        // Subdomain multisite: each subdomain is its own WP host, so the routing
        // is the same as single — the difference is in wp-config + DNS/SSL.
        RewriteMode::Single | RewriteMode::SubdomainMultisite => {
            "\t\tlocation / {\n\
             \t\t\ttry_files $uri $uri/ /index.php?$args;\n\
             \t\t}\n"
                .to_string()
        }
        // Subdirectory multisite: WordPress's official network rewrite rules.
        RewriteMode::SubdirectoryMultisite => {
            "\t\tlocation / {\n\
             \t\t\ttry_files $uri $uri/ /index.php?$args;\n\
             \t\t}\n\
             \t\tif (!-e $request_filename) {\n\
             \t\t\trewrite /wp-admin$ $scheme://$host$uri/ permanent;\n\
             \t\t\trewrite ^(/[^/]+)?(/wp-.*) $2 last;\n\
             \t\t\trewrite ^(/[^/]+)?(/.*\\.php) $2 last;\n\
             \t\t}\n"
                .to_string()
        }
    }
}

/// FastCGI params our nginx template passes to php-fpm (inlined so we don't
/// depend on an external `fastcgi_params` file). A NAMED list so
/// `core::site_env::RESERVED` is tested against it — every param emitted here
/// must be rejected as a user env-var name (duplicate FastCGI params reach PHP
/// undefined; overriding SCRIPT_FILENAME/DOCUMENT_ROOT = arbitrary file serving).
pub const TEMPLATE_FCGI_PARAMS: &[(&str, &str)] = &[
    ("SCRIPT_FILENAME", "$document_root$fastcgi_script_name"),
    ("QUERY_STRING", "$query_string"),
    ("REQUEST_METHOD", "$request_method"),
    ("CONTENT_TYPE", "$content_type"),
    ("CONTENT_LENGTH", "$content_length"),
    ("SCRIPT_NAME", "$fastcgi_script_name"),
    ("REQUEST_URI", "$request_uri"),
    ("DOCUMENT_URI", "$document_uri"),
    ("DOCUMENT_ROOT", "$document_root"),
    ("SERVER_PROTOCOL", "$server_protocol"),
    ("GATEWAY_INTERFACE", "CGI/1.1"),
    ("SERVER_SOFTWARE", "nginx"),
    ("REMOTE_ADDR", "$remote_addr"),
    ("REMOTE_PORT", "$remote_port"),
    ("SERVER_ADDR", "$server_addr"),
    ("SERVER_PORT", "$server_port"),
    ("SERVER_NAME", "$server_name"),
    ("REQUEST_SCHEME", "$scheme"),
    // HTTPS=on is set when Caddy forwards an https request.
    ("HTTPS", "$rexenv_https if_not_empty"),
];

fn fcgi_params() -> String {
    TEMPLATE_FCGI_PARAMS
        .iter()
        .map(|(name, value)| format!("\t\t\tfastcgi_param {name} {value};\n"))
        .collect()
}

/// Per-site user env vars as `fastcgi_param` lines, AFTER the template params
/// (deterministic; name collisions are impossible — reserved names rejected).
/// Values emit double-quoted with `\`/`"` escaped; everything unescapable
/// (`$`, `{`, `}`, control chars) was rejected at validation.
fn env_params(env: &[(String, String)]) -> String {
    env.iter()
        .map(|(name, value)| {
            format!(
                "\t\t\tfastcgi_param {name} \"{}\";\n",
                crate::core::site_env::escape_value(value)
            )
        })
        .collect()
}

/// Dot-segment guard, emitted before the `.php` location (regex locations match
/// in order — `/.hidden/x.php` must 404, never reach fastcgi). Docroots carry
/// `.git`/`.env` (git-cloned plugins, hand-copied repos) and tunnels make a
/// served docroot PUBLIC; only the root `/.well-known/` subtree stays reachable
/// (ACME/app probes). 404, not 403 — don't advertise what exists.
const NGINX_DOTFILE_DENY: &str = "\t\tlocation ~ /\\.(?!well-known(/|$)) {\n\
     \t\t\treturn 404;\n\
     \t\t}\n";

fn server_block(http_port: u16, site: &NginxSite) -> String {
    // Subdomain multisite serves every sub-site (`a.mysite.test`) from the same
    // block, so the wildcard joins the exact host in `server_name` (§10.2).
    let server_name = match site.rewrite {
        RewriteMode::SubdomainMultisite => format!("{d} *.{d}", d = site.domain),
        _ => site.domain.clone(),
    };
    // `absolute_redirect off` → nginx issues RELATIVE redirects. It listens on an
    // internal loopback port behind the Caddy edge, so an absolute redirect (e.g. the
    // `/wp-admin` → `/wp-admin/` directory redirect) would otherwise leak
    // `http://<host>:18088/…` to the browser and break the request.
    let body_limit = site
        .body_limit
        .map(|b| format!("\t\tclient_max_body_size {b};\n"))
        .unwrap_or_default();
    format!(
        "\n\tserver {{\n\
         \t\tlisten 127.0.0.1:{port};\n\
         \t\tserver_name {server_name};\n\
         \t\tabsolute_redirect off;\n\
         {body_limit}\
         \t\troot \"{root}\";\n\
         \t\tindex index.php index.html;\n\
         {rewrite}\
         {dotdeny}\
         \t\tlocation ~ \\.php$ {{\n\
         \t\t\tfastcgi_pass 127.0.0.1:{fpm};\n\
         \t\t\tfastcgi_index index.php;\n\
         {params}\
         {env}\
         \t\t}}\n\
         \t}}\n",
        port = http_port,
        server_name = server_name,
        root = site.docroot.display(),
        fpm = site.php_fpm_port,
        rewrite = rewrite_block(site.rewrite),
        dotdeny = NGINX_DOTFILE_DENY,
        params = fcgi_params(),
        env = env_params(&site.env),
    )
}

/// Render the shared nginx config: one `http {}` with a `server {}` per site,
/// each routed by `server_name` and proxying `.php` to php-fpm.
pub fn generate_nginx_config(cfg: &NginxConfig) -> String {
    let mut s = String::new();
    s.push_str("worker_processes 1;\n");
    s.push_str("daemon off;\n"); // foreground for ProcessSupervisor
    // Quote path values: app-data paths contain spaces ("Application Support").
    s.push_str(&format!("pid \"{}\";\n", cfg.pid.display()));
    s.push_str(&format!("error_log \"{}\";\n", cfg.error_log.display()));
    s.push_str("events {\n\tworker_connections 256;\n}\n\n");
    s.push_str("http {\n");
    // Per-HOST access lines (host + ISO time + bytes) — the default "combined"
    // format has no $host, so per-site activity (req/min + bytes on the Sites
    // page) couldn't be attributed. Kept minimal on purpose: this log is parsed
    // every poll (core::site_metrics).
    s.push_str("\tlog_format rexenv '$host $time_iso8601 $body_bytes_sent';\n");
    s.push_str(&format!("\taccess_log \"{}\" rexenv;\n", cfg.access_log.display()));
    s.push_str(
        "\ttypes {\n\
         \t\ttext/html html htm;\n\
         \t\ttext/css css;\n\
         \t\tapplication/javascript js;\n\
         \t\timage/png png;\n\
         \t\timage/jpeg jpg jpeg;\n\
         \t\timage/gif gif;\n\
         \t\timage/svg+xml svg;\n\
         \t\tapplication/json json;\n\
         \t}\n",
    );
    s.push_str("\tdefault_type application/octet-stream;\n");
    s.push_str("\tsendfile on;\n");
    s.push_str("\tclient_max_body_size 128m;\n");
    s.push_str(&format!(
        "\tclient_body_temp_path \"{t}/client_body\";\n\
         \tfastcgi_temp_path \"{t}/fastcgi\";\n\
         \tproxy_temp_path \"{t}/proxy\";\n\
         \tuwsgi_temp_path \"{t}/uwsgi\";\n\
         \tscgi_temp_path \"{t}/scgi\";\n",
        t = cfg.temp_root.display()
    ));
    // Map Caddy's X-Forwarded-Proto to an HTTPS flag for php (WordPress is_ssl()).
    s.push_str("\tmap $http_x_forwarded_proto $rexenv_https {\n\t\tdefault '';\n\t\thttps on;\n\t}\n");
    for site in &cfg.sites {
        s.push_str(&server_block(cfg.http_port, site));
    }
    s.push_str("}\n");
    s
}

/// Write the shared nginx config + create its prefix/log/temp dirs. Returns
/// `(config_path, prefix_dir)`.
pub fn write_nginx_config(
    platform: &dyn Platform,
    http_port: u16,
    sites: Vec<NginxSite>,
) -> Result<(PathBuf, PathBuf)> {
    let config_dir = platform.paths().config_dir()?;
    let log_dir = platform.paths().log_dir()?;
    let prefix = platform.paths().app_data_dir()?.join("nginx");
    let temp_root = prefix.join("tmp");
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&log_dir)?;
    std::fs::create_dir_all(&temp_root)?;

    let cfg = NginxConfig {
        http_port,
        pid: prefix.join("nginx.pid"),
        error_log: log_dir.join("nginx-error.log"),
        access_log: log_dir.join("nginx-access.log"),
        temp_root,
        sites,
    };
    let conf = config_dir.join("nginx.conf");
    std::fs::write(&conf, generate_nginx_config(&cfg))?;
    Ok((conf, prefix))
}

fn nginx_args(conf: &Path, prefix: &Path, flag: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(f) = flag {
        args.push(f.to_string());
    }
    // Prefix must end with a separator for nginx.
    args.push("-p".to_string());
    args.push(format!("{}/", prefix.display()));
    args.push("-c".to_string());
    args.push(conf.display().to_string());
    args
}

/// Validate the nginx config (`nginx -t`).
pub fn test_nginx_config(
    platform: &dyn Platform,
    nginx_bin: &Path,
    conf: &Path,
    prefix: &Path,
) -> Result<()> {
    let mut child = platform
        .supervisor()
        .spawn(nginx_bin, &nginx_args(conf, prefix, Some("-t")))?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "nginx -t failed (exit {:?})",
            status.code()
        )))
    }
}

/// Start the shared nginx (foreground via `daemon off`) via `ProcessSupervisor`.
pub fn start_nginx(
    platform: &dyn Platform,
    nginx_bin: &Path,
    conf: &Path,
    prefix: &Path,
) -> Result<Child> {
    let log = platform.paths().log_dir()?.join("nginx-stdout.log");
    platform
        .supervisor()
        .spawn_logged(nginx_bin, &nginx_args(conf, prefix, None), &log)
}

/// Reload a running nginx's config (`nginx -s reload`) after the config changes
/// (e.g. a site was added/removed).
pub fn reload_nginx(
    platform: &dyn Platform,
    nginx_bin: &Path,
    conf: &Path,
    prefix: &Path,
) -> Result<()> {
    let mut args = vec!["-s".to_string(), "reload".to_string()];
    args.extend(nginx_args(conf, prefix, None));
    let mut child = platform.supervisor().spawn(nginx_bin, &args)?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "nginx -s reload failed (exit {:?})",
            status.code()
        )))
    }
}

/// True if the shared nginx is accepting connections on its loopback port.
pub fn nginx_running(port: u16) -> bool {
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
    fn fpm_config_has_global_and_pool() {
        let cfg = generate_fpm_config(
            9783,
            Path::new("/run/php-fpm-8.3.pid"),
            Path::new("/logs/php-fpm-8.3.log"),
            None,
            &[],
        );
        assert!(cfg.contains("[global]"));
        assert!(cfg.contains("daemonize = no"));
        assert!(cfg.contains("pid = /run/php-fpm-8.3.pid"));
        assert!(cfg.contains("error_log = /logs/php-fpm-8.3.log"));
        assert!(cfg.contains("[www]"));
        assert!(cfg.contains("listen = 127.0.0.1:9783"));
        assert!(cfg.contains("pm = dynamic"));
        // Starvation guards (all default sites share one pool): headroom + stuck-
        // worker recycling — see generate_fpm_config.
        assert!(cfg.contains("pm.max_children = 10"));
        assert!(cfg.contains("request_terminate_timeout = 300s"));
        assert!(cfg.contains("pm.max_requests = 500"));
        // No user/group: we run as the current user, not root.
        assert!(!cfg.contains("\nuser ="));
        assert!(!cfg.contains("\ngroup ="));
        // No mail routing unless requested.
        assert!(!cfg.contains("sendmail_path"));
    }

    #[test]
    fn fpm_config_pins_sendmail_path_when_given() {
        let shim = "'/opt/mailpit' sendmail -t -S 127.0.0.1:11025";
        let cfg = generate_fpm_config(
            9783,
            Path::new("/run/php-fpm-8.3.pid"),
            Path::new("/logs/php-fpm-8.3.log"),
            Some(shim),
            &[],
        );
        // Routed via php_admin_value (sites can't override it), double-quoted so
        // the ini parser preserves the inner single-quoted binary path.
        assert!(cfg.contains(&format!("php_admin_value[sendmail_path] = \"{shim}\"")));
        // Sits inside the [www] pool, after the pm.* directives.
        assert!(cfg.find("[www]").unwrap() < cfg.find("sendmail_path").unwrap());
    }

    #[test]
    fn fpm_config_writes_php_values_and_tracks_terminate_timeout() {
        let settings = vec![
            ("memory_limit".to_string(), "512M".to_string()),
            ("upload_max_filesize".to_string(), "64M".to_string()),
            ("max_execution_time".to_string(), "600".to_string()),
        ];
        let cfg = generate_fpm_config(
            9783,
            Path::new("/run/php-fpm-8.3.pid"),
            Path::new("/logs/php-fpm-8.3.log"),
            None,
            &settings,
        );
        assert!(cfg.contains("php_value[memory_limit] = 512M"));
        assert!(cfg.contains("php_value[upload_max_filesize] = 64M"));
        assert!(cfg.contains("php_value[max_execution_time] = 600"));
        // The wall-clock recycle rises to the user's max_execution_time so a
        // long request isn't killed at 300s (the setting must not lie).
        assert!(cfg.contains("request_terminate_timeout = 600s"), "got: {cfg}");
    }

    #[test]
    fn fpm_terminate_timeout_keeps_300s_floor() {
        // Below the floor, or 0 (unlimited CPU time): the 300s wall-clock stuck-
        // worker guard stays — surfaced as a documented cap in the UI.
        for val in ["30", "0"] {
            let settings = vec![("max_execution_time".to_string(), val.to_string())];
            let cfg = generate_fpm_config(
                9783,
                Path::new("/p.pid"),
                Path::new("/l.log"),
                None,
                &settings,
            );
            assert!(cfg.contains("request_terminate_timeout = 300s"), "val {val}: {cfg}");
        }
    }

    #[test]
    fn fpm_running_false_on_closed_port() {
        // An unlikely-to-be-open high port: status should read stopped.
        assert!(!fpm_running(8))
    }

    fn nginx_cfg(mode: RewriteMode) -> NginxConfig {
        NginxConfig {
            http_port: 18088,
            pid: PathBuf::from("/run/nginx.pid"),
            error_log: PathBuf::from("/logs/nginx-error.log"),
            access_log: PathBuf::from("/logs/nginx-access.log"),
            temp_root: PathBuf::from("/tmp/rexenv-nginx"),
            sites: vec![NginxSite {
                domain: "acme.test".into(),
                docroot: PathBuf::from("/Sites/acme/public"),
                php_fpm_port: 9783,
                rewrite: mode,
                body_limit: None,
                env: Vec::new(),
            }],
        }
    }

    #[test]
    fn nginx_body_limit_is_per_server_and_optional() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites[0].body_limit = Some(64 * 1024 * 1024);
        let out = generate_nginx_config(&cfg);
        // Inside the server block, mirroring the site's PHP upload/post sizes…
        assert!(out.contains("client_max_body_size 67108864;"), "got: {out}");
        // …while the http-level default stays for sites without settings.
        assert!(out.contains("client_max_body_size 128m;"));
        let none = generate_nginx_config(&nginx_cfg(RewriteMode::Single));
        assert_eq!(none.matches("client_max_body_size").count(), 1);
    }

    #[test]
    fn nginx_config_has_shared_listen_and_fastcgi() {
        let cfg = generate_nginx_config(&nginx_cfg(RewriteMode::Single));
        assert!(cfg.contains("daemon off;"));
        assert!(cfg.contains("listen 127.0.0.1:18088;"));
        assert!(cfg.contains("server_name acme.test;"));
        assert!(cfg.contains("root \"/Sites/acme/public\";"));
        // Relative redirects only, so nginx's internal :18088 never leaks to the browser
        // on a directory redirect like /wp-admin → /wp-admin/.
        assert!(cfg.contains("absolute_redirect off;"));
        assert!(cfg.contains("fastcgi_pass 127.0.0.1:9783;"));
        assert!(cfg.contains("fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;"));
        assert!(cfg.contains("try_files $uri $uri/ /index.php?$args;"));
        // HTTPS flag mapped from Caddy's X-Forwarded-Proto.
        assert!(cfg.contains("map $http_x_forwarded_proto $rexenv_https"));
    }

    #[test]
    fn site_env_emits_escaped_fastcgi_params_inside_the_php_location() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites[0].env = vec![
            ("API_URL".into(), "https://api.example.test/v2".into()),
            ("MESSAGE".into(), "he said \"hi\" and C:\\path".into()),
        ];
        let out = generate_nginx_config(&cfg);
        assert!(out.contains("fastcgi_param API_URL \"https://api.example.test/v2\";"), "got: {out}");
        // Quotes/backslashes escaped → the value can't close the string.
        assert!(
            out.contains("fastcgi_param MESSAGE \"he said \\\"hi\\\" and C:\\\\path\";"),
            "got: {out}"
        );
        // Emitted AFTER the template params (deterministic order).
        let std_pos = out.find("fastcgi_param HTTPS").unwrap();
        assert!(out.find("fastcgi_param API_URL").unwrap() > std_pos);
        // No env → no extra params, config identical shape to before.
        let none = generate_nginx_config(&nginx_cfg(RewriteMode::Single));
        assert!(!none.contains("API_URL"));
    }

    #[test]
    fn dotfile_paths_are_denied_before_php_execution() {
        // A cloned plugin's `.git/`, a repo `.env`: 404 (root /.well-known/
        // exempt). Regex locations match in ORDER — the deny must precede the
        // `.php` location so `/.hidden/x.php` hits the deny, never fastcgi.
        for mode in [
            RewriteMode::Single,
            RewriteMode::SubdomainMultisite,
            RewriteMode::SubdirectoryMultisite,
        ] {
            let out = generate_nginx_config(&nginx_cfg(mode));
            assert!(
                out.contains("location ~ /\\.(?!well-known(/|$)) {"),
                "got: {out}"
            );
            assert!(out.contains("return 404;"));
            assert!(
                out.find("location ~ /\\.").unwrap()
                    < out.find("location ~ \\.php$").unwrap(),
                "deny must precede the php location"
            );
        }
    }

    #[test]
    fn rewrite_slots_differ_by_mode() {
        // Single has no WP network rewrites; subdirectory multisite does.
        assert!(!generate_nginx_config(&nginx_cfg(RewriteMode::Single)).contains("rewrite /wp-admin$"));
        assert!(generate_nginx_config(&nginx_cfg(RewriteMode::SubdirectoryMultisite))
            .contains("rewrite /wp-admin$"));
        // Subdomain multisite routes like single (try_files), no path stripping.
        let sub = generate_nginx_config(&nginx_cfg(RewriteMode::SubdomainMultisite));
        assert!(sub.contains("try_files $uri $uri/ /index.php?$args;"));
        assert!(!sub.contains("rewrite /wp-admin$"));
    }

    #[test]
    fn subdomain_multisite_server_name_includes_wildcard() {
        // Subdomain multisite must serve every `*.acme.test` sub-site from the one
        // block (§10.2); single/subdirectory stay exact-host only.
        let sub = generate_nginx_config(&nginx_cfg(RewriteMode::SubdomainMultisite));
        assert!(sub.contains("server_name acme.test *.acme.test;"), "got: {sub}");
        let single = generate_nginx_config(&nginx_cfg(RewriteMode::Single));
        assert!(single.contains("server_name acme.test;"));
        assert!(!single.contains("*.acme.test"));
    }

    #[test]
    fn nginx_renders_each_site() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites.push(NginxSite {
            domain: "two.test".into(),
            docroot: PathBuf::from("/Sites/two"),
            php_fpm_port: 9783,
            rewrite: RewriteMode::Single,
            body_limit: None,
            env: Vec::new(),
        });
        let out = generate_nginx_config(&cfg);
        assert!(out.contains("server_name acme.test;"));
        assert!(out.contains("server_name two.test;"));
        assert_eq!(out.matches("fastcgi_pass").count(), 2);
    }
}
