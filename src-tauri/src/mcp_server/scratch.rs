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
//! The plumbing, the disjointness guard and the sweep union landed BEFORE the
//! first tool did, deliberately: an executing tool arrives into a structure that
//! already refuses to let it be registered untested, unlisted or unswept.
//!
//! **One scrubber, not one per door.** Anything a tool here re-emits goes
//! through [`super::view::scrub_log_line`] — `tail_log`'s scrubber (#201), which
//! is called from this module, never copied into it. wp names the site's
//! absolute docroot in ordinary success output, so it is the same leak arriving
//! through a third door; a local "quick scrub" would agree the day it was
//! written and drift afterwards, which is what
//! `one_scrubber_serves_every_door_a_docroot_can_leave_by` fails on.

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
}, ScratchTool {
    name: "scratch_add_package",
    description: "Copy a plugin or theme you are developing into a scratch site so it can be \
                  tested. Takes `site_id` and `source` (the directory holding the plugin's main \
                  PHP file or the theme's style.css). rexenv works out which it is from the \
                  source's own header. **It is COPIED, not linked**: nothing the site does can \
                  write back to the source, and the site runs the code as of this moment — call \
                  scratch_sync_package after you change it.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "source": { "type": "string", "description": "Absolute path to the plugin/theme directory." }
        },
        "required": ["site_id", "source"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id, "source": "/tmp/rexenv-sweep-probe" }),
    handler: add_package,
}, ScratchTool {
    name: "scratch_sync_package",
    description: "Re-copy a plugin or theme into the scratch site from where it was added, so \
                  the site runs your latest code. Takes `site_id` and `slug`. The site runs a \
                  SNAPSHOT — call this after every change you want the site to see.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "slug": { "type": "string", "description": "The package's folder name (from scratch_add_package)." }
        },
        "required": ["site_id", "slug"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id, "slug": "sweep-probe" }),
    handler: sync_package,
}, ScratchTool {
    name: "wp_run",
    description: "Run a WP-CLI command inside a scratch site — activate a plugin, set an option, \
                  run the plugin's own commands, whatever the check needs. Takes `site_id` and \
                  `args`, the command as an array of words WITHOUT the leading `wp` (e.g. \
                  [\"plugin\", \"activate\", \"acme\"]). rexenv decides which site it runs against, \
                  from `site_id`, so `--path`, `--url`, `--ssh`, `--http` and `@aliases` are \
                  refused — everything else is yours to run. Only works on scratch sites the agent \
                  created. The command's exit code, stdout and stderr all come back; a non-zero \
                  exit is an answer, not a tool failure, so check `succeeded`.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "The WP-CLI command as separate words, without `wp` and without \
                                `--path` (rexenv adds that): [\"plugin\", \"activate\", \"acme\"]."
            }
        },
        "required": ["site_id", "args"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id, "args": ["option", "get", "home"] }),
    handler: wp_run,
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

    fn platform(&self) -> &dyn crate::platform::traits::Platform {
        self.state.platform.as_ref()
    }

    /// The bundled PHP CLI for a site's PHP minor + the wp-cli phar — the same
    /// pinned, checksum-locked pair the UI and the CLI run (`BinaryProvider`),
    /// resolved through the platform trait. Downloads on first use, so the
    /// caller must treat the gap either side of it as a real window.
    async fn wp_tools(&self, php_minor: &str) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
        let patch = crate::core::php::patch_for_minor(php_minor).ok_or_else(|| {
            Error::Other(format!(
                "this site is set to PHP {php_minor}, which rexenv has no pinned build for — the \
                 person you're working with can change the site's PHP version in rexenv."
            ))
        })?;
        let php_bin = crate::core::binaries::resolve(self.platform(), "php", patch).await?;
        // wp-cli is a .phar, not a Mach-O → resolve_file (no chmod/codesign).
        let wp_phar = crate::core::binaries::resolve_file(
            self.platform(),
            "wp-cli",
            crate::core::binaries::WP_CLI_VERSION,
        )
        .await?;
        Ok((php_bin, wp_phar))
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
    #[test]
    fn a_package_lands_in_the_sites_recorded_content_dir_by_kind() {
        // The destination follows the site's RECORDED content dir (Bedrock's
        // `app/`, Radicle's `content/`), not a hardcoded wp-content — writing to
        // a dead wp-content would silently install nothing.
        let mut site = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        assert!(package_dest(&site, "plugin", "acme").ends_with("wp-content/plugins/acme"));
        assert!(package_dest(&site, "theme", "acme").ends_with("wp-content/themes/acme"));
        site.content_dir = Some("app".into());
        assert!(package_dest(&site, "plugin", "acme").ends_with("app/plugins/acme"));
    }

    #[test]
    fn one_scrubber_serves_every_door_a_docroot_can_leave_by() {
        // #201 closed the docroot leak for `tail_log`. `wp_run` opens a THIRD
        // door onto the same value — wp prints absolute paths in ordinary
        // success output — so the fix is one function called twice, never two
        // that happen to agree. Both halves matter: the behaviour, and the
        // structural fact that there is only one implementation to drift from.

        // Half one — BEHAVIOUR. The same planted line down both tools' paths.
        let docroot = "/Users/somebody/Sites/probe.scratch.rex";
        let line = format!("Success: Created {docroot}/wp-content/plugins/acme/acme.php");
        let (via_wp_run, truncated) = agent_stream(line.as_bytes(), docroot);
        let mut tail =
            super::super::view::AgentLogTail::from_lines("id", "d.rex", docroot, "wp-debug", vec![line.clone()]);
        let via_tail_log = tail.lines.remove(0);
        assert!(!truncated, "a short stream is not truncated");
        assert_eq!(via_wp_run, via_tail_log, "the two doors must agree because they are one function");
        assert_ne!(via_wp_run, line, "…and non-vacuously: the line WAS changed");
        assert!(!via_wp_run.contains("/Users/somebody"), "OS username reached the agent: {via_wp_run}");
        assert!(via_wp_run.contains("<docroot>/wp-content/plugins/acme/acme.php"), "{via_wp_run}");

        // Half two — STRUCTURE. `scrub_log_line` is defined once, in view.rs, and
        // no tool module grows its own. This is the half that fails on a future
        // "quick local scrub" rather than letting it pass quietly.
        assert_eq!(
            include_str!("view.rs").matches("pub fn scrub_log_line").count(),
            1,
            "scrub_log_line must be defined exactly once, in view.rs"
        );
        assert!(
            production_lines(include_str!("scratch.rs"))
                .iter()
                .any(|(_, l)| l.split("//").next().unwrap_or("").contains("view::scrub_log_line")),
            "the executing tools must CALL the shared scrubber — if this call went away, either a \
             tool stopped scrubbing or it grew its own scrubber; both are the #201 leak returning. \
             (Checked against production lines only: this test's own prose names the function.)"
        );
        for (file, src, canary) in [
            ("mcp_server/scratch.rs", include_str!("scratch.rs"), "fn agent_stream"),
            ("mcp_server/tools.rs", include_str!("tools.rs"), "fn tail_log"),
        ] {
            let production = production_lines(src);
            // The scan's own coverage, checked rather than assumed: a
            // `#[cfg(test)]` module can sit ANYWHERE in a file (in this one it
            // sits in the middle), and a stripper that cut from it to the end of
            // the file would skip most of the production code while still
            // passing — the guard-covers-a-narrower-surface-than-its-claim
            // family, caught here by planting.
            assert!(
                production.iter().any(|(_, l)| l.contains(canary)),
                "the scan missed `{canary}` in {file} — it is not covering the file it claims to"
            );
            for (n, l) in production {
                // Comments stripped: prose names the tokens to explain the rule.
                let code = l.split("//").next().unwrap_or("");
                // A scrubbing ROUTINE — a fn, or a closure bound to a name. A
                // local named `scrubbed` holding the shared function's OUTPUT is
                // not one, which is why the closure arm keys on `= |`.
                let names_one = code.contains("scrub") || code.contains("redact");
                let defines_a_scrubber =
                    names_one && (code.contains("fn ") || code.contains("= |"));
                assert!(
                    !defines_a_scrubber,
                    "a SECOND scrubber at {}:{} — `{}`.\n\
                     Agent-facing output is scrubbed by ONE function, `view::scrub_log_line`, \
                     because a second one agrees on the day it is written and drifts after (#201 \
                     was the docroot leaking through tail_log; wp_run was the same value through a \
                     third door). If the shapes don't fit, WIDEN scrub_log_line and let both \
                     callers inherit it — do not fork it here.",
                    file,
                    n,
                    code.trim()
                );
            }
        }
    }

    /// A file's PRODUCTION lines (1-indexed), with any `#[cfg(test)]` module
    /// removed by brace depth rather than by cutting to end-of-file — the test
    /// module is not always last, and a guard that assumed it was would quietly
    /// stop covering everything after it.
    fn production_lines(src: &str) -> Vec<(usize, &str)> {
        let mut out = Vec::new();
        let mut depth: Option<i32> = None;
        for (i, line) in src.lines().enumerate() {
            match depth.as_mut() {
                None if line.trim_start().starts_with("#[cfg(test)]") => depth = Some(0),
                None => out.push((i + 1, line)),
                Some(d) => {
                    *d += line.matches('{').count() as i32 - line.matches('}').count() as i32;
                    if *d <= 0 && line.contains('}') {
                        depth = None;
                    }
                }
            }
        }
        out
    }

    #[test]
    fn wp_run_takes_its_target_from_the_witness_and_refuses_the_agents() {
        // Ruling 1, at the tool's own door. The screen itself is core's
        // (`refuse_wp_target_override`, exhaustively tested there); what this
        // pins is that wp_run's ARGUMENT PARSING cannot smuggle a target past it
        // — a whole command line in one string, or a non-string entry, would
        // both reach wp-cli as argv this screen never inspected word by word.
        let one_string = json!({ "site_id": "x", "args": "plugin activate acme --path=/elsewhere" });
        let err = wp_argv(&one_string).unwrap_err().to_string();
        assert!(err.contains("array of separate words"), "{err}");
        assert!(err.contains("not \"plugin activate acme\""), "shows the shape, not just the rule: {err}");

        assert!(wp_argv(&json!({ "args": [] })).unwrap_err().to_string().contains("at least one word"));
        assert!(wp_argv(&json!({}))
            .unwrap_err()
            .to_string()
            .contains("array of separate words"));
        let mixed = json!({ "args": ["plugin", 7, "acme"] });
        assert!(wp_argv(&mixed).unwrap_err().to_string().contains("has to be a string"));

        // A well-formed argv survives intact, word for word — the screen sees
        // exactly what wp-cli will.
        let ok = wp_argv(&json!({ "args": ["plugin", "activate", "acme"] })).unwrap();
        assert_eq!(ok, vec!["plugin", "activate", "acme"]);
        crate::core::scratch::refuse_wp_target_override(&ok).unwrap();
    }

    #[test]
    fn a_wp_run_result_says_whether_it_worked_rather_than_leaving_it_to_be_inferred() {
        // A non-zero exit comes back as a normal RESULT, not a tool error: the
        // tool worked, and `wp plugin is-active x` exits 1 to mean "no". The
        // price of that choice is that the payload must be impossible to skim
        // past — hence `succeeded`, and a detail that leads with FAILED.
        let v = serde_json::to_value(AgentWpRun {
            succeeded: false,
            exit_code: Some(1),
            stdout: String::new(),
            stderr: "Error: The 'acme' plugin could not be found.".into(),
            truncated: false,
            detail: "`wp plugin activate acme` FAILED in `probe.scratch.rex` (exit 1). What WP-CLI \
                     said is in `stderr`."
                .into(),
        })
        .unwrap();
        assert_eq!(v["succeeded"], false);
        assert_eq!(v["exitCode"], 1);
        assert!(v["detail"].as_str().unwrap().contains("FAILED"), "{v}");
        assert!(v["stderr"].as_str().unwrap().contains("could not be found"), "the reason travels: {v}");
    }

    #[test]
    fn a_flood_of_output_is_cut_and_the_cut_is_stated() {
        // A raw runner can emit a database dump; a tool reply is a context
        // window. The cap is not the interesting part — the fact that it is
        // REPORTED is, because silent truncation reads as a complete answer.
        let flood = "x".repeat(WP_OUTPUT_CAP * 2);
        let (kept, truncated) = agent_stream(flood.as_bytes(), "/dr");
        assert!(truncated, "a stream over the cap must report the cut");
        assert!(kept.len() <= WP_OUTPUT_CAP, "cut at the cap: {}", kept.len());
        // Multi-byte content is cut on a character boundary, not mid-codepoint.
        let wide = "é".repeat(WP_OUTPUT_CAP);
        let (kept, truncated) = agent_stream(wide.as_bytes(), "/dr");
        assert!(truncated && kept.chars().all(|c| c == 'é'), "cut mid-codepoint");
    }

    #[test]
    fn the_sync_wording_never_claims_the_source_is_unchanged() {
        // The fingerprint is stat-only: a difference is reliable, sameness is a
        // strong hint. So the copy says what was DETECTED, and hedges the rest.
        let same = format!(
            "`acme` was re-copied. No changes were detected in the source since the last sync, \
             so the site was probably already running this code."
        );
        assert!(same.contains("No changes were detected"), "{same}");
        assert!(!same.contains("unchanged"), "must not claim more than a stat read can know: {same}");
        assert!(same.contains("probably"), "hedged, deliberately: {same}");
    }

}

