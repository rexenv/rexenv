//! M1 read-only tools — the closed registry and its handlers.
//!
//! Every handler reaches app state ONLY through `ReadCtx` and returns an
//! `Agent*` view. It has no path to a manager, a command, or raw app state, and
//! the read-only import guard (`super::tests`) FAILS LOUDLY if a future edit
//! reaches for one. A tool that must mutate or execute is an M2 tool in a
//! different module (a different capability), not here.
//!
//! Handlers are async (a diagnostic tool probes over the network), so a handler
//! returns a boxed future. Two fields make the safety nets mandatory rather than
//! remembered: `sweep_args` (so a tool CANNOT be registered without the
//! secret-leak sweep being able to exercise it — see `examples/mcp_secret_sweep`)
//! and, of course, presence in `REGISTRY` (the only way a tool exists).

use super::readctx::ReadCtx;
use super::view::{AgentLogTail, AgentSiteStatus, AgentSiteView};
use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

/// `tail_log` line bounds — a scope limit, not a filter: the less of a log an
/// agent can pull, the less an unknown-shaped secret in it matters.
const DEFAULT_LOG_LINES: usize = 100;
const MAX_LOG_LINES: usize = 200;

/// A tool's async result, boxed so the registry can hold handlers uniformly.
/// `Send` so the session task stays `Send`; `'a` borrows the `ReadCtx`.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;

/// A read-only tool handler: `(ReadCtx, args, acted) -> future of JSON`.
/// `ReadCtx` is `Copy` (a single `&AppState`), passed by value so the future can
/// borrow it for `'a` without a nested reference.
///
/// `acted` is the out-parameter a handler uses to name the site REXENV acted on
/// (`ActedTarget`), for the feed. **M1's handlers never touch it** — they act on
/// nothing, so their feed target stays the site the agent named. It is in the
/// signature from here because an M2 create has no `site_id` argument to record
/// and must not be able to reach the feed through the tool's RESULT (that would
/// be a channel for agent-influenced content into a deliberately typed record).
pub type ToolHandler =
    for<'a> fn(ReadCtx<'a>, &'a Value, &'a super::feed::ActedTarget) -> ToolFuture<'a>;

/// One read-only tool: its MCP name, description, input schema, the arguments
/// the secret-leak sweep exercises it with, and the handler.
pub struct ReadTool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    /// The arguments the secret-leak sweep calls this tool with, given the id of
    /// a planted fixture site. REQUIRED, so a new tool cannot be registered
    /// without the sweep covering its output.
    pub sweep_args: fn(site_id: &str) -> Value,
    /// What this call was ABOUT, for the feed (v30) — or `None` when the tool's
    /// name and target already describe it, which is true of every M1 tool.
    /// REQUIRED like `sweep_args`: a tool must SAY that its name is enough
    /// rather than be silently assumed to have nothing to add.
    pub summarise: fn(&Value) -> Option<String>,
    pub handler: ToolHandler,
}

/// The registry — the closed set of M1 tools. Adding a row is the only way a
/// tool comes to exist.
pub fn registry() -> &'static [ReadTool] {
    REGISTRY
}

