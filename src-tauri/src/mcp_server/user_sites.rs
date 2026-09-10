//! Parity tools — the THIRD registry: tools that act on the USER's own sites
//! and on the stack, each behind a scope grant the user gave in the app
//! (`docs/archive/PLAN-mcp-parity.md` §3, `core::agent_grants`).
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
                      `db_engine` (mysql / mariadb / postgres — not for WordPress), `blueprint` (a saved blueprint's name), and for \
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
                      `list`, `super_admins` and `login_url` {user_id — omit for the primary \
                      administrator; a one-time link that signs into wp-admin WITHOUT a password — \
                      open it in a browser, or fetch it once headlessly with a cookie jar; single-use, \
                      expires in two minutes, never recorded, changes nothing about the account} need \
                      `read`; `create` {login, email, role, password — omit to have rexenv generate \
                      one, returned ONCE}, `set_role` {user_id, role}, `super_admin_add` {user} need \
                      `manage`; `set_password` {user_id, password} and `delete` \
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
        name: "wp_data",
        description: "Data on a WordPress site the user owns. Takes `site_id` and `action`: \
                      `db_export` (a SQL dump into the user's Downloads folder — the reply names the \
                      file, not the path) and `content_export` (WXR files, same place) need \
                      `manage`; `search_replace` {from, to, dry_run} — with `dry_run: true` it \
                      only counts and needs `manage`; with `dry_run: false` it rewrites the \
                      database in place and needs `destroy`; `db_import` {path — a .sql file on \
                      this machine, replaces the database} and `reset` (a fresh WordPress over the \
                      same site — everything in it is lost) need `destroy`.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["db_export", "content_export", "search_replace", "db_import", "reset"] },
                "from": { "type": "string" }, "to": { "type": "string" }, "dry_run": { "type": "boolean" },
                "path": { "type": "string", "description": "db_import: absolute path to the .sql file." }
            },
            "required": ["site_id", "action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "search_replace", "from": "a", "to": "b", "dry_run": true }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("data {a}")),
        scope: Scope::Destroy,
        handler: wp_data,
    },
    UserTool {
        name: "wp_network",
        description: "Multisite on a WordPress site the user owns. Takes `site_id` and `action`: \
                      `sites` (the network's sites) needs `read`; `convert` {mode: subdomain / \
                      subdirectory} (turn a single site into a network — the app refuses it while \
                      the site is shared publicly) and `site_create` {slug} need `manage`; \
                      `site_delete` {blog_id} needs `destroy`.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["sites", "convert", "site_create", "site_delete"] },
                "mode": { "type": "string", "enum": ["subdomain", "subdirectory"] },
                "slug": { "type": "string" }, "blog_id": { "type": "string" }
            },
            "required": ["site_id", "action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "sites" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("network {a}")),
        scope: Scope::Destroy,
        handler: wp_network,
    },
    UserTool {
        name: "site_wp_run",
        description: "Run a raw WP-CLI command in a WordPress site the user owns — the same shape as \
                      wp_run on a scratch site, but on THEIR site and as THEM, so it needs the \
                      user's `run` permission on that site (asked for in the app; it can never be \
                      auto-allowed away for `destroy`, and `run` is its own decision). Takes \
                      `site_id` and `args` (the command as an array of words WITHOUT `wp`). rexenv \
                      decides which site it runs against: `--path`, `--url`, `--ssh`, `--http` and \
                      `@aliases` are refused. Prefer the vetted tools (wp_plugin, wp_user, …) when \
                      one fits; this is for a plugin's own commands and `wp eval`. The exit code, \
                      stdout and stderr come back; a non-zero exit is an answer, not a tool \
                      failure — check `succeeded`.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "args": { "type": "array", "items": { "type": "string" }, "description": "The WP-CLI command as separate words, without `wp` and without `--path`." }
            },
            "required": ["site_id", "args"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "args": ["option", "get", "home"] }),
        summarise: super::scratch::summarise_wp_run_public,
        scope: Scope::Run,
        handler: site_wp_run,
    },
    UserTool {
        name: "site_artisan",
        description: "Run `php artisan …` in a Laravel site the user owns, on the PHP the site \
                      serves with, in its project root — needs the user's `run` permission on \
                      that site (asked for in the app). Takes `site_id` and `args` (the artisan \
                      command as an array of words WITHOUT `php artisan`: [\"migrate\", \
                      \"--seed\"]). Non-interactive: rexenv appends `--no-interaction` and gives no \
                      stdin, so `tinker` exits instead of waiting (use `--execute`) and a command \
                      that would prompt answers itself no. Laravel's destructive commands \
                      (`db:wipe`, `migrate:fresh`) only confirm in production — in this local \
                      site they run at once, so do not use them to test the prompt. The exit \
                      code, stdout and stderr come back; a non-zero exit is an answer, not a \
                      tool failure — check `succeeded`. Only for Laravel sites that finished \
                      installing; scratch sites are WordPress and have no artisan.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "args": { "type": "array", "items": { "type": "string" }, "description": "The artisan command as separate words, without `php artisan`." }
            },
            "required": ["site_id", "args"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "args": ["about"] }),
        summarise: super::scratch::summarise_wp_run_public,
        scope: Scope::Run,
        handler: site_artisan,
    },
    UserTool {
        name: "composer_link",
        description: "Link a Composer package you are developing into one of the user's own \
                      PHP or Laravel sites as a `path` repository — `composer config \
                      repositories.<name> path <source>` then `composer require <name>:@dev`, \
                      on the site's PHP with rexenv's pinned Composer. Takes `site_id` and \
                      `source` (the package checkout's directory; its composer.json `name` is \
                      read from there, never guessed). Needs the user's `run` permission on the \
                      site: Composer runs the package's scripts as the user. THE LINK IS A \
                      SYMLINK: the site runs the checkout live, edits show without a sync, and \
                      anything the site writes under vendor/<name> lands in the checkout. The \
                      home folder, Desktop/Documents/Downloads, a volume root, rexenv's own \
                      folders and any site's folder are refused as a source. For a WordPress \
                      plugin use wp_plugin, or scratch_add_package on a scratch site.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "source": { "type": "string", "description": "Absolute path of the package checkout — the directory holding its composer.json." }
            },
            "required": ["site_id", "source"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "source": "/Users/somebody/Projects/acme-widgets" }),
        summarise: |v| v.get("package").and_then(Value::as_str).map(|p| format!("linked {p}")),
        scope: Scope::Run,
        handler: composer_link,
    },
    UserTool {
        name: "site_logs",
        description: "Every log that concerns one of the user's own sites — its web server, PHP \
                      pool, edge and database logs (shared across sites) and, for WordPress, its \
                      debug log. Takes `site_id`; without `source` it lists the sources by key; \
                      with `source` (a key from that list, or `wp-debug`) it returns the most \
                      recent lines (default 100, max 200), with rexenv's login tokens, cookie \
                      headers and the paths rexenv knows removed — otherwise the raw log. Needs \
                      the user's `read` permission on the site (the free tail_log covers only the \
                      WordPress debug log).",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "source": { "type": "string", "description": "A key from the list, or `wp-debug`. Omit to list." },
                "lines": { "type": "integer", "description": "Max 200; default 100." }
            },
            "required": ["site_id"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id }),
        summarise: |args| args.get("source").and_then(Value::as_str).map(str::to_string),
        scope: Scope::Read,
        handler: site_logs,
    },
    UserTool {
        name: "mail_inbox",
        description: "The user's whole Mailpit inbox — every message every site on this machine \
                      sent, including password-reset links for their own sites. Takes `action`: \
                      `list` {query?, unread?, limit?}, `get` {message_id}, `raw` {message_id} are \
                      reads — free at the Agent access dial's Read, on whenever the endpoint is \
                      (the inbox is shared, not a site's); `mark_read` needs `manage`; \
                      `delete` {message_ids} and `clear` need `destroy`. For a scratch site's own \
                      mail use mail_list / mail_get, which need no permission.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "get", "raw", "mark_read", "delete", "clear"] },
                "query": { "type": "string" }, "unread": { "type": "boolean" }, "limit": { "type": "integer" },
                "message_id": { "type": "string" }, "message_ids": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "action": "list" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("inbox {a}")),
        scope: Scope::Destroy,
        handler: mail_inbox,
    },
    UserTool {
        name: "stack",
        description: "Control rexenv's stack. Takes `action` and, for some, `service`. `start` and \
                      `stop` bring the WHOLE stack up or down (edge, web server, PHP pools, \
                      databases, mail) and need the user's `system` permission on rexenv itself — \
                      AND, because the edge is a root daemon, macOS asks the user for their \
                      password in a dialog the agent cannot answer; that dialog is a second consent, \
                      and if they cancel it the call fails. `restart` {service: nginx / edge / \
                      php-8.3 …} rebuilds that service's config and restarts it (the edge is \
                      reloaded, never stopped); `start_database` / `stop_database` {service: mysql \
                      / mariadb / postgres} and `start_mail` / `stop_mail` need `manage` on rexenv \
                      itself and never prompt. `start_sites` / `stop_sites` serve — or stop \
                      serving — EVERY one of the user's sites at once, leaving rexenv's services \
                      running (a stopped site answers a `site stopped` page; the stack is \
                      untouched, so this is `manage`, not `system`). A stopped web tier is every site down: prefer \
                      `restart` to stopping.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["start", "stop", "restart", "start_database", "stop_database", "start_mail", "stop_mail", "start_sites", "stop_sites"] },
                "service": { "type": "string", "description": "restart: nginx / edge / php-<minor>; start_database / stop_database: mysql / mariadb / postgres." }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "action": "restart", "service": "nginx" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(str::to_string),
        scope: Scope::System,
        handler: stack,
    },
    UserTool {
        name: "php",
        description: "PHP versions on this machine. Takes `action` and `minor` (e.g. `8.3`): \
                      `install` (download and enable a pool for it) / `uninstall`, `settings_set` \
                      {key, value — a php.ini override listed by php_settings; empty value = back to the \
                      default} and `update_check` need `manage` on rexenv itself; `default` (the \
                      version new sites get) and `update_apply` {patch — from update_check, \
                      swapping the running pool to a newer signed build} need `system`. Which \
                      versions exist: stack_status. Per-site PHP: site_configure `php`.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["install", "uninstall", "settings_set", "update_check", "default", "update_apply"] },
                "minor": { "type": "string" }, "key": { "type": "string" }, "value": { "type": "string" }, "patch": { "type": "string" }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "action": "update_check" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(str::to_string),
        scope: Scope::System,
        handler: php,
    },
    UserTool {
        name: "settings",
        description: "Write one rexenv setting — the same allow-list `rex config set` uses, so a key \
                      that is read-only or refused there is refused here with the reason (the sites \
                      folder, the signed update chain, the MCP switches). Takes `key` and `value`. \
                      Needs `system` on rexenv itself. Reading is settings_get, which needs nothing.",
        input_schema: || json!({
            "type": "object",
            "properties": { "key": { "type": "string" }, "value": { "type": "string" } },
            "required": ["key", "value"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "key": "mcp_enabled", "value": "false" }),
        summarise: |args| args.get("key").and_then(Value::as_str).map(str::to_string),
        scope: Scope::System,
        handler: settings,
    },
    UserTool {
        name: "tld",
        description: "The top-level domains rexenv resolves. Takes `action` and `tld`: `set` makes it \
                      the default for new sites and installs its resolver file; `repair` puts back \
                      the resolver file for a TLD your sites already answer on (what stack_status \
                      or rex doctor would tell you is missing); `remove` takes rexenv's own file for \
                      a TLD no site uses back out. All three write under /etc/resolver, so they \
                      need `system` on rexenv itself AND macOS asks the user for their password — a \
                      dialog the agent cannot answer. The current default is in stack_status.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["set", "repair", "remove"] },
                "tld": { "type": "string" }
            },
            "required": ["action", "tld"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "action": "repair", "tld": "rex" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(str::to_string),
        scope: Scope::System,
        handler: tld,
    },
    UserTool {
        name: "open",
        description: "Open one of the user's own sites on THEIR screen: `target` `browser` (the \
                      site's URL in their preferred browser, or `app` = a browser id; `private` for \
                      a private window), `editor` (the site's folder in their preferred editor, or \
                      `app` = an editor id) or `finder` (reveal the folder). Takes `site_id`. Needs \
                      `manage` on the site. Never an arbitrary URL or path — only this site's.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "target": { "type": "string", "enum": ["browser", "editor", "finder"] },
                "app": { "type": "string", "description": "A browser or editor id from the user's installed apps; omit for their preference." },
                "private": { "type": "boolean" }
            },
            "required": ["site_id", "target"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "target": "finder" }),
        summarise: |args| args.get("target").and_then(Value::as_str).map(str::to_string),
        scope: Scope::Manage,
        handler: open,
    },
    UserTool {
        name: "share",
        description: "Publish one of the user's own sites to the internet through a Cloudflare quick \
                      tunnel, or stop and inspect one. Takes `action`: `start` {site_id, minutes?} \
                      needs `run` — the Agent access dial at Full — and rexenv STOPS the share on \
                      its own after the minutes asked for (30 by default, 60 at most) and when \
                      rexenv quits; `stop` {site_id} needs `manage`; `status` {site_id?} needs \
                      `read`. While it runs, anyone with the URL reaches that site as it is on \
                      this machine — say so before you start one.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["start", "stop", "status"] },
                "minutes": { "type": "integer", "description": "start: how long, 1–60. Default 30." }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "stop" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("share {a}")),
        scope: Scope::Run,
        handler: share,
    },
    UserTool {
        name: "blueprints",
        description: "Save or delete a blueprint — a preset site_create's `blueprint` names. `save` \
                      {name, spec: {siteType, phpVersion, webServer, multisite?, plugins?: [{slug, \
                      activate}], themes?, wpDebug?, language?}} needs `manage` on rexenv itself \
                      (saving over an existing name replaces it); `delete` {name} needs `destroy`. \
                      Listing is blueprints_list.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["save", "delete"] },
                "name": { "type": "string" },
                "spec": { "type": "object" }
            },
            "required": ["action", "name"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "action": "delete", "name": "sweep-probe" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("blueprint {a}")),
        scope: Scope::Destroy,
        handler: blueprints,
    },
    UserTool {
        name: "repo",
        description: "Git-backed plugins and themes inside one of the user's own sites — the site's \
                      Repo tab. Takes `site_id`, `action`, and for most `dir` (the plugin/theme \
                      folder name) and `kind` (plugin, the default, or theme). Reads under `read` on \
                      the site: `assets`, `status` {dir}, `branches`, `prs`, `stashes`, `scripts`, \
                      `info`, `jobs`, `job` {job_id}, `watches`, `unmanaged`, `check` {dir} \
                      (dependency check, runs nothing). Under `read` on rexenv itself: `tools` \
                      {refresh}, `probe` {url}. Everything that runs code or writes into the site \
                      needs `run` on the site: `add` {url, ref?, dir?, install?} (clone a \
                      repository in), `adopt` {dir}, `link` {path, dir?} (symlink a checkout in), \
                      `git` {dir, op: fetch/pull/checkout/push/stash/stash-pop/reset/status, ref?, \
                      install?}, `run_step` {job_id, step}, `run_offered` {job_id} (composer / \
                      npm install / build, in order), `script` {dir, script}, `dist_archive` \
                      {dir}, `watch_start` {dir, script}, `watch_stop` {watch_id}, `cancel` \
                      {job_id}. Job-shaped actions block until the job settles and return its \
                      steps and log.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["assets", "status", "branches", "prs", "stashes", "scripts", "info", "jobs", "job", "watches", "unmanaged", "check", "tools", "probe", "add", "adopt", "link", "git", "run_step", "run_offered", "script", "dist_archive", "watch_start", "watch_stop", "cancel"] },
                "kind": { "type": "string", "enum": ["plugin", "theme"] },
                "dir": { "type": "string" }, "url": { "type": "string" }, "ref": { "type": "string" },
                "path": { "type": "string" }, "op": { "type": "string" }, "install": { "type": "boolean" },
                "job_id": { "type": "string" }, "step": { "type": "string" }, "script": { "type": "string" },
                "watch_id": { "type": "string" }, "refresh": { "type": "boolean" }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "assets" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("repo {a}")),
        scope: Scope::Run,
        handler: repo,
    },
    UserTool {
        name: "valet_import",
        description: "Bring sites over from Laravel Valet or Herd. Takes `action`: `scan` (what \
                      Valet/Herd serve on this machine and which TLDs they own — reads their config, \
                      runs nothing) and `drift` (TLDs another tool has taken back) need `read` on \
                      rexenv itself; `run` {domains: [...], php?: {domain: minor}, \
                      import_databases?} imports the named sites — their folders are LINKED, never \
                      moved, and a database import is a COPY of theirs — and needs `run` on rexenv \
                      itself; `cancel` needs `manage`; `take_over` {tld} / `hand_back` {tld} rewrite \
                      the OS resolver for that TLD (root — macOS asks the user for their password) \
                      and need `system`.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["scan", "drift", "run", "cancel", "take_over", "hand_back"] },
                "domains": { "type": "array", "items": { "type": "string" } },
                "php": { "type": "object", "additionalProperties": { "type": "string" } },
                "import_databases": { "type": "boolean" },
                "tld": { "type": "string" }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "action": "drift" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("valet {a}")),
        scope: Scope::System,
        handler: valet_import,
    },
    UserTool {
        name: "connection_rewrite",
        description: "Point an imported site's own config (wp-config.php or .env) at the database \
                      rexenv imported for it. Takes `site_id` and `action`: `preview` (the exact \
                      diff, and a `fingerprint` of the file as it is now) needs `read`; `apply` \
                      {fingerprint — from the preview; refused if the file changed since} writes \
                      the file after backing it up, and `revert` {force?} puts the original back \
                      (refused without force if the file was edited since) — both need `destroy`. \
                      The diff may show the site's database credentials.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "site_id": { "type": "string" },
                "action": { "type": "string", "enum": ["preview", "apply", "revert"] },
                "fingerprint": { "type": "string" }, "force": { "type": "boolean" }
            },
            "required": ["site_id", "action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "site_id": id, "action": "preview" }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("rewrite {a}")),
        scope: Scope::Destroy,
        handler: connection_rewrite,
    },
    UserTool {
        name: "db_import",
        description: "The per-site database import from Valet/Herd. Takes `action`: `status` \
                      {site_id} (the running or last job, and what was imported) needs `read` on the \
                      site; `records` and `leftovers` (dumps kept after a failed import) need `read` \
                      on rexenv itself; `start` {site_id, confirm_overwrite?: the site's domain, \
                      when a database already exists — it is DROPPED and rebuilt} needs `destroy` on \
                      the site and blocks until it settles; `cancel` {site_id} needs `manage`; \
                      `delete_leftover` {file} needs `destroy` on rexenv itself.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["status", "records", "leftovers", "start", "cancel", "delete_leftover"] },
                "site_id": { "type": "string" }, "confirm_overwrite": { "type": "string" }, "file": { "type": "string" }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        sweep_args: |id| json!({ "action": "status", "site_id": id }),
        summarise: |args| args.get("action").and_then(Value::as_str).map(|a| format!("dbimport {a}")),
        scope: Scope::Destroy,
        handler: db_import,
    },
    UserTool {
        name: "site_configure",
        description: "Change how one of the user's own sites is set up — the things the site's \
                      Settings tab does. Takes `site_id` and `action`, plus the action's field: \
                      `rename` {name}; `php` {version, a minor like `8.3`}; `server` {server: \
                      nginx / apache / frankenphp}; `xdebug` {enabled}; `enabled` {enabled: false stops serving \
                      THIS site — no server block and a 503 at its address, while the shared web \
                      server and PHP pools keep running for every other site; true serves it \
                      again, and the reply says whether anything is actually answering}; \
                      `env_set` {key, value} and \
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
                "enum": ["rename", "php", "server", "xdebug", "env_set", "env_unset", "domain", "add_domain", "remove_domain", "move", "relink", "regenerate_cert", "enabled"]
            },
            "name": { "type": "string", "description": "rename: the new display name." },
            "version": { "type": "string", "description": "php: a minor like `8.3`." },
            "server": { "type": "string", "enum": ["nginx", "apache", "frankenphp"], "description": "server: the web server to switch to." },
            "enabled": { "type": "boolean", "description": "xdebug: on or off. enabled: true serves the site, false stops serving it." },
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
            "db_engine": { "type": "string", "enum": ["mysql", "mariadb", "postgres"], "description": "Defaults to mysql. `postgres` is for `php` and `laravel` sites on a PHP build with pdo_pgsql — WordPress cannot use it, and the refusal says why." },
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
                    // Annotations are added in ONE place, `mcp_server::Tool::descriptor`,
                    // from the registry a tool came from — never here.
                })
            })
            .collect(),
    )
}

