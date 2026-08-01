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
use crate::core::sites::Ownership;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{NewSite, Site};
use serde::Serialize;
use serde_json::{json, Value};
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

static REGISTRY: &[ScratchTool] = &[ScratchTool {
    name: "scratch_create_site",
    description: "Create a fresh, disposable WordPress site for testing — a \"scratch\" site the \
                  agent owns and can delete. Takes a single-word `name` (rexenv adds \
                  `.scratch.<tld>` itself) and an optional `php` version. Scratch sites are \
                  capped, and expire on their own once nothing has used them for a while. This \
                  can take a minute or two: WordPress is downloaded and installed. It never \
                  touches the user's own sites.",
    input_schema: create_params,
    sweep_args: |_id| json!({ "name": "sweep-probe" }),
    handler: create_site,
}, ScratchTool {
    name: "scratch_delete_site",
    description: "Delete a scratch site the agent created — its files, its database and its \
                  configuration. Only works on scratch sites: the user's own sites are refused. \
                  Takes `site_id`.",
    input_schema: || json!({
        "type": "object",
        "properties": { "site_id": { "type": "string", "description": "The scratch site's id." } },
        "required": ["site_id"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id }),
    handler: delete_site,
}];

fn create_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "description": "A single word, no dots — the site becomes `<name>.scratch.<tld>`."
            },
            "php": {
                "type": "string",
                "description": "PHP minor version, e.g. `8.3`. Defaults to rexenv's default."
            }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

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
    /// The one path to the site-creation job. A trait object because the
    /// provision path is generic over `tauri::Runtime` and a `static` registry
    /// of fn pointers cannot be: erasing the runtime here keeps ONE create path
    /// (the same one the app and the CLI use) instead of a second implementation
    /// for agents.
    creator: &'a dyn SiteCreator,
    /// The MCP client's self-reported name, recorded on the row it creates.
    client: &'a str,
    /// Deleting goes through the app's own full delete path (tunnel stop, DB
    /// drop, teardown, reload) — same runtime erasure, same one-brain rule.
    deleter: &'a dyn SiteDeleter,
}

/// Deleting a site through the app's own delete path.
pub trait SiteDeleter: Send + Sync {
    fn delete<'a>(&'a self, id: String) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}

/// Creating a site through the app's own provision job.
///
/// Implemented over the `AppHandle` in `mcp_server`, so this module never
/// re-implements provisioning — the one-brain rule, kept across the runtime
/// erasure that the static registry forces.
pub trait SiteCreator: Send + Sync {
    fn create<'a>(
        &'a self,
        new: NewSite,
        ownership: Ownership,
    ) -> Pin<
        Box<
            dyn Future<Output = std::result::Result<Site, crate::commands::sites::CreateFailure>>
                + Send
                + 'a,
        >,
    >;
}

impl<'a> ScratchCtx<'a> {
    pub fn new(
        state: &'a AppState,
        creator: &'a dyn SiteCreator,
        deleter: &'a dyn SiteDeleter,
        client: &'a str,
    ) -> Self {
        ScratchCtx { state, creator, deleter, client }
    }

    /// The refusals a create must clear BEFORE anything is built, in the order
    /// that gives the most actionable answer first. Returns the pieces the
    /// create needs: the full domain, the PHP version, and the client name to
    /// record on the row.
    fn plan_create(&self, name: &str, php: Option<String>) -> Result<(String, String, String)> {
        let conn = self.db()?;
        // The ceiling first: a name that would be fine is not worth validating
        // if the pool is full, and the cap refusal is the one that hands back
        // the sites it may delete.
        crate::core::scratch::ensure_capacity(&conn)?;
        let tld = crate::core::sites::default_tld(&conn)?;
        let domain = crate::core::scratch::scratch_domain(name, &tld)?;
        if crate::state::store::domain_exists(&conn, &domain)? {
            return Err(Error::Other(format!(
                "`{domain}` already exists. Pick a different name, or use the site that is \
                 already there (list_sites shows it)."
            )));
        }
        let php_version = match php {
            Some(v) => v,
            None => crate::state::store::list_php_versions(&conn)?
                .into_iter()
                .find(|v| v.is_default)
                .map(|v| v.minor)
                .ok_or_else(|| {
                    Error::Other(
                        "rexenv has no default PHP version yet — the person you're working with \
                         needs to finish rexenv's setup before sites can be created."
                            .into(),
                    )
                })?,
        };
        Ok((domain, php_version, self.client.to_string()))
    }

