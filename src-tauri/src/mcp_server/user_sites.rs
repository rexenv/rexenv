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
        name: "wp_info",
        description: "Read a WordPress site the user owns — the things its WordPress tab shows. \
                      Takes `site_id` and `what`: `info` (version, multisite), `options` (the \
                      general settings form — title, admin email, timezone, roles), `debug` (WP_DEBUG \
                      and the named debug flags), `maintenance`, `permalinks`, `languages`, \
                      `cron` (scheduled events), `checksums` (core files verified against \
                      WordPress.org), `primary_admin`. Every read boots the site's own code through \
                      wp-cli, as the user, so it needs the user's `read` permission on that site. \
                      Refused on a scratch site (use wp_run there) and on a non-WordPress site.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "what": { "type": "string", "enum": ["info", "options", "debug", "maintenance", "permalinks", "languages", "cron", "checksums", "primary_admin"] },
                "flag": { "type": "string", "description": "debug: one named flag (WP_DEBUG_LOG, WP_DEBUG_DISPLAY, SCRIPT_DEBUG, SAVEQUERIES) instead of WP_DEBUG." }
            },
            "required": ["site_id", "what"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "what": "info" }),
        summarise: |args| args.get("what").and_then(Value::as_str).map(str::to_string),
        scope: Scope::Read,
        handler: wp_info,
    },
    UserTool {
        name: "wp_plugin",
        description: "Plugins on a WordPress site the user owns. Takes `site_id`, `action` and \
                      `names` (plugin slugs; `list` needs none). `list` (with `check_updates`) \
                      needs `read`; `activate`, `deactivate`, `update`, `activate_network`, \
                      `deactivate_network` need `manage`; `delete` needs `destroy`. Installing is \
                      not here — it is a job with progress, coming separately. The app's own rules \
                      apply: a plugin rexenv linked from a repo is unlinked on delete, never \
                      removed from the checkout.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["list", "activate", "deactivate", "update", "delete", "activate_network", "deactivate_network"] },
                "names": { "type": "array", "items": { "type": "string" }, "description": "Plugin slugs (folder names)." },
                "check_updates": { "type": "boolean", "description": "list: also ask WordPress.org for available updates (slower)." }
            },
            "required": ["site_id", "action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "list" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("plugin {a}")),
        scope: Scope::Destroy,
        handler: wp_plugin,
    },
    UserTool {
        name: "wp_theme",
        description: "Themes on a WordPress site the user owns. Takes `site_id`, `action` and \
                      `names` (theme slugs; `list` and `network_enabled` need none). `list` (with \
                      `check_updates`) and `network_enabled` need `read`; `activate`, `update`, \
                      `enable_network`, `disable_network` need `manage`; `delete` needs `destroy`. \
                      Installing is a separate job tool.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["list", "activate", "update", "delete", "network_enabled", "enable_network", "disable_network"] },
                "names": { "type": "array", "items": { "type": "string" }, "description": "Theme slugs (folder names)." },
                "check_updates": { "type": "boolean" }
            },
            "required": ["site_id", "action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "list" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("theme {a}")),
        scope: Scope::Destroy,
        handler: wp_theme,
    },
    UserTool {
        name: "wp_user",
        description: "Users on a WordPress site the user owns. Takes `site_id` and `action`: \
                      `list` and `super_admins` need `read`; `create` {login, email, role, \
                      password — omit to have rexenv generate one, returned ONCE}, `set_role` \
                      {user_id, role}, `login_url` {user_id — omit for the primary administrator; a \
                      one-time browser link into wp-admin, never recorded}, `super_admin_add` \
                      {user} need `manage`; `set_password` {user_id, password} and `delete` \
                      {user_id, and EXACTLY ONE of reassign (a user id to give their posts to) or \
                      delete_posts: true} need `destroy` — a reset locks a person out, and the \
                      app refuses to delete the primary administrator or a multisite user.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["list", "create", "set_role", "login_url", "super_admins", "super_admin_add", "set_password", "delete"] },
                "login": { "type": "string" }, "email": { "type": "string" }, "role": { "type": "string" },
                "password": { "type": "string" }, "user_id": { "type": "integer" }, "user": { "type": "string" },
                "reassign": { "type": "integer" }, "delete_posts": { "type": "boolean" }
            },
            "required": ["site_id", "action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "list" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("user {a}")),
        scope: Scope::Destroy,
        handler: wp_user,
    },
    UserTool {
        name: "wp_option",
        description: "Settings on a WordPress site the user owns — the writes wp_info reads. Takes \
                      `site_id` and `action`: `update` {name, value} (one option, through the \
                      app's vetted setter), `debug` {on} (WP_DEBUG), `debug_flag` {flag, on} \
                      (WP_DEBUG_LOG / WP_DEBUG_DISPLAY / SCRIPT_DEBUG / SAVEQUERIES), \
                      `maintenance` {on}, `permalinks` {structure}, `language` {locale}. All need \
                      `manage` on the site.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["update", "debug", "debug_flag", "maintenance", "permalinks", "language"] },
                "name": { "type": "string" }, "value": { "type": "string" }, "on": { "type": "boolean" },
                "flag": { "type": "string" }, "structure": { "type": "string" }, "locale": { "type": "string" }
            },
            "required": ["site_id", "action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "maintenance", "on": false }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("option {a}")),
        scope: Scope::Manage,
        handler: wp_option,
    },
    UserTool {
        name: "wp_maintain",
        description: "Maintenance on a WordPress site the user owns. Takes `site_id` and `action`: \
                      `cache_flush`, `rewrite_flush`, `transient_delete_all`, `cron_run_due`, \
                      `cron_run_hook` {hook}, `checksum_cleanup` {paths — the files wp_info's \
                      `checksums` reported as not WordPress's own}, `core_update` (to the latest), \
                      `core_reinstall` (the same version, files only) need `manage`; \
                      `core_switch` {version} needs `destroy` — a downgrade can leave the database \
                      ahead of the code, and the reply says whether a database update is needed.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["cache_flush", "rewrite_flush", "transient_delete_all", "cron_run_due", "cron_run_hook", "checksum_cleanup", "core_update", "core_reinstall", "core_switch"] },
                "hook": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } }, "version": { "type": "string" }
            },
            "required": ["site_id", "action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "cache_flush" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(str::to_string),
        scope: Scope::Destroy,
        handler: wp_maintain,
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

/// The app's own WordPress operations (`commands::wordpress`), runtime-erased
/// like [`SiteOps`] and for the same reason. Every method is one `#[tauri::command]`
/// the WordPress tabs call; nothing here re-implements a wp-cli invocation, so
/// the vetted argument hygiene in `core::wordpress` (slug guards, the
/// `DEBUG_FLAGS` allow-list, the #446 user-delete fork) applies unchanged.
pub trait WpOps: Send + Sync {
    fn info<'a>(&'a self, id: String) -> OpFuture<'a, Result<crate::core::wordpress::WpInfo>>;
    fn plugins<'a>(&'a self, id: String, check_updates: bool) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpPlugin>>>;
    fn plugin_activate<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>>;
    fn plugin_deactivate<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>>;
    fn plugin_update<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>>;
    fn plugin_delete<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>>;
    fn plugin_activate_network<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>>;
    fn plugin_deactivate_network<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>>;
    fn themes<'a>(&'a self, id: String, check_updates: bool) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpTheme>>>;
    fn theme_activate<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<()>>;
    fn theme_update<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>>;
    fn theme_delete<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>>;
    fn themes_network_enabled<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<String>>>;
    fn theme_enable_network<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<()>>;
    fn theme_disable_network<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<()>>;
    // ── the reads `wp_info` groups ──
    fn options<'a>(&'a self, id: String) -> OpFuture<'a, Result<crate::core::wordpress::WpOptionsForm>>;
    fn debug_get<'a>(&'a self, id: String) -> OpFuture<'a, Result<bool>>;
    fn debug_flag_get<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<bool>>;
    fn maintenance_get<'a>(&'a self, id: String) -> OpFuture<'a, Result<bool>>;
    fn permalink_get<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn languages<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpLanguage>>>;
    fn cron_events<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpCronEvent>>>;
    fn core_verify_checksums<'a>(&'a self, id: String) -> OpFuture<'a, Result<crate::core::wordpress::WpChecksumReport>>;
    fn primary_admin<'a>(&'a self, id: String) -> OpFuture<'a, Result<u64>>;
    // ── wp_user ──
    fn users<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpUser>>>;
    fn user_create<'a>(&'a self, id: String, login: String, email: String, role: String, password: String) -> OpFuture<'a, Result<()>>;
    fn user_set_password<'a>(&'a self, id: String, user_id: u64, password: String) -> OpFuture<'a, Result<()>>;
    fn user_set_role<'a>(&'a self, id: String, user_id: u64, role: String) -> OpFuture<'a, Result<()>>;
    fn user_delete<'a>(&'a self, id: String, user_id: u64, reassign: Option<u64>, delete_posts: bool) -> OpFuture<'a, Result<()>>;
    fn user_login_url<'a>(&'a self, id: String, user_id: u64) -> OpFuture<'a, Result<String>>;
    fn admin_login_url<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn super_admins<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<String>>>;
    fn super_admin_add<'a>(&'a self, id: String, user: String) -> OpFuture<'a, Result<()>>;
    // ── wp_option ──
    fn option_update<'a>(&'a self, id: String, name: String, value: String) -> OpFuture<'a, Result<()>>;
    fn debug_set<'a>(&'a self, id: String, on: bool) -> OpFuture<'a, Result<()>>;
    fn debug_flag_set<'a>(&'a self, id: String, name: String, on: bool) -> OpFuture<'a, Result<()>>;
    fn maintenance_set<'a>(&'a self, id: String, on: bool) -> OpFuture<'a, Result<()>>;
    fn permalink_set<'a>(&'a self, id: String, structure: String) -> OpFuture<'a, Result<()>>;
    fn switch_language<'a>(&'a self, id: String, locale: String) -> OpFuture<'a, Result<()>>;
    // ── wp_maintain ──
    fn cache_flush<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn rewrite_flush<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>>;
    fn transient_delete_all<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn cron_run_due<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn cron_run_hook<'a>(&'a self, id: String, hook: String) -> OpFuture<'a, Result<String>>;
    fn checksum_cleanup<'a>(&'a self, id: String, paths: Vec<String>) -> OpFuture<'a, Result<crate::core::wordpress::ChecksumCleanup>>;
    fn core_update<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn core_reinstall<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn core_switch_version<'a>(&'a self, id: String, version: String) -> OpFuture<'a, Result<crate::core::wordpress::WpCoreSwitch>>;
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
    wp: &'a dyn WpOps,
}

