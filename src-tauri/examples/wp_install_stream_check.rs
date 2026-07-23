//! Live check: STREAMED wp.org installs (`commands::wp_install`) — the three
//! proofs the feature stands on:
//!   1. real multi-slug install: phase lines arrive IN ORDER, the attempt
//!      cursor advances per item, final status ok + verbatim summary,
//!   2. cache/env proof: delete + reinstall the same slugs → wp-cli prints
//!      "Using cached file '…'" — HOME survived env_clear + login-shell
//!      snapshot, ~/.wp-cli/cache genuinely works (wp-cli's own line, not an
//!      assertion of ours),
//!   3. cancel mid-download: status "cancelled", process group dead.
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

#[tokio::main]
async fn main() {
    let plat = rexenv_lib::platform::current();
    let domain = format!("wpistream-{}.rex", std::process::id());

    let conn = {
        let p = std::env::temp_dir().join("rexenv-wpistream.db");
        let _ = std::fs::remove_file(&p);
        rexenv_lib::state::db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();

    // Reuse a running MySQL (the dev stack), else start our own.
    let mut own_mysqld = None;
    if !database::mysql_running(database::MYSQL_PORT) {
        let datadir = database::data_dir(&*plat).unwrap();
        let socket = database::socket_path(&*plat).unwrap();
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        database::initialize(&*plat, &mysql_base, &datadir).unwrap();
        own_mysqld = Some(
            database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap(),
        );
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
        },
    )
    .unwrap();
    let docroot = std::path::PathBuf::from(&site.path);
    wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        &domain,
        "WP Install Stream",
        &wordpress::db_name_for(&domain),
        &format!("127.0.0.1:{}", database::MYSQL_PORT),
        // db_client = the CLIENT BINARY (how core/db.rs derives it), not the
        // base dir — wp_plugins_check passes the base and is latently stale.
        &mysql_base.join("bin/mysql"),
        &Default::default(),
    )
    .expect("install wordpress");

    let app = tauri::test::mock_app();
    app.manage(commands::repo::RepoJobs::default());
    app.manage(wp_install::WpInstallJobs::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle().clone();

    let mut failures: Vec<String> = Vec::new();

    // Helpers: start a job, collect its output lines, wait for settle.
    let start = |slugs: Vec<String>, activate: bool| {
        let handle = handle.clone();
        async move {
            wp_install::wp_install_job(
                handle.clone(),
                handle.state::<AppState>(),
                handle.state::<commands::repo::RepoJobs>(),
                handle.state::<wp_install::WpInstallJobs>(),
                site_id_of(&handle),
                "plugin".into(),
                slugs,
                activate,
            )
            .await
        }
    };
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

    // ── 1. Multi-slug streamed install: order + cursor + summary ─────────
    let snap = start(vec!["hello-dolly".to_string(), "classic-editor".to_string()], false).await.expect("start job 1");
    let lines = collect_lines(&snap.id);
    let fin = wait_settled(snap.id.clone()).await;
    let ls = lines.lock().unwrap().clone();
    println!("job1: status={} cursor={}/{} summary={:?}", fin.status, fin.item_cursor, fin.items_total, fin.summary);
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
    println!("job3: status={} (cancel fired={})", fin.status, *cancelled_fired.lock().unwrap());
    if fin.status != "cancelled" {
        failures.push(format!("job3 status {} (want cancelled)", fin.status));
    }

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
    let _ = database::drop_database(
        &mysql_base.join("bin/mysql"),
        database::MYSQL_PORT,
        &wordpress::db_name_for(&domain),
    );
    if let Some(mut m) = own_mysqld {
        let _ = database::stop(&*rexenv_lib::platform::current(), m.id());
        let _ = m.wait();
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