/// Where a package lands inside a site, and what it is called there.
fn package_dest(site: &Site, kind: &str, slug: &str) -> std::path::PathBuf {
    std::path::Path::new(&site.path)
        .join(site.content_dir_rel())
        .join(if kind == "theme" { "themes" } else { "plugins" })
        .join(slug)
}

/// Copy a plugin/theme the user is developing INTO a scratch site (S1).
fn add_package<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let (id, source) = two_args(args, "site_id", "source", "scratch_add_package")?;
        let scratch = ctx.claim(&id)?;
        acted.set(scratch.site());
        // Blast radius FIRST: cloning is not a licence to read. `$HOME`, a
        // volume root, Desktop/Documents/Downloads, app-data and overlaps are
        // refused exactly as hard as linking them would have been (§4.4) — the
        // clone changed the WRITE direction, not the read direction.
        let src = {
            let conn = ctx.db()?;
            crate::core::sites::validate_linked_docroot(&conn, ctx.platform(), &source)?
        };
        // Then the header, on the SOURCE, before anything is copied.
        let kind = crate::core::scratch::detect_kind(&src)?;
        let slug = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Other("that source path has no directory name.".into()))?
            .to_string();
        let dest = package_dest(scratch.site(), kind, &slug);
        let _ = std::fs::remove_dir_all(&dest);
        crate::core::scratch::clone_tree(&src, &dest)?;
        let fingerprint = crate::core::scratch::fingerprint(&src)?;
        let synced_at = {
            let conn = ctx.db()?;
            let now = crate::state::store::db_now(&conn)?;
            crate::state::store::upsert_scratch_package(
                &conn,
                &crate::state::models::ScratchPackage {
                    site_id: scratch.id().to_string(),
                    slug: slug.clone(),
                    kind: kind.to_string(),
                    source_path: src.display().to_string(),
                    synced_at: now.clone(),
                    fingerprint,
                },
            )?;
            now
        };
        Ok(json!({
            "slug": slug,
            "kind": kind,
            "syncedAt": synced_at,
            "detail": format!(
                "`{slug}` was COPIED into {} — the site runs it as of now, and nothing it does \
                 can write back to your source. Call scratch_sync_package after you change it.",
                scratch.domain()
            ),
        }))
    })
}

