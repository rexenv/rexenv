//! M2 scratch tools — the executing registry, and a DIFFERENT capability from
//! M1's read-only one.
//!
//! **Why a separate module rather than more rows in `tools.rs`.** M1's
//! guarantee (#199) is structural because of where its handlers live: the
//! read-only guard scans `tools.rs` and `readctx.rs` for any reach toward a
//! manager, a command, or a syscall. A mutating tool added there would trip that
//! guard — correctly. So M2's tools live here, with their own context type, and
//! M1's guarantee is untouched by their arrival rather than quietly widened to
//! accommodate them. The two registries are **disjoint by test**
//! (`the_two_registries_are_disjoint_and_say_which_side_a_tool_belongs_on`): one
//! name, one capability, no shadowing.
//!
//! **The capability, precisely.** An M1 handler receives a `ReadCtx`, which has
//! no mutating method. A scratch handler receives a [`ScratchCtx`], whose door
//! to any site is [`ScratchCtx::claim`] — `core::scratch::claim`, which reads
//! the row and proves `origin = 'agent'` (#208). So the difference between the
//! tiers is not "these ones are trusted": it is that a scratch handler can only
//! reach sites the agent owns, and cannot obtain a handle to any other.
//!
//! The registry is EMPTY until the create tool lands (M2a task 8). That is
//! deliberate — the plumbing, the disjointness guard, and the sweep union land
//! first, so the first executing tool arrives into a structure that already
//! refuses to let it be registered untested or unswept.

use crate::core::scratch::ScratchSite;
use crate::error::Result;
use crate::state::app::AppState;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// A scratch tool's async result — same shape as the read side's.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;

/// A scratch tool handler: `(ScratchCtx, args, acted) -> future of JSON`.
/// `acted` records the site rexenv acted on for the feed (#206) — an
/// out-parameter, so a create that inserts a row and then fails still names it.
pub type ToolHandler = for<'a> fn(
    ScratchCtx<'a>,
    &'a Value,
    &'a super::feed::ActedTarget,
) -> ToolFuture<'a>;

/// One executing tool. Deliberately the SAME field discipline as `ReadTool`,
/// including the required `sweep_args`: a tool cannot be registered without the
/// secret-leak sweep being able to exercise it, on this side either.
pub struct ScratchTool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    /// The arguments the secret-leak sweep calls this tool with. REQUIRED — the
    /// sweep enumerates the registry, so a new tool is covered by construction.
    pub sweep_args: fn(site_id: &str) -> Value,
    pub handler: ToolHandler,
}

/// The closed set of M2 scratch tools. Empty until task 8.
pub fn registry() -> &'static [ScratchTool] {
    REGISTRY
}

static REGISTRY: &[ScratchTool] = &[];

/// Look up a scratch tool by name.
pub fn find(name: &str) -> Option<&'static ScratchTool> {
    registry().iter().find(|t| t.name == name)
}

/// Every scratch tool paired with the arguments the sweep exercises it with —
/// the mirror of `tools::sweep_plan`, so the sweep covers BOTH registries. A
/// sweep that walked only M1's would silently narrow "every registered tool's
/// output is swept" to "every read tool's" the moment this list grows.
pub fn sweep_plan(fixture_site_id: &str) -> Vec<(&'static ScratchTool, Value)> {
    registry().iter().map(|t| (t, (t.sweep_args)(fixture_site_id))).collect()
}

/// What an executing handler can reach: app state, and — for any SITE — only
/// through [`ScratchCtx::claim`].
///
/// `Copy` (a single `&AppState`), like `ReadCtx`, so async handlers take it by
/// value.
#[derive(Clone, Copy)]
pub struct ScratchCtx<'a> {
    state: &'a AppState,
}

impl<'a> ScratchCtx<'a> {
    pub fn new(state: &'a AppState) -> Self {
        ScratchCtx { state }
    }

    /// Prove a site is the agent's, or refuse with the policy statement
    /// (`core::scratch::claim`, #208). **The only way a scratch handler obtains
    /// a site**: there is no `site_by_id` here, so "I'll just read the row and
    /// check it myself" is not an available shortcut.
    pub fn claim(&self, id: &str) -> Result<ScratchSite> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| crate::error::Error::Other("the app database lock is poisoned".into()))?;
        crate::core::scratch::claim(&conn, id)
    }
}

/// This registry's tools as MCP descriptors, for the union `tools/list`.
pub fn tools_list_descriptors() -> Value {
    Value::Array(
        registry()
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": (t.input_schema)(),
                })
            })
            .collect(),
    )
}
