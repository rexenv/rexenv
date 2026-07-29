//! `ReadCtx` — the ONLY door an M1 tool handler has to app state, and it opens
//! only onto reads. A handler receives a `ReadCtx` and nothing else, so it
//! physically cannot start, stop, write, or delete anything: the capability
//! boundary is this type's method set.
//!
//! **Keep it minimal to the point of inconvenience.** Every method here is
//! permanent M1 surface — it is far easier to add one later than to remove one
//! after a tool depends on it. If a tool needs data ReadCtx doesn't expose,
//! prefer widening that tool's own conversion over widening ReadCtx.
//!
//! This module is the trusted bridge, so it (unlike `tools`) may reach `core`
//! reads, the `AppState` snapshot, and the network for a probe. The `tools`
//! module imports only `ReadCtx` — enforced by the read-only import guard in the
//! parent module. `ReadCtx` is `Copy` (a single `&AppState`) so async handlers
//! can take it by value.

use super::view::ServingSignals;
use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::Site;
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

    /// Gather the honest signals behind "is this site serving, and if not, why".
    /// Mixes the manager's belief (a snapshot — no lock held across the network
    /// waits) with WIRE truth (does our edge actually answer, is anything on
    /// :443, what does the site's URL return). The classification of these into
    /// a verdict is pure and lives in `ServingSignals::classify`.
    pub async fn probe_serving(&self, site: &Site) -> ServingSignals {
        // Manager belief: edge && this site's upstream up (the Sites-page bool).
        let serving_manager =
            core::service_manager::site_serving(std::slice::from_ref(site), &self.state.service_infos())
                .first()
                .is_some_and(|s| s.serving);

        // Wire: does OUR edge answer (marker header) for this exact host?
        let edge_answers_ours =
            core::proxy::edge_answers_as_ours(&site.domain, EDGE_HTTPS_PORT).await;

        // Wire: is ANYTHING listening on :443 (to tell "stack stopped" from
        // "another server holds the port")?
        let tcp_443_open = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, EDGE_HTTPS_PORT)),
        )
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false);

        // Wire: the site's actual HTTP response for its home URL, if the edge is
        // ours (no point probing a foreign or absent edge).
        let http_status = if edge_answers_ours {
            site_http_status(&site.domain, EDGE_HTTPS_PORT).await
        } else {
            None
        };

        ServingSignals { edge_answers_ours, tcp_443_open, serving_manager, http_status }
    }
}

/// GET the site's home URL through the edge (resolved to loopback), returning
/// the HTTP status. Identity comes from having asked our edge (the caller only
/// probes when the edge answered as ours); the leaf won't chain for reqwest's
/// store, so certs are not verified. `None` on any transport failure.
async fn site_http_status(host: &str, https_port: u16) -> Option<u16> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .resolve(host, std::net::SocketAddr::from(([127, 0, 0, 1], https_port)))
        .timeout(std::time::Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let url = format!("https://{host}:{https_port}/");
    client.get(&url).send().await.ok().map(|r| r.status().as_u16())
}