/// Re-copy a recorded package so the site runs the latest code.
fn sync_package<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let (id, slug) = two_args(args, "site_id", "slug", "scratch_sync_package")?;
        let scratch = ctx.claim(&id)?;
        acted.set(scratch.site());
        let recorded = {
            let conn = ctx.db()?;
            crate::state::store::scratch_package(&conn, scratch.id(), &slug)?
        }
        .ok_or_else(|| {
            Error::Other(format!(
                "`{slug}` was never added to `{}`. Add it with scratch_add_package first.",
                scratch.domain()
            ))
        })?;
        // Only ever re-reads where the clone CAME FROM — the recorded path,
        // never one supplied now.
        let src = std::path::PathBuf::from(&recorded.source_path);
        if !src.is_dir() {
            return Err(Error::Other(format!(
                "the source `{slug}` was copied from is no longer there. If you moved it, add it \
                 again with scratch_add_package."
            )));
        }
        let now_print = crate::core::scratch::fingerprint(&src)?;
        // "changed" is reliable; sameness is a strong HINT, not a proof (an edit
        // preserving mtime, size and count is invisible to a stat-only read) —
        // so the wording is "no changes detected", never "unchanged".
        let changed = now_print != recorded.fingerprint;
        let dest = package_dest(scratch.site(), &recorded.kind, &slug);
        let _ = std::fs::remove_dir_all(&dest);
        crate::core::scratch::clone_tree(&src, &dest)?;
        let synced_at = {
            let conn = ctx.db()?;
            let now = crate::state::store::db_now(&conn)?;
            crate::state::store::upsert_scratch_package(
                &conn,
                &crate::state::models::ScratchPackage {
                    synced_at: now.clone(),
                    fingerprint: now_print,
                    ..recorded
                },
            )?;
            now
        };
        Ok(json!({
            "slug": slug,
            "syncedAt": synced_at,
            "sourceHadChanged": changed,
            "detail": if changed {
                format!("`{slug}` was re-copied — the site now runs your latest code.")
            } else {
                format!(
                    "`{slug}` was re-copied. No changes were detected in the source since the \
                     last sync, so the site was probably already running this code."
                )
            },
        }))
    })
}

