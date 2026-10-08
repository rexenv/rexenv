//! App-side half of the `rex` CLI: a private unix-socket request server.
//!
//! The CLI is remote-control ONLY. The `cli/` crate never links this lib — it
//! cannot open the app SQLite or spawn services; it writes one JSON request
//! line to the socket and prints the reply. THIS process executes every
//! request through the same `commands::*` fns the UI invokes, so there is one
//! code path, one ServiceManager, one SQLite writer, one stack-guard identity.
//! When the app isn't running there is no socket and the CLI errors — a
//! headless CLI-spawned backend would be exactly the second-writer bug class
//! the stack guard exists for.
//!
//! Security: `0600` socket next to `caddy-admin.sock` (same trust boundary —
//! a same-user process can already drive our SQLite and kill our processes;
//! other users are locked out by fs perms). Never TCP.
//!
//! Protocol: one connection = one request = one exchange, newline-delimited
//! JSON. Request `{"cmd":"status","args":{…}}` → reply `{"ok":true,"data":…}` or
//! `{"ok":false,"error":"…"}`. Long commands hold the connection until done.
//!
//! A client may add `"stream":true`, and then the app writes zero or more
//! `{"progress":…}` lines BEFORE that single envelope, which is always the last
//! line. Opt-in because the client already installed reads exactly one line and
//! treats it as the reply: streaming unasked would hand an older `rex` a
//! progress record as the result of its command.

use crate::commands;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Deserialize;
use serde_json::{json, Value};
// Only the unix-socket `claim`/`bind` take a bare `Path`; on Windows they are gated out
// and the import was a `-D unused-imports` red under clippy (W12).
#[cfg(unix)]
use std::path::Path;
use std::time::Duration;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixListener;

pub const SOCKET_FILE: &str = "rexenv-cli.sock";

/// Cap on one request line. A CLI request is a single JSON command line whose
/// fields are short strings (imports reference file PATHS, never inline blobs),
/// so this is orders of magnitude above any real request — it only bounds a
/// same-user client that streams bytes without a newline (memory). B17.
const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

/// Deadline for the request LINE to arrive. The client builds the JSON in memory
/// and writes it in one `write_all` over a local unix socket (sub-ms), so this
/// never cuts a legitimate request — it bounds a client that connects and never
/// sends (a leaked task). The handler runs AFTER this, untimed, so long commands
/// are unaffected. B17.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// One request line.
#[derive(Debug, Deserialize)]
pub struct Request {
    pub cmd: String,
    #[serde(default)]
    pub args: Value,
    /// Opt-in progress streaming. When true, the server may write
    /// `{"progress": …}` lines BEFORE the single `{"ok": …}` envelope that ends
    /// every request.
    ///
    /// Negotiated rather than always-on, and that is the whole design: an older
    /// `rex` reads exactly one line and treats it as the reply, so a server that
    /// streamed unasked would hand it a progress line as the result. The flag is
    /// the client saying "I know how to read more than one line".
    #[serde(default)]
    pub stream: bool,
}

/// `site.create` args — only `domain` is required; everything else falls back
/// to the New Site dialog's defaults. Enum fields reuse the models' serde
/// forms, so the CLI accepts exactly the values the UI submits.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SiteCreateArgs {
    domain: String,
    name: Option<String>,
    #[serde(rename = "type")]
    site_type: Option<crate::state::models::SiteType>,
    php: Option<String>,
    server: Option<crate::state::models::WebServer>,
    db: Option<crate::state::models::SiteDbEngine>,
    /// Blueprint NAME (resolved to its id against the saved list).
    blueprint: Option<String>,
    /// Convert to multisite right after the install (`subdomain` /
    /// `subdirectory`) — the same convert-after-install flow blueprints use.
    multisite: Option<String>,
    /// Serve an EXISTING folder in place instead of creating one under the
    /// sites folder. Adopted as-is: never written into, never deleted with the
    /// site. Validated by core exactly as the dialog's picker is.
    path: Option<String>,
    /// Blank-PHP only: create the database, seed `starter_items`, write
    /// `db.php`. Core records this field ONLY where the question was asked
    /// (`sites::create`'s `.then_some`), so a caller who asks for it anywhere
    /// else would be silently ignored — which is why the arm refuses instead.
    starter_db: Option<bool>,
}

pub fn parse_request(line: &str) -> Result<Request> {
    serde_json::from_str(line.trim()).map_err(|e| Error::Other(format!("bad request: {e}")))
}

// ── The transport: unix-only ─────────────────────────────────────────────────
// Everything from here to `serve`, plus `spawn` and `hand_off_to_running_instance`,
// is the unix socket, and is `cfg(unix)`. The request model, `handle_request` and
// `dispatch` below are not, so they compile on every OS — the examples drive
// them in-process, and the Windows named pipe (owner ruling D3, port W8) will
// reach the same dispatch. Gating the whole module used to hide that.

/// What taking the socket path found.
#[cfg(unix)]
pub enum Claim {
    /// Ours: bound, `0600`, nobody else was listening.
    Bound(std::os::unix::net::UnixListener),
    /// A live listener answered a connect on the path — another rexenv owns
    /// this app-data directory. The file was NOT touched.
    AnotherIsListening,
}

/// Take the private socket path — the single-instance LOCK (ledger #441).
///
/// A stale file is unlinked first (socket files outlive a crash — same lesson
/// as `admin_alive`: only a connect tells the truth, and the CLI treats
/// connect-refused as "app not running"). A file something is LISTENING on is
/// never unlinked: the first version removed whatever was there and bound over
/// it, so the loser of a launch race — the second copy, seconds behind — stole
/// the winner's socket, and `rex` then talked to a headless orphan while the
/// real app kept adopting services. Perms are locked to `0600` before the
/// first request is served. Plain `std`, no runtime needed: this runs before
/// Tauri boots, so the lock is held for the whole of startup rather than from
/// the end of `setup`.
#[cfg(unix)]
pub fn claim(path: &Path) -> Result<Claim> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Ok(Claim::AnotherIsListening);
        }
        std::fs::remove_file(path)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(Claim::Bound(listener))
}

/// [`claim`] as a tokio listener — for callers already on the runtime (tests,
/// and the late bind when the startup claim could not be made).
#[cfg(unix)]
pub fn bind(path: &Path) -> Result<UnixListener> {
    match claim(path)? {
        Claim::Bound(l) => {
            l.set_nonblocking(true)?;
            Ok(UnixListener::from_std(l)?)
        }
        Claim::AnotherIsListening => Err(Error::Other(format!(
            "another rexenv is listening on {} — this copy must not bind over it",
            path.display()
        ))),
    }
}

/// What `run()` does about the socket before Tauri boots.
#[cfg(unix)]
pub enum StartupClaim {
    /// Continue: the socket is ours (`Some`), or could not be claimed at all
    /// (`None` — `spawn` tries again later, and the app works without its CLI).
    Ours(Option<std::os::unix::net::UnixListener>),
    /// Another instance owns this app-data directory; it has been asked to
    /// show its window, and this process must exit.
    AnotherInstanceRuns,
}

/// Claim the socket at process start, or hand off to the instance that has it.
///
/// The claim happens HERE, not at the end of `setup`, because everything in
/// between — the DNS agent probe (up to 2s), migrations, the CA, three
/// backfills, two process-table sweeps, `adopt_startup` — is seconds during
/// which a second launch used to find no socket, boot fully as a second
/// writer, and then take the socket from the first.
#[cfg(unix)]
pub fn claim_at_startup() -> StartupClaim {
    let Ok(dir) = crate::platform::current().paths().config_dir() else {
        return StartupClaim::Ours(None);
    };
    let path = dir.join(SOCKET_FILE);
    match claim(&path) {
        Ok(Claim::Bound(l)) => StartupClaim::Ours(Some(l)),
        Ok(Claim::AnotherIsListening) => {
            if hand_off_to_running_instance() {
                StartupClaim::AnotherInstanceRuns
            } else {
                // It was listening a moment ago and is not now — it died
                // between the probe and the hand-off. This copy is the app.
                StartupClaim::Ours(None)
            }
        }
        Err(e) => {
            eprintln!("rexenv: could not claim {}: {e} — starting without the lock", path.display());
            StartupClaim::Ours(None)
        }
    }
}

/// Where a long command writes progress. Cloneable and cheap; dropping it is
/// fine, and sending on it when the client did not ask for streaming is a
/// deliberate no-op — see [`Progress`].
#[derive(Clone)]
pub struct Progress(Option<tokio::sync::mpsc::UnboundedSender<String>>);

impl Progress {
    /// A sink that discards — what every non-streaming request gets, so a
    /// command can report progress unconditionally and the protocol decides
    /// whether anyone hears it.
    pub fn none() -> Progress {
        Progress(None)
    }

    /// Send one progress record. Serialised as `{"progress": <value>}` so a
    /// reader can tell it from the terminating `{"ok": …}` envelope by KEY
    /// rather than by counting lines.
    pub fn send(&self, value: Value) {
        if let Some(tx) = &self.0 {
            let _ = tx.send(json!({ "progress": value }).to_string());
        }
    }
}

/// Accept loop: one JSON line in, zero or more `{"progress": …}` lines, then
/// exactly ONE `{"ok": …}` envelope, close. Generic over the handler so framing
/// is testable without an app.
///
/// The envelope is always last and always exactly one, which is what lets a
/// reader that knows nothing about a given command still know when it is done.
#[cfg(unix)]
pub async fn serve<F, Fut>(listener: UnixListener, handler: F)
where
    F: Fn(String, Progress) -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = String> + Send + 'static,
{
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue; // transient accept error; the socket itself stays bound
        };
        let handler = handler.clone();
        tokio::spawn(async move {
            let (read, write) = stream.into_split();
            serve_connection(read, write, handler).await;
        });
    }
}

/// One exchange on a connected client, whatever carries it — the unix socket and Windows' named pipe alike
/// (W8 S2, ledger #631): a request line in, the progress records the client asked for, then exactly one
/// envelope, last. One function for both transports, so the framing, the opt-in streaming and a command
/// outliving its client cannot differ between macOS and Windows.
pub async fn serve_connection<Rd, Wr, F, Fut>(read: Rd, mut write: Wr, handler: F)
where
    Rd: AsyncRead + Unpin,
    Wr: tokio::io::AsyncWrite + Unpin,
    F: FnOnce(String, Progress) -> Fut,
    Fut: std::future::Future<Output = String> + Send + 'static,
{
    let Some(line) = read_request_line(read, MAX_REQUEST_BYTES, REQUEST_READ_TIMEOUT).await else {
        return;
    };
    let streaming = parse_request(&line).map(|r| r.stream).unwrap_or(false);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let progress = if streaming { Progress(Some(tx)) } else { Progress::none() };
    // The handler runs in ITS OWN task, so the command outlives
    // the client: `rex site create … | head -1` used to hang up
    // after the first record, the write error returned from this
    // task, and the handler future was DROPPED mid-command — the
    // site provisioned (a spawned job) but the multisite convert
    // that follows the wait never ran, with no error anywhere.
    // A task also turns a panicking arm into a `JoinError` this
    // loop can answer, keeping "exactly one envelope, always last".
    let mut work = tokio::spawn(handler(line, progress));
    let mut client_gone = false;
    let response = loop {
        tokio::select! {
            // Progress first when both are ready: a record produced
            // before the result must reach the client before the
            // envelope that ends the exchange.
            biased;
            Some(p) = rx.recv() => {
                if !client_gone
                    && (write.write_all(p.as_bytes()).await.is_err()
                        || write.write_all(b"\n").await.is_err())
                {
                    // Keep draining so the handler never blocks on
                    // a full channel; the command runs to its end.
                    client_gone = true;
                }
            }
            done = &mut work => break match done {
                Ok(reply) => reply,
                Err(e) => serde_json::json!({
                    "ok": false,
                    "error": format!("the command crashed inside the app: {e}"),
                })
                .to_string(),
            },
        }
    };
    if client_gone {
        return;
    }
    // Anything queued between the last poll and the handler
    // returning — dropped otherwise, which would lose the final
    // phase of every job that reports one just before finishing.
    while let Ok(p) = rx.try_recv() {
        let _ = write.write_all(p.as_bytes()).await;
        let _ = write.write_all(b"\n").await;
    }
    let _ = write.write_all(response.as_bytes()).await;
    let _ = write.write_all(b"\n").await;
}

/// Read one request line, bounded by `max_bytes` (memory) and `timeout` (a
/// client that connects and never sends). Returns the line, or `None` when it's
/// empty, over-timeout, or a read error — the handler is skipped in every case.
/// Only the read is bounded; the handler runs afterwards untimed (B17).
async fn read_request_line(
    read: impl AsyncRead + Unpin,
    max_bytes: u64,
    timeout: Duration,
) -> Option<String> {
    let mut line = String::new();
    let mut reader = BufReader::new(read.take(max_bytes));
    let fut = reader.read_line(&mut line);
    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(_)) if !line.trim().is_empty() => Some(line),
        _ => None,
    }
}

/// Parse + dispatch one request line, encode the reply envelope.
pub async fn handle_request<R, M>(app: &M, line: String, progress: Progress) -> String
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    let result = match parse_request(&line) {
        Ok(req) => dispatch(app, &req.cmd, req.args, &progress).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(data) => json!({"ok": true, "data": data}).to_string(),
        Err(e) => json!({"ok": false, "error": e.to_string()}).to_string(),
    }
}


