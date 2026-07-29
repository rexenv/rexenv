//! `ReadCtx` — the ONLY door an M1 tool handler has to app state, and it opens
//! only onto reads. A handler receives `&ReadCtx` and nothing else, so it
//! physically cannot start, stop, write, or delete anything: the capability
//! boundary is this type's method set.
//!
//! **Keep it minimal to the point of inconvenience.** Every method here is
//! permanent M1 surface — it is far easier to add one later than to remove one
//! after a tool depends on it. If a tool needs data ReadCtx doesn't expose,
//! prefer widening that tool's own conversion over widening ReadCtx.
//!
//! This module is the trusted bridge, so it (unlike `tools`) may reach `core`
//! reads and the `AppState` snapshot. The `tools` module imports only `ReadCtx`
//! — enforced by the read-only import guard in the parent module.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::Site;
use std::collections::HashSet;

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

    /// The set of domains ACTUALLY serving right now — the non-blocking
    /// `service_infos()` snapshot fed through the same `site_serving` computation
    /// the Sites page uses. Reads a snapshot, never a live service handle, and
    /// holds no lock across the two reads (the locking rule).
    pub fn serving_domains(&self) -> Result<HashSet<String>> {
        let sites = self.sites()?;
        let serving = core::service_manager::site_serving(&sites, &self.state.service_infos());
        Ok(serving.into_iter().filter(|s| s.serving).map(|s| s.domain).collect())
    }
}