// ---------------------------------------------------------------------------
// wp_run — the raw runner (D1), and the two rulings it is built to
// ---------------------------------------------------------------------------

/// How much of each stream an agent gets back. A raw runner can emit a database
/// dump; a tool reply is a model's context window.
const WP_OUTPUT_CAP: usize = 32 * 1024;

/// What a WP-CLI run looks like to an agent.
///
/// The exit code and BOTH streams travel, because a raw runner that hides
/// either is useless: wp writes its answer to stdout and its reason to stderr,
/// and which one carries the news depends on the command.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentWpRun {
    /// Exit 0. Named rather than left to be inferred from `exitCode`, because a
    /// non-zero exit comes back as a normal result (see [`wp_run`]) and the one
    /// thing that must not be skimmed past is whether it worked.
    succeeded: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    /// Whether either stream was cut at the cap — stated, never silent.
    truncated: bool,
    detail: String,
}

/// Run an arbitrary WP-CLI command inside a scratch site.
///
/// **The target comes from the WITNESS, not from the agent.** `--path` is
/// rexenv's, derived from the claimed [`ScratchSite`]'s recorded docroot, and an
/// agent-supplied `--path`/`--url`/`--ssh`/`--http`/`@alias` is REFUSED
/// (`core::scratch::refuse_wp_target_override`) rather than overridden. Passing
/// ours and hoping would not be enough: in WP-CLI a later `--path` wins, so
/// "ours is appended last" is a race we happen to win, not a rule. The refusal
/// is the rule.
///
/// **There is no subcommand denylist, and that is a decision — not an
/// oversight.** Activating a plugin already runs arbitrary user-level PHP
/// (PLAN §3.1, D1, #197), so a list of forbidden verbs would buy nothing real
/// while LOOKING like protection — the guard-covers-a-narrower-surface-than-its-
/// claim family this codebase rejected in S1. `wp eval`, `wp db query`, `wp
/// plugin install --force` all run. What is screened is which SITE, because that
/// is the tier boundary itself: a closed, small, documented set, where the
/// narrower-surface failure cannot happen. Refusing "which site" is enforceable;
/// refusing "which command" isn't.
///
/// **Output goes through the SAME scrub as `tail_log`** (`view::scrub_log_line`,
/// #201). wp re-emits absolute docroot paths constantly — "Created
/// /Users/<name>/Sites/…" — which is the leak #201 closed for log tails,
/// arriving through a third door. One function, called from both, never copied.
fn wp_run<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("wp_run needs a `site_id`.".into()))?;
        let argv = wp_argv(args)?;

        // THE gate, first: the recorded origin decides, and the witness is the
        // only handle to a site this module has.
        let scratch = ctx.claim(id)?;
        acted.set(scratch.site());
        // Then the target screen, BEFORE anything is resolved or spawned.
        crate::core::scratch::refuse_wp_target_override(&argv)?;

        let site = scratch.site();
        let docroot = std::path::PathBuf::from(&site.path);
        // Resolving the bundled PHP + wp-cli phar can DOWNLOAD on first use —
        // minutes, not milliseconds. That is a real window between the claim and
        // the run, so the recorded fact is re-read immediately before spawning:
        // the witness proves the path was gated when it claimed, never that the
        // user has not pressed Keep since (the snapshot rule).
        let (php_bin, wp_phar) = ctx.wp_tools(&site.php_version).await?;
        {
            let conn = ctx.db()?;
            if !crate::core::scratch::still_the_agents(&conn, scratch.id())? {
                return Err(Error::Other(format!(
                    "`{}` is no longer a scratch site — the person you're working with kept it, so \
                     it is theirs now and agent tools cannot run anything in it.",
                    scratch.domain()
                )));
            }
        }

        let printed = argv.join(" ");
        let timeout =
            std::time::Duration::from_secs(crate::core::scratch::WP_RUN_TIMEOUT_SECS);
        let out = crate::commands::wordpress::wp_blocking(move || {
            crate::core::wordpress::wp_run_raw(&php_bin, &wp_phar, &docroot, &argv, timeout)
        })
        .await?;

        let dr = site.path.clone();
        let (stdout, cut_out) = agent_stream(&out.stdout, &dr);
        let (stderr, cut_err) = agent_stream(&out.stderr, &dr);
        let succeeded = out.status.success();
        let exit_code = out.status.code();
        let mut detail = if succeeded {
            format!("`wp {printed}` ran in `{}` and succeeded.", scratch.domain())
        } else {
            format!(
                "`wp {printed}` FAILED in `{}` (exit {}). What WP-CLI said is in `stderr`.",
                scratch.domain(),
                exit_code.map_or_else(|| "killed by a signal".to_string(), |c| c.to_string())
            )
        };
        if cut_out || cut_err {
            detail.push_str(&format!(
                " The output was longer than {} KB and has been cut — run a narrower command if \
                 you need the rest.",
                WP_OUTPUT_CAP / 1024
            ));
        }
        let view = AgentWpRun {
            succeeded,
            exit_code,
            stdout,
            stderr,
            truncated: cut_out || cut_err,
            detail,
        };
        serde_json::to_value(view).map_err(|e| Error::Other(format!("serialising the run: {e}")))
    })
}