    /// Run the create through the SAME provision path the app and the CLI use,
    /// and turn its two failure shapes into answers an agent can act on.
    ///
    /// The half-built case is the one that matters: a failure after the row
    /// exists leaves a real site in the user's list. The reply NAMES it (and
    /// records it in the feed via `acted`), because "nothing happened" would be
    /// false and would send the agent to create another one.
    async fn create(
        &self,
        new: NewSite,
        ownership: Ownership,
        acted: &super::feed::ActedTarget,
    ) -> Result<Site> {
        let domain = new.domain.clone();
        match self.creator.create(new, ownership).await {
            Ok(site) => {
                acted.set(&site);
                Ok(site)
            }
            Err(failure) => {
                let conn = self.db().ok();
                Err(translate_create_failure(&domain, failure, conn.as_deref(), acted))
            }
        }
    }

    /// Delete a proven scratch site, re-asserting the recorded fact at the
    /// destructive write.
    ///
    /// The witness is a snapshot: the user can press Keep between the claim and
    /// this call, and then the site is THEIRS. So the row deletion carries
    /// `AND origin = 'agent'` in its own `WHERE`, and a miss is reported
    /// honestly — "no longer the agent's" — rather than passing as a silent
    /// no-op that would leave the agent believing it deleted something.
    ///
    /// The re-assert happens BEFORE anything destructive, deliberately: putting
    /// it at the last write would mean discovering the adoption after the
    /// database was already dropped. The residual window between this check and
    /// the resource teardown is milliseconds and cannot be closed without a
    /// transaction spanning MySQL — stated, not pretended away.
    async fn delete(&self, scratch: &ScratchSite) -> Result<()> {
        {
            let conn = self.db()?;
            if !crate::core::scratch::still_the_agents(&conn, scratch.id())? {
                return Err(Error::Other(format!(
                    "`{}` is no longer a scratch site — the person you're working with kept it, \
                     so it is theirs now and agent tools cannot delete it.",
                    scratch.domain()
                )));
            }
        }
        self.deleter.delete(scratch.id().to_string()).await
    }

    /// The created site's serving status, in M1's vocabulary.
    async fn status_of(&self, site: &Site) -> super::view::AgentSiteStatus {
        let read = super::readctx::ReadCtx::new(self.state);
        let signals = read.probe_serving(site).await;
        super::view::AgentSiteStatus::from_signals(site, &signals)
    }

    fn db(&self) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
        self.state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))
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

/// Turn a create failure into an answer an agent can act on — and, when a site
/// EXISTS, make the reply name it.
///
/// The two shapes are genuinely different advice. Nothing created: the refusal
/// stands alone and the agent should fix its input. Created-then-failed: a real
/// site is sitting in the user's list marked "setup incomplete", so reporting a
/// bare failure would be false AND would send the agent to create another one —
/// filling the pool with half-built sites nobody meant to make. It also records
/// the site for the feed (#206), for the same reason.
///
/// The app/CLI failure text points at a local log file; that path never reaches
/// a tool reply. The agent gets the tools it can actually use instead.
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
         incomplete\", where they can retry or remove it — retrying needs them, not you. You can \
         read what went wrong with tail_log, or clear it away with scratch_delete_site and try \
         again."
    ))
}

