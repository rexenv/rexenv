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
        "wp.plugin.install" => {
            let state = app_state(app)?;
            let (id, slug) = (need_str(&args, "id", cmd)?, need_str(&args, "slug", cmd)?);
            let activate = args["activate"].as_bool().unwrap_or(false);
            commands::wordpress::wp_plugin_install(state.clone(), id, slug, activate).await?;
            Ok(Value::Null)
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
                    commands::wordpress::wp_plugin_update(state.clone(), id, names).await?
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
        "wp.theme.install" => {
            let state = app_state(app)?;
            let (id, slug) = (need_str(&args, "id", cmd)?, need_str(&args, "slug", cmd)?);
            let activate = args["activate"].as_bool().unwrap_or(false);
            commands::wordpress::wp_theme_install(state.clone(), id, slug, activate).await?;
            Ok(Value::Null)
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
                commands::wordpress::wp_theme_update(state.clone(), id, names).await?;
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
                commands::wordpress::wp_core_update(state.clone(), need_str(&args, "id", cmd)?)
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
            Ok(to_value(&commands::mail::mailpit_messages(state.clone(), query).await?)?)
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
            let tunnels = app
                .try_state::<commands::tunnels::Tunnels>()
                .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?;
            Ok(json!({ "tunnels": to_value(&commands::tunnels::tunnels_status(tunnels).await?)? }))
        }
        "tunnel.start" | "tunnel.stop" => {
            let state = app_state(app)?;
            let tunnels = app
                .try_state::<commands::tunnels::Tunnels>()
                .ok_or_else(|| Error::Other("tunnel registry not ready".into()))?;
            let id = need_str(&args, "id", cmd)?;
            if cmd == "tunnel.start" {
                let info = commands::tunnels::start_tunnel(state.clone(), tunnels, id).await?;
                Ok(to_value(&info)?)
            } else {
                commands::tunnels::stop_tunnel(state.clone(), tunnels, id).await?;
                Ok(Value::Null)
            }
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
            let wire_ours = crate::core::proxy::edge_answers_as_ours(
                crate::core::adminer::ADMINER_HOST,
                crate::core::proxy::DEFAULT_HTTPS_PORT,
            )
            .await;
            let edge_conflict = (caddy_running && !wire_ours).then(|| {
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
            Ok(json!({
                "app": to_value(&commands::system::app_info())?,
                "dns": to_value(&dns)?,
                "services": to_value(&services)?,
                "edge": { "running": caddy_running, "wireOurs": wire_ours, "conflict": edge_conflict },
                "portConflicts": port_conflicts,
                "cli": to_value(&cli)?,
            }))
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
        for cmd in [
            "status", "start", "stop", "site.list", "site.create", "site.delete", "site.info",
            "site.login", "logs.targets", "logs.tail", "logs.list", "doctor", "db.export",
            "db.import", "php.list", "php.default", "php.installed", "site.php", "site.xdebug",
            "wp.plugins", "wp.plugin.install", "wp.plugin.activate", "wp.themes",
            "wp.theme.install", "wp.users", "wp.user.create", "wp.user.password", "wp.user.role",
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
