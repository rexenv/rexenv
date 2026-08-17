//! Live check for download-manager step 2: a cold "Start all" PLANS its binary
//! set, prefetches every missing one as a hub batch with real progress, and —
//! the lock point — the ServiceManager mutex stays uncontended the whole time
//! (status polls can't block), because prefetch never touches it.
//! Run: `cargo run --example prefetch_responsiveness_check`
//!
//! Mirrors the exact `start_services` command flow (plan → prefetch → only THEN
//! lock + start) against a manager behind the same async-Mutex pattern AppState
//! uses. Clears only the small binaries (caddy/nginx/mailpit/adminer, ~35MB —
//! re-downloaded right here); big cached trees (MySQL, php-fpm) stay and must
//! show up as `cached` rows that the batch does NOT count. No services are
//! started or stopped.

use rexenv_lib::core::service_manager::ServiceManager;
use rexenv_lib::core::{binaries, downloads};
use rexenv_lib::platform;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let bin_dir = plat.paths().bin_dir().expect("bin dir");

    // Go cold on the small binaries.
    for (name, version) in [
        ("caddy", binaries::CADDY_VERSION),
        ("nginx", binaries::NGINX_VERSION),
        ("mailpit", binaries::MAILPIT_VERSION),
        ("adminer", binaries::ADMINER_VERSION),
    ] {
        let dir = bin_dir.join(format!("{name}-{version}"));
        std::fs::remove_dir_all(&dir).ok();
        assert!(!binaries::is_cached(&*plat, name, version), "{name} still cached");
    }

    // 1) PLAN — full set resolved up front, split cached vs missing.
    // `_pinned`: this check has no app database, so there is no selection to floor.
    let plan = downloads::plan_for_start_pinned(&*plat, &[], &[], &Default::default());
    let missing: Vec<String> = plan.iter().filter(|p| !p.cached).map(|p| p.name.clone()).collect();
    let cached: Vec<String> = plan.iter().filter(|p| p.cached).map(|p| p.name.clone()).collect();
    println!("plan: {} binaries — missing {missing:?}, cached {cached:?}", plan.len());
    for n in ["caddy", "nginx", "mailpit", "adminer"] {
        assert!(missing.iter().any(|m| m == n), "{n} should be missing");
    }

    // 2) The services mutex, exactly as AppState holds it. A poller hammers
    //    try_lock + status() every 50ms — the app's status path. If prefetch
    //    held the lock anywhere, attempts would start failing.
    let services = Arc::new(tokio::sync::Mutex::new(ServiceManager::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let attempts = Arc::new(AtomicU64::new(0));
    let successes = Arc::new(AtomicU64::new(0));
    let poller = {
        let (services, stop, attempts, successes) =
            (services.clone(), stop.clone(), attempts.clone(), successes.clone());
        tokio::spawn(async move {
            let plat = platform::current();
            while !stop.load(Ordering::Relaxed) {
                attempts.fetch_add(1, Ordering::Relaxed);
                if let Ok(mgr) = services.try_lock() {
                    let _ = mgr.status(&*plat, &[]); // the real status read
                    successes.fetch_add(1, Ordering::Relaxed);
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
    };

    // 3) Watch the batch counter progress (0/N → N/N) while prefetching.
    let mut rx = downloads::hub().subscribe();
    let batch_log = tokio::spawn(async move {
        let mut seen: Vec<(usize, usize)> = Vec::new();
        while rx.changed().await.is_ok() {
            if let Some(b) = downloads::hub().snapshot().batch {
                if seen.last() != Some(&(b.done, b.total)) {
                    println!("  batch [{}] {}/{}", b.action, b.done, b.total);
                    seen.push((b.done, b.total));
                }
                if b.done == b.total {
                    break;
                }
            }
        }
        seen
    });

    // 4) PREFETCH — the lock is untouched by design; the poller proves it.
    downloads::prefetch(&*plat, "Start all", &plan).await.expect("prefetch");
    stop.store(true, Ordering::Relaxed);
    poller.await.unwrap();
    let seen = batch_log.await.unwrap();

    let (a, s) = (attempts.load(Ordering::Relaxed), successes.load(Ordering::Relaxed));
    println!("status polls during prefetch: {s}/{a} succeeded");
    assert!(a >= 10, "prefetch too fast to sample ({a} polls) — rerun");
    assert_eq!(s, a, "services lock was contended during downloads!");

    // Batch counted ONLY the missing items and finished them all.
    let last = *seen.last().expect("no batch snapshots");
    assert_eq!(last, (missing.len(), missing.len()), "batch end state");

    // Every planned binary is now a cache hit — the locked start phase that
    // would follow in start_services downloads nothing.
    for p in &plan {
        assert!(binaries::is_cached(&*plat, &p.name, &p.version), "{} not cached after prefetch", p.name);
    }
    let snap = downloads::hub().snapshot();
    assert!(snap.items.iter().all(|i| matches!(
        i.phase,
        downloads::Phase::Done | downloads::Phase::Cached
    )));
    println!(
        "OK — planned {} (cached rows visible, not counted), prefetched {} with live batch progress, services lock never contended.",
        plan.len(),
        missing.len()
    );
}
