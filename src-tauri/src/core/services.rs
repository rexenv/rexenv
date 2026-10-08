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
/// be added later, each on its own port. When `catch` is `Some`, the pool pins
/// BOTH halves of the mail catch-all: `php_admin_value[sendmail_path]`, so every
/// site's PHP `mail()` goes through the Mailpit shim (§2.2) and sites can't
/// override it (`_admin_`), and `env[MAIL_*]`, which a Laravel app cannot
/// outvote from its own `.env` (see [`super::mail::laravel_env`]).
/// `settings` are the user's whitelisted, pre-validated per-version ini values
/// (`core::php::SETTINGS`), written as overridable `php_value[key]` lines so
/// WordPress can still `ini_set()` at runtime.
pub fn generate_fpm_config(
    port: u16,
    pid_file: &Path,
    log_file: &Path,
    catch: Option<&super::mail::Catch>,
    settings: &[(String, String)],
    mysql_socket: Option<&Path>,
) -> String {
    // Pool sizing guards the "every site hangs while the UI shows running" spiral:
    // ALL default sites share this one pool, so N wall-clock-stuck workers (heavy
    // plugin imports, loopback self-requests) starve every site at once — the port
    // still accepts, so no probe sees it. `request_terminate_timeout` recycles a
    // stuck worker after 5min; `pm.max_requests` recycles leaky workers.
    //
    // This comment used to say max_execution_time "only counts CPU time on unix",
    // which is why request_terminate_timeout was described as the only wall-clock
    // guard. NOT TRUE of our builds: `nginx_php_serve` had a plain `sleep(66)`
    // killed at "Maximum execution time of 30 seconds exceeded" (6 Aug 2026), so
    // max_execution_time bites wall time here. request_terminate_timeout still
    // earns its place — it kills a worker PHP's own limit can't (a hang inside a
    // blocking extension call, and any request where the setting was raised) —
    // but it is not the only thing standing between a long request and a kill.
    //
    // Wrap the value in DOUBLE quotes: PHP's ini parser strips the outer quotes
    // but preserves the inner single-quoted binary path verbatim, so the shim's
    // space-containing path survives to `sh -c`. (Bare single quotes get eaten by
    // the ini parser, leaving an unquoted path that `sh` splits on the space.)
    let sendmail = catch
        .map(|c| format!("php_admin_value[sendmail_path] = \"{}\"\n", c.sendmail_path))
        .unwrap_or_default();
    // The Laravel half. `env[…]` and not `php_admin_value[…]`, because the
    // variable has to reach the PROCESS environment: Laravel reads it through
    // Dotenv's immutable repository, which is what makes it beat the site's own
    // `.env`. An ini value would be invisible to `env()` and the app would keep
    // mailing wherever its file said.
    //
    // php-fpm's `clear_env` stays at its default (yes), so this is the whole
    // environment a worker gets — nothing of the user's shell leaks in beside it.
    //
    // **Every value is QUOTED, and that is not tidiness.** php-fpm parses this
    // file with PHP's ini parser, which reads the BARE words `null`, `none`,
    // `off`, `no` and `false` as the empty string — and an `env[]` whose value
    // parses empty is a hard `ERROR: empty value` that refuses the whole config
    // and takes the pool down with it. `MAIL_URL = null` did exactly that,
    // measured 4 Sep 2026 against a real php-fpm 8.2 before this shipped. In
    // quotes the parser keeps the four characters, which is what Laravel's
    // `Env` then maps to a real null.
    let mail_env: String = catch
        .map(|c| {
            c.env
                .iter()
                .map(|(k, v)| format!("env[{k}] = \"{v}\"\n"))
                .collect::<String>()
        })
        .unwrap_or_default();
    // The `DB_HOST=localhost` free win (Stage 3 D5): PHP treats `localhost`
    // as "use the unix socket", and our static builds compile
    // `mysqli.default_socket` in EMPTY (verified on the cached 8.0/8.3/8.5
    // binaries) — so an imported localhost WordPress site fails today, and
    // pointing the default at OUR MySQL socket is strictly additive: no
    // config rexenv generates uses `localhost` (provisioning writes
    // `127.0.0.1:<port>`), so no existing rexenv site can be affected.
    //
    // MySQL's socket on every pool, deliberately — per-pool-majority was
    // rejected as derived, mutable state deciding runtime behaviour; a
    // MariaDB-on-localhost site gets the tell-only "use 127.0.0.1:13307" in
    // its own panel instead of a silent half-support.
    //
    // `pdo_mysql.default_socket` stays out PERMANENTLY (settled 28 Jul
    // 2026, plan §2): its compiled default is `/tmp/mysql.sock` (verified) —
    // the Homebrew MySQL location — so an override would silently redirect a
    // linked PDO site that works against a Homebrew server today: it keeps
    // running and writes to the WRONG database with no error. The mysqli
    // half has no such hazard because its compiled default is empty. The
    // pin test asserting the absent line is intent, not an omission.
    let socket = mysql_socket
        .map(|p| format!("php_admin_value[mysqli.default_socket] = \"{}\"\n", p.display()))
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
         pm.max_children = {max_children}\n\
         pm.start_servers = 2\n\
         pm.min_spare_servers = 1\n\
         pm.max_spare_servers = 3\n\
         pm.max_requests = 500\n\
         request_terminate_timeout = {terminate}s\n\
         catch_workers_output = yes\n\
         {sendmail}\
         {mail_env}\
         {socket}\
         {values}",
        pid = pid_file.display(),
        log = log_file.display(),
        max_children = FPM_MAX_CHILDREN,
    )
}

/// Write the php-fpm config for `version` (on `port`) under the config dir,
/// creating the run/log dirs. `catch` (when set) routes the pool's PHP `mail()`
/// AND its Laravel apps' mail into Mailpit (§2.2). Returns the config path.
pub fn write_fpm_config(
    platform: &dyn Platform,
    version: &str,
    port: u16,
    catch: Option<&super::mail::Catch>,
    settings: &[(String, String)],
) -> Result<PathBuf> {
    write_fpm_config_named(platform, version, port, catch, settings, "conf")
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
    // No catch-all lines: the shim path and the MAIL_* set live behind the
    // services lock and their fixed-format lines can't be invalidated by user
    // settings — the gate is about the user's values.
    write_fpm_config_named(platform, version, port, None, settings, "conf.candidate")
}