impl<'a> UserCtx<'a> {
    pub fn new(state: &'a AppState, ops: &'a dyn SiteOps, wp: &'a dyn WpOps, client: &'a str) -> Self {
        UserCtx { state, client, ops, wp }
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
    /// (`multisite` itself is the view's mode field, flattened in above.)
    #[serde(skip_serializing_if = "Option::is_none")]
    multisite_conversion: Option<String>,
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
            site: super::view::AgentSiteView::from_site(&site, serving, Vec::new()),
            url: format!("https://{}", site.domain),
            status: super::view::AgentSiteStatus::from_signals(&site, &signals),
            admin,
            multisite_conversion: multisite,
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
    let aliases = {
        let conn = ctx.db()?;
        crate::state::store::all_site_aliases(&conn)?.remove(&site.id).unwrap_or_default()
    };
    serde_json::to_value(super::view::AgentSiteView::from_site(&site, serving, aliases))
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


/// The WordPress tools' shared preconditions, checked on the ROW before any
/// permission is asked for: a scratch site is the agent's (the scratch tools
/// apply), and a site with no WordPress in it has nothing for wp-cli to boot.
/// Both are shape refusals — an ask for `read` on a Laravel site would prompt
/// the user about a permission that cannot be used.
fn wp_precheck(ctx: &UserCtx<'_>, site_id: &str, tool: &str) -> Result<()> {
    let conn = ctx.db()?;
    let Some(site) = crate::state::store::get_site(&conn, site_id)? else {
        return Err(Error::Other(format!("There is no site with id `{site_id}`. Use list_sites to see the sites that exist.")));
    };
    if site.is_scratch() {
        return Err(Error::Other(format!(
            "`{}` is a scratch site the agent created — {tool} is for the user's own sites. Use wp_run on it instead; no permission is needed.",
            site.domain
        )));
    }
    if site.site_type != SiteType::Wordpress {
        return Err(Error::Other(format!(
            "`{}` is a {} site, not WordPress — {tool} has nothing to run there.",
            site.domain,
            site.site_type.as_db()
        )));
    }
    Ok(())
}

/// One typed claim per scope, chosen by a VALUE — the grouped WordPress tools
/// decide the scope per ACTION (list reads, delete destroys) from a table the
/// tests hold, and this is the only place that table meets the witness types.
/// The witness is claimed, its site taken, and the witness dropped: a grouped
/// tool's handler holds no `Granted<S>` of a scope it did not ask for.
fn claim_scope(ctx: &UserCtx<'_>, site_id: &str, scope: Scope, wanted: &str) -> Result<(Site, bool)> {
    let take = |site: Option<&Site>| site.cloned().ok_or_else(|| Error::Other("this tool needs a site, not the stack.".into()));
    Ok(match scope {
        Scope::Read => {
            let c = ctx.claim::<scope::Read>(Some(site_id), wanted)?;
            (take(c.granted.site())?, c.auto_granted)
        }
        Scope::Manage => {
            let c = ctx.claim::<scope::Manage>(Some(site_id), wanted)?;
            (take(c.granted.site())?, c.auto_granted)
        }
        Scope::Destroy => {
            let c = ctx.claim::<scope::Destroy>(Some(site_id), wanted)?;
            (take(c.granted.site())?, c.auto_granted)
        }
        Scope::Run => {
            let c = ctx.claim::<scope::Run>(Some(site_id), wanted)?;
            (take(c.granted.site())?, c.auto_granted)
        }
        Scope::System => {
            let c = ctx.claim::<scope::System>(Some(site_id), wanted)?;
            (take(c.granted.site())?, c.auto_granted)
        }
    })
}

fn names_arg(args: &Value, tool: &str, action: &str) -> Result<Vec<String>> {
    let names: Vec<String> = args
        .get("names")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    if names.is_empty() {
        return Err(Error::Other(format!("{tool} `{action}` needs `names` — at least one slug.")));
    }
    Ok(names)
}

fn with_consent(mut value: Value, auto_granted: bool) -> Value {
    if auto_granted {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("consent".into(), Value::String(agent_grants::AUTO_GRANTED_NOTE.to_string()));
        }
    }
    value
}

fn to_json<T: Serialize>(v: T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| Error::Other(format!("serialising the reply: {e}")))
}