/// One progress record from a provisioning job, for a streaming client.
///
/// A SNAPSHOT rather than a delta: the client may join late (it does — the first
/// poll happens after the job starts), and a stream of deltas is only readable
/// by someone who saw the first one. `pct` and the phase are what a terminal
/// line needs; everything else stays in the final envelope.
fn provision_progress(state: &commands::site_provision::SiteProvisionState) -> Value {
    let phase = state
        .phases
        .get(state.phase_cursor)
        .map(|p| json!({ "key": p.key, "label": p.label, "status": p.status }));
    json!({
        "kind": "provision",
        "domain": state.domain,
        "pct": state.pct,
        "status": state.status,
        "phase": phase,
    })
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| Error::Other(format!("encode response: {e}")))
}

/// repo group: `--theme` flips the asset kind (plugin is the default).
fn repo_kind(args: &Value) -> String {
    if args["theme"].as_bool().unwrap_or(false) { "theme".into() } else { "plugin".into() }
}

fn wp_install_jobs_state<R, M>(
    app: &M,
) -> Result<tauri::State<'_, commands::wp_install::WpInstallJobs>>
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    app.try_state::<commands::wp_install::WpInstallJobs>()
        .ok_or_else(|| Error::Other("install job registry not ready".into()))
}

fn repo_jobs_state<R, M>(app: &M) -> Result<tauri::State<'_, commands::repo::RepoJobs>>
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    app.try_state::<commands::repo::RepoJobs>()
        .ok_or_else(|| Error::Other("repo job registry not ready".into()))
}

fn provision_jobs_state<R, M>(
    app: &M,
) -> Result<tauri::State<'_, commands::site_provision::ProvisionJobs>>
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    app.try_state::<commands::site_provision::ProvisionJobs>()
        .ok_or_else(|| Error::Other("provision job registry not ready".into()))
}

// The settle rule lives beside `RepoJobState` in `commands::repo`, not here: the
// MCP server applies the same rule, and it moved out while this whole module was
// still `cfg(unix)` (docs/PLAN-windows-port.md W1).
use crate::commands::repo::repo_job_settled;

