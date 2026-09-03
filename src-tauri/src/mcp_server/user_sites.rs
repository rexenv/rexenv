//! Parity tools — the THIRD registry: tools that act on the USER's own sites
//! and on the stack, each behind a scope grant the user gave in the app
//! (`docs/PLAN-mcp-parity.md` §3, `core::agent_grants`).
//!
//! **Why a third module, not more rows in `scratch.rs`.** The registry IS the
//! capability: a `Read` tool's handler holds a `ReadCtx` (no mutator), a
//! `Scratch` tool's holds a `ScratchCtx` (whose only door to a site is the
//! `origin`-checked witness, #208). A tool that changes the user's OWN site is
//! a different capability from both, and putting it in `scratch.rs` would have
//! meant that module's context growing a door to user sites — which is the one
//! thing #208 exists to make impossible. So this module has its own context,
//! [`UserCtx`], whose only door to any site is [`UserCtx::claim`]: the
//! `Granted<S>` witness (#471), minted by a gate that reads the recorded grant
//! and refuses a scratch site outright (those are the agent's; the scratch
//! tools apply and no grant is needed).
//!
//! **The capability, precisely.** A parity handler can reach exactly the site
//! (or the stack) it claimed, for exactly the scope in its witness's type, and
//! only while the user's "Let agents manage my own sites" switch is on AND a
//! live grant for that client exists. Off, every tool here refuses BY NAME,
//! pointing at the switch — the registry stays listed so `tools/list` is stable
//! and the leak sweep covers it, exactly as the mail tools behave under theirs.
//!
//! **What this is NOT (#197, restated for the widest surface yet):** a
//! sandbox. A grant bounds which site and which verb. Code an agent runs inside
//! a granted site — `wp_run` under `run`, a plugin it activated under `manage`
//! — runs as the user. The grant dialog says so in those words.
//!
//! The plumbing landed BEFORE the first tool did, as it did for `scratch.rs`:
//! a parity tool arrives into a structure that already refuses to let it be
//! registered untested, unlisted, unswept or unranked.

use crate::core::agent_grants::{self, scope, Claimed, Granted, Scope};
use crate::core::sites::Ownership;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{MultisiteMode, NewSite, Site, SiteDbEngine, SiteType, WebServer};
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

/// A parity tool's async result — the same shape as both other registries'.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;

/// A parity tool handler: `(UserCtx, args, acted) -> future of JSON`.
pub type ToolHandler = for<'a> fn(
    UserCtx<'a>,
    &'a Value,
    &'a super::feed::ActedTarget,
) -> ToolFuture<'a>;

/// One parity tool. The same required-field discipline as the other two
/// registries (`sweep_args`, `summarise`), plus **`scope`: the tool is ranked
/// where it is declared** (PLAN §2.4 — "you cannot register a tool without
/// ranking it"). The handler's `Granted<S>` type is what ENFORCES the scope;
/// this field is what `tools/list` annotations and the guards read, and a test
/// holds the two together.
pub struct UserTool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    /// The arguments the secret-leak sweep calls this tool with. REQUIRED.
    pub sweep_args: fn(site_id: &str) -> Value,
    /// What this call was ABOUT, for the feed (v30). REQUIRED; `None` when the
    /// tool's name and target already say it.
    pub summarise: fn(&Value) -> Option<String>,
    /// The scope this tool demands — the one its handler claims.
    pub scope: Scope,
    pub handler: ToolHandler,
}

/// The closed set of parity tools. The registry, the dispatch route, the sweep
/// coverage and the disjointness guard existed before the first tool did, so no
/// tool here arrived unranked or unswept.
pub fn registry() -> &'static [UserTool] {
    REGISTRY
}

static REGISTRY: &[UserTool] = &[
    UserTool {
        name: "site_create",
        description: "Create a real site for the person you're working with — the same kind the \
                      app's New Site dialog makes: `type` is `wordpress` (installed and ready to \
                      log in), `php` (a blank PHP site) or `laravel` (a fresh skeleton). Takes \
                      `name`, `domain` (a full hostname like `shop.rex`), `type`, and optionally \
                      `php` (a minor like `8.3`), `server` (nginx / apache / frankenphp), \
                      `db_engine` (mysql / mariadb), `blueprint` (a saved blueprint's name), and for \
                      WordPress `wp` ({title, admin_user, admin_email, admin_password, language}) \
                      and `multisite` (subdomain / subdirectory); for a blank PHP site `starter_db` \
                      (true creates a database with a sample table). Needs the user's `manage` \
                      permission for rexenv itself — asked for in the app if it is missing. This \
                      can take a minute or two. Never links a folder or clones a repository; those \
                      are separate tools. Admin credentials come back ONCE, in the reply.",
        input_schema: create_params,
        // The sweep runs with the sites switch OFF, so this refuses by name
        // before touching anything — and the refusal is what gets swept.
        sweep_args: |_id| json!({ "name": "sweep-probe", "domain": "sweep-probe.rex", "type": "php" }),
        summarise: |args| args.get("type").and_then(Value::as_str).map(str::to_string),
        scope: Scope::Manage,
        handler: site_create,
    },
    UserTool {
        name: "site_delete",
        description: "Delete one of the user's own sites — its files (if rexenv made them), its \
                      database and its configuration. Takes `site_id`. Needs the user's `destroy` \
                      permission on that site, which they can only give for the current session; \
                      it is asked for in the app if missing. A scratch site the agent created is \
                      refused here — use scratch_delete_site for those.",
        input_schema: || json!({
            "type": "object",
            "properties": { "site_id": { "type": "string", "description": "The site's id (from list_sites)." } },
            "required": ["site_id"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id }),
        summarise: |_| None,
        scope: Scope::Destroy,
        handler: site_delete,
    },
    UserTool {
        name: "site_configure",
        description: "Change how one of the user's own sites is set up — the things the site's \
                      Settings tab does. Takes `site_id` and `action`, plus the action's field: \
                      `rename` {name}; `php` {version, a minor like `8.3`}; `server` {server: \
                      nginx / apache / frankenphp}; `xdebug` {enabled}; `env_set` {key, value} and \
                      `env_unset` {key} (per-request environment variables — values are never read \
                      back, only the names); `domain` {domain} (changes the primary hostname; on \
                      WordPress rexenv backs the database up to the user's Downloads first and \
                      rewrites URLs); `add_domain` / `remove_domain` {domain} (extra hostnames the \
                      site also answers on); `move` {dest_parent} (relocate a docroot rexenv \
                      manages); `relink` {path} (re-point a linked site at a folder the user moved); \
                      `regenerate_cert`. Needs the user's `manage` permission on that site — asked \
                      for in the app if missing. Every change is the app's own operation with the \
                      app's own refusals; the reply is the site as it now is.",
        input_schema: configure_params,
        sweep_args: |id| json!({ "site_id": id, "action": "rename", "name": "sweep-probe" }),
        // The ACTION is the verb; the value (a name, a domain, an env value) is
        // never summarised — the clamp would refuse most of them anyway.
        summarise: |args| args.get("action").and_then(Value::as_str).map(str::to_string),
        scope: Scope::Manage,
        handler: site_configure,
    },
    UserTool {
        name: "site_restart",
        description: "Restart one of the user's own sites so it picks up a change. Takes `site_id` \
                      and optional `pool` (default false). A site on the shared web server has no \
                      process of its own: rexenv rebuilds its config and reloads the web tier. A site \
                      with its own backend (FrankenPHP, Apache) gets that backend restarted. `pool: \
                      true` ALSO restarts the PHP-FPM pool for the site's PHP version — which stops \
                      every other site on that version for a moment, so the reply says how many; \
                      leave it false unless the user asked. Needs `manage` on the site.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "pool": { "type": "boolean", "description": "Also restart the shared PHP pool. Affects every site on that PHP version." }
            },
            "required": ["site_id"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "pool": false }),
        summarise: |_| None,
        scope: Scope::Manage,
        handler: site_restart,
    },
    UserTool {
        name: "site_retry",
        description: "Finish a site whose setup did not complete (listed as \"setup incomplete\" in \
                      rexenv). Takes `site_id`. Re-runs only the steps that did not finish; a site \
                      whose setup is complete is refused. Blocks until it settles (up to several \
                      minutes for a WordPress download). Needs `manage` on the site.",
        input_schema: || json!({
            "type": "object",
            "properties": { "site_id": { "type": "string" } },
            "required": ["site_id"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id }),
        summarise: |_| None,
        scope: Scope::Manage,
        handler: site_retry,
    },
];

