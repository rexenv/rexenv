//! WordPress live ↔ local sync (`docs/PLAN-wp-live-sync.md`) — the client half
//! of the rexsync1 protocol (`docs/rexsync-protocol.md`).
//!
//! Built so far: the pairing key and request signing (L5's pure core), checked
//! against the vectors file the plugin's tests read too. Nothing calls it yet:
//! the HTTP client, the jobs and the UI are later tasks of the plan.

pub mod sign;

pub mod client;
pub mod pull;
pub mod secrets;