/// A parity tool's async result over the app's own site operations.
pub type OpFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a Composer run left behind: whether it succeeded, and every line it
/// printed (unscrubbed — the handler owns the one scrubber).
#[derive(Debug, Clone, Default)]
pub struct ComposerRun {
    pub ok: bool,
    pub log: Vec<String>,
}

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
    /// Start a public tunnel through the app's command, and arrange its stop
    /// after `minutes` — the auto-stop D11 made the condition of this tool.
    fn share_start<'a>(&'a self, id: String, minutes: u64) -> OpFuture<'a, Result<crate::commands::tunnels::TunnelInfo>>;
    fn share_stop<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>>;
    /// Every live tunnel, from the app's registry (`tunnels_status`).
    fn shares<'a>(&'a self) -> OpFuture<'a, Result<Vec<crate::commands::tunnels::TunnelInfo>>>;
    fn save_blueprint<'a>(&'a self, bp: crate::state::models::Blueprint) -> OpFuture<'a, Result<()>>;
    fn delete_blueprint<'a>(&'a self, id: String) -> OpFuture<'a, Result<bool>>;
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
    /// Serve this site, or stop serving it (v44). The MECHANISM, never the
    /// Tauri command: that one promotes a scratch site, and an agent that could
    /// stop-and-start its way to adoption would be free of the disposable cap
    /// (#214's trap, in its cheapest form yet).
    fn set_enabled<'a>(
        &'a self,
        id: String,
        enabled: bool,
    ) -> OpFuture<'a, Result<Option<crate::commands::sites::SiteEnabledReport>>>;
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
    // ── wp_data ──
    fn db_export<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn content_export<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<String>>>;
    fn search_replace<'a>(&'a self, id: String, from: String, to: String, dry_run: bool) -> OpFuture<'a, Result<u64>>;
    fn db_import<'a>(&'a self, id: String, path: String) -> OpFuture<'a, Result<()>>;
    fn site_reset<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>>;
    // ── wp_network ──
    fn network_sites<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpNetworkSite>>>;
    fn network_site_create<'a>(&'a self, id: String, slug: String) -> OpFuture<'a, Result<()>>;
    fn network_site_delete<'a>(&'a self, id: String, blog_id: String) -> OpFuture<'a, Result<()>>;
}

/// The app's own stack controls (`commands::services`, `database`, `mail`),
/// runtime-erased. `start_all`/`stop_all` reach `run_privileged` (the edge is a
/// root daemon) — the macOS dialog they raise is a SECOND consent on top of the
/// `system` grant, and the only two methods here that can raise one.
pub trait StackOps: Send + Sync {
    fn start_all<'a>(&'a self) -> OpFuture<'a, Result<()>>;
    fn stop_all<'a>(&'a self) -> OpFuture<'a, Result<()>>;
    fn restart_web<'a>(&'a self, target: String) -> OpFuture<'a, Result<crate::commands::services::WebRestartReport>>;
    fn start_database<'a>(&'a self, key: String) -> OpFuture<'a, Result<()>>;
    fn stop_database<'a>(&'a self, key: String) -> OpFuture<'a, Result<()>>;
    fn start_mail<'a>(&'a self) -> OpFuture<'a, Result<()>>;
    fn stop_mail<'a>(&'a self) -> OpFuture<'a, Result<()>>;
    /// Serve every site, or stop serving every site (v44). NOT `stop_all`: the
    /// services stay up and only the sites' serving surface changes, which is
    /// why it is `manage` and never raises the password dialog.
    fn set_all_sites_enabled<'a>(
        &'a self,
        enabled: bool,
    ) -> OpFuture<'a, Result<crate::commands::sites::BulkEnabledReport>>;
}

/// The app's own machine-wide settings and the open-in-app verbs
/// (`commands::php`, `settings`, `system`), runtime-erased. `set_default_tld`,
/// `repair_resolver` and `remove_resolver` write under `/etc/resolver` and
/// raise the macOS dialog — the `system` scope's second consent, like the stack.
pub trait SystemOps: Send + Sync {
    fn set_setting<'a>(&'a self, key: String, value: String) -> OpFuture<'a, Result<()>>;
    fn set_default_tld<'a>(&'a self, tld: String) -> OpFuture<'a, Result<String>>;
    fn repair_resolver<'a>(&'a self, tld: String) -> OpFuture<'a, Result<String>>;
    fn remove_resolver<'a>(&'a self, tld: String) -> OpFuture<'a, Result<bool>>;
    fn set_php_installed<'a>(&'a self, minor: String, installed: bool) -> OpFuture<'a, Result<()>>;
    fn set_default_php<'a>(&'a self, minor: String) -> OpFuture<'a, Result<()>>;
    fn apply_php_settings<'a>(&'a self, minor: String, settings: Vec<crate::commands::php::PhpSettingInput>) -> OpFuture<'a, Result<()>>;
    fn php_update_check<'a>(&'a self) -> OpFuture<'a, Result<Vec<crate::state::models::PhpVersionView>>>;
    fn php_update_apply<'a>(&'a self, minor: String, patch: String) -> OpFuture<'a, Result<crate::commands::php::PhpUpdateOutcome>>;
    fn browsers<'a>(&'a self) -> OpFuture<'a, Vec<crate::platform::traits::BrowserApp>>;
    fn open_in_browser<'a>(&'a self, browser_id: String, url: String, private: bool) -> OpFuture<'a, Result<()>>;
    fn editors<'a>(&'a self) -> OpFuture<'a, Vec<crate::platform::traits::EditorApp>>;
    fn open_in_editor<'a>(&'a self, editor_id: String, path: String) -> OpFuture<'a, Result<()>>;
    fn reveal_path<'a>(&'a self, path: String) -> OpFuture<'a, Result<()>>;
}

/// The app's own git/asset operations (`commands::repo`), runtime-erased.
/// Job-shaped ops (`add`, `git_op`, `script`, `dist_archive`, the offered
/// steps) return only once the job has SETTLED — the CLI's shape (`repo_wait_settled`),
/// because a tool reply is one message and a job id an agent must poll is a
/// worse one; progress is P6's business.
pub trait RepoOps: Send + Sync {
    /// Link a package checkout into a site's project as a Composer `path`
    /// repository (`core::laravel::composer_link`). Lives with the app rather
    /// than in the handler because the run wants the user's login-shell env
    /// (`RepoJobs`, for `COMPOSER_HOME`/auth.json) and a blocking thread the
    /// handler's borrowed state cannot lend. Returns Composer's lines, ok or
    /// not — a failed require's reason is in them.
    fn composer_link<'a>(&'a self, site_id: String, link: crate::core::laravel::ComposerLink) -> OpFuture<'a, Result<ComposerRun>>;
    fn assets<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<Vec<crate::state::models::GitAsset>>>;
    fn asset_status<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<crate::commands::repo::AssetStatusResult>>;
    fn branches<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<crate::commands::repo::RepoBranches>>;
    fn pull_refs<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<Vec<crate::core::repo::PullRef>>>;
    fn stashes<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<Vec<crate::core::repo::StashEntry>>>;
    fn scripts<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<crate::commands::repo::RepoScriptsInfo>>;
    fn site_info<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<crate::commands::repo::SiteRepoInfo>>;
    fn site_jobs<'a>(&'a self, site_id: String, kind: String) -> OpFuture<'a, Result<Vec<crate::commands::repo::RepoJobState>>>;
    fn job_state<'a>(&'a self, job_id: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>>;
    fn watches<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<Vec<crate::commands::repo::WatchState>>>;
    fn unmanaged<'a>(&'a self, site_id: String, kind: String) -> OpFuture<'a, Result<Vec<crate::core::repo::UnmanagedRepo>>>;
    fn check<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>>;
    fn tools<'a>(&'a self, refresh: bool) -> OpFuture<'a, Result<Vec<crate::commands::repo::ToolStatus>>>;
    fn probe<'a>(&'a self, url: String) -> OpFuture<'a, Result<crate::commands::repo::RepoProbeResult>>;
    fn add<'a>(&'a self, site_id: String, kind: String, url: String, git_ref: Option<String>, dir: Option<String>, install: bool) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>>;
    fn adopt<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<()>>;
    fn link<'a>(&'a self, site_id: String, kind: String, dir: Option<String>, target: String) -> OpFuture<'a, Result<crate::commands::repo::RepoLinkResult>>;
    fn git_op<'a>(&'a self, site_id: String, kind: String, dir: String, op: String, target_ref: Option<String>, install: bool) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>>;
    fn run_step<'a>(&'a self, job_id: String, step: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>>;
    fn run_offered<'a>(&'a self, job_id: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>>;
    fn script<'a>(&'a self, site_id: String, kind: String, dir: String, script: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>>;
    fn dist_archive<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>>;
    fn watch_start<'a>(&'a self, site_id: String, kind: String, dir: String, script: String) -> OpFuture<'a, Result<crate::commands::repo::WatchState>>;
    fn watch_stop<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>>;
    fn cancel<'a>(&'a self, job_id: String) -> OpFuture<'a, Result<()>>;
    /// The job's flat log (through `logs.tail`), for the settled reply.
    fn job_log<'a>(&'a self, log_key: String) -> OpFuture<'a, Vec<String>>;
}

/// The Valet/Herd migration, the connection rewrite and the per-site database
/// import (`commands::{valet_import, rewrite, db_import}`), runtime-erased.
pub trait ImportOps: Send + Sync {
    fn valet_scan<'a>(&'a self) -> OpFuture<'a, Result<crate::commands::valet_import::ImportScan>>;
    fn valet_drift<'a>(&'a self) -> OpFuture<'a, Result<Vec<String>>>;
    fn valet_run<'a>(&'a self, request: crate::commands::valet_import::ImportRequest) -> OpFuture<'a, Result<crate::commands::valet_import::ImportResult>>;
    fn valet_cancel<'a>(&'a self) -> OpFuture<'a, Result<()>>;
    fn resolver_take_over<'a>(&'a self, tld: String) -> OpFuture<'a, Result<()>>;
    fn resolver_hand_back<'a>(&'a self, tld: String) -> OpFuture<'a, Result<crate::core::dns::ResolverPlan>>;
    fn rewrite_preview<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<crate::commands::rewrite::RewritePreview>>;
    fn rewrite_apply<'a>(&'a self, site_id: String, fingerprint: String) -> OpFuture<'a, Result<crate::commands::rewrite::RewriteApplied>>;
    fn rewrite_revert<'a>(&'a self, site_id: String, force: bool) -> OpFuture<'a, Result<crate::commands::rewrite::RevertOutcome>>;
    /// Start the import and block until it settles (the app's job is polled by
    /// `db_import_state`; the settled snapshot comes back).
    fn db_import_start<'a>(&'a self, site_id: String, confirm_overwrite: Option<String>) -> OpFuture<'a, Result<crate::commands::db_import::DbImportJobState>>;
    fn db_import_state<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<Option<crate::commands::db_import::DbImportJobState>>>;
    fn db_import_cancel<'a>(&'a self, job_id: String) -> OpFuture<'a, Result<()>>;
    fn db_import_record<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<Option<crate::state::store::DbImportRecord>>>;
    fn db_import_records<'a>(&'a self) -> OpFuture<'a, Result<Vec<crate::state::store::DbImportRecord>>>;
    fn db_import_leftovers<'a>(&'a self) -> OpFuture<'a, Result<Vec<crate::commands::db_import::LeftoverDump>>>;
    fn db_import_delete_leftover<'a>(&'a self, file: String) -> OpFuture<'a, Result<()>>;
}

/// The app's own Mailpit reads and writes (`commands::mail`), runtime-erased.
pub trait MailOps: Send + Sync {
    fn list<'a>(&'a self, query: Option<String>, unread_only: bool) -> OpFuture<'a, Result<crate::core::mail::MailList>>;
    fn detail<'a>(&'a self, id: String) -> OpFuture<'a, Result<crate::core::mail::MailDetail>>;
    fn raw<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>>;
    fn mark_all_read<'a>(&'a self) -> OpFuture<'a, Result<()>>;
    fn clear<'a>(&'a self) -> OpFuture<'a, Result<()>>;
    fn delete<'a>(&'a self, ids: Vec<String>) -> OpFuture<'a, Result<()>>;
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
    mail: &'a dyn MailOps,
    stack: &'a dyn StackOps,
    sys: &'a dyn SystemOps,
    repo: &'a dyn RepoOps,
    import: &'a dyn ImportOps,
}

impl<'a> UserCtx<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(state: &'a AppState, ops: &'a dyn SiteOps, wp: &'a dyn WpOps, mail: &'a dyn MailOps, stack: &'a dyn StackOps, sys: &'a dyn SystemOps, repo: &'a dyn RepoOps, import: &'a dyn ImportOps, client: &'a str) -> Self {
        UserCtx { state, client, ops, wp, mail, stack, sys, repo, import }
    }

    pub(crate) fn db(&self) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
        self.state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))
    }

    /// The ONLY door to a site or to the stack: `agent_grants::claim_by_level`,
    /// which refuses a scratch site (the agent's own — the scratch tools apply)
    /// and then asks the Agent access dial for `S`. No client, no row, no
    /// prompt: after D15–D17 the dial answers every scope, publishing included.
    ///
    /// `wanted` is this tool's one-line description of what the agent is trying
    /// to do. It is kept in the signature — and deliberately unused — because
    /// it is what a future "why was this refused" surface would show, and
    /// threading it back through fifty call sites is the cost of removing it.
    pub fn claim<S: scope::Marker>(&self, site_id: Option<&str>, wanted: &str) -> Result<Claimed<S>> {
        let _ = wanted;
        let conn = self.db()?;
        let granted = agent_grants::claim_by_level::<S>(&conn, site_id)?;
        Ok(Claimed { granted })
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
            .map_err(|_| Error::Other(format!("`{e}` is not a database engine here — use `mysql`, `mariadb` or `postgres`.")))?,
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
///
/// **The job's own reason is IN the reply, through the path scrubber.** It used
/// to be dropped whole (the text can name a local log file) and the agent was
/// sent to `tail_log` — which reads only a WordPress debug log, so no tool
/// anywhere held the reason. Found 11 Sep 2026 driving the site matrix through
/// this tool: a Laravel create on PHP 8.1 failed with a sentence naming the
/// advisory-blocked package and the fix, and the agent was told only "setup did
/// not finish". `site_retry` already returned its error through the same
/// scrubber; this is that, for the first attempt.
fn translate_create_failure(
    domain: &str,
    failure: crate::commands::sites::CreateFailure,
    conn: Option<&rusqlite::Connection>,
    paths: &dyn crate::platform::traits::Paths,
    acted: &super::feed::ActedTarget,
) -> Error {
    let Some(id) = failure.site_id else {
        return failure.error;
    };
    let mut docroot = String::new();
    if let Some(conn) = conn {
        if let Ok(Some(site)) = crate::state::store::get_site(conn, &id) {
            docroot = site.path.clone();
            acted.set(&site);
        }
    }
    let known = super::view::KnownPaths::for_site(paths, &docroot);
    let reason = super::view::create_failure_reason(&failure.error.to_string(), &known);
    Error::Other(format!(
        "`{domain}` was created but its setup did not finish, so it is not usable yet. What \
         failed:\n{reason}\n\nIt exists (id `{id}`) and the person you're working with can see \
         it in rexenv listed as \"setup incomplete\", where they can retry or remove it. You can \
         retry it with site_retry (after changing what the reason names, e.g. site_configure \
         `php`), or remove it with site_delete (which needs their `destroy` permission)."
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
                return Err(translate_create_failure(
                    &domain,
                    failure,
                    conn.as_deref(),
                    ctx.state.platform.paths(),
                    acted,
                ));
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
        serde_json::to_value(view).map_err(|e| Error::Other(format!("serialising the site: {e}")))
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
            Enabled(bool),
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
            "enabled" => Change::Enabled(
                args.get("enabled").and_then(Value::as_bool).ok_or_else(|| {
                    Error::Other(
                        "site_configure `enabled` needs `enabled` (true to serve the site, \
                         false to stop serving it)."
                            .into(),
                    )
                })?,
            ),
            other => {
                return Err(Error::Other(format!(
                    "`{other}` is not a site_configure action. Use one of: rename, php, server, xdebug, \
                     env_set, env_unset, domain, add_domain, remove_domain, move, relink, regenerate_cert, enabled."
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
            Change::Enabled(on) => {
                if *on { "start serving it".to_string() } else { "stop serving it".to_string() }
            }
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
            Change::Enabled(on) => {
                let report = ops.set_enabled(id.clone(), on).await?;
                // The honest pair, both from the app: what was recorded, and
                // whether anything is actually answering. An agent that reported
                // "started" off the switch alone would tell the user their site
                // is up while the browser gets a 503 — the same failure the UI
                // is built to avoid, arriving through a different door.
                if let Some(r) = report {
                    extra.insert("serving".into(), json!(r.serving));
                    if let Some(note) = r.note {
                        extra.insert("note".into(), json!(note));
                    }
                }
            }
        }
        let mut value = site_after(&ctx, &id)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("action".into(), json!(action));
            obj.extend(extra);
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
fn claim_scope(ctx: &UserCtx<'_>, site_id: &str, scope: Scope, wanted: &str) -> Result<Site> {
    let take = |site: Option<&Site>| site.cloned().ok_or_else(|| Error::Other("this tool needs a site, not the stack.".into()));
    Ok(match scope {
        Scope::Read => {
            let c = ctx.claim::<scope::Read>(Some(site_id), wanted)?;
            take(c.granted.site())?
        }
        Scope::Manage => {
            let c = ctx.claim::<scope::Manage>(Some(site_id), wanted)?;
            take(c.granted.site())?
        }
        Scope::Destroy => {
            let c = ctx.claim::<scope::Destroy>(Some(site_id), wanted)?;
            take(c.granted.site())?
        }
        Scope::Run => {
            let c = ctx.claim::<scope::Run>(Some(site_id), wanted)?;
            take(c.granted.site())?
        }
        Scope::System => {
            let c = ctx.claim::<scope::System>(Some(site_id), wanted)?;
            take(c.granted.site())?
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
        let site = claim_scope(&ctx, id, Scope::Read, &format!("read its WordPress {what}"))?;
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
        Ok(json!({ "domain": site.domain, "what": what, "result": value }))
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
        let site = claim_scope(&ctx, id, scope, &wanted)?;
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
        Ok(json!({ "domain": site.domain, "action": action, "result": result }))
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
        let site = claim_scope(&ctx, id, scope, &wanted)?;
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
        Ok(json!({ "domain": site.domain, "action": action, "result": result }))
    })
}


pub(crate) fn wp_user_scope(action: &str) -> Option<Scope> {
    Some(match action {
        // `login_url` is READ by the owner's ruling (5 Sep 2026, D2 widened):
        // the point of the MCP server is that an agent gets into any site
        // without the person logging in for it or a password changing hands,
        // and Read is the level a fresh install sits at. The link is the app's
        // own single-use, 120 s, loopback-only token; what the signed-in session
        // can then do is WordPress's own capability model, not rexenv's dial —
        // stated in the description so the choice is visible where it is made.
        "list" | "super_admins" | "login_url" => Scope::Read,
        "create" | "set_role" | "super_admin_add" => Scope::Manage,
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
        let site = claim_scope(&ctx, id, scope, &wanted)?;
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
        Ok(json!({ "domain": site.domain, "action": action, "result": result }))
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
        let site = claim_scope(&ctx, id, Scope::Manage, &wanted)?;
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
        Ok(json!({ "domain": site.domain, "action": action, "done": true, "detail": wanted }))
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
        let site = claim_scope(&ctx, id, scope, &wanted)?;
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
        Ok(json!({ "domain": site.domain, "action": action, "result": result }))
    })
}


pub(crate) fn wp_data_scope(action: &str, dry_run: bool) -> Option<Scope> {
    Some(match action {
        "db_export" | "content_export" => Scope::Manage,
        "search_replace" => if dry_run { Scope::Manage } else { Scope::Destroy },
        "db_import" | "reset" => Scope::Destroy,
        _ => return None,
    })
}

pub(crate) fn wp_network_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "sites" => Scope::Read,
        "convert" | "site_create" => Scope::Manage,
        "site_delete" => Scope::Destroy,
        _ => return None,
    })
}

/// A file the app wrote for the user, as the agent may name it: the file name
/// alone. The directory is the user's Downloads folder, which the reply states
/// in words — the absolute path is a location on disk and the view rule keeps
/// it out (the same rule as the domain change's backup).
fn basename(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
}

fn wp_data<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_data needs a `site_id`.".into()))?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_data needs an `action`.".into()))?;
        let dry_run = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
        let scope = wp_data_scope(action, dry_run).ok_or_else(|| {
            Error::Other(format!("`{action}` is not a wp_data action. Use db_export, content_export, search_replace, db_import or reset."))
        })?;
        let wanted = match action {
            "db_export" => "export its database to Downloads".to_string(),
            "content_export" => "export its content to Downloads".to_string(),
            "search_replace" => format!("{} `{}` with `{}` across the database", if dry_run { "count what replacing" } else { "REPLACE" }, str_field(args, "from", action)?, str_field(args, "to", action)?),
            "db_import" => format!("REPLACE its database with `{}`", basename(str_field(args, "path", action)?)),
            _ => "RESET it to a fresh WordPress — everything in it is lost".to_string(),
        };
        wp_precheck(&ctx, id, "wp_data")?;
        let site = claim_scope(&ctx, id, scope, &wanted)?;
        acted.set(&site);
        let sid = site.id.clone();
        let wp = ctx.wp;
        let result = match action {
            "db_export" => json!({ "file": basename(&wp.db_export(sid).await?), "location": "the user's Downloads folder" }),
            "content_export" => json!({ "files": wp.content_export(sid).await?.iter().map(|p| basename(p)).collect::<Vec<_>>(), "location": "the user's Downloads folder" }),
            "search_replace" => {
                let n = wp.search_replace(sid, str_field(args, "from", action)?.to_string(), str_field(args, "to", action)?.to_string(), dry_run).await?;
                json!({ "replacements": n, "dryRun": dry_run })
            }
            "db_import" => { wp.db_import(sid, str_field(args, "path", action)?.to_string()).await?; json!({ "imported": true }) }
            _ => { wp.site_reset(sid).await?; json!({ "reset": true }) }
        };
        Ok(json!({ "domain": site.domain, "action": action, "result": result }))
    })
}

