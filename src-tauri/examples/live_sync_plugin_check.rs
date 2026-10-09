//! Live check: the rexenv Sync plugin INSIDE a real WordPress (L3/L4 of
//! `docs/PLAN-wp-live-sync.md`). Run: `cargo run --example live_sync_plugin_check`
//!
//! Provisions a fixture WordPress through the real job (sites dir pinned to a
//! fixture, database tier adopted only — the edge is never touched), copies
//! `companion/rexenv-sync` into its plugins, activates it with the bundled
//! wp-cli, and runs `tests/integration-test.php` with `wp eval-file`. That script
//! goes through WordPress's OWN REST dispatcher (`rest_do_request`), so the
//! routes, the `permission_callback`, the request's route/query/body as WordPress
//! hands them over — everything but the network — are the real ones.
//!
//! What it does NOT prove: a request over HTTPS from rexenv's client (L5/L6),
//! a shared host's limits, a WAF.

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::Path;
use std::time::Duration;
use tauri::Manager;

mod common;

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let plat = rexenv_lib::platform::current();
    let pid = std::process::id();
    let db_file = std::env::temp_dir().join(format!("rexenv-live-sync-check-{pid}.db"));
    let _ = std::fs::remove_file(&db_file);
    let conn = rexenv_lib::state::db::open(&db_file).unwrap();
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "livesync");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let php = binaries::resolve(&*plat, "php", binaries::pins().php).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli).await.unwrap();
    let (mysql, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, binaries::pins().mysql)
        .await
        .expect("bundled MySQL client");
    let app = tauri::test::mock_app();
    app.manage(commands::repo::RepoJobs::default());
    app.manage(site_provision::ProvisionJobs::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle().clone();
    let _engines = common::engines_as_found();
    {
        let state = handle.state::<AppState>();
        let mut mgr = state.services.lock().await;
        mgr.adopt_dbs(state.platform.as_ref());
    }

    let domain = format!("lsync{pid}.rex");
    let snap = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        NewSite {
            name: "Live Sync Fixture".into(),
            domain: domain.clone(),
            site_type: SiteType::Wordpress,
            php_version: binaries::pins().php.split('.').take(2).collect::<Vec<_>>().join("."),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: false,
            git_build_assets: false,
            starter_db: false,
        },
        None,
        None,
    )
    .await
    .expect("start the fixture site");
    let site_id = snap.site_id.clone().expect("a site id");
    let mut status = String::new();
    for _ in 0..2400 {
        let st = site_provision::state_of(&handle.state::<site_provision::ProvisionJobs>(), &snap.id).unwrap();
        if st.status != "running" {
            status = st.status.clone();
            println!("fixture settled {} — {:?}", st.status, st.summary.as_deref().or(st.error.as_deref()));
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let site = {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        sites::get(&conn, &site_id).unwrap().expect("the fixture row")
    };
    let mut ok = status == "ok";
    if ok {
        let docroot = site.served_root();
        let plugin = docroot.join("wp-content/plugins/rexenv-sync");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../companion/rexenv-sync");
        copy_dir(&src, &plugin);
        match wordpress::wp_run(&php, &wp, &docroot, &["plugin", "activate", "rexenv-sync"]) {
            Ok(o) => println!("{o}"),
            Err(e) => {
                println!("FAIL: activate: {e}");
                ok = false;
            }
        }
        let script = plugin.join("tests/integration-test.php");
        match wordpress::wp_run(&php, &wp, &docroot, &["eval-file", &script.to_string_lossy()]) {
            Ok(out) => {
                println!("{out}");
                ok &= out.contains("integration: 0 failed") && !out.contains("FAIL ");
            }
            Err(e) => {
                println!("FAIL: eval-file: {e}");
                ok = false;
            }
        }
    }

    // Teardown: this run's database and row; the folder is under the pinned
    // fixture sites dir, which `_sites_dir` removes on drop.
    {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = rexenv_lib::core::db::DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &site.db_name);
        let _ = sites::teardown(&conn, state.platform.as_ref(), &site_id);
    }
    let _ = std::fs::remove_file(&db_file);
    if ok {
        println!("live_sync_plugin_check: ALL PASS");
        std::process::ExitCode::SUCCESS
    } else {
        println!("live_sync_plugin_check: FAILED");
        std::process::ExitCode::FAILURE
    }
}