/// Poll a job until settled (no cap — same philosophy as the UI: builds run
/// long, the connection is held, a Ctrl-C'd client just abandons the reply
/// while the job finishes independently).
async fn repo_wait_settled<R: tauri::Runtime, M: Manager<R>>(
    app: &M,
    job_id: &str,
    waiting_for: Option<&str>,
) -> Result<commands::repo::RepoJobState> {
    loop {
        let jobs = repo_jobs_state(app)?;
        let st = commands::repo::repo_job_state(jobs, job_id.to_string()).await?;
        if repo_job_settled(&st, waiting_for) {
            return Ok(st);
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

/// Run the job's OFFERED install steps (composer → install → build, in the
/// job's own order) one at a time; stop at the first failure. The explicit
/// `--install` consent arrived on the command line. Thin delegate — the loop
/// was promoted to `commands::repo::run_offered_steps` (shared with the
/// panel's "Run all"), which also holds the job's step_running flag for the
/// whole sequence and marks never-ran steps "skipped".
async fn repo_run_offered<R: tauri::Runtime, M: Manager<R>>(
    _app: &M,
    handle: tauri::AppHandle<R>,
    job_id: &str,
) -> Result<commands::repo::RepoJobState> {
    commands::repo::run_offered_steps(handle, job_id.to_string()).await
}

/// Settled-job reply: final snapshot + the job's flat log (the socket can't
/// stream events — output arrives at completion, stated honestly by the CLI).
fn repo_job_reply<R, M>(app: &M, st: &commands::repo::RepoJobState) -> Result<Value>
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    let state = app_state(app)?;
    let log = crate::core::logs::tail(state.platform.as_ref(), &st.log_key, 200)
        .unwrap_or_default();
    Ok(json!({ "job": to_value(st)?, "log": log }))
}

fn repo_watches_state<R, M>(app: &M) -> Result<tauri::State<'_, commands::repo::RepoWatches>>
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    app.try_state::<commands::repo::RepoWatches>()
        .ok_or_else(|| Error::Other("watch registry not ready".into()))
}

fn need_str(args: &Value, key: &str, cmd: &str) -> Result<String> {
    args[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::Other(format!("{cmd} needs `{key}`")))
}

fn need_names(args: &Value, cmd: &str) -> Result<Vec<String>> {
    let names: Vec<String> = args["names"]
        .as_array()
        .map(|v| v.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if names.is_empty() {
        return Err(Error::Other(format!("{cmd} needs `names`")));
    }
    Ok(names)
}

/// AppState is managed only after a successful DB/CA init — mirror the UI's
/// InitError screen with a plain error instead of a state panic. Fetched per
/// command arm so an unknown cmd reports as unknown even before init.
fn app_state<R, M>(app: &M) -> Result<tauri::State<'_, AppState>>
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    app.try_state::<AppState>().ok_or_else(|| {
        Error::Other(
            "rexenv is still starting (or failed to initialize) — check the app window".into(),
        )
    })
}

/// Route a command to the SAME `commands::*` fn the UI calls — never a
/// parallel implementation.
async fn dispatch<R, M>(app: &M, cmd: &str, args: Value, progress: &Progress) -> Result<Value>
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    match cmd {
        "status" => {
            let state = app_state(app)?;
            let services = commands::services::services_status(state.clone()).await?;
            let dns = app
                .try_state::<crate::state::app::DnsState>()
                .map(|dns| commands::system::dns_status(state.clone(), dns));
            // The app-update offer, as a READ. No new dispatch arm and no `rex
            // update` verb: install stays a GUI click, because a self-update
            // replaces the process that enforces the agent dial and
            // `settings_access`, and the relaunch kills this very socket
            // mid-call — the caller could never observe the result of what it
            // asked for. Read from the in-process snapshot, like the tray.
            let update = crate::core::app_update::current_offer().map(|o| {
                json!({ "version": o.version, "sizeBytes": o.size_bytes })
            });
            Ok(json!({
                "services": to_value(&services)?,
                "dns": to_value(&dns)?,
                "update": update,
            }))
        }
        // Global lifecycle — exactly the footer buttons. `rex restart` is the
        // CLI sending `stop` then `start`; no third code path exists.
        "start" => {
            commands::services::start_services(app_state(app)?).await?;
            Ok(Value::Null)
        }
        "stop" => {
            commands::services::stop_services(app_state(app)?, provision_jobs_state(app)?).await?;
            Ok(Value::Null)
        }
        // Sites — the Sites screen's data, merged client-side for display.
        // Settings, through `core::settings_access`'s ruling — never the raw
        // key/value door. The policy lives in core because the guard in
        // `core::sites` reads the same list: a boundary with two copies is the
        // defect this tree keeps finding.
        "config.get" => {
            let state = app_state(app)?;
            let key = need_str(&args, "key", cmd)?;
            match crate::core::settings_access::cli_access(&key) {
                crate::core::settings_access::CliAccess::Denied(why) => {
                    Err(Error::Other(format!("`{key}` cannot be read from the CLI: {why}")))
                }
                _ => {
                    // THROUGH the app's own command, like `config.set` below —
                    // one read path for the UI and the shell, so a key that
                    // later grows a derived or redacted read cannot answer two
                    // different things depending on who asked.
                    let value = commands::settings::get_setting(state.clone(), key.clone())?;
                    Ok(json!({ "key": key, "value": value }))
                }
            }
        }
        "config.set" => {
            let state = app_state(app)?;
            let key = need_str(&args, "key", cmd)?;
            let value = need_str(&args, "value", cmd)?;
            match crate::core::settings_access::cli_access(&key) {
                crate::core::settings_access::CliAccess::ReadWrite => {
                    // THROUGH the app's own command, so a key with a validating
                    // setter still gets it. Re-implementing the write here would
                    // be the way round the validation this guards.
                    commands::settings::set_setting(state.clone(), key.clone(), value.clone())?;
                    Ok(json!({ "key": key, "value": value }))
                }
                crate::core::settings_access::CliAccess::ReadOnly(why) => {
                    Err(Error::Other(format!("`{key}` is read-only from the CLI: {why}")))
                }
                crate::core::settings_access::CliAccess::Denied(why) => {
                    Err(Error::Other(format!("`{key}` cannot be set from the CLI: {why}")))
                }
            }
        }
        "site.list" => {
            let state = app_state(app)?;
            let sites = commands::sites::list_sites(state.clone())?;
            let serving = commands::sites::sites_serving(state.clone())?;
            // Extra domains ride along (v42), keyed by site id. `rex` resolves a
            // site from whatever hostname the user typed, and after extra
            // domains shipped that stopped being the same thing as the site's
            // own domain — `rex site info shop.rex` answered "no site with
            // domain shop.rex" about a site that answers on it.
            let aliases = {
                let conn = state
                    .db
                    .lock()
                    .map_err(|_| Error::Other("database lock poisoned".into()))?;
                crate::state::store::all_site_aliases(&conn)?
            };
            Ok(json!({
                "sites": to_value(&sites)?,
                "serving": to_value(&serving)?,
                "aliases": to_value(&aliases)?,
            }))
        }
        // Site create: exactly what the New Site dialog submits — `path` empty
        // (the backend derives it under the sites folder) or a folder to LINK,
        // WordPress
        // install options falling back to their site-derived defaults, and the
        // dialog's own default choices for anything the flag set omits. All
        // validation stays where it lives (validate_domain, core checks).
        "site.create" => {
            let state = app_state(app)?;
            let a: SiteCreateArgs = serde_json::from_value(args)
                .map_err(|e| Error::Other(format!("bad site.create args: {e}")))?;
            if a.domain.is_empty() {
                return Err(Error::Other("site.create needs a domain".into()));
            }
            // The rule lives in core, next to the `.then_some` that would
            // otherwise drop this field without a word (`sites::create`).
            if a.starter_db == Some(true) {
                if let Some(why) = crate::core::sites::starter_db_refusal(
                    a.site_type.unwrap_or(crate::state::models::SiteType::Wordpress),
                    a.path.as_deref().unwrap_or_default(),
                    false, // `rex site create` has no repo flag yet
                ) {
                    return Err(Error::Other(why.into()));
                }
            }
            let php_version = match a.php {
                Some(v) => v,
                // The dialog preselects the registry's default minor.
                None => {
                    let conn = state
                        .db
                        .lock()
                        .map_err(|_| Error::Other("database lock poisoned".into()))?;
                    crate::core::php::default_minor(&conn)?
                        .ok_or_else(|| Error::Other("no default PHP version".into()))?
                }
            };
            let site = crate::state::models::NewSite {
                // No `--name`: the domain without its TLD, the dialog's pairing read
                // backwards — not the whole domain repeated as the name (#753).
                name: a
                    .name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| crate::core::sites::name_from_domain(&a.domain)),
                domain: a.domain,
                site_type: a.site_type.unwrap_or(crate::state::models::SiteType::Wordpress),
                php_version,
                web_server: a.server.unwrap_or(crate::state::models::WebServer::Nginx),
                // Empty = create the docroot under the sites folder (the
                // dialog's default); a value = link that existing folder.
                path: a.path.clone().unwrap_or_default(),
                db_engine: a.db.unwrap_or(crate::state::models::SiteDbEngine::Mysql),
                // `rex site create` has no repo flag yet — the app is the only
                // caller that can clone (docs/archive/PLAN-git-site-clone.md, CLI in a
                // later stage).
                git_url: String::new(),
                git_ref: None,
                git_migrate: true,
                git_build_assets: false,
                starter_db: a.starter_db.unwrap_or(false),
            };
            let blueprint_id = match &a.blueprint {
                None => None,
                Some(name) => {
                    let all = commands::blueprints::list_blueprints(state.clone())?;
                    let found = all.iter().find(|b| &b.name == name).map(|b| b.id.clone());
                    Some(found.ok_or_else(|| {
                        Error::Other(format!(
                            "no blueprint named `{name}` (saved: {})",
                            all.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(", ")
                        ))
                    })?)
                }
            };
            let is_wp = matches!(
                a.site_type.unwrap_or(crate::state::models::SiteType::Wordpress),
                crate::state::models::SiteType::Wordpress
            );
            let multisite = a.multisite.clone();
            if multisite.is_some() && !is_wp {
                return Err(Error::Other("--multisite needs a WordPress site".into()));
            }
            // Progress while it runs, for a client that asked for it. Polled
            // beside the call rather than plumbed through it: the provisioning
            // job already publishes its own state for the app's card, so the
            // CLI reads the SAME source the UI does instead of inventing a
            // second progress channel that can disagree with the first.
            let domain_for_progress = site.domain.clone();
            let created = {
                let fut = commands::sites::create_site(
                    app.app_handle().clone(),
                    state.clone(),
                    provision_jobs_state(app)?,
                    site,
                    None,
                    blueprint_id,
                );
                tokio::pin!(fut);
                let mut sent_phase = usize::MAX;
                loop {
                    tokio::select! {
                        biased;
                        done = &mut fut => break done?,
                        _ = tokio::time::sleep(std::time::Duration::from_millis(400)) => {
                            if let Ok(Some(snap)) = commands::site_provision::site_provision_active(
                                provision_jobs_state(app)?,
                                Some(domain_for_progress.clone()),
                            )
                            .await
                            {
                                if snap.phase_cursor != sent_phase {
                                    sent_phase = snap.phase_cursor;
                                    progress.send(provision_progress(&snap));
                                }
                            }
                        }
                    }
                }
            };
            // Convert-after-install — the blueprint flow's seam, reused.
            let created = match multisite {
                Some(mode) => commands::wordpress::wp_multisite_convert(
                    state.clone(),
                    app.try_state::<commands::tunnels::Tunnels>()
                        .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?,
                    created.id.clone(),
                    mode,
                )
                .await?
                .unwrap_or(created),
                None => created,
            };
            to_value(&created)
        }
        // Site detail: the SiteDetail overview's data, merged. Resources, cert
        // and WP info are best-effort — a stopped stack or a fresh site must
        // degrade fields to null, never fail the whole view.
        "site.info" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("site.info needs an id".into()))?
                .to_string();
            let site = commands::sites::list_sites(state.clone())?
                .into_iter()
                .find(|s| s.id == id)
                .ok_or_else(|| Error::Other(format!("no site {id}")))?;
            // The MCP's classification (#200) — the wire asked, setup-incomplete
            // and stopped-by-you kept distinct — not the manager's belief, which
            // said `serving` about a site the edge refused (23 Sep 2026). `serving`
            // is that verdict's boolean, so the two fields cannot disagree.
            let status = {
                let ctx = crate::mcp_server::ReadCtx::new(state.inner());
                let signals = ctx.probe_serving(&site).await;
                crate::mcp_server::AgentSiteStatus::from_signals(&site, &signals)
            };
            let serving = status.serving;
            let resources = commands::sites::sites_resources(state.clone())
                .await
                .ok()
                .and_then(|v| v.into_iter().find(|r| r.id == id));
            let cert = commands::sites::site_cert_info(state.clone(), id.clone()).ok().flatten();
            let wp = if matches!(site.site_type, crate::state::models::SiteType::Wordpress) {
                commands::wordpress::wp_info(state.clone(), id.clone()).await.ok()
            } else {
                None
            };
            // The site's EXTRA domains (v42) — `site info` is the "tell me
            // everything about this site" verb, and a site that answers on
            // three names showing one is the same half-answer the Sites list
            // had until this morning.
            let domains = commands::sites::site_domains(state.clone(), id.clone()).await?;
            Ok(json!({
                "site": to_value(&site)?,
                "serving": serving,
                "status": to_value(&status)?,
                "resources": to_value(&resources)?,
                "cert": to_value(&cert)?,
                "wp": to_value(&wp)?,
                "domains": to_value(&domains)?,
            }))
        }
        // Magic wp-admin login link — the Sites row action.
        "site.login" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("site.login needs an id".into()))?
                .to_string();
            let url = commands::wordpress::wp_admin_login_url(state.clone(), id, None).await?;
            Ok(json!({ "url": url }))
        }
        // Site delete: the CLI resolves domain → id via site.list first; this
        // arm is by-id like the UI row action (tunnel stop + DB drop + files).
        // Finish a half-provisioned site (`provisioned = 0`) — the recovery the
        // app offers as the "setup incomplete" badge's Retry, which the CLI had
        // no equivalent of.
        //
        // `docs/CLI-ROADMAP.md` says `site.create`'s failure "points here". That
        // is the ROADMAP's claim and it is NOT verified: grepping the tree finds
        // no message naming this command, and reproducing a mid-provision
        // failure to read the text was not done. Recorded rather than repeated —
        // writing the claim into a code comment would have laundered somebody
        // else's untested sentence into a fact.
        //
        // Polls SERVER-side rather than handing the CLI a job id: `site.create`
        // already blocks for the whole provision, `request()` has no read
        // timeout for exactly that reason, and a second protocol for the same
        // user-visible operation would be two things to keep in step.
        "site.retry" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let started = commands::site_provision::site_provision_retry(
                app.app_handle().clone(),
                state.clone(),
                provision_jobs_state(app)?,
                id,
            )
            .await?;
            let domain = started.domain.clone();
            // The job runs in a spawned task; wait for it to settle. Bounded
            // because a wedged provision must not hold the socket forever — the
            // caller gets the last snapshot and can read the log it names.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
            let mut last = started;
            progress.send(provision_progress(&last));
            let mut sent_phase = last.phase_cursor;
            while last.status == "running" && std::time::Instant::now() < deadline {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                match commands::site_provision::site_provision_active(
                    provision_jobs_state(app)?,
                    Some(domain.clone()),
                )
                .await?
                {
                    Some(snap) => {
                        // One record per PHASE, not per poll: at 400ms a
                        // fifteen-minute provision is 2 250 lines of the same
                        // sentence, which is noise a reader has to filter to
                        // find the one thing that changed.
                        if snap.phase_cursor != sent_phase || snap.status != last.status {
                            sent_phase = snap.phase_cursor;
                            progress.send(provision_progress(&snap));
                        }
                        last = snap;
                    }
                    None => break,
                }
            }
            to_value(&last)
        }
        "site.delete" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("site.delete needs an id".into()))?
                .to_string();
            let tunnels = app
                .try_state::<commands::tunnels::Tunnels>()
                .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?;
            let deleted = commands::sites::delete_site(state.clone(), tunnels, id).await?;
            Ok(json!({ "deleted": deleted }))
        }
        // Site settings — the SiteDetail Settings-card actions.
        "site.rename" => {
            let state = app_state(app)?;
            let site = commands::sites::rename_site(
                state.clone(),
                need_str(&args, "id", cmd)?,
                need_str(&args, "name", cmd)?,
            )?;
            to_value(&site)
        }
        "site.domain" => {
            let state = app_state(app)?;
            let tunnels = app
                .try_state::<commands::tunnels::Tunnels>()
                .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?;
            let change = commands::sites::change_site_domain(
                state.clone(),
                tunnels,
                need_str(&args, "id", cmd)?,
                need_str(&args, "domain", cmd)?,
            )
            .await?;
            to_value(&change)
        }
        "site.move" => {
            let state = app_state(app)?;
            let site = commands::sites::move_site_docroot(
                state.clone(),
                app.try_state::<commands::tunnels::Tunnels>()
                    .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?,
                need_str(&args, "id", cmd)?,
                need_str(&args, "destParent", cmd)?,
            )
            .await?;
            to_value(&site)
        }
        // Re-point a LINKED/imported site at a folder the user moved themselves.
        // Distinct from `site.move`, which relocates a docroot rexenv owns:
        // this one records the new location and reloads, and touches no file.
        "site.relink" => {
            let state = app_state(app)?;
            let site = commands::sites::relink_site_docroot(
                state.clone(),
                app.try_state::<commands::tunnels::Tunnels>()
                    .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?,
                need_str(&args, "id", cmd)?,
                need_str(&args, "path", cmd)?,
            )
            .await?;
            to_value(&site)
        }
        "site.env" => {
            let state = app_state(app)?;
            let vars = commands::sites::list_site_env(state.clone(), need_str(&args, "id", cmd)?)?;
            Ok(json!({ "vars": to_value(&vars)? }))
        }
        // Replaces the WHOLE set (the UI's model) — the CLI merges client-side.
        "site.env.set" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let vars: Vec<commands::sites::EnvVarInput> =
                serde_json::from_value(args["vars"].clone())
                    .map_err(|e| Error::Other(format!("bad site.env.set vars: {e}")))?;
            commands::sites::set_site_env(state.clone(), id, vars).await?;
            Ok(Value::Null)
        }
        // Extra domains (v42): add/remove/list the hostnames a site answers on.
        // One arm, because they are one operation from the user's side — "which
        // names does this site have" — and the reply is always the full list.
        "site.domains" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let domains = match (args["add"].as_str(), args["remove"].as_str()) {
                (Some(d), None) => {
                    commands::sites::add_site_domain(state.clone(), id, d.to_string()).await?
                }
                (None, Some(d)) => {
                    commands::sites::remove_site_domain(state.clone(), id, d.to_string()).await?
                }
                (None, None) => commands::sites::site_domains(state.clone(), id).await?,
                (Some(_), Some(_)) => {
                    return Err(Error::Other(
                        "site.domains takes `add` or `remove`, not both".into(),
                    ))
                }
            };
            Ok(json!({ "domains": domains }))
        }
        "site.cert" => {
            let state = app_state(app)?;
            let cert =
                commands::sites::site_cert_info(state.clone(), need_str(&args, "id", cmd)?)?;
            to_value(&cert)
        }
        "site.cert.regenerate" => {
            let state = app_state(app)?;
            commands::sites::regenerate_site_cert(state.clone(), need_str(&args, "id", cmd)?)
                .await?;
            Ok(Value::Null)
        }
        "site.server" => {
            let state = app_state(app)?;
            let server: crate::state::models::WebServer =
                serde_json::from_value(args["server"].clone())
                    .map_err(|e| Error::Other(format!("bad server: {e}")))?;
            let site = commands::sites::set_site_web_server(
                state.clone(),
                app.try_state::<commands::tunnels::Tunnels>()
                    .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?,
                need_str(&args, "id", cmd)?,
                server,
            )
            .await?;
            to_value(&site)
        }
        // Single-site restart. What it MEANS differs per site and the reply
        // says which: a site with its own backend gets that process bounced; a
        // default (shared-nginx) site gets its config rebuilt and the web tier
        // reloaded, because there is no per-site process to restart and killing
        // the shared pool would stop every other site on that PHP minor. The
        // pool bounce is therefore opt-in (`--pool`), and the reply carries how
        // many sites it covers either way.
        "site.restart" => {
            let state = app_state(app)?;
            let report = commands::sites::restart_site(
                state.clone(),
                need_str(&args, "id", cmd)?,
                args.get("pool").and_then(Value::as_bool).unwrap_or(false),
            )
            .await?;
            to_value(&report)
        }
        // Start or stop ONE site (v44). The CLI's verb for the switch the Sites
        // list flips: `rex site stop shop.rex` takes that site off the serving
        // surface — no server block, a 503 at its address — while the shared web
        // server and PHP pools keep serving every other site. The reply carries
        // whether it is actually answering, so a `start` while the stack is down
        // prints the reason instead of a cheerful lie.
        "site.enabled" => {
            let state = app_state(app)?;
            let report = commands::sites::set_site_enabled(
                state.clone(),
                need_str(&args, "id", cmd)?,
                args.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            )
            .await?;
            to_value(&report)
        }
        // Every site at once — the Sites page's bulk action, not `rex stop`.
        // The services stay up; only the sites' serving surface changes.
        "sites.enabled" => {
            let state = app_state(app)?;
            let report = commands::sites::set_all_sites_enabled(
                state.clone(),
                args.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            )
            .await?;
            to_value(&report)
        }
        "blueprint.list" => {
            let state = app_state(app)?;
            Ok(json!({ "blueprints": to_value(&commands::blueprints::list_blueprints(state.clone())?)? }))
        }
        // Logs. `logs.targets` = the Logs tab's curated per-site sources (plus
        // the WP debug.log entry the tab exposes separately); `logs.tail` = any
        // key within the log dir; `logs.list` = every service log file (the
        // log dir listing — no site scope). Follow mode is client-side polling
        // of the same tail, exactly like the Logs tab.
        "logs.targets" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("logs.targets needs an id".into()))?
                .to_string();
            let mut targets =
                to_value(&commands::logs::log_targets(state.clone(), id.clone())?)?;
            let is_wp = commands::sites::list_sites(state.clone())?
                .into_iter()
                .any(|s| s.id == id && matches!(s.site_type, crate::state::models::SiteType::Wordpress));
            if let (Some(list), true) = (targets.as_array_mut(), is_wp) {
                list.push(json!({ "key": "wp-debug", "label": "WordPress debug.log" }));
            }
            Ok(json!({ "targets": targets }))
        }
        "logs.tail" => {
            let state = app_state(app)?;
            let key = args["key"]
                .as_str()
                .ok_or_else(|| Error::Other("logs.tail needs a key".into()))?
                .to_string();
            let lines = args["lines"].as_u64().unwrap_or(100) as usize;
            // The per-site WP debug.log lives in the DOCROOT, not the log dir —
            // routed by pseudo-key + site id (same split as the Logs tab).
            let lines = if key == "wp-debug" {
                let id = args["id"]
                    .as_str()
                    .ok_or_else(|| Error::Other("wp-debug tail needs an id".into()))?
                    .to_string();
                commands::logs::wp_debug_log_tail(state.clone(), id, lines)?
            } else {
                commands::logs::tail_log(state.clone(), key, lines)?
            };
            Ok(json!({ "lines": lines }))
        }
        "logs.list" => {
            let state = app_state(app)?;
            let files: Vec<Value> = crate::core::logs::list_files(state.platform.as_ref())?
                .into_iter()
                .map(|(key, bytes)| json!({ "key": key, "bytes": bytes }))
                .collect();
            Ok(json!({ "files": files }))
        }
        // PHP versions — the Settings PHP card + SiteDetail switches. All
        // rules live backend-side (uninstall refused while sites use the
        // minor; xdebug refused on 8.0/FrankenPHP; live pools restarted).
        "php.list" => {
            let state = app_state(app)?;
            Ok(json!({ "versions": to_value(&commands::php::list_php_versions(state.clone())?)? }))
        }
        "php.default" => {
            let state = app_state(app)?;
            let minor = args["minor"]
                .as_str()
                .ok_or_else(|| Error::Other("php.default needs a minor".into()))?
                .to_string();
            commands::php::set_default_php_version(state.clone(), minor)?;
            Ok(Value::Null)
        }
        "php.installed" => {
            let state = app_state(app)?;
            let minor = args["minor"]
                .as_str()
                .ok_or_else(|| Error::Other("php.installed needs a minor".into()))?
                .to_string();
            let installed = args["installed"]
                .as_bool()
                .ok_or_else(|| Error::Other("php.installed needs installed: bool".into()))?;
            commands::php::set_php_version_installed(state.clone(), minor, installed).await?;
            Ok(Value::Null)
        }
        "site.php" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("site.php needs an id".into()))?
                .to_string();
            let version = args["version"]
                .as_str()
                .ok_or_else(|| Error::Other("site.php needs a version".into()))?
                .to_string();
            let site = commands::sites::set_site_php_version(state.clone(), id, version).await?;
            to_value(&site)
        }
        "site.xdebug" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("site.xdebug needs an id".into()))?
                .to_string();
            let enabled = args["enabled"]
                .as_bool()
                .ok_or_else(|| Error::Other("site.xdebug needs enabled: bool".into()))?;
            let site = commands::sites::set_site_xdebug(state.clone(), id, enabled).await?;
            to_value(&site)
        }
        // WordPress manager — plugins/themes/users, the WP Manager tab's fns.
        // Every op runs vetted WP-CLI backend-side; nothing here is a raw
        // passthrough. Multi-name ops take {names: [..]}.
        "wp.plugins" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            // check_updates: one synchronous wp-org pass so the CLI list shows
            // update badges (the UI does the fast list + a background refresh).
            let plugins =
                commands::wordpress::wp_plugins(state.clone(), id, Some(true)).await?;
            Ok(json!({ "plugins": to_value(&plugins)? }))
        }
        // One execution path with the panel: the STREAMED install job (the
        // socket can't stream — held connection, reply at completion, the
        // repo.op precedent). Observable behavior unchanged: success replies
        // exactly `null`; failure replies ok:false + the verbatim wp-cli
        // Warning:/Error: lines (rex exits 1).
        "wp.plugin.install" | "wp.theme.install" => {
            let state = app_state(app)?;
            let repo_jobs = repo_jobs_state(app)?;
            let wjobs = wp_install_jobs_state(app)?;
            let (id, slug) = (need_str(&args, "id", cmd)?, need_str(&args, "slug", cmd)?);
            let activate = args["activate"].as_bool().unwrap_or(false);
            let kind = if cmd == "wp.plugin.install" { "plugin" } else { "theme" };
            let snap = commands::wp_install::wp_install_job(
                app.app_handle().clone(),
                state,
                repo_jobs,
                wjobs,
                id,
                kind.into(),
                // The CLI takes wp.org slugs only — `rex wp plugin install`
                // has no zip form (the zip source is a picked-file flow).
                "wporg".into(),
                vec![slug],
                activate,
                // No `--force` from the CLI: `rex wp plugin install` takes
                // wp.org slugs, where an existing install is an UPDATE
                // question, not an overwrite one.
                false,
            )
            .await?;
            let st = loop {
                let wjobs = wp_install_jobs_state(app)?;
                let st = commands::wp_install::state_of(&wjobs, &snap.id)?;
                if st.status != "running" {
                    break st;
                }
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            };
            if st.status == "ok" {
                Ok(Value::Null)
            } else {
                Err(Error::Other(format!(
                    "wp {kind} install failed: {}",
                    st.error.or(st.summary).unwrap_or_else(|| st.status.clone())
                )))
            }
        }
        "wp.plugin.activate" | "wp.plugin.deactivate" | "wp.plugin.update" | "wp.plugin.delete" => {
            let state = app_state(app)?;
            let (id, names) = (need_str(&args, "id", cmd)?, need_names(&args, cmd)?);
            match cmd {
                "wp.plugin.activate" => {
                    commands::wordpress::wp_plugin_activate(state.clone(), id, names).await?
                }
                "wp.plugin.deactivate" => {
                    commands::wordpress::wp_plugin_deactivate(state.clone(), id, names).await?
                }
                "wp.plugin.update" => {
                    commands::wordpress::wp_plugin_update(app.app_handle().clone(), state.clone(), id, names)
                        .await?
                }
                _ => commands::wordpress::wp_plugin_delete(state.clone(), id, names).await?,
            }
            Ok(Value::Null)
        }
        "wp.themes" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let themes = commands::wordpress::wp_themes(state.clone(), id, Some(true)).await?;
            Ok(json!({ "themes": to_value(&themes)? }))
        }
        "wp.theme.activate" => {
            let state = app_state(app)?;
            let (id, name) = (need_str(&args, "id", cmd)?, need_str(&args, "name", cmd)?);
            commands::wordpress::wp_theme_activate(state.clone(), id, name).await?;
            Ok(Value::Null)
        }
        "wp.theme.update" | "wp.theme.delete" => {
            let state = app_state(app)?;
            let (id, names) = (need_str(&args, "id", cmd)?, need_names(&args, cmd)?);
            if cmd == "wp.theme.update" {
                commands::wordpress::wp_theme_update(app.app_handle().clone(), state.clone(), id, names).await?;
            } else {
                commands::wordpress::wp_theme_delete(state.clone(), id, names).await?;
            }
            Ok(Value::Null)
        }
        "wp.users" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            Ok(json!({ "users": to_value(&commands::wordpress::wp_users(state.clone(), id).await?)? }))
        }
        "wp.user.create" => {
            let state = app_state(app)?;
            commands::wordpress::wp_user_create(
                state.clone(),
                need_str(&args, "id", cmd)?,
                need_str(&args, "login", cmd)?,
                need_str(&args, "email", cmd)?,
                need_str(&args, "role", cmd)?,
                need_str(&args, "password", cmd)?,
            )
            .await?;
            Ok(Value::Null)
        }
        "wp.user.delete" => {
            let state = app_state(app)?;
            commands::wordpress::wp_user_delete(
                state.clone(),
                need_str(&args, "id", cmd)?,
                args["userId"]
                    .as_u64()
                    .ok_or_else(|| Error::Other("wp.user.delete needs `userId`".into()))?,
                args["reassign"].as_u64(),
                args["deletePosts"].as_bool().unwrap_or(false),
            )
            .await?;
            Ok(Value::Null)
        }
        "wp.user.password" | "wp.user.role" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let user_id = args["userId"]
                .as_u64()
                .ok_or_else(|| Error::Other(format!("{cmd} needs `userId`")))?;
            if cmd == "wp.user.password" {
                commands::wordpress::wp_user_set_password(
                    state.clone(),
                    id,
                    user_id,
                    need_str(&args, "password", cmd)?,
                )
                .await?;
            } else {
                commands::wordpress::wp_user_set_role(
                    state.clone(),
                    id,
                    user_id,
                    need_str(&args, "role", cmd)?,
                )
                .await?;
            }
            Ok(Value::Null)
        }
        // WP singles — Tools-tab one-shots. search-replace carries the
        // backend's dry_run flag; maintenance get/set share one arm.
        "wp.search-replace" => {
            let state = app_state(app)?;
            let count = commands::wordpress::wp_search_replace(
                state.clone(),
                need_str(&args, "id", cmd)?,
                need_str(&args, "from", cmd)?,
                need_str(&args, "to", cmd)?,
                args["dryRun"].as_bool().unwrap_or(false),
            )
            .await?;
            Ok(json!({ "replacements": count }))
        }
        "wp.cache-flush" => {
            let state = app_state(app)?;
            let msg =
                commands::wordpress::wp_cache_flush(state.clone(), need_str(&args, "id", cmd)?)
                    .await?;
            Ok(json!({ "message": msg }))
        }
        "wp.cron-run" => {
            let state = app_state(app)?;
            let msg =
                commands::wordpress::wp_cron_run_due(state.clone(), need_str(&args, "id", cmd)?)
                    .await?;
            Ok(json!({ "message": msg }))
        }
        "wp.maintenance" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            match args["on"].as_bool() {
                Some(on) => {
                    commands::wordpress::wp_maintenance_set(state.clone(), id, on).await?;
                    Ok(json!({ "on": on }))
                }
                None => Ok(json!({
                    "on": commands::wordpress::wp_maintenance_get(state.clone(), id).await?
                })),
            }
        }
        "wp.core-update" => {
            let state = app_state(app)?;
            let msg =
                commands::wordpress::wp_core_update(app.app_handle().clone(), state.clone(), need_str(&args, "id", cmd)?)
                    .await?;
            Ok(json!({ "message": msg }))
        }
        // Database export/import — the SiteDetail Tools actions. Export writes
        // `<domain>-db.sql` into ~/Downloads (numbered on collision) and
        // returns the path; import is DESTRUCTIVE and .sql/engine-gated
        // backend-side. The CLI confirms import (--yes) like the UI's typed
        // confirm.
        "db.export" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("db.export needs an id".into()))?
                .to_string();
            let path = commands::wordpress::wp_db_export(state.clone(), id).await?;
            Ok(json!({ "path": path }))
        }
        "db.import" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("db.import needs an id".into()))?
                .to_string();
            let path = args["path"]
                .as_str()
                .ok_or_else(|| Error::Other("db.import needs a path".into()))?
                .to_string();
            commands::wordpress::wp_db_import(state.clone(), id, path).await?;
            Ok(Value::Null)
        }
        // PHP ini settings — whitelisted keys only (core::php::SETTINGS);
        // apply restarts that minor's live pool.
        "php.settings" => {
            let state = app_state(app)?;
            let minor = need_str(&args, "minor", cmd)?;
            Ok(json!({ "settings": to_value(&commands::php::get_php_settings(state.clone(), minor)?)? }))
        }
        "php.settings.set" => {
            let state = app_state(app)?;
            let minor = need_str(&args, "minor", cmd)?;
            let settings: Vec<commands::php::PhpSettingInput> =
                serde_json::from_value(args["settings"].clone())
                    .map_err(|e| Error::Other(format!("bad php settings: {e}")))?;
            commands::php::apply_php_settings(state.clone(), minor, settings).await?;
            Ok(Value::Null)
        }
        // DB engine versions + the nuclear per-site reset (typed-confirm in
        // the CLI, like the UI's dialog).
        "db.versions" => {
            let state = app_state(app)?;
            let status = commands::database::databases_status(state.clone()).await?;
            let available = commands::database::db_engine_versions()?;
            Ok(json!({ "engines": to_value(&status)?, "available": to_value(&available)? }))
        }
        "db.version.set" => {
            let state = app_state(app)?;
            commands::database::set_db_engine_version(
                state.clone(),
                need_str(&args, "key", cmd)?,
                need_str(&args, "version", cmd)?,
            )
            .await?;
            Ok(Value::Null)
        }
        "db.reset" => {
            let state = app_state(app)?;
            commands::wordpress::wp_site_reset(state.clone(), need_str(&args, "id", cmd)?)
                .await?;
            Ok(Value::Null)
        }
        // Core version pinning — update lives above; switch is exact-version.
        "wp.core-versions" => {
            Ok(json!({ "versions": to_value(&commands::wordpress::wp_core_versions().await?)? }))
        }
        "wp.core-switch" => {
            let state = app_state(app)?;
            let switch = commands::wordpress::wp_core_switch_version(
                state.clone(),
                need_str(&args, "id", cmd)?,
                need_str(&args, "version", cmd)?,
            )
            .await?;
            to_value(&switch)
        }
        // Optional services — the Databases/Mail start-stop toggles. Engine
        // keys are validated by engine_from_key; web-tier singles stay
        // deliberately unmapped (topology invariants).
        "service.db" => {
            let state = app_state(app)?;
            let key = need_str(&args, "key", cmd)?;
            if args["running"].as_bool().ok_or_else(|| Error::Other("service.db needs `running`".into()))? {
                commands::database::start_database(state.clone(), key).await?;
            } else {
                commands::database::stop_database(state.clone(), key).await?;
            }
            Ok(Value::Null)
        }
        // Web tier: RESTART only, one service at a time. There is no
        // `service.stop nginx` on purpose — a stopped web-tier service is every
        // site on it failing with nothing to say why, and stopping the stack is
        // `stop`. See `core::service_manager::WebTarget`.
        "service.restart" => {
            let state = app_state(app)?;
            let report = commands::services::restart_web_service(
                state.clone(),
                need_str(&args, "target", cmd)?,
            )
            .await?;
            to_value(&report)
        }
        "service.mail" => {
            let state = app_state(app)?;
            if args["running"].as_bool().ok_or_else(|| Error::Other("service.mail needs `running`".into()))? {
                commands::mail::start_mail(state.clone()).await?;
            } else {
                commands::mail::stop_mail(state.clone()).await?;
            }
            Ok(Value::Null)
        }
        // Mail — the Mail screen's list/clear + the web-UI port for `mail open`.
        "mail.list" => {
            let state = app_state(app)?;
            let query = args["query"].as_str().map(str::to_string);
            // Optional and absent-means-false: an older `rex` talking to a newer
            // app sends no `unread`, and must keep getting the whole inbox.
            let unread = args["unread"].as_bool();
            Ok(to_value(&commands::mail::mailpit_messages(state.clone(), query, unread).await?)?)
        }
        "mail.mark_read" => {
            let state = app_state(app)?;
            commands::mail::mailpit_mark_all_read(state.clone()).await?;
            Ok(Value::Null)
        }
        "mail.clear" => {
            let state = app_state(app)?;
            commands::mail::mailpit_clear(state.clone()).await?;
            Ok(Value::Null)
        }
        "mail.status" => {
            let state = app_state(app)?;
            Ok(to_value(&commands::mail::mailpit_status(state.clone()).await?)?)
        }
        // Tunnels — the SiteDetail Share actions (cloudflared).
        "tunnel.list" => {
            let state = app_state(app)?;
            let tunnels = app
                .try_state::<commands::tunnels::Tunnels>()
                .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?;
            Ok(json!({ "tunnels": to_value(&commands::tunnels::tunnels_status(state.clone(), tunnels).await?)? }))
        }
        "tunnel.start" | "tunnel.stop" => {
            let state = app_state(app)?;
            let tunnels = app
                .try_state::<commands::tunnels::Tunnels>()
                .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?;
            let id = need_str(&args, "id", cmd)?;
            if cmd == "tunnel.start" {
                let provision = app
                    .try_state::<commands::site_provision::ProvisionJobs>()
                    .ok_or_else(|| Error::Other("provision registry not ready".into()))?;
                let info = commands::tunnels::start_tunnel(
                    app.app_handle().clone(),
                    state.clone(),
                    tunnels,
                    provision,
                    id,
                )
                .await?;
                Ok(to_value(&info)?)
            } else {
                commands::tunnels::stop_tunnel(state.clone(), tunnels, id).await?;
                Ok(Value::Null)
            }
        }
        // ── repo group (git/asset assets) — wave 1: pure request/response.
        // Every arm rides the SAME commands::repo fns the UI calls; `--theme`
        // arrives as `theme: true` and flips the kind.
        "repo.list" => {
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let assets =
                commands::repo::repo_assets(state.clone(), id.clone()).await?;
            let mut statuses = serde_json::Map::new();
            if args["status"].as_bool().unwrap_or(false) {
                // Per-row live status (the 🟡 flag): N status calls, each the
                // same fn `repo status` uses. Errors per row, never aborting
                // the list (a broken checkout still lists).
                for a in &assets {
                    let key = format!("{}/{}", a.kind, a.dir_name);
                    let st = commands::repo::repo_asset_status(
                        app.app_handle().clone(),
                        id.clone(),
                        a.kind.clone(),
                        a.dir_name.clone(),
                    )
                    .await;
                    statuses.insert(
                        key,
                        match st {
                            Ok(v) => to_value(&v)?,
                            Err(e) => json!({ "error": e.to_string() }),
                        },
                    );
                }
            }
            Ok(json!({ "assets": to_value(&assets)?, "statuses": statuses }))
        }
        "repo.status" => {
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            let st = commands::repo::repo_asset_status(app.app_handle().clone(), id, kind, dir).await?;
            Ok(to_value(&st)?)
        }
        "repo.branches" => {
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            let b = commands::repo::repo_branches(app.app_handle().clone(), id, kind, dir).await?;
            Ok(to_value(&b)?)
        }
        "repo.prs" => {
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            let p = commands::repo::repo_pull_refs(app.app_handle().clone(), id, kind, dir).await?;
            Ok(to_value(&p)?)
        }
        "repo.adopt" => {
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            commands::repo::repo_adopt(app.app_handle().clone(), id, kind, dir).await?;
            Ok(Value::Null)
        }
        "repo.link" => {
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let target = need_str(&args, "target", cmd)?;
            let name = args["name"].as_str().map(str::to_string);
            let r = commands::repo::repo_link(app.app_handle().clone(), id, kind, name, target).await?;
            Ok(to_value(&r)?)
        }
        "repo.tools" => {
            let refresh = args["refresh"].as_bool().unwrap_or(false);
            let t = commands::repo::repo_tools(app.app_handle().clone(), refresh).await?;
            Ok(json!({ "tools": to_value(&t)? }))
        }
        "repo.watch.list" => {
            let watches = repo_watches_state(app)?;
            let id = args["id"].as_str().map(str::to_string);
            let w = commands::repo::repo_watches(watches, id, None).await?;
            Ok(json!({ "watchers": to_value(&w)? }))
        }
        "repo.watch.start" => {
            let state = app_state(app)?;
            let watches = repo_watches_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            let script = need_str(&args, "script", cmd)?;
            let w = commands::repo::repo_watch_start(
                app.app_handle().clone(),
                state.clone(),
                watches,
                id,
                kind,
                dir,
                script,
            )
            .await?;
            Ok(to_value(&w)?)
        }
        "repo.watch.stop" => {
            // Stop BY DIR (the CLI-friendly key): resolve the watcher id via
            // the same list fn, then the same stop fn the panel button calls.
            let state = app_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            let watches = repo_watches_state(app)?;
            let all = commands::repo::repo_watches(
                watches,
                Some(id.clone()),
                Some(kind.clone()),
            )
            .await?;
            let Some(w) = all.iter().find(|w| w.dir_name == dir) else {
                return Err(Error::Other(format!("no watcher running for {dir}")));
            };
            let watches = repo_watches_state(app)?;
            commands::repo::repo_watch_stop(app.app_handle().clone(), state.clone(), watches, w.id.clone())
                .await?;
            Ok(Value::Null)
        }
        // ── repo group wave 2: job-shaped commands (hold the connection,
        // poll to terminal, reply with snapshot + the flat job log).
        "repo.add" => {
            let state = app_state(app)?;
            let jobs = repo_jobs_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let url = need_str(&args, "url", cmd)?;
            let branch = args["branch"].as_str().map(str::to_string);
            let name = args["name"].as_str().map(str::to_string);
            let install = args["install"].as_bool().unwrap_or(false);
            let snap = commands::repo::repo_add(
                app.app_handle().clone(),
                state.clone(),
                jobs,
                id,
                kind,
                url,
                branch,
                name,
            )
            .await?;
            let st = if install {
                repo_run_offered(app, app.app_handle().clone(), &snap.id).await?
            } else {
                repo_wait_settled(app, &snap.id, None).await?
            };
            repo_job_reply(app, &st)
        }
        "repo.check" => {
            let state = app_state(app)?;
            let jobs = repo_jobs_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            // repo_check awaits its (instant, zero-exec) worker and returns
            // the settled snapshot — no wait loop needed.
            let install = args["install"].as_bool().unwrap_or(false);
            let st = commands::repo::repo_check(
                app.app_handle().clone(),
                state.clone(),
                jobs,
                id,
                kind,
                dir,
            )
            .await?;
            let st = if install {
                repo_run_offered(app, app.app_handle().clone(), &st.id).await?
            } else {
                st
            };
            repo_job_reply(app, &st)
        }
        "repo.op" => {
            let state = app_state(app)?;
            let jobs = repo_jobs_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            let op = need_str(&args, "op", cmd)?;
            let target_ref = args["ref"].as_str().map(str::to_string);
            let install = args["install"].as_bool().unwrap_or(false);
            let snap = commands::repo::repo_git_op(
                app.app_handle().clone(),
                state.clone(),
                jobs,
                id,
                kind,
                dir,
                op.clone(),
                target_ref,
            )
            .await?;
            let st = if install && matches!(op.as_str(), "pull" | "checkout") {
                repo_run_offered(app, app.app_handle().clone(), &snap.id).await?
            } else {
                repo_wait_settled(app, &snap.id, None).await?
            };
            repo_job_reply(app, &st)
        }
        "repo.run" => {
            let state = app_state(app)?;
            let jobs = repo_jobs_state(app)?;
            let id = need_str(&args, "id", cmd)?;
            let kind = repo_kind(&args);
            let dir = need_str(&args, "dir", cmd)?;
            let script = need_str(&args, "script", cmd)?;
            let snap = commands::repo::repo_script_job(
                app.app_handle().clone(),
                state.clone(),
                jobs,
                id,
                kind,
                dir,
                script,
            )
            .await?;
            let st = repo_wait_settled(app, &snap.id, Some("script")).await?;
            repo_job_reply(app, &st)
        }
        // Default TLD (new sites) — policy stays in core::tld.
        "tld.get" => {
            let state = app_state(app)?;
            Ok(json!({ "tld": commands::settings::default_tld(state.clone())? }))
        }
        "tld.set" => {
            let state = app_state(app)?;
            let tld = need_str(&args, "tld", cmd)?;
            commands::settings::set_default_tld(state.clone(), tld.clone())?;
            Ok(json!({ "tld": tld }))
        }
        // Re-install the OS resolver file for a TLD a site actually answers on.
        // The fix `doctor` names, because it found the problem: a resolver
        // rexenv installed and lost leaves a site perfectly served and
        // unreachable, and until now nothing in the CLI could put it back.
        //
        // Scoped to TLDs IN USE on purpose. `ensure_resolver` writes a root-owned
        // file under `/etc/resolver` behind a privileged prompt; a verb that
        // installed one for any string a caller passed would be a way to point
        // arbitrary TLDs at this machine's loopback resolver, which is a bigger
        // door than "repair what my sites need".
        "tld.repair" => {
            let state = app_state(app)?;
            let outcome = commands::system::repair_resolver(state.clone(), need_str(&args, "tld", cmd)?).await?;
            Ok(to_value(&outcome)?)
        }
        "tld.remove" => {
            let state = app_state(app)?;
            let tld = need_str(&args, "tld", cmd)?;
            let removed = commands::system::remove_resolver(state.clone(), tld.clone()).await?;
            Ok(json!({ "tld": tld, "removed": removed }))
        }
        "version" => Ok(to_value(&commands::system::app_info())?),
        // Bring the app's window up. Two callers, and the SECOND is why this
        // exists: `rex open` (a window is one command away from a terminal, now
        // that the app has no dock icon to click), and a second rexenv launch
        // handing itself off to the one already running — see
        // `hand_off_to_running_instance`. Deliberately NOT state-dependent: it
        // must work while the app is still starting, which is exactly when an
        // impatient second launch happens.
        //
        // A LOGIN launch's handoff (`handoff_request(true)`) raises nothing and
        // asks for login-start instead — the login is the one fact that launch
        // carried, and dropping it left the stack down after a reboot on 0.8.10's
        // draft (`crate::LoginStartGate`). Held until setup is ready if it is not.
        "app.open" => {
            if handoff_is_login(&args) {
                crate::request_login_start(app.app_handle());
            } else {
                crate::show_main_window(app.app_handle());
            }
            Ok(Value::Null)
        }
        // Doctor: one honest diagnosis pass composing the app's own probes —
        // nothing here invents a new check, it reuses the exact machinery the
        // watchdog/Start-all/Settings already trust.
        "doctor" => {
            let state = app_state(app)?;
            let services = commands::services::services_status(state.clone()).await?;
            let dns = app
                .try_state::<crate::state::app::DnsState>()
                .map(|dns| commands::system::dns_status(state.clone(), dns));
            let cli = commands::system::cli_status(state.clone()).ok();
            // Edge WIRE identity — the Herd class: every process check can be
            // green while a foreign 127.0.0.1:443 listener answers all sites.
            let caddy_running = services.iter().any(|s| s.name == "Caddy" && s.running);
            let wire = crate::core::proxy::edge_wire(
                crate::core::adminer::ADMINER_HOST,
                crate::core::proxy::DEFAULT_HTTPS_PORT,
            )
            .await;
            let wire_ours = wire == crate::core::proxy::EdgeWire::Ours;
            // NOTHING listening while our Caddy is running is not a conflict —
            // it is our edge failing to serve, which has a different fix. The
            // old code reported both as a conflict and named a holder from a
            // lookup that finds nobody.
            let edge_silent = caddy_running && wire == crate::core::proxy::EdgeWire::NoAnswer;
            let edge_conflict = (caddy_running
                && wire == crate::core::proxy::EdgeWire::Foreign)
                .then(|| {
                let help = state
                    .platform
                    .supervisor()
                    .port_conflict_help(crate::core::proxy::DEFAULT_HTTPS_PORT, false);
                json!({ "holder": help.holder, "app": help.app, "fix": help.free_command })
            });
            // Foreign holders on rexenv's fixed ports. Ours-by-marker is fine
            // (running services); 80/443 are judged by the wire probe above
            // (the root edge's binary lives outside the user marker); the DNS
            // UDP port is judged by dns_status.
            let marker = state
                .platform
                .paths()
                .app_data_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            let mut port_conflicts = Vec::new();
            for req in crate::core::ports::default_ports() {
                if matches!(req.proto, crate::core::ports::Proto::Udp)
                    || req.port == crate::core::proxy::DEFAULT_HTTPS_PORT
                    || req.port == crate::core::proxy::DEFAULT_HTTP_PORT
                    || crate::core::ports::is_free(&*state.platform, req.port, req.proto)
                    || (!marker.is_empty()
                        && state.platform.supervisor().owned_master(req.port, &marker).is_some())
                {
                    continue;
                }
                let help = state.platform.supervisor().port_conflict_help(req.port, false);
                port_conflicts.push(json!({
                    "service": req.service,
                    "port": req.port,
                    "holder": help.holder,
                    "fix": help.free_command,
                }));
            }
            // Borrowed resolver files another tool reclaimed — invisible to
            // every other probe, because our resolver keeps answering.
            // Which PHP builds in USE carry the c-ares bug class (measured per
            // minor in `core::wp_dns`), and how many sites sit on them.
            let (ares_minors, ares_sites) = {
                let sites = state
                    .db
                    .lock()
                    .ok()
                    .and_then(|c| crate::core::sites::list(&c).ok())
                    .unwrap_or_default();
                let minors = crate::core::wp_dns::ares_minors_in_use(&sites);
                let count = sites
                    .iter()
                    .filter(|s| {
                        minors.contains(&crate::core::php::minor_of(
                            &crate::core::wp_dns::effective_php_version(s),
                        ))
                    })
                    .count();
                (minors, count)
            };
            let resolver_drift = {
                let conn = state
                    .db
                    .lock()
                    .map_err(|_| Error::Other("database lock poisoned".into()))?;
                crate::core::dns::drifted_takeovers(
                    &conn,
                    state.platform.as_ref(),
                    crate::core::dns::DEFAULT_DNS_PORT,
                )
            };
            // Every TLD a site ANSWERS on that this machine cannot resolve —
            // including one only an extra domain uses. `resolverDrift` below is
            // narrower on purpose (files we borrowed and lost); this catches a
            // resolver we installed ourselves and lost, which no other probe
            // sees because our own DNS keeps answering on the other TLDs.
            let unresolvable = {
                let conn = state
                    .db
                    .lock()
                    .map_err(|_| Error::Other("database lock poisoned".into()))?;
                crate::core::dns::unresolvable_tlds_in_use(
                    &conn,
                    state.platform.as_ref(),
                    crate::core::dns::DEFAULT_DNS_PORT,
                )
                .into_iter()
                .map(|(tld, foreign)| json!({ "tld": tld, "foreign": foreign }))
                .collect::<Vec<_>>()
            };
            // WordPress sites missing long-named core files (rexenv 0.4.0–0.7.1 cut them, ledger
            // #604). One wordpress.org GET per VERSION, cached; a site whose list could not be
            // fetched is `unchecked`, never silently clean.
            let core_files = {
                let roots: Vec<(String, std::path::PathBuf)> = state
                    .db
                    .lock()
                    .ok()
                    .and_then(|c| crate::core::sites::list(&c).ok())
                    .unwrap_or_default()
                    .iter()
                    .map(|s| (s.domain.clone(), s.served_root()))
                    .collect();
                let (mut broken, mut unchecked) = (Vec::new(), Vec::new());
                // A version wordpress.org did not answer for is not asked again in this run: on a
                // network that drops packets, every site on that version would wait out the client's
                // 10-second timeout in turn (the 8 Oct VM smoke, with the host refused, failed fast).
                let mut failed: std::collections::HashMap<String, String> = Default::default();
                for (domain, root) in roots {
                    let version = crate::core::wordpress::installed_version(&root);
                    if let Some(e) = version.as_ref().and_then(|v| failed.get(v)) {
                        unchecked.push(json!({ "domain": domain, "error": e }));
                        continue;
                    }
                    let report = commands::wordpress::cut_name_report_for(&root).await;
                    if let (Err(e), Some(v)) = (&report, version) {
                        failed.insert(v, e.to_string());
                    }
                    match report {
                        Ok(Some(r)) if !r.missing.is_empty() => broken.push(json!({
                            "domain": domain,
                            "missing": r.missing.len(),
                            "message": r.message,
                        })),
                        Ok(_) => {}
                        Err(e) => unchecked.push(json!({ "domain": domain, "error": e.to_string() })),
                    }
                }
                json!({ "broken": broken, "unchecked": unchecked })
            };
            Ok(json!({
                "app": to_value(&commands::system::app_info())?,
                "coreFiles": core_files,
                "unresolvableTlds": unresolvable,
                "dns": to_value(&dns)?,
                "services": to_value(&services)?,
                "edge": {
                    "running": caddy_running,
                    "wireOurs": wire_ours,
                    "conflict": edge_conflict,
                    // Distinct from `conflict`: our edge is up and answering
                    // nothing, so there is no third party to name.
                    "silent": edge_silent,
                },
                "portConflicts": port_conflicts,
                "cli": to_value(&cli)?,
                "runtime": to_value(&state.platform.binaries().runtime_problem())?,
                // Borrowed resolver files another tool reclaimed — invisible to
                // every other probe, because our resolver keeps answering.
                "resolverDrift": to_value(&resolver_drift)?,
                // A LIMITATION, not a fault: a bundled build that links c-ares (8.0 today),
                // whose curl cannot resolve a `.rex` host at all. WordPress is
                // covered by the mu-plugin; a plugin's raw `curl_init()` and any
                // non-WordPress PHP app are not, and today that arrives as an
                // unexplained "Could not resolve host" inside somebody's code.
                // Reported so it can be READ somewhere, never counted as a
                // finding — nothing here is broken or fixable by the user.
                "curlResolver": {
                    "aresMinors": ares_minors,
                    "sites": ares_sites,
                },
            }))
        }
        // A typo never reaches here — `rex`'s own match rejects an unknown
        // subcommand client-side — so this ALWAYS means the two builds
        // disagree. The app cannot tell which one is stale, which is why the
        // wording stayed a hedge for so long; `rex` now follows this with the
        // two version numbers (`version_skew`, 23 Aug 2026), so the hedge is
        // the app's half of a sentence the CLI finishes.
        other => Err(Error::Other(format!(
            "unknown command: {other} (this rex and the running app are different builds)"
        ))),
    }
}