fn wp_network<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_network needs a `site_id`.".into()))?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("wp_network needs an `action`.".into()))?;
        let scope = wp_network_scope(action).ok_or_else(|| Error::Other(format!("`{action}` is not a wp_network action. Use sites, convert, site_create or site_delete.")))?;
        let wanted = match action {
            "sites" => "list the network's sites".to_string(),
            "convert" => {
                let m = str_field(args, "mode", action)?;
                if !matches!(m, "subdomain" | "subdirectory") {
                    return Err(Error::Other(format!("`{m}` is not a multisite mode — use `subdomain` or `subdirectory`.")));
                }
                format!("convert it to a {m} network")
            }
            "site_create" => format!("add the network site `{}`", str_field(args, "slug", action)?),
            _ => format!("delete network site {}", str_field(args, "blog_id", action)?),
        };
        wp_precheck(&ctx, id, "wp_network")?;
        let site = claim_scope(&ctx, id, scope, &wanted)?;
        acted.set(&site);
        let sid = site.id.clone();
        let result = match action {
            "sites" => to_json(ctx.wp.network_sites(sid).await?)?,
            "convert" => { let m = str_field(args, "mode", action)?.to_string(); ctx.ops.multisite_convert(sid, m.clone()).await?; json!({ "converted": m }) }
            "site_create" => { let slug = str_field(args, "slug", action)?.to_string(); ctx.wp.network_site_create(sid, slug.clone()).await?; json!({ "created": slug }) }
            _ => { let b = str_field(args, "blog_id", action)?.to_string(); ctx.wp.network_site_delete(sid, b.clone()).await?; json!({ "deleted": b }) }
        };
        Ok(json!({ "domain": site.domain, "action": action, "result": result }))
    })
}

/// The raw runner on the USER's site — `scratch::wp_run`'s mechanism (the same
/// resolver, the same target screen, the same runner, the same scrubber; none
/// copied) behind the `run` witness instead of the scratch one.
///
/// Order matters and is the same as the scratch tool's: gate, then the target
/// screen BEFORE anything is resolved or spawned, then the resolver (which can
/// DOWNLOAD on first use — minutes), then the re-assert, then the run.
fn site_wp_run<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("site_wp_run needs a `site_id`.".into()))?;
        let argv = super::scratch::wp_argv(args)?;
        wp_precheck(&ctx, id, "site_wp_run")?;
        // The target screen is a SHAPE refusal and sits before the gate: a
        // `--path` argv is refused whether or not a grant exists, and never
        // records an ask for a call that could not have run. (Found 3 Sep 2026
        // in the live run: with the switch off the gate's message hid it.)
        crate::core::scratch::refuse_wp_target_override(&argv)?;
        let wanted = format!("run `wp {}` in it", argv.iter().take(2).cloned().collect::<Vec<_>>().join(" "));
        let claimed = ctx.claim::<scope::Run>(Some(id), &wanted)?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("site_wp_run needs a site.".into()))?;
        acted.set(&site);
        let docroot = std::path::PathBuf::from(&site.path);
        let (php_bin, wp_phar) = super::scratch::resolve_wp_tools(ctx.state, &site.php_version).await?;
        if !ctx.still_granted(&claimed.granted)? {
            return Err(Error::Other(format!("the `run` permission on `{}` was revoked before the command ran — nothing was run.", site.domain)));
        }
        let printed = argv.join(" ");
        let timeout = std::time::Duration::from_secs(crate::core::scratch::WP_RUN_TIMEOUT_SECS);
        let out = crate::commands::wordpress::wp_blocking(move || crate::core::wordpress::wp_run_raw(&php_bin, &wp_phar, &docroot, &argv, timeout)).await?;
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), &site.path);
        let (stdout, cut_out) = super::scratch::agent_stream(&out.stdout, &known);
        let (stderr, cut_err) = super::scratch::agent_stream(&out.stderr, &known);
        let succeeded = out.status.success();
        let exit_code = out.status.code();
        let mut detail = if succeeded {
            format!("`wp {printed}` ran in `{}` and succeeded.", site.domain)
        } else {
            format!("`wp {printed}` FAILED in `{}` (exit {}). What WP-CLI said is in `stderr`.", site.domain, exit_code.map_or_else(|| "killed by a signal".to_string(), |c| c.to_string()))
        };
        if cut_out || cut_err {
            detail.push_str(&format!(" The output was longer than {} KB and has been cut — run a narrower command if you need the rest.", super::scratch::WP_OUTPUT_CAP / 1024));
        }
        if let Some(tell) = crate::core::wp_packages::explain_missing_command_here(&stderr) {
            detail.push(' ');
            detail.push_str(&super::view::scrub_log_line(&tell, &known));
        }
        let view = super::scratch::AgentWpRun { succeeded, exit_code, stdout, stderr, truncated: cut_out || cut_err, detail, note: super::scratch::WP_RUN_NOTE };
        to_json(view)
    })
}

/// The scrub's scope for an artisan reply — `WP_RUN_NOTE`'s sentence with the
/// right program named.
const ARTISAN_NOTE: &str = "Absolute paths rexenv knows — the site's project folder, rexenv's own \
    directories, the home directory — are shown as labels like <docroot>. Paths rexenv doesn't \
    know are printed as artisan wrote them: this is raw command output, not sanitised content.";

/// The site a Laravel runner may touch: the user's, Laravel, and INSTALLED —
/// a project whose `composer create-project` is still running (or failed) has
/// no `artisan` to run, and the honest answer names `site_status`/`site_retry`
/// rather than a "No such file" from PHP. A shape check, before the gate.
fn laravel_precheck(ctx: &UserCtx<'_>, site_id: &str, tool: &str) -> Result<Site> {
    let conn = ctx.db()?;
    let Some(site) = crate::state::store::get_site(&conn, site_id)? else {
        return Err(Error::Other(format!("There is no site with id `{site_id}`. Use list_sites to see the sites that exist.")));
    };
    if site.is_scratch() {
        return Err(Error::Other(format!(
            "`{}` is a scratch site — scratch sites are WordPress, and {tool} has nothing to run there. Use wp_run on it.",
            site.domain
        )));
    }
    if site.site_type != SiteType::Laravel {
        return Err(Error::Other(format!(
            "`{}` is a {} site, not a Laravel site — {tool} has no artisan to run there.",
            site.domain,
            site.site_type.as_db()
        )));
    }
    if !crate::core::laravel::is_installed(std::path::Path::new(&site.path)) {
        return Err(Error::Other(format!(
            "`{}` has not finished installing — its project has no artisan or no installed dependencies (vendor/) yet. Check site_status; site_retry re-runs a failed install.",
            site.domain
        )));
    }
    Ok(site)
}

/// The shape of `args` for `site_artisan` — `wp_argv`'s rule with artisan's
/// words: an array of separate strings, at least one, none of them `php` or
/// `artisan` (a model that pastes the whole line gets told, not silently run).
fn artisan_argv(args: &Value) -> Result<Vec<String>> {
    let Some(list) = args.get("args").and_then(Value::as_array) else {
        return Err(Error::Other(
            "site_artisan needs `args`: the command as an array of separate words, without `php artisan` — \
             [\"migrate\", \"--seed\"], not \"php artisan migrate --seed\"."
                .into(),
        ));
    };
    let words = list
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or_else(|| Error::Other("every entry in `args` has to be a string — one artisan word per entry.".into())))
        .collect::<Result<Vec<String>>>()?;
    match words.first().map(String::as_str) {
        None => Err(Error::Other("site_artisan needs at least one word in `args` — the artisan command to run, e.g. [\"migrate:status\"].".into())),
        Some("php") | Some("artisan") => Err(Error::Other("`args` starts with the command itself — leave out `php artisan`; the first word is the artisan command, e.g. [\"migrate\", \"--seed\"].".into())),
        Some(_) => Ok(words),
    }
}

/// Run `php artisan …` in a Laravel site the user owns, under `run`.
///
/// `site_wp_run`'s shape (#480): the project root comes from the witness's
/// row, never from an argument; `--no-interaction` is rexenv's and last; the
/// timeout is `wp_run`'s; both streams pass through the one scrubber and the
/// cap; a non-zero exit is a normal result with `succeeded: false`.
fn site_artisan<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("site_artisan needs a `site_id`.".into()))?;
        let argv = artisan_argv(args)?;
        laravel_precheck(&ctx, id, "site_artisan")?;
        let wanted = format!("run `php artisan {}` in it", argv.iter().take(2).cloned().collect::<Vec<_>>().join(" "));
        let claimed = ctx.claim::<scope::Run>(Some(id), &wanted)?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("site_artisan needs a site.".into()))?;
        acted.set(&site);
        let project = std::path::PathBuf::from(&site.path);
        let php_bin = super::scratch::resolve_php(ctx.state, &site.php_version).await?;
        if !ctx.still_granted(&claimed.granted)? {
            return Err(Error::Other(format!("the `run` permission on `{}` was revoked before the command ran — nothing was run.", site.domain)));
        }
        let printed = argv.join(" ");
        let timeout = std::time::Duration::from_secs(crate::core::scratch::WP_RUN_TIMEOUT_SECS);
        // The catch-all's agent half: an agent that triggers a notification on
        // the user's own site must not be able to mail the user's customers.
        let mail_env = { let conn = ctx.db()?; crate::core::laravel::mail_env(&conn) };
        let out = crate::commands::wordpress::wp_blocking(move || crate::core::laravel::artisan_raw(&php_bin, &project, &argv, &mail_env, timeout)).await?;
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), &site.path);
        let (stdout, cut_out) = super::scratch::agent_stream(&out.stdout, &known);
        let (stderr, cut_err) = super::scratch::agent_stream(&out.stderr, &known);
        let succeeded = out.status.success();
        let exit_code = out.status.code();
        let mut detail = if succeeded {
            format!("`php artisan {printed}` ran in `{}` and succeeded.", site.domain)
        } else {
            format!("`php artisan {printed}` FAILED in `{}` (exit {}). What artisan said is in `stderr` (or `stdout` — Symfony Console writes errors to both).", site.domain, exit_code.map_or_else(|| "killed by a signal".to_string(), |c| c.to_string()))
        };
        if cut_out || cut_err {
            detail.push_str(&format!(" The output was longer than {} KB and has been cut — run a narrower command if you need the rest.", super::scratch::WP_OUTPUT_CAP / 1024));
        }
        let view = super::scratch::AgentWpRun { succeeded, exit_code, stdout, stderr, truncated: cut_out || cut_err, detail, note: ARTISAN_NOTE };
        to_json(view)
    })
}

/// The site a Composer link may go into: the user's, not WordPress (a plugin
/// has its own tools), with a `composer.json` at its project root.
fn composer_precheck(ctx: &UserCtx<'_>, site_id: &str) -> Result<Site> {
    let conn = ctx.db()?;
    let Some(site) = crate::state::store::get_site(&conn, site_id)? else {
        return Err(Error::Other(format!("There is no site with id `{site_id}`. Use list_sites to see the sites that exist.")));
    };
    if site.is_scratch() {
        return Err(Error::Other(format!(
            "`{}` is a scratch site — use scratch_add_package to put a plugin or theme into it; no permission is needed.",
            site.domain
        )));
    }
    if site.site_type == SiteType::Wordpress {
        return Err(Error::Other(format!(
            "`{}` is a WordPress site — a plugin or theme goes in through wp_plugin/wp_theme, not Composer.",
            site.domain
        )));
    }
    if !std::path::Path::new(&site.path).join("composer.json").is_file() {
        return Err(Error::Other(format!(
            "`{}` has no composer.json at its project root — there is nothing to link a package into. If the site is still installing, check site_status.",
            site.domain
        )));
    }
    Ok(site)
}

/// Link a package checkout into a user's site as a Composer path repository
/// (D14 — a SYMLINK, the S1 ruling re-run for Composer).
///
/// Order, and why: shape and site precheck refuse before any ask; the `run`
/// claim comes BEFORE the source is looked at, because reading a manifest out
/// of an arbitrary path is a read of the user's disk and belongs behind the
/// grant; then the blast-radius rule the scratch clone uses (#231: home,
/// volumes, Desktop/Documents/Downloads, app-data, any site's folder — the
/// site's own included), then the manifest, then Composer through the app.
fn composer_link<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let (id, source) = super::scratch::two_args(args, "site_id", "source", "composer_link")?;
        composer_precheck(&ctx, &id)?;
        let wanted = format!("link the Composer package in `{}` into it", basename(&source));
        let claimed = ctx.claim::<scope::Run>(Some(&id), &wanted)?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("composer_link needs a site.".into()))?;
        acted.set(&site);
        let src = {
            let conn = ctx.db()?;
            crate::core::sites::validate_linked_docroot(&conn, ctx.state.platform.as_ref(), &source)?
        };
        let link = crate::core::laravel::read_composer_link(&src)?;
        if !ctx.still_granted(&claimed.granted)? {
            return Err(Error::Other(format!("the `run` permission on `{}` was revoked before Composer ran — nothing was linked.", site.domain)));
        }
        let run = ctx.repo.composer_link(site.id.clone(), link.clone()).await?;
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), &site.path);
        // Composer prints the source's absolute path ("Symlinking from …") and
        // the one scrubber only knows rexenv's own paths — the source is
        // labelled here, before the scrub, so the reply names it and never
        // locates it. Found in the live run of 3 Sep 2026.
        let source_abs = link.source.display().to_string();
        let log: Vec<String> = run
            .log
            .iter()
            .map(|l| super::view::scrub_log_line(&l.replace(&source_abs, "<source>"), &known))
            .collect();
        if !run.ok {
            let tail: Vec<&str> = log.iter().rev().take(12).rev().map(String::as_str).collect();
            return Err(Error::Other(format!(
                "Composer could not link `{}` into `{}` — nothing is required. Its last lines:\n{}",
                link.name,
                site.domain,
                tail.join("\n")
            )));
        }
        Ok(json!({
                "ok": true,
                "package": link.name,
                "repositoryKey": link.key,
                "source": basename(&source),
                "symlinked": true,
                "log": log,
                "detail": format!(
                    "`{}` is required by `{}` at @dev through a path repository that is a SYMLINK to `{}`: the site runs the checkout live — edits show at once, no sync step — and anything the site writes under vendor/{} lands in the checkout.",
                    link.name, site.domain, basename(&source), link.name
                ),
            }))
    })
}

const LOG_DEFAULT: usize = 100;
const LOG_MAX: usize = 200;

fn site_logs<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("site_logs needs a `site_id`.".into()))?;
        let source = args.get("source").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
        let lines = args.get("lines").and_then(Value::as_u64).map_or(LOG_DEFAULT, |n| (n as usize).clamp(1, LOG_MAX));
        // A scratch site is the agent's: its debug log is tail_log's, its shared
        // logs are the same files as everyone else's — refused here so the
        // grant surface stays about the user's sites.
        {
            let conn = ctx.db()?;
            if let Some(s) = crate::state::store::get_site(&conn, id)? {
                if s.is_scratch() {
                    return Err(Error::Other(format!("`{}` is a scratch site — tail_log reads its debug log without any permission.", s.domain)));
                }
            }
        }
        let wanted = match source { Some(k) => format!("read its `{k}` log"), None => "list its logs".to_string() };
        let claimed = ctx.claim::<scope::Read>(Some(id), &wanted)?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("site_logs needs a site.".into()))?;
        acted.set(&site);
        let log_dir = ctx.state.platform.paths().log_dir()?;
        // The site's OWN target list, from core — the same one its Logs tab
        // shows — is the closed set of keys this tool will tail. A key outside
        // it (another site's, a made-up one) is refused before core is asked.
        let targets = crate::core::logs::targets_for_site(&site, &log_dir);
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), &site.path);
        let scrub = |v: Vec<String>| v.iter().map(|l| super::view::scrub_log_line(l, &known)).collect::<Vec<_>>();
        let value = match source {
            None => {
                let mut sources: Vec<Value> = targets.iter().map(|t| json!({ "key": t.key, "label": t.label, "category": t.category })).collect();
                if site.site_type == SiteType::Wordpress {
                    sources.push(json!({ "key": "wp-debug", "label": "WordPress debug log", "category": "site" }));
                }
                json!({ "domain": site.domain, "sources": sources })
            }
            Some("wp-debug") => {
                if site.site_type != SiteType::Wordpress {
                    return Err(Error::Other(format!("`{}` is not a WordPress site, so it has no debug log.", site.domain)));
                }
                let raw = crate::core::logs::wp_debug_log_tail(std::path::Path::new(&site.path), site.content_dir_rel(), lines)?;
                json!({ "domain": site.domain, "source": "wp-debug", "lines": scrub(raw) })
            }
            Some(key) => {
                if !targets.iter().any(|t| t.key == key) {
                    return Err(Error::Other(format!(
                        "`{key}` is not one of `{}`'s logs. Call site_logs without `source` to see its keys.",
                        site.domain
                    )));
                }
                let raw = crate::core::logs::tail(ctx.state.platform.as_ref(), key, lines)?;
                json!({ "domain": site.domain, "source": key, "lines": scrub(raw) })
            }
        };
        Ok(json!({ "note": "rexenv-issued login tokens, cookie headers and the paths rexenv knows are removed; the rest is the raw log.", "result": value }))
    })
}

pub(crate) fn mail_inbox_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "list" | "get" | "raw" => Scope::Read,
        "mark_read" => Scope::Manage,
        "delete" | "clear" => Scope::Destroy,
        _ => return None,
    })
}

const INBOX_DEFAULT: usize = 20;
const INBOX_MAX: usize = 50;

fn mail_inbox<'a>(ctx: UserCtx<'a>, args: &'a Value, _acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("mail_inbox needs an `action`.".into()))?;
        let scope = mail_inbox_scope(action).ok_or_else(|| Error::Other(format!("`{action}` is not a mail_inbox action. Use list, get, raw, mark_read, delete or clear.")))?;
        let message_id = || str_field(args, "message_id", action).map(str::to_string);
        let wanted = match action {
            "list" => "read the inbox".to_string(),
            "get" | "raw" => format!("read message {}", message_id()?),
            "mark_read" => "mark every message read".to_string(),
            "delete" => "delete messages from the inbox".to_string(),
            _ => "empty the inbox".to_string(),
        };
        // D16: the inbox is a READ on rexenv itself — free at the dial's Read
        // level whenever the endpoint is on; the mail switch that stood in
        // front of it is gone. The scrub below is the whole of what stands
        // between a reset link and the agent, and the note says so.
        match scope {
            Scope::Read => { ctx.claim::<scope::Read>(None, &wanted)?; }
            Scope::Manage => { ctx.claim::<scope::Manage>(None, &wanted)?; }
            _ => { ctx.claim::<scope::Destroy>(None, &wanted)?; }
        };
        let mail = ctx.mail;
        let unreachable = |e: Error| Error::Other(format!("rexenv's mail catcher isn't answering ({e}). It starts with the rest of the stack — that is the user's move in rexenv."));
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), "");
        let result = match action {
            "list" => {
                let limit = args.get("limit").and_then(Value::as_u64).map_or(INBOX_DEFAULT, |n| (n as usize).clamp(1, INBOX_MAX));
                let inbox = mail.list(args.get("query").and_then(Value::as_str).map(String::from), args.get("unread").and_then(Value::as_bool).unwrap_or(false)).await.map_err(unreachable)?;
                json!({
                    "total": inbox.total, "unread": inbox.unread,
                    "messages": inbox.messages.into_iter().take(limit).map(|m| json!({
                        "id": m.id, "from": m.from.address, "to": m.to.into_iter().map(|a| a.address).collect::<Vec<_>>(),
                        "subject": m.subject, "date": m.created, "read": m.read, "snippet": super::view::scrub_log_line(&m.snippet, &known),
                    })).collect::<Vec<_>>(),
                })
            }
            "get" => {
                let m = mail.detail(message_id()?).await.map_err(unreachable)?;
                json!({
                    "id": m.id, "from": m.from.address, "to": m.to.into_iter().map(|a| a.address).collect::<Vec<_>>(),
                    "subject": m.subject, "date": m.date,
                    "text": m.text.lines().map(|l| super::view::scrub_log_line(l, &known)).collect::<Vec<_>>().join("\n"),
                    // HTML-only mail (WooCommerce, most plugins) has no text part;
                    // the decoded HTML is the readable route, scrubbed line-wise.
                    "html": m.html.lines().map(|l| super::view::scrub_log_line(l, &known)).collect::<Vec<_>>().join("\n"),
                    // A cookie header's VALUE carries no `cookie:` prefix for the
                    // scrubber to key on, so the name decides (the review's find).
                    "headers": m.headers.into_iter().map(|h| {
                        let value = if h.name.eq_ignore_ascii_case("cookie") || h.name.eq_ignore_ascii_case("set-cookie") { "<redacted>".to_string() } else { super::view::scrub_log_line(&h.value, &known) };
                        json!({ "name": h.name, "value": value })
                    }).collect::<Vec<_>>(),
                })
            }
            "raw" => json!({ "raw": scrub_raw_source(&mail.raw(message_id()?).await.map_err(unreachable)?, &known) }),
            "mark_read" => { mail.mark_all_read().await.map_err(unreachable)?; json!({ "markedRead": true }) }
            "delete" => {
                let ids: Vec<String> = args.get("message_ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
                if ids.is_empty() {
                    return Err(Error::Other("mail_inbox `delete` needs `message_ids`.".into()));
                }
                mail.delete(ids.clone()).await.map_err(unreachable)?;
                json!({ "deleted": ids })
            }
            _ => { mail.clear().await.map_err(unreachable)?; json!({ "cleared": true }) }
        };
        Ok(json!({ "action": action, "result": result, "note": INBOX_NOTE }))
    })
}

/// What the inbox reply is, in words: every site's mail, with the token shapes
/// rexenv knows removed — and no more than that.
const INBOX_NOTE: &str = "This is the whole inbox — every site's mail, the user's own included. rexenv-issued \
    login tokens, WordPress password-reset keys and cookie headers are removed from what is shown; \
    other secrets a message carries (one-time codes, generated passwords) are not, and reading a \
    message marks it read in the user's inbox. `raw` is the message source with its headers \
    scrubbed; an encoded body (quoted-printable, base64) is omitted from it, because a line-wise \
    scrub cannot see through the encoding — use `get` for the decoded text.";

