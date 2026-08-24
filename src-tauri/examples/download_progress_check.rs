//! Live check for the download manager (download-manager step 1): real byte
//! progress streamed through `core::downloads::hub()` while a binary resolves.
//! Run: `cargo run --example download_progress_check`
//!
//! Moves the cached WP-CLI phar ASIDE (small, ~7MB), re-resolves it, and asserts
//! the hub reported a real progress sequence: downloading (bytes growing toward
//! a known Content-Length total) → preparing → done. Touches nothing but the
//! wp-cli cache dir — no services.
//!
//! It used to DELETE the entry, on the argument that everything in the binary
//! cache is re-fetchable by checksum. That holds while the network does. The
//! entry is renamed now and restored by `Drop` unless the run replaced it, so a
//! failed download — offline, or a registry outage like the GitHub 504 seen on
//! 21 Aug 2026 — leaves the user's cache exactly as it found it.

use rexenv_lib::core::binaries;
use rexenv_lib::core::downloads::{self, Phase};
use rexenv_lib::platform;

/// The cached entry, moved ASIDE for the duration and put back if this run does
/// not replace it.
///
/// **The old form deleted it outright**, and the comment on `SandboxPaths::bin`
/// argued that was safe "because every entry is re-fetchable by checksum". True
/// while the network is. Run this offline, or during a registry outage — a
/// GitHub 504 was hit on this machine on 21 Aug 2026 — and the user's wp-cli
/// phar is simply gone, with every WordPress action re-downloading it before it
/// can do anything. The example is network-tier, so that is unlikely rather than
/// impossible, and "unlikely" is not the standard for destroying somebody's
/// cache.
///
/// A RENAME, not a copy: same filesystem, atomic, and no second copy of the
/// bytes. `Drop` puts it back, so a panic mid-run restores it too — the same
/// reason `common::OwnedService` reaps in `Drop` rather than at the end of main.
struct StashedCache {
    live: std::path::PathBuf,
    stash: std::path::PathBuf,
    stashed: bool,
}

impl StashedCache {
    fn take(live: std::path::PathBuf) -> Self {
        let stash = live.with_extension(format!("taken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&stash);
        let stashed = std::fs::rename(&live, &stash).is_ok();
        Self { live, stash, stashed }
    }

    /// The run replaced it: the stash is now the OLD copy and can go.
    fn superseded(&mut self) {
        if self.stashed {
            let _ = std::fs::remove_dir_all(&self.stash);
            self.stashed = false;
        }
    }
}

impl Drop for StashedCache {
    fn drop(&mut self) {
        if !self.stashed {
            return;
        }
        // Only restore if the run did NOT produce a fresh entry — putting the
        // old one back over a good new one would undo the thing being tested.
        if self.live.exists() {
            let _ = std::fs::remove_dir_all(&self.stash);
        } else {
            let _ = std::fs::rename(&self.stash, &self.live);
            eprintln!(
                "download_progress_check: the re-download did not complete — the cached \
                 wp-cli was put back at {}",
                self.live.display()
            );
        }
    }
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let bin_dir = plat.paths().bin_dir().expect("bin dir");
    let cache = bin_dir.join(format!("wp-cli-{}", binaries::WP_CLI_VERSION));
    let mut stash = StashedCache::take(cache.clone());
    println!("cleared cache: {} (kept aside until the re-download lands)", cache.display());

    // Watch the hub while the resolve runs; record the phase sequence and the
    // byte high-water mark, printing transitions + quartile progress.
    let mut rx = downloads::hub().subscribe();
    let id = downloads::item_id("wp-cli", binaries::WP_CLI_VERSION);
    let watcher = tokio::spawn(async move {
        let mut phases: Vec<Phase> = Vec::new();
        let mut max_bytes = 0u64;
        let mut total: Option<u64> = None;
        let mut saw_rate = false;
        let mut last_quart = 0u64;
        while rx.changed().await.is_ok() {
            let snap = downloads::hub().snapshot();
            let Some(item) = snap.items.iter().find(|i| i.id == id) else {
                continue;
            };
            if phases.last() != Some(&item.phase) {
                phases.push(item.phase);
                println!(
                    "  phase={:?} bytes={} total={:?}",
                    item.phase, item.downloaded_bytes, item.total_bytes
                );
            }
            max_bytes = max_bytes.max(item.downloaded_bytes);
            if item.total_bytes.is_some() {
                total = item.total_bytes;
            }
            saw_rate |= item.bytes_per_sec.is_some();
            if let Some(t) = item.total_bytes {
                let quart = item.downloaded_bytes * 4 / t.max(1);
                if quart > last_quart {
                    last_quart = quart;
                    println!(
                        "  {:>3}%  {} / {} bytes  rate={:?} B/s",
                        item.downloaded_bytes * 100 / t.max(1),
                        item.downloaded_bytes,
                        t,
                        item.bytes_per_sec
                    );
                }
            }
            if matches!(item.phase, Phase::Done | Phase::Failed) {
                if item.phase == Phase::Failed {
                    println!("  error={:?}", item.error);
                }
                break;
            }
        }
        (phases, max_bytes, total, saw_rate)
    });

    let path = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION)
        .await
        .expect("resolve wp-cli");
    let (phases, max_bytes, total, saw_rate) = watcher.await.expect("watcher task");

    println!("resolved: {}", path.display());
    println!("phase sequence: {phases:?}");

    assert!(path.exists(), "resolved phar missing on disk");
    assert!(
        phases.contains(&Phase::Downloading)
            && phases.contains(&Phase::Preparing)
            && phases.last() == Some(&Phase::Done),
        "expected downloading → preparing → done, got {phases:?}"
    );
    let total = total.expect("GitHub sends Content-Length — total should be known");
    assert!(total > 1_000_000, "phar total suspiciously small: {total}");
    assert_eq!(
        max_bytes, total,
        "download should have streamed to exactly the announced total"
    );
    let on_disk = std::fs::metadata(&path).expect("stat phar").len();
    assert_eq!(on_disk, total, "bytes on disk != announced total");
    // Every assertion above passed AND a fresh entry is on disk, so the old copy
    // is genuinely superseded. Placed here rather than right after the resolve:
    // an assertion failing between the two should still put the cache back.
    stash.superseded();
    println!(
        "OK — streamed {max_bytes} bytes to disk with live progress (rate observed: {saw_rate})"
    );
}
