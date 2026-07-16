//! Utility: stop an ORPHANED rexenv stack (services left running after the app
//! exited without Stop-all — e.g. a crashed dev session).
//! Run: `cargo run --example stack_stop`
//!
//! Uses the exact code the app runs on launch (`reconcile_startup`):
//!   - `stop_stale_owned` — TERMs any process still holding one of our managed
//!     loopback ports, gated to OUR binaries by the app-data path marker, so an
//!     unrelated process (system MySQL, DBngin, …) on the same port is never touched;
//!   - `proxy::stop_edge` — stops a leftover (even root) Caddy edge over OUR admin
//!     unix socket (no privilege needed), never TCP :2019, so a foreign Caddy is
//!     invisible to it.

use rexenv_lib::core::service_manager::ServiceManager;
use rexenv_lib::platform;

fn main() {
    // Deliberate real-stack control: this utility exists to adopt/stop the
    // shared stack. Without this, core::stack_guard skips adopted services.
    rexenv_lib::core::stack_guard::allow_real_stack_control();
    let plat = platform::current();
    let mgr = ServiceManager::default();
    mgr.reconcile_startup(&*plat);
    println!("OK — orphan sweep + edge stop dispatched (marker-gated to rexenv-owned processes).");
    println!("Verify: `ps aux | grep -E 'dev.rexenv' | grep -v grep` should list nothing.");
}