fn configure_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The site's id (from list_sites)." },
            "action": {
                "type": "string",
                "enum": ["rename", "php", "server", "xdebug", "env_set", "env_unset", "domain", "add_domain", "remove_domain", "move", "relink", "regenerate_cert"]
            },
            "name": { "type": "string", "description": "rename: the new display name." },
            "version": { "type": "string", "description": "php: a minor like `8.3`." },
            "server": { "type": "string", "enum": ["nginx", "apache", "frankenphp"], "description": "server: the web server to switch to." },
            "enabled": { "type": "boolean", "description": "xdebug: on or off." },
            "key": { "type": "string", "description": "env_set / env_unset: the variable's name." },
            "value": { "type": "string", "description": "env_set: the value." },
            "domain": { "type": "string", "description": "domain / add_domain / remove_domain: the hostname." },
            "dest_parent": { "type": "string", "description": "move: the folder to move the site's docroot INTO." },
            "path": { "type": "string", "description": "relink: the folder the user moved the site to." }
        },
        "required": ["site_id", "action"],
        "additionalProperties": false
    })
}

fn create_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Display name, e.g. `My Shop`." },
            "domain": { "type": "string", "description": "Full hostname, e.g. `shop.rex`. The TLD must be one rexenv already resolves." },
            "type": { "type": "string", "enum": ["wordpress", "php", "laravel"] },
            "php": { "type": "string", "description": "PHP minor, e.g. `8.3`. Defaults to rexenv's default." },
            "server": { "type": "string", "enum": ["nginx", "apache", "frankenphp"], "description": "Defaults to nginx." },
            "db_engine": { "type": "string", "enum": ["mysql", "mariadb"], "description": "Defaults to mysql." },
            "blueprint": { "type": "string", "description": "The NAME of a saved blueprint (WordPress only)." },
            "multisite": { "type": "string", "enum": ["subdomain", "subdirectory"], "description": "WordPress only: convert to a network after install." },
            "starter_db": { "type": "boolean", "description": "Blank PHP only: create a database with a sample table and a db.php." },
            "wp": {
                "type": "object",
                "description": "WordPress install options; every field optional.",
                "properties": {
                    "title": { "type": "string" },
                    "admin_user": { "type": "string" },
                    "admin_email": { "type": "string" },
                    "admin_password": { "type": "string", "description": "Omit to use rexenv's local-dev default; the reply says which." },
                    "language": { "type": "string", "description": "A WordPress locale like `de_DE`." }
                },
                "additionalProperties": false
            }
        },
        "required": ["name", "domain", "type"],
        "additionalProperties": false
    })
}

/// This registry's tools as MCP descriptors, for the union `tools/list`.
pub fn tools_list_descriptors() -> Value {
    Value::Array(
        registry()
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": (t.input_schema)(),
                    // MCP tool annotations — UX hints a client may use to
                    // confirm before a destructive call. The spec says clients
                    // must treat them as untrusted, and so does rexenv: the
                    // scope the handler's witness type enforces is the boundary;
                    // this is the same fact said in the protocol's vocabulary.
                    "annotations": {
                        "readOnlyHint": t.scope == Scope::Read,
                        "destructiveHint": t.scope == Scope::Destroy,
                    },
                })
            })
            .collect(),
    )
}

/// A parity tool's async result over the app's own site operations.
pub type OpFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The app's own site operations, reached through the app handle.
///
/// A trait object for the reason `scratch::SiteCreator` is one: the commands
/// are generic over `tauri::Runtime` and a `static` registry of fn pointers
/// cannot be, so the runtime is erased HERE and every parity tool runs exactly
/// the code the app and the CLI run — the one-brain rule, kept across the
/// erasure. Nothing in this module re-implements a site operation.
pub trait SiteOps: Send + Sync {
    /// The app's create — the streamed provision job, blocking until settled.
    fn create<'a>(
        &'a self,
        new: NewSite,
        wp: Option<crate::core::wordpress::InstallOptions>,
        blueprint_id: Option<String>,
        ownership: Ownership,
    ) -> OpFuture<'a, std::result::Result<Site, crate::commands::sites::CreateFailure>>;
    /// The app's full delete (tunnel stop, DB drop by provenance, teardown by
    /// `docroot_managed`, agent accounts, reload).
    fn delete<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>>;
    /// `wp core multisite-convert` through the app's command (share-guarded).
    fn multisite_convert<'a>(&'a self, id: String, mode: String) -> OpFuture<'a, Result<()>>;
    // ── the site's Settings tab, one method per app command ──
    fn rename<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<Option<Site>>>;
    fn change_domain<'a>(&'a self, id: String, domain: String) -> OpFuture<'a, Result<crate::commands::sites::DomainChange>>;
    fn add_domain<'a>(&'a self, id: String, domain: String) -> OpFuture<'a, Result<Vec<String>>>;
    fn remove_domain<'a>(&'a self, id: String, domain: String) -> OpFuture<'a, Result<Vec<String>>>;
    fn set_php<'a>(&'a self, id: String, version: String) -> OpFuture<'a, Result<Option<Site>>>;
    fn set_server<'a>(&'a self, id: String, server: WebServer) -> OpFuture<'a, Result<Option<Site>>>;
    fn set_xdebug<'a>(&'a self, id: String, enabled: bool) -> OpFuture<'a, Result<Option<Site>>>;
    fn list_env<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::commands::sites::EnvVarInput>>>;
    fn set_env<'a>(&'a self, id: String, vars: Vec<crate::commands::sites::EnvVarInput>) -> OpFuture<'a, Result<()>>;
    fn move_docroot<'a>(&'a self, id: String, dest_parent: String) -> OpFuture<'a, Result<Site>>;
    fn relink<'a>(&'a self, id: String, path: String) -> OpFuture<'a, Result<Site>>;
    fn regenerate_cert<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>>;
    fn restart<'a>(&'a self, id: String, pool: bool) -> OpFuture<'a, Result<crate::commands::sites::SiteRestartReport>>;
    fn retry<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<crate::commands::site_provision::SiteProvisionState>>;
}

/// What a parity handler can reach: app state, the app's own site operations,
/// and — for any SITE or for the stack — only through [`UserCtx::claim`], which
/// yields a `Granted<S>` or a refusal an agent can act on.
///
/// `Copy` (a single `&AppState`), like the other two contexts, so async handlers
/// take it by value.
#[derive(Clone, Copy)]
pub struct UserCtx<'a> {
    state: &'a AppState,
    /// The MCP client's self-reported name — the principal a grant is TO.
    client: &'a str,
    ops: &'a dyn SiteOps,
}

impl<'a> UserCtx<'a> {
    pub fn new(state: &'a AppState, ops: &'a dyn SiteOps, client: &'a str) -> Self {
        UserCtx { state, client, ops }
    }