fn wp_info<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_info needs a `site_id`.".into()))?;
        let what = args.get("what").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_info needs `what`.".into()))?;
        const WHATS: &[&str] = &["info", "options", "debug", "maintenance", "permalinks", "languages", "cron", "checksums", "primary_admin"];
        if !WHATS.contains(&what) {
            return Err(Error::Other(format!("`{what}` is not something wp_info reads. Use one of: {}.", WHATS.join(", "))));
        }
        wp_precheck(&ctx, id, "wp_info")?;
        let (site, auto) = claim_scope(&ctx, id, Scope::Read, &format!("read its WordPress {what}"))?;
        acted.set(&site);
        let sid = site.id.clone();
        let wp = ctx.wp;
        let value = match what {
            "info" => to_json(wp.info(sid).await?)?,
            "options" => to_json(wp.options(sid).await?)?,
            "debug" => match args.get("flag").and_then(Value::as_str) {
                Some(flag) => json!({ "flag": flag, "on": wp.debug_flag_get(sid, flag.to_string()).await? }),
                None => json!({ "flag": "WP_DEBUG", "on": wp.debug_get(sid).await? }),
            },
            "maintenance" => json!({ "on": wp.maintenance_get(sid).await? }),
            "permalinks" => json!({ "structure": wp.permalink_get(sid).await? }),
            "languages" => to_json(wp.languages(sid).await?)?,
            "cron" => to_json(wp.cron_events(sid).await?)?,
            "checksums" => {
                // The report's raw wp-cli output names the docroot: scrubbed.
                let r = wp.core_verify_checksums(sid).await?;
                let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), &site.path);
                json!({ "ok": r.ok, "real": r.real, "benign": r.benign, "output": super::view::scrub_log_line(&r.output, &known) })
            }
            _ => json!({ "primaryAdminUserId": wp.primary_admin(sid).await? }),
        };
        Ok(with_consent(json!({ "domain": site.domain, "what": what, "result": value }), auto))
    })
}

/// Which scope each plugin action demands — the table the tests hold.
pub(crate) fn wp_plugin_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "list" => Scope::Read,
        "activate" | "deactivate" | "update" | "activate_network" | "deactivate_network" => Scope::Manage,
        "delete" => Scope::Destroy,
        _ => return None,
    })
}

/// Which scope each theme action demands.
pub(crate) fn wp_theme_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "list" | "network_enabled" => Scope::Read,
        "activate" | "update" | "enable_network" | "disable_network" => Scope::Manage,
        "delete" => Scope::Destroy,
        _ => return None,
    })
}

fn wp_plugin<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_plugin needs a `site_id`.".into()))?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_plugin needs an `action`.".into()))?;
        let scope = wp_plugin_scope(action).ok_or_else(|| {
            Error::Other(format!("`{action}` is not a wp_plugin action. Use list, activate, deactivate, update, delete, activate_network or deactivate_network."))
        })?;
        let names = if action == "list" { Vec::new() } else { names_arg(args, "wp_plugin", action)? };
        wp_precheck(&ctx, id, "wp_plugin")?;
        let wanted = if action == "list" { "list its plugins".to_string() } else { format!("{} the plugin(s) {}", action.replace('_', " "), names.join(", ")) };
        let (site, auto) = claim_scope(&ctx, id, scope, &wanted)?;
        acted.set(&site);
        let sid = site.id.clone();
        let wp = ctx.wp;
        let result = match action {
            "list" => to_json(wp.plugins(sid, args.get("check_updates").and_then(Value::as_bool).unwrap_or(false)).await?)?,
            "activate" => { wp.plugin_activate(sid, names.clone()).await?; json!({ "activated": names }) }
            "deactivate" => { wp.plugin_deactivate(sid, names.clone()).await?; json!({ "deactivated": names }) }
            "update" => { wp.plugin_update(sid, names.clone()).await?; json!({ "updated": names }) }
            "delete" => { wp.plugin_delete(sid, names.clone()).await?; json!({ "deleted": names }) }
            "activate_network" => { wp.plugin_activate_network(sid, names.clone()).await?; json!({ "networkActivated": names }) }
            _ => { wp.plugin_deactivate_network(sid, names.clone()).await?; json!({ "networkDeactivated": names }) }
        };
        Ok(with_consent(json!({ "domain": site.domain, "action": action, "result": result }), auto))
    })
}

fn wp_theme<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_theme needs a `site_id`.".into()))?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_theme needs an `action`.".into()))?;
        let scope = wp_theme_scope(action).ok_or_else(|| {
            Error::Other(format!("`{action}` is not a wp_theme action. Use list, activate, update, delete, network_enabled, enable_network or disable_network."))
        })?;
        let names = if matches!(action, "list" | "network_enabled") { Vec::new() } else { names_arg(args, "wp_theme", action)? };
        wp_precheck(&ctx, id, "wp_theme")?;
        let wanted = if names.is_empty() { format!("{} its themes", action.replace('_', " ")) } else { format!("{} the theme(s) {}", action.replace('_', " "), names.join(", ")) };
        let (site, auto) = claim_scope(&ctx, id, scope, &wanted)?;
        acted.set(&site);
        let sid = site.id.clone();
        let wp = ctx.wp;
        let one = |names: &[String]| names.first().cloned().unwrap_or_default();
        let result = match action {
            "list" => to_json(wp.themes(sid, args.get("check_updates").and_then(Value::as_bool).unwrap_or(false)).await?)?,
            "network_enabled" => json!({ "networkEnabled": wp.themes_network_enabled(sid).await? }),
            "activate" => { let n = one(&names); wp.theme_activate(sid, n.clone()).await?; json!({ "activated": n }) }
            "update" => { wp.theme_update(sid, names.clone()).await?; json!({ "updated": names }) }
            "delete" => { wp.theme_delete(sid, names.clone()).await?; json!({ "deleted": names }) }
            "enable_network" => { let n = one(&names); wp.theme_enable_network(sid, n.clone()).await?; json!({ "networkEnabled": n }) }
            _ => { let n = one(&names); wp.theme_disable_network(sid, n.clone()).await?; json!({ "networkDisabled": n }) }
        };
        Ok(with_consent(json!({ "domain": site.domain, "action": action, "result": result }), auto))
    })
}