/// The RFC-822 source, scrubbed line by line — with the one honest cut: a
/// quoted-printable or base64 body is not text, so the scrub cannot see a key
/// in it, and the review that made the inbox a Read found PHPMailer switches
/// to quoted-printable for any long line (HTML mail, the common case). Such a
/// body is omitted rather than shipped; `get` returns the decoded text.
fn scrub_raw_source(source: &str, known: &super::view::KnownPaths) -> String {
    // RFC 822 source is CRLF-terminated (Mailpit hands it back as sent); a
    // `\n\n` search never matches `\r\n\r\n`, and a split that silently
    // fails would treat the whole message as headers and scrub the encoded
    // body line-wise instead of omitting it. Both line endings, blank line first.
    let (headers, body) = match source.find("\r\n\r\n").or_else(|| source.find("\n\n")) {
        Some(i) => (&source[..i], &source[i..]),
        None => (source, ""),
    };
    // Any part's transfer encoding counts — a multipart message declares it
    // per part, below the top-level headers (the review's find).
    let lower = source.to_ascii_lowercase();
    let encoded = lower
        .lines()
        .any(|l| l.trim_start().starts_with("content-transfer-encoding:") && (l.contains("quoted-printable") || l.contains("base64")));
    let mut out: Vec<String> = headers.lines().map(|l| super::view::scrub_log_line(l, known)).collect();
    if encoded {
        out.push(String::new());
        out.push("<encoded body omitted — use `get` for the decoded text>".to_string());
    } else {
        out.extend(body.lines().map(|l| super::view::scrub_log_line(l, known)));
    }
    out.join("\n")
}

pub(crate) fn stack_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "start" | "stop" => Scope::System,
        // `start_sites` / `stop_sites` touch no service and raise no password
        // dialog — they are the Sites page's bulk switch, so they sit with the
        // other `manage` arms and NOT with the stack's own start/stop.
        "restart" | "start_database" | "stop_database" | "start_mail" | "stop_mail"
        | "start_sites" | "stop_sites" => Scope::Manage,
        _ => return None,
    })
}

/// `stack` — the only parity tool whose `system` arm can raise a privileged
/// prompt, and it says so in its description: the grant is one consent, the
/// macOS dialog the second, and neither can be given by the agent. The
/// `manage` arms (a web-tier restart, an engine, the mail catcher) are
/// user-level and never prompt, exactly as the app's own commands are.
fn stack<'a>(ctx: UserCtx<'a>, args: &'a Value, _acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("stack needs an `action`.".into()))?;
        let scope = stack_scope(action).ok_or_else(|| Error::Other(format!("`{action}` is not a stack action. Use start, stop, restart, start_database, stop_database, start_mail, stop_mail, start_sites or stop_sites.")))?;
        let service = args.get("service").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
        let wanted = match action {
            "start" => "start rexenv's whole stack (macOS will also ask for your password)".to_string(),
            "stop" => "stop rexenv's whole stack — every site goes offline (macOS will also ask for your password)".to_string(),
            "restart" => format!("restart the `{}` service", service.ok_or_else(|| Error::Other("stack `restart` needs `service` (nginx, edge or php-<minor>).".into()))?),
            "start_database" | "stop_database" => {
                let s = service.ok_or_else(|| Error::Other(format!("stack `{action}` needs `service` (mysql, mariadb or postgres).")))?;
                if !matches!(s, "mysql" | "mariadb" | "postgres") {
                    return Err(Error::Other(format!("`{s}` is not a database engine here — use mysql, mariadb or postgres.")));
                }
                format!("{} the {s} engine", if action == "start_database" { "start" } else { "stop" })
            }
            "start_mail" => "start the mail catcher".to_string(),
            "stop_mail" => "stop the mail catcher".to_string(),
            "start_sites" => "serve every one of the user's sites again".to_string(),
            _ => "stop serving every one of the user's sites (rexenv's services keep running)".to_string(),
        };
        match scope {
            // `System` has no auto-allow variant (#470): this arm is reached only
            // by a person's click, and then by a second person's click in macOS.
            Scope::System => { ctx.claim::<scope::System>(None, &wanted)?; }
            _ => { ctx.claim::<scope::Manage>(None, &wanted)?; }
        };
        let st = ctx.stack;
        let result = match action {
            "start" => { st.start_all().await?; json!({ "started": true, "detail": "The stack is up. Sites are served again; stack_status shows each service." }) }
            "stop" => { st.stop_all().await?; json!({ "stopped": true, "detail": "The stack is down: every site is offline until it is started again." }) }
            "restart" => {
                let r = st.restart_web(service.unwrap().to_string()).await?;
                json!({ "service": r.service, "outcome": r.outcome })
            }
            "start_database" => { st.start_database(service.unwrap().to_string()).await?; json!({ "started": service }) }
            "stop_database" => { st.stop_database(service.unwrap().to_string()).await?; json!({ "stopped": service }) }
            "start_mail" => { st.start_mail().await?; json!({ "started": "mail" }) }
            "stop_mail" => { st.stop_mail().await?; json!({ "stopped": "mail" }) }
            act @ ("start_sites" | "stop_sites") => {
                let on = act == "start_sites";
                let r = st.set_all_sites_enabled(on).await?;
                // Counts, not "done": the user's next question is how many, and
                // a site left out because its setup never finished has to be
                // named or "5 of 6" reads as a bug.
                let mut detail = format!(
                    "{} of the user's sites {} now — {} changed by this call. rexenv's services were not touched.",
                    r.total,
                    if on { "are served" } else { "are stopped" },
                    r.changed,
                );
                if r.skipped_unprovisioned > 0 {
                    detail.push_str(&format!(
                        " {} site(s) were left alone because their setup never finished (site_retry finishes them).",
                        r.skipped_unprovisioned
                    ));
                }
                if let Some(note) = &r.note {
                    detail.push(' ');
                    detail.push_str(note);
                }
                json!({ "enabled": r.enabled, "changed": r.changed, "total": r.total, "detail": detail })
            }
            _ => unreachable!("stack_scope admits no other action"),
        };
        Ok(json!({ "action": action, "result": result }))
    })
}


pub(crate) fn php_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "install" | "uninstall" | "settings_set" | "update_check" => Scope::Manage,
        "default" | "update_apply" => Scope::System,
        _ => return None,
    })
}

fn php<'a>(ctx: UserCtx<'a>, args: &'a Value, _acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("php needs an `action`.".into()))?;
        let scope = php_scope(action).ok_or_else(|| Error::Other(format!("`{action}` is not a php action. Use install, uninstall, settings_set, update_check, default or update_apply.")))?;
        let minor = || str_field(args, "minor", action);
        let wanted = match action {
            "install" => format!("install PHP {}", minor()?),
            "uninstall" => format!("uninstall PHP {}", minor()?),
            "settings_set" => format!("set `{}` for PHP {}", str_field(args, "key", action)?, minor()?),
            "update_check" => "check for PHP updates".to_string(),
            "default" => format!("make PHP {} the default for new sites", minor()?),
            _ => format!("update PHP {} to {}", minor()?, str_field(args, "patch", action)?),
        };
        match scope {
            Scope::System => { ctx.claim::<scope::System>(None, &wanted)?; }
            _ => { ctx.claim::<scope::Manage>(None, &wanted)?; }
        };
        let sys = ctx.sys;
        let result = match action {
            "install" => { sys.set_php_installed(minor()?.to_string(), true).await?; json!({ "installed": minor()? }) }
            "uninstall" => { sys.set_php_installed(minor()?.to_string(), false).await?; json!({ "uninstalled": minor()? }) }
            "settings_set" => {
                let key = str_field(args, "key", action)?.to_string();
                let value = args.get("value").and_then(Value::as_str).unwrap_or("").to_string();
                sys.apply_php_settings(minor()?.to_string(), vec![crate::commands::php::PhpSettingInput { key: key.clone(), value: value.clone() }]).await?;
                json!({ "minor": minor()?, "key": key, "value": value })
            }
            "update_check" => {
                let views = sys.php_update_check().await?;
                json!({ "versions": views.iter().map(|v| json!({ "minor": v.minor, "patch": v.patch, "installed": v.installed, "default": v.is_default, "updatable": v.updatable })).collect::<Vec<_>>() })
            }
            "default" => { sys.set_default_php(minor()?.to_string()).await?; json!({ "default": minor()? }) }
            _ => {
                let o = sys.php_update_apply(minor()?.to_string(), str_field(args, "patch", action)?.to_string()).await?;
                json!({ "minor": minor()?, "patch": o.patch, "restarted": o.restarted })
            }
        };
        Ok(json!({ "action": action, "result": result }))
    })
}

fn settings<'a>(ctx: UserCtx<'a>, args: &'a Value, _acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let key = str_field(args, "key", "settings")?.to_string();
        let value = args.get("value").and_then(Value::as_str).ok_or_else(|| Error::Other("settings needs `value`.".into()))?.to_string();
        // The CLI's own policy, BEFORE the gate: a key that `rex config set`
        // would refuse is refused here with the same reason, and no ask is
        // recorded for a permission that could not be used.
        match crate::core::settings_access::cli_access(&key) {
            crate::core::settings_access::CliAccess::ReadWrite => {}
            crate::core::settings_access::CliAccess::ReadOnly(why) => return Err(Error::Other(format!("`{key}` is read-only: {why}."))),
            crate::core::settings_access::CliAccess::Denied(why) => return Err(Error::Other(format!("`{key}` cannot be set through an agent: {why}."))),
        }
        ctx.claim::<scope::System>(None, &format!("set the rexenv setting `{key}`"))?;
        ctx.sys.set_setting(key.clone(), value.clone()).await?;
        Ok(json!({ "key": key, "value": value, "set": true }))
    })
}

fn tld<'a>(ctx: UserCtx<'a>, args: &'a Value, _acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("tld needs an `action`.".into()))?;
        let tld = str_field(args, "tld", action)?.trim_start_matches('.').to_ascii_lowercase();
        let wanted = match action {
            "set" => format!("make `.{tld}` the default TLD and install its resolver (macOS will also ask for your password)"),
            "repair" => format!("put back the resolver file for `.{tld}` (macOS will also ask for your password)"),
            "remove" => format!("remove rexenv's resolver file for `.{tld}` (macOS will also ask for your password)"),
            other => return Err(Error::Other(format!("`{other}` is not a tld action. Use set, repair or remove."))),
        };
        ctx.claim::<scope::System>(None, &wanted)?;
        let result = match action {
            "set" => json!({ "defaultTld": ctx.sys.set_default_tld(tld.clone()).await? }),
            "repair" => json!({ "repaired": tld, "detail": ctx.sys.repair_resolver(tld.clone()).await? }),
            _ => json!({ "removed": ctx.sys.remove_resolver(tld.clone()).await?, "tld": tld }),
        };
        Ok(json!({ "action": action, "result": result }))
    })
}

fn open<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("open needs a `site_id`.".into()))?;
        let target = args.get("target").and_then(Value::as_str).ok_or_else(|| Error::Other("open needs `target`: browser, editor or finder.".into()))?;
        if !matches!(target, "browser" | "editor" | "finder") {
            return Err(Error::Other(format!("`{target}` is not an open target. Use browser, editor or finder.")));
        }
        let app = args.get("app").and_then(Value::as_str).map(str::to_string);
        let private = args.get("private").and_then(Value::as_bool).unwrap_or(false);
        let claimed = ctx.claim::<scope::Manage>(Some(id), &format!("open it in the user's {target}"))?;
        let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("open needs a site.".into()))?;
        acted.set(&site);
        let sys = ctx.sys;
        let preferred = |key: &str| -> Option<String> {
            let conn = ctx.db().ok()?;
            crate::state::store::get_setting(&conn, key).ok().flatten()
        };
        let opened = match target {
            "browser" => {
                let browsers = sys.browsers().await;
                let want = app.or_else(|| preferred(crate::commands::system::PREFERRED_BROWSER_KEY));
                let chosen = match want {
                    Some(w) => browsers.iter().find(|b| b.id == w).map(|b| b.id.clone()).ok_or_else(|| Error::Other(format!("`{w}` is not an installed browser. Installed: {}.", browsers.iter().map(|b| b.id.as_str()).collect::<Vec<_>>().join(", "))))?,
                    None => browsers.first().map(|b| b.id.clone()).ok_or_else(|| Error::Other("no browser was detected on this machine.".into()))?,
                };
                sys.open_in_browser(chosen.clone(), format!("https://{}", site.domain), private).await?;
                json!({ "browser": chosen, "url": format!("https://{}", site.domain), "private": private })
            }
            "editor" => {
                let editors = sys.editors().await;
                let want = app.or_else(|| preferred("preferred_editor"));
                let chosen = match want {
                    Some(w) => editors.iter().find(|e| e.id == w).map(|e| e.id.clone()).ok_or_else(|| Error::Other(format!("`{w}` is not an installed editor. Installed: {}.", editors.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().join(", "))))?,
                    None => editors.first().map(|e| e.id.clone()).ok_or_else(|| Error::Other("no editor was detected on this machine.".into()))?,
                };
                // The site's OWN folder — never a path the agent chose.
                sys.open_in_editor(chosen.clone(), site.path.clone()).await?;
                json!({ "editor": chosen })
            }
            _ => { sys.reveal_path(site.path.clone()).await?; json!({ "revealed": true }) }
        };
        Ok(json!({ "domain": site.domain, "target": target, "result": opened }))
    })
}


pub(crate) const SHARE_MAX_MINUTES: u64 = 60;
pub(crate) const SHARE_DEFAULT_MINUTES: u64 = 30;

/// `share` — D6's reopening conditions, met one by one: real demand (the
/// owner's brief), a consent (the dial at **Full** — D17 folded publishing into
/// the level that already says "run commands and code of its choosing … as
/// you", because asking twice for a thing the level describes was the
/// complexity the owner asked to remove), and auto-stop (a bounded timer the
/// app runs, ≤60 minutes, dying with the app like every tunnel). The bound is
/// not the consent: it holds whatever the level says.
fn share<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("share needs an `action`: start, stop or status.".into()))?;
        if action == "status" {
            let site_id = args.get("site_id").and_then(Value::as_str);
            let domain = match site_id {
                Some(id) => {
                    let c = ctx.claim::<scope::Read>(Some(id), "see whether it is shared")?;
                    let site = c.granted.site().cloned().ok_or_else(|| Error::Other("share needs a site.".into()))?;
                    acted.set(&site);
                    Some(site.domain)
                }
                None => { ctx.claim::<scope::Read>(None, "list every public share")?; None }
            };
            let all = ctx.ops.shares().await?;
            let shares: Vec<Value> = all
                .into_iter()
                .filter(|t| domain.as_deref().map_or(true, |d| d == t.domain))
                .map(|t| json!({ "domain": t.domain, "url": t.url, "running": t.running, "health": t.health, "warning": t.warning }))
                .collect();
            return Ok(json!({ "shares": shares }));
        }
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("share needs a `site_id`.".into()))?;
        match action {
            "start" => {
                let minutes = args.get("minutes").and_then(Value::as_u64).unwrap_or(SHARE_DEFAULT_MINUTES);
                if !(1..=SHARE_MAX_MINUTES).contains(&minutes) {
                    return Err(Error::Other(format!("share `minutes` must be between 1 and {SHARE_MAX_MINUTES}.")));
                }
                let claimed = ctx.claim::<scope::Run>(Some(id), &format!("publish it to the internet for {minutes} minutes"))?;
                let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("share needs a site.".into()))?;
                acted.set(&site);
                // The dial can be turned down between the claim and the tunnel:
                // publishing is the one action where that window is a URL.
                if !ctx.still_granted(&claimed.granted)? {
                    return Err(Error::Other("Agent access was turned down before the share started — nothing is published.".into()));
                }
                let info = ctx.ops.share_start(site.id.clone(), minutes).await?;
                let mut detail = format!("`{}` is public at that URL for {minutes} minutes, then rexenv stops the share on its own (or sooner if rexenv quits). Anyone with the URL reaches it.", site.domain);
                // A stopped site is shared, not refused — and the agent is told
                // what the link actually shows, in the app's own sentence. Put
                // in `detail` as well as its own field because a model relaying
                // "it's live at this URL" while the visitor gets a stop page is
                // the failure this warning exists to prevent.
                if let Some(w) = &info.warning {
                    detail.push(' ');
                    detail.push_str(w);
                }
                Ok(json!({
                    "domain": site.domain, "url": info.url, "running": info.running, "minutes": minutes,
                    "warning": info.warning,
                    "detail": detail,
                }))
            }
            "stop" => {
                let claimed = ctx.claim::<scope::Manage>(Some(id), "stop sharing it")?;
                let site = claimed.granted.site().cloned().ok_or_else(|| Error::Other("share needs a site.".into()))?;
                acted.set(&site);
                ctx.ops.share_stop(site.id.clone()).await?;
                Ok(json!({ "domain": site.domain, "stopped": true }))
            }
            other => Err(Error::Other(format!("`{other}` is not a share action. Use start or stop."))),
        }
    })
}


fn blueprints<'a>(ctx: UserCtx<'a>, args: &'a Value, _acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("blueprints needs an `action`: save or delete.".into()))?;
        let name = str_field(args, "name", action)?.to_string();
        let existing = {
            let conn = ctx.db()?;
            crate::state::store::list_blueprints(&conn)?.into_iter().find(|b| b.name.eq_ignore_ascii_case(&name))
        };
        match action {
            "save" => {
                // Shape first: the spec must be a blueprint the app would accept.
                let spec: crate::state::models::BlueprintSpec = serde_json::from_value(args.get("spec").cloned().unwrap_or(Value::Null))
                    .map_err(|e| Error::Other(format!("blueprints `save` needs a `spec` the app understands: {e}")))?;
                ctx.claim::<scope::Manage>(None, &format!("save the blueprint `{name}`"))?;
                let id = existing.map(|b| b.id).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                ctx.ops.save_blueprint(crate::state::models::Blueprint { id, name: name.clone(), spec }).await?;
                Ok(json!({ "saved": name }))
            }
            "delete" => {
                let Some(bp) = existing else {
                    return Err(Error::Other(format!("there is no blueprint called `{name}` — blueprints_list shows the saved ones.")));
                };
                ctx.claim::<scope::Destroy>(None, &format!("delete the blueprint `{name}`"))?;
                let gone = ctx.ops.delete_blueprint(bp.id).await?;
                Ok(json!({ "deleted": gone, "name": name }))
            }
            other => Err(Error::Other(format!("`{other}` is not a blueprints action. Use save or delete."))),
        }
    })
}


pub(crate) fn repo_scope(action: &str) -> Option<(Scope, bool)> {
    // (scope, stack-level?) — the two machine-wide reads take no site.
    Some(match action {
        "assets" | "status" | "branches" | "prs" | "stashes" | "scripts" | "info" | "jobs" | "job" | "watches" | "unmanaged" | "check" => (Scope::Read, false),
        "tools" | "probe" => (Scope::Read, true),
        "add" | "adopt" | "link" | "git" | "run_step" | "run_offered" | "script" | "dist_archive" | "watch_start" | "watch_stop" | "cancel" => (Scope::Run, false),
        _ => return None,
    })
}

/// A repo job, as the agent sees it: steps and their outcomes, the inspection,
/// the archive's FILE NAME, and the log through the scrubber. Never the job's
/// `path` or the archive's path.
fn job_view(st: &crate::commands::repo::RepoJobState, log: Vec<String>, known: &super::view::KnownPaths) -> Value {
    let scrub = |s: &str| super::view::scrub_log_line(s, known);
    json!({
        "jobId": st.id, "op": st.op, "kind": st.kind, "dir": st.dir_name, "url": st.url, "ref": st.git_ref,
        "finishedOk": st.finished_ok,
        "steps": st.steps.iter().map(|s| json!({ "key": s.key, "label": s.label, "status": s.status, "error": s.error.as_deref().map(scrub) })).collect::<Vec<_>>(),
        "inspection": st.inspection.as_ref().map(|i| json!({ "composer": i.composer, "node": i.node.as_ref().map(|n| json!({ "manager": n.manager, "pinnedBy": n.pinned_by, "hasBuild": n.has_build })), "wp": json!({ "kind": i.wp.kind, "name": i.wp.name }), "nodeWant": i.node_want })),
        "nodeWarning": st.node_warning.as_deref().map(scrub),
        "archive": st.archive.as_ref().map(|a| json!({ "fileName": a.file_name, "versionMissing": a.version_missing })),
        "log": log.iter().map(|l| scrub(l)).collect::<Vec<_>>(),
    })
}

