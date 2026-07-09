//! Live check for the download manager (download-manager step 1): real byte
//! progress streamed through `core::downloads::hub()` while a binary resolves.
//! Run: `cargo run --example download_progress_check`
//!
//! Deletes the cached WP-CLI phar (small, ~7MB — safe: re-downloaded right
//! here), re-resolves it, and asserts the hub reported a real progress
//! sequence: downloading (bytes growing toward a known Content-Length total) →
//! preparing → done. Touches nothing but the wp-cli cache dir — no services.

use rexenv_lib::core::binaries;
use rexenv_lib::core::downloads::{self, Phase};
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let bin_dir = plat.paths().bin_dir().expect("bin dir");
    let cache = bin_dir.join(format!("wp-cli-{}", binaries::WP_CLI_VERSION));
    let _ = std::fs::remove_dir_all(&cache);
    println!("cleared cache: {}", cache.display());

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
    println!(
        "OK — streamed {max_bytes} bytes to disk with live progress (rate observed: {saw_rate})"
    );
}