/// **Is another rexenv already running? Then hand this launch to it.**
///
/// The menu-bar app made a second instance INVISIBLE. Before it, a second
/// launch announced itself with a second window and a second dock tile, and
/// closing a window ended it; now an app with no dock icon, no window and a
/// `--hidden` login mode can sit there adopting services, opening the database
/// as a SECOND WRITER and fighting for these very sockets. Two of them ran on
/// the developer's machine during Phase C and the only symptom was two icons in
/// the menu bar.
///
/// **The socket IS the lock, and that is the point.** A pid file records a
/// claim that outlives the process that made it — after a crash it lies, and
/// every user of one eventually writes the "is this pid still ours" code that
/// gets it wrong. A listening unix socket cannot lie: the listener dies with
/// the process, so a stale socket FILE refuses connections (`ECONNREFUSED`)
/// while a live one accepts. So `connect` SUCCEEDING is the whole test —
/// proof that a process is listening right now, needing no reply and no
/// timeout to interpret. The activate is best-effort on top of that: a wedged
/// app that accepts and never answers still owns the stack, and starting a
/// second copy of it would be strictly worse than making the user wait.
///
/// Returns true when the caller must exit. Never returns true on an error it
/// cannot interpret: if anything about this is unclear, the app starts, because
/// refusing to launch is the worse failure of the two.
#[cfg(unix)]
pub fn hand_off_to_running_instance() -> bool {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let Ok(dir) = crate::platform::current().paths().config_dir() else {
        return false;
    };
    let path = dir.join(SOCKET_FILE);
    // ENOENT (never bound) and ECONNREFUSED (stale file after a crash) both
    // mean nobody is home — the exact treatment `rex` gives the same two.
    let Ok(mut sock) = UnixStream::connect(&path) else {
        return false;
    };
    // Best-effort from here: the answer to "should I exit" is already yes.
    let _ = sock.set_write_timeout(Some(std::time::Duration::from_millis(500)));
    let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(1500)));
    let login = crate::is_hidden_launch();
    let _ = sock.write_all(handoff_request(login).as_bytes());
    let _ = sock.flush();
    let mut buf = [0u8; 64];
    let _ = sock.read(&mut buf);
    // stderr, not `log`: the logger belongs to the instance that owns the app
    // data, and this process is about to stop existing. A launch that vanishes
    // silently is indistinguishable from one that crashed.
    eprintln!("{}", handoff_note(login));
    true
}