fn write_fpm_config_named(
    platform: &dyn Platform,
    version: &str,
    port: u16,
    catch: Option<&super::mail::Catch>,
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
    // Asked, not spelled: the Logs tab reads this same name (ledger #650).
    let log = log_dir.join(platform.supervisor().php_pool_model().log_name(version));
    // Same fixed-format-line reasoning as the catch-all: the candidate (`-t` gate
    // for user settings) omits the socket default; the real config gets it.
    let mysql_socket =
        (ext == "conf").then(|| super::database::socket_path(platform)).transpose()?;
    std::fs::write(
        &conf,
        generate_fpm_config(port, &pid, &log, catch, settings, mysql_socket.as_deref()),
    )?;
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
    let log = platform
        .paths()
        .log_dir()?
        .join(platform.supervisor().php_pool_model().output_log_name(""));
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
    let log = platform
        .paths()
        .log_dir()?
        .join(platform.supervisor().php_pool_model().output_log_name(""));
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
            "php-fpm config test failed (exit {})",
            crate::core::proc::exit_text(status.code())
        )))
    }
}

/// Stop a running php-fpm by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// A php-fpm pool's `pm.max_children` — also the worker count the busy-workers note compares
/// held connections with (`php::pool_workers`, ledger #608), so the two cannot drift apart.
pub const FPM_MAX_CHILDREN: u32 = 10;