static REGISTRY: &[ReadTool] = &[
    ReadTool {
        name: "list_sites",
        description: "List the local development sites rexenv manages. Each entry has the site's \
                      domain, name, type, PHP version, web server, and whether it is currently \
                      serving. Use the returned `id` or `domain` to refer to a site in later calls.",
        input_schema: no_params,
        sweep_args: |_id| json!({}),
        summarise: |_| None,
        handler: list_sites,
    },
    ReadTool {
        name: "site_status",
        description: "Diagnose why a site is or isn't serving, from the stack's OWN state — whether \
                      rexenv's edge is up and whether this site's backend is running — reported as a \
                      specific verdict (serving / backend-down / setup-incomplete / edge-blocked / \
                      edge-down) and whether resolving it is the user's action in rexenv. It does \
                      NOT request the site (so it never runs the site's code); whether the site's \
                      own code renders correctly is checked with tail_log. Takes `site_id`.",
        input_schema: site_id_param,
        sweep_args: |id| json!({ "site_id": id }),
        summarise: |_| None,
        handler: site_status,
    },
    ReadTool {
        name: "site_info",
        description: "Everything rexenv records about one site, in one read: the list_sites view \
                      (type, PHP, server, owner, extra domains, whether setup finished, whether the \
                      folder is the user's own), the serving verdict site_status gives, the \
                      database engine, when it was made, its HTTPS certificate (validity, days \
                      left, names) and — for a scratch site — the packages the agent added and \
                      when each was last synced. Runs nothing. Takes `site_id`.",
        input_schema: site_id_param,
        sweep_args: |id| json!({ "site_id": id }),
        summarise: |_| None,
        handler: site_info,
    },
    ReadTool {
        name: "site_inspect_folder",
        description: "Look at a folder on this machine the way the New Site dialog does before \
                      linking it: what kind of project it holds (WordPress, Laravel, a plain PHP \
                      site…), which subfolder would be served, and whether it already holds an \
                      installed app. Creates and runs nothing; refuses the same folders the dialog \
                      refuses (the home folder, Desktop/Documents/Downloads, a whole volume, \
                      rexenv's own data, a folder another site already uses) with the dialog's own \
                      reason. Takes `path` (absolute).",
        input_schema: || json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "Absolute path to the folder." } },
            "required": ["path"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "path": "/tmp/rexenv-sweep-probe" }),
        summarise: |_| None,
        handler: site_inspect_folder,
    },
    ReadTool {
        name: "stack_status",
        description: "The whole stack as rexenv sees it, in one read: every service (edge, web \
                      server, PHP pools, databases, mail) with whether it is running and its port; \
                      whether rexenv's own edge and DNS are answering; whether the resolver file \
                      and the local CA are in place; PHP versions installed and the default; the \
                      default TLD; whether the `rex` CLI is on PATH. Runs nothing, requests no \
                      site. When something is down, the user's Start button in rexenv (or the \
                      `stack` tool under their `system` permission) is the way forward.",
        input_schema: no_params,
        sweep_args: |_id| json!({}),
        summarise: |_| None,
        handler: stack_status,
    },
    ReadTool {
        name: "settings_get",
        description: "Read one rexenv setting by key — the same allow-list `rex config get` uses: \
                      preferences like `preferred_browser`, `preferred_editor`, \
                      `start_services_on_launch`, `default_tld`, the pinned engine versions. Keys \
                      that hold signed update state or the MCP switches are refused with the \
                      reason. Takes `key`. Writing is the `settings` tool, under `system`.",
        input_schema: || json!({
            "type": "object",
            "properties": { "key": { "type": "string" } },
            "required": ["key"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "key": "preferred_browser" }),
        summarise: |args| args.get("key").and_then(Value::as_str).map(str::to_string),
        handler: settings_get,
    },
    ReadTool {
        name: "php_settings",
        description: "The php.ini overrides rexenv applies to one PHP version's pool — every key it \
                      edits (memory limit, upload size, execution time, …) with the stored value, \
                      if any, and the default. Takes `minor` (e.g. `8.3`). Changing them is the \
                      `php` tool's `settings_set`.",
        input_schema: || json!({
            "type": "object",
            "properties": { "minor": { "type": "string" } },
            "required": ["minor"],
            "additionalProperties": false
        }),
        sweep_args: |_id| json!({ "minor": "8.3" }),
        summarise: |_| None,
        handler: php_settings,
    },
    ReadTool {
        name: "wp_org_search",
        description: "Search the WordPress.org directory for plugins or themes — slug, name, \
                      author, rating, active installs. A public network read; no site involved \
                      and nothing installed. Takes `kind` (plugins / themes) and `query`.",
        input_schema: || json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["plugins", "themes"] },
                "query": { "type": "string" }
            },
            "required": ["kind", "query"],
            "additionalProperties": false
        }),
        // Not exercised against the network by the sweep: an empty query is
        // refused before any request, and the refusal is what is swept.
        sweep_args: |_id| json!({ "kind": "plugins", "query": "" }),
        summarise: |args| args.get("kind").and_then(Value::as_str).map(str::to_string),
        handler: wp_org_search,
    },
    ReadTool {
        name: "tail_log",
        description: "Read the tail of a WordPress site's OWN debug log — its plugin/theme PHP \
                      errors and warnings — the most recent lines (tail-only, capped at 200, \
                      default 100). rexenv-issued login tokens, cookie headers and the absolute \
                      paths rexenv knows (shown as labels like <docroot>) are removed, but the log \
                      is otherwise the site's RAW output and is NOT sanitised: it can contain \
                      whatever the site's code logged, including paths rexenv doesn't know \
                      (request data, config dumps, API responses). Only the WordPress debug log is \
                      exposed — shared server, edge, \
                      database, and access logs are not. Takes `site_id` and optional `lines`.",
        input_schema: site_id_lines_param,
        sweep_args: |id| json!({ "site_id": id }),
        summarise: |_| None,
        handler: tail_log,
    },
];