    pub(crate) fn db(&self) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
        self.state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))
    }

    /// The ONLY door to a site or to the stack. In order: the sub-toggle
    /// (refused by name — the user's switch, not the agent's), then
    /// `agent_grants::claim_or_ask` under the three locks it needs, which
    /// refuses a scratch site, runs the gate for `S`, records the ask on
    /// refusal, and answers with auto-allow only where a variant exists.
    ///
    /// `wanted` is this tool's one-line description of what the agent is trying
    /// to do, shown in the prompt so the user answers a concrete question.
    pub fn claim<S: scope::Marker>(&self, site_id: Option<&str>, wanted: &str) -> Result<Claimed<S>> {
        let conn = self.db()?;
        if !crate::mcp_server::sites_enabled(&conn) {
            return Err(Error::Other(format!(
                "acting on the user's own sites is turned off. The person you're working with can \
                 switch it on in rexenv under Settings → AI agents (MCP) → \"{}\", and then allow \
                 specific permissions per site. That is their decision, not something an agent can \
                 change.",
                crate::mcp_server::SITES_TOGGLE_LABEL
            )));
        }
        // Poisoned locks read as the SAFE direction: no requests recorded is a
        // lost prompt, not a lost boundary; auto-allow unreadable is OFF.
        let mut requests = self
            .state
            .agent_site_requests
            .lock()
            .map_err(|_| Error::Other("the app's request list lock is poisoned".into()))?;
        let auto = self
            .state
            .agent_site_auto_allow
            .lock()
            .map_err(|_| Error::Other("the app's auto-allow lock is poisoned".into()))?;
        agent_grants::claim_or_ask::<S>(&conn, &mut requests, &auto, site_id, self.client, wanted)
    }

    /// The re-read before a destructive step (#471's `still_granted`): the user
    /// may have pressed Revoke since the claim.
    pub fn still_granted<S: scope::Marker>(&self, granted: &Granted<S>) -> Result<bool> {
        let conn = self.db()?;
        agent_grants::still_granted(&conn, granted)
    }
}


/// The created site, as the agent sees it: M1's view + M1's status vocabulary +
/// the one thing only the creator can know, the admin credentials — once.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCreatedSite {
    #[serde(flatten)]
    site: super::view::AgentSiteView,
    url: String,
    #[serde(flatten)]
    status: super::view::AgentSiteStatus,
    /// WordPress only. The password is shown HERE and nowhere else — not in the
    /// feed (the summariser records the type, never a value), not in a later
    /// call. An agent that loses it asks the user, who can reset it in rexenv.
    #[serde(skip_serializing_if = "Option::is_none")]
    admin: Option<AgentAdmin>,
    /// What happened to a requested multisite conversion, when one was asked
    /// for: the site exists either way, so this is a field, not a failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    multisite: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentAdmin {
    user: String,
    password: String,
    note: &'static str,
}

/// Everything `site_create` decided from its arguments BEFORE asking for
/// permission — every refusal here is about the SHAPE of the request and needs
/// no grant to answer, so an agent with a typo is told about the typo rather
/// than sent to ask the user for a permission it would then misuse.
struct CreatePlan {
    new: NewSite,
    wp: Option<crate::core::wordpress::InstallOptions>,
    blueprint_id: Option<String>,
    multisite: Option<MultisiteMode>,
}

fn plan_create(ctx: &UserCtx<'_>, args: &Value) -> Result<CreatePlan> {
    let str_arg = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let name = str_arg("name").ok_or_else(|| Error::Other("site_create needs a `name`.".into()))?;
    let domain = str_arg("domain")
        .ok_or_else(|| Error::Other("site_create needs a `domain`, a full hostname like `shop.rex`.".into()))?
        .to_ascii_lowercase();
    let site_type = match str_arg("type") {
        Some(t) => SiteType::parse_db(t).map_err(|_| {
            Error::Other(format!("`{t}` is not a site type — use `wordpress`, `php` or `laravel`."))
        })?,
        None => return Err(Error::Other("site_create needs a `type`: `wordpress`, `php` or `laravel`.".into())),
    };
    crate::core::sites::validate_domain(&domain)?;
    let web_server = match str_arg("server") {
        Some(s) => WebServer::parse_db(s)
            .map_err(|_| Error::Other(format!("`{s}` is not a web server rexenv runs — use `nginx`, `apache` or `frankenphp`.")))?,
        None => WebServer::Nginx,
    };
    let db_engine = match str_arg("db_engine") {
        Some(e) => SiteDbEngine::parse_db(e)
            .map_err(|_| Error::Other(format!("`{e}` is not a database engine here — use `mysql` or `mariadb`.")))?,
        None => SiteDbEngine::Mysql,
    };
    let multisite = match str_arg("multisite") {
        None => None,
        Some(m) => {
            if site_type != SiteType::Wordpress {
                return Err(Error::Other("`multisite` only applies to a `wordpress` site.".into()));
            }
            match MultisiteMode::parse_db(m) {
                Ok(MultisiteMode::None) | Err(_) => {
                    return Err(Error::Other(format!("`{m}` is not a multisite mode — use `subdomain` or `subdirectory`.")))
                }
                Ok(mode) => Some(mode),
            }
        }
    };
    let starter_db = args.get("starter_db").and_then(Value::as_bool).unwrap_or(false);
    if starter_db && site_type != SiteType::Php {
        // The app's own rule (`sites::starter_db_refusal`, #462): the field is
        // dropped in silence everywhere but a blank PHP site, so asking for it
        // elsewhere would produce a site with no error and nothing to say why.
        return Err(Error::Other("`starter_db` only applies to a `php` (blank PHP) site.".into()));
    }
    let wp = if site_type == SiteType::Wordpress {
        let w = args.get("wp").cloned().unwrap_or_else(|| json!({}));
        let field = |k: &str| w.get(k).and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
        Some(crate::core::wordpress::InstallOptions {
            title: field("title"),
            admin_user: field("admin_user"),
            admin_email: field("admin_email"),
            admin_password: field("admin_password"),
            language: field("language"),
        })
    } else {
        if args.get("wp").is_some() {
            return Err(Error::Other("`wp` options only apply to a `wordpress` site.".into()));
        }
        None
    };
    let conn = ctx.db()?;
    if let Some(owner) = crate::core::sites::domain_taken_by(&conn, &domain)? {
        return Err(Error::Other(format!(
            "`{domain}` already reaches the site \"{owner}\" — one hostname can only reach one site. \
             Pick a different domain, or use the site that is already there (list_sites shows it)."
        )));
    }
    let blueprint_id = match str_arg("blueprint") {
        None => None,
        Some(bp_name) => {
            let all = crate::state::store::list_blueprints(&conn)?;
            let Some(bp) = all.iter().find(|b| b.name.eq_ignore_ascii_case(bp_name)) else {
                let names: Vec<&str> = all.iter().map(|b| b.name.as_str()).collect();
                return Err(Error::Other(if names.is_empty() {
                    format!("there is no blueprint called `{bp_name}` — the person you're working with has not saved any.")
                } else {
                    format!("there is no blueprint called `{bp_name}`. The saved ones are: {}.", names.join(", "))
                }));
            };
            if bp.spec.site_type != site_type {
                return Err(Error::Other(format!(
                    "blueprint `{}` is for a `{}` site, not a `{}` one.",
                    bp.name,
                    bp.spec.site_type.as_db(),
                    site_type.as_db()
                )));
            }
            Some(bp.id.clone())
        }
    };
    let php_version = match str_arg("php") {
        Some(v) => v.to_string(),
        None => crate::state::store::list_php_versions(&conn)?
            .into_iter()
            .find(|v| v.is_default)
            .map(|v| v.minor)
            .ok_or_else(|| {
                Error::Other(
                    "rexenv has no default PHP version yet — the person you're working with needs to \
                     finish rexenv's setup before sites can be created."
                        .into(),
                )
            })?,
    };
    Ok(CreatePlan {
        new: NewSite {
            name: name.to_string(),
            domain,
            site_type,
            php_version,
            web_server,
            path: String::new(),    // never a caller path: linking is its own tool, under `run`
            db_engine,
            git_url: String::new(), // never a clone: its own tool, under `run`
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db,
        },
        wp,
        blueprint_id,
        multisite,
    })
}

