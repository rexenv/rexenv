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
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

pub const SOCKET_FILE: &str = "rexenv-cli.sock";

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
            let mut line = String::new();
            if BufReader::new(read).read_line(&mut line).await.is_ok()
                && !line.trim().is_empty()
            {
                let response = handler(line).await;
                let _ = write.write_all(response.as_bytes()).await;
                let _ = write.write_all(b"\n").await;
            }
        });
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
        "site.list" => {
            let state = app_state(app)?;
            let sites = commands::sites::list_sites(state.clone())?;
            let serving = commands::sites::sites_serving(state.clone())?;
            Ok(json!({ "sites": to_value(&sites)?, "serving": to_value(&serving)? }))
        }
        // Site create: exactly what the New Site dialog submits — an empty
        // `path` (the backend derives it under the sites folder), WordPress
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
                    crate::core::php::list_versions(&conn)?
                        .into_iter()
                        .find(|v| v.is_default)
                        .map(|v| v.minor)
                        .ok_or_else(|| Error::Other("no default PHP version".into()))?
                }
            };
            let site = crate::state::models::NewSite {
                name: a.name.unwrap_or_else(|| a.domain.clone()),
                domain: a.domain,
                site_type: a.site_type.unwrap_or(crate::state::models::SiteType::Wordpress),
                php_version,
                web_server: a.server.unwrap_or(crate::state::models::WebServer::Nginx),
                path: String::new(),
                db_engine: a.db.unwrap_or(crate::state::models::SiteDbEngine::Mysql),
            };
            let created = commands::sites::create_site(state.clone(), site, None, None).await?;
            to_value(&created)
        }
        // Site delete: the CLI resolves domain → id via site.list first; this
        // arm is by-id like the UI row action (tunnel stop + DB drop + files).
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
        other => Err(Error::Other(format!(
            "unknown command: {other} (this rex may be newer than the running app)"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch_sock(name: &str) -> std::path::PathBuf {
        // Keep it short: unix socket paths cap at ~104 bytes.
        std::env::temp_dir().join(format!("rexcli-{}-{name}.sock", std::process::id()))
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
    async fn handle_request_reports_missing_state_and_unknown_cmd_as_errors() {
        // A mock app with NO managed state = the init-failed / still-starting
        // shape; the reply must be an error envelope, never a panic.
        let app = tauri::test::mock_app();
        // Every ROUTED command reaches the state check (proving the arm
        // exists); an unrouted one must say so instead.
        for cmd in ["status", "start", "stop", "site.list", "site.create", "site.delete"] {
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
