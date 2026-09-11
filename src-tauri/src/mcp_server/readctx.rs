//! `ReadCtx` — the ONLY door an M1 tool handler has to app state, and it opens
//! only onto reads.
//!
//! **The guarantee, stated honestly (assembly-review correction):** a handler
//! receives a `ReadCtx` and nothing else, and `ReadCtx` exposes no mutating
//! method and keeps its `state` field private — so a handler **cannot mutate
//! rexenv's state** (start/stop a service, write the DB, change a site). It is
//! NOT a claim that a handler "cannot write or delete anything" in the abstract:
//! a handler is a plain `fn` and could in principle call `std::fs`/`std::process`
//! itself. That the shipped handlers don't is checked by the read-only guard
//! (which scans BOTH `tools.rs` and this bridge); the type-level boundary is
//! that state is reachable only through this read-only method set.
//!
//! **Keep it minimal to the point of inconvenience.** Every method here is
//! permanent M1 surface — easier to add one later than to remove one a tool
//! depends on. Prefer widening a tool's own conversion over widening ReadCtx.
//!
//! This module is the trusted bridge, so it (unlike `tools`) may reach `core`
//! READS and the `AppState` snapshot — but never a mutator (guard-scanned). The
//! `tools` module imports only `ReadCtx`. `ReadCtx` is `Copy` (a single
//! `&AppState`) so async handlers can take it by value.

use super::view::ServingSignals;
use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{Site, SiteType};
use std::collections::{HashMap, HashSet};

/// The edge's HTTPS port — the one place a browser reaches a site.
const EDGE_HTTPS_PORT: u16 = 443;

/// One service in the stack snapshot — name, liveness (ownership AND liveness,
/// the manager's own verdict), port. No pid.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackService {
    pub name: String,
    pub running: bool,
    pub port: u16,
    pub optional: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackPhp {
    pub minor: String,
    pub installed: bool,
    pub default: bool,
}

/// The stack, as `stack_status` reports it. Every field is named; nothing on
/// disk is.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackSnapshot {
    pub services: Vec<StackService>,
    /// False when the services lock was busy and `services` is the previous
    /// snapshot — the tray's own honesty rule, carried to the agent.
    pub services_fresh: bool,
    pub dns_answers: bool,
    pub resolver_installed: bool,
    pub ca_trusted: bool,
    pub mail_running: bool,
    /// Whether every site's outgoing mail is caught in Mailpit (`true`, the
    /// default) or sent for real. Changed with `stack` `catch_mail` /
    /// `stop_catching_mail`.
    pub mail_catch_all: bool,
    pub php: Vec<StackPhp>,
    pub default_tld: String,
    pub cli_installed: bool,
    pub cli_current: bool,
    /// A newer rexenv a VERIFIED offer names, if there is one.
    ///
    /// A Read, and only a Read: there is no MCP tool that installs it. A
    /// self-update replaces the process that enforces the agent-access dial and
    /// `settings_access`, so an agent asking for one would be asking to replace
    /// the thing that bounds it; and the relaunch kills the socket the answer
    /// would come back on, so it could never observe the result anyway. Telling
    /// it that a newer version exists costs nothing and saves it reporting a
    /// bug that is already fixed (ledger #530).
    pub app_update: Option<String>,
}

/// What `inspect_folder` learned — the dialog's `LinkedFolderInfo` minus the two
/// absolute paths (the agent supplied the root, and the served folder is
/// `docroot_rel` under it).
pub struct InspectedFolder {
    pub site_type: SiteType,
    pub docroot_rel: String,
    pub label: &'static str,
    pub existing_install: bool,
    pub has_custom_valet_driver: bool,
}

#[derive(Clone, Copy)]
pub struct ReadCtx<'a> {
    state: &'a AppState,
}

impl<'a> ReadCtx<'a> {
    pub fn new(state: &'a AppState) -> Self {
        ReadCtx { state }
    }