/// A create failure, translated for an agent — the scratch tool's shape, with
/// the parity tools' names for the way forward.
fn translate_create_failure(
    domain: &str,
    failure: crate::commands::sites::CreateFailure,
    conn: Option<&rusqlite::Connection>,
    acted: &super::feed::ActedTarget,
) -> Error {
    let Some(id) = failure.site_id else {
        return failure.error;
    };
    if let Some(conn) = conn {
        if let Ok(Some(site)) = crate::state::store::get_site(conn, &id) {
            acted.set(&site);
        }
    }
    Error::Other(format!(
        "`{domain}` was created but its setup did not finish, so it is not usable yet. It exists \
         (id `{id}`) and the person you're working with can see it in rexenv listed as \"setup \
         incomplete\", where they can retry or remove it. You can read what went wrong with \
         tail_log, retry it with site_retry, or remove it with site_delete (which needs their \
         `destroy` permission)."
    ))
}

fn site_create<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        // Shape first — no permission needed to be told about a typo.
        let plan = plan_create(&ctx, args)?;
        let domain = plan.new.domain.clone();
        let wanted = format!("create a {} site `{domain}`", plan.new.site_type.as_db());
        // Then the user's word: `manage` on rexenv itself (there is no site yet).
        let claimed = ctx.claim::<scope::Manage>(None, &wanted)?;
        let _ = &claimed.granted;

        // The resolved credentials are computed from the SAME function the
        // provision job uses, so what the reply reports is what was set.
        let admin = plan.wp.as_ref().map(|opts| {
            let r = crate::core::wordpress::resolve_install_options(&domain, &plan.new.name, opts);
            AgentAdmin {
                user: r.admin_user,
                password: r.admin_password,
                note: "Shown once. Not recorded anywhere an agent can read it again; the person \
                       you're working with can reset it in rexenv.",
            }
        });
        let ownership = Ownership::UserByAgent { client: ctx.client.to_string() };
        let site = match ctx.ops.create(plan.new, plan.wp, plan.blueprint_id, ownership).await {
            Ok(site) => {
                acted.set(&site);
                site
            }
            Err(failure) => {
                let conn = ctx.db().ok();
                return Err(translate_create_failure(&domain, failure, conn.as_deref(), acted));
            }
        };
        // The multisite conversion is a SECOND operation on a site that now
        // exists (the dialog does the same after its create). Its failure is
        // reported in the reply, never as a failure of the create — "it failed"
        // about a site sitting in the user's list would send the agent to make
        // another one.
        let multisite = match plan.multisite {
            None => None,
            Some(mode) => Some(match ctx.ops.multisite_convert(site.id.clone(), mode.as_db().to_string()).await {
                Ok(()) => format!("converted to a {} network", mode.as_db()),
                Err(e) => format!("the site was created but converting it to a {} network failed: {e}", mode.as_db()),
            }),
        };
        let read = super::readctx::ReadCtx::new(ctx.state);
        let signals = read.probe_serving(&site).await;
        let serving = signals.serving_manager && signals.edge_answers_ours;
        let view = AgentCreatedSite {
            site: super::view::AgentSiteView::from_site(&site, serving),
            url: format!("https://{}", site.domain),
            status: super::view::AgentSiteStatus::from_signals(&site, &signals),
            admin,
            multisite,
        };
        let mut value = serde_json::to_value(view).map_err(|e| Error::Other(format!("serialising the site: {e}")))?;
        if claimed.auto_granted {
            if let Some(obj) = value.as_object_mut() {
                obj.insert("consent".into(), Value::String(agent_grants::AUTO_GRANTED_NOTE.to_string()));
            }
        }
        Ok(value)
    })
}

fn site_delete<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("site_delete needs a `site_id`.".into()))?;
        // THE gate: the witness, typed `Destroy` — a `manage` grant cannot reach this line.
        let claimed = ctx.claim::<scope::Destroy>(Some(id), "delete the site — its files, its database and its configuration")?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("site_delete needs a site, not the stack.".into()))?;
        acted.set(&site);
        // The witness is a snapshot: re-assert before the destructive step, for
        // the case it does not cover — the user pressing Revoke in between.
        if !ctx.still_granted(&claimed.granted)? {
            return Err(Error::Other(format!(
                "the permission to delete `{}` was revoked before anything was done — nothing was deleted.",
                site.domain
            )));
        }
        ctx.ops.delete(site.id.clone()).await?;
        Ok(json!({
            "deleted": true,
            "domain": site.domain,
            "detail": format!("`{}` and its database are gone.", site.domain),
        }))
    })
}


/// The site after a change, as the agent sees it — M1's view, freshly read, so
/// the reply is the row as it now is and not what the agent asked for.
fn site_after(ctx: &UserCtx<'_>, id: &str) -> Result<Value> {
    // The row under a BRIEF lock, released before `service_infos` — which
    // takes the same lock itself (a held guard here deadlocked the first run).
    let site = {
        let conn = ctx.db()?;
        crate::state::store::get_site(&conn, id)?
            .ok_or_else(|| Error::Other(format!("no site with id {id:?} after the change")))?
    };
    let serving = crate::core::service_manager::site_serving(std::slice::from_ref(&site), &ctx.state.service_infos())
        .first()
        .is_some_and(|s| s.serving);
    serde_json::to_value(super::view::AgentSiteView::from_site(&site, serving))
        .map_err(|e| Error::Other(format!("serialising the site: {e}")))
}

fn str_field<'v>(args: &'v Value, key: &str, action: &str) -> Result<&'v str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Other(format!("site_configure `{action}` needs `{key}`.")))
}

