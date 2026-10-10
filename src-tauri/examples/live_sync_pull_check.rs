//! Live check: rexenv's sync CLIENT against the plugin, over a real HTTP
//! connection (L5 of `docs/PLAN-wp-live-sync.md`). Run:
//! `cargo run --example live_sync_pull_check`
//!
//! A fixture WordPress (the real provisioning job, sites dir pinned, database
//! tier adopted only) gets the plugin and a pairing; the bundled PHP's built-in
//! server serves it on a fixture port; `core::live_sync::client::Client` — signed
//! requests, cursor loops, the frame parser, the `.partial` export — talks to it.
//!
//! Proves: the manifest answers for the key's own site; the file list pages to
//! the end and holds the plugin's file; files arrive byte-identical, and a missing
//! one comes back as the plugin's refusal, not as a hole; the options table
//! exports through `.partial` with the pairing row absent; a REGENERATED pairing
//! makes the old key fail with the §6 sentence.
//!
//! What it does NOT prove: HTTPS (the fixture server is plain HTTP on loopback,
//! reached through `with_base_url` — the key itself still names the https site),
//! a shared host, a WAF.

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::live_sync::{client::Client, sign};
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
    let db_file = std::env::temp_dir().join(format!("rexenv-live-pull-check-{pid}.db"));
    let _ = std::fs::remove_file(&db_file);
    let conn = rexenv_lib::state::db::open(&db_file).unwrap();
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "livepull");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let php = binaries::resolve_program(&*plat, "php", binaries::pins().php).await.unwrap();
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

    let domain = format!("lpull{pid}.rex");
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
        // The tree's copy, or — on a machine the binary was copied to (the Dell) — the
        // folder `REXENV_SYNC_PLUGIN_DIR` names.
        let src = std::env::var_os("REXENV_SYNC_PLUGIN_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../companion/rexenv-sync"));
        copy_dir(&src, &plugin);
        match wordpress::wp_run(&php, &wp, &docroot, &["plugin", "activate", "rexenv-sync"]) {
            Ok(o) => println!("{o}"),
            Err(e) => {
                println!("FAIL: activate: {e}");
                ok = false;
            }
        }
        ok &= pull_legs(&php, &wp, &docroot).await;
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
        println!("live_sync_pull_check: ALL PASS");
        std::process::ExitCode::SUCCESS
    } else {
        println!("live_sync_pull_check: FAILED");
        std::process::ExitCode::FAILURE
    }
}

const PORT: u16 = 13392;

async fn pull_legs(php: &Path, wp: &Path, docroot: &Path) -> bool {
    let mut ok = true;
    let mut check = |good: bool, what: &str| {
        println!("{} {what}", if good { "ok  " } else { "FAIL" });
        ok &= good;
    };
    common::require_ports_free(&[(PORT, "the fixture PHP server")]);
    let key_text = wordpress::wp_run(php, wp, docroot, &["eval", "echo Rexenv_Sync_Pairing::create();"]).unwrap_or_default();
    let key = match sign::parse_key(key_text.trim()) {
        Ok(k) => k,
        Err(e) => {
            check(false, &format!("the plugin's key parses ({e})"));
            return false;
        }
    };
    let server = std::process::Command::new(php)
        .args(["-S", &format!("127.0.0.1:{PORT}"), "-t", &docroot.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("php -S");
    let _server = common::Reaped::new(server, PORT, "php");
    for _ in 0..50 {
        if rexenv_lib::core::ports::is_listening(PORT) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let client = Client::new(key.clone()).unwrap().with_base_url(&format!("http://127.0.0.1:{PORT}"));

    match client.manifest().await {
        Ok(m) => check(m.site_url == key.site_url && m.tables.iter().any(|t| t.name.ends_with("options")), "manifest: the key's own site, its tables"),
        Err(e) => check(false, &format!("manifest: {e}")),
    }
    let own = "plugins/rexenv-sync/rexenv-sync.php".to_string();
    let files = client.list_files(&[], None).await.unwrap_or_default();
    check(files.iter().any(|f| f.path == own), &format!("the file list pages to the end ({} files) and holds the plugin's main file", files.len()));

    let dest = std::env::temp_dir().join(format!("rexenv-live-pull-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    let asked = [own.clone(), "plugins/rexenv-sync/includes/class-rexenv-sync-reader.php".to_string(), "plugins/not-there.php".to_string()];
    let sized: Vec<(String, u64)> = asked.iter().map(|p| (p.clone(), 0)).collect();
    match client.read_files(&sized, &dest, &[]).await {
        Ok(refused) => {
            let same = |rel: &str| {
                std::fs::read(dest.join(rel)).ok() == std::fs::read(docroot.join("wp-content").join(rel)).ok()
            };
            check(same(&asked[0]) && same(&asked[1]), "two files arrive byte-identical");
            check(refused.len() == 1 && refused[0].path == "plugins/not-there.php", "a missing file comes back as the plugin's refusal");
        }
        Err(e) => check(false, &format!("read_files: {e}")),
    }

    let out = dest.join("options.sql");
    match client.export_table(&format!("{}options", "wp_"), &out).await {
        Ok(rows) => {
            let sql = std::fs::read_to_string(&out).unwrap_or_default();
            check(
                rows > 0 && sql.contains("'siteurl'") && !sql.contains("rexsync_pairing") && !dest.join("options.sql.partial").exists(),
                &format!("the options table ({rows} rows) through .partial, without the pairing row"),
            );
        }
        Err(e) => check(false, &format!("export_table: {e}")),
    }

    // A regenerated pairing: the old key now fails with the §6 sentence.
    let _ = wordpress::wp_run(php, wp, docroot, &["eval", "Rexenv_Sync_Pairing::create();"]);
    match client.manifest().await {
        Err(e) => check(e.to_string().contains("paste its current key"), &format!("the old key is refused by name ({e})")),
        Ok(_) => check(false, "the old key still worked after Regenerate"),
    }
    let _ = std::fs::remove_dir_all(&dest);
    ok
}