/// **What a second launch sends the instance that holds the lock** — one line, built HERE for both
/// transports (the unix socket above, Windows' pipe in `platform::windows::app_pipe`), so the two
/// cannot say different things. `login` = this launch was `--hidden`: the holder then runs
/// login-start instead of raising its window. Until 29 Sep 2026 both sent a bare `app.open`, and a
/// login that found rexenv already open (macOS's "Reopen windows" relaunch wins the race at every
/// ordinary reboot) was lost — see `crate::LoginStartGate`.
pub fn handoff_request(login: bool) -> String {
    let args = if login { json!({ "login": true }) } else { json!({}) };
    format!("{}\n", json!({ "cmd": "app.open", "args": args }))
}

/// The `app.open` arm's reading of [`handoff_request`]: only an explicit `true` is a login.
fn handoff_is_login(args: &Value) -> bool {
    args.get("login").and_then(Value::as_bool) == Some(true)
}

/// What the exiting second copy prints (stderr) after a handoff.
pub fn handoff_note(login: bool) -> &'static str {
    if login {
        "rexenv is already running — handed this login launch to it (it runs Start all).\n\
         This second copy has exited."
    } else {
        "rexenv is already running — brought its window to the front.\n\
         (Its menu-bar icon is the one to use; this second copy has exited.)"
    }
}