pub(crate) fn wp_user_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "list" | "super_admins" => Scope::Read,
        "create" | "set_role" | "login_url" | "super_admin_add" => Scope::Manage,
        "set_password" | "delete" => Scope::Destroy,
        _ => return None,
    })
}

pub(crate) fn wp_maintain_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "cache_flush" | "rewrite_flush" | "transient_delete_all" | "cron_run_due" | "cron_run_hook" | "checksum_cleanup" | "core_update" | "core_reinstall" => Scope::Manage,
        "core_switch" => Scope::Destroy,
        _ => return None,
    })
}

fn u64_field(args: &Value, key: &str, tool: &str, action: &str) -> Result<u64> {
    args.get(key).and_then(Value::as_u64).ok_or_else(|| Error::Other(format!("{tool} `{action}` needs `{key}` (a number).")))
}

fn wp_user<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_user needs a `site_id`.".into()))?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_user needs an `action`.".into()))?;
        let scope = wp_user_scope(action).ok_or_else(|| {
            Error::Other(format!("`{action}` is not a wp_user action. Use list, create, set_role, login_url, super_admins, super_admin_add, set_password or delete."))
        })?;
        // Shape, per action, before any ask.
        let s_field = |k: &str| str_field(args, k, action);
        let wanted = match action {
            "list" => "list its users".to_string(),
            "super_admins" => "list its super admins".to_string(),
            "create" => format!("create the user `{}`", s_field("login")?),
            "set_role" => format!("make user {} a {}", u64_field(args, "user_id", "wp_user", action)?, s_field("role")?),
            "login_url" => "mint a one-time login link".to_string(),
            "super_admin_add" => format!("make `{}` a super admin", s_field("user")?),
            "set_password" => format!("reset the password of user {}", u64_field(args, "user_id", "wp_user", action)?),
            _ => {
                let uid = u64_field(args, "user_id", "wp_user", action)?;
                let reassign = args.get("reassign").and_then(Value::as_u64);
                let delete_posts = args.get("delete_posts").and_then(Value::as_bool).unwrap_or(false);
                // The #446 fork, stated here as well as in the app: neither or both is a refusal.
                if reassign.is_some() == delete_posts {
                    return Err(Error::Other(
                        "wp_user `delete` needs EXACTLY ONE of `reassign` (a user id to give the posts to) or `delete_posts: true` — deleting a user is always a decision about their posts, and rexenv will not guess.".into(),
                    ));
                }
                format!("delete user {uid} and {}", if delete_posts { "their posts".to_string() } else { format!("give their posts to user {}", reassign.unwrap()) })
            }
        };
        wp_precheck(&ctx, id, "wp_user")?;
        let (site, auto) = claim_scope(&ctx, id, scope, &wanted)?;
        acted.set(&site);
        let sid = site.id.clone();
        let wp = ctx.wp;
        let result = match action {
            "list" => to_json(wp.users(sid).await?)?,
            "super_admins" => json!({ "superAdmins": wp.super_admins(sid).await? }),
            "create" => {
                let (login, email, role) = (s_field("login")?.to_string(), s_field("email")?.to_string(), s_field("role")?.to_string());
                let generated = args.get("password").and_then(Value::as_str).filter(|p| !p.is_empty()).is_none();
                let password = if generated { crate::core::wordpress::generate_password() } else { args["password"].as_str().unwrap().to_string() };
                wp.user_create(sid, login.clone(), email, role, password.clone()).await?;
                json!({ "created": login, "password": password, "note": if generated { "Generated by rexenv and shown once — not recorded anywhere an agent can read it again." } else { "The password you supplied, shown once." } })
            }
            "set_role" => { let uid = args["user_id"].as_u64().unwrap(); wp.user_set_role(sid, uid, s_field("role")?.to_string()).await?; json!({ "userId": uid, "role": s_field("role")? }) }
            "login_url" => {
                let url = match args.get("user_id").and_then(Value::as_u64) {
                    Some(uid) => wp.user_login_url(sid, uid).await?,
                    None => wp.admin_login_url(sid).await?,
                };
                json!({ "loginUrl": url, "note": "One-time, expires quickly, logs in as that user. Hand it to the person you're working with or open it once yourself; it is not recorded." })
            }
            "super_admin_add" => { let u = s_field("user")?.to_string(); wp.super_admin_add(sid, u.clone()).await?; json!({ "superAdminAdded": u }) }
            "set_password" => {
                let uid = args["user_id"].as_u64().unwrap();
                let generated = args.get("password").and_then(Value::as_str).filter(|p| !p.is_empty()).is_none();
                let password = if generated { crate::core::wordpress::generate_password() } else { args["password"].as_str().unwrap().to_string() };
                wp.user_set_password(sid, uid, password.clone()).await?;
                json!({ "userId": uid, "password": password, "note": "Shown once." })
            }
            _ => {
                let uid = args["user_id"].as_u64().unwrap();
                let reassign = args.get("reassign").and_then(Value::as_u64);
                let delete_posts = args.get("delete_posts").and_then(Value::as_bool).unwrap_or(false);
                wp.user_delete(sid, uid, reassign, delete_posts).await?;
                json!({ "deleted": uid, "postsReassignedTo": reassign, "postsDeleted": delete_posts })
            }
        };
        Ok(with_consent(json!({ "domain": site.domain, "action": action, "result": result }), auto))
    })
}