/// What a scratch site is, as an agent sees it.
///
/// Reuses M1's `verdict`/`resolution` vocabulary (`AgentSiteStatus`) rather than
/// inventing a second one, so "it isn't serving, and that is the USER's action
/// in rexenv" reads identically whether the agent asked `site_status` or just
/// created the site. Carries no docroot, no database name, no local path.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentScratchSite {
    pub id: String,
    pub domain: String,
    pub url: String,
    /// When it will expire if nothing uses it. Using it pushes this out.
    pub expires_at: Option<String>,
    /// The M1 status shape — serving/verdict/detail/resolution.
    #[serde(flatten)]
    pub status: super::view::AgentSiteStatus,
}

/// Create the site, then answer with WHAT HAPPENED — including the cases where
/// the site exists but is not yet usable.
fn create_site<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("scratch_create_site needs a `name` (a single word).".into()))?;
        let php = args.get("php").and_then(Value::as_str).map(str::to_string);

        // Refusals FIRST, cheapest and most actionable first, each naming a way
        // forward (core::scratch): the pool ceiling, then the name's shape.
        let (domain, php_version, client) = ctx.plan_create(name, php)?;

        let new = NewSite {
            name: name.trim().to_string(),
            domain,
            site_type: crate::state::models::SiteType::Wordpress,
            php_version,
            web_server: crate::state::models::WebServer::Nginx,
            path: String::new(), // never a caller path: an agent cannot link a folder
            db_engine: crate::state::models::SiteDbEngine::Mysql,
        };
        let site = ctx.create(new, Ownership::Agent { client, ttl_hours: crate::core::sites::SCRATCH_TTL_HOURS }, acted).await?;
        let status = ctx.status_of(&site).await;
        let view = AgentScratchSite {
            url: format!("https://{}", site.domain),
            id: site.id.clone(),
            domain: site.domain.clone(),
            expires_at: site.expires_at.clone(),
            status,
        };
        serde_json::to_value(view).map_err(|e| Error::Other(format!("serialising the site: {e}")))
    })
}

