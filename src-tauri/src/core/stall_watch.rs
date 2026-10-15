//! core::stall_watch — says in the log when one of the app's two serial lanes stalls.
//!
//! Every `async` command runs on the async runtime; every IPC message from the webview
//! arrives on the main thread (and a non-`async` command runs there, inline). A long
//! blocking call on either lane delays everything queued behind it — and nothing in the
//! log says so: the command that waited logs its own first line only once it finally
//! runs. The Live tab's Check status (14 Oct 2026) was the case that needed this: its
//! body took 3–11 s on every machine, but on the Dell and both VMs it STARTED 7–110 s
//! after the click, with no line in the log for the gap.
//!
//! The rule is pure (`lag_of`) so it is testable; the two watchers are wired from the app
//! shell (`lib.rs`), which owns the `AppHandle` the main-thread probe needs.

use std::time::Duration;

/// How often each lane is probed.
pub const TICK: Duration = Duration::from_secs(2);

/// A probe that lands this much later than its tick is a stall worth a line.
pub const STALL: Duration = Duration::from_millis(1500);

/// A main-thread probe that has not run after this long is reported as "still
/// stalled" before its final lag is known.
pub const OVERDUE: Duration = Duration::from_secs(10);

/// The lag a probe measured, if it is a stall: `elapsed` is how long the probe took
/// to land, `expected` what it should have taken (its tick, or 0 for a closure queued
/// to run at once). Under `STALL` → `None`.
pub fn lag_of(expected: Duration, elapsed: Duration) -> Option<Duration> {
    let lag = elapsed.checked_sub(expected)?;
    (lag >= STALL).then_some(lag)
}

/// The log line for a stall on `lane` ("async runtime" / "main thread").
pub fn line(lane: &str, lag: Duration) -> String {
    format!("stall: the {lane} was busy for {:.1}s beyond its tick — every command queued on it waited that long", lag.as_secs_f32())
}

/// Watch the async runtime from inside it: sleep one tick, measure how late the wake
/// was. Runs for the life of the app.
pub async fn watch_runtime() {
    loop {
        let t = std::time::Instant::now();
        tokio::time::sleep(TICK).await;
        if let Some(lag) = lag_of(TICK, t.elapsed()) {
            log::warn!("{}", line("async runtime", lag));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A probe that lands within `STALL` of its tick is not a stall; later is, by the
    /// amount beyond the tick.** Plant: `lag >= STALL` → `lag > TICK` and the 1.6 s leg fails.
    #[test]
    fn a_stall_is_the_lag_beyond_the_tick() {
        assert_eq!(lag_of(TICK, TICK), None);
        assert_eq!(lag_of(TICK, TICK + Duration::from_millis(1400)), None);
        assert_eq!(lag_of(TICK, TICK + Duration::from_millis(1600)), Some(Duration::from_millis(1600)));
        assert_eq!(lag_of(Duration::ZERO, Duration::from_secs(40)), Some(Duration::from_secs(40)));
        // A probe that lands EARLY (clock skew) is not a stall either.
        assert_eq!(lag_of(TICK, Duration::from_secs(1)), None);
        assert!(line("main thread", Duration::from_secs(3)).contains("main thread was busy for 3.0s"));
    }
}