fn wp_option<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_option needs a `site_id`.".into()))?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_option needs an `action`.".into()))?;
        let on = || args.get("on").and_then(Value::as_bool).ok_or_else(|| Error::Other(format!("wp_option `{action}` needs `on` (true or false).")));
        let wanted = match action {
            "update" => format!("set the option `{}`", str_field(args, "name", action)?),
            "debug" => format!("turn WP_DEBUG {}", if on()? { "on" } else { "off" }),
            "debug_flag" => format!("turn {} {}", str_field(args, "flag", action)?, if on()? { "on" } else { "off" }),
            "maintenance" => format!("turn maintenance mode {}", if on()? { "on" } else { "off" }),
            "permalinks" => format!("set permalinks to `{}`", str_field(args, "structure", action)?),
            "language" => format!("switch its language to {}", str_field(args, "locale", action)?),
            other => return Err(Error::Other(format!("`{other}` is not a wp_option action. Use update, debug, debug_flag, maintenance, permalinks or language."))),
        };
        wp_precheck(&ctx, id, "wp_option")?;
        let (site, auto) = claim_scope(&ctx, id, Scope::Manage, &wanted)?;
        acted.set(&site);
        let sid = site.id.clone();
        let wp = ctx.wp;
        match action {
            "update" => wp.option_update(sid, str_field(args, "name", action)?.to_string(), args.get("value").and_then(Value::as_str).unwrap_or("").to_string()).await?,
            "debug" => wp.debug_set(sid, on()?).await?,
            "debug_flag" => wp.debug_flag_set(sid, str_field(args, "flag", action)?.to_string(), on()?).await?,
            "maintenance" => wp.maintenance_set(sid, on()?).await?,
            "permalinks" => wp.permalink_set(sid, str_field(args, "structure", action)?.to_string()).await?,
            _ => wp.switch_language(sid, str_field(args, "locale", action)?.to_string()).await?,
        }
        Ok(with_consent(json!({ "domain": site.domain, "action": action, "done": true, "detail": wanted }), auto))
    })
}