/// The `tools/list` result — the registry as MCP tool descriptors.
pub fn tools_list_result() -> Value {
    let tools: Vec<Value> = registry()
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": (t.input_schema)(),
            })
        })
        .collect();
    json!({ "tools": tools })
}

fn no_params() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn site_id_param() -> Value {
    json!({
        "type": "object",
        "properties": { "site_id": { "type": "string", "description": "The site's id (from list_sites)." } },
        "required": ["site_id"],
        "additionalProperties": false
    })
}

fn site_id_lines_param() -> Value {
    json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The site's id (from list_sites)." },
            "lines": { "type": "integer", "description": "How many recent lines (max 200; default 100)." }
        },
        "required": ["site_id"],
        "additionalProperties": false
    })
}

fn list_sites<'a>(
    ctx: ReadCtx<'a>,
    _args: &'a Value,
    // M1 acts on nothing — see `ToolHandler`.
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let sites = ctx.sites()?;
        let serving = ctx.serving_domains()?;
        let mut aliases = ctx.aliases_by_site()?;
        let views: Vec<AgentSiteView> = sites
            .iter()
            .map(|s| AgentSiteView::from_site(s, serving.contains(&s.domain), aliases.remove(&s.id).unwrap_or_default()))
            .collect();
        serde_json::to_value(views).map_err(|e| Error::Other(format!("serialising sites: {e}")))
    })
}

fn site_status<'a>(
    ctx: ReadCtx<'a>,
    args: &'a Value,
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("site_status needs a `site_id` string".into()))?;
        let site = ctx
            .site_by_id(id)?
            .ok_or_else(|| Error::Other(format!("no site with id `{id}`")))?;
        let signals = ctx.probe_serving(&site).await;
        let status = AgentSiteStatus::from_signals(&site, &signals);
        serde_json::to_value(status).map_err(|e| Error::Other(format!("serialising status: {e}")))
    })
}

fn site_info<'a>(
    ctx: ReadCtx<'a>,
    args: &'a Value,
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("site_info needs a `site_id` string".into()))?;
        let site = ctx
            .site_by_id(id)?
            .ok_or_else(|| Error::Other(format!("no site with id `{id}`")))?;
        let aliases = ctx.aliases_by_site()?.remove(&site.id).unwrap_or_default();
        let signals = ctx.probe_serving(&site).await;
        let serving = signals.serving_manager && signals.edge_answers_ours;
        let view = AgentSiteView::from_site(&site, serving, aliases);
        let status = AgentSiteStatus::from_signals(&site, &signals);
        // The certificate WITHOUT its directory — a path into app-data, which
        // the agent has no use for and the view rule keeps out.
        let cert = ctx.cert_info(&site)?.map(|c| {
            json!({ "notBefore": c.not_before, "notAfter": c.not_after, "daysLeft": c.days_left, "sans": c.sans })
        });
        // Packages: slug, kind and the sync time. NOT the recorded source path —
        // the agent supplied it and the user's card shows it; a tool reply is a
        // third place for a path into someone's project to travel.
        let packages: Vec<Value> = ctx
            .scratch_packages_of(&site)?
            .iter()
            .map(|p| json!({ "slug": p.slug, "kind": p.kind, "syncedAt": p.synced_at }))
            .collect();
        let mut value = serde_json::to_value(view).map_err(|e| Error::Other(format!("serialising site: {e}")))?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("status".into(), serde_json::to_value(status).map_err(|e| Error::Other(e.to_string()))?);
            obj.insert("dbEngine".into(), json!(site.db_engine));
            obj.insert("createdAt".into(), json!(site.created_at));
            obj.insert("certificate".into(), cert.unwrap_or(Value::Null));
            obj.insert("packages".into(), Value::Array(packages));
        }
        Ok(value)
    })
}

