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

/// A read-only tool handler: `(ReadCtx, args) -> future of JSON`. `ReadCtx` is
/// `Copy` (a single `&AppState`), passed by value so the future can borrow it
/// for `'a` without a nested reference.
pub type ToolHandler = for<'a> fn(ReadCtx<'a>, &'a Value) -> ToolFuture<'a>;

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
        handler: list_sites,
    },
    ReadTool {
        name: "site_status",
        description: "Diagnose why a site is or isn't serving: whether rexenv's edge is up, \
                      whether this site's backend answers, and the site's HTTP response — reported \
                      as a specific verdict (serving / backend down / site error / edge down / \
                      edge blocked …) with what the probe can and cannot tell, and whether \
                      resolving it is the user's action in rexenv or something to check in the \
                      site's own code and logs. Takes `site_id` (from list_sites).",
        input_schema: site_id_param,
        sweep_args: |id| json!({ "site_id": id }),
        handler: site_status,
    },
    ReadTool {
        name: "tail_log",
        description: "Read the tail of a WordPress site's OWN debug log — its plugin/theme PHP \
                      errors and warnings — the most recent lines (tail-only, capped at 200, \
                      default 100). rexenv-issued login tokens and cookie headers are removed, \
                      but the log is otherwise the site's RAW output and is NOT sanitised: it can \
                      contain whatever the site's code logged (request data, config dumps, API \
                      responses). Only the WordPress debug log is exposed — shared server, edge, \
                      database, and access logs are not. Takes `site_id` and optional `lines`.",
        input_schema: site_id_lines_param,
        sweep_args: |id| json!({ "site_id": id }),
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

/// Look up a tool by name, or `None` if it is not registered.
pub fn find(name: &str) -> Option<&'static ReadTool> {
    registry().iter().find(|t| t.name == name)
}

/// The secret-leak sweep's plan: EVERY registered tool paired with the arguments
/// to exercise it, given a planted fixture site id. The sweep
/// (`examples/mcp_secret_sweep`, driven via `super::sweep_tool_outputs`)
/// enumerates this, so a new tool is covered by construction — it cannot be
/// registered without a `sweep_args` and thus without being swept.
pub fn sweep_plan(fixture_site_id: &str) -> Vec<(&'static ReadTool, Value)> {
    registry().iter().map(|t| (t, (t.sweep_args)(fixture_site_id))).collect()
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

fn list_sites<'a>(ctx: ReadCtx<'a>, _args: &'a Value) -> ToolFuture<'a> {
    Box::pin(async move {
        let sites = ctx.sites()?;
        let serving = ctx.serving_domains()?;
        let views: Vec<AgentSiteView> = sites
            .iter()
            .map(|s| AgentSiteView::from_site(s, serving.contains(&s.domain)))
            .collect();
        serde_json::to_value(views).map_err(|e| Error::Other(format!("serialising sites: {e}")))
    })
}

fn site_status<'a>(ctx: ReadCtx<'a>, args: &'a Value) -> ToolFuture<'a> {
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

fn tail_log<'a>(ctx: ReadCtx<'a>, args: &'a Value) -> ToolFuture<'a> {
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
        // handler stays free of state types.
        let raw = ctx.wp_debug_log_tail(&site, lines)?;
        let tail = AgentLogTail::from_lines(&site.id, &site.domain, "wp-debug", raw);
        serde_json::to_value(tail).map_err(|e| Error::Other(format!("serialising log tail: {e}")))
    })
}
