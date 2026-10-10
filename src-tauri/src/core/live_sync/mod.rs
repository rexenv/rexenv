//! WordPress live ↔ local sync (`docs/PLAN-wp-live-sync.md`) — the client half
//! of the rexsync1 protocol (`docs/rexsync-protocol.md`).
//!
//! `sign` (the key and request signing, checked against the vectors the plugin's
//! tests read too), `client` (the HTTP side), `pull`/`push` (the two jobs' cores),
//! `base` (what live looked like at the last sync), `secrets` (the owner-only
//! pairing file), `plugin_zip` (the plugin itself, for "Download plugin"). The
//! IPC edge is `commands::live_sync`.

pub mod sign;

pub mod client;
pub mod pull;
pub mod secrets;
pub mod base;
pub mod push;
pub mod plugin_zip;