fn site_inspect_folder<'a>(
    ctx: ReadCtx<'a>,
    args: &'a Value,
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| Error::Other("site_inspect_folder needs an absolute `path`".into()))?;
        let found = ctx.inspect_folder(path)?;
        Ok(json!({
            "type": found.site_type,
            "label": found.label,
            "docrootRel": found.docroot_rel,
            "existingInstall": found.existing_install,
            "customValetDriver": found.has_custom_valet_driver,
            "note": if found.docroot_rel.is_empty() {
                "The folder itself would be served.".to_string()
            } else {
                format!("`{}` under this folder would be served, not the folder itself.", found.docroot_rel)
            },
        }))
    })
}

fn stack_status<'a>(
    ctx: ReadCtx<'a>,
    _args: &'a Value,
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let snap = ctx.stack_snapshot()?;
        // The two wire facts `site_status` probes, once, for the stack as a whole.
        let tcp_443_open = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, 443)),
        )
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false);
        let mut value = serde_json::to_value(snap).map_err(|e| Error::Other(format!("serialising the stack: {e}")))?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("tcp443Open".into(), json!(tcp_443_open));
        }
        Ok(value)
    })
}

fn settings_get<'a>(
    ctx: ReadCtx<'a>,
    args: &'a Value,
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let key = args.get("key").and_then(Value::as_str).map(str::trim).filter(|k| !k.is_empty())
            .ok_or_else(|| Error::Other("settings_get needs a `key`".into()))?;
        let value = ctx.setting(key)?;
        Ok(json!({ "key": key, "value": value, "set": value.is_some() }))
    })
}

fn php_settings<'a>(
    ctx: ReadCtx<'a>,
    args: &'a Value,
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let minor = args.get("minor").and_then(Value::as_str).map(str::trim).filter(|k| !k.is_empty())
            .ok_or_else(|| Error::Other("php_settings needs a `minor` like `8.3`".into()))?;
        let rows: Vec<Value> = ctx.php_settings(minor)?
            .into_iter()
            .map(|(key, value, default)| json!({ "key": key, "value": value, "default": default }))
            .collect();
        Ok(json!({ "minor": minor, "settings": rows }))
    })
}

fn wp_org_search<'a>(
    ctx: ReadCtx<'a>,
    args: &'a Value,
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let kind = args.get("kind").and_then(Value::as_str).unwrap_or("");
        if !matches!(kind, "plugins" | "themes") {
            return Err(Error::Other("wp_org_search needs `kind`: `plugins` or `themes`".into()));
        }
        let query = args.get("query").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if query.is_empty() {
            return Err(Error::Other("wp_org_search needs a non-empty `query`".into()));
        }
        ctx.wporg_search(kind, query).await
    })
}

fn tail_log<'a>(
    ctx: ReadCtx<'a>,
    args: &'a Value,
    _acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("tail_log needs a `site_id` string".into()))?;
        let lines = args
            .get("lines")
            .and_then(Value::as_u64)
            .map_or(DEFAULT_LOG_LINES, |n| (n as usize).min(MAX_LOG_LINES));
        let site = ctx
            .site_by_id(id)?
            .ok_or_else(|| Error::Other(format!("no site with id `{id}`")))?;
        // The WordPress-only gate lives in ReadCtx (the trusted bridge), so this
        // handler stays free of state types. A non-WP site is a normal empty
        // result, not an error (so it isn't a misleading "concerning" feed row).
        let tail = match ctx.wp_debug_log_tail(&site, lines)? {
            Some(raw) => AgentLogTail::from_lines(&site.id, &site.domain, &ctx.known_paths(&site), "wp-debug", raw),
            None => AgentLogTail::none_for_non_wordpress(&site.id, &site.domain),
        };
        serde_json::to_value(tail).map_err(|e| Error::Other(format!("serialising log tail: {e}")))
    })
}