/// Spawn the listener at app startup, on the socket `claim_at_startup` already
/// holds (or a late bind when it could not). Failure is logged, never fatal —
/// the app works without its CLI.
#[cfg(unix)]
pub fn spawn(app: tauri::AppHandle, claimed: Option<std::os::unix::net::UnixListener>) {
    let path = match crate::platform::current().paths().config_dir() {
        Ok(dir) => dir.join(SOCKET_FILE),
        Err(e) => {
            log::error!("cli: no config dir for the socket: {e}");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        let adopted = claimed.map(|l| -> Result<UnixListener> {
            l.set_nonblocking(true)?;
            Ok(UnixListener::from_std(l)?)
        });
        let listener = match adopted.unwrap_or_else(|| bind(&path)) {
            Ok(l) => l,
            Err(e) => {
                log::error!("cli: could not bind {}: {e}", path.display());
                return;
            }
        };
        log::info!("cli: listening on {}", path.display());
        serve(listener, move |line, progress| {
            let app = app.clone();
            async move { handle_request(&app, line, progress).await }
        })
        .await;
    });
}

// ── Windows: the single-instance pipe (W7 S1, plan §5 W7 ruling Q1, ledger #620) ──────────────────
// The unix socket above is both the lock and the CLI. On Windows the lock is a named pipe (W7 S1), and since
// W8 S2 it is the CLI's transport too: every connection runs `serve_connection`, the exchange the socket runs
// (ledger #631). MCP gets a pipe of its own (plan §5 W8 ruling Q2).

/// What `run()` does about the single-instance pipe before Tauri boots — Windows' [`StartupClaim`].
#[cfg(windows)]
pub enum PipeStartup {
    /// Continue: the lock is ours (`Some`), or could not be taken (`None` — the app starts without it).
    Ours(Option<crate::platform::HeldAppPipe>),
    /// Another instance holds this app-data directory; it has been asked to show its window.
    AnotherInstanceRuns,
}

/// Claim the pipe at process start, or hand off to the instance that holds it. Before Tauri boots for
/// the reason `claim_at_startup` gives: the seconds of startup are when a second launch used to become a
/// second writer.
#[cfg(windows)]
pub fn claim_pipe_at_startup() -> PipeStartup {
    let Ok(dir) = crate::platform::current().paths().config_dir() else {
        return PipeStartup::Ours(None);
    };
    match crate::platform::claim_app_pipe(&dir) {
        crate::platform::AppPipeClaim::Ours(held) => PipeStartup::Ours(Some(held)),
        crate::platform::AppPipeClaim::AnotherInstance => {
            let login = crate::is_hidden_launch();
            if crate::platform::hand_off_to_app_pipe(&dir, &handoff_request(login)) {
                eprintln!("{}", handoff_note(login));
                PipeStartup::AnotherInstanceRuns
            } else {
                // Held a moment ago and gone now — this copy is the app.
                PipeStartup::Ours(None)
            }
        }
        crate::platform::AppPipeClaim::Unclear(code) => {
            eprintln!("rexenv: could not take the single-instance pipe (error {code}) — starting without the lock");
            PipeStartup::Ours(None)
        }
    }
}

/// Serve the pipe the startup claim holds: every CLI request, through `serve_connection` (ledger #631). The
/// next instance is always made BEFORE the one in hand is given away: the lock is "some instance of this name
/// exists", so a moment with none is a moment a second launch could take it.
#[cfg(windows)]
pub fn spawn_pipe(app: tauri::AppHandle, held: Option<crate::platform::HeldAppPipe>) {
    let Some(held) = held else {
        log::warn!("app pipe: no single-instance lock this launch — a second launch would start a second app, and rex cannot reach this one");
        return;
    };
    tauri::async_runtime::spawn(async move {
        let name = held.name().to_string();
        let mut server = match held.into_server() {
            Ok(server) => server,
            Err(e) => {
                log::error!("app pipe: could not serve {name}: {e}");
                return;
            }
        };
        log::info!("app pipe: holding {name}");
        loop {
            let connected = server.connect().await;
            let next = match crate::platform::next_app_pipe_instance(&name) {
                Ok(next) => next,
                Err(e) => {
                    log::error!("app pipe: could not make the next instance of {name}: {e} — keeping this one");
                    if connected.is_ok() {
                        // No next instance: serve this client on the one in hand, then listen on it again.
                        let (mut read, mut write) = tokio::io::split(server);
                        let app = app.clone();
                        serve_connection(&mut read, &mut write, move |line, progress| async move {
                            handle_request(&app, line, progress).await
                        })
                        .await;
                        server = read.unsplit(write);
                        let _ = server.disconnect();
                    }
                    continue;
                }
            };
            let current = std::mem::replace(&mut server, next);
            if connected.is_ok() {
                let app = app.clone();
                tokio::spawn(async move {
                    let (read, write) = tokio::io::split(current);
                    serve_connection(read, write, move |line, progress| async move {
                        handle_request(&app, line, progress).await
                    })
                    .await;
                });
            }
        }
    });
}


