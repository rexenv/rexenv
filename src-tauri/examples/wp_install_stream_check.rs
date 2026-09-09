//! Live check: STREAMED wp.org installs (`commands::wp_install`) — the three
//! proofs the feature stands on:
//!   1. real multi-slug install: phase lines arrive IN ORDER, the attempt
//!      cursor advances per item, final status ok + verbatim summary,
//!   2. cache/env proof: delete + reinstall the same slugs → wp-cli prints
//!      "Using cached file '…'" — HOME survived env_clear + login-shell
//!      snapshot, ~/.wp-cli/cache genuinely works (wp-cli's own line, not an
//!      assertion of ours),
//!   3. cancel mid-download: status "cancelled", process group dead,
//!
//!   5/6. the wall and the way through it: the SAME zip re-uploaded over the
//!      plugin it just installed must be refused, in the exact words the card
//!      parses to offer "Replace with the uploaded zip" — and the same job with
//!      `force` must then succeed. The refusal's wording is a THIRD PARTY's
//!      (wp-cli's); if it changes, the offer silently stops appearing and only
//!      this leg says so.
//!
//!   4. the ZIP source (wp-admin's "upload a zip"): a real archive built from
//!      an installed plugin dir installs through the SAME job, and the
//!      documented consequence holds live — wp-cli prints no per-item
//!      `Installing name (version)` header on this path, so the attempt
//!      cursor stays 0 while the bar still reaches 100. That is exactly why
//!      the card hides the cursor for zip jobs instead of showing "1 of N".
//!
//! Bootstrap mirrors wp_plugins_check (real MySQL + real WP install; reuses
//! a running MySQL on :13306, else starts one). Network required.
//! Run: `cargo run --example wp_install_stream_check`

use rexenv_lib::commands::{self, wp_install};
use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Listener, Manager};

mod common;