/// Service status the services/UI layer reads: a pool is `running` iff its
/// loopback FastCGI port accepts a connection.
pub fn fpm_running(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// One FastCGI `FCGI_GET_VALUES` management record asking for `FCGI_MPXS_CONNS` — a value PHP
/// always sets: version 1, type 9, request id 0, a 17-byte body (name length 15, value length 0,
/// the name), no padding.
const GET_VALUES_RECORD: [u8; 25] = [
    1, 9, 0, 0, 0, 17, 0, 0, 15, 0, b'F', b'C', b'G', b'I', b'_', b'M', b'P', b'X', b'S', b'_', b'C',
    b'O', b'N', b'N', b'S',
];

/// Whether a pool's FastCGI layer ANSWERS: one [`GET_VALUES_RECORD`] round trip, a
/// `FCGI_GET_VALUES_RESULT` (type 10) header back inside `timeout`, no script run.
///
/// PHP answers the record in a WORKER, after `accept()` (main/fastcgi.c, the same code for php-fpm
/// and php-cgi), so a yes means a worker is up and free — and a no means none is: a pool whose
/// every worker is busy and a frozen pool look the same (measured on both OSes, 14 Sep 2026,
/// `examples/pool_get_values_probe.rs`). Readiness reads it alone; health reads it together with
/// the connections held on the port (`php::pool_serving`, ledger #607).
pub fn pool_answers(port: u16, timeout: Duration) -> bool {
    use std::io::{Read, Write};
    let Ok(mut stream) = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let mut head = [0u8; 8];
    stream.write_all(&GET_VALUES_RECORD).is_ok() && stream.read_exact(&mut head).is_ok() && head[0] == 1 && head[1] == 10
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
    /// FastCGI read/send timeout in SECONDS, or `None` for nginx's own 60s
    /// default. Set it only where nginx must NOT be the component that gives up:
    /// a timeout here returns 504 while php-fpm keeps working, so the user sees
    /// a failure over a job that is still running (a half-loaded database, for an
    /// import). php-fpm's `request_terminate_timeout` is the guard that should
    /// fire — it actually ends the work.
    pub read_timeout: Option<u64>,
    /// Per-request `PHP_VALUE` ini lines for this vhost (`key=value`, one per
    /// line), or `None` for the pool's own settings. php-fpm applies these OVER
    /// the pool's overridable `php_value[…]` lines, so a single vhost can raise
    /// a limit without touching the shared pool or any other site on it.
    /// OURS ONLY — never user input (unlike `env`, which is validated); the
    /// renderer escapes newlines for nginx and nothing else.
    pub php_value: Option<String>,
    /// Extra hostnames this site also answers on (v42). They join
    /// `server_name`, so one server block serves every name — never a second
    /// block, which would double every future change to this site and let the
    /// two drift.
    pub aliases: Vec<String>,
    /// Laravel only: the project's `storage/app/public` directory, served at
    /// `/storage/…` the way Valet's own driver does.
    ///
    /// **Why this exists at all.** Laravel's documented setup is `php artisan
    /// storage:link`, a `public/storage` symlink — but Valet serves `/storage/*`
    /// from the real directory WITHOUT that symlink, so a project developed
    /// under Valet can rely on uploads resolving and nobody ever ran the
    /// artisan command. Imported into rexenv, every one of those URLs 404s, and
    /// the site looks broken in a way that points at the migration and not at
    /// the missing symlink.
    ///
    /// `None` for every non-Laravel site and for a project with no such
    /// directory.
    pub storage_root: Option<PathBuf>,
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
    /// Sites the user STOPPED (v44) — a server block that answers the stopped
    /// page with 503 and nothing else. See [`NginxStopped`].
    pub stopped: Vec<NginxStopped>,
}

/// A stopped site's presence in the SHARED nginx (v44).
///
/// # Why a stopped site still gets a server block
///
/// The first cut gave it none, reasoning that the honest 503 belongs at the edge
/// — and that was wrong in the one way that matters, found by the owner within a
/// day: **nginx serves the FIRST matching server, and with no match it serves its
/// default server, which is another site.** Anything that reaches nginx without
/// going through Caddy therefore got a neighbour's content at the stopped site's
/// address. A public tunnel does exactly that — `cloudflared` proxies to the
/// shared nginx with `--http-host-header <domain>` — so sharing a stopped site
/// published somebody else's site to the internet. That is the same cross-site
/// fallthrough `override_fallthrough_check` measures for override sites, and the
/// reason `tunnels.rs` refuses to share those.
///
/// So the rule is: **every name rexenv knows must resolve to its OWN answer in
/// every tier that can be reached directly.** A stopped site's answer is the
/// stopped page, served here as well as at the edge — one file, two servers.
#[derive(Debug, Clone)]
pub struct NginxStopped {
    pub domain: String,
    /// This site's OWN stopped-page directory (`stopped_page::ensure_for`) — the
    /// page names the site, so it cannot be shared with another one.
    pub page_dir: PathBuf,
    /// Extra hostnames (v42) — they must be covered too, or the alias of a
    /// stopped site falls through while its primary does not.
    pub aliases: Vec<String>,
    /// Subdomain multisite: `*.domain` joins the block, for the same reason the
    /// serving block does it — otherwise `a.stopped.test` reaches a neighbour.
    pub wildcard: bool,
}

/// The `location /` (and any rewrite) block for a site's mode. Single uses plain
/// try_files; subdirectory multisite adds WordPress's network rewrite rules.
fn rewrite_block(mode: RewriteMode) -> String {
    match mode {
        RewriteMode::Single => {
            "\t\tlocation / {\n\
             \t\t\ttry_files $uri $uri/ /index.php?$args;\n\
             \t\t}\n"
                .to_string()
        }
        // BOTH network modes get WordPress's official network rewrite rules.
        //
        // Subdirectory multisite needs them to find the script at all. Subdomain
        // multisite does NOT need them locally — each subdomain is its own WP
        // host, so `/wp-admin/` already exists on disk and the `!-e` guard keeps
        // the whole block inert. It needs them through a TUNNEL: one tunnel pins
        // one Host and issues no wildcard, so a subdomain network's sub-sites are
        // served as SUBDIRECTORIES while shared (`core/wp_tunnel`'s sunrise
        // drop-in), and `/s1/wp-admin/` is a path with no file behind it. The
        // rules point nginx at the real `/wp-admin/`, while `REQUEST_URI` stays
        // `$request_uri` — the ORIGINAL, prefix and all — which is exactly what
        // sunrise reads to decide which blog the request belongs to. Take either
        // half away and the sub-site admin 404s (measured 27 Aug 2026).
        RewriteMode::SubdomainMultisite | RewriteMode::SubdirectoryMultisite => {
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

/// Valet's `/storage/*` mapping for a Laravel project, or nothing.
///
/// Three things are deliberate here, and each of them is a hole if dropped:
///
/// - **`^~`**, so this prefix beats the regex locations below it — that is what
///   makes the mapping take effect at all.
/// - **PHP is refused inside it.** Because `^~` wins over `location ~ \.php$`,
///   a `.php` file under `storage/app/public` would otherwise be served as
///   SOURCE. That directory holds user uploads; serving one as source is a
///   disclosure, and executing it would be worse.
/// - **Dotfiles are refused inside it**, for the same reason: `^~` also beats
///   the vhost's dotfile deny, so the guard has to be repeated INSIDE rather
///   than assumed from outside. A `.env` copied into an uploads folder is
///   exactly the file this project already refuses to serve everywhere else.
fn storage_block(storage_root: Option<&Path>) -> String {
    let Some(root) = storage_root else { return String::new() };
    format!(
        "\t\tlocation ^~ /storage/ {{\n\
         \t\t\tlocation ~ /\\.(?!well-known(/|$)) {{ return 404; }}\n\
         \t\t\tlocation ~* \\.php$ {{ return 404; }}\n\
         \t\t\talias \"{root}/\";\n\
         \t\t\ttry_files $uri =404;\n\
         \t\t}}\n",
        root = nginx_path(root)
    )
}

/// A path as nginx.conf must spell it: forward slashes only.
///
/// Inside a quoted nginx string a backslash starts an escape. Measured on the Dell
/// (14 Sep 2026, nginx 1.30.4): `…\rexenv-probe-w3\ngx prefix\nginx.pid` was read as
/// `exenv-probe-w3 / gx prefix / ginx.pid` — `\r` and `\n` swallowed — and `nginx -t`
/// failed, while the same config with forward slashes passed. Every path written into the
/// nginx config goes through here; a path whose separator is `/` is returned unchanged.
pub fn nginx_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn server_block(http_port: u16, site: &NginxSite) -> String {
    // Subdomain multisite serves every sub-site (`a.mysite.test`) from the same
    // block, so the wildcard joins the exact host in `server_name` (§10.2).
    let mut names = match site.rewrite {
        RewriteMode::SubdomainMultisite => vec![site.domain.clone(), format!("*.{}", site.domain)],
        _ => vec![site.domain.clone()],
    };
    // Extra domains join the SAME block. A subdomain network's alias gets the
    // wildcard too, or `a.alias.test` would fall through to nginx's default
    // server while `a.primary.test` works — a half-migrated network is worse
    // than one that never accepted the alias.
    for alias in &site.aliases {
        names.push(alias.clone());
        if matches!(site.rewrite, RewriteMode::SubdomainMultisite) {
            names.push(format!("*.{alias}"));
        }
    }
    let server_name = names.join(" ");
    // `absolute_redirect off` → nginx issues RELATIVE redirects. It listens on an
    // internal loopback port behind the Caddy edge, so an absolute redirect (e.g. the
    // `/wp-admin` → `/wp-admin/` directory redirect) would otherwise leak
    // `http://<host>:18088/…` to the browser and break the request.
    let body_limit = site
        .body_limit
        .map(|b| format!("\t\tclient_max_body_size {b};\n"))
        .unwrap_or_default();
    // Both directions: `read` covers PHP's think time (the long one), `send`
    // covers streaming a multi-GB body up to the pool.
    let timeout = site
        .read_timeout
        .map(|s| format!("\t\t\tfastcgi_read_timeout {s}s;\n\t\t\tfastcgi_send_timeout {s}s;\n"))
        .unwrap_or_default();
    // nginx has no literal newline inside a quoted string, but it does expand the
    // `\n` escape — which is what php-fpm needs to split PHP_VALUE into ini lines.
    let php_value = site
        .php_value
        .as_deref()
        .map(|v| format!("\t\t\tfastcgi_param PHP_VALUE \"{}\";\n", v.replace('\n', "\\n")))
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
         {storage}\
         {dotdeny}\
         \t\tlocation ~* \\.php$ {{\n\
         \t\t\tfastcgi_pass 127.0.0.1:{fpm};\n\
         \t\t\tfastcgi_index index.php;\n\
         {params}\
         {timeout}\
         {php_value}\
         {env}\
         \t\t}}\n\
         \t}}\n",
        port = http_port,
        server_name = server_name,
        root = nginx_path(&site.docroot),
        fpm = site.php_fpm_port,
        rewrite = rewrite_block(site.rewrite),
        storage = storage_block(site.storage_root.as_deref()),
        dotdeny = NGINX_DOTFILE_DENY,
        params = fcgi_params(),
        env = env_params(&site.env),
    )
}

/// A stopped site's server block: 503 with rexenv's stopped page, for every
/// path, and no PHP anywhere near it.
///
/// `error_page 503 /stopped.html` + an `internal` exact location is the shape
/// that keeps the STATUS while serving the file (verified against the pinned
/// nginx build before this was written: 503 on `/`, 503 on a deep `.php` path,
/// the neighbouring site untouched). `return 503` sits in `location /` so every
/// path — including `/wp-admin/index.php` — lands there rather than in a PHP
/// handler this block deliberately does not have.
fn stopped_block(http_port: u16, site: &NginxStopped) -> String {
    let mut names = vec![site.domain.clone()];
    if site.wildcard {
        names.push(format!("*.{}", site.domain));
    }
    for alias in &site.aliases {
        names.push(alias.clone());
        if site.wildcard {
            names.push(format!("*.{alias}"));
        }
    }
    format!(
        "\n\tserver {{\n\
         \t\tlisten 127.0.0.1:{port};\n\
         \t\tserver_name {names};\n\
         \t\tabsolute_redirect off;\n\
         \t\troot \"{root}\";\n\
         \t\tlocation = /stopped.html {{\n\
         \t\t\tinternal;\n\
         \t\t}}\n\
         \t\terror_page 503 /stopped.html;\n\
         \t\tlocation / {{\n\
         \t\t\treturn 503;\n\
         \t\t}}\n\
         \t}}\n",
        port = http_port,
        names = names.join(" "),
        root = nginx_path(&site.page_dir),
    )
}

/// Render the shared nginx config: one `http {}` with a `server {}` per site,
/// each routed by `server_name` and proxying `.php` to php-fpm.
pub fn generate_nginx_config(cfg: &NginxConfig) -> String {
    let mut s = String::new();
    s.push_str("worker_processes 1;\n");
    s.push_str("daemon off;\n"); // foreground for ProcessSupervisor
    // Quote path values: app-data paths contain spaces ("Application Support").
    s.push_str(&format!("pid \"{}\";\n", nginx_path(&cfg.pid)));
    s.push_str(&format!("error_log \"{}\";\n", nginx_path(&cfg.error_log)));
    s.push_str("events {\n\tworker_connections 256;\n}\n\n");
    s.push_str("http {\n");
    // Per-HOST access lines (host + ISO time + bytes) — the default "combined"
    // format has no $host, so per-site activity (req/min + bytes on the Sites
    // page) couldn't be attributed. Kept minimal on purpose: this log is parsed
    // every poll (core::site_metrics).
    s.push_str("\tlog_format rexenv '$host $time_iso8601 $body_bytes_sent';\n");
    s.push_str(&format!("\taccess_log \"{}\" rexenv;\n", nginx_path(&cfg.access_log)));
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
    // The global limit is the pools' DEFAULT body ceiling (max of the default
    // upload/post sizes), derived, so a minor with nothing stored — absent from
    // `nginx_body_limits` — never meets a 413 for a body PHP would take. A typed
    // 128m here outlived the 2M/8M PHP defaults it matched (19 Sep 2026).
    s.push_str(&format!("\tclient_max_body_size {};\n", crate::core::php::default_body_limit()));
    s.push_str(&format!(
        "\tclient_body_temp_path \"{t}/client_body\";\n\
         \tfastcgi_temp_path \"{t}/fastcgi\";\n\
         \tproxy_temp_path \"{t}/proxy\";\n\
         \tuwsgi_temp_path \"{t}/uwsgi\";\n\
         \tscgi_temp_path \"{t}/scgi\";\n",
        t = nginx_path(&cfg.temp_root)
    ));
    // Map Caddy's X-Forwarded-Proto to an HTTPS flag for php (WordPress is_ssl()).
    s.push_str("\tmap $http_x_forwarded_proto $rexenv_https {\n\t\tdefault '';\n\t\thttps on;\n\t}\n");
    for site in &cfg.sites {
        s.push_str(&server_block(cfg.http_port, site));
    }
    // Stopped sites LAST is cosmetic — nginx matches by `server_name`, not by
    // order, and no name can be in both lists (a site is served or stopped).
    for site in &cfg.stopped {
        s.push_str(&stopped_block(cfg.http_port, site));
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
    stopped: Vec<NginxStopped>,
) -> Result<(PathBuf, PathBuf)> {
    let config_dir = platform.paths().config_dir()?;
    let log_dir = platform.paths().log_dir()?;
    let prefix = platform.paths().app_data_dir()?.join("nginx");
    let temp_root = prefix.join("tmp");
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&log_dir)?;
    std::fs::create_dir_all(&temp_root)?;
    // nginx opens `<prefix>/logs/error.log` BEFORE it reads the config that names our own
    // error log, and alerts when that folder is missing (measured on the Dell with the
    // Windows build, 14 Sep 2026) — so the folder exists, even though nothing is logged there.
    std::fs::create_dir_all(prefix.join("logs"))?;

    let cfg = NginxConfig {
        http_port,
        pid: prefix.join("nginx.pid"),
        error_log: log_dir.join("nginx-error.log"),
        access_log: log_dir.join("nginx-access.log"),
        temp_root,
        sites,
        stopped,
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
            "nginx -t failed (exit {})",
            crate::core::proc::exit_text(status.code())
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
/// What a reload attempt actually found. The three possible situations are
/// genuinely different and conflating them produced a real bug: a user's import
/// failed with "nginx -s reload failed (exit Some(1))" when the config was
/// perfectly valid and nginx was perfectly healthy — only its pid FILE was
/// unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// The running master was signalled; the new config is live.
    Reloaded,
    /// Nothing is running. NOT an error for a caller that is about to start
    /// nginx — only for one that believed it was already up.
    NotRunning,
}

/// Reload the shared nginx onto the current config.
///
/// Three things this does that the naive `-s reload` did not:
///
/// 1. **Validates first.** `-s reload` reports a bare exit code; `-t` names the
///    file, line and reason. A caller can then show the user something they can
///    act on instead of "exit Some(1)".
/// 2. **Falls back to signalling the master directly** when the pid file is
///    missing, empty or stale. That file is nginx's only way to find its own
///    master, and anything that writes the same prefix can clobber it — but we
///    can identify our master independently, so an unusable pid file should not
///    fail a reload we are perfectly able to perform.
/// 3. **Distinguishes "nothing to reload"** from a failure.
///
/// The fallback uses `owned_master`, which matches our app-data marker on the
/// process command line — the same positive-identification discipline as
/// adoption and the port gate. A foreign nginx on this port is never signalled;
/// if one is there and none of it is ours, that is reported, not signalled.
pub fn reload_nginx(
    platform: &dyn Platform,
    nginx_bin: &Path,
    conf: &Path,
    prefix: &Path,
    port: u16,
) -> Result<ReloadOutcome> {
    // 1) Validate — the diagnosis lives here, not in the reload's exit code.
    let mut test_args = vec!["-t".to_string()];
    test_args.extend(nginx_args(conf, prefix, None));
    let test = crate::platform::command(nginx_bin).args(&test_args).output()?;
    if !test.status.success() {
        return Err(Error::Other(format!(
            "the generated nginx config is invalid, so it was NOT applied:\n{}",
            String::from_utf8_lossy(&test.stderr).trim()
        )));
    }

    // 2) The ordinary path.
    let mut args = vec!["-s".to_string(), "reload".to_string()];
    args.extend(nginx_args(conf, prefix, None));
    let out = crate::platform::command(nginx_bin).args(&args).output()?;
    if out.status.success() {
        return Ok(ReloadOutcome::Reloaded);
    }
    let why = String::from_utf8_lossy(&out.stderr).trim().to_string();

    // 3) `-s reload` only failed because it couldn't FIND the master. If we can
    //    identify ours, signal it ourselves.
    let marker = platform
        .paths()
        .app_data_dir()
        .ok()
        .map(|d| d.display().to_string())
        .filter(|m| !m.is_empty());
    if let Some(pid) = marker.and_then(|m| platform.supervisor().owned_master(port, &m)) {
        // SIGHUP on macOS; `false` where the OS has no reload signal, which falls
        // through to the next step exactly like a failed send.
        if platform.supervisor().signal_reload(pid) {
            // HEAL the pid file. SIGHUP does not make nginx rewrite it, so
            // without this every later reload keeps taking the fallback and the
            // broken state persists invisibly until someone restarts nginx —
            // exactly the trap that made a user's Retry look useless. The file
            // is inside our own prefix and we are restoring the value nginx
            // itself maintains.
            let healed = std::fs::write(prefix.join("nginx.pid"), format!("{pid}\n"));
            log::warn!(
                "nginx: the pid file was unusable ({why}) — signalled our master {pid} \
                 directly and rewrote the pid file (healed: {})",
                healed.is_ok()
            );
            return Ok(ReloadOutcome::Reloaded);
        }
    }

    // Nothing of ours is listening: there is simply nothing to reload.
    if !nginx_running(port) {
        return Ok(ReloadOutcome::NotRunning);
    }
    // Something holds the port but it isn't ours — never signal it.
    Err(Error::Other(format!(
        "nginx could not be reloaded: {why}. Something is listening on port {port} that \
         rexenv doesn't own, so it was left alone — check what is using that port."
    )))
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

    /// Every path in the nginx config is forward-slashed (the Dell measurement on
    /// `nginx_path`). Plant: `display()` for the docroot lets the backslash through.
    #[test]
    fn every_nginx_config_path_is_forward_slashed() {
        assert_eq!(nginx_path(Path::new(r"C:\Users\DELL\AppData\Local\rexenv\rexenv\data\nginx\nginx.pid")),
                   "C:/Users/DELL/AppData/Local/rexenv/rexenv/data/nginx/nginx.pid");
        assert_eq!(nginx_path(Path::new("/Users/me/Library/Application Support/x")), "/Users/me/Library/Application Support/x");
        let back = PathBuf::from(r"C:\sites\new site");
        let cfg = NginxConfig {
            http_port: 18088,
            pid: PathBuf::from(r"C:\data\nginx\nginx.pid"),
            error_log: PathBuf::from(r"C:\data\logs\nginx-error.log"),
            access_log: PathBuf::from(r"C:\data\logs\nginx-access.log"),
            temp_root: PathBuf::from(r"C:\data\nginx\tmp"),
            // A SERVING site too: without one the docroot line is never rendered, and a
            // `display()` there passed this test (the first plant, 14 Sep 2026).
            sites: vec![NginxSite {
                domain: "serving.rex".into(),
                docroot: PathBuf::from(r"C:\sites\serving root"),
                php_fpm_port: 9783,
                rewrite: RewriteMode::Single,
                body_limit: None,
                read_timeout: None,
                php_value: None,
                aliases: vec![],
                storage_root: Some(back.clone()),
                env: vec![],
            }],
            stopped: vec![NginxStopped { domain: "stopped.rex".into(), page_dir: back.clone(), aliases: vec![], wildcard: false }],
        };
        let text = generate_nginx_config(&cfg);
        let quoted_with_backslash: Vec<&str> = text.lines().filter(|l| l.contains('"') && l.contains('\\')).collect();
        assert!(quoted_with_backslash.is_empty(), "a backslash reached a quoted nginx string: {quoted_with_backslash:?}");
        assert!(text.contains("C:/sites/new site"), "{text}");
        assert!(text.contains("root \"C:/sites/serving root\";"), "{text}");
        assert!(storage_block(Some(&back)).contains("alias \"C:/sites/new site/\""));
    }

    #[test]
    fn fpm_config_has_global_and_pool() {
        let cfg = generate_fpm_config(
            9783,
            Path::new("/run/php-fpm-8.3.pid"),
            Path::new("/logs/php-fpm-8.3.log"),
            None,
            &[],
            None,
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
        // No mail routing unless requested — NEITHER half.
        assert!(!cfg.contains("sendmail_path"));
        assert!(!cfg.contains("env[MAIL_"));
    }

    /// **The catch is one fact, so a pool gets both halves or neither.**
    ///
    /// `sendmail_path` catches PHP's own `mail()` — WordPress. `env[MAIL_*]`
    /// catches Laravel, which never reads php.ini for its transport. A config
    /// carrying only the first looks completely correct and delivers every
    /// Laravel site's mail to the real internet, which is the bug measured on
    /// 4 Sep 2026. Asserting both here is what makes them inseparable.
    #[test]
    fn fpm_config_pins_both_halves_of_the_catch_when_given() {
        let catch = super::super::mail::Catch {
            sendmail_path: "'/opt/mailpit' sendmail -t -S 127.0.0.1:11025".to_string(),
            env: super::super::mail::laravel_env(),
        };
        let cfg = generate_fpm_config(
            9783,
            Path::new("/run/php-fpm-8.3.pid"),
            Path::new("/logs/php-fpm-8.3.log"),
            Some(&catch),
            &[],
            None,
        );
        // Routed via php_admin_value (sites can't override it), double-quoted so
        // the ini parser preserves the inner single-quoted binary path.
        assert!(cfg.contains(&format!(
            "php_admin_value[sendmail_path] = \"{}\"",
            catch.sendmail_path
        )));
        // The Laravel half is `env[…]`, not `php_admin_value[…]`: it has to
        // reach the process environment, which is where Dotenv's immutable
        // repository looks and why it beats the site's own `.env`.
        for (k, v) in &catch.env {
            // QUOTED: php-fpm's ini parser reads a bare `null` as the empty
            // string and then refuses the config outright ("empty value"),
            // which takes the pool down — measured 4 Sep 2026 on php-fpm 8.2.
            assert!(cfg.contains(&format!("env[{k}] = \"{v}\"\n")), "missing env[{k}] in:\n{cfg}");
        }
        assert!(
            !cfg.contains("env[MAIL_URL] = null\n"),
            "a bare `null` here is `ERROR: empty value` and the pool never starts"
        );
        assert!(!cfg.contains("php_admin_value[MAIL_"), "an ini value would be invisible to env()");
        // Both sit inside the [www] pool, after the pm.* directives.
        assert!(cfg.find("[www]").unwrap() < cfg.find("sendmail_path").unwrap());
        assert!(cfg.find("[www]").unwrap() < cfg.find("env[MAIL_MAILER]").unwrap());
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
            None,
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
                None,
            );
            assert!(cfg.contains("request_terminate_timeout = 300s"), "val {val}: {cfg}");
        }
    }

    #[test]
    fn fpm_config_pins_the_mysqli_socket_default_and_only_that() {
        // Stage 3 D5: mysqli's compiled default is EMPTY (verified on the
        // cached static binaries), so pointing it at OUR MySQL socket is
        // strictly additive — an imported DB_HOST=localhost WordPress site
        // starts working; nothing rexenv generates uses localhost.
        let cfg = generate_fpm_config(
            9783,
            Path::new("/run/php-fpm-8.3.pid"),
            Path::new("/logs/php-fpm-8.3.log"),
            None,
            &[],
            Some(Path::new("/Users/x/Library/Application Support/dev.rexenv.rexenv/run/mysql.sock")),
        );
        // Locked (admin) and double-quoted: app-data paths contain spaces.
        assert!(cfg.contains(
            "php_admin_value[mysqli.default_socket] = \
             \"/Users/x/Library/Application Support/dev.rexenv.rexenv/run/mysql.sock\""
        ));
        // pdo_mysql.default_socket is deliberately ABSENT: its compiled
        // default is /tmp/mysql.sock — the Homebrew MySQL location — and
        // overriding it could silently redirect an existing PDO site that
        // works against a Homebrew server today. A re-added line flips this.
        assert!(!cfg.contains("pdo_mysql"), "{cfg}");
        // Candidate-style calls (no socket) emit neither.
        let candidate = generate_fpm_config(
            9783,
            Path::new("/p.pid"),
            Path::new("/l.log"),
            None,
            &[],
            None,
        );
        assert!(!candidate.contains("default_socket"));
    }

    #[test]
    fn fpm_running_false_on_closed_port() {
        // An unlikely-to-be-open high port: status should read stopped.
        assert!(!fpm_running(8))
    }

    /// A fake FastCGI server on a loopback port: it reads one record and replies with `reply`
    /// (nothing at all when `None`), holding the connection open for `hold`.
    fn fake_fastcgi(reply: Option<[u8; 8]>, hold: Duration) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut got = vec![0u8; GET_VALUES_RECORD.len()];
            let _ = conn.read_exact(&mut got);
            if let Some(r) = reply {
                let _ = conn.write_all(&r);
            }
            std::thread::sleep(hold);
            got
        });
        (port, server)
    }

    /// The health round trip (ledger #607): a `GET_VALUES_RESULT` is an answer; silence inside the
    /// timeout, a different record type and a closed port are not. The record sent is the one PHP
    /// parses — version 1, type 9, id 0, `FCGI_MPXS_CONNS` in a 17-byte body.
    #[test]
    fn a_pool_answers_only_with_a_get_values_result_inside_the_timeout() {
        let (port, server) = fake_fastcgi(Some([1, 10, 0, 0, 0, 0, 0, 0]), Duration::ZERO);
        assert!(pool_answers(port, Duration::from_secs(2)), "a GET_VALUES_RESULT is an answer");
        let sent = server.join().unwrap();
        assert_eq!(sent, GET_VALUES_RECORD.to_vec());
        assert_eq!((sent[0], sent[1], sent[5]), (1, 9, 17), "version 1, FCGI_GET_VALUES, a 17-byte body");
        assert_eq!(&sent[10..], b"FCGI_MPXS_CONNS");

        // A worker that never frees up: the kernel accepted, nothing answers.
        let (port, server) = fake_fastcgi(None, Duration::from_millis(900));
        let started = std::time::Instant::now();
        assert!(!pool_answers(port, Duration::from_millis(300)), "silence is not an answer");
        assert!(started.elapsed() < Duration::from_millis(800), "the timeout bounds the wait");
        let _ = server.join();

        // Something that is not a GET_VALUES_RESULT (an END_REQUEST).
        let (port, server) = fake_fastcgi(Some([1, 3, 0, 0, 0, 8, 0, 0]), Duration::ZERO);
        assert!(!pool_answers(port, Duration::from_secs(2)), "another record type is not an answer");
        let _ = server.join();

        assert!(!pool_answers(8, Duration::from_millis(300)), "a closed port does not answer");
    }

    fn nginx_cfg(mode: RewriteMode) -> NginxConfig {
        NginxConfig {
            http_port: 18088,
            pid: PathBuf::from("/run/nginx.pid"),
            error_log: PathBuf::from("/logs/nginx-error.log"),
            access_log: PathBuf::from("/logs/nginx-access.log"),
            temp_root: PathBuf::from("/tmp/rexenv-nginx"),
            stopped: Vec::new(),
            sites: vec![NginxSite {
                domain: "acme.test".into(),
                docroot: PathBuf::from("/Sites/acme/public"),
                php_fpm_port: 9783,
                rewrite: mode,
                body_limit: None,
                read_timeout: None,
                php_value: None,
                storage_root: None,
                aliases: Vec::new(),
                env: Vec::new(),
            }],
        }
    }

    /// Valet's `/storage/*` mapping — emitted only for a Laravel project that
    /// has the directory, and REFUSING php and dotfiles inside it.
    ///
    /// The two nested denies are the whole reason this block is not two lines:
    /// `^~` beats every regex location in the vhost, so without them a `.php`
    /// or `.env` under an UPLOADS directory would be served — as source, and
    /// past the dotfile guard that covers the rest of the site.
    /// Extra domains join the SAME server block — one block, every name.
    ///
    /// A second block per alias would compile and serve, and then every future
    /// change to this site would have to be made twice; the first one somebody
    /// forgets is a site whose alias serves the old docroot, the old body limit
    /// or the old PHP pool. And a subdomain network's alias needs the wildcard
    /// too, or `a.alias.test` falls through to nginx's DEFAULT server while
    /// `a.primary.test` works — a half-migrated network is worse than one that
    /// refused the alias.
    #[test]
    fn extra_domains_share_one_server_block() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites[0].aliases = vec!["shop.test".into(), "old.test".into()];
        let out = generate_nginx_config(&cfg);
        // `server_name <names>;` — the DIRECTIVE, not every occurrence of the
        // word (`fastcgi_param SERVER_NAME $server_name` is one too, and
        // counting it made this assertion fail for a reason unrelated to its
        // claim).
        assert_eq!(
            out.matches("\t\tserver_name ").count(),
            1,
            "one site must still be ONE server block: {out}"
        );
        assert!(
            out.contains("server_name acme.test shop.test old.test;"),
            "every name must be on the block: {out}"
        );

        // Subdomain multisite: the primary keeps its wildcard and each alias
        // gets its own.
        let mut ms = nginx_cfg(RewriteMode::SubdomainMultisite);
        ms.sites[0].aliases = vec!["shop.test".into()];
        let out = generate_nginx_config(&ms);
        assert!(
            out.contains("server_name acme.test *.acme.test shop.test *.shop.test;"),
            "a network's alias needs the wildcard, or its sub-sites hit the default server: {out}"
        );
    }

    /// **A stopped site has a block of its OWN, because "no block" means
    /// somebody else's site.**
    ///
    /// This is the defect the owner found the day after the switch shipped, and
    /// it is worth stating plainly because the first design was argued the other
    /// way: with no `server_name` match, nginx answers from its DEFAULT server —
    /// the first block in the file, i.e. another site. Everything that reaches
    /// nginx without passing the edge therefore got a neighbour's content at the
    /// stopped site's address, and a public tunnel is exactly that path
    /// (`cloudflared --http-host-header <domain>` → shared nginx). Sharing a
    /// stopped site published someone else's site to the internet.
    ///
    /// So the block exists, it answers 503 with rexenv's own page on EVERY path,
    /// and it names every hostname the site answers on — an uncovered alias is
    /// the same hole through a different door.
    #[test]
    fn a_stopped_site_answers_for_itself_instead_of_falling_through_to_a_neighbour() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.stopped = vec![NginxStopped {
            domain: "stopped.test".into(),
            page_dir: PathBuf::from("/appdata/config/stopped/site-id"),
            aliases: vec!["old-stopped.test".into()],
            wildcard: false,
        }];
        let out = generate_nginx_config(&cfg);

        assert!(
            out.contains("server_name stopped.test old-stopped.test;"),
            "every name the stopped site answers on must be on its block, or the alias \
             falls through while the primary does not: {out}"
        );
        let block = out
            .split("server_name stopped.test old-stopped.test;")
            .nth(1)
            .and_then(|b| b.split("\n\t}").next())
            .expect("the stopped block");
        assert!(block.contains("return 503;"), "{block}");
        assert!(block.contains("error_page 503 /stopped.html;"), "{block}");
        assert!(
            block.contains("/appdata/config/stopped/site-id"),
            "the block must root at THIS site's own page dir — the page names the site, so a \
             shared directory would show one site's name at another's address: {block}"
        );
        // No PHP anywhere near it: a stopped site must not reach a pool, and a
        // `location ~* \.php$` here would send `/wp-admin/index.php` to one.
        assert!(!block.contains("fastcgi_pass"), "a stopped site must not reach php-fpm: {block}");
        // The serving site is untouched — this is a block ALONGSIDE, never a
        // replacement.
        assert!(out.contains("server_name acme.test;"), "{out}");

        // Subdomain multisite: the wildcard is on the stopped block too, or
        // `a.stopped.test` reaches the default server exactly as before.
        cfg.stopped[0].wildcard = true;
        let out = generate_nginx_config(&cfg);
        assert!(
            out.contains("server_name stopped.test *.stopped.test old-stopped.test *.old-stopped.test;"),
            "{out}"
        );
    }

    #[test]
    fn the_storage_mapping_serves_uploads_and_refuses_code() {
        let plain = generate_nginx_config(&nginx_cfg(RewriteMode::Single));
        assert!(
            !plain.contains("/storage/"),
            "a site with no storage directory must not get the block — an `alias` for a \
             directory that is not there turns every /storage request into a 404 instead of \
             letting the app route it"
        );

        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites[0].storage_root = Some(PathBuf::from("/Sites/acme/storage/app/public"));
        let out = generate_nginx_config(&cfg);
        let block = out
            .split("location ^~ /storage/ {")
            .nth(1)
            .and_then(|b| b.split("\n\t\t}").next())
            .expect("the storage block");
        assert!(
            block.contains("alias \"/Sites/acme/storage/app/public/\";"),
            "the alias must point at the real directory, with the trailing slash `alias` \
             needs to map the prefix: {block}"
        );
        assert!(block.contains("try_files $uri =404;"), "no directory listing / fallthrough");
        assert!(
            block.contains("location ~* \\.php$ { return 404; }"),
            "PHP is not refused inside the storage mapping — `^~` beats the vhost's `.php` \
             location, so an uploaded script would be served as SOURCE: {block}"
        );
        assert!(
            block.contains("location ~ /\\.(?!well-known(/|$)) { return 404; }"),
            "dotfiles are not refused inside the storage mapping — `^~` also beats the \
             vhost's dotfile deny, so a `.env` in an uploads folder would be served: {block}"
        );
        // Ordering: the mapping must come before the `.php` location it is
        // meant to take precedence over — nginx picks the longest prefix and
        // only falls to regex when none matched, so this is about a reader
        // finding them in the order they take effect.
        assert!(
            out.find("location ^~ /storage/").unwrap() < out.find("location ~* \\.php$").unwrap()
        );
    }

    #[test]
    fn nginx_body_limit_is_per_server_and_optional() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites[0].body_limit = Some(64 * 1024 * 1024);
        let out = generate_nginx_config(&cfg);
        // Inside the server block, mirroring the site's PHP upload/post sizes…
        assert!(out.contains("client_max_body_size 67108864;"), "got: {out}");
        // …while the http-level default stays for sites without settings.
        assert!(out.contains(&format!("client_max_body_size {};", crate::core::php::default_body_limit())), "got: {out}");
        assert_eq!(crate::core::php::default_body_limit(), 8u64 << 30, "the default post size is the ceiling");
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
    fn read_timeout_is_per_server_and_covers_both_directions() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites[0].read_timeout = Some(86_400);
        let out = generate_nginx_config(&cfg);
        assert!(out.contains("fastcgi_read_timeout 86400s;"), "got: {out}");
        // Send too: a multi-GB body streams UP to the pool, and nginx's default
        // there is the same 60s.
        assert!(out.contains("fastcgi_send_timeout 86400s;"), "got: {out}");
        assert!(out.find("fastcgi_read_timeout").unwrap() > out.find("location ~* \\.php$").unwrap());
        // None ⇒ nginx's own default, unchanged for every site.
        assert!(!generate_nginx_config(&nginx_cfg(RewriteMode::Single)).contains("timeout"));
    }

    #[test]
    fn php_value_is_one_escaped_param_inside_the_php_location() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites[0].php_value = Some("upload_max_filesize=2048M\npost_max_size=2048M".into());
        let out = generate_nginx_config(&cfg);
        // ONE param, ini lines joined by nginx's `\n` escape — a literal newline
        // inside the quoted string would not survive nginx's config parser.
        assert!(
            out.contains(
                "fastcgi_param PHP_VALUE \"upload_max_filesize=2048M\\npost_max_size=2048M\";"
            ),
            "got: {out}"
        );
        assert_eq!(out.matches("PHP_VALUE").count(), 1);
        assert!(!out.contains("upload_max_filesize=2048M\npost_max_size"), "raw newline emitted");
        // Inside the `.php` location, after the template params.
        let php_loc = out.find("location ~* \\.php$").unwrap();
        assert!(out.find("PHP_VALUE").unwrap() > php_loc);
        assert!(out.find("PHP_VALUE").unwrap() > out.find("fastcgi_param HTTPS").unwrap());
        // None ⇒ nothing emitted; the pool's own settings stand.
        assert!(!generate_nginx_config(&nginx_cfg(RewriteMode::Single)).contains("PHP_VALUE"));
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
                    < out.find("location ~* \\.php$").unwrap(),
                "deny must precede the php location"
            );
        }

        // THE FILE THIS PROTECTS, named. Adminer's docroot stages the real
        // console as a dotfile precisely because this deny exists — so the two
        // are one fact, and asserting the ordering without asserting the name is
        // how they drift apart. Before the move it was `adminer.php`, and
        // `https://adminer.rexenv.rex/adminer.php` served Adminer with NO
        // wrapper: no `login()` override (the loopback gate), no `csp()`, no
        // `headers()` — every control rexenv installs for that console
        // bypassable by dropping `index.php` from the URL.
        let staged = crate::core::adminer::STAGED_ADMINER;
        assert!(
            staged.starts_with('.'),
            "`{staged}` does not start with a dot, so the deny above does not cover it and \
             raw Adminer is served at /{staged}"
        );
    }

    #[test]
    fn rewrite_slots_differ_by_mode() {
        // Single has no WP network rewrites; subdirectory multisite does.
        assert!(!generate_nginx_config(&nginx_cfg(RewriteMode::Single)).contains("rewrite /wp-admin$"));
        assert!(generate_nginx_config(&nginx_cfg(RewriteMode::SubdirectoryMultisite))
            .contains("rewrite /wp-admin$"));
        // Subdomain multisite gets them TOO — inert locally (every sub-site's
        // /wp-admin/ exists on disk, so the `!-e` guard never opens), and
        // load-bearing through a tunnel, where the network is served as
        // subdirectories and /s1/wp-admin/ has no file behind it.
        let sub = generate_nginx_config(&nginx_cfg(RewriteMode::SubdomainMultisite));
        assert!(sub.contains("try_files $uri $uri/ /index.php?$args;"));
        assert!(sub.contains("rewrite /wp-admin$"));
        assert!(sub.contains("if (!-e $request_filename) {"), "the guard, not a bare rewrite: {sub}");
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
            read_timeout: None,
            php_value: None,
            env: Vec::new(),
            storage_root: None,
            aliases: Vec::new(),
        });
        let out = generate_nginx_config(&cfg);
        assert!(out.contains("server_name acme.test;"));
        assert!(out.contains("server_name two.test;"));
        assert_eq!(out.matches("fastcgi_pass").count(), 2);
    }
}
