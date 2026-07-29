//! M1 read-only tools — the closed registry and its handlers.
//!
//! Every handler reaches app state ONLY through `ReadCtx` and returns an
//! `Agent*` view. It has no path to a manager, a command, or raw app state, and
//! the read-only import guard (`super::tests`) FAILS LOUDLY if a future edit
//! reaches for one. A tool that must mutate or execute is an M2 tool in a
//! different module (a different capability), not here.
//!
//! `list_sites` is the first tool, so its shape — an `Agent*` view, a conversion
//! that drops rather than redacts, a handler that only takes `(&ReadCtx, &args)`
//! — is the template every later read-only tool copies.

use super::readctx::ReadCtx;
use super::view::AgentSiteView;
use crate::error::{Error, Result};
use serde_json::{json, Value};

/// One read-only tool: its MCP name, human description, input schema, and a
/// handler mapping `(ReadCtx, args)` to a JSON result.
pub struct ReadTool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    pub handler: fn(&ReadCtx, &Value) -> Result<Value>,
}

/// The registry — the closed set of M1 tools. Adding a row is the only way a
/// tool comes to exist.
pub fn registry() -> &'static [ReadTool] {
    REGISTRY
}

static REGISTRY: &[ReadTool] = &[ReadTool {
    name: "list_sites",
    description: "List the local development sites rexenv manages. Each entry has the site's \
                  domain, name, type, PHP version, web server, and whether it is currently \
                  serving. Use the returned `id` or `domain` to refer to a site in later calls.",
    input_schema: no_params,
    handler: list_sites,
}];

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

/// A JSON Schema for a tool that takes no parameters.
fn no_params() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn list_sites(ctx: &ReadCtx, _args: &Value) -> Result<Value> {
    let sites = ctx.sites()?;
    let serving = ctx.serving_domains()?;
    let views: Vec<AgentSiteView> = sites
        .iter()
        .map(|s| AgentSiteView::from_site(s, serving.contains(&s.domain)))
        .collect();
    serde_json::to_value(views).map_err(|e| Error::Other(format!("serialising sites: {e}")))
}