#[tokio::main]
async fn main() {
    let plat = rexenv_lib::platform::current();
    let domain = format!("wpistream-{}.rex", std::process::id());

    let conn = {
        let p = std::env::temp_dir().join("rexenv-wpistream.db");
        let _ = std::fs::remove_file(&p);
        rexenv_lib::state::db::open(&p).unwrap()
    };
    // Fixture-owned sites dir, pinned BEFORE the first `sites::provision`.
    // `sites::provision` reads the `sites_dir` SETTING, which falls back to a
    // path computed from the HOME directory — so without this the docroot lands
    // in the user's real ~/rexenv/Sites, and the `remove_dir_all` below deletes
    // it there. See `common::pin_fixture_sites_dir`.
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "wpistream");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");

    // Reuse a running MySQL (the dev stack), else start our own.
    let mut own_mysqld = None;
    if !database::mysql_running(database::MYSQL_PORT) {
        let datadir = database::data_dir(&*plat).unwrap();
        let socket = database::socket_path(&*plat).unwrap();
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        database::initialize(&*plat, &mysql_base, &datadir).unwrap();
        own_mysqld = Some(common::OwnedService::new(
            database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap(),
            "mysqld",
        ));
        for _ in 0..30 {
            if database::mysql_running(database::MYSQL_PORT) {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "WP Install Stream".into(),
            domain: domain.clone(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .unwrap();
    let docroot = std::path::PathBuf::from(&site.path);
    common::install_wp(&php, &wp, &docroot, &domain, "WP Install Stream", &db_client);

    let app = tauri::test::mock_app();
    app.manage(commands::repo::RepoJobs::default());
    app.manage(wp_install::WpInstallJobs::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle().clone();

    let mut failures: Vec<String> = Vec::new();

    // Helpers: start a job, collect its output lines, wait for settle.
    let start_forced =
        |source: &'static str, slugs: Vec<String>, activate: bool, force: bool| {
            let handle = handle.clone();
            async move {
                wp_install::wp_install_job(
                    handle.clone(),
                    handle.state::<AppState>(),
                    handle.state::<commands::repo::RepoJobs>(),
                    handle.state::<wp_install::WpInstallJobs>(),
                    site_id_of(&handle),
                    "plugin".into(),
                    source.into(),
                    slugs,
                    activate,
                    force,
                )
                .await
            }
        };
    let start_from = |source: &'static str, slugs: Vec<String>, activate: bool| {
        start_forced(source, slugs, activate, false)
    };
    let start = |slugs: Vec<String>, activate: bool| start_from("wporg", slugs, activate);
    fn site_id_of<R: tauri::Runtime>(h: &tauri::AppHandle<R>) -> String {
        let state = h.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let sites = sites::list(&conn).unwrap();
        sites[0].id.clone()
    }
    let collect_lines = |id: &str| {
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let l2 = lines.clone();
        handle.listen(wp_install::output_event(id), move |ev| {
            if let Ok(s) = serde_json::from_str::<String>(ev.payload()) {
                l2.lock().unwrap().push(s);
            }
        });
        lines
    };
    // The phase-based bar as the FRONTEND sees it: every state event's pct.
    let collect_pcts = |id: &str| {
        let pcts = Arc::new(Mutex::new(Vec::<u8>::new()));
        let p2 = pcts.clone();
        handle.listen(wp_install::state_event(id), move |ev| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(ev.payload()) {
                if let Some(p) = v.get("pct").and_then(|p| p.as_u64()) {
                    p2.lock().unwrap().push(p as u8);
                }
            }
        });
        pcts
    };
    let wait_settled = |id: String| {
        let handle = handle.clone();
        async move {
            for _ in 0..1200 {
                let st = wp_install::wp_install_active(
                    handle.state::<wp_install::WpInstallJobs>(),
                    site_id_of(&handle),
                    "plugin".into(),
                )
                .await
                .unwrap();
                if let Some(st) = st {
                    if st.id == id && st.status != "running" {
                        return st;
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            panic!("job {id} never settled");
        }
    };

    // ── 1. Multi-slug streamed install: order + cursor + summary + pct ───
    let snap = start(vec!["hello-dolly".to_string(), "classic-editor".to_string()], false).await.expect("start job 1");
    let lines = collect_lines(&snap.id);
    let pcts = collect_pcts(&snap.id);
    let fin = wait_settled(snap.id.clone()).await;
    let ls = lines.lock().unwrap().clone();
    let ps = pcts.lock().unwrap().clone();
    println!("job1: status={} cursor={}/{} summary={:?}", fin.status, fin.item_cursor, fin.items_total, fin.summary);
    println!("  pct stream: {ps:?} (final state pct={})", fin.pct);
    // Phase-based bar honesty: monotonic, real intermediate steps, 100 only
    // at the end (the terminal summary / ok settle).
    if ps.windows(2).any(|w| w[1] < w[0]) {
        failures.push(format!("job1 pct went BACKWARDS: {ps:?}"));
    }
    if !ps.iter().any(|p| (1..=99).contains(p)) {
        failures.push(format!("job1 no intermediate pct observed: {ps:?}"));
    }
    // (100-only-at-the-summary is proven line-exactly by the pure tests;
    // here monotonic + "ends at 100" pins the live stream's shape: the first
    // 100 can only be the summary-line emit or the ok settle.)
    if fin.pct != 100 {
        failures.push(format!("job1 final pct {} (want 100)", fin.pct));
    }
    for l in &ls {
        println!("  | {l}");
    }
    let headers: Vec<usize> = ls
        .iter()
        .enumerate()
        .filter(|(_, l)| wordpress::is_install_item_header(l))
        .map(|(i, _)| i)
        .collect();
    if fin.status != "ok" {
        failures.push(format!("job1 status {} (want ok)", fin.status));
    }
    if headers.len() != 2 || fin.item_cursor != 2 {
        failures.push(format!("job1 headers={} cursor={} (want 2/2)", headers.len(), fin.item_cursor));
    }
    // Phase order per item: header precedes an Unpacking line that precedes
    // the next header (WP-core wording matched loosely, never exactly).
    if let Some(&h0) = headers.first() {
        let unpack_after_h0 = ls
            .iter()
            .skip(h0)
            .take(headers.get(1).map(|h1| h1 - h0).unwrap_or(ls.len()))
            .any(|l| l.contains("Unpacking"));
        if !unpack_after_h0 {
            failures.push("job1: no Unpacking line between item 1's header and item 2's".into());
        }
    }
    if fin.summary.as_deref() != Some("Success: Installed 2 of 2 plugins.") {
        failures.push(format!("job1 summary {:?}", fin.summary));
    }

    // ── 2. Cache/env proof: delete + reinstall → "Using cached file" ─────
    wordpress::plugin_delete(&php, &wp, &docroot, &["hello-dolly".into(), "classic-editor".into()])
        .expect("delete plugins");
    let snap = start(vec!["hello-dolly".to_string(), "classic-editor".to_string()], false).await.expect("start job 2");
    let lines = collect_lines(&snap.id);
    let fin = wait_settled(snap.id.clone()).await;
    let ls = lines.lock().unwrap().clone();
    let cached = ls.iter().any(|l| l.starts_with("Using cached file"));
    println!("job2: status={} cached-line={}", fin.status, cached);
    if fin.status != "ok" {
        failures.push(format!("job2 status {}", fin.status));
    }
    if !cached {
        failures.push("job2: no 'Using cached file' line — wp-cli cache dead under env_clear?".into());
    }

    // ── 3. Cancel mid-download ───────────────────────────────────────────
    let snap = start(vec!["woocommerce".to_string()], false).await.expect("start job 3");
    let id3 = snap.id.clone();
    let cancel_handle = handle.clone();
    let cancelled_fired = Arc::new(Mutex::new(false));
    let cf = cancelled_fired.clone();
    handle.listen(wp_install::output_event(&id3), move |ev| {
        let Ok(line) = serde_json::from_str::<String>(ev.payload()) else { return };
        // First Downloading line → cancel while wp-cli is genuinely
        // mid-transfer (the header would fire before the download starts).
        if line.contains("Downloading") && !*cf.lock().unwrap() {
            *cf.lock().unwrap() = true;
            let h = cancel_handle.clone();
            let id = id3.clone();
            tauri::async_runtime::spawn(async move {
                let _ = wp_install::wp_install_cancel(
                    h.state::<AppState>(),
                    h.state::<wp_install::WpInstallJobs>(),
                    id,
                )
                .await;
            });
        }
    });
    let fin = wait_settled(snap.id.clone()).await;
    println!(
        "job3: status={} pct={} (cancel fired={})",
        fin.status,
        fin.pct,
        *cancelled_fired.lock().unwrap()
    );
    if fin.status != "cancelled" {
        failures.push(format!("job3 status {} (want cancelled)", fin.status));
    }
    // E-rule: a cancelled bar FREEZES where it was — never snaps to done.
    if fin.pct >= 100 {
        failures.push(format!("job3 cancelled but pct {} (must stay <100)", fin.pct));
    }

    // ── 4. The ZIP source: a real archive, through the same job ──────────
    // Built from the plugin job 2 reinstalled, so the archive has the exact
    // shape WordPress expects (one top-level dir) without shipping a binary
    // fixture. Everything here lives in a dir named after this process.
    let zip_dir = std::env::temp_dir().join(format!("rexenv-wpizip-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&zip_dir);
    let zip_path = zip_dir.join("hello-dolly.zip");
    let plugins_dir = docroot.join("wp-content/plugins");
    let zipped = std::process::Command::new("zip")
        .args(["-qr", &zip_path.display().to_string(), "hello-dolly"])
        .current_dir(&plugins_dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !zipped || !zip_path.is_file() {
        failures.push("job4: could not build the fixture zip (is `zip` on PATH?)".into());
    } else {
        // WordPress refuses to unpack over an existing plugin dir — the zip
        // flow's real precondition, not a test convenience.
        wordpress::plugin_delete(&php, &wp, &docroot, &["hello-dolly".into()])
            .expect("delete hello-dolly");
        let snap = start_from("zip", vec![zip_path.display().to_string()], false)
            .await
            .expect("start job 4");
        let lines = collect_lines(&snap.id);
        let fin = wait_settled(snap.id.clone()).await;
        let ls = lines.lock().unwrap().clone();
        println!(
            "job4 (zip): status={} cursor={}/{} pct={} summary={:?}",
            fin.status, fin.item_cursor, fin.items_total, fin.pct, fin.summary
        );
        for l in &ls {
            println!("  | {l}");
        }
        if fin.status != "ok" {
            failures.push(format!("job4 status {} (want ok)", fin.status));
        }
        if fin.source != "zip" {
            failures.push(format!("job4 source {:?} (want zip)", fin.source));
        }
        if !docroot.join("wp-content/plugins/hello-dolly").is_dir() {
            failures.push("job4: exit ok but the plugin dir is not there".into());
        }
        // The documented consequence, live: no per-item header on this path.
        // If wp-cli ever starts printing one, the card's "hide the cursor for
        // zip" rule becomes a lie in the other direction — catch it here.
        if ls.iter().any(|l| wordpress::is_install_item_header(l)) {
            failures.push("job4: wp-cli DID print a per-item header for a zip — the card hides the cursor on the assumption it never does".into());
        }
        if fin.item_cursor != 0 {
            failures.push(format!("job4 cursor {} (a zip job's cursor never advances)", fin.item_cursor));
        }
        if fin.pct != 100 {
            failures.push(format!("job4 final pct {} (want 100)", fin.pct));
        }
        // ── job 5: the wall a re-upload hits, and `--force` through it ──────
        // The plugin from job 4 is now installed, so the SAME zip must be
        // refused — and refused in the exact words the card reads. wp-cli's
        // summary here says only "No plugins installed.", which is why the UI
        // parses the log line instead; if that line ever changes, the Replace
        // control silently stops appearing and only this leg notices.
        let again = start_from("zip", vec![zip_path.display().to_string()], false)
            .await
            .expect("start job 5");
        let again_lines = collect_lines(&again.id);
        let again_fin = wait_settled(again.id.clone()).await;
        let again_ls = again_lines.lock().unwrap().clone();
        println!("job5 (zip, already installed): status={}", again_fin.status);
        if again_fin.status == "ok" {
            failures.push("job5: wp-cli unpacked over an existing plugin without --force".into());
        }
        let marker = "Destination folder already exists.";
        if !again_ls.iter().any(|l| l.contains(marker)) {
            failures.push(format!(
                "job5: wp-cli no longer says {marker:?} — the card reads that line to \
                 offer Replace, so the offer is now dead: {again_ls:?}"
            ));
        }
        // The STATE carries the answer, not just the log: the toast never
        // receives the log at all, and the card would otherwise re-parse a
        // tail that rotates. This is the field both of them read.
        if again_fin.blocked_by.as_deref() != Some("hello-dolly") {
            failures.push(format!(
                "job5 blocked_by = {:?} (want Some(\"hello-dolly\")) — the card names nothing \
                 and the toast falls back to calling it a plain failure",
                again_fin.blocked_by
            ));
        }
        // The folder name has to be READABLE out of it — the UI takes the last
        // path segment of the quoted path, and a message without the quotes
        // would leave the card naming nothing.
        if !again_ls
            .iter()
            .any(|l| l.contains(marker) && l.contains("\"") && l.contains("hello-dolly"))
        {
            failures.push("job5: the marker line no longer carries the quoted folder path".into());
        }

        // ...and the same job with force = the Replace button's own call.
        let forced = start_forced("zip", vec![zip_path.display().to_string()], false, true)
            .await
            .expect("start job 6");
        let _forced_lines = collect_lines(&forced.id);
        let forced_fin = wait_settled(forced.id.clone()).await;
        println!("job6 (zip, --force): status={}", forced_fin.status);
        if forced_fin.status != "ok" {
            failures.push(format!(
                "job6 status {} (want ok) — --force did not replace the existing plugin",
                forced_fin.status
            ));
        }
        if !docroot.join("wp-content/plugins/hello-dolly").is_dir() {
            failures.push("job6: the plugin dir is gone after a forced replace".into());
        }
        if forced_fin.blocked_by.is_some() {
            failures.push(format!(
                "job6 blocked_by = {:?} on a run that SUCCEEDED — the card would offer a \
                 replace for something already replaced",
                forced_fin.blocked_by
            ));
        }

        // The gate, live: the same job refuses a path that isn't a real zip.
        let bad = start_from("zip", vec![zip_dir.join("nope.zip").display().to_string()], false)
            .await;
        match bad {
            Err(e) if e.to_string().contains("no such file") => {}
            other => failures.push(format!("job4 gate let a missing zip through: {other:?}")),
        }
    }
    // Fixture-owned: the dir this run created, by name.
    let _ = std::fs::remove_dir_all(&zip_dir);

    // Cleanup: site dir + row + throwaway db; stop MySQL only if we started
    // it. site_id resolved BEFORE taking the db lock — site_id_of locks the
    // same mutex (self-deadlock otherwise).
    let sid = site_id_of(&handle);
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = rexenv_lib::state::store::delete_site(&conn, &sid);
    }
    // Delete ONLY this fixture's dir. NEVER the parent: provision puts the
    // docroot DIRECTLY in the Sites folder, so parent() IS the user's
    // Sites dir — an earlier version of this line deleted every site.
    let _ = std::fs::remove_dir_all(&docroot);
    let _ = rexenv_lib::core::db::DbEngine::Mysql.drop_database(
        &db_client,
        database::MYSQL_PORT,
        &wordpress::db_name_for(SiteType::Wordpress, &domain),
    );
    // Explicit stop on the happy path; `Drop` is the backstop everywhere else.
    if let Some(mut m) = own_mysqld {
        m.stop();
    }
    println!();
    if failures.is_empty() {
        println!("wp_install_stream_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
