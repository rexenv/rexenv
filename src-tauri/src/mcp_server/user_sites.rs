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
use crate::error::{Error, Result};
use crate::state::app::AppState;
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

/// The closed set of parity tools. Empty until P2's first tool lands — the
/// registry, the dispatch route, the sweep coverage and the disjointness guard
/// all exist first, so the first tool cannot arrive unranked or unswept.
pub fn registry() -> &'static [UserTool] {
    REGISTRY
}

static REGISTRY: &[UserTool] = &[];

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

/// What a parity handler can reach: app state, and — for any SITE or for the
/// stack — only through [`UserCtx::claim`], which yields a `Granted<S>` or a
/// refusal an agent can act on.
///
/// `Copy` (a single `&AppState`), like the other two contexts, so async handlers
/// take it by value.
#[derive(Clone, Copy)]
pub struct UserCtx<'a> {
    state: &'a AppState,
    /// The MCP client's self-reported name — the principal a grant is TO.
    client: &'a str,
}

// Until P2's first tool lands nothing calls these — the registry is empty by
// design (the structure arrives first). Lifted with the first handler.
#[allow(dead_code)]
impl<'a> UserCtx<'a> {
    pub fn new(state: &'a AppState, client: &'a str) -> Self {
        UserCtx { state, client }
    }

    pub fn client(&self) -> &'a str {
        self.client
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
        let body = &prod[ctx..];
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
}