fn repo<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("repo needs an `action`.".into()))?;
        let (scope, stack_level) = repo_scope(action).ok_or_else(|| Error::Other(format!("`{action}` is not a repo action.")))?;
        let kind = match args.get("kind").and_then(Value::as_str).unwrap_or("plugin") {
            k @ ("plugin" | "theme") => k.to_string(),
            other => return Err(Error::Other(format!("`{other}` is not a repo kind — use plugin or theme."))),
        };
        let dir = || str_field(args, "dir", action).map(str::to_string);
        let s = |k: &str| str_field(args, k, action).map(str::to_string);
        let install = args.get("install").and_then(Value::as_bool).unwrap_or(false);
        // Shape, per action, BEFORE the gate.
        let wanted = match action {
            "add" => format!("clone `{}` into it{}", s("url")?, if install { " and run its install steps" } else { "" }),
            "adopt" => format!("adopt the checkout `{}`", dir()?),
            "link" => format!("link the checkout at `{}` into it", basename(&s("path")?)),
            "git" => {
                let op = s("op")?;
                if !matches!(op.as_str(), "fetch" | "pull" | "checkout" | "push" | "stash" | "stash-pop" | "reset" | "status") {
                    return Err(Error::Other(format!("`{op}` is not a git op — use fetch, pull, checkout, push, stash, stash-pop, reset or status.")));
                }
                format!("run git {op} in `{}`{}", dir()?, if install { " and its install steps" } else { "" })
            }
            "run_step" => format!("run install step `{}` of job {}", s("step")?, s("job_id")?),
            "run_offered" => format!("run the install steps of job {}", s("job_id")?),
            "script" => format!("run the script `{}` in `{}`", s("script")?, dir()?),
            "dist_archive" => format!("build a distributable zip of `{}`", dir()?),
            "watch_start" => format!("start watching `{}` with `{}`", dir()?, s("script")?),
            "watch_stop" => format!("stop watch {}", s("watch_id")?),
            "cancel" => format!("cancel job {}", s("job_id")?),
            "status" | "branches" | "prs" | "stashes" | "scripts" | "check" => format!("read the repo state of `{}`", dir()?),
            "job" => format!("read job {}", s("job_id")?),
            "probe" => format!("probe the repository `{}`", s("url")?),
            other => format!("read its repo {other}"),
        };
        let site_id = if stack_level { None } else { Some(args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other(format!("repo `{action}` needs a `site_id`.")))?) };
        let site = match (scope, site_id) {
            (Scope::Read, None) => { ctx.claim::<scope::Read>(None, &wanted)?; None }
            (Scope::Read, Some(id)) => ctx.claim::<scope::Read>(Some(id), &wanted)?.granted.site().cloned(),
            (_, Some(id)) => ctx.claim::<scope::Run>(Some(id), &wanted)?.granted.site().cloned(),
            _ => unreachable!("a run action always names a site"),
        };
        if let Some(site) = &site {
            acted.set(site);
        }
        let sid = site.as_ref().map(|s| s.id.clone()).unwrap_or_default();
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), site.as_ref().map(|s| s.path.as_str()).unwrap_or(""));
        let r = ctx.repo;
        let known_ref = &known;
        let job = |st: crate::commands::repo::RepoJobState| async move {
            let log = r.job_log(st.log_key.clone()).await;
            job_view(&st, log, known_ref)
        };
        let result = match action {
            "assets" => to_json(r.assets(sid).await?)?,
            "status" => {
                let st = r.asset_status(sid, kind, dir()?).await?;
                // `link_target` is a path into the user's checkout; `log_key` an app-data file name.
                json!({ "status": st.status, "detachedAt": st.detached_at, "remote": st.remote, "lossWarning": st.loss_warning, "linked": st.link_target.is_some() })
            }
            "branches" => to_json(r.branches(sid, kind, dir()?).await?)?,
            "prs" => to_json(r.pull_refs(sid, kind, dir()?).await?)?,
            "stashes" => to_json(r.stashes(sid, kind, dir()?).await?)?,
            "scripts" => to_json(r.scripts(sid, kind, dir()?).await?)?,
            "info" => { let i = r.site_info(sid).await?; json!({ "present": i.present, "clonedFrom": i.cloned_from }) }
            "jobs" => { let all = r.site_jobs(sid, kind).await?; json!(all.iter().map(|st| json!({ "jobId": st.id, "op": st.op, "dir": st.dir_name, "finishedOk": st.finished_ok })).collect::<Vec<_>>()) }
            "job" => job(r.job_state(s("job_id")?).await?).await,
            "watches" => { let w = r.watches(sid).await?; json!(w.iter().map(|w| json!({ "watchId": w.id, "dir": w.dir_name, "kind": w.kind, "script": w.script, "status": w.status, "exit": w.exit })).collect::<Vec<_>>()) }
            "unmanaged" => to_json(r.unmanaged(sid, kind).await?)?,
            "check" => job(r.check(sid, kind, dir()?).await?).await,
            "tools" => { let t = r.tools(args.get("refresh").and_then(Value::as_bool).unwrap_or(false)).await?; json!(t.iter().map(|t| json!({ "name": t.name, "ok": t.ok, "version": t.version, "error": t.error.as_deref().map(|e| super::view::scrub_log_line(e, &known)) })).collect::<Vec<_>>()) }
            "probe" => { let p = r.probe(s("url")?).await?; json!({ "url": p.url, "host": p.host, "dir": p.dir_name, "refCandidate": p.ref_candidate, "defaultBranch": p.default_branch, "branches": p.branches }) }
            "add" => job(r.add(sid, kind, s("url")?, args.get("ref").and_then(Value::as_str).map(String::from), args.get("dir").and_then(Value::as_str).map(String::from), install).await?).await,
            "adopt" => { r.adopt(sid, kind, dir()?).await?; json!({ "adopted": dir()? }) }
            "link" => { let l = r.link(sid, kind, args.get("dir").and_then(Value::as_str).map(String::from), s("path")?).await?; json!({ "dir": l.dir_name, "isGit": l.is_git, "wp": json!({ "kind": l.wp.kind, "name": l.wp.name }) }) }
            "git" => job(r.git_op(sid, kind, dir()?, s("op")?, args.get("ref").and_then(Value::as_str).map(String::from), install).await?).await,
            "run_step" => job(r.run_step(s("job_id")?, s("step")?).await?).await,
            "run_offered" => job(r.run_offered(s("job_id")?).await?).await,
            "script" => job(r.script(sid, kind, dir()?, s("script")?).await?).await,
            "dist_archive" => job(r.dist_archive(sid, kind, dir()?).await?).await,
            "watch_start" => { let w = r.watch_start(sid, kind, dir()?, s("script")?).await?; json!({ "watchId": w.id, "status": w.status, "note": "The watcher runs inside rexenv and stops when rexenv quits." }) }
            "watch_stop" => { r.watch_stop(s("watch_id")?).await?; json!({ "stopped": true }) }
            _ => { r.cancel(s("job_id")?).await?; json!({ "cancelled": true }) }
        };
        Ok(json!({ "action": action, "result": result }))
    })
}


pub(crate) fn valet_scope(action: &str) -> Option<Scope> {
    Some(match action {
        "scan" | "drift" => Scope::Read,
        "run" => Scope::Run,
        "cancel" => Scope::Manage,
        "take_over" | "hand_back" => Scope::System,
        _ => return None,
    })
}

/// A stack-level claim by scope value — the grouped stack tools' one place.
fn claim_stack(ctx: &UserCtx<'_>, scope: Scope, wanted: &str) -> Result<()> {
    match scope {
        Scope::Read => { ctx.claim::<scope::Read>(None, wanted)?; }
        Scope::Manage => { ctx.claim::<scope::Manage>(None, wanted)?; }
        Scope::Destroy => { ctx.claim::<scope::Destroy>(None, wanted)?; }
        Scope::Run => { ctx.claim::<scope::Run>(None, wanted)?; }
        Scope::System => { ctx.claim::<scope::System>(None, wanted)?; }
    }
    Ok(())
}

fn valet_import<'a>(ctx: UserCtx<'a>, args: &'a Value, _acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("valet_import needs an `action`.".into()))?;
        let scope = valet_scope(action).ok_or_else(|| Error::Other(format!("`{action}` is not a valet_import action. Use scan, drift, run, cancel, take_over or hand_back.")))?;
        let domains: Vec<String> = args.get("domains").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(|d| d.trim().to_ascii_lowercase()).filter(|d| !d.is_empty()).collect()).unwrap_or_default();
        let wanted = match action {
            "scan" => "scan Valet/Herd for sites to import".to_string(),
            "drift" => "check the resolvers".to_string(),
            "run" => {
                if domains.is_empty() {
                    return Err(Error::Other("valet_import `run` needs `domains` — the sites to import, from `scan`.".into()));
                }
                format!("import {} from Valet/Herd", domains.join(", "))
            }
            "cancel" => "cancel the running import".to_string(),
            _ => format!("{} the resolver for `.{}` (macOS will also ask for your password)", action.replace('_', " "), str_field(args, "tld", action)?.trim_start_matches('.')),
        };
        claim_stack(&ctx, scope, &wanted)?;
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), "");
        let scrub = |s: &str| super::view::scrub_log_line(s, &known);
        let im = ctx.import;
        let result = match action {
            "scan" => {
                let scan = im.valet_scan().await?;
                json!({
                    "sources": scan.sources.iter().map(|s| json!({ "kind": s.kind, "tld": s.tld, "loopback": s.loopback, "parked": s.parked.len() })).collect::<Vec<_>>(),
                    "candidates": scan.candidates.iter().map(|c| json!({ "name": c.name, "domain": c.domain, "source": c.source, "siteType": c.site_type, "label": c.label, "docrootRel": c.docroot_rel, "phpMinor": c.php_minor, "phpTarget": c.php_target, "secured": c.secured, "proxyTo": c.proxy_to, "alsoIn": c.also_in })).collect::<Vec<_>>(),
                    "tlds": scan.tlds.iter().map(|t| json!({ "tld": t.tld, "owner": t.owner, "rexenvSites": t.rexenv_sites })).collect::<Vec<_>>(),
                    "availablePhp": scan.available_php,
                })
            }
            "drift" => json!({ "driftedTlds": im.valet_drift().await? }),
            "run" => {
                let php: std::collections::HashMap<String, String> = args.get("php").and_then(Value::as_object).map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default();
                let r = im.valet_run(crate::commands::valet_import::ImportRequest { domains, php, import_databases: args.get("import_databases").and_then(Value::as_bool).unwrap_or(false) }).await?;
                json!({
                    "imported": r.imported, "failed": r.failed, "skipped": r.skipped, "dbImported": r.db_imported, "dbFailed": r.db_failed,
                    "outcomes": r.outcomes.iter().map(|o| json!({ "domain": o.domain, "status": o.status, "reason": o.reason.as_deref().map(scrub), "siteId": o.site_id, "db": o.db.as_deref().map(scrub) })).collect::<Vec<_>>(),
                    "serving": r.serving.as_ref().map(|s| json!({ "kind": s.kind, "holder": s.holder, "app": s.app })),
                })
            }
            "cancel" => { im.valet_cancel().await?; json!({ "cancelled": true }) }
            "take_over" => { im.resolver_take_over(str_field(args, "tld", action)?.trim_start_matches('.').to_string()).await?; json!({ "takenOver": true }) }
            _ => {
                let plan = im.resolver_hand_back(str_field(args, "tld", action)?.trim_start_matches('.').to_string()).await?;
                json!({ "removed": plan.remove, "restored": plan.restore.iter().map(|(t, _)| t).collect::<Vec<_>>(), "dropRecords": plan.drop_records, "backupMissing": plan.backup_missing, "reclaimed": plan.reclaimed })
            }
        };
        Ok(json!({ "action": action, "result": result }))
    })
}

fn connection_rewrite<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other("connection_rewrite needs a `site_id`.".into()))?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("connection_rewrite needs an `action`: preview, apply or revert.".into()))?;
        let scope = match action {
            "preview" => Scope::Read,
            "apply" | "revert" => Scope::Destroy,
            other => return Err(Error::Other(format!("`{other}` is not a connection_rewrite action. Use preview, apply or revert."))),
        };
        let wanted = match action {
            "preview" => "preview the connection rewrite of its config".to_string(),
            "apply" => { str_field(args, "fingerprint", action)?; "REWRITE its config to point at rexenv's database".to_string() }
            _ => "put its original config back".to_string(),
        };
        let site = claim_scope(&ctx, id, scope, &wanted)?;
        acted.set(&site);
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), &site.path);
        let scrub = |s: &str| super::view::scrub_log_line(s, &known);
        let im = ctx.import;
        use crate::commands::rewrite::{RevertOutcome, RewriteApplied, RewritePreview};
        let result = match action {
            "preview" => match im.rewrite_preview(site.id.clone()).await? {
                RewritePreview::Ready { file, diff, fingerprint, creates_user, backup_exists, laravel_cache_warning, target } => json!({
                    "status": "ready", "file": basename(&file), "fingerprint": fingerprint, "createsUser": creates_user, "backupExists": backup_exists,
                    "laravelCacheWarning": laravel_cache_warning, "target": target,
                    "diff": diff.iter().map(|d| json!({ "sign": d.sign.to_string(), "line": d.line, "text": scrub(&d.text) })).collect::<Vec<_>>(),
                }),
                RewritePreview::Refused { reason, file } => json!({ "status": "refused", "reason": scrub(&reason), "file": file.as_deref().map(basename) }),
            },
            "apply" => match im.rewrite_apply(site.id.clone(), str_field(args, "fingerprint", action)?.to_string()).await? {
                RewriteApplied::Applied { record, message } => json!({ "status": "applied", "message": scrub(&message), "state": record.state, "dbName": record.db_name }),
                RewriteApplied::FileChanged { message } => json!({ "status": "fileChanged", "message": scrub(&message) }),
                RewriteApplied::EngineStopped { message } => json!({ "status": "engineStopped", "message": scrub(&message) }),
                RewriteApplied::VerifyFailed { reason, message } => json!({ "status": "verifyFailed", "reason": scrub(&reason), "message": scrub(&message) }),
                RewriteApplied::Refused { reason, file } => json!({ "status": "refused", "reason": scrub(&reason), "file": file.as_deref().map(basename) }),
            },
            _ => match im.rewrite_revert(site.id.clone(), args.get("force").and_then(Value::as_bool).unwrap_or(false)).await? {
                RevertOutcome::Reverted { file, message } => json!({ "status": "reverted", "file": basename(&file), "message": scrub(&message) }),
                RevertOutcome::RefusedEdited { file, reason, message } => json!({ "status": "refusedEdited", "file": basename(&file), "reason": reason, "message": scrub(&message) }),
                RevertOutcome::BackupMissing { file, message } => json!({ "status": "backupMissing", "file": basename(&file), "message": scrub(&message) }),
                RevertOutcome::NoRewrite { message } => json!({ "status": "noRewrite", "message": scrub(&message) }),
            },
        };
        Ok(json!({ "domain": site.domain, "action": action, "result": result }))
    })
}

fn db_import_job_view(st: &crate::commands::db_import::DbImportJobState, scrub: &dyn Fn(&str) -> String) -> Value {
    json!({
        "status": st.status, "pct": st.pct,
        "phases": st.phases.iter().map(|p| json!({ "key": p.key, "label": p.label, "status": p.status })).collect::<Vec<_>>(),
        "error": st.error.as_deref().map(scrub),
        "keptArtifact": st.kept_artifact.as_deref().map(basename),
        "result": st.result.as_ref().map(|r| json!({ "state": r.state, "dbName": r.db_name, "tables": r.table_count, "sizeBytes": r.size_bytes, "source": r.source_label, "skippedTables": r.skipped_tables, "importedAt": r.imported_at })),
    })
}

fn db_import<'a>(ctx: UserCtx<'a>, args: &'a Value, acted: &'a super::feed::ActedTarget) -> ToolFuture<'a> {
    Box::pin(async move {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| Error::Other("db_import needs an `action`.".into()))?;
        let known = super::view::KnownPaths::for_site(ctx.state.platform.paths(), "");
        let scrub = |s: &str| super::view::scrub_log_line(s, &known);
        let im = ctx.import;
        let site_id = || args.get("site_id").and_then(Value::as_str).ok_or_else(|| Error::Other(format!("db_import `{action}` needs a `site_id`.")));
        let value = match action {
            "records" => {
                claim_stack(&ctx, Scope::Read, "list the imported databases")?;
                let rows = im.db_import_records().await?;
                json!(rows.iter().map(|r| json!({ "siteId": r.site_id, "state": r.state, "dbName": r.db_name, "tables": r.table_count, "sizeBytes": r.size_bytes, "source": r.source_label, "importedAt": r.imported_at })).collect::<Vec<_>>())
            }
            "leftovers" => {
                claim_stack(&ctx, Scope::Read, "list the dumps kept after failed imports")?;
                let rows = im.db_import_leftovers().await?;
                json!(rows.iter().map(|l| json!({ "file": l.file, "sizeBytes": l.size_bytes })).collect::<Vec<_>>())
            }
            "delete_leftover" => {
                let file = str_field(args, "file", action)?.to_string();
                if file.contains('/') {
                    return Err(Error::Other("db_import `delete_leftover` takes a file NAME from `leftovers`, not a path.".into()));
                }
                claim_stack(&ctx, Scope::Destroy, &format!("delete the kept dump `{file}`"))?;
                im.db_import_delete_leftover(file.clone()).await?;
                json!({ "deleted": file })
            }
            "status" => {
                let site = claim_scope(&ctx, site_id()?, Scope::Read, "read its database import")?;
                acted.set(&site);
                let job = im.db_import_state(site.id.clone()).await?;
                let record = im.db_import_record(site.id.clone()).await?;
                json!({ "domain": site.domain, "job": job.as_ref().map(|j| db_import_job_view(j, &scrub)), "record": record.as_ref().map(|r| json!({ "state": r.state, "dbName": r.db_name, "tables": r.table_count, "sizeBytes": r.size_bytes, "source": r.source_label, "importedAt": r.imported_at })) })
            }
            "start" => {
                let site = claim_scope(&ctx, site_id()?, Scope::Destroy, "import its database from Valet/Herd, DROPPING the one rexenv has")?;
                acted.set(&site);
                let st = im.db_import_start(site.id.clone(), args.get("confirm_overwrite").and_then(Value::as_str).map(String::from)).await?;
                json!({ "domain": site.domain, "job": db_import_job_view(&st, &scrub) })
            }
            "cancel" => {
                let site = claim_scope(&ctx, site_id()?, Scope::Manage, "cancel its database import")?;
                acted.set(&site);
                let Some(job) = im.db_import_state(site.id.clone()).await? else {
                    return Err(Error::Other(format!("`{}` has no database import running.", site.domain)));
                };
                im.db_import_cancel(job.id).await?;
                json!({ "domain": site.domain, "cancelled": true })
            }
            other => return Err(Error::Other(format!("`{other}` is not a db_import action. Use status, records, leftovers, start, cancel or delete_leftover."))),
        };
        Ok(json!({ "action": action, "result": value }))
    })
}