    /// The sites rexenv manages (read-only).
    pub fn sites(&self) -> Result<Vec<Site>> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        core::sites::list(&conn)
    }

    /// One site by id (read-only), or `None` if there is no such site.
    pub fn site_by_id(&self, id: &str) -> Result<Option<Site>> {
        Ok(self.sites()?.into_iter().find(|s| s.id == id))
    }

    /// Every site's EXTRA hostnames, keyed by site id (v42) — one read for the
    /// whole list, the Sites page's own shape.
    pub fn aliases_by_site(&self) -> Result<HashMap<String, Vec<String>>> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        crate::state::store::all_site_aliases(&conn)
    }

    /// The site's HTTPS leaf certificate, parsed from the file (read-only;
    /// `None` when none has been issued yet). The caller drops the cert DIR.
    pub fn cert_info(&self, site: &Site) -> Result<Option<core::ssl::SiteCertInfo>> {
        core::ssl::site_cert_info(self.state.platform.paths(), &site.domain)
    }

    /// The packages an agent added to a SCRATCH site (v29) — empty for the
    /// user's own sites, which have none by construction.
    pub fn scratch_packages_of(&self, site: &Site) -> Result<Vec<crate::state::models::ScratchPackage>> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        Ok(crate::state::store::all_scratch_packages(&conn)?
            .into_iter()
            .filter(|p| p.site_id == site.id)
            .collect())
    }

    /// The stack as rexenv itself sees it — `rex doctor`'s composite, read-only:
    /// the manager's service snapshot, whether OUR edge answers, whether OUR DNS
    /// answers, whether the backbone resolver file is installed and the CA is
    /// trusted, the mail catcher, PHP versions, the default TLD, the CLI link.
    /// No path, no pid, no socket: what an agent needs to say "the stack is
    /// stopped — press Start in rexenv", never where anything lives.
    pub fn stack_snapshot(&self) -> Result<StackSnapshot> {
        let (services, fresh) = self.state.service_infos_fresh();
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        let php: Vec<StackPhp> = crate::state::store::list_php_versions(&conn)?
            .into_iter()
            .map(|v| StackPhp { minor: v.minor, installed: v.installed, default: v.is_default })
            .collect();
        let default_tld = core::sites::default_tld(&conn)?;
        let mail_catch_all = core::mail::catch_all_enabled(&conn);
        drop(conn);
        let platform = self.state.platform.as_ref();
        let cli = core::cli::status(platform).ok();
        Ok(StackSnapshot {
            services: services.into_iter().map(|s| StackService { name: s.name, running: s.running, port: s.port, optional: s.optional }).collect(),
            services_fresh: fresh,
            dns_answers: core::dns::answers_as_ours(core::dns::DEFAULT_DNS_PORT),
            resolver_installed: platform.dns().resolver_path(core::tld::BACKBONE_TLD).exists(),
            ca_trusted: platform.cert_trust().is_trusted(&self.state.ca.cert_path),
            mail_running: core::mail::running(),
            mail_catch_all,
            php,
            default_tld,
            cli_installed: cli.as_ref().is_some_and(|c| c.installed),
            cli_current: cli.as_ref().is_some_and(|c| c.current),
            // The published snapshot, like the tray reads: no verify, no
            // network, and no database read on a status call.
            app_update: crate::core::app_update::current_offer().map(|o| o.version),
        })
    }

    /// One setting, through the CLI's own allow-list (`core::settings_access`,
    /// deny by default): a Denied key is refused with the policy's reason and
    /// never read — the signed update chain and the MCP switches live in the
    /// same table as the preferences.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        if let core::settings_access::CliAccess::Denied(why) = core::settings_access::cli_access(key) {
            return Err(Error::Other(format!("`{key}` is not readable through an agent: {why}.")));
        }
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        crate::state::store::get_setting(&conn, key)
    }

    /// A PHP minor's ini overrides — every key rexenv edits, with its stored
    /// value (if any) and default. Pure read of the settings row.
    pub fn php_settings(&self, minor: &str) -> Result<Vec<(&'static str, Option<String>, &'static str)>> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        let stored = crate::state::store::get_php_settings(&conn, minor)?;
        Ok(core::php::SETTINGS
            .iter()
            .map(|s| (s.key, stored.iter().find(|(k, _)| k == s.key).map(|(_, v)| v.clone()), s.default))
            .collect())
    }

    /// The saved blueprints (create-time presets): name and spec, no ids — a
    /// blueprint is referred to by NAME everywhere on the agent surface.
    pub fn blueprints(&self) -> Result<Vec<crate::state::models::Blueprint>> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        crate::state::store::list_blueprints(&conn)
    }

    /// The agent activity feed — the user's own audit view, readable by the
    /// agent too: "what did I do" is a fair question. Rows are typed and every
    /// value in them is rexenv's (the summariser records verbs, #222).
    pub fn activity(&self, site: Option<&str>, limit: usize) -> Result<Vec<super::feed::AgentAction>> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        let mut rows = match site {
            Some(id) => super::feed::recent_for_site(&conn, id, limit),
            None => super::feed::recent(&conn, limit),
        }?;
        super::feed::resolve_target_labels(&conn, &mut rows)?;
        Ok(rows)
    }

    /// Search the WordPress.org directory — a network READ of a public API, no
    /// site involved (the Add-plugin/theme flows' own call).
    pub async fn wporg_search(&self, kind: &str, query: &str) -> Result<serde_json::Value> {
        match kind {
            "plugins" => serde_json::to_value(core::wporg::search_plugins(query).await?),
            _ => serde_json::to_value(core::wporg::search_themes(query).await?),
        }
        .map_err(|e| Error::Other(format!("serialising the search: {e}")))
    }

    /// Classify a folder the agent names WITHOUT creating anything — the New
    /// Site dialog's own preflight (`validate_linked_docroot`, which refuses
    /// `/`, the home folder, Desktop/Documents/Downloads, volume roots, app-data
    /// and overlaps with another site) and then `detect_project`, which reads
    /// marker files and executes nothing. A refusal is the dialog's own words.
    pub fn inspect_folder(&self, path: &str) -> Result<InspectedFolder> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        let platform = self.state.platform.as_ref();
        let root = core::sites::validate_linked_docroot(&conn, platform, path)?;
        let detected = core::sites::detect_project(&root);
        let serve = if detected.docroot_rel.is_empty() { root.clone() } else { root.join(&detected.docroot_rel) };
        core::sites::validate_linked_docroot(&conn, platform, &serve.display().to_string())?;
        Ok(InspectedFolder {
            site_type: detected.site_type,
            docroot_rel: detected.docroot_rel,
            label: detected.label,
            existing_install: detected.existing_install,
            has_custom_valet_driver: core::sites::has_custom_valet_driver(&root),
        })
    }

    /// The set of domains ACTUALLY serving right now — the non-blocking
    /// `service_infos()` snapshot fed through the same `site_serving` computation
    /// the Sites page uses. Reads a snapshot, never a live service handle, and
    /// holds no lock across the two reads (the locking rule).
    pub fn serving_domains(&self) -> Result<HashSet<String>> {
        let sites = self.sites()?;
        let serving = core::service_manager::site_serving(&sites, &self.state.service_infos());
        Ok(serving.into_iter().filter(|s| s.serving).map(|s| s.domain).collect())
    }

    /// Gather the signals behind "is this site serving, and if not, why" —
    /// WITHOUT requesting the site (M1 runs nothing; a GET would boot WordPress
    /// and fire wp-cron). Reads the SERVING PATH's own state: the manager's
    /// belief (a snapshot, no lock across the network waits), whether OUR edge
    /// answers its 204 marker probe (the edge answers that itself — it does not
    /// proxy to the site), and whether anything holds :443. The site's OWN render
    /// errors are `tail_log`'s territory, never a signal here. Classification is
    /// pure (`ServingSignals::classify`).
    pub async fn probe_serving(&self, site: &Site) -> ServingSignals {
        // Manager belief: edge && this site's upstream up (the Sites-page bool).
        let serving_manager =
            core::service_manager::site_serving(std::slice::from_ref(site), &self.state.service_infos())
                .first()
                .is_some_and(|s| s.serving);

        // Wire: does OUR edge answer (marker header) for this host? The probe path
        // short-circuits at the edge (`respond 204`), so this NEVER runs the site.
        let edge_answers_ours =
            core::proxy::edge_answers_as_ours(&site.domain, EDGE_HTTPS_PORT).await;

        // Wire: is ANYTHING listening on :443 (to tell "stack stopped" from
        // "another server holds the port")? A bare TCP connect, no request.
        let tcp_443_open = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, EDGE_HTTPS_PORT)),
        )
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false);

        ServingSignals { edge_answers_ours, tcp_443_open, serving_manager }
    }

    /// The RAW tail of the site's WordPress debug log (the caller scrubs), capped
    /// to `lines` and tail-only via `core::logs`. `Ok(None)` for a non-WordPress
    /// site (the WP debug log is the only source M1 exposes) — a NORMAL answer,
    /// not an error, so the tool reports "no log for this site type" without a
    /// misleading concerning feed row. Missing/empty log ⇒ `Ok(Some(empty))`.
    pub fn wp_debug_log_tail(&self, site: &Site, lines: usize) -> Result<Option<Vec<String>>> {
        if site.site_type != SiteType::Wordpress {
            return Ok(None);
        }
        let content_rel = site.content_dir.clone().unwrap_or_else(|| "wp-content".into());
        core::logs::wp_debug_log_tail(std::path::Path::new(&site.path), &content_rel, lines).map(Some)
    }

    /// The absolute paths rexenv knows it might emit for this site — the input
    /// to the one scrubber. Built HERE because it needs the platform's `Paths`,
    /// which `tools.rs` deliberately cannot reach; the handler asks for the set
    /// rather than assembling one, so a directory added to `Paths` is scrubbed
    /// without any tool hearing about it.
    pub fn known_paths(&self, site: &Site) -> super::view::KnownPaths {
        super::view::KnownPaths::for_site(self.state.platform.paths(), &site.path)
    }
}