fn wp_maintain<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_maintain needs a `site_id`.".into()))?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_maintain needs an `action`.".into()))?;
        let scope = wp_maintain_scope(action).ok_or_else(|| {
            Error::Other(format!("`{action}` is not a wp_maintain action. Use cache_flush, rewrite_flush, transient_delete_all, cron_run_due, cron_run_hook, checksum_cleanup, core_update, core_reinstall or core_switch."))
        })?;
        let wanted = match action {
            "cron_run_hook" => format!("run the cron hook `{}`", str_field(args, "hook", action)?),
            "core_switch" => format!("switch WordPress to {}", str_field(args, "version", action)?),
            "checksum_cleanup" => "remove files that are not WordPress's own".to_string(),
            other => other.replace('_', " "),
        };
        wp_precheck(&ctx, id, "wp_maintain")?;
        let (site, auto) = claim_scope(&ctx, id, scope, &wanted)?;
        acted.set(&site);
        let sid = site.id.clone();
        let wp = ctx.wp;
        // wp-cli's own words come back in several of these; every one goes
        // through the one scrubber (#201) — a flush names nothing, an update
        // and a checksum report both name the docroot.
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), &site.path);
        let scrub = |t: String| super::view::scrub_log_line(&t, &known);
        let result = match action {
            "cache_flush" => json!({ "output": scrub(wp.cache_flush(sid).await?) }),
            "rewrite_flush" => { wp.rewrite_flush(sid).await?; json!({ "done": true }) }
            "transient_delete_all" => json!({ "output": scrub(wp.transient_delete_all(sid).await?) }),
            "cron_run_due" => json!({ "output": scrub(wp.cron_run_due(sid).await?) }),
            "cron_run_hook" => json!({ "output": scrub(wp.cron_run_hook(sid, str_field(args, "hook", action)?.to_string()).await?) }),
            "checksum_cleanup" => {
                let paths: Vec<String> = args.get("paths").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
                if paths.is_empty() {
                    return Err(Error::Other("wp_maintain `checksum_cleanup` needs `paths` — the files wp_info `checksums` reported.".into()));
                }
                let r = wp.checksum_cleanup(sid, paths).await?;
                json!({ "removed": r.removed, "skipped": r.skipped.len(), "reportOk": r.report.ok, "output": scrub(r.report.output) })
            }
            "core_update" => json!({ "output": scrub(wp.core_update(sid).await?) }),
            "core_reinstall" => json!({ "output": scrub(wp.core_reinstall(sid).await?) }),
            _ => {
                let r = wp.core_switch_version(sid, str_field(args, "version", action)?.to_string()).await?;
                json!({ "version": r.version, "dbUpdateRequired": r.db_update_required })
            }
        };
        Ok(with_consent(json!({ "domain": site.domain, "action": action, "result": result }), auto))
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

    /// The WordPress commands, recorded; the reads answer canned shapes.
    impl WpOps for FakeOps {
        fn info<'a>(&'a self, id: String) -> OpFuture<'a, Result<crate::core::wordpress::WpInfo>> {
            self.calls.lock().unwrap().push(format!("wp info {id}"));
            Box::pin(async { Ok(crate::core::wordpress::WpInfo { is_wordpress: true, version: Some("6.6".into()), multisite: false }) })
        }
        fn plugins<'a>(&'a self, id: String, check_updates: bool) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpPlugin>>> {
            self.calls.lock().unwrap().push(format!("wp plugins {id} {check_updates}"));
            Box::pin(async { Ok(vec![]) })
        }
        fn plugin_activate<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp plugin activate {id} {}", names.join(",")));
            Box::pin(async { Ok(()) })
        }
        fn plugin_deactivate<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp plugin deactivate {id} {}", names.join(",")));
            Box::pin(async { Ok(()) })
        }
        fn plugin_update<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp plugin update {id} {}", names.join(",")));
            Box::pin(async { Ok(()) })
        }
        fn plugin_delete<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp plugin delete {id} {}", names.join(",")));
            Box::pin(async { Ok(()) })
        }
        fn plugin_activate_network<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp plugin activate_network {id} {}", names.join(",")));
            Box::pin(async { Ok(()) })
        }
        fn plugin_deactivate_network<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp plugin deactivate_network {id} {}", names.join(",")));
            Box::pin(async { Ok(()) })
        }
        fn themes<'a>(&'a self, id: String, check_updates: bool) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpTheme>>> {
            self.calls.lock().unwrap().push(format!("wp themes {id} {check_updates}"));
            Box::pin(async { Ok(vec![]) })
        }
        fn theme_activate<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp theme activate {id} {name}"));
            Box::pin(async { Ok(()) })
        }
        fn theme_update<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp theme update {id} {}", names.join(",")));
            Box::pin(async { Ok(()) })
        }
        fn theme_delete<'a>(&'a self, id: String, names: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp theme delete {id} {}", names.join(",")));
            Box::pin(async { Ok(()) })
        }
        fn themes_network_enabled<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<String>>> {
            self.calls.lock().unwrap().push(format!("wp themes network_enabled {id}"));
            Box::pin(async { Ok(vec!["twentytwentyfour".into()]) })
        }
        fn theme_enable_network<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp theme enable_network {id} {name}"));
            Box::pin(async { Ok(()) })
        }
        fn theme_disable_network<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp theme disable_network {id} {name}"));
            Box::pin(async { Ok(()) })
        }
        fn options<'a>(&'a self, id: String) -> OpFuture<'a, Result<crate::core::wordpress::WpOptionsForm>> {
            self.calls.lock().unwrap().push(format!("wp options {id}"));
            Box::pin(async { Ok(crate::core::wordpress::WpOptionsForm { fields: vec![], timezones: vec![], roles: vec![] }) })
        }
        fn debug_get<'a>(&'a self, id: String) -> OpFuture<'a, Result<bool>> {
            self.calls.lock().unwrap().push(format!("wp debug_get {id}"));
            Box::pin(async { Ok(true) })
        }
        fn debug_flag_get<'a>(&'a self, id: String, name: String) -> OpFuture<'a, Result<bool>> {
            self.calls.lock().unwrap().push(format!("wp debug_flag_get {id} {name}"));
            Box::pin(async { Ok(false) })
        }
        fn maintenance_get<'a>(&'a self, id: String) -> OpFuture<'a, Result<bool>> {
            self.calls.lock().unwrap().push(format!("wp maintenance_get {id}"));
            Box::pin(async { Ok(false) })
        }
        fn permalink_get<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp permalink_get {id}"));
            Box::pin(async { Ok("/%postname%/".into()) })
        }
        fn languages<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpLanguage>>> {
            self.calls.lock().unwrap().push(format!("wp languages {id}"));
            Box::pin(async { Ok(vec![]) })
        }
        fn cron_events<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpCronEvent>>> {
            self.calls.lock().unwrap().push(format!("wp cron_events {id}"));
            Box::pin(async { Ok(vec![]) })
        }
        fn core_verify_checksums<'a>(&'a self, id: String) -> OpFuture<'a, Result<crate::core::wordpress::WpChecksumReport>> {
            self.calls.lock().unwrap().push(format!("wp checksums {id}"));
            Box::pin(async {
                Ok(crate::core::wordpress::WpChecksumReport {
                    ok: false,
                    real: vec!["wp-includes/x.php".into()],
                    benign: vec![],
                    output: "Warning: File doesn't verify: /Users/somebody/Library/Application Support/rexenv/Sites/mine.rex/wp-includes/x.php".into(),
                })
            })
        }
        fn primary_admin<'a>(&'a self, id: String) -> OpFuture<'a, Result<u64>> {
            self.calls.lock().unwrap().push(format!("wp primary_admin {id}"));
            Box::pin(async { Ok(1) })
        }
        fn users<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpUser>>> {
            self.calls.lock().unwrap().push(format!("wp users {id}"));
            Box::pin(async { Ok(vec![]) })
        }
        fn user_create<'a>(&'a self, id: String, login: String, email: String, role: String, password: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp user create {id} {login} {email} {role} {password}"));
            Box::pin(async { Ok(()) })
        }
        fn user_set_password<'a>(&'a self, id: String, user_id: u64, password: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp user set_password {id} {user_id} {password}"));
            Box::pin(async { Ok(()) })
        }
        fn user_set_role<'a>(&'a self, id: String, user_id: u64, role: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp user set_role {id} {user_id} {role}"));
            Box::pin(async { Ok(()) })
        }
        fn user_delete<'a>(&'a self, id: String, user_id: u64, reassign: Option<u64>, delete_posts: bool) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp user delete {id} {user_id} {reassign:?} {delete_posts}"));
            Box::pin(async { Ok(()) })
        }
        fn user_login_url<'a>(&'a self, id: String, user_id: u64) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp user login_url {id} {user_id}"));
            Box::pin(async { Ok("https://blog.rex/?rexenv_login=tok".into()) })
        }
        fn admin_login_url<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp admin login_url {id}"));
            Box::pin(async { Ok("https://blog.rex/?rexenv_login=admintok".into()) })
        }
        fn super_admins<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<String>>> {
            self.calls.lock().unwrap().push(format!("wp super_admins {id}"));
            Box::pin(async { Ok(vec!["admin".into()]) })
        }
        fn super_admin_add<'a>(&'a self, id: String, user: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp super_admin_add {id} {user}"));
            Box::pin(async { Ok(()) })
        }
        fn option_update<'a>(&'a self, id: String, name: String, value: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp option update {id} {name}={value}"));
            Box::pin(async { Ok(()) })
        }
        fn debug_set<'a>(&'a self, id: String, on: bool) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp debug_set {id} {on}"));
            Box::pin(async { Ok(()) })
        }
        fn debug_flag_set<'a>(&'a self, id: String, name: String, on: bool) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp debug_flag_set {id} {name} {on}"));
            Box::pin(async { Ok(()) })
        }
        fn maintenance_set<'a>(&'a self, id: String, on: bool) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp maintenance_set {id} {on}"));
            Box::pin(async { Ok(()) })
        }
        fn permalink_set<'a>(&'a self, id: String, structure: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp permalink_set {id} {structure}"));
            Box::pin(async { Ok(()) })
        }
        fn switch_language<'a>(&'a self, id: String, locale: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp switch_language {id} {locale}"));
            Box::pin(async { Ok(()) })
        }
        fn cache_flush<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp cache_flush {id}"));
            Box::pin(async { Ok("Success: The cache was flushed.".into()) })
        }
        fn rewrite_flush<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp rewrite_flush {id}"));
            Box::pin(async { Ok(()) })
        }
        fn transient_delete_all<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp transient_delete_all {id}"));
            Box::pin(async { Ok("Success: 3 transients deleted.".into()) })
        }
        fn cron_run_due<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp cron_run_due {id}"));
            Box::pin(async { Ok("Executed 2 events.".into()) })
        }
        fn cron_run_hook<'a>(&'a self, id: String, hook: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp cron_run_hook {id} {hook}"));
            Box::pin(async { Ok("Executed the cron event 'x'.".into()) })
        }
        fn checksum_cleanup<'a>(&'a self, id: String, paths: Vec<String>) -> OpFuture<'a, Result<crate::core::wordpress::ChecksumCleanup>> {
            self.calls.lock().unwrap().push(format!("wp checksum_cleanup {id} {}", paths.join(",")));
            Box::pin(async {
                Ok(crate::core::wordpress::ChecksumCleanup {
                    removed: 1,
                    skipped: vec![],
                    report: crate::core::wordpress::WpChecksumReport { ok: true, real: vec![], benign: vec![], output: "Success: WordPress installation verifies against checksums.".into() },
                })
            })
        }
        fn core_update<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp core_update {id}"));
            Box::pin(async { Ok("Updating to version 6.7 (/Users/somebody/Library/Application Support/rexenv/Sites/blog.rex)…".into()) })
        }
        fn core_reinstall<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp core_reinstall {id}"));
            Box::pin(async { Ok("Success: WordPress reinstalled.".into()) })
        }
        fn core_switch_version<'a>(&'a self, id: String, version: String) -> OpFuture<'a, Result<crate::core::wordpress::WpCoreSwitch>> {
            self.calls.lock().unwrap().push(format!("wp core_switch {id} {version}"));
            Box::pin(async move { Ok(crate::core::wordpress::WpCoreSwitch { version, db_update_required: true }) })
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
        let ctx = UserCtx::new(&state, &ops, &ops, "claude-code");
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
        assert!(v.get("multisiteConversion").is_none());
        assert_eq!(v["multisite"], "none", "the view's own mode field");
        assert_eq!(v["owner"], "user");
        assert_eq!(acted.take().as_deref(), Some("11111111-2222-4333-8444-555555555555"), "the feed names what was made");
        assert!(asks(&state).is_empty(), "the grant answered the ask");

        // A blank PHP site: no `wp`, no admin block; `multisite` runs the
        // SECOND operation and reports it in the reply.
        let v = run(json!({ "name": "Net", "domain": "net.rex", "type": "wordpress", "multisite": "subdirectory" })).await.unwrap();
        assert_eq!(ops.converted.lock().unwrap().as_slice(), &[("11111111-2222-4333-8444-555555555555".to_string(), "subdirectory".to_string())]);
        assert!(v["multisiteConversion"].as_str().unwrap().contains("subdirectory"));
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
        let ctx = UserCtx::new(&state, &ops, &ops, "claude-code");
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
        let ctx = UserCtx::new(&state, &ops, &ops, "claude-code");
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
        let ctx = UserCtx::new(&state, &ops, &ops, "claude-code");
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

    /// **`site_inspect_folder` is the dialog's own preflight: it classifies a
    /// real folder without creating anything, and refuses the folders the
    /// dialog refuses with the dialog's reason.**
    #[test]
    fn inspect_folder_classifies_without_creating_and_refuses_what_the_dialog_refuses() {
        let state = app_state();
        let read = super::super::readctx::ReadCtx::new(&state);
        let dir = std::env::temp_dir().join(format!("rexenv-inspect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("public")).unwrap();
        std::fs::write(dir.join("artisan"), "#!/usr/bin/env php\n").unwrap();
        std::fs::write(dir.join("public/index.php"), "<?php\n").unwrap();
        let found = read.inspect_folder(&dir.display().to_string()).unwrap();
        assert_eq!(found.site_type, SiteType::Laravel);
        assert_eq!(found.docroot_rel, "public");
        assert!(std::fs::read_dir(&dir).unwrap().count() == 2, "nothing was created in the folder");
        // The home folder: the dialog's refusal, verbatim through the same function.
        let home = directories::BaseDirs::new().unwrap().home_dir().display().to_string();
        assert!(read.inspect_folder(&home).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The grouped WordPress tools decide the scope per ACTION from one table,
    /// refuse a scratch or non-WordPress site before asking, claim exactly that
    /// scope, and run exactly the app command — with wp-cli's own text scrubbed.**
    #[tokio::test]
    async fn wp_tools_claim_per_action_and_refuse_non_wordpress_before_asking() {
        // The table itself: list reads, delete destroys, everything else manages.
        assert_eq!(wp_plugin_scope("list"), Some(Scope::Read));
        assert_eq!(wp_plugin_scope("delete"), Some(Scope::Destroy));
        for a in ["activate", "deactivate", "update", "activate_network", "deactivate_network"] {
            assert_eq!(wp_plugin_scope(a), Some(Scope::Manage), "{a}");
        }
        assert_eq!(wp_plugin_scope("install"), None, "install is a job, not here");
        assert_eq!(wp_theme_scope("list"), Some(Scope::Read));
        assert_eq!(wp_theme_scope("network_enabled"), Some(Scope::Read));
        assert_eq!(wp_theme_scope("delete"), Some(Scope::Destroy));
        assert_eq!(wp_theme_scope("enable_network"), Some(Scope::Manage));

        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, "claude-code");
        let wp_site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "blog.rex", SiteOrigin::User);
        let mut php_site = test_site("bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee", "plain.rex", SiteOrigin::User);
        php_site.site_type = SiteType::Php;
        let theirs = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        {
            let conn = state.db.lock().unwrap();
            for s in [&wp_site, &php_site, &theirs] {
                store::insert_site(&conn, s).unwrap();
            }
        }
        // Shape and precheck refusals — no ask for any of them.
        let err = wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "install", "names": ["x"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not a wp_plugin action"), "{err}");
        let err = wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "activate" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("needs `names`"), "{err}");
        let err = wp_info(ctx, &json!({ "site_id": php_site.id, "what": "info" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not WordPress"), "{err}");
        let err = wp_theme(ctx, &json!({ "site_id": theirs.id, "action": "list" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("wp_run"), "a scratch site is sent to the scratch tools: {err}");
        assert!(asks(&state).is_empty(), "none of those asks for anything");

        // No grant: each action asks for ITS scope.
        assert!(wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "list" }), &acted).await.is_err());
        assert!(wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "delete", "names": ["akismet"] }), &acted).await.is_err());
        let a = asks(&state);
        let scopes: Vec<Scope> = a.iter().map(|r| r.scope).collect();
        assert!(scopes.contains(&Scope::Read) && scopes.contains(&Scope::Destroy), "{a:?}");
        assert!(a.iter().any(|r| r.wanted.contains("delete the plugin(s) akismet")), "{a:?}");

        // A `manage` grant covers list (implication) and activate, not delete.
        {
            let conn = state.db.lock().unwrap();
            store::grant_agent_site(&conn, "g1", Some(&wp_site.id), "claude-code", "manage", 7, false, false).unwrap();
        }
        let v = wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "list", "check_updates": true }), &acted).await.unwrap();
        assert_eq!(v["action"], "list");
        let v = wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "activate", "names": ["akismet", "hello"] }), &acted).await.unwrap();
        assert_eq!(v["result"]["activated"], json!(["akismet", "hello"]));
        assert!(wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "delete", "names": ["akismet"] }), &acted).await.is_err(), "manage does not delete");
        let v = wp_theme(ctx, &json!({ "site_id": wp_site.id, "action": "activate", "names": ["twentytwentyfour"] }), &acted).await.unwrap();
        assert_eq!(v["result"]["activated"], "twentytwentyfour");
        let v = wp_theme(ctx, &json!({ "site_id": wp_site.id, "action": "network_enabled" }), &acted).await.unwrap();
        assert_eq!(v["result"]["networkEnabled"], json!(["twentytwentyfour"]));
        // Every `what` of wp_info reaches its read; the checksum output is scrubbed.
        for what in ["info", "options", "debug", "maintenance", "permalinks", "languages", "cron", "checksums", "primary_admin"] {
            let v = wp_info(ctx, &json!({ "site_id": wp_site.id, "what": what }), &acted).await.unwrap();
            assert_eq!(v["what"], what);
        }
        let v = wp_info(ctx, &json!({ "site_id": wp_site.id, "what": "debug", "flag": "SCRIPT_DEBUG" }), &acted).await.unwrap();
        assert_eq!(v["result"]["flag"], "SCRIPT_DEBUG");
        let v = wp_info(ctx, &json!({ "site_id": wp_site.id, "what": "checksums" }), &acted).await.unwrap();
        let out = v["result"]["output"].as_str().unwrap();
        assert!(!out.contains("/Users/somebody"), "wp-cli's own text is scrubbed: {out}");

        // A session `destroy` grant lets delete through.
        {
            let conn = state.db.lock().unwrap();
            store::grant_agent_site(&conn, "g2", Some(&wp_site.id), "claude-code", "destroy", 1, false, true).unwrap();
        }
        let v = wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "delete", "names": ["akismet"] }), &acted).await.unwrap();
        assert_eq!(v["result"]["deleted"], json!(["akismet"]));
        let calls = ops.calls.lock().unwrap().clone();
        let id = wp_site.id.as_str();
        for expect in [
            format!("wp plugins {id} true"), format!("wp plugin activate {id} akismet,hello"), format!("wp plugin delete {id} akismet"),
            format!("wp theme activate {id} twentytwentyfour"), format!("wp themes network_enabled {id}"),
            format!("wp info {id}"), format!("wp debug_flag_get {id} SCRIPT_DEBUG"), format!("wp checksums {id}"), format!("wp primary_admin {id}"),
        ] {
            assert!(calls.contains(&expect), "missing app call {expect:?} in {calls:?}");
        }
        assert_eq!(acted.take().as_deref(), Some(id));
    }

    /// **`wp_user`, `wp_option`, `wp_maintain`: the per-action tables, the #446
    /// fork restated before any ask, a generated password shown once, a login
    /// link never summarised, wp-cli's words scrubbed.**
    #[tokio::test]
    async fn wp_user_option_and_maintain_claim_per_action_and_keep_secrets_out_of_the_feed() {
        assert_eq!(wp_user_scope("list"), Some(Scope::Read));
        assert_eq!(wp_user_scope("create"), Some(Scope::Manage));
        assert_eq!(wp_user_scope("login_url"), Some(Scope::Manage));
        assert_eq!(wp_user_scope("set_password"), Some(Scope::Destroy));
        assert_eq!(wp_user_scope("delete"), Some(Scope::Destroy));
        assert_eq!(wp_maintain_scope("core_update"), Some(Scope::Manage));
        assert_eq!(wp_maintain_scope("core_switch"), Some(Scope::Destroy));

        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "blog.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
            store::grant_agent_site(&conn, "g1", Some(&site.id), "claude-code", "manage", 7, false, false).unwrap();
        }
        // The delete fork is a SHAPE refusal — neither, and both, before any ask.
        for args in [json!({ "site_id": site.id, "action": "delete", "user_id": 5 }), json!({ "site_id": site.id, "action": "delete", "user_id": 5, "reassign": 1, "delete_posts": true })] {
            let err = wp_user(ctx, &args, &acted).await.unwrap_err().to_string();
            assert!(err.contains("EXACTLY ONE"), "{err}");
        }
        assert!(asks(&state).is_empty());
        // Delete needs destroy: a manage grant asks, with the posts decision in the verb.
        let err = wp_user(ctx, &json!({ "site_id": site.id, "action": "delete", "user_id": 5, "reassign": 1 }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "{err}");
        assert!(asks(&state)[0].wanted.contains("give their posts to user 1"));

        // create with no password: generated, returned once, never in the feed summary.
        let v = wp_user(ctx, &json!({ "site_id": site.id, "action": "create", "login": "bob", "email": "b@x.rex", "role": "editor" }), &acted).await.unwrap();
        let pw = v["result"]["password"].as_str().unwrap().to_string();
        assert!(pw.len() >= 12, "a generated password: {pw}");
        assert!(v["result"]["note"].as_str().unwrap().contains("shown once"));
        let summary = (registry().iter().find(|t| t.name == "wp_user").unwrap().summarise)(&json!({ "action": "create", "password": pw.clone() }));
        assert_eq!(summary.as_deref(), Some("user create"), "the summary is the verb, never the value");
        // login_url comes back, and nothing about it reaches a summary.
        let v = wp_user(ctx, &json!({ "site_id": site.id, "action": "login_url" }), &acted).await.unwrap();
        assert!(v["result"]["loginUrl"].as_str().unwrap().contains("rexenv_login="));
        let v = wp_user(ctx, &json!({ "site_id": site.id, "action": "set_role", "user_id": 7, "role": "author" }), &acted).await.unwrap();
        assert_eq!(v["result"]["role"], "author");

        // wp_option: every action one app call.
        for (args, expect) in [
            (json!({ "action": "update", "name": "blogname", "value": "Hi" }), "wp option update"),
            (json!({ "action": "debug", "on": true }), "wp debug_set"),
            (json!({ "action": "debug_flag", "flag": "SCRIPT_DEBUG", "on": false }), "wp debug_flag_set"),
            (json!({ "action": "maintenance", "on": true }), "wp maintenance_set"),
            (json!({ "action": "permalinks", "structure": "/%postname%/" }), "wp permalink_set"),
            (json!({ "action": "language", "locale": "de_DE" }), "wp switch_language"),
        ] {
            let mut a = args.clone();
            a["site_id"] = json!(site.id);
            let v = wp_option(ctx, &a, &acted).await.unwrap();
            assert_eq!(v["done"], true);
            assert!(ops.calls.lock().unwrap().iter().any(|c| c.starts_with(expect)), "missing {expect}");
        }
        let err = wp_option(ctx, &json!({ "site_id": site.id, "action": "debug" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("needs `on`"), "{err}");

        // wp_maintain: manage actions run; core_update's output loses the docroot;
        // core_switch needs destroy.
        let v = wp_maintain(ctx, &json!({ "site_id": site.id, "action": "core_update" }), &acted).await.unwrap();
        let out = v["result"]["output"].as_str().unwrap();
        assert!(!out.contains("/Users/somebody") && out.contains("<"), "scrubbed: {out}");
        let err = wp_maintain(ctx, &json!({ "site_id": site.id, "action": "core_switch", "version": "6.5" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "{err}");
        let err = wp_maintain(ctx, &json!({ "site_id": site.id, "action": "checksum_cleanup" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("needs `paths`"), "{err}");
        {
            let conn = state.db.lock().unwrap();
            store::grant_agent_site(&conn, "g2", Some(&site.id), "claude-code", "destroy", 1, false, true).unwrap();
        }
        let v = wp_maintain(ctx, &json!({ "site_id": site.id, "action": "core_switch", "version": "6.5" }), &acted).await.unwrap();
        assert_eq!(v["result"]["dbUpdateRequired"], true);
        let v = wp_user(ctx, &json!({ "site_id": site.id, "action": "delete", "user_id": 5, "delete_posts": true }), &acted).await.unwrap();
        assert_eq!(v["result"]["postsDeleted"], true);
        let calls = ops.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|c| c == &format!("wp user delete {} 5 None true", site.id)), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with(&format!("wp user create {} bob b@x.rex editor ", site.id))), "{calls:?}");
    }
}