fn site_configure<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("site_configure needs a `site_id`.".into()))?;
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("site_configure needs an `action`.".into()))?;
        // SHAPE first: parse the action and its field before asking anything.
        enum Change {
            Rename(String),
            Php(String),
            Server(WebServer),
            Xdebug(bool),
            EnvSet(String, String),
            EnvUnset(String),
            Domain(String),
            AddDomain(String),
            RemoveDomain(String),
            Move(String),
            Relink(String),
            RegenerateCert,
        }
        let change = match action {
            "rename" => Change::Rename(str_field(args, "name", action)?.to_string()),
            "php" => Change::Php(str_field(args, "version", action)?.to_string()),
            "server" => {
                let s = str_field(args, "server", action)?;
                Change::Server(WebServer::parse_db(s).map_err(|_| {
                    Error::Other(format!("`{s}` is not a web server rexenv runs — use `nginx`, `apache` or `frankenphp`."))
                })?)
            }
            "xdebug" => Change::Xdebug(
                args.get("enabled")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| Error::Other("site_configure `xdebug` needs `enabled` (true or false).".into()))?,
            ),
            "env_set" => Change::EnvSet(
                str_field(args, "key", action)?.to_string(),
                args.get("value")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Other("site_configure `env_set` needs `value`.".into()))?
                    .to_string(),
            ),
            "env_unset" => Change::EnvUnset(str_field(args, "key", action)?.to_string()),
            "domain" => Change::Domain(str_field(args, "domain", action)?.to_ascii_lowercase()),
            "add_domain" => Change::AddDomain(str_field(args, "domain", action)?.to_ascii_lowercase()),
            "remove_domain" => Change::RemoveDomain(str_field(args, "domain", action)?.to_ascii_lowercase()),
            "move" => Change::Move(str_field(args, "dest_parent", action)?.to_string()),
            "relink" => Change::Relink(str_field(args, "path", action)?.to_string()),
            "regenerate_cert" => Change::RegenerateCert,
            other => {
                return Err(Error::Other(format!(
                    "`{other}` is not a site_configure action. Use one of: rename, php, server, xdebug, \
                     env_set, env_unset, domain, add_domain, remove_domain, move, relink, regenerate_cert."
                )))
            }
        };
        let wanted = match &change {
            Change::Rename(n) => format!("rename it to `{n}`"),
            Change::Php(v) => format!("switch it to PHP {v}"),
            Change::Server(s) => format!("serve it with {}", s.as_db()),
            Change::Xdebug(on) => format!("turn Xdebug {}", if *on { "on" } else { "off" }),
            Change::EnvSet(k, _) => format!("set the environment variable `{k}`"),
            Change::EnvUnset(k) => format!("remove the environment variable `{k}`"),
            Change::Domain(d) => format!("change its domain to `{d}`"),
            Change::AddDomain(d) => format!("also answer on `{d}`"),
            Change::RemoveDomain(d) => format!("stop answering on `{d}`"),
            Change::Move(_) => "move its folder".to_string(),
            Change::Relink(_) => "re-point it at a folder the user moved".to_string(),
            Change::RegenerateCert => "regenerate its certificate".to_string(),
        };
        let claimed = ctx.claim::<scope::Manage>(Some(id), &wanted)?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("site_configure needs a site, not the stack.".into()))?;
        acted.set(&site);
        let id = site.id.clone();
        let ops = ctx.ops;
        let mut extra = serde_json::Map::new();
        match change {
            Change::Rename(n) => {
                ops.rename(id.clone(), n).await?;
            }
            Change::Php(v) => {
                ops.set_php(id.clone(), v).await?;
            }
            Change::Server(sv) => {
                ops.set_server(id.clone(), sv).await?;
            }
            Change::Xdebug(on) => {
                ops.set_xdebug(id.clone(), on).await?;
            }
            Change::EnvSet(k, v) => {
                // The app's command REPLACES the set (like its editor), so one
                // variable is set by merging into what is there. Values never
                // leave rexenv: the reply lists NAMES.
                let mut vars = ops.list_env(id.clone()).await?;
                vars.retain(|e| e.name != k);
                vars.push(crate::commands::sites::EnvVarInput { name: k, value: v });
                ops.set_env(id.clone(), vars.clone()).await?;
                extra.insert("env".into(), json!(vars.iter().map(|e| e.name.as_str()).collect::<Vec<_>>()));
            }
            Change::EnvUnset(k) => {
                let mut vars = ops.list_env(id.clone()).await?;
                let before = vars.len();
                vars.retain(|e| e.name != k);
                if vars.len() == before {
                    return Err(Error::Other(format!("`{k}` is not set on `{}` — nothing to remove.", site.domain)));
                }
                ops.set_env(id.clone(), vars.clone()).await?;
                extra.insert("env".into(), json!(vars.iter().map(|e| e.name.as_str()).collect::<Vec<_>>()));
            }
            Change::Domain(d) => {
                let change = ops.change_domain(id.clone(), d).await?;
                extra.insert("replacements".into(), json!(change.replacements));
                if change.backup_path.is_some() {
                    // The path stays inside rexenv; the FACT of a backup is what the
                    // agent needs to relay.
                    extra.insert("backup".into(), json!("a copy of the database as it was before the change was saved to the user's Downloads folder"));
                }
            }
            Change::AddDomain(d) => {
                let names = ops.add_domain(id.clone(), d).await?;
                extra.insert("domains".into(), json!(names));
            }
            Change::RemoveDomain(d) => {
                let names = ops.remove_domain(id.clone(), d).await?;
                extra.insert("domains".into(), json!(names));
            }
            Change::Move(dest) => {
                ops.move_docroot(id.clone(), dest).await?;
            }
            Change::Relink(path) => {
                ops.relink(id.clone(), path).await?;
            }
            Change::RegenerateCert => {
                ops.regenerate_cert(id.clone()).await?;
                extra.insert("certificate".into(), json!("reissued"));
            }
        }
        let mut value = site_after(&ctx, &id)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("action".into(), json!(action));
            obj.extend(extra);
            if claimed.auto_granted {
                obj.insert("consent".into(), Value::String(agent_grants::AUTO_GRANTED_NOTE.to_string()));
            }
        }
        Ok(value)
    })
}

fn site_restart<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("site_restart needs a `site_id`.".into()))?;
        let pool = args.get("pool").and_then(Value::as_bool).unwrap_or(false);
        let wanted = if pool { "restart it and the PHP pool it shares" } else { "restart it" };
        let claimed = ctx.claim::<scope::Manage>(Some(id), wanted)?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("site_restart needs a site.".into()))?;
        acted.set(&site);
        let r = ctx.ops.restart(site.id.clone(), pool).await?;
        // #444's three honest outcomes, in the CLI's words; loopback ports
        // dropped — an agent has no use for them and the view rule is "only
        // what the answer needs".
        let detail = match r.kind {
            "backend" => format!("`{}`'s own {} backend was stopped and started again.", site.domain, r.server.clone().unwrap_or_default()),
            "shared" => format!("`{}` has no process of its own: its config was rebuilt and the web tier reloaded, which is what makes a default site pick up a change.", site.domain),
            _ => format!("`{}`'s backend was adopted from outside rexenv and was not stopped — the person you're working with owns that process.", site.domain),
        };
        Ok(json!({
            "domain": site.domain,
            "kind": r.kind,
            "server": r.server,
            "phpMinor": r.php_minor,
            "sitesOnPool": r.sites_on_pool,
            "poolRestarted": r.pool_restarted,
            "detail": detail,
        }))
    })
}