/// The `args` array, as strings — refusing the shapes that would silently run
/// the wrong thing (a bare string an agent meant as a whole command line, a
/// number, an empty array).
fn wp_argv(args: &Value) -> Result<Vec<String>> {
    let Some(list) = args.get("args").and_then(Value::as_array) else {
        return Err(Error::Other(
            "wp_run needs `args`: the command as an array of separate words, without `wp` — \
             [\"plugin\", \"activate\", \"acme\"], not \"plugin activate acme\"."
                .into(),
        ));
    };
    if list.is_empty() {
        return Err(Error::Other(
            "wp_run needs at least one word in `args` — the WP-CLI subcommand to run, e.g. \
             [\"plugin\", \"list\"]."
                .into(),
        ));
    }
    list.iter()
        .map(|v| {
            v.as_str().map(str::to_string).ok_or_else(|| {
                Error::Other(
                    "every entry in `args` has to be a string — one WP-CLI word per entry."
                        .into(),
                )
            })
        })
        .collect()
}

/// One captured stream, made fit for an agent: cut at the cap, then scrubbed
/// line by line through **`view::scrub_log_line`** — `tail_log`'s scrubber
/// (#201), not a local one. wp names the docroot in ordinary success output, so
/// this is the same leak arriving through a third door; a second scrubber would
/// agree the day it was written and drift after.
///
/// The cut keeps the HEAD: wp writes its column headers, its `Success:` line and
/// its first error at the start, so the front of a long stream is the part with
/// the answer in it. The cut is always reported (`truncated`), never silent.
fn agent_stream(raw: &[u8], docroot: &str) -> (String, bool) {
    let text = String::from_utf8_lossy(raw);
    let mut end = WP_OUTPUT_CAP.min(text.len());
    while end < text.len() && !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < text.len();
    let scrubbed = text[..end]
        .lines()
        .map(|l| super::view::scrub_log_line(l, docroot))
        .collect::<Vec<_>>()
        .join("\n");
    (scrubbed, truncated)
}

fn two_args(args: &Value, a: &str, b: &str, tool: &str) -> Result<(String, String)> {
    let get = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
    match (get(a), get(b)) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(Error::Other(format!("{tool} needs `{a}` and `{b}`."))),
    }
}
