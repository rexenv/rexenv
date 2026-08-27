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
//! Protocol: one connection = one request = one reply, newline-delimited JSON.
//! Request `{"cmd":"status","args":{…}}` → reply `{"ok":true,"data":…}` or
//! `{"ok":false,"error":"…"}`. Long commands hold the connection until done.

use crate::commands;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
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
}

pub fn parse_request(line: &str) -> Result<Request> {
    serde_json::from_str(line.trim()).map_err(|e| Error::Other(format!("bad request: {e}")))
}

/// Bind the private socket. A stale file is unlinked first (socket files
/// outlive a crash — same lesson as `admin_alive`: only a connect tells the
/// truth, and the CLI treats connect-refused as "app not running"). Perms are
/// locked to `0600` before the first request is served.
pub fn bind(path: &Path) -> Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Accept loop: one JSON line in, one JSON line out, close. Generic over the
/// handler so framing is testable without an app.
pub async fn serve<F, Fut>(listener: UnixListener, handler: F)
where
    F: Fn(String) -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = String> + Send,
{
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue; // transient accept error; the socket itself stays bound
        };
        let handler = handler.clone();
        tokio::spawn(async move {
            let (read, mut write) = stream.into_split();
            if let Some(line) =
                read_request_line(read, MAX_REQUEST_BYTES, REQUEST_READ_TIMEOUT).await
            {
                let response = handler(line).await;
                let _ = write.write_all(response.as_bytes()).await;
                let _ = write.write_all(b"\n").await;
            }
        });
    }
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
pub async fn handle_request<R, M>(app: &M, line: String) -> String
where
    R: tauri::Runtime,
    M: Manager<R>,
{
    let result = match parse_request(&line) {
        Ok(req) => dispatch(app, &req.cmd, req.args).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(data) => json!({"ok": true, "data": data}).to_string(),
        Err(e) => json!({"ok": false, "error": e.to_string()}).to_string(),
    }
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

/// A repo job has SETTLED for CLI purposes: nothing runs and nothing the job
/// itself would still run is pending. Offered install steps stay `pending`
/// until the user asks — they never block settling (`--install` runs them as
/// their own settled-waits).
fn repo_job_settled(st: &commands::repo::RepoJobState, waiting_for: Option<&str>) -> bool {
    let status_of = |k: &str| {
        st.steps.iter().find(|x| x.key == k).map(|x| x.status.clone()).unwrap_or_default()
    };
    if let Some(key) = waiting_for {
        return !matches!(status_of(key).as_str(), "pending" | "running");
    }
    match st.op.as_str() {
        // add = clone → detect in ONE worker; detect settles the job (a
        // failed/cancelled clone settles it early — detect never runs).
        "add" => {
            matches!(status_of("clone").as_str(), "failed" | "cancelled")
                || !matches!(status_of("detect").as_str(), "pending" | "running")
        }
        op => !matches!(status_of(op).as_str(), "pending" | "running"),
    }
}

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
async fn dispatch<R, M>(app: &M, cmd: &str, args: Value) -> Result<Value>
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
            Ok(json!({ "services": to_value(&services)?, "dns": to_value(&dns)? }))
        }
        // Global lifecycle — exactly the footer buttons. `rex restart` is the
        // CLI sending `stop` then `start`; no third code path exists.
        "start" => {
            commands::services::start_services(app_state(app)?).await?;
            Ok(Value::Null)
        }
        "stop" => {
            commands::services::stop_services(app_state(app)?).await?;
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
                    let conn = state
                        .db
                        .lock()
                        .map_err(|_| Error::Other("database lock poisoned".into()))?;
                    let value = crate::state::store::get_setting(&conn, &key)?;
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
            Ok(json!({ "sites": to_value(&sites)?, "serving": to_value(&serving)? }))
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
                name: a.name.unwrap_or_else(|| a.domain.clone()),
                domain: a.domain,
                site_type: a.site_type.unwrap_or(crate::state::models::SiteType::Wordpress),
                php_version,
                web_server: a.server.unwrap_or(crate::state::models::WebServer::Nginx),
                // Empty = create the docroot under the sites folder (the
                // dialog's default); a value = link that existing folder.
                path: a.path.clone().unwrap_or_default(),
                db_engine: a.db.unwrap_or(crate::state::models::SiteDbEngine::Mysql),
                // `rex site create` has no repo flag yet — the app is the only
                // caller that can clone (docs/PLAN-git-site-clone.md, CLI in a
                // later stage).
                git_url: String::new(),
                git_ref: None,
                git_migrate: true,
                git_build_assets: false,
                // `rex site create` has no starter-database flag yet — the
                // dialog is the only caller that asks. False keeps the CLI's
                // blank site exactly what it has always been: files, no engine.
                starter_db: false,
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
            let created = commands::sites::create_site(
                app.app_handle().clone(),
                state.clone(),
                provision_jobs_state(app)?,
                site,
                None,
                blueprint_id,
            )
            .await?;
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
            let serving = commands::sites::sites_serving(state.clone())?
                .into_iter()
                .find(|r| r.domain == site.domain)
                .map(|r| r.serving)
                .unwrap_or(false);
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
            Ok(json!({
                "site": to_value(&site)?,
                "serving": serving,
                "resources": to_value(&resources)?,
                "cert": to_value(&cert)?,
                "wp": to_value(&wp)?,
            }))
        }
        // Magic wp-admin login link — the Sites row action.
        "site.login" => {
            let state = app_state(app)?;
            let id = args["id"]
                .as_str()
                .ok_or_else(|| Error::Other("site.login needs an id".into()))?
                .to_string();
            let url = commands::wordpress::wp_admin_login_url(state.clone(), id).await?;
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
            while last.status == "running" && std::time::Instant::now() < deadline {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                match commands::site_provision::site_provision_active(
                    provision_jobs_state(app)?,
                    Some(domain.clone()),
                )
                .await?
                {
                    Some(snap) => last = snap,
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
            let dir = state.platform.paths().log_dir()?;
            let mut files: Vec<Value> = std::fs::read_dir(&dir)
                .map_err(Error::from)?
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let meta = e.metadata().ok()?;
                    meta.is_file().then(|| json!({ "key": name, "bytes": meta.len() }))
                })
                .collect();
            files.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
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
        "version" => Ok(to_value(&commands::system::app_info())?),
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
                    || crate::core::ports::is_free(req.port, req.proto)
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
            Ok(json!({
                "app": to_value(&commands::system::app_info())?,
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
                // Borrowed resolver files another tool reclaimed — invisible to
                // every other probe, because our resolver keeps answering.
                "resolverDrift": to_value(&resolver_drift)?,
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

/// Spawn the listener at app startup. Failure is logged, never fatal — the
/// app works without its CLI.
pub fn spawn(app: tauri::AppHandle) {
    let path = match crate::platform::current().paths().config_dir() {
        Ok(dir) => dir.join(SOCKET_FILE),
        Err(e) => {
            log::error!("cli: no config dir for the socket: {e}");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        let listener = match bind(&path) {
            Ok(l) => l,
            Err(e) => {
                log::error!("cli: could not bind {}: {e}", path.display());
                return;
            }
        };
        log::info!("cli: listening on {}", path.display());
        serve(listener, move |line| {
            let app = app.clone();
            async move { handle_request(&app, line).await }
        })
        .await;
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
        let deps = manifest
            .split("[dependencies]")
            .nth(1)
            .and_then(|d| d.split("\n[").next())
            .expect("cli/Cargo.toml has a [dependencies] section");

        for banned in ["rexenv", "rusqlite", "tauri", "path ="] {
            assert!(
                !deps.contains(banned),
                "`cli/Cargo.toml` now depends on `{banned}`.\n\n\
                 The rex CLI is deliberately a thin socket client with ONE dependency \
                 (serde_json). Linking the app library — or SQLite directly — would let a `rex` \
                 command open the same database the running app has open, and two writers on one \
                 SQLite file is the corruption class this separation exists to make IMPOSSIBLE \
                 rather than merely avoided (ledger #54).\n\n\
                 If you need something the app knows, add a command to `cli_server.rs` and ask \
                 for it over the socket — that is the whole design, and it is also what keeps \
                 the CLI and the UI on the same code path (#57).\n\n\
                 [dependencies] is currently:{deps}"
            );
        }
        // The canary: a manifest we failed to read would pass every check above.
        assert!(
            deps.contains("serde_json"),
            "the [dependencies] section did not parse as expected — this guard would pass on an \
             empty string. Fix the parse before trusting the result.{deps}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

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
        for line in THIS.lines() {
            if line.trim_start().starts_with("match cmd {") {
                inside = true;
                continue;
            }
            if !inside {
                continue;
            }
            if line.starts_with("        _ =>") {
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

    #[test]
    fn parse_request_defaults_args_and_rejects_garbage() {
        let req = parse_request("{\"cmd\":\"status\"}\n").expect("bare cmd parses");
        assert_eq!(req.cmd, "status");
        assert!(req.args.is_null());
        assert!(parse_request("not json").is_err());
        assert!(parse_request("{\"args\":{}}").is_err(), "cmd is required");
    }

    #[tokio::test]
    async fn bind_locks_perms_and_replaces_a_stale_socket() {
        let path = scratch_sock("perms");
        let first = bind(&path).expect("first bind");
        let mode = std::fs::metadata(&path).expect("socket exists").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "socket must be private to the user");
        drop(first); // socket FILE stays — the stale-crash shape
        assert!(path.exists(), "dropped listener leaves the file (stale)");
        let _second = bind(&path).expect("rebind over a stale file");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn serve_round_trips_one_line_per_connection() {
        let path = scratch_sock("echo");
        let listener = bind(&path).expect("bind");
        tokio::spawn(serve(listener, |line: String| async move {
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

    #[tokio::test]
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
        // Every ROUTED command reaches the state check (proving the arm
        // exists); an unrouted one must say so instead.
        for cmd in [
            "status", "start", "stop", "site.list", "site.create", "site.delete", "site.info",
            "site.login", "logs.targets", "logs.tail", "logs.list", "doctor", "db.export",
            "db.import", "php.list", "php.default", "php.installed", "site.php", "site.xdebug",
            "wp.plugins", "wp.plugin.install", "wp.plugin.activate", "wp.themes",
            "wp.theme.install", "wp.users", "wp.user.create", "wp.user.password", "wp.user.role",
            "repo.list", "repo.watch.start", "repo.watch.stop", "repo.add", "repo.op",
            "repo.run",
        ] {
            let reply =
                handle_request(app.handle(), format!("{{\"cmd\":\"{cmd}\"}}")).await;
            let v: Value = serde_json::from_str(&reply).expect("valid envelope");
            assert_eq!(v["ok"], false);
            assert!(v["error"].as_str().unwrap().contains("starting"), "{cmd}: {v}");
        }
        let reply = handle_request(app.handle(), "{\"cmd\":\"bogus\"}".into()).await;
        let v: Value = serde_json::from_str(&reply).expect("valid envelope");
        assert!(v["error"].as_str().unwrap().contains("unknown command"));
        let reply = handle_request(app.handle(), "not json at all".into()).await;
        let v: Value = serde_json::from_str(&reply).expect("valid envelope");
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("bad request"));
    }
}