fn site_retry<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("site_retry needs a `site_id`.".into()))?;
        let claimed = ctx.claim::<scope::Manage>(Some(id), "finish its setup")?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("site_retry needs a site.".into()))?;
        acted.set(&site);
        let st = ctx.ops.retry(site.id.clone()).await?;
        // The job's own error text can name a local path; through the one
        // scrubber, like every other door a path leaves by (#201).
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), &site.path);
        let scrub = |s: Option<String>| s.map(|t| super::view::scrub_log_line(&t, &known));
        Ok(json!({
            "domain": st.domain,
            "status": st.status,
            "phases": st.phases.iter().map(|p| json!({ "label": p.label, "status": p.status })).collect::<Vec<_>>(),
            "summary": scrub(st.summary),
            "error": scrub(st.error),
            "servingBlocked": st.serving_blocked,
            "servingHolder": st.serving_holder,
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A parity handler's only door to a site is the scope witness, and the
    /// context offers no other.** Stated as a property of the CONTEXT while the
    /// registry is empty, exactly as #209 did for `ScratchCtx`: `UserCtx` has
    /// no `site_by_id`, no `sites`, no `get_site` — `claim` is the one method
    /// that yields anything about a site, and what it yields is `Granted<S>`.
    #[test]
    fn a_parity_handler_can_only_reach_a_site_through_the_scope_witness() {
        let me = include_str!("user_sites.rs");
        let prod = &me[..me.find("#[cfg(test)]").unwrap()];
        let ctx = prod.find("impl<'a> UserCtx<'a> {").unwrap();
        // The impl block ONLY — a handler below it may resolve a row it was
        // already handed (the feed's `acted` target), but the CONTEXT offers
        // no method that does.
        let body = &prod[ctx..ctx + prod[ctx..].find("\n}\n").unwrap()];
        for door in ["fn site_by_id", "fn sites(", "fn site(", "get_site(", "list_sites("] {
            assert!(!body.contains(door), "UserCtx grew a second door to sites: {door}");
        }
        assert!(body.contains("pub fn claim<S: scope::Marker>"), "the one door");
        assert!(body.contains("claim_or_ask::<S>"), "…and it goes through the one call-site shape (#471)");
        // The toggle is checked FIRST, so a user with it off is sent to the
        // switch rather than to a grant they cannot give yet.
        let claim_at = body.find("pub fn claim<").unwrap();
        let toggle_at = body[claim_at..].find("sites_enabled(").unwrap();
        let gate_at = body[claim_at..].find("claim_or_ask").unwrap();
        assert!(toggle_at < gate_at, "the toggle must be refused before the gate runs");
    }

    /// Every parity tool declares its scope, and declares a sweep — the
    /// registry is empty today, so this pins the SHAPE (the fields are
    /// required, and the plan is derivable) rather than a count.
    #[test]
    fn every_parity_tool_is_ranked_and_sweepable_by_construction() {
        for t in registry() {
            let args = (t.sweep_args)("fixture-site-id");
            assert!(args.is_object(), "{}: sweep_args must be a JSON object", t.name);
            assert!(Scope::ALL.contains(&t.scope));
        }
        assert_eq!(tools_list_descriptors().as_array().map(Vec::len), Some(registry().len()));
    }

    // ── The handlers, against a real AppState and a fake app ──────────────
    use crate::state::models::{test_site, SiteOrigin};
    use crate::state::store;
    use std::sync::Mutex;

    struct SandboxPaths;
    impl crate::platform::traits::Paths for SandboxPaths {
        fn app_data_dir(&self) -> Result<std::path::PathBuf> { Ok("/Users/somebody/Library/Application Support/rexenv".into()) }
        fn config_dir(&self) -> Result<std::path::PathBuf> { Ok("/Users/somebody/Library/Application Support/rexenv/config".into()) }
        fn log_dir(&self) -> Result<std::path::PathBuf> { Ok("/Users/somebody/Library/Application Support/rexenv/logs".into()) }
        fn bin_dir(&self) -> Result<std::path::PathBuf> { Ok("/Users/somebody/Library/Application Support/rexenv/bin".into()) }
        fn hosts_file(&self) -> std::path::PathBuf { "/etc/hosts".into() }
    }
    struct StubPlatform;
    impl crate::platform::traits::Platform for StubPlatform {
        fn paths(&self) -> &dyn crate::platform::traits::Paths { &SandboxPaths }
        fn dns(&self) -> &dyn crate::platform::traits::DnsManager { unimplemented!() }
        fn cert_trust(&self) -> &dyn crate::platform::traits::CertTrustManager { unimplemented!() }
        fn privileges(&self) -> &dyn crate::platform::traits::PrivilegeManager { unimplemented!() }
        fn supervisor(&self) -> &dyn crate::platform::traits::ProcessSupervisor { unimplemented!() }
        fn autostart(&self) -> &dyn crate::platform::traits::AutostartManager { unimplemented!() }
        fn permissions(&self) -> &dyn crate::platform::traits::PermissionManager { unimplemented!() }
        fn shell(&self) -> &dyn crate::platform::traits::ShellRunner { unimplemented!() }
        fn binaries(&self) -> &dyn crate::platform::traits::BinaryProvider { unimplemented!() }
        fn edge(&self) -> &dyn crate::platform::traits::EdgeSupervisor { unimplemented!() }
        fn dns_agent(&self) -> &dyn crate::platform::traits::DnsAgentManager { unimplemented!() }
    }

    fn app_state() -> AppState {
        let conn = crate::state::db::open_in_memory().unwrap();
        let ca = crate::core::ssl::LocalCa {
            cert_pem: String::new(),
            key_pem: String::new(),
            cert_path: "/tmp/never/ca.pem".into(),
            key_path: "/tmp/never/ca.key".into(),
        };
        AppState::new(conn, Box::new(StubPlatform), ca)
    }

    type Created = (NewSite, Option<crate::core::wordpress::InstallOptions>, Option<String>, Ownership);

    /// Records what the app was asked to do; builds nothing.
    #[derive(Default)]
    struct FakeOps {
        created: Mutex<Vec<Created>>,
        deleted: Mutex<Vec<String>>,
        converted: Mutex<Vec<(String, String)>>,
        calls: Mutex<Vec<String>>,
        env: Mutex<Vec<crate::commands::sites::EnvVarInput>>,
    }
    impl SiteOps for FakeOps {
        fn create<'a>(
            &'a self,
            new: NewSite,
            wp: Option<crate::core::wordpress::InstallOptions>,
            blueprint_id: Option<String>,
            ownership: Ownership,
        ) -> OpFuture<'a, std::result::Result<Site, crate::commands::sites::CreateFailure>> {
            let site = test_site("11111111-2222-4333-8444-555555555555", &new.domain, SiteOrigin::User);
            self.created.lock().unwrap().push((new, wp, blueprint_id, ownership));
            Box::pin(async move { Ok(site) })
        }
        fn delete<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>> {
            self.deleted.lock().unwrap().push(id);
            Box::pin(async { Ok(()) })
        }
        fn multisite_convert<'a>(&'a self, id: String, mode: String) -> OpFuture<'a, Result<()>> {
            self.converted.lock().unwrap().push((id, mode));
            Box::pin(async { Ok(()) })
        }
        fn rename<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<Option<Site>>> {
            self.calls.lock().unwrap().push(format!("rename {id} {name}"));
            Box::pin(async { Ok(None) })
        }
        fn change_domain<'a>(&'a self, id: String, domain: String) -> OpFuture<'a, Result<crate::commands::sites::DomainChange>> {
            self.calls.lock().unwrap().push(format!("domain {id} {domain}"));
            let site = test_site(&id, &domain, SiteOrigin::User);
            Box::pin(async move { Ok(crate::commands::sites::DomainChange { site, backup_path: Some("/Users/somebody/Downloads/x.sql".into()), replacements: 12 }) })
        }
        fn add_domain<'a>(&'a self, id: String, domain: String) -> OpFuture<'a, Result<Vec<String>>> {
            self.calls.lock().unwrap().push(format!("add_domain {id} {domain}"));
            Box::pin(async move { Ok(vec!["mine.rex".into(), domain]) })
        }
        fn remove_domain<'a>(&'a self, id: String, domain: String) -> OpFuture<'a, Result<Vec<String>>> {
            self.calls.lock().unwrap().push(format!("remove_domain {id} {domain}"));
            Box::pin(async { Ok(vec!["mine.rex".into()]) })
        }
        fn set_php<'a>(&'a self, id: String, version: String) -> OpFuture<'a, Result<Option<Site>>> {
            self.calls.lock().unwrap().push(format!("php {id} {version}"));
            Box::pin(async { Ok(None) })
        }
        fn set_server<'a>(&'a self, id: String, server: WebServer) -> OpFuture<'a, Result<Option<Site>>> {
            self.calls.lock().unwrap().push(format!("server {id} {}", server.as_db()));
            Box::pin(async { Ok(None) })
        }
        fn set_xdebug<'a>(&'a self, id: String, enabled: bool) -> OpFuture<'a, Result<Option<Site>>> {
            self.calls.lock().unwrap().push(format!("xdebug {id} {enabled}"));
            Box::pin(async { Ok(None) })
        }
        fn list_env<'a>(&'a self, _id: String) -> OpFuture<'a, Result<Vec<crate::commands::sites::EnvVarInput>>> {
            let env = self.env.lock().unwrap().clone();
            Box::pin(async move { Ok(env) })
        }
        fn set_env<'a>(&'a self, id: String, vars: Vec<crate::commands::sites::EnvVarInput>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("env {id} {}", vars.iter().map(|v| format!("{}={}", v.name, v.value)).collect::<Vec<_>>().join(",")));
            *self.env.lock().unwrap() = vars;
            Box::pin(async { Ok(()) })
        }
        fn move_docroot<'a>(&'a self, id: String, dest_parent: String) -> OpFuture<'a, Result<Site>> {
            self.calls.lock().unwrap().push(format!("move {id} {dest_parent}"));
            let site = test_site(&id, "mine.rex", SiteOrigin::User);
            Box::pin(async move { Ok(site) })
        }
        fn relink<'a>(&'a self, id: String, path: String) -> OpFuture<'a, Result<Site>> {
            self.calls.lock().unwrap().push(format!("relink {id} {path}"));
            let site = test_site(&id, "mine.rex", SiteOrigin::User);
            Box::pin(async move { Ok(site) })
        }
        fn regenerate_cert<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("cert {id}"));
            Box::pin(async { Ok(()) })
        }
        fn restart<'a>(&'a self, id: String, pool: bool) -> OpFuture<'a, Result<crate::commands::sites::SiteRestartReport>> {
            self.calls.lock().unwrap().push(format!("restart {id} {pool}"));
            Box::pin(async move {
                Ok(crate::commands::sites::SiteRestartReport {
                    kind: "shared", server: None, port: None, php_minor: "8.3".into(), pool_port: 19083, sites_on_pool: 4, pool_restarted: pool,
                })
            })
        }
        fn retry<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<crate::commands::site_provision::SiteProvisionState>> {
            self.calls.lock().unwrap().push(format!("retry {site_id}"));
            Box::pin(async move {
                Ok(crate::commands::site_provision::SiteProvisionState {
                    id: "job".into(),
                    domain: "mine.rex".into(),
                    site_id: Some(site_id),
                    phases: vec![crate::commands::site_provision::PhaseState { key: "core_download".into(), label: "downloading WordPress".into(), status: "ok".into() }],
                    phase_cursor: 0,
                    pct: 100,
                    status: "ok".into(),
                    summary: Some("done; log at /Users/somebody/Library/Application Support/rexenv/logs/x.log".into()),
                    error: None,
                    log_key: "x.log".into(),
                    download_ids: vec![],
                    serving_blocked: false,
                    serving_holder: None,
                    serving_app: None,
                    assets_warning: None,
                })
            })
        }
    }

    fn switch_on(state: &AppState) {
        let conn = state.db.lock().unwrap();
        store::set_setting(&conn, crate::mcp_server::MCP_SITES_ENABLED_KEY, "true").unwrap();
    }

    /// A default PHP version, so a create without `php` can resolve one — the
    /// SHAPE step runs before the switch is consulted, so this is seeded first.
    fn seed_php(state: &AppState) {
        let conn = state.db.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO php_versions (minor, fpm_port, installed, is_default) VALUES ('8.3', 19083, 1, 1)",
            [],
        )
        .unwrap();
    }

    fn asks(state: &AppState) -> Vec<crate::core::agent_grants::GrantRequest> {
        state.agent_site_requests.lock().unwrap().list().to_vec()
    }

    /// **`site_create` refuses a bad request on its shape before asking for
    /// anything, asks for `manage` on rexenv itself when the shape is fine,
    /// and — granted — runs the app's own create as the USER's site, returning
    /// the admin credentials once.**
    #[tokio::test]
    async fn site_create_refuses_shape_before_the_gate_and_asks_after_it() {
        let state = app_state();
        seed_php(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, "claude-code");
        let run = |args: Value| {
            let acted = &acted;
            async move { site_create(ctx, &args, acted).await }
        };
        let ok_args = json!({ "name": "Shop", "domain": "Shop.rex", "type": "wordpress" });

        // The switch is off: refused by name, before anything else.
        let err = run(ok_args.clone()).await.unwrap_err().to_string();
        assert!(err.contains(crate::mcp_server::SITES_TOGGLE_LABEL), "{err}");
        assert!(asks(&state).is_empty(), "a switched-off surface records no ask");

        switch_on(&state);
        // SHAPE refusals need no permission and record no ask.
        for (args, expect) in [
            (json!({ "name": "x", "domain": "x.rex", "type": "drupal" }), "not a site type"),
            (json!({ "name": "x", "domain": "x.rex", "type": "php", "multisite": "subdomain" }), "only applies to a `wordpress`"),
            (json!({ "name": "x", "domain": "x.rex", "type": "wordpress", "starter_db": true }), "only applies to a `php`"),
            (json!({ "name": "x", "domain": "x.rex", "type": "laravel", "wp": {} }), "only apply to a `wordpress`"),
            (json!({ "name": "x", "domain": "x.rex", "type": "php", "server": "iis" }), "not a web server"),
            (json!({ "name": "x", "domain": "x.rex", "type": "php", "blueprint": "nope" }), "no blueprint called"),
        ] {
            let err = run(args).await.unwrap_err().to_string();
            assert!(err.contains(expect), "expected {expect:?}: {err}");
        }
        assert!(asks(&state).is_empty(), "a shape refusal never asks the user for anything");

        // A domain some site already answers on: refused, no ask.
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "taken.rex", SiteOrigin::User)).unwrap();
        }
        let err = run(json!({ "name": "x", "domain": "taken.rex", "type": "php" })).await.unwrap_err().to_string();
        assert!(err.contains("already reaches"), "{err}");
        assert!(asks(&state).is_empty());

        // Good shape, no grant: refused with the place consent lives, and the
        // ask is recorded — stack-level, `manage`, naming the domain.
        let err = run(ok_args.clone()).await.unwrap_err().to_string();
        assert!(err.contains("Site access") && err.contains("`manage`"), "{err}");
        let a = asks(&state);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].site_id, None, "creating a site is a permission on rexenv itself");
        assert_eq!(a[0].scope, Scope::Manage);
        assert!(a[0].wanted.contains("shop.rex") && a[0].wanted.contains("wordpress"), "{}", a[0].wanted);
        assert!(ops.created.lock().unwrap().is_empty(), "nothing ran");

        // Granted: the app's create runs, as the USER's site, with the shape
        // the agent asked for and nothing it did not.
        {
            let conn = state.db.lock().unwrap();
            store::grant_agent_site(&conn, "g1", None, "claude-code", "manage", 7, false, false).unwrap();
        }
        let v = run(json!({ "name": "Shop", "domain": "Shop.rex", "type": "wordpress", "php": "8.2", "server": "frankenphp",
                            "wp": { "admin_user": "owner" } })).await.unwrap();
        {
            let created = ops.created.lock().unwrap();
            assert_eq!(created.len(), 1);
            let (new, wp, bp, ownership) = &created[0];
            assert_eq!(new.domain, "shop.rex", "lower-cased on the way in");
            assert_eq!(new.site_type, SiteType::Wordpress);
            assert_eq!(new.php_version, "8.2");
            assert_eq!(new.web_server, WebServer::Frankenphp);
            assert!(new.path.is_empty() && new.git_url.is_empty(), "never a link, never a clone");
            assert_eq!(wp.as_ref().map(|w| w.admin_user.as_str()), Some("owner"));
            assert_eq!(*bp, None);
            assert!(matches!(ownership, Ownership::UserByAgent { client } if client == "claude-code"));
        }
        // The reply: the site, its url, the credentials ONCE (the resolved
        // defaults — the same function the job uses), and no consent note
        // because a person clicked.
        assert_eq!(v["domain"], "shop.rex");
        assert_eq!(v["url"], "https://shop.rex");
        assert_eq!(v["admin"]["user"], "owner");
        assert_eq!(v["admin"]["password"], crate::core::wordpress::DEFAULT_ADMIN);
        assert!(v["admin"]["note"].as_str().unwrap().contains("Shown once"));
        assert!(v.get("consent").is_none());
        assert!(v.get("multisite").is_none());
        assert_eq!(acted.take().as_deref(), Some("11111111-2222-4333-8444-555555555555"), "the feed names what was made");
        assert!(asks(&state).is_empty(), "the grant answered the ask");

        // A blank PHP site: no `wp`, no admin block; `multisite` runs the
        // SECOND operation and reports it in the reply.
        let v = run(json!({ "name": "Net", "domain": "net.rex", "type": "wordpress", "multisite": "subdirectory" })).await.unwrap();
        assert_eq!(ops.converted.lock().unwrap().as_slice(), &[("11111111-2222-4333-8444-555555555555".to_string(), "subdirectory".to_string())]);
        assert!(v["multisite"].as_str().unwrap().contains("subdirectory"));
        let v = run(json!({ "name": "Blank", "domain": "blank.rex", "type": "php", "starter_db": true })).await.unwrap();
        assert!(v.get("admin").is_none(), "no credentials for a site with no WordPress");
        let created = ops.created.lock().unwrap();
        let (new, wp, _, _) = created.last().unwrap();
        assert!(new.starter_db && wp.is_none());
        drop(created);
    }

    /// **`site_delete` needs `destroy` — a `manage` grant does not reach it —
    /// refuses the agent's own scratch site, and runs the app's full delete.**
    #[tokio::test]
    async fn site_delete_needs_destroy_and_refuses_the_agents_own_scratch_site() {
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, "claude-code");
        let mine = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        let theirs = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &mine).unwrap();
            store::insert_site(&conn, &theirs).unwrap();
            store::grant_agent_site(&conn, "g1", Some(&mine.id), "claude-code", "manage", 7, false, false).unwrap();
        }
        let err = site_delete(ctx, &json!({ "site_id": mine.id }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "a manage grant must not reach a delete: {err}");
        let a = asks(&state);
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].site_id.as_deref(), a[0].scope), (Some(mine.id.as_str()), Scope::Destroy));
        assert!(ops.deleted.lock().unwrap().is_empty());

        // The scratch site: refused before the gate, scratch tools named.
        let err = site_delete(ctx, &json!({ "site_id": theirs.id }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("scratch_delete_site"), "{err}");

        // A session-long destroy grant: the app's delete runs, the feed names it.
        {
            let conn = state.db.lock().unwrap();
            store::grant_agent_site(&conn, "g2", Some(&mine.id), "claude-code", "destroy", 1, false, true).unwrap();
        }
        let v = site_delete(ctx, &json!({ "site_id": mine.id }), &acted).await.unwrap();
        assert_eq!(v["deleted"], true);
        assert_eq!(v["domain"], "mine.rex");
        assert_eq!(ops.deleted.lock().unwrap().as_slice(), std::slice::from_ref(&mine.id));
        assert_eq!(acted.take().as_deref(), Some(mine.id.as_str()));
    }

    /// **`site_configure` parses the action on its shape before asking, claims
    /// `manage` ONCE on the site, runs exactly the app command for that action,
    /// and answers with the site as it now is — env VALUES never included.**
    #[tokio::test]
    async fn site_configure_dispatches_each_action_through_the_app_after_one_manage_claim() {
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, "claude-code");
        let mine = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &mine).unwrap();
        }
        let run = |args: Value| {
            let acted = &acted;
            async move { site_configure(ctx, &args, acted).await }
        };
        let with = |action: &str, k: &str, v: Value| {
            let mut m = serde_json::Map::new();
            m.insert("site_id".into(), json!(mine.id));
            m.insert("action".into(), json!(action));
            m.insert(k.into(), v);
            Value::Object(m)
        };

        // Shape: an unknown action and a missing field are answered without a
        // permission — and record no ask.
        let err = run(with("paint", "name", json!("x"))).await.unwrap_err().to_string();
        assert!(err.contains("not a site_configure action") && err.contains("regenerate_cert"), "{err}");
        let err = run(json!({ "site_id": mine.id, "action": "php" })).await.unwrap_err().to_string();
        assert!(err.contains("needs `version`"), "{err}");
        let err = run(with("server", "server", json!("iis"))).await.unwrap_err().to_string();
        assert!(err.contains("not a web server"), "{err}");
        assert!(asks(&state).is_empty(), "shape refusals ask for nothing");

        // Good shape, no grant: `manage` asked for on THIS site, with the verb.
        let err = run(with("php", "version", json!("8.4"))).await.unwrap_err().to_string();
        assert!(err.contains("Site access"), "{err}");
        let a = asks(&state);
        assert_eq!((a[0].site_id.as_deref(), a[0].scope), (Some(mine.id.as_str()), Scope::Manage));
        assert!(a[0].wanted.contains("PHP 8.4"), "{}", a[0].wanted);
        assert!(ops.calls.lock().unwrap().is_empty());

        {
            let conn = state.db.lock().unwrap();
            store::grant_agent_site(&conn, "g1", Some(&mine.id), "claude-code", "manage", 7, false, false).unwrap();
        }
        // Every action reaches exactly its app command.
        let v = run(with("php", "version", json!("8.4"))).await.unwrap();
        assert_eq!(v["action"], "php");
        assert_eq!(v["domain"], "mine.rex", "the reply is the row, re-read");
        run(with("rename", "name", json!("Mine"))).await.unwrap();
        run(with("server", "server", json!("frankenphp"))).await.unwrap();
        run(with("xdebug", "enabled", json!(true))).await.unwrap();
        let v = run(with("add_domain", "domain", json!("ALSO.rex"))).await.unwrap();
        assert_eq!(v["domains"], json!(["mine.rex", "also.rex"]), "lower-cased, and the whole list comes back");
        run(with("remove_domain", "domain", json!("also.rex"))).await.unwrap();
        let v = run(with("domain", "domain", json!("new.rex"))).await.unwrap();
        assert_eq!(v["replacements"], 12);
        assert!(v["backup"].as_str().unwrap().contains("Downloads"), "the FACT of the backup, not its path: {v}");
        assert!(!v.to_string().contains("/Users/"), "no local path in the reply: {v}");
        run(with("move", "dest_parent", json!("/Users/somebody/Sites"))).await.unwrap();
        run(with("relink", "path", json!("/Users/somebody/Projects/mine"))).await.unwrap();
        let v = run(json!({ "site_id": mine.id, "action": "regenerate_cert" })).await.unwrap();
        assert_eq!(v["certificate"], "reissued");
        // env: a merge, and NAMES only in the reply.
        let mut set = with("env_set", "key", json!("API_KEY"));
        set["value"] = json!("s3cret");
        let v = run(set).await.unwrap();
        assert_eq!(v["env"], json!(["API_KEY"]));
        assert!(!v.to_string().contains("s3cret"), "an env value left rexenv: {v}");
        let mut set2 = with("env_set", "key", json!("DEBUG"));
        set2["value"] = json!("1");
        let v = run(set2).await.unwrap();
        assert_eq!(v["env"], json!(["API_KEY", "DEBUG"]), "set is a merge, not a replace");
        let v = run(with("env_unset", "key", json!("API_KEY"))).await.unwrap();
        assert_eq!(v["env"], json!(["DEBUG"]));
        let err = run(with("env_unset", "key", json!("NOPE"))).await.unwrap_err().to_string();
        assert!(err.contains("not set"), "{err}");

        let calls = ops.calls.lock().unwrap().clone();
        let id = mine.id.as_str();
        for expect in [
            format!("php {id} 8.4"), format!("rename {id} Mine"), format!("server {id} frankenphp"), format!("xdebug {id} true"),
            format!("add_domain {id} also.rex"), format!("remove_domain {id} also.rex"), format!("domain {id} new.rex"),
            format!("move {id} /Users/somebody/Sites"), format!("relink {id} /Users/somebody/Projects/mine"), format!("cert {id}"),
            format!("env {id} API_KEY=s3cret"), format!("env {id} API_KEY=s3cret,DEBUG=1"), format!("env {id} DEBUG=1"),
        ] {
            assert!(calls.contains(&expect), "missing app call {expect:?} in {calls:?}");
        }
        assert_eq!(acted.take().as_deref(), Some(id));
    }

    /// **`site_restart` and `site_retry` need `manage` on the site, run the app's
    /// own operation, and their replies carry the outcome in words — never a
    /// loopback port the agent has no use for, never a local log path.**
    #[tokio::test]
    async fn site_restart_and_retry_need_manage_and_answer_in_words() {
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, "claude-code");
        let mine = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &mine).unwrap();
        }
        assert!(site_restart(ctx, &json!({ "site_id": mine.id }), &acted).await.is_err());
        assert!(site_retry(ctx, &json!({ "site_id": mine.id }), &acted).await.is_err());
        assert_eq!(asks(&state).len(), 1, "one key: same site, same client, same scope — one prompt");
        {
            let conn = state.db.lock().unwrap();
            store::grant_agent_site(&conn, "g1", Some(&mine.id), "claude-code", "manage", 7, false, false).unwrap();
        }
        let v = site_restart(ctx, &json!({ "site_id": mine.id, "pool": true }), &acted).await.unwrap();
        assert_eq!(v["kind"], "shared");
        assert_eq!(v["sitesOnPool"], 4);
        assert_eq!(v["poolRestarted"], true);
        assert!(v.get("poolPort").is_none() && v.get("port").is_none(), "ports dropped: {v}");
        assert!(v["detail"].as_str().unwrap().contains("reloaded"));
        let v = site_retry(ctx, &json!({ "site_id": mine.id }), &acted).await.unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["phases"][0]["label"], "downloading WordPress");
        let text = v.to_string();
        assert!(!text.contains("/Users/somebody"), "the job's own text is scrubbed: {text}");
        assert!(text.contains("<"), "the scrubbed path reads as a label: {text}");
        assert!(ops.calls.lock().unwrap().iter().any(|c| c == &format!("retry {}", mine.id)));
    }
}
