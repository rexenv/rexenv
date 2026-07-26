//! Live check: STREAMED site provisioning (`commands::site_provision`) — the
//! three proofs the feature stands on:
//!   1. happy path: a real WordPress create streams its phases IN ORDER, the
//!      pct is monotonic with real intermediate values, settles ok at 100
//!      with the honest stack-stopped summary, and flips `provisioned=1`,
//!   2. deterministic failure + RETRY: a bogus `--locale` makes
//!      `wp core download` fail → status failed at that phase, pct FROZEN,
//!      `provisioned=0` — then `site_provision_retry` (default options)
//!      re-enters the idempotent steps and completes → `provisioned=1`.
//!      Retry-on-idempotent-install is the whole basis for "keep + badge"
//!      over auto-rollback,
//!   3. cancel: flag set mid-run → job settles "cancelled" at the next
//!      boundary, pct frozen <100, `provisioned=0`.
//!
//! Edge safety: the fixture manager adopts ONLY the database tier
//! (`adopt_dbs`) — never the edge/nginx — so `is_running()` stays false and
//! the serve phase is SKIPPED: this example can never rebuild the real
//! stack's vhosts from its throwaway database.
//! Run: `cargo run --example site_provision_check`

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Listener, Manager};

#[tokio::main]
async fn main() {
    let plat = rexenv_lib::platform::current();
    let pid = std::process::id();

    let conn = {
        let p = std::env::temp_dir().join("rexenv-provision-check.db");
        let _ = std::fs::remove_file(&p);
        rexenv_lib::state::db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();

    let app = tauri::test::mock_app();
    app.manage(commands::repo::RepoJobs::default());
    app.manage(site_provision::ProvisionJobs::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle().clone();

    // DB tier ONLY — see module doc. If nothing is adopted (stack stopped),
    // the job's own spawn_db starts the engine; it outlives the example (the
    // app adopts it on next launch — by design, services outlive processes).
    {
        let state = handle.state::<AppState>();
        let mut mgr = state.services.lock().await;
        let n = mgr.adopt_dbs(state.platform.as_ref());
        println!("adopted {n} running database engine(s)");
    }

    let mut failures: Vec<String> = Vec::new();
    // Fixture-owned resources, recorded AS CREATED (the cleanup rule: delete
    // only what this run created, from its own records — never derived paths).
    let created_site_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let new_site = |domain: &str| NewSite {
        name: format!("Provision Check {domain}"),
        domain: domain.into(),
        site_type: SiteType::Wordpress,
        php_version: "8.3".into(),
        web_server: WebServer::Nginx,
        path: String::new(),
        db_engine: SiteDbEngine::Mysql,
    };
    let collect = |id: &str| {
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let pcts = Arc::new(Mutex::new(Vec::<u8>::new()));
        let (l2, p2) = (lines.clone(), pcts.clone());
        handle.listen(site_provision::output_event(id), move |ev| {
            if let Ok(s) = serde_json::from_str::<String>(ev.payload()) {
                l2.lock().unwrap().push(s);
            }
        });
        handle.listen(site_provision::state_event(id), move |ev| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(ev.payload()) {
                if let Some(p) = v.get("pct").and_then(|p| p.as_u64()) {
                    p2.lock().unwrap().push(p as u8);
                }
            }
        });
        (lines, pcts)
    };
    let wait_settled = |id: String| {
        let handle = handle.clone();
        async move {
            for _ in 0..1200 {
                let st = site_provision::state_of(
                    &handle.state::<site_provision::ProvisionJobs>(),
                    &id,
                )
                .unwrap();
                if st.status != "running" {
                    return st;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            panic!("job {id} never settled");
        }
    };
    let provisioned_of = |site_id: &str| {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        sites::get(&conn, site_id).unwrap().map(|s| s.provisioned)
    };

    // ── 1. Happy path: phases in order, monotonic pct, ok@100, flag flip ──
    let domain_a = format!("wpprova-{pid}.rex");
    let snap = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        new_site(&domain_a),
        None,
        None,
    )
    .await
    .expect("start job A");
    created_site_ids.lock().unwrap().extend(snap.site_id.clone());
    let (lines, pcts) = collect(&snap.id);
    let fin = wait_settled(snap.id.clone()).await;
    let ls = lines.lock().unwrap().clone();
    let ps = pcts.lock().unwrap().clone();
    println!("job A: status={} pct={} summary={:?}", fin.status, fin.pct, fin.summary);
    println!("  pct stream: {ps:?}");
    for l in &ls {
        println!("  | {l}");
    }
    if fin.status != "ok" || fin.pct != 100 {
        failures.push(format!("job A settled {}@{} (want ok@100)", fin.status, fin.pct));
    }
    // Phase markers in execution order (our OWN boundaries, no parsing).
    let marker_order = ["── downloading binaries", "── starting database", "── downloading WordPress core", "── writing wp-config", "── installing WordPress", "── starting to serve"];
    let mut at = 0usize;
    for m in marker_order {
        match ls.iter().skip(at).position(|l| l.starts_with(m)) {
            Some(i) => at += i,
            None => failures.push(format!("job A: marker {m:?} missing or out of order")),
        }
    }
    if ps.windows(2).any(|w| w[1] < w[0]) {
        failures.push(format!("job A pct went BACKWARDS: {ps:?}"));
    }
    if !ps.iter().any(|p| (1..=99).contains(p)) {
        failures.push(format!("job A no intermediate pct: {ps:?}"));
    }
    if !fin.summary.as_deref().unwrap_or_default().contains("stack is stopped") {
        failures.push(format!(
            "job A summary must carry the honest stack-stopped note: {:?}",
            fin.summary
        ));
    }
    match fin.site_id.as_deref().and_then(provisioned_of) {
        Some(true) => {}
        got => failures.push(format!("job A provisioned flag = {got:?} (want Some(true))")),
    }

    // ── 2. Deterministic failure at core_download → RETRY completes ──────
    let domain_b = format!("wpprovb-{pid}.rex");
    let bad_locale = wordpress::InstallOptions { language: "xx_XX".into(), ..Default::default() };
    let snap = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        new_site(&domain_b),
        Some(bad_locale),
        None,
    )
    .await
    .expect("start job B");
    created_site_ids.lock().unwrap().extend(snap.site_id.clone());
    let site_b = snap.site_id.clone().expect("job B site id");
    let fin = wait_settled(snap.id.clone()).await;
    println!("job B: status={} pct={} error={:?}", fin.status, fin.pct, fin.error);
    if fin.status != "failed" || fin.pct >= 100 {
        failures.push(format!("job B settled {}@{} (want failed, pct frozen <100)", fin.status, fin.pct));
    }
    let dl_phase = fin.phases.iter().find(|p| p.key == "core_download");
    if dl_phase.map(|p| p.status.as_str()) != Some("failed") {
        failures.push(format!("job B core_download phase = {dl_phase:?} (want failed)"));
    }
    if provisioned_of(&site_b) != Some(false) {
        failures.push("job B: provisioned must stay 0 after a failed provision".into());
    }
    // RETRY: default options (no bogus locale) re-enter the idempotent steps.
    let snap = site_provision::site_provision_retry(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        site_b.clone(),
    )
    .await
    .expect("start retry");
    let fin = wait_settled(snap.id.clone()).await;
    println!("job B retry: status={} pct={}", fin.status, fin.pct);
    if fin.status != "ok" || fin.pct != 100 {
        failures.push(format!("job B retry settled {}@{} (want ok@100)", fin.status, fin.pct));
    }
    if provisioned_of(&site_b) != Some(true) {
        failures.push("job B retry: provisioned must flip to 1 on settle-ok".into());
    }
    // A retry of a COMPLETE site must refuse (nothing to retry).
    let refused = site_provision::site_provision_retry(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        site_b.clone(),
    )
    .await;
    if refused.is_ok() {
        failures.push("retry of a provisioned site must be refused".into());
    }

    // ── 3. Cancel: flag mid-run → cancelled at next boundary, frozen pct ──
    let domain_c = format!("wpprovc-{pid}.rex");
    let snap = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        new_site(&domain_c),
        None,
        None,
    )
    .await
    .expect("start job C");
    created_site_ids.lock().unwrap().extend(snap.site_id.clone());
    let site_c = snap.site_id.clone().expect("job C site id");
    // Cancel as soon as the core-download phase opens: whether the kill lands
    // mid-transfer or the (cached) step finishes first, the persistent flag
    // guarantees the job bails at the NEXT boundary — deterministic either way.
    let cancel_handle = handle.clone();
    let cid = snap.id.clone();
    let fired = Arc::new(Mutex::new(false));
    let f2 = fired.clone();
    handle.listen(site_provision::output_event(&snap.id), move |ev| {
        let Ok(line) = serde_json::from_str::<String>(ev.payload()) else { return };
        if line.starts_with("── downloading WordPress core") && !*f2.lock().unwrap() {
            *f2.lock().unwrap() = true;
            let (h, id) = (cancel_handle.clone(), cid.clone());
            tauri::async_runtime::spawn(async move {
                let _ = site_provision::site_provision_cancel(
                    h.state::<AppState>(),
                    h.state::<site_provision::ProvisionJobs>(),
                    id,
                )
                .await;
            });
        }
    });
    let fin = wait_settled(snap.id.clone()).await;
    println!("job C: status={} pct={} (cancel fired={})", fin.status, fin.pct, *fired.lock().unwrap());
    if fin.status != "cancelled" || fin.pct >= 100 {
        failures.push(format!("job C settled {}@{} (want cancelled, frozen <100)", fin.status, fin.pct));
    }
    if provisioned_of(&site_c) != Some(false) {
        failures.push("job C: provisioned must stay 0 after cancel".into());
    }

    // Cleanup — ONLY fixture-owned resources, from this run's own records:
    // each recorded site id → drop its DB (name from ITS row), then
    // `sites::teardown` (row + cert dir + docroot, managed-dir guarded).
    // NEVER a derived parent path (the 24 Jul Sites-folder incident).
    let ids = created_site_ids.lock().unwrap().clone();
    for id in ids {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        if let Ok(Some(site)) = sites::get(&conn, &id) {
            let _ = database::drop_database(
                &mysql_base.join("bin/mysql"),
                database::MYSQL_PORT,
                &site.db_name,
            );
            let plat = rexenv_lib::platform::current();
            match sites::teardown(&conn, &*plat, &id) {
                Ok(t) if t.existed => println!(
                    "cleaned up {} (docroot removed = {})",
                    site.domain, t.docroot_removed
                ),
                other => println!("teardown {} -> {other:?}", site.domain),
            }
        }
    }
    let _ = std::fs::remove_file(std::env::temp_dir().join("rexenv-provision-check.db"));

    println!();
    if failures.is_empty() {
        println!("site_provision_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
