//! Live check: the `wp.plugin.install` / `wp.theme.install` dispatch arms —
//! run BEFORE and AFTER the stage-3 switch (captured fns → the streamed
//! install job) to prove the CLI's observable behavior is unchanged:
//! same success envelope (`{"ok":true,"data":null}`), same error class on a
//! bad slug (ok:false + message ⇒ rex exit 1), same `--json` payload.
//! Prints the RAW envelopes for side-by-side comparison.
//! Bootstrap mirrors wp_install_stream_check (real WP + MySQL; reuses a
//! running MySQL on :13306). Network required.
//! Run: `cargo run --example cli_wp_install_check`

use rexenv_lib::cli_server;
use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::Manager;

mod common;

#[tokio::main]
async fn main() {
    let plat = rexenv_lib::platform::current();
    let domain = format!("cliwpi-{}.rex", std::process::id());

    let conn = {
        let p = std::env::temp_dir().join("rexenv-cliwpi.db");
        let _ = std::fs::remove_file(&p);
        rexenv_lib::state::db::open(&p).unwrap()
    };
    // Fixture-owned sites dir, pinned BEFORE the first `sites::provision`.
    // `sites::provision` reads the `sites_dir` SETTING, which falls back to a
    // path computed from the HOME directory — so without this the docroot lands
    // in the user's real ~/rexenv/Sites, and the `remove_dir_all` below deletes
    // it there. See `common::pin_fixture_sites_dir`.
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "cliwpi");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");

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
            name: "CLI WP Install".into(),
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
        },
    )
    .unwrap();
    let site_id = site.id.clone();
    let docroot = std::path::PathBuf::from(&site.path);
    wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        &domain,
        "CLI WP Install",
        &wordpress::db_name_for(SiteType::Wordpress, &domain),
        &format!("127.0.0.1:{}", database::MYSQL_PORT),
        &db_client,
        &Default::default(),
    )
    .expect("install wordpress");

    let app = tauri::test::mock_app();
    app.manage(rexenv_lib::commands::repo::RepoJobs::default());
    app.manage(rexenv_lib::commands::wp_install::WpInstallJobs::default());
    app.manage(AppState::new(conn, plat, ca));

    let ask = |cmd: &str, args: Value| {
        let line = json!({ "cmd": cmd, "args": args }).to_string();
        let handle = app.handle().clone();
        async move {
            let reply = cli_server::handle_request(&handle, line).await;
            serde_json::from_str::<Value>(&reply).expect("valid envelope")
        }
    };
    let mut failures: Vec<String> = Vec::new();

    // Success case: exact envelope printed for the before/after diff.
    let r = ask(
        "wp.plugin.install",
        json!({ "id": site_id, "slug": "hello-dolly", "activate": true }),
    )
    .await;
    println!("ENVELOPE success plugin: {r}");
    if r["ok"] != json!(true) || r["data"] != Value::Null {
        failures.push(format!("success envelope changed: {r}"));
    }
    // The install must be REAL: list shows it active.
    let list = ask("wp.plugins", json!({ "id": site_id })).await;
    let active = list["data"]["plugins"]
        .as_array()
        .map(|a| {
            a.iter()
                .any(|p| p["name"] == json!("hello-dolly") && p["status"] == json!("active"))
        })
        .unwrap_or(false);
    println!("hello-dolly active after install+activate: {active}");
    if !active {
        failures.push("hello-dolly not active after install".into());
    }

    // Failure case: nonexistent slug → ok:false + message (rex exit 1 path).
    let r = ask(
        "wp.plugin.install",
        json!({ "id": site_id, "slug": "definitely-not-a-real-plugin-xyz9", "activate": false }),
    )
    .await;
    println!("ENVELOPE failure plugin: {r}");
    if r["ok"] != json!(false) || r["error"].as_str().unwrap_or("").is_empty() {
        failures.push(format!("failure envelope changed: {r}"));
    }

    // Theme success envelope.
    let r = ask(
        "wp.theme.install",
        json!({ "id": site_id, "slug": "twentytwentyfour", "activate": false }),
    )
    .await;
    println!("ENVELOPE success theme: {r}");
    if r["ok"] != json!(true) || r["data"] != Value::Null {
        failures.push(format!("theme success envelope changed: {r}"));
    }

    // Cleanup.
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = rexenv_lib::state::store::delete_site(&conn, &site_id);
    }
    // Delete ONLY this fixture's dir. NEVER the parent: provision puts the
    // docroot DIRECTLY in the Sites folder, so parent() IS the user's
    // Sites dir — an earlier version of this line deleted every site.
    let _ = std::fs::remove_dir_all(&docroot);
    let _ = database::drop_database(
        &db_client,
        database::MYSQL_PORT,
        &wordpress::db_name_for(SiteType::Wordpress, &domain),
    );
    if let Some(mut m) = own_mysqld {
        let _ = database::stop(&*rexenv_lib::platform::current(), m.id());
        let _ = m.wait();
    }
    println!();
    if failures.is_empty() {
        println!("cli_wp_install_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