#[cfg(test)]
pub(crate) mod tests {
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
        assert!(body.contains("claim_by_level::<S>"), "…and it asks the dial (D15)");
        // The dial is the only door (D17).
        // D17: there is no second door. Publishing goes through the dial like
        // everything else, so nothing in this file reaches a grant row.
        assert!(!prod.contains("claim_or_ask") && !prod.contains("claim_share"), "a grant-shaped door came back");
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
        fn app_bundle(&self) -> &dyn crate::platform::traits::AppBundle { unimplemented!() }
    }

    /// The same stub-platform state, for a sibling test module.
    pub(crate) fn app_state_for_scrub() -> AppState {
        app_state()
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
        fn share_start<'a>(&'a self, id: String, minutes: u64) -> OpFuture<'a, Result<crate::commands::tunnels::TunnelInfo>> {
            self.calls.lock().unwrap().push(format!("share start {id} {minutes}"));
            Box::pin(async {
                Ok(crate::commands::tunnels::TunnelInfo {
                    domain: "mine.rex".into(), url: "https://abc.trycloudflare.com".into(), running: true,
                    health: crate::core::tunnels::TunnelHealth::Reachable, diagnosis: None,
                    warning: None,
                })
            })
        }
        fn share_stop<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("share stop {id}"));
            Box::pin(async { Ok(()) })
        }
        fn shares<'a>(&'a self) -> OpFuture<'a, Result<Vec<crate::commands::tunnels::TunnelInfo>>> {
            Box::pin(async {
                Ok(vec![
                    crate::commands::tunnels::TunnelInfo { domain: "mine.rex".into(), url: "https://abc.trycloudflare.com".into(), running: true, health: crate::core::tunnels::TunnelHealth::Reachable, diagnosis: None, warning: None },
                    crate::commands::tunnels::TunnelInfo { domain: "other.rex".into(), url: "https://xyz.trycloudflare.com".into(), running: true, health: crate::core::tunnels::TunnelHealth::Unverified, diagnosis: None, warning: Some("other.rex is stopped in rexenv, so this link shows the \"site stopped\" page to anyone who opens it. Start the site to serve it.".into()) },
                ])
            })
        }
        fn save_blueprint<'a>(&'a self, bp: crate::state::models::Blueprint) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("blueprint save {} {}", bp.name, bp.spec.php_version));
            Box::pin(async { Ok(()) })
        }
        fn delete_blueprint<'a>(&'a self, id: String) -> OpFuture<'a, Result<bool>> {
            self.calls.lock().unwrap().push(format!("blueprint delete {id}"));
            Box::pin(async { Ok(true) })
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
        fn set_enabled<'a>(
            &'a self,
            id: String,
            enabled: bool,
        ) -> OpFuture<'a, Result<Option<crate::commands::sites::SiteEnabledReport>>> {
            self.calls.lock().unwrap().push(format!("enabled {id} {enabled}"));
            Box::pin(async move {
                Ok(Some(crate::commands::sites::SiteEnabledReport {
                    enabled,
                    serving: enabled,
                    note: None,
                    own_backend: false,
                }))
            })
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
        fn db_export<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("wp db_export {id}"));
            Box::pin(async { Ok("/Users/somebody/Downloads/blog.rex-2026-09-03.sql".into()) })
        }
        fn content_export<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<String>>> {
            self.calls.lock().unwrap().push(format!("wp content_export {id}"));
            Box::pin(async { Ok(vec!["/Users/somebody/Downloads/blog.wordpress.2026-09-03.000.xml".into()]) })
        }
        fn search_replace<'a>(&'a self, id: String, from: String, to: String, dry_run: bool) -> OpFuture<'a, Result<u64>> {
            self.calls.lock().unwrap().push(format!("wp search_replace {id} {from} {to} {dry_run}"));
            Box::pin(async { Ok(7) })
        }
        fn db_import<'a>(&'a self, id: String, path: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp db_import {id} {path}"));
            Box::pin(async { Ok(()) })
        }
        fn site_reset<'a>(&'a self, id: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp site_reset {id}"));
            Box::pin(async { Ok(()) })
        }
        fn network_sites<'a>(&'a self, id: String) -> OpFuture<'a, Result<Vec<crate::core::wordpress::WpNetworkSite>>> {
            self.calls.lock().unwrap().push(format!("wp network_sites {id}"));
            Box::pin(async { Ok(vec![]) })
        }
        fn network_site_create<'a>(&'a self, id: String, slug: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp network_site_create {id} {slug}"));
            Box::pin(async { Ok(()) })
        }
        fn network_site_delete<'a>(&'a self, id: String, blog_id: String) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("wp network_site_delete {id} {blog_id}"));
            Box::pin(async { Ok(()) })
        }
    }

    impl MailOps for FakeOps {
        fn list<'a>(&'a self, query: Option<String>, unread_only: bool) -> OpFuture<'a, Result<crate::core::mail::MailList>> {
            self.calls.lock().unwrap().push(format!("mail list {query:?} {unread_only}"));
            Box::pin(async {
                Ok(crate::core::mail::MailList {
                    total: 1, unread: 1,
                    messages: vec![crate::core::mail::MailSummary {
                        id: "m1".into(),
                        from: crate::core::mail::MailAddress { name: "WP".into(), address: "wordpress@blog.rex".into() },
                        to: vec![crate::core::mail::MailAddress { name: String::new(), address: "me@x.rex".into() }],
                        subject: "Password Reset".into(), created: "2026-09-03".into(), read: false,
                        snippet: "https://blog.rex/wp-login.php?action=rp&key=abc&rexenv_login=tok".into(),
                    }],
                })
            })
        }
        fn detail<'a>(&'a self, id: String) -> OpFuture<'a, Result<crate::core::mail::MailDetail>> {
            self.calls.lock().unwrap().push(format!("mail detail {id}"));
            Box::pin(async {
                Ok(crate::core::mail::MailDetail {
                    id: "m1".into(),
                    from: crate::core::mail::MailAddress { name: "WP".into(), address: "wordpress@blog.rex".into() },
                    to: vec![], cc: vec![], subject: "Password Reset".into(), date: "2026-09-03".into(),
                    text: "Visit https://blog.rex/?rexenv_login=tok to log in, or https://shop.rex/my-account/lost-password/?key=WOOKEY1234567890ABCD&id=3".into(), html: String::new(),
                    headers: vec![crate::core::mail::MailHeader { name: "Set-Cookie".into(), value: "wordpress_logged_in=COOKIESECRET; Path=/".into() }, crate::core::mail::MailHeader { name: "Subject".into(), value: "Password Reset".into() }],
                })
            })
        }
        fn raw<'a>(&'a self, id: String) -> OpFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(format!("mail raw {id}"));
            // CRLF, as Mailpit hands the RFC 822 source back — the split must see it.
            // Multipart: the encoding sits in a PART's headers, below the blank
            // line; the body carries no `key=` so only omission can remove it.
            Box::pin(async { Ok("Subject: x\r\nContent-Type: multipart/alternative; boundary=b1\r\nSet-Cookie: a=RAWCOOKIE\r\n\r\n--b1\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nSecret: ENCODEDBODY42\r\n--b1--\r\n".into()) })
        }
        fn mark_all_read<'a>(&'a self) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push("mail mark_all_read".into());
            Box::pin(async { Ok(()) })
        }
        fn clear<'a>(&'a self) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push("mail clear".into());
            Box::pin(async { Ok(()) })
        }
        fn delete<'a>(&'a self, ids: Vec<String>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("mail delete {}", ids.join(",")));
            Box::pin(async { Ok(()) })
        }
    }

    impl StackOps for FakeOps {
        fn start_all<'a>(&'a self) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push("stack start".into()); Box::pin(async { Ok(()) }) }
        fn stop_all<'a>(&'a self) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push("stack stop".into()); Box::pin(async { Ok(()) }) }
        fn restart_web<'a>(&'a self, target: String) -> OpFuture<'a, Result<crate::commands::services::WebRestartReport>> {
            self.calls.lock().unwrap().push(format!("stack restart {target}"));
            Box::pin(async move { Ok(crate::commands::services::WebRestartReport { service: target, outcome: "restarted" }) })
        }
        fn start_database<'a>(&'a self, key: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("db start {key}")); Box::pin(async { Ok(()) }) }
        fn stop_database<'a>(&'a self, key: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("db stop {key}")); Box::pin(async { Ok(()) }) }
        fn start_mail<'a>(&'a self) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push("mail start".into()); Box::pin(async { Ok(()) }) }
        fn stop_mail<'a>(&'a self) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push("mail stop".into()); Box::pin(async { Ok(()) }) }
        fn set_all_sites_enabled<'a>(&'a self, enabled: bool) -> OpFuture<'a, Result<crate::commands::sites::BulkEnabledReport>> {
            self.calls.lock().unwrap().push(format!("sites enabled {enabled}"));
            Box::pin(async move {
                Ok(crate::commands::sites::BulkEnabledReport {
                    enabled,
                    changed: 2,
                    total: 3,
                    skipped_unprovisioned: 1,
                    note: None,
                })
            })
        }
    }

    impl SystemOps for FakeOps {
        fn set_setting<'a>(&'a self, key: String, value: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("setting {key}={value}")); Box::pin(async { Ok(()) }) }
        fn set_default_tld<'a>(&'a self, tld: String) -> OpFuture<'a, Result<String>> { self.calls.lock().unwrap().push(format!("tld set {tld}")); Box::pin(async move { Ok(tld) }) }
        fn repair_resolver<'a>(&'a self, tld: String) -> OpFuture<'a, Result<String>> { self.calls.lock().unwrap().push(format!("tld repair {tld}")); Box::pin(async { Ok("installed".into()) }) }
        fn remove_resolver<'a>(&'a self, tld: String) -> OpFuture<'a, Result<bool>> { self.calls.lock().unwrap().push(format!("tld remove {tld}")); Box::pin(async { Ok(true) }) }
        fn set_php_installed<'a>(&'a self, minor: String, installed: bool) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("php installed {minor} {installed}")); Box::pin(async { Ok(()) }) }
        fn set_default_php<'a>(&'a self, minor: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("php default {minor}")); Box::pin(async { Ok(()) }) }
        fn apply_php_settings<'a>(&'a self, minor: String, settings: Vec<crate::commands::php::PhpSettingInput>) -> OpFuture<'a, Result<()>> {
            self.calls.lock().unwrap().push(format!("php settings {minor} {}", settings.iter().map(|s| format!("{}={}", s.key, s.value)).collect::<Vec<_>>().join(",")));
            Box::pin(async { Ok(()) })
        }
        fn php_update_check<'a>(&'a self) -> OpFuture<'a, Result<Vec<crate::state::models::PhpVersionView>>> { self.calls.lock().unwrap().push("php update_check".into()); Box::pin(async { Ok(vec![]) }) }
        fn php_update_apply<'a>(&'a self, minor: String, patch: String) -> OpFuture<'a, Result<crate::commands::php::PhpUpdateOutcome>> {
            self.calls.lock().unwrap().push(format!("php update_apply {minor} {patch}"));
            Box::pin(async move { Ok(crate::commands::php::PhpUpdateOutcome { patch, restarted: false }) })
        }
        fn browsers<'a>(&'a self) -> OpFuture<'a, Vec<crate::platform::traits::BrowserApp>> {
            Box::pin(async { vec![crate::platform::traits::BrowserApp { id: "chrome".into(), name: "Chrome".into(), icon: None, system_default: true, supports_private: true }] })
        }
        fn open_in_browser<'a>(&'a self, browser_id: String, url: String, private: bool) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("open browser {browser_id} {url} {private}")); Box::pin(async { Ok(()) }) }
        fn editors<'a>(&'a self) -> OpFuture<'a, Vec<crate::platform::traits::EditorApp>> {
            Box::pin(async { vec![crate::platform::traits::EditorApp { id: "phpstorm".into(), name: "PhpStorm".into(), icon: None }] })
        }
        fn open_in_editor<'a>(&'a self, editor_id: String, path: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("open editor {editor_id} {path}")); Box::pin(async { Ok(()) }) }
        fn reveal_path<'a>(&'a self, path: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("reveal {path}")); Box::pin(async { Ok(()) }) }
    }

    fn fake_job(op: &str) -> crate::commands::repo::RepoJobState {
        crate::commands::repo::RepoJobState {
            id: "job-1".into(), site_id: "s".into(), kind: "plugin".into(), dir_name: "acme".into(), url: "https://github.com/acme/acme.git".into(),
            git_ref: None, op: op.into(), log_key: "repo-acme.log".into(),
            steps: vec![crate::commands::repo::RepoStepState { key: op.into(), label: op.into(), status: "ok".into(), error: Some("failed at /Users/somebody/Library/Application Support/rexenv/Sites/mine.rex/wp-content/plugins/acme".into()) }],
            inspection: None, node_warning: None, finished_ok: true,
            archive: Some(crate::commands::repo::ArchiveResult { path: "/Users/somebody/Downloads/acme.zip".into(), file_name: "acme.zip".into(), version_missing: false }),
        }
    }
    impl RepoOps for FakeOps {
        fn composer_link<'a>(&'a self, site_id: String, link: crate::core::laravel::ComposerLink) -> OpFuture<'a, Result<ComposerRun>> {
            self.calls.lock().unwrap().push(format!("composer link {site_id} {} {} {}", link.name, link.key, link.source.display()));
            Box::pin(async move { Ok(ComposerRun { ok: true, log: vec![format!("$ composer require {}:@dev --no-interaction", link.name), format!("  - Installing {} (dev-main): Symlinking from {}", link.name, link.source.display())] }) })
        }
        fn assets<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<Vec<crate::state::models::GitAsset>>> { self.calls.lock().unwrap().push(format!("repo assets {site_id}")); Box::pin(async { Ok(vec![]) }) }
        fn asset_status<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<crate::commands::repo::AssetStatusResult>> {
            self.calls.lock().unwrap().push(format!("repo status {site_id} {kind} {dir}"));
            Box::pin(async { Ok(crate::commands::repo::AssetStatusResult { status: Default::default(), detached_at: None, remote: Some("origin".into()), loss_warning: None, log_key: None, link_target: Some("/Users/somebody/Projects/acme".into()), has_distignore: false }) })
        }
        fn branches<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<crate::commands::repo::RepoBranches>> { self.calls.lock().unwrap().push(format!("repo branches {site_id} {kind} {dir}")); Box::pin(async { Ok(crate::commands::repo::RepoBranches { current: Some("main".into()), local: vec!["main".into()], remote: vec![], tags: vec![] }) }) }
        fn pull_refs<'a>(&'a self, _s: String, _k: String, _d: String) -> OpFuture<'a, Result<Vec<crate::core::repo::PullRef>>> { Box::pin(async { Ok(vec![]) }) }
        fn stashes<'a>(&'a self, _s: String, _k: String, _d: String) -> OpFuture<'a, Result<Vec<crate::core::repo::StashEntry>>> { Box::pin(async { Ok(vec![]) }) }
        fn scripts<'a>(&'a self, _s: String, _k: String, _d: String) -> OpFuture<'a, Result<crate::commands::repo::RepoScriptsInfo>> { Box::pin(async { Ok(crate::commands::repo::RepoScriptsInfo { manager: None, scripts: vec![] }) }) }
        fn site_info<'a>(&'a self, _s: String) -> OpFuture<'a, Result<crate::commands::repo::SiteRepoInfo>> { Box::pin(async { Ok(crate::commands::repo::SiteRepoInfo { present: true, project_root: "/Users/somebody/Sites/mine.rex".into(), cloned_from: None }) }) }
        fn site_jobs<'a>(&'a self, _s: String, _k: String) -> OpFuture<'a, Result<Vec<crate::commands::repo::RepoJobState>>> { Box::pin(async { Ok(vec![fake_job("add")]) }) }
        fn job_state<'a>(&'a self, _j: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>> { Box::pin(async { Ok(fake_job("add")) }) }
        fn watches<'a>(&'a self, _s: String) -> OpFuture<'a, Result<Vec<crate::commands::repo::WatchState>>> { Box::pin(async { Ok(vec![]) }) }
        fn unmanaged<'a>(&'a self, _s: String, _k: String) -> OpFuture<'a, Result<Vec<crate::core::repo::UnmanagedRepo>>> { Box::pin(async { Ok(vec![]) }) }
        fn check<'a>(&'a self, _s: String, _k: String, _d: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>> { Box::pin(async { Ok(fake_job("check")) }) }
        fn tools<'a>(&'a self, refresh: bool) -> OpFuture<'a, Result<Vec<crate::commands::repo::ToolStatus>>> { self.calls.lock().unwrap().push(format!("repo tools {refresh}")); Box::pin(async { Ok(vec![]) }) }
        fn probe<'a>(&'a self, url: String) -> OpFuture<'a, Result<crate::commands::repo::RepoProbeResult>> { self.calls.lock().unwrap().push(format!("repo probe {url}")); Box::pin(async move { Ok(crate::commands::repo::RepoProbeResult { url, host: "github.com".into(), dir_name: "acme".into(), ref_candidate: None, default_branch: Some("main".into()), branches: vec![], tags: vec![] }) }) }
        fn add<'a>(&'a self, site_id: String, kind: String, url: String, _r: Option<String>, _d: Option<String>, install: bool) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>> { self.calls.lock().unwrap().push(format!("repo add {site_id} {kind} {url} {install}")); Box::pin(async { Ok(fake_job("add")) }) }
        fn adopt<'a>(&'a self, site_id: String, kind: String, dir: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("repo adopt {site_id} {kind} {dir}")); Box::pin(async { Ok(()) }) }
        fn link<'a>(&'a self, site_id: String, kind: String, _d: Option<String>, target: String) -> OpFuture<'a, Result<crate::commands::repo::RepoLinkResult>> { self.calls.lock().unwrap().push(format!("repo link {site_id} {kind} {target}")); Box::pin(async { Ok(crate::commands::repo::RepoLinkResult { dir_name: "acme".into(), is_git: true, wp: crate::core::repo::WpHeader { kind: "plugin".into(), name: Some("Acme".into()) } }) }) }
        fn git_op<'a>(&'a self, site_id: String, kind: String, dir: String, op: String, _r: Option<String>, install: bool) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>> { self.calls.lock().unwrap().push(format!("repo git {site_id} {kind} {dir} {op} {install}")); Box::pin(async move { Ok(fake_job(&op)) }) }
        fn run_step<'a>(&'a self, _j: String, _s: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>> { Box::pin(async { Ok(fake_job("add")) }) }
        fn run_offered<'a>(&'a self, _j: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>> { Box::pin(async { Ok(fake_job("add")) }) }
        fn script<'a>(&'a self, _s: String, _k: String, _d: String, _sc: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>> { Box::pin(async { Ok(fake_job("script")) }) }
        fn dist_archive<'a>(&'a self, _s: String, _k: String, _d: String) -> OpFuture<'a, Result<crate::commands::repo::RepoJobState>> { Box::pin(async { Ok(fake_job("dist_archive")) }) }
        fn watch_start<'a>(&'a self, _s: String, _k: String, _d: String, _sc: String) -> OpFuture<'a, Result<crate::commands::repo::WatchState>> { Box::pin(async { Err(Error::Other("no watches in the fake".into())) }) }
        fn watch_stop<'a>(&'a self, _i: String) -> OpFuture<'a, Result<()>> { Box::pin(async { Ok(()) }) }
        fn cancel<'a>(&'a self, _j: String) -> OpFuture<'a, Result<()>> { Box::pin(async { Ok(()) }) }
        fn job_log<'a>(&'a self, _k: String) -> OpFuture<'a, Vec<String>> { Box::pin(async { vec!["Cloning into '/Users/somebody/Library/Application Support/rexenv/Sites/mine.rex/wp-content/plugins/acme'...".into()] }) }
    }

    impl ImportOps for FakeOps {
        fn valet_scan<'a>(&'a self) -> OpFuture<'a, Result<crate::commands::valet_import::ImportScan>> {
            self.calls.lock().unwrap().push("valet scan".into());
            Box::pin(async { Ok(crate::commands::valet_import::ImportScan { sources: vec![], candidates: vec![], tlds: vec![], available_php: vec!["8.3".into()] }) })
        }
        fn valet_drift<'a>(&'a self) -> OpFuture<'a, Result<Vec<String>>> { Box::pin(async { Ok(vec!["test".into()]) }) }
        fn valet_run<'a>(&'a self, request: crate::commands::valet_import::ImportRequest) -> OpFuture<'a, Result<crate::commands::valet_import::ImportResult>> {
            self.calls.lock().unwrap().push(format!("valet run {} db={}", request.domains.join(","), request.import_databases));
            Box::pin(async { Ok(crate::commands::valet_import::ImportResult { outcomes: vec![], imported: 1, failed: 0, skipped: 0, db_imported: 0, db_failed: 0, serving: None }) })
        }
        fn valet_cancel<'a>(&'a self) -> OpFuture<'a, Result<()>> { Box::pin(async { Ok(()) }) }
        fn resolver_take_over<'a>(&'a self, tld: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("resolver take_over {tld}")); Box::pin(async { Ok(()) }) }
        fn resolver_hand_back<'a>(&'a self, _tld: String) -> OpFuture<'a, Result<crate::core::dns::ResolverPlan>> { Box::pin(async { Ok(crate::core::dns::ResolverPlan { remove: vec![], restore: vec![], drop_records: vec![], backup_missing: vec![], reclaimed: vec![] }) }) }
        fn rewrite_preview<'a>(&'a self, site_id: String) -> OpFuture<'a, Result<crate::commands::rewrite::RewritePreview>> {
            self.calls.lock().unwrap().push(format!("rewrite preview {site_id}"));
            Box::pin(async { Ok(crate::commands::rewrite::RewritePreview::Ready {
                file: "/Users/somebody/Sites/shop/wp-config.php".into(),
                diff: vec![crate::core::confedit::DiffLine { sign: '+', line: 3, text: "define('DB_HOST', '127.0.0.1:13306'); // was /Users/somebody/Library/Application Support/rexenv/x".into() }],
                fingerprint: "sha256:abc".into(), creates_user: None, backup_exists: false, laravel_cache_warning: false, target: "127.0.0.1:13306".into(),
            }) })
        }
        fn rewrite_apply<'a>(&'a self, site_id: String, fingerprint: String) -> OpFuture<'a, Result<crate::commands::rewrite::RewriteApplied>> {
            self.calls.lock().unwrap().push(format!("rewrite apply {site_id} {fingerprint}"));
            Box::pin(async { Ok(crate::commands::rewrite::RewriteApplied::FileChanged { message: "changed".into() }) })
        }
        fn rewrite_revert<'a>(&'a self, site_id: String, force: bool) -> OpFuture<'a, Result<crate::commands::rewrite::RevertOutcome>> {
            self.calls.lock().unwrap().push(format!("rewrite revert {site_id} {force}"));
            Box::pin(async { Ok(crate::commands::rewrite::RevertOutcome::NoRewrite { message: "nothing".into() }) })
        }
        fn db_import_start<'a>(&'a self, site_id: String, confirm: Option<String>) -> OpFuture<'a, Result<crate::commands::db_import::DbImportJobState>> {
            self.calls.lock().unwrap().push(format!("dbimport start {site_id} {confirm:?}"));
            Box::pin(async { Ok(crate::commands::db_import::DbImportJobState { id: "j".into(), site_id: "s".into(), domain: "mine.rex".into(), phases: vec![], phase_cursor: 0, pct: 100, status: "ok".into(), error: None, log_key: "x".into(), kept_artifact: Some("/Users/somebody/Library/Application Support/rexenv/imports/mine.sql".into()), result: None }) })
        }
        fn db_import_state<'a>(&'a self, _s: String) -> OpFuture<'a, Result<Option<crate::commands::db_import::DbImportJobState>>> { Box::pin(async { Ok(None) }) }
        fn db_import_cancel<'a>(&'a self, _j: String) -> OpFuture<'a, Result<()>> { Box::pin(async { Ok(()) }) }
        fn db_import_record<'a>(&'a self, _s: String) -> OpFuture<'a, Result<Option<crate::state::store::DbImportRecord>>> { Box::pin(async { Ok(None) }) }
        fn db_import_records<'a>(&'a self) -> OpFuture<'a, Result<Vec<crate::state::store::DbImportRecord>>> { Box::pin(async { Ok(vec![]) }) }
        fn db_import_leftovers<'a>(&'a self) -> OpFuture<'a, Result<Vec<crate::commands::db_import::LeftoverDump>>> { Box::pin(async { Ok(vec![crate::commands::db_import::LeftoverDump { file: "shop.sql".into(), path: "/Users/somebody/Library/Application Support/rexenv/imports/shop.sql".into(), size_bytes: 12 }]) }) }
        fn db_import_delete_leftover<'a>(&'a self, file: String) -> OpFuture<'a, Result<()>> { self.calls.lock().unwrap().push(format!("dbimport delete_leftover {file}")); Box::pin(async { Ok(()) }) }
    }

    /// The old "switch on": the door is open at Read by default now (D15) —
    /// kept as a no-op so each test still reads as "the surface is on".
    fn switch_on(_state: &AppState) {}

    /// Turn the dial (D15) — what a person's grant row used to do, globally.
    fn dial(state: &AppState, level: crate::core::agent_access::AccessLevel) {
        let conn = state.db.lock().unwrap();
        let mode = if level == crate::core::agent_access::AccessLevel::Read { None } else { Some(crate::core::agent_access::Mode::Always) };
        crate::core::agent_access::set(&conn, level, mode).unwrap();
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

    /// **`site_create` refuses a bad request on its shape before asking for
    /// anything, asks for `manage` on rexenv itself when the shape is fine,
    /// and — granted — runs the app's own create as the USER's site, returning
    /// the admin credentials once.**
    /// **`site_create`'s `db_engine` enum offers exactly the engines a site can
    /// be created on.** PostgreSQL sites shipped on 10 Sep 2026 with the handler
    /// parsing `postgres` and the schema still listing two engines — a client
    /// that validates against the schema (Claude Code does) could not send it,
    /// so the agent surface silently lacked the feature. Found by the site
    /// matrix run, not by a test: the handler's tests passed because they call
    /// the handler directly, past the schema.
    #[test]
    fn site_create_schema_offers_every_site_engine() {
        let offered: Vec<String> = create_params()["properties"]["db_engine"]["enum"]
            .as_array()
            .expect("db_engine is an enum")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        // Exhaustive on purpose: a fourth engine fails to COMPILE here until
        // someone decides whether the agent may create sites on it.
        let every = |e: SiteDbEngine| match e {
            SiteDbEngine::Mysql | SiteDbEngine::Mariadb | SiteDbEngine::Postgres => e.as_db().to_string(),
        };
        let expected: Vec<String> =
            [SiteDbEngine::Mysql, SiteDbEngine::Mariadb, SiteDbEngine::Postgres].into_iter().map(every).collect();
        assert_eq!(offered, expected);
        for o in &offered {
            assert!(SiteDbEngine::parse_db(o).is_ok(), "the schema offers {o} and the handler cannot parse it");
        }
    }

    /// **A half-built create hands the agent the job's OWN reason, without the
    /// app's log path or its app-only advice.** The error shape is the real one
    /// `create_site_owned_with` writes; the reason is the one a Laravel create on
    /// PHP 8.1 gave on 11 Sep 2026, which the reply used to replace with "setup
    /// did not finish" and a pointer to a log tool that never held it.
    #[test]
    fn a_half_built_create_hands_the_agent_the_jobs_own_reason_without_the_log_path() {
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut row = test_site("bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee", "shop.rex", SiteOrigin::User);
        row.provisioned = false;
        store::insert_site(&conn, &row).unwrap();
        let platform = crate::platform::current();
        let log = platform.paths().app_data_dir().unwrap().join("logs/provision/shop.rex.log");
        let error = format!(
            "site create shop.rex at \"installing Laravel\": composer create-project failed: Composer \
             refused to install laravel/framework: every release of it that runs on this PHP has a \
             published security advisory, and Composer blocks those.\n  the site stays listed as \
             \"setup incomplete\" — Retry it from the app, or delete it\n  full log: {}",
            log.display()
        );
        let acted = super::super::feed::ActedTarget::default();
        let msg = translate_create_failure(
            "shop.rex",
            crate::commands::sites::CreateFailure { site_id: Some(row.id.clone()), error: Error::Other(error) },
            Some(&conn),
            platform.paths(),
            &acted,
        )
        .to_string();
        assert!(msg.contains("laravel/framework") && msg.contains("security advisory"), "the reason: {msg}");
        assert!(!msg.contains("full log:"), "no log line: {msg}");
        assert!(!msg.contains(&log.display().to_string()), "no local path: {msg}");
        assert!(!msg.contains("Retry it from the app"), "the app's advice is not the agent's: {msg}");
        assert!(msg.contains("site_retry") && msg.contains(&row.id), "the agent's way forward: {msg}");
        assert_eq!(acted.take().as_deref(), Some(row.id.as_str()));
    }

    #[tokio::test]
    async fn site_create_refuses_shape_before_the_gate_and_asks_after_it() {
        let state = app_state();
        seed_php(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let run = |args: Value| {
            let acted = &acted;
            async move { site_create(ctx, &args, acted).await }
        };
        let ok_args = json!({ "name": "Shop", "domain": "Shop.rex", "type": "wordpress" });

        // The switch is off: refused by name, before anything else.
        let err = run(ok_args.clone()).await.unwrap_err().to_string();
        assert!(err.contains("`Agent access`"), "{err}");

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

        // A domain some site already answers on: refused, no ask.
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "taken.rex", SiteOrigin::User)).unwrap();
        }
        let err = run(json!({ "name": "x", "domain": "taken.rex", "type": "php" })).await.unwrap_err().to_string();
        assert!(err.contains("already reaches"), "{err}");

        // Good shape, no grant: refused with the place consent lives, and the
        // ask is recorded — stack-level, `manage`, naming the domain.
        let err = run(ok_args.clone()).await.unwrap_err().to_string();
        assert!(err.contains("`Agent access`") && err.contains("`manage`"), "{err}");
        assert!(ops.created.lock().unwrap().is_empty(), "nothing ran");

        // Granted: the app's create runs, as the USER's site, with the shape
        // the agent asked for and nothing it did not.
        dial(&state, crate::core::agent_access::AccessLevel::Changes);
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
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let mine = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        let theirs = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &mine).unwrap();
            store::insert_site(&conn, &theirs).unwrap();
            crate::core::agent_access::set(&conn, crate::core::agent_access::AccessLevel::Changes, Some(crate::core::agent_access::Mode::Always)).unwrap();
        }
        let err = site_delete(ctx, &json!({ "site_id": mine.id }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "a manage grant must not reach a delete: {err}");
        assert!(ops.deleted.lock().unwrap().is_empty());

        // The scratch site: refused before the gate, scratch tools named.
        let err = site_delete(ctx, &json!({ "site_id": theirs.id }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("scratch_delete_site"), "{err}");

        // A session-long destroy grant: the app's delete runs, the feed names it.
        dial(&state, crate::core::agent_access::AccessLevel::Full);
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
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
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

        // Good shape, no grant: `manage` asked for on THIS site, with the verb.
        let err = run(with("php", "version", json!("8.4"))).await.unwrap_err().to_string();
        assert!(err.contains("`Agent access`"), "{err}");
        assert!(ops.calls.lock().unwrap().is_empty());

        dial(&state, crate::core::agent_access::AccessLevel::Changes);
        // Every action reaches exactly its app command.
        let v = run(with("php", "version", json!("8.4"))).await.unwrap();
        assert_eq!(v["action"], "php");
        assert_eq!(v["domain"], "mine.rex", "the reply is the row, re-read");
        run(with("rename", "name", json!("Mine"))).await.unwrap();
        run(with("server", "server", json!("frankenphp"))).await.unwrap();
        run(with("xdebug", "enabled", json!(true))).await.unwrap();
        // The per-site switch (v44) rides the same `manage` claim — and its
        // reply carries what actually happened, not the switch echoed back: an
        // agent telling the user "started" off the flag alone would be claiming
        // a site is up that the browser answers 503 for.
        let v = run(with("enabled", "enabled", json!(false))).await.unwrap();
        assert_eq!(v["action"], "enabled");
        assert_eq!(v["serving"], json!(false));
        let err = run(json!({ "site_id": mine.id, "action": "enabled" })).await.unwrap_err().to_string();
        assert!(err.contains("needs `enabled`"), "{err}");
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
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let mine = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &mine).unwrap();
        }
        assert!(site_restart(ctx, &json!({ "site_id": mine.id }), &acted).await.is_err());
        assert!(site_retry(ctx, &json!({ "site_id": mine.id }), &acted).await.is_err());
        dial(&state, crate::core::agent_access::AccessLevel::Changes);
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
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
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

        // Dial at Read: list is free, delete names Full.
        assert!(wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "list" }), &acted).await.is_ok());
        let err = wp_plugin(ctx, &json!({ "site_id": wp_site.id, "action": "delete", "names": ["akismet"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`") && err.contains("Full"), "{err}");

        // A `manage` grant covers list (implication) and activate, not delete.
        dial(&state, crate::core::agent_access::AccessLevel::Changes);
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
        dial(&state, crate::core::agent_access::AccessLevel::Full);
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
        // D2 widened 5 Sep 2026: a login link is Read, the free level — an agent
        // gets into any site without a person logging in or a password moving.
        assert_eq!(wp_user_scope("login_url"), Some(Scope::Read));
        let desc = registry().iter().find(|t| t.name == "wp_user").unwrap().description;
        for must in ["WITHOUT a password", "single-use", "two minutes", "never recorded"] {
            assert!(desc.contains(must), "wp_user's description must say `{must}`");
        }
        assert_eq!(wp_user_scope("set_password"), Some(Scope::Destroy));
        assert_eq!(wp_user_scope("delete"), Some(Scope::Destroy));
        assert_eq!(wp_maintain_scope("core_update"), Some(Scope::Manage));
        assert_eq!(wp_maintain_scope("core_switch"), Some(Scope::Destroy));

        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "blog.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
            crate::core::agent_access::set(&conn, crate::core::agent_access::AccessLevel::Changes, Some(crate::core::agent_access::Mode::Always)).unwrap();
        }
        // The delete fork is a SHAPE refusal — neither, and both, before any ask.
        for args in [json!({ "site_id": site.id, "action": "delete", "user_id": 5 }), json!({ "site_id": site.id, "action": "delete", "user_id": 5, "reassign": 1, "delete_posts": true })] {
            let err = wp_user(ctx, &args, &acted).await.unwrap_err().to_string();
            assert!(err.contains("EXACTLY ONE"), "{err}");
        }
        // Delete needs destroy: a manage grant asks, with the posts decision in the verb.
        let err = wp_user(ctx, &json!({ "site_id": site.id, "action": "delete", "user_id": 5, "reassign": 1 }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "{err}");

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
        dial(&state, crate::core::agent_access::AccessLevel::Full);
        let v = wp_maintain(ctx, &json!({ "site_id": site.id, "action": "core_switch", "version": "6.5" }), &acted).await.unwrap();
        assert_eq!(v["result"]["dbUpdateRequired"], true);
        let v = wp_user(ctx, &json!({ "site_id": site.id, "action": "delete", "user_id": 5, "delete_posts": true }), &acted).await.unwrap();
        assert_eq!(v["result"]["postsDeleted"], true);
        let calls = ops.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|c| c == &format!("wp user delete {} 5 None true", site.id)), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with(&format!("wp user create {} bob b@x.rex editor ", site.id))), "{calls:?}");
    }

    /// **`wp_data`, `wp_network` and `site_wp_run`: a dry run manages and a live
    /// one destroys; exported files are named, never located; the raw runner
    /// needs `run` and screens the target BEFORE anything is resolved.**
    #[tokio::test]
    async fn wp_data_network_and_the_raw_runner_gate_per_action_and_name_no_path() {
        assert_eq!(wp_data_scope("search_replace", true), Some(Scope::Manage));
        assert_eq!(wp_data_scope("search_replace", false), Some(Scope::Destroy));
        assert_eq!(wp_data_scope("db_import", true), Some(Scope::Destroy), "dry_run means nothing to an import");
        assert_eq!(wp_data_scope("reset", true), Some(Scope::Destroy));
        assert_eq!(wp_network_scope("sites"), Some(Scope::Read));
        assert_eq!(wp_network_scope("site_delete"), Some(Scope::Destroy));

        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "blog.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
            crate::core::agent_access::set(&conn, crate::core::agent_access::AccessLevel::Changes, Some(crate::core::agent_access::Mode::Always)).unwrap();
        }
        let v = wp_data(ctx, &json!({ "site_id": site.id, "action": "db_export" }), &acted).await.unwrap();
        assert_eq!(v["result"]["file"], "blog.rex-2026-09-03.sql");
        assert!(!v.to_string().contains("/Users/"), "the Downloads path stayed inside rexenv: {v}");
        let v = wp_data(ctx, &json!({ "site_id": site.id, "action": "search_replace", "from": "http://old", "to": "https://new", "dry_run": true }), &acted).await.unwrap();
        assert_eq!(v["result"]["replacements"], 7);
        let err = wp_data(ctx, &json!({ "site_id": site.id, "action": "search_replace", "from": "a", "to": "b", "dry_run": false }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "a live replace destroys: {err}");
        let err = wp_data(ctx, &json!({ "site_id": site.id, "action": "db_import", "path": "/tmp/dump.sql" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "{err}");

        let err = wp_network(ctx, &json!({ "site_id": site.id, "action": "convert", "mode": "mesh" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not a multisite mode"), "{err}");
        let v = wp_network(ctx, &json!({ "site_id": site.id, "action": "convert", "mode": "subdomain" }), &acted).await.unwrap();
        assert_eq!(v["result"]["converted"], "subdomain");
        assert_eq!(ops.converted.lock().unwrap().len(), 1, "convert goes through the app's SHARE-GUARDED command");
        let v = wp_network(ctx, &json!({ "site_id": site.id, "action": "sites" }), &acted).await.unwrap();
        assert_eq!(v["action"], "sites");
        assert!(wp_network(ctx, &json!({ "site_id": site.id, "action": "site_delete", "blog_id": "3" }), &acted).await.is_err());

        // The raw runner: `run` is its own scope — manage does not reach it —
        // and the target screen fires BEFORE any binary is resolved (the stub
        // platform would panic on `binaries()`; it is never reached).
        let err = site_wp_run(ctx, &json!({ "site_id": site.id, "args": ["plugin", "list", "--path=/etc"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("--path"), "the target screen is a shape refusal: {err}");
        let err = site_wp_run(ctx, &json!({ "site_id": site.id, "args": ["plugin", "list"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`run`"), "{err}");
        dial(&state, crate::core::agent_access::AccessLevel::Full);
        let err = site_wp_run(ctx, &json!({ "site_id": site.id, "args": ["plugin", "list", "--path=/etc"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("--path"), "the target screen, before resolution: {err}");
        let err = site_wp_run(ctx, &json!({ "site_id": site.id, "args": "plugin list" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("array"), "{err}");
    }

    /// **`site_logs` tails only the site's own target keys under `read`, and
    /// `mail_inbox` needs the mail switch AND a stack-level grant per action —
    /// with tokens scrubbed from what comes back.**
    #[tokio::test]
    async fn site_logs_and_mail_inbox_gate_and_scrub() {
        assert_eq!(mail_inbox_scope("list"), Some(Scope::Read));
        assert_eq!(mail_inbox_scope("mark_read"), Some(Scope::Manage));
        assert_eq!(mail_inbox_scope("clear"), Some(Scope::Destroy));
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "blog.rex", SiteOrigin::User);
        let theirs = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
            store::insert_site(&conn, &theirs).unwrap();
        }
        let err = site_logs(ctx, &json!({ "site_id": theirs.id }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("tail_log"), "{err}");
        // D15: a read is free at the dial's default.
        let v = site_logs(ctx, &json!({ "site_id": site.id }), &acted).await.unwrap();
        let sources = v["result"]["sources"].as_array().unwrap();
        assert!(sources.iter().any(|s| s["key"] == "wp-debug"));
        assert!(!v.to_string().contains("/Users/somebody"), "a target's path leaked: {v}");
        let err = site_logs(ctx, &json!({ "site_id": site.id, "source": "../etc/passwd" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not one of"), "a key outside the site's list is refused before core: {err}");

        // D16: the inbox is a read — free at the dial's Read, no switch in front.
        let v = mail_inbox(ctx, &json!({ "action": "list", "unread": true }), &acted).await.unwrap();
        let snippet = v["result"]["messages"][0]["snippet"].as_str().unwrap();
        assert!(!snippet.contains("rexenv_login=tok"), "a login token left in a snippet: {snippet}");
        assert!(!snippet.contains("key=abc") && snippet.contains("key=<redacted>"), "a reset key left in a snippet: {snippet}");
        assert!(v["note"].as_str().unwrap().contains("every site's mail"), "the inbox note: {v}");
        let v = mail_inbox(ctx, &json!({ "action": "get", "message_id": "m1" }), &acted).await.unwrap();
        let text = v["result"]["text"].as_str().unwrap();
        assert!(!text.contains("=tok") && !text.contains("WOOKEY"), "a token or a WooCommerce reset key left in the body: {text}");
        assert!(v["result"]["html"].is_string(), "the decoded HTML route exists for HTML-only mail");
        let headers = v["result"]["headers"].as_array().unwrap();
        assert!(headers.iter().any(|h| h["name"] == "Set-Cookie" && h["value"] == "<redacted>"), "a cookie header value left: {headers:?}");
        assert!(headers.iter().any(|h| h["name"] == "Subject" && h["value"] == "Password Reset"), "a benign header lost: {headers:?}");
        let v = mail_inbox(ctx, &json!({ "action": "raw", "message_id": "m1" }), &acted).await.unwrap();
        let raw = v["result"]["raw"].as_str().unwrap();
        assert!(!raw.contains("ENCODEDBODY42") && raw.contains("encoded body omitted") && !raw.contains("RAWCOOKIE"), "an encoded body or a raw cookie header reached the agent: {raw}");
        let err = mail_inbox(ctx, &json!({ "action": "clear" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "{err}");
        let err = mail_inbox(ctx, &json!({ "action": "mark_read" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`manage`"), "{err}");
        assert!(ops.calls.lock().unwrap().iter().any(|c| c == "mail list None true"));
    }

    /// **The stack's start/stop are reachable only through `system` — a scope
    /// with no auto-allow variant — and the two privileged-reaching app calls
    /// live in exactly that arm; a restart, an engine or the mail catcher are
    /// `manage` on rexenv itself and never prompt.**
    #[tokio::test]
    async fn stack_start_and_stop_need_system_and_nothing_else_reaches_the_privileged_calls() {
        assert_eq!(stack_scope("start"), Some(Scope::System));
        assert_eq!(stack_scope("stop"), Some(Scope::System));
        assert_eq!(stack_scope("restart"), Some(Scope::Manage));
        assert_eq!(stack_scope("stop_database"), Some(Scope::Manage));
        // The source: `start_all(`/`stop_all(` appear ONCE each in production
        // code, inside `stack`, and the arm that reaches them claims `System`.
        let me = include_str!("user_sites.rs");
        let prod = &me[..me.find("#[cfg(test)]").unwrap()];
        let handler = &prod[prod.find("fn stack<'a>(").unwrap()..];
        let handler = &handler[..handler.find("\n}\n").unwrap()];
        for call in ["st.start_all(", "st.stop_all("] {
            assert_eq!(prod.matches(call).count(), 1, "{call} must be called from exactly one place");
            assert!(handler.contains(call), "{call} must be inside the stack handler");
        }
        assert!(handler.contains("ctx.claim::<scope::System>"), "the privileged arm claims System");
        assert!(handler.find("ctx.claim::<scope::System>").unwrap() < handler.find("st.start_all(").unwrap());

        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let err = stack(ctx, &json!({ "action": "start" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`system`") && err.contains("rexenv itself"), "{err}");
        let err = stack(ctx, &json!({ "action": "stop_database", "service": "redis" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not a database engine"), "{err}");
        dial(&state, crate::core::agent_access::AccessLevel::Changes);
        // D15: `system` sits at Changes — the macOS dialog is the second consent.
        assert!(stack(ctx, &json!({ "action": "start" }), &acted).await.is_ok(), "Changes covers the stack's start");
        let v = stack(ctx, &json!({ "action": "restart", "service": "nginx" }), &acted).await.unwrap();
        assert_eq!(v["result"]["outcome"], "restarted");
        let v = stack(ctx, &json!({ "action": "start_database", "service": "mysql" }), &acted).await.unwrap();
        assert_eq!(v["result"]["started"], "mysql");
        stack(ctx, &json!({ "action": "stop_mail" }), &acted).await.unwrap();
        dial(&state, crate::core::agent_access::AccessLevel::Changes);
        let v = stack(ctx, &json!({ "action": "stop" }), &acted).await.unwrap();
        assert_eq!(v["result"]["stopped"], true);
        let calls = ops.calls.lock().unwrap().clone();
        for c in ["stack restart nginx", "db start mysql", "mail stop", "stack stop"] {
            assert!(calls.iter().any(|x| x == c), "missing {c} in {calls:?}");
        }
        assert_eq!(calls.iter().filter(|x| *x == "stack start").count(), 1, "start ran once, under Changes");

        // **The bulk site switch is `manage`, NOT `system`** (v44): it touches no
        // service and raises no password dialog — it changes which sites the
        // running stack serves. Filing it with the stack's own start/stop would
        // have made "stop the user's sites" as heavy as "stop their machine's
        // web server", and (worse) implied a privileged prompt that never comes.
        assert_eq!(stack_scope("start_sites"), Some(Scope::Manage));
        assert_eq!(stack_scope("stop_sites"), Some(Scope::Manage));
        let v = stack(ctx, &json!({ "action": "stop_sites" }), &acted).await.unwrap();
        assert_eq!(v["result"]["enabled"], false);
        assert_eq!(v["result"]["total"], 3);
        let detail = v["result"]["detail"].as_str().unwrap_or_default().to_string();
        // Counts, the skipped half-provisioned sites NAMED (or "5 of 6" reads as
        // a bug), and the fact the services were left alone — an agent relaying
        // "stopped everything" would otherwise be describing `stack stop`.
        assert!(detail.contains("3 of the user's sites"), "{detail}");
        assert!(detail.contains("setup never finished"), "{detail}");
        assert!(detail.contains("services were not touched"), "{detail}");
        let v = stack(ctx, &json!({ "action": "start_sites" }), &acted).await.unwrap();
        assert_eq!(v["result"]["enabled"], true);
        let calls = ops.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|c| c == "sites enabled false"));
        assert!(calls.iter().any(|c| c == "sites enabled true"));
        // …and it never reached the stack's own lifecycle.
        assert_eq!(calls.iter().filter(|x| *x == "stack stop").count(), 1, "the bulk switch stopped the STACK");
    }

    /// **`php`, `settings`, `tld`, `open`: the per-action tables, the CLI's
    /// settings policy applied BEFORE the gate, the resolver writes naming the
    /// password dialog, and `open` reaching only the site's own URL and folder.**
    #[tokio::test]
    async fn php_settings_tld_and_open_gate_per_action_and_reach_only_the_sites_own_things() {
        assert_eq!(php_scope("install"), Some(Scope::Manage));
        assert_eq!(php_scope("default"), Some(Scope::System));
        assert_eq!(php_scope("update_apply"), Some(Scope::System));
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "blog.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
        }
        // settings: the CLI's policy first — a Denied or ReadOnly key records no ask.
        let err = settings(ctx, &json!({ "key": "mcp_enabled", "value": "true" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("cannot be set through an agent"), "{err}");
        let err = settings(ctx, &json!({ "key": "adminer_version", "value": "5" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("read-only"), "{err}");
        let err = settings(ctx, &json!({ "key": "preferred_browser", "value": "chrome" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`system`"), "{err}");
        // tld: every action names the password dialog in its ask.
        assert!(tld(ctx, &json!({ "action": "repair", "tld": ".Test" }), &acted).await.is_err());
        // php: manage for install, system for default.
        {
            let conn = state.db.lock().unwrap();
            crate::core::agent_access::set(&conn, crate::core::agent_access::AccessLevel::Changes, Some(crate::core::agent_access::Mode::Always)).unwrap();
            crate::core::agent_access::set(&conn, crate::core::agent_access::AccessLevel::Changes, Some(crate::core::agent_access::Mode::Always)).unwrap();
        }
        let v = php(ctx, &json!({ "action": "install", "minor": "8.4" }), &acted).await.unwrap();
        assert_eq!(v["result"]["installed"], "8.4");
        let v = php(ctx, &json!({ "action": "settings_set", "minor": "8.3", "key": "memory_limit", "value": "768M" }), &acted).await.unwrap();
        assert_eq!(v["result"]["key"], "memory_limit");
        // D15: `system` is Changes — the default switch runs under it.
        let v = php(ctx, &json!({ "action": "default", "minor": "8.4" }), &acted).await.unwrap();
        assert_eq!(v["result"]["default"], "8.4");
        let err = php(ctx, &json!({ "action": "paint" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not a php action"), "{err}");
        // open: the site's own URL and folder, the preferred app or the named one.
        let v = open(ctx, &json!({ "site_id": site.id, "target": "browser", "private": true }), &acted).await.unwrap();
        assert_eq!(v["result"]["url"], "https://blog.rex");
        let err = open(ctx, &json!({ "site_id": site.id, "target": "browser", "app": "netscape" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not an installed browser") && err.contains("chrome"), "{err}");
        open(ctx, &json!({ "site_id": site.id, "target": "editor" }), &acted).await.unwrap();
        open(ctx, &json!({ "site_id": site.id, "target": "finder" }), &acted).await.unwrap();
        let err = open(ctx, &json!({ "site_id": site.id, "target": "terminal" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not an open target"), "{err}");
        dial(&state, crate::core::agent_access::AccessLevel::Changes);
        let v = settings(ctx, &json!({ "key": "preferred_browser", "value": "chrome" }), &acted).await.unwrap();
        assert_eq!(v["set"], true);
        let v = tld(ctx, &json!({ "action": "set", "tld": "dev" }), &acted).await.unwrap();
        assert_eq!(v["result"]["defaultTld"], "dev");
        let v = php(ctx, &json!({ "action": "default", "minor": "8.4" }), &acted).await.unwrap();
        assert_eq!(v["result"]["default"], "8.4");
        let calls = ops.calls.lock().unwrap().clone();
        for c in ["php installed 8.4 true", "php settings 8.3 memory_limit=768M", "open browser chrome https://blog.rex true", &format!("open editor phpstorm {}", site.path), &format!("reveal {}", site.path), "setting preferred_browser=chrome", "tld set dev", "php default 8.4"] {
            assert!(calls.iter().any(|x| x == c), "missing {c} in {calls:?}");
        }
    }

    /// **`share start` needs `run` given by a PERSON — an auto-granted row is
    /// refused even when a later claim finds it — is bounded to 60 minutes, and
    /// `stop` needs only `manage`.**
    #[tokio::test]
    async fn share_needs_the_dial_at_full_and_is_bounded() {
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
        }
        // The minutes bound is a SHAPE refusal — before the dial, whatever it says.
        let err = share(ctx, &json!({ "site_id": site.id, "action": "start", "minutes": 61 }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("between 1 and 60"), "{err}");
        // D17: publishing is `run` — the dial at Full, no prompt, no grant row.
        let err = share(ctx, &json!({ "site_id": site.id, "action": "start" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`run`") && err.contains("Full"), "{err}");
        assert!(ops.calls.lock().unwrap().iter().all(|c| !c.starts_with("share start")), "nothing was published");
        dial(&state, crate::core::agent_access::AccessLevel::Full);
        let v = share(ctx, &json!({ "site_id": site.id, "action": "start", "minutes": 10 }), &acted).await.unwrap();
        assert_eq!(v["url"], "https://abc.trycloudflare.com");
        assert!(v["detail"].as_str().unwrap().contains("10 minutes"));
        dial(&state, crate::core::agent_access::AccessLevel::Read);
        let err = share(ctx, &json!({ "site_id": site.id, "action": "stop" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`manage`"), "stop is a manage action on the dial, not the share grant: {err}");
        dial(&state, crate::core::agent_access::AccessLevel::Changes);
        share(ctx, &json!({ "site_id": site.id, "action": "stop" }), &acted).await.unwrap();
        let calls = ops.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|c| c == &format!("share start {} 10", site.id)) && calls.iter().any(|c| c == &format!("share stop {}", site.id)), "{calls:?}");
    }

    /// **The scratch cap and TTL are settings with a range, read live, and the
    /// defaults when unset or nonsense.**
    #[test]
    fn scratch_cap_and_ttl_are_ranged_settings_with_the_old_constants_as_defaults() {
        use crate::core::scratch::{scratch_cap, scratch_ttl_hours, set_scratch_cap, set_scratch_ttl_hours, MAX_SCRATCH_SITES};
        let conn = crate::state::db::open_in_memory().unwrap();
        assert_eq!(scratch_cap(&conn), MAX_SCRATCH_SITES);
        assert_eq!(scratch_ttl_hours(&conn), crate::core::sites::SCRATCH_TTL_HOURS);
        assert!(set_scratch_cap(&conn, "0").is_err() && set_scratch_cap(&conn, "21").is_err() && set_scratch_cap(&conn, "five").is_err());
        set_scratch_cap(&conn, "12").unwrap();
        assert_eq!(scratch_cap(&conn), 12);
        assert!(set_scratch_ttl_hours(&conn, "0").is_err() && set_scratch_ttl_hours(&conn, "169").is_err());
        set_scratch_ttl_hours(&conn, "72").unwrap();
        assert_eq!(scratch_ttl_hours(&conn), 72);
        // A hand-written nonsense value reads as the default, never as zero.
        store::set_setting(&conn, crate::core::scratch::SCRATCH_CAP_KEY, "lots").unwrap();
        assert_eq!(scratch_cap(&conn), MAX_SCRATCH_SITES);
        // …and both are writable through the CLI's policy because they are gated.
        assert_eq!(crate::core::settings_access::cli_access(crate::core::scratch::SCRATCH_CAP_KEY), crate::core::settings_access::CliAccess::ReadWrite);
    }

    /// **`share status` reads under `read` (a site's, or all under rexenv
    /// itself); `blueprints` save is `manage` with the spec validated on shape
    /// first, delete is `destroy`, both by NAME.**
    #[tokio::test]
    async fn share_status_and_blueprints_gate_and_work_by_name() {
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
        }
        let v = share(ctx, &json!({ "action": "status", "site_id": site.id }), &acted).await.unwrap();
        assert_eq!(v["shares"].as_array().unwrap().len(), 1, "only this site's share: {v}");
        assert_eq!(v["shares"][0]["domain"], "mine.rex");
        // D15: reads on rexenv itself are free too.
        let v = share(ctx, &json!({ "action": "status" }), &acted).await.unwrap();
        assert_eq!(v["shares"].as_array().unwrap().len(), 2);

        // blueprints: a bad spec is a shape refusal; save needs Changes; delete Full.
        let err = blueprints(ctx, &json!({ "action": "save", "name": "Shop", "spec": { "siteType": "drupal" } }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("the app understands"), "{err}");
        let good = json!({ "action": "save", "name": "Shop", "spec": { "siteType": "wordpress", "phpVersion": "8.3", "webServer": "nginx" } });
        let err = blueprints(ctx, &good, &acted).await.unwrap_err().to_string();
        assert!(err.contains("`manage`") && err.contains("Changes"), "{err}");
        dial(&state, crate::core::agent_access::AccessLevel::Changes);
        let v = blueprints(ctx, &good, &acted).await.unwrap();
        assert_eq!(v["saved"], "Shop");
        let err = blueprints(ctx, &json!({ "action": "delete", "name": "Nope" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("no blueprint called"), "{err}");
        assert!(ops.calls.lock().unwrap().iter().any(|c| c == "blueprint save Shop 8.3"));
    }

    /// **`repo`: reads under `read` on the site (two under rexenv itself), every
    /// write under `run`; a job reply carries steps, the archive's file name and
    /// the scrubbed log — never a path.**
    #[tokio::test]
    async fn repo_reads_under_read_writes_under_run_and_replies_name_no_path() {
        assert_eq!(repo_scope("assets"), Some((Scope::Read, false)));
        assert_eq!(repo_scope("tools"), Some((Scope::Read, true)));
        assert_eq!(repo_scope("add"), Some((Scope::Run, false)));
        assert_eq!(repo_scope("watch_stop"), Some((Scope::Run, false)));
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
            crate::core::agent_access::set(&conn, crate::core::agent_access::AccessLevel::Read, Some(crate::core::agent_access::Mode::Always)).unwrap();
        }
        let err = repo(ctx, &json!({ "site_id": site.id, "action": "git", "dir": "acme", "op": "rebase" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not a git op"), "{err}");
        let err = repo(ctx, &json!({ "site_id": site.id, "action": "add", "url": "https://github.com/acme/acme.git" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`run`"), "a read grant does not clone: {err}");
        let v = repo(ctx, &json!({ "site_id": site.id, "action": "status", "dir": "acme" }), &acted).await.unwrap();
        assert_eq!(v["result"]["linked"], true);
        assert!(!v.to_string().contains("/Users/"), "the link target is a path: {v}");
        let v = repo(ctx, &json!({ "site_id": site.id, "action": "info" }), &acted).await.unwrap();
        assert!(v["result"].get("projectRoot").is_none() && !v.to_string().contains("/Users/"));
        let v = repo(ctx, &json!({ "site_id": site.id, "action": "job", "job_id": "job-1" }), &acted).await.unwrap();
        assert_eq!(v["result"]["archive"]["fileName"], "acme.zip");
        assert!(v["result"].get("path").is_none());
        let text = v.to_string();
        assert!(!text.contains("/Users/somebody"), "a path in the job reply or its log: {text}");
        assert!(text.contains("<"), "scrubbed to a label: {text}");
        assert!(repo(ctx, &json!({ "action": "tools" }), &acted).await.is_ok(), "tools is a read on rexenv itself — free at the dial's default");
        {
            let conn = state.db.lock().unwrap();
            crate::core::agent_access::set(&conn, crate::core::agent_access::AccessLevel::Read, Some(crate::core::agent_access::Mode::Always)).unwrap();
            crate::core::agent_access::set(&conn, crate::core::agent_access::AccessLevel::Full, Some(crate::core::agent_access::Mode::Always)).unwrap();
        }
        repo(ctx, &json!({ "action": "tools", "refresh": true }), &acted).await.unwrap();
        let v = repo(ctx, &json!({ "site_id": site.id, "action": "add", "url": "https://github.com/acme/acme.git", "install": true }), &acted).await.unwrap();
        assert_eq!(v["result"]["finishedOk"], true);
        let v = repo(ctx, &json!({ "site_id": site.id, "action": "link", "path": "/Users/somebody/Projects/acme", "kind": "theme" }), &acted).await.unwrap();
        assert_eq!(v["result"]["wp"]["name"], "Acme");
        repo(ctx, &json!({ "site_id": site.id, "action": "git", "dir": "acme", "op": "pull" }), &acted).await.unwrap();
        let calls = ops.calls.lock().unwrap().clone();
        for c in ["repo tools true", &format!("repo add {} plugin https://github.com/acme/acme.git true", site.id), &format!("repo link {} theme /Users/somebody/Projects/acme", site.id), &format!("repo git {} plugin acme pull false", site.id)] {
            assert!(calls.iter().any(|x| x == c), "missing {c} in {calls:?}");
        }
    }

    /// **The migration surfaces: scan/drift `read`, import `run`, resolver
    /// writes `system` naming the dialog; a rewrite preview reads and apply /
    /// revert destroy, with the file named and the diff scrubbed; a database
    /// import destroys, its kept dump named never located, and a leftover is
    /// deleted by NAME only.**
    #[tokio::test]
    async fn valet_rewrite_and_db_import_gate_per_action_and_name_no_path() {
        assert_eq!(valet_scope("scan"), Some(Scope::Read));
        assert_eq!(valet_scope("run"), Some(Scope::Run));
        assert_eq!(valet_scope("take_over"), Some(Scope::System));
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "mine.rex", SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &site).unwrap();
        }
        let err = valet_import(ctx, &json!({ "action": "run" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("needs `domains`"), "{err}");
        assert!(valet_import(ctx, &json!({ "action": "scan" }), &acted).await.is_ok(), "a scan is a read — free");
        let err = valet_import(ctx, &json!({ "action": "take_over", "tld": ".test" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`system`") && err.contains("Changes"), "{err}");
        let err = db_import(ctx, &json!({ "action": "delete_leftover", "file": "/etc/passwd" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("NAME"), "a path is refused on shape: {err}");
        // Reads are free; the writes name their level BEFORE the dial moves.
        let v = valet_import(ctx, &json!({ "action": "scan" }), &acted).await.unwrap();
        assert_eq!(v["result"]["availablePhp"], json!(["8.3"]));
        let err = valet_import(ctx, &json!({ "action": "run", "domains": ["Shop.test"], "import_databases": true }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`run`") && err.contains("Full"), "{err}");
        let v = connection_rewrite(ctx, &json!({ "site_id": site.id, "action": "preview" }), &acted).await.unwrap();
        assert_eq!(v["result"]["file"], "wp-config.php");
        assert!(!v.to_string().contains("/Users/somebody"), "the diff and file are scrubbed/named: {v}");
        let err = connection_rewrite(ctx, &json!({ "site_id": site.id, "action": "apply", "fingerprint": "sha256:abc" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`destroy`"), "{err}");
        let err = connection_rewrite(ctx, &json!({ "site_id": site.id, "action": "apply" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("needs `fingerprint`"), "{err}");
        let v = db_import(ctx, &json!({ "action": "leftovers" }), &acted).await.unwrap();
        assert_eq!(v["result"][0]["file"], "shop.sql");
        assert!(v["result"][0].get("path").is_none());
        assert!(db_import(ctx, &json!({ "action": "start", "site_id": site.id }), &acted).await.is_err(), "an import destroys");
        dial(&state, crate::core::agent_access::AccessLevel::Full);
        let v = valet_import(ctx, &json!({ "action": "run", "domains": ["Shop.test"], "import_databases": true }), &acted).await.unwrap();
        assert_eq!(v["result"]["imported"], 1);
        let v = db_import(ctx, &json!({ "action": "start", "site_id": site.id, "confirm_overwrite": "mine.rex" }), &acted).await.unwrap();
        assert_eq!(v["result"]["job"]["status"], "ok");
        assert_eq!(v["result"]["job"]["keptArtifact"], "mine.sql", "named, never located");
        let calls = ops.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|c| c == "valet run shop.test db=true"), "{calls:?}");
        assert!(calls.iter().any(|c| c == &format!("dbimport start {} Some(\"mine.rex\")", site.id)), "{calls:?}");
    }

    /// **`site_artisan` refuses on shape and on the site (not Laravel, scratch,
    /// not finished installing) before any ask, and — the project real — asks
    /// for `run` on the site.** A pasted `php artisan …` is told, not run.
    #[tokio::test]
    async fn site_artisan_refuses_shape_and_unfinished_projects_before_the_gate_then_asks_run() {
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let dir = std::env::temp_dir().join(format!("rexenv-artisan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("public")).unwrap();
        let wp = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "blog.rex", SiteOrigin::User);
        let scratch = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        let mut lv = test_site("bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee", "shop.rex", SiteOrigin::User);
        lv.site_type = SiteType::Laravel;
        lv.path = dir.display().to_string();
        lv.docroot_subdir = "public".into();
        {
            let conn = state.db.lock().unwrap();
            for s in [&wp, &scratch, &lv] {
                store::insert_site(&conn, s).unwrap();
            }
        }
        let err = site_artisan(ctx, &json!({ "site_id": lv.id, "args": "migrate" }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("array of separate words"), "{err}");
        let err = site_artisan(ctx, &json!({ "site_id": lv.id, "args": ["php", "artisan", "migrate"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("leave out `php artisan`"), "{err}");
        let err = site_artisan(ctx, &json!({ "site_id": wp.id, "args": ["migrate"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not a Laravel site"), "{err}");
        let err = site_artisan(ctx, &json!({ "site_id": scratch.id, "args": ["migrate"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("scratch"), "{err}");
        let err = site_artisan(ctx, &json!({ "site_id": lv.id, "args": ["migrate"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not finished installing"), "{err}");

        std::fs::write(dir.join("artisan"), "#!/usr/bin/env php").unwrap();
        std::fs::write(dir.join("public/index.php"), "<?php").unwrap();
        // The shape a failed `create-project` leaves (ledger #557): artisan with
        // no dependencies cannot run, so it is still unfinished.
        let err = site_artisan(ctx, &json!({ "site_id": lv.id, "args": ["migrate"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("not finished installing") && err.contains("vendor/"), "{err}");
        std::fs::create_dir_all(dir.join("vendor")).unwrap();
        std::fs::write(dir.join("vendor/autoload.php"), "<?php").unwrap();
        let err = site_artisan(ctx, &json!({ "site_id": lv.id, "args": ["migrate", "--seed"] }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`run`"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// **`composer_link` refuses WordPress, scratch and a project with no
    /// composer.json on shape; asks for `run` BEFORE looking at the source;
    /// granted, refuses the home folder as a source exactly as a link would,
    /// reads the package name from the source's manifest, runs the app's
    /// Composer op, and says in the reply that the link is a symlink.**
    #[tokio::test]
    async fn composer_link_asks_run_before_reading_the_source_and_says_the_link_is_a_symlink() {
        let state = app_state();
        switch_on(&state);
        let ops = FakeOps::default();
        let acted = super::super::feed::ActedTarget::default();
        let ctx = UserCtx::new(&state, &ops, &ops, &ops, &ops, &ops, &ops, &ops, "claude-code");
        let root = std::env::temp_dir().join(format!("rexenv-composer-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("shop");
        let pkg = root.join("acme-widgets");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&pkg).unwrap();
        let wp = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "blog.rex", SiteOrigin::User);
        let mut lv = test_site("bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee", "shop.rex", SiteOrigin::User);
        lv.site_type = SiteType::Laravel;
        lv.path = project.display().to_string();
        {
            let conn = state.db.lock().unwrap();
            store::insert_site(&conn, &wp).unwrap();
            store::insert_site(&conn, &lv).unwrap();
        }
        let src = pkg.display().to_string();
        let err = composer_link(ctx, &json!({ "site_id": wp.id, "source": src }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("wp_plugin"), "{err}");
        let err = composer_link(ctx, &json!({ "site_id": lv.id, "source": src }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("no composer.json at its project root"), "{err}");
        std::fs::write(project.join("composer.json"), r#"{"name": "acme/shop"}"#).unwrap();

        // The source has no manifest yet — and is not looked at: the ask comes first.
        let err = composer_link(ctx, &json!({ "site_id": lv.id, "source": src }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("`run`"), "{err}");
        assert!(!err.contains("composer.json"), "the source was not read before the grant: {err}");

        dial(&state, crate::core::agent_access::AccessLevel::Full);
        let home = std::env::var("HOME").unwrap();
        let err = composer_link(ctx, &json!({ "site_id": lv.id, "source": home }), &acted).await.unwrap_err().to_string();
        assert!(!err.contains("`run`"), "granted — the refusal is the blast radius's: {err}");
        assert!(ops.calls.lock().unwrap().iter().all(|c| !c.starts_with("composer link")));
        let err = composer_link(ctx, &json!({ "site_id": lv.id, "source": src }), &acted).await.unwrap_err().to_string();
        assert!(err.contains("no composer.json"), "{err}");

        std::fs::write(pkg.join("composer.json"), r#"{"name": "acme/widgets"}"#).unwrap();
        let v = composer_link(ctx, &json!({ "site_id": lv.id, "source": src }), &acted).await.unwrap();
        assert_eq!(v["package"], "acme/widgets");
        assert_eq!(v["repositoryKey"], "acme-widgets");
        assert_eq!(v["symlinked"], true);
        assert_eq!(v["source"], "acme-widgets", "the source is named, never located");
        let detail = v["detail"].as_str().unwrap();
        assert!(detail.contains("SYMLINK") && detail.contains("lands in the checkout"), "{detail}");
        let canonical = std::fs::canonicalize(&pkg).unwrap();
        let calls = ops.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|c| c == &format!("composer link {} acme/widgets acme-widgets {}", lv.id, canonical.display())), "{calls:?}");
        let log = v["log"].as_array().unwrap();
        assert!(log.iter().any(|l| l.as_str().unwrap().contains("Symlinking from <source>")), "the source is labelled, never located: {log:?}");
        assert!(!v.to_string().contains(&canonical.display().to_string()), "no absolute source path anywhere in the reply: {v}");
        assert!(!v.to_string().contains(&home), "no absolute home path in the reply: {v}");
        assert_eq!(acted.take().as_deref(), Some(lv.id.as_str()));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
