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
use std::collections::HashSet;

/// The edge's HTTPS port — the one place a browser reaches a site.
const EDGE_HTTPS_PORT: u16 = 443;

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