/// #54 — the `rex` crate never links the app library.
///
/// The claim is a whole BUG CLASS made impossible rather than avoided: if `cli/`
/// could link `rexenv_lib`, a `rex` command could open the same SQLite the app
/// has open, and two writers on one database is the corruption class. The CLI is
/// therefore a thin socket client with ONE dependency, and every command it has
/// is dispatched by the app.
///
/// A "what can this reach" claim, and the way it goes false is a single line in
/// a manifest, added by someone who wanted a type. So the failure explains the
/// design rather than showing a mismatch — the person adding the dependency is
/// looking at a diff, not at this file.
#[cfg(test)]
mod cli_isolation {
    #[test]
    fn the_rex_crate_never_links_the_app_library() {
        let manifest = include_str!("../../cli/Cargo.toml");
        // EVERY dependency table — `[dependencies]` and each `[target.'…'.dependencies]` (build and dev tables
        // too). This read `[dependencies]` alone until W8 put tokio under a Windows-only table (ledger #630):
        // a banned crate added there would have passed unread.
        let tables: Vec<&str> = manifest
            .split("\n[")
            .filter(|table| table.lines().next().unwrap_or("").trim_end().trim_end_matches(']').ends_with("dependencies"))
            .collect();
        let deps: String = tables
            .iter()
            .flat_map(|table| table.lines().skip(1))
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");

        for banned in ["rexenv", "rusqlite", "tauri", "path ="] {
            assert!(
                !deps.contains(banned),
                "`cli/Cargo.toml` now depends on `{banned}`.\n\n\
                 The rex CLI is deliberately a thin client of the running app — serde_json, sha2 for the \
                 Windows pipe's name, tokio for the Windows pipe itself. Linking the app library — or SQLite \
                 directly — would let a `rex` command open the same database the running app has open, and \
                 two writers on one SQLite file is the corruption class this separation exists to make \
                 IMPOSSIBLE rather than merely avoided (ledger #54).\n\n\
                 If you need something the app knows, add a command to `cli_server.rs` and ask \
                 for it over the socket or pipe — that is the whole design, and it is also what keeps \
                 the CLI and the UI on the same code path (#57).\n\n\
                 The dependency tables currently hold:\n{deps}"
            );
        }
        // The canary: a manifest we failed to read would pass every check above — and one read without the
        // Windows table would pass a banned crate added there.
        assert!(
            deps.contains("serde_json") && deps.contains("tokio"),
            "the dependency tables did not parse as expected (serde_json from [dependencies], tokio from the \
             Windows table) — this guard would pass on an empty string. Fix the parse before trusting the \
             result.\n{deps}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ledger #631 — **one exchange, whatever the transport**: `serve_connection` over an in-memory pipe (no
    /// socket, the shape Windows' named pipe hands it) keeps the framing both transports promise — one envelope
    /// for a client that did not ask to stream; the progress records in order, then the envelope, for one that
    /// did — and a command whose client hangs up mid-stream still runs to its end.
    #[tokio::test]
    async fn one_exchange_over_any_transport_frames_streams_and_outlives_its_client() {
        async fn exchange(request: &str) -> Vec<String> {
            let (client, server) = tokio::io::duplex(4096);
            let (read, write) = tokio::io::split(server);
            tokio::spawn(serve_connection(read, write, |line: String, p: Progress| async move {
                p.send(json!({ "step": 1 }));
                p.send(json!({ "step": 2 }));
                json!({ "ok": true, "data": line.trim() }).to_string()
            }));
            let (client_read, mut client_write) = tokio::io::split(client);
            client_write.write_all(format!("{request}\n").as_bytes()).await.expect("write");
            let mut lines = Vec::new();
            let mut reader = BufReader::new(client_read);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => lines.push(line.trim().to_string()),
                }
            }
            lines
        }

        let plain = exchange("{\"cmd\":\"x\"}").await;
        assert_eq!(plain.len(), 1, "a client that did not ask to stream got more than the envelope: {plain:?}");
        assert!(plain[0].contains("\"ok\":true"), "{plain:?}");

        let streamed = exchange("{\"cmd\":\"x\",\"stream\":true}").await;
        assert_eq!(streamed.len(), 3, "two progress records and one envelope: {streamed:?}");
        assert!(streamed[0].contains("\"step\":1") && streamed[1].contains("\"step\":2"), "out of order: {streamed:?}");
        assert!(streamed[2].contains("\"ok\":true"), "the envelope must be last: {streamed:?}");

        // The client reads one progress record and hangs up; the command must still finish.
        let (client, server) = tokio::io::duplex(4096);
        let (read, write) = tokio::io::split(server);
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel::<()>();
        let (gone_tx, gone_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(serve_connection(read, write, move |_line: String, p: Progress| async move {
            p.send(json!({ "step": 1 }));
            let _ = gone_rx.await;
            p.send(json!({ "step": 2 }));
            // Still working after the write that finds the client gone: a server that drops the command
            // on that write must drop it before this line.
            tokio::time::sleep(Duration::from_millis(300)).await;
            let _ = finished_tx.send(());
            json!({ "ok": true }).to_string()
        }));
        let (client_read, mut client_write) = tokio::io::split(client);
        client_write.write_all(b"{\"cmd\":\"x\",\"stream\":true}\n").await.expect("write");
        let mut first = String::new();
        let mut reader = BufReader::new(client_read);
        reader.read_line(&mut first).await.expect("the first progress record");
        assert!(first.contains("\"step\":1"), "{first:?}");
        drop(reader);
        drop(client_write);
        let _ = gone_tx.send(());
        let finished = tokio::time::timeout(Duration::from_secs(5), finished_rx).await;
        assert!(
            matches!(finished, Ok(Ok(()))),
            "the command was dropped when its client hung up — a `rex site create … | head -1` would leave a half-made site"
        );
    }

    /// Ledger #631 — **both transports hand every connection to the one exchange**: the unix `serve` and
    /// Windows' `spawn_pipe` call `serve_connection`, and nothing on the Windows side answers a request itself
    /// any more (W7's `app.open`-only rule and its reply are gone).
    #[test]
    fn both_transports_hand_every_connection_to_the_one_exchange() {
        let src = include_str!("cli_server.rs");
        let body = |head: &str| -> &str {
            let start = src.find(head).unwrap_or_else(|| panic!("`{head}` is gone"));
            let rest = &src[start..];
            &rest[..rest.find("\n}\n").expect("the function's end")]
        };
        assert!(body("pub async fn serve<F, Fut>(").contains("serve_connection("), "the unix socket answers a connection itself");
        let pipe = body("pub fn spawn_pipe(");
        assert_eq!(pipe.matches("serve_connection(").count(), 2, "both of the pipe's serving paths go through the exchange:\n{pipe}");
        for gone in [concat!("app_pipe_", "serves"), concat!("app_pipe_", "not_yet"), concat!("fn serve_pipe_", "connection")] {
            assert!(!src.contains(gone), "`{gone}` is back — the Windows pipe would answer requests its own way again");
        }
    }
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    fn scratch_sock(name: &str) -> std::path::PathBuf {
        // Keep it short: unix socket paths cap at ~104 bytes.
        std::env::temp_dir().join(format!("rexcli-{}-{name}.sock", std::process::id()))
    }

    /// **Every command this server answers is reachable from a `rex` verb.**
    ///
    /// `mail.mark_read` was answered here from the day mail shipped and no verb
    /// ever sent it — a door built and left shut, found only by a docs reconcile
    /// reading the dispatch table by eye. It is the CLI's version of the defect
    /// `core::copy_scan::every_ipc_wrapper_is_actually_called` guards on the
    /// frontend, and it had no guard at all.
    ///
    /// The rule is deliberately LOOSE: an arm counts as reached if its name
    /// appears as a string literal ANYWHERE in the CLI, not only inside a
    /// `request(…)` call. That is not laziness — the CLI sends commands four
    /// ways, and three of them defeat a strict scan: multi-line `request(\n
    /// "x.y", …)`, computed names (`format!("tunnel.{act}")`), and genuinely
    /// dynamic ones (`request(step, …)` where `step` came from a slice). A
    /// **Every top-level key the `status` arm emits is READ by the CLI.**
    ///
    /// The payload is built here and rendered a crate away, so a field added to
    /// one end and not the other is invisible: the server sends it, `rex status`
    /// silently ignores it, and the only symptom is a line nobody sees. That is
    /// the same one-fact-in-two-places shape this tree keeps finding, and it
    /// already happened once in the other direction — `phpUpdateCheck` shipped
    /// as a command nothing called.
    #[test]
    fn every_field_the_status_arm_emits_is_rendered_by_rex_status() {
        let server = crate::core::copy_scan::production_source(include_str!("cli_server.rs"));
        let at = server.find(r#""status" =>"#).expect("the status arm exists");
        let arm = &server[at..at + 1200.min(server.len() - at)];
        assert!(arm.contains("services_status"), "sliced the wrong arm");

        // The keys the arm puts on the wire, read out of the json! literal.
        let mut keys: Vec<&str> = Vec::new();
        for line in arm.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix('"') {
                if let Some((k, tail)) = rest.split_once('"') {
                    if tail.trim_start().starts_with(':') && !k.is_empty() {
                        keys.push(k);
                    }
                }
            }
        }
        keys.sort();
        keys.dedup();
        assert!(
            keys.contains(&"services") && keys.contains(&"dns") && keys.contains(&"update"),
            "the status payload no longer carries the keys this guard was written for: {keys:?}"
        );

        let cli = include_str!("../../cli/src/main.rs");
        let start = cli.find("fn cmd_status").expect("rex status exists");
        let end = cli[start..].find("\nfn ").map(|i| start + i).unwrap_or(cli.len());
        let printer = &cli[start..end];
        for k in keys {
            assert!(
                printer.contains(&format!("data[\"{k}\"]")),
                "the status arm sends {k:?} and `rex status` never reads it — a field on \
                 the wire that renders nowhere is a line nobody sees"
            );
        }
    }

    /// strict rule reported ten false positives on a tree with zero real ones,
    /// and a guard that cries wolf gets an allow-list that swallows the next
    /// real case.
    ///
    /// What it therefore cannot catch: an arm whose name appears in the CLI only
    /// in a COMMENT. It catches the defect that actually happened — a name
    /// nothing in the CLI mentions at all.
    #[test]
    fn every_command_this_server_answers_is_reachable_from_the_cli() {
        const THIS: &str = include_str!("cli_server.rs");
        const CLI: &str = include_str!("../../cli/src/main.rs");

        // The arms of `dispatch`'s own `match cmd`, by their declaration form.
        // Started at the match rather than the file so the OTHER `match` in this
        // file (the watch subcommands) cannot leak in.
        let mut arms: Vec<&str> = Vec::new();
        let mut inside = false;
        // The block ENDS at its catch-all, and the terminator is asserted below.
        // It was `_ =>` only, and `dispatch`'s catch-all is `other =>`, so the
        // scan never stopped: it read past the function to the end of the file
        // and reported the first 8-space-indented string literal it met as an
        // unreachable command (a message inside `hand_off_to_running_instance`,
        // 31 Aug 2026). The guard's own comment claimed the second `match cmd`
        // in this file could not leak in; it could, and everything after it too.
        // A scan that cannot find its end is a scan reading the wrong text.
        let mut terminated = false;
        for line in THIS.lines() {
            if line.trim_start().starts_with("match cmd {") {
                inside = true;
                continue;
            }
            if !inside {
                continue;
            }
            if line.starts_with("        _ =>") || line.starts_with("        other =>") {
                terminated = true;
                break;
            }
            if let Some(rest) = line.strip_prefix("        \"") {
                if let Some(name) = rest.split('"').next() {
                    if !name.is_empty() {
                        arms.push(name);
                    }
                }
            }
        }
        assert!(
            terminated,
            "the arm scan never met `dispatch`'s catch-all — it read to the end of the \
             file, so anything below the function is being reported as a command"
        );
        assert!(
            arms.len() > 50,
            "only {} dispatch arms parsed — the scan is broken, not the table small",
            arms.len()
        );

        // Prefixes the CLI builds at runtime: `format!("tunnel.{act}")`.
        let prefixes: Vec<&str> = CLI
            .match_indices("&format!(\"")
            .filter_map(|(i, _)| CLI[i + 10..].split('"').next())
            .filter_map(|lit| lit.split_once(".{").map(|(head, _)| head))
            .collect();

        let unreachable: Vec<&str> = arms
            .iter()
            .copied()
            .filter(|arm| {
                let quoted = format!("\"{arm}\"");
                !CLI.contains(&quoted)
                    && !prefixes.iter().any(|p| arm.starts_with(&format!("{p}.")))
            })
            .collect();
        assert!(
            unreachable.is_empty(),
            "these commands are answered by cli_server and no `rex` verb sends them: {unreachable:?}\n\
             Either add the verb, or delete the arm — an arm nothing can reach is a \
             promise the CLI does not keep."
        );
    }

    /// **`site.info` answers with the MCP's serving classification, never the
    /// manager's belief.** `rex site info legacy-mwp.rex` printed `serving`
    /// about a setup-incomplete site with no vhost (23 Sep 2026) because the arm
    /// read `sites_serving` — edge up && upstream up — while `site_status`
    /// asked the wire and kept setup-incomplete distinct. One fact, two answers.
    #[test]
    fn site_info_renders_the_classification_not_the_belief() {
        const THIS: &str = include_str!("cli_server.rs");
        let prod = THIS.split("\n#[cfg(test)]").next().unwrap_or(THIS);
        let arm = prod.split_once("        \"site.info\" => {").expect("the site.info arm").1;
        let arm = arm.split("\n        \"").next().unwrap_or(arm);
        assert!(arm.contains("probe_serving(&site)"), "site.info asks the wire");
        assert!(arm.contains("AgentSiteStatus::from_signals("), "…and renders the MCP's verdict");
        assert!(arm.contains("let serving = status.serving;"), "`serving` IS the verdict's boolean");
        assert!(!arm.contains("sites_serving("), "site.info reads the belief again");
        assert!(arm.contains("\"status\": to_value(&status)?"), "the verdict reaches `rex`");
    }

    /// #57 — **every command this server answers runs the SAME `commands::*` fn
    /// the UI invokes.** That is the whole architecture of the CLI: it is remote
    /// control, not a second implementation. An arm that does the work itself
    /// gets a second code path for one behaviour — one that skips whatever the
    /// command does around it (promotion of a scratch site, a share guard, a
    /// validating setter, an event emit) — and the two drift silently, because
    /// nothing fails when only one of them learns a new rule.
    ///
    /// Deny by default, with a declared table of the exceptions and WHY each is
    /// one, so a new inline arm is a build failure rather than a review catch.
    ///
    /// **What it does NOT catch, measured rather than assumed** (a plant that
    /// came back green): an arm that calls a command AND does some of the work
    /// itself. Replacing `site.list`'s `list_sites` with a raw
    /// `core::sites::list` still passed, because the arm also calls
    /// `sites_serving`. Eight arms legitimately touch `core::` today (doctor
    /// composes probes, `config.*` consults the access policy, `site.create`
    /// builds its argument struct), so a stricter rule would need a second
    /// exception table earning its keep against one hypothetical defect. This
    /// catches the shape that has actually appeared: an arm wired to nothing
    /// the UI runs.
    #[test]
    fn every_arm_runs_the_same_command_the_ui_does() {
        /// Arms with no `commands::*` call, each with the reason that is
        /// acceptable. Both are CLI-only surface: there is no UI command to
        /// route to, because the UI does the thing directly.
        const CLI_ONLY: &[(&str, &str)] = &[
            (
                "app.open",
                "shows the app window — the UI equivalent is the window already being \
                 there; there is no IPC command for it, and `rex open` exists precisely \
                 because the menu-bar app has no dock tile",
            ),
            (
                "logs.list",
                "lists every file in the log dir; the UI lists per-SITE targets instead \
                 (`log_targets`), so there is no command with this answer. Routed through \
                 `core::logs::list_files` so the log dir's shape stays defined in one place",
            ),
        ];

        const THIS: &str = include_str!("cli_server.rs");
        // `split_once`, not `split(…).nth(1)`: the pattern is a SUBSTRING of the
        // deeper-indented `match cmd {` inside the wp-plugin arm, so a plain
        // split ends the body there and the scan sees 32 of 79 arms — green,
        // having never looked at two thirds of the table.
        let body = THIS.split_once("    match cmd {").expect("dispatch's match").1;
        let mut arms: Vec<(String, String)> = Vec::new();
        let mut current: Option<(String, String)> = None;
        for line in body.lines() {
            if line.starts_with("        _ =>") || line.starts_with("        other =>") {
                break;
            }
            let is_arm = line.starts_with("        \"") && line.contains("=>");
            if is_arm {
                if let Some(done) = current.take() {
                    arms.push(done);
                }
                let name = line.trim_start().trim_start_matches('"');
                let name = name.split('"').next().unwrap_or_default().to_string();
                current = Some((name, String::new()));
            }
            if let Some((_, buf)) = current.as_mut() {
                buf.push_str(line);
                buf.push('\n');
            }
        }
        if let Some(done) = current.take() {
            arms.push(done);
        }
        assert!(
            arms.len() > 50,
            "only {} arms parsed — the scan is broken, not the table small (a guard that \
             reads nothing passes for the wrong reason)",
            arms.len()
        );

        for (name, arm) in &arms {
            if arm.contains("commands::") {
                assert!(
                    !CLI_ONLY.iter().any(|(n, _)| n == name),
                    "`{name}` is listed as CLI-only but now calls a `commands::` fn — delete \
                     the exception, or the next reader trusts a list that has stopped being true"
                );
                continue;
            }
            assert!(
                CLI_ONLY.iter().any(|(n, _)| n == name),
                "`{name}` answers the CLI without calling any `commands::` fn, so `rex` runs \
                 different code from the UI for it. Route it through the command the UI \
                 invokes — or, if this is genuinely CLI-only surface, add it to `CLI_ONLY` \
                 with the reason"
            );
        }
        for (name, _) in CLI_ONLY {
            assert!(
                arms.iter().any(|(n, _)| n == name),
                "`{name}` is excused as CLI-only and is not a dispatch arm any more — a stale \
                 exception is a hole waiting for a command to be given that name"
            );
        }
    }

    /// **A second launch says whether it was a login, and both transports say it the same way**
    /// (ledger #739). The unix socket and Windows' pipe send `handoff_request`'s bytes; the
    /// `app.open` arm reads them with `handoff_is_login`, so the round trip IS the contract.
    #[test]
    fn a_login_handoff_says_login_and_both_transports_send_the_one_line() {
        for login in [false, true] {
            let line = handoff_request(login);
            assert!(line.ends_with('\n'), "one line per request");
            let req = parse_request(&line).expect("the handoff line parses");
            assert_eq!(req.cmd, "app.open");
            assert_eq!(handoff_is_login(&req.args), login, "{line}");
        }
        // Only an explicit `true` is a login; `rex open`'s bare args raise the window.
        assert!(!handoff_is_login(&json!({})));
        assert!(!handoff_is_login(&json!({ "login": "true" })));
        assert!(!handoff_is_login(&json!({ "login": false })));

        let src = crate::core::copy_scan::production_source(include_str!("cli_server.rs"));
        assert!(src.contains("sock.write_all(handoff_request(login).as_bytes())"), "the unix handoff");
        assert!(src.contains("hand_off_to_app_pipe(&dir, &handoff_request(login))"), "the Windows handoff");
        let pipe = crate::core::copy_scan::production_source(include_str!("platform/windows/app_pipe.rs"));
        assert!(pipe.contains("pipe.write_all(request.as_bytes())"));
        assert!(!pipe.contains("app.open"), "the pipe builds its own request again");
    }

    #[test]
    fn parse_request_defaults_args_and_rejects_garbage() {
        let req = parse_request("{\"cmd\":\"status\"}\n").expect("bare cmd parses");
        assert_eq!(req.cmd, "status");
        assert!(req.args.is_null());
        assert!(parse_request("not json").is_err());
        assert!(parse_request("{\"args\":{}}").is_err(), "cmd is required");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn bind_locks_perms_and_replaces_a_stale_socket() {
        let path = scratch_sock("perms");
        let first = bind(&path).expect("first bind");
        let mode = std::fs::metadata(&path).expect("socket exists").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "socket must be private to the user");
        // A LIVE listener is never bound over: that is the other instance's
        // lock, and taking it made `rex` talk to the wrong process.
        let stolen = bind(&path);
        assert!(stolen.is_err(), "bound over a socket something was listening on");
        assert!(first.local_addr().is_ok(), "the first listener must survive the attempt");
        drop(first); // socket FILE stays — the stale-crash shape
        assert!(path.exists(), "dropped listener leaves the file (stale)");
        let _second = bind(&path).expect("rebind over a stale file");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn serve_round_trips_one_line_per_connection() {
        let path = scratch_sock("echo");
        let listener = bind(&path).expect("bind");
        tokio::spawn(serve(listener, |line: String, _p: Progress| async move {
            format!("echo:{}", line.trim())
        }));
        let mut stream = tokio::net::UnixStream::connect(&path).await.expect("connect");
        stream.write_all(b"{\"cmd\":\"x\"}\n").await.expect("write");
        let (read, _w) = stream.into_split();
        let mut line = String::new();
        BufReader::new(read).read_line(&mut line).await.expect("read");
        assert_eq!(line.trim(), "echo:{\"cmd\":\"x\"}");
        let _ = std::fs::remove_file(&path);
    }

    /// **Streaming is opt-in, and the envelope is always the last line.**
    ///
    /// The compatibility half is the point: an older `rex` reads exactly ONE
    /// line and treats it as the reply, so a server that streamed unasked would
    /// hand it a progress record as the result of the command. The flag is the
    /// client saying it can read more than one line, and a request without it
    /// must get the old framing byte for byte.
    #[tokio::test]
    #[cfg(unix)]
    async fn progress_lines_only_go_to_a_client_that_asked_for_them() {
        let path = scratch_sock("stream");
        let listener = bind(&path).expect("bind");
        tokio::spawn(serve(listener, |line: String, p: Progress| async move {
            // Every request reports progress unconditionally; the PROTOCOL
            // decides whether anyone hears it, which is what keeps a command
            // from having to know who is asking.
            p.send(json!({ "step": 1 }));
            p.send(json!({ "step": 2 }));
            json!({ "ok": true, "data": line.trim() }).to_string()
        }));

        async fn exchange(path: &std::path::Path, request: &str) -> Vec<String> {
            let mut stream = tokio::net::UnixStream::connect(path).await.expect("connect");
            stream.write_all(format!("{request}\n").as_bytes()).await.expect("write");
            let (read, _w) = stream.into_split();
            let mut lines = Vec::new();
            let mut reader = BufReader::new(read);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => lines.push(line.trim().to_string()),
                }
            }
            lines
        }

        // No flag: exactly one line, and it is the envelope — the old contract.
        let plain = exchange(&path, "{\"cmd\":\"x\"}").await;
        assert_eq!(
            plain.len(),
            1,
            "a client that did not ask for streaming got {} lines — an older `rex` reads the \
             first one as the reply, so this hands it a progress record as the result: {plain:?}",
            plain.len()
        );
        assert!(plain[0].contains("\"ok\":true"), "the single line must be the envelope");

        // With the flag: the progress records IN ORDER, then the envelope LAST.
        let streamed = exchange(&path, "{\"cmd\":\"x\",\"stream\":true}").await;
        assert_eq!(streamed.len(), 3, "expected two progress lines and one envelope: {streamed:?}");
        assert!(streamed[0].contains("\"step\":1"), "first record out of order: {streamed:?}");
        assert!(streamed[1].contains("\"step\":2"), "second record out of order: {streamed:?}");
        assert!(
            streamed[2].contains("\"ok\":true"),
            "the envelope must be LAST — it is how a reader that understands none of the \
             progress records still knows the exchange is over: {streamed:?}"
        );
        // …and progress is identifiable by KEY, not by position: that is what
        // lets a client skip records it does not understand.
        for record in &streamed[..2] {
            let v: Value = serde_json::from_str(record).expect("progress is JSON");
            assert!(v.get("progress").is_some(), "a progress line must be keyed `progress`");
            assert!(v.get("ok").is_none(), "a progress line must never look like an envelope");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    #[cfg(unix)] // the idle-peer leg uses a unix socket pair
    async fn read_request_line_bounds_size_and_timeout() {
        // A normal request fits well under the cap and round-trips intact.
        let ok = read_request_line(&b"{\"cmd\":\"x\"}\n"[..], 1024, Duration::from_secs(5)).await;
        assert_eq!(ok.as_deref().map(str::trim), Some("{\"cmd\":\"x\"}"));

        // A long line with no newline is bounded to the cap (memory guard) — it
        // never grows past max_bytes even though 100 bytes were offered.
        let long = [b'a'; 100];
        let capped = read_request_line(&long[..], 8, Duration::from_secs(5)).await;
        assert_eq!(capped.as_deref().map(str::len), Some(8), "bounded to the cap");

        // A peer that connects but never sends times out to None (no leaked
        // task) — the handler is never reached.
        let (a, _b) = tokio::net::UnixStream::pair().expect("pair");
        let timed = read_request_line(a, 1024, Duration::from_millis(50)).await;
        assert!(timed.is_none(), "idle connection times out to None");
    }

    #[tokio::test]
    async fn handle_request_reports_missing_state_and_unknown_cmd_as_errors() {
        // A mock app with NO managed state = the init-failed / still-starting
        // shape; the reply must be an error envelope, never a panic.
        let app = tauri::test::mock_app();
        // A SAMPLE of routed commands reaches the state check (proving those
        // arms exist); an unrouted one must say so instead. This list is
        // hand-written and covers a third of the arms — the guard that
        // enumerates EVERY arm is `every_dispatch_arm_runs_the_command_the_ui_runs`
        // (9a2108c), which reads the match itself; this one is the
        // envelope-shape check.
        for cmd in [
            "status", "start", "stop", "site.list", "site.create", "site.delete", "site.info",
            "site.login", "logs.targets", "logs.tail", "logs.list", "doctor", "db.export",
            "db.import", "php.list", "php.default", "php.installed", "site.php", "site.xdebug",
            "wp.plugins", "wp.plugin.install", "wp.plugin.activate", "wp.themes",
            "wp.theme.install", "wp.users", "wp.user.create", "wp.user.password", "wp.user.role",
            "wp.user.delete",
            "repo.list", "repo.watch.start", "repo.watch.stop", "repo.add", "repo.op",
            "repo.run",
        ] {
            let reply =
                handle_request(app.handle(), format!("{{\"cmd\":\"{cmd}\"}}"), Progress::none()).await;
            let v: Value = serde_json::from_str(&reply).expect("valid envelope");
            assert_eq!(v["ok"], false);
            assert!(v["error"].as_str().unwrap().contains("starting"), "{cmd}: {v}");
        }
        let reply = handle_request(app.handle(), "{\"cmd\":\"bogus\"}".into(), Progress::none()).await;
        let v: Value = serde_json::from_str(&reply).expect("valid envelope");
        assert!(v["error"].as_str().unwrap().contains("unknown command"));
        let reply = handle_request(app.handle(), "not json at all".into(), Progress::none()).await;
        let v: Value = serde_json::from_str(&reply).expect("valid envelope");
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("bad request"));
    }
}