/// Delete a scratch site — gated by the WITNESS, not by its name.
///
/// `claim` is what decides (#208): it reads the recorded `origin`, so a site the
/// user hand-named `*.scratch.rex` is refused with the ownership policy
/// statement, not with a "not found" that would send the agent looking again.
fn delete_site<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("scratch_delete_site needs a `site_id`.".into()))?;
        // THE gate. Not a suffix check, not a name check — the recorded fact.
        let scratch = ctx.claim(id)?;
        acted.set(scratch.site());
        let domain = scratch.domain().to_string();
        ctx.delete(&scratch).await?;
        Ok(json!({
            "deleted": true,
            "domain": domain,
            "detail": format!("`{domain}` and its database are gone."),
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::{test_site, SiteOrigin};

    /// A creator that fails the way provisioning does — AFTER the row exists.
    struct HalfBuilt {
        site_id: String,
    }
    impl SiteCreator for HalfBuilt {
        fn create<'a>(
            &'a self,
            _new: NewSite,
            _ownership: Ownership,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<Site, crate::commands::sites::CreateFailure>> + Send + 'a>>
        {
            let id = self.site_id.clone();
            Box::pin(async move {
                Err(crate::commands::sites::CreateFailure {
                    site_id: Some(id),
                    error: Error::Other("site create failed at \"core_download\"".into()),
                })
            })
        }
    }

    /// A creator that fails BEFORE anything exists (the prepare phase).
    struct NothingCreated;
    impl SiteCreator for NothingCreated {
        fn create<'a>(
            &'a self,
            _new: NewSite,
            _ownership: Ownership,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<Site, crate::commands::sites::CreateFailure>> + Send + 'a>>
        {
            Box::pin(async move {
                Err(crate::commands::sites::CreateFailure {
                    site_id: None,
                    error: Error::Other("`probe.scratch.rex` already exists. Pick a different name.".into()),
                })
            })
        }
    }

    #[test]
    fn the_created_view_speaks_m1s_verdict_and_resolution_vocabulary() {
        // A create that lands while the stack is stopped is a SUCCESS whose site
        // is not serving — and the agent must learn that fixing it is the user's
        // action in rexenv, not a tool it should go hunting for. Reusing M1's
        // shape means "not serving, and here is whose move it is" reads
        // identically whether it asked site_status or just created the site.
        let site = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        let signals = super::super::view::ServingSignals {
            edge_answers_ours: false,
            tcp_443_open: false,
            serving_manager: false,
        };
        let view = AgentScratchSite {
            url: format!("https://{}", site.domain),
            id: site.id.clone(),
            domain: site.domain.clone(),
            expires_at: Some("2026-08-03 09:00:00".into()),
            status: super::super::view::AgentSiteStatus::from_signals(&site, &signals),
        };
        let v = serde_json::to_value(&view).unwrap();
        assert_eq!(v["serving"], false);
        assert_eq!(v["verdict"], "edge-down", "the specific verdict, not 'not serving': {v}");
        assert_eq!(v["resolution"], "user-action-in-rexenv", "whose move it is: {v}");
        assert!(v["detail"].as_str().unwrap().len() > 10);
        assert_eq!(v["url"], "https://probe.scratch.rex");
        assert!(v["expiresAt"].is_string(), "the agent can see when it dies: {v}");
        // No internals: the docroot and database name never leave rexenv.
        let text = v.to_string();
        assert!(!text.contains("/Users/") && !text.contains("wp_probe"), "leak: {text}");
    }

    #[tokio::test]
    async fn a_create_that_half_builds_names_the_site_rather_than_reading_as_nothing_happened() {
        // The ActedTarget rule surfacing in the tool's OWN output: the site
        // exists, the user can see it, so the reply must name it — otherwise the
        // agent reports failure and creates another one, and the pool fills with
        // half-built sites nobody meant to make.
        let creator = HalfBuilt { site_id: "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24".into() };
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut row = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        row.provisioned = false;
        row.expires_at = Some("2099-01-01 00:00:00".into());
        crate::state::store::insert_site(&conn, &row).unwrap();

        let acted = super::super::feed::ActedTarget::default();
        // Exercise the failure translation directly (constructing a full
        // AppState here would add nothing the assertion needs).
        let err = translate_create_failure(
            "probe.scratch.rex",
            crate::commands::sites::CreateFailure {
                site_id: Some(row.id.clone()),
                error: Error::Other("failed at core_download".into()),
            },
            Some(&conn),
            &acted,
        );
        let msg = err.to_string();
        assert!(msg.contains("probe.scratch.rex"), "names the site: {msg}");
        assert!(msg.contains(&row.id), "gives the id the agent can act on: {msg}");
        assert!(msg.contains("was created"), "does not read as nothing happened: {msg}");
        assert!(msg.contains("retry") || msg.contains("retrying"), "says retry exists: {msg}");
        assert!(msg.contains("them, not you"), "and that retrying is the USER's: {msg}");
        assert!(msg.contains("scratch_delete_site"), "and what the agent CAN do: {msg}");
        assert!(!msg.contains("full log:"), "no local log path in a tool reply: {msg}");
        // And the feed names it too — the row exists, so the record says so.
        assert_eq!(acted.take().as_deref(), Some(row.id.as_str()));
        let _ = creator;
        let _ = NothingCreated;
    }

    #[test]
    fn a_create_that_built_nothing_says_so_without_naming_a_site() {
        let acted = super::super::feed::ActedTarget::default();
        let err = translate_create_failure(
            "probe.scratch.rex",
            crate::commands::sites::CreateFailure {
                site_id: None,
                error: Error::Other("`probe.scratch.rex` already exists. Pick a different name.".into()),
            },
            None,
            &acted,
        );
        assert!(err.to_string().contains("Pick a different name"), "the refusal stands alone");
        assert!(!err.to_string().contains("was created"), "nothing was");
        assert_eq!(acted.take(), None, "and the feed names no site");
    }
    #[test]
    fn deleting_is_gated_by_the_witness_not_by_the_name() {
        // The gate is the RECORDED origin. A site the user hand-named
        // `*.scratch.rex` must read as "that one is yours" — the ownership
        // policy statement — and never as "not found", which would send the
        // agent looking for it again instead of understanding the rule.
        let conn = crate::state::db::open_in_memory().unwrap();
        let theirs = test_site("a1b2c3d4-1111-4222-8333-444455556666", "mine.scratch.rex", SiteOrigin::User);
        crate::state::store::insert_site(&conn, &theirs).unwrap();
        let err = crate::core::scratch::claim(&conn, &theirs.id).unwrap_err().to_string();
        assert!(err.contains("your own sites"), "the ownership refusal: {err}");
        assert!(!err.contains("no site with id"), "NOT a not-found: {err}");
        assert!(err.contains("mine.scratch.rex"), "names it: {err}");
    }

    #[test]
    fn a_site_kept_between_the_claim_and_the_delete_is_no_longer_the_agents() {
        // The witness is a snapshot, so the destructive path re-reads the
        // recorded fact immediately before it does anything. This is that read.
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut row = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        row.expires_at = Some("2099-01-01 00:00:00".into());
        crate::state::store::insert_site(&conn, &row).unwrap();
        let scratch = crate::core::scratch::claim(&conn, &row.id).unwrap();
        assert!(crate::core::scratch::still_the_agents(&conn, scratch.id()).unwrap());
        // The user presses Keep — origin flips under the held witness.
        conn.execute("UPDATE sites SET origin = 'user', expires_at = NULL WHERE id = ?1", [&row.id])
            .unwrap();
        assert!(
            !crate::core::scratch::still_the_agents(&conn, scratch.id()).unwrap(),
            "a Keep between claim and delete must be seen — a stale witness is not permission"
        );
        // ...and a site deleted in between is not the agent's either.
        conn.execute("DELETE FROM sites WHERE id = ?1", [&row.id]).unwrap();
        assert!(!crate::core::scratch::still_the_agents(&conn, scratch.id()).unwrap());
    }

    #[test]
    fn a_reap_that_keeps_failing_says_so_once_not_once_per_launch() {
        // Retry-once-per-launch means a site that CANNOT be deleted would write
        // an identical row every launch, forever: a slow flood that buries the
        // feed and makes one persistent problem look like many events. The first
        // occurrence is news; the same failure after it is not.
        use crate::mcp_server::feed::{last_reap, record_reap, recent, Outcome};
        let conn = crate::state::db::open_in_memory().unwrap();
        let id = "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24";
        assert!(record_reap(&conn, id, Outcome::Error, Some("database drop failed".into())).unwrap());
        for _ in 0..5 {
            assert!(
                !record_reap(&conn, id, Outcome::Error, Some("database drop failed".into())).unwrap(),
                "the same problem is not news on every launch"
            );
        }
        assert_eq!(recent(&conn, 50).unwrap().len(), 1, "one row per distinct problem");
        // A DIFFERENT reason is news again — it tells the user something changed.
        assert!(record_reap(&conn, id, Outcome::Error, Some("docroot is not writable".into())).unwrap());
        assert_eq!(recent(&conn, 50).unwrap().len(), 2);
        // And success is always recorded: the site is gone, which is the single
        // most consequential event in this lifecycle.
        assert!(record_reap(&conn, id, Outcome::Ok, None).unwrap());
        assert!(record_reap(&conn, id, Outcome::Ok, None).unwrap(), "a reap is never deduped away");
        assert_eq!(last_reap(&conn, id).unwrap().unwrap().0, Outcome::Ok);
    }
}
