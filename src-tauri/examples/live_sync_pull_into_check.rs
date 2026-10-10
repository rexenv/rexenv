//! Live check: a whole PULL — a "live" WordPress into a "local" one (L6 of
//! `docs/PLAN-wp-live-sync.md`, `core::live_sync::pull::pull_into`). Run:
//! `cargo run --example live_sync_pull_into_check`
//!
//! Two fixture WordPress sites through the real provisioning job (sites dir pinned,
//! database tier adopted only). The "live" one gets the plugin, a pairing, a post
//! and an upload only it has, and is served by the bundled PHP's built-in server;
//! the "local" one gets a post only IT has. Then `pull_into`.
//!
//! Proves:
//!   1. the local site's database is now the live one's — the live post is there,
//!      the local-only post is not — with `siteurl` on the LOCAL domain;
//!   2. the previous local tables are kept in `<db>_prepull`, the local-only post
//!      in them;
//!   3. the live upload arrived; the pairing row did not;
//!   4. the plugin that came along is not active locally;
//!   5. a pull that fails before the swap (a key the site no longer knows) leaves
//!      the local site exactly as it was.
//!
//! Does NOT prove: HTTPS, a real host, a table prefix that differs (both fixtures
//! use `wp_`), files deleted on live (never deleted locally in v1).

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::db::{DbEngine, SqlClient};
use rexenv_lib::core::live_sync::{client::Client, pull, sign};
use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::Path;
use std::time::Duration;
use tauri::Manager;

mod common;

const PORT: u16 = 13392;

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

fn query(mysql: &SqlClient, db: &str, sql: &str) -> String {
    let out = std::process::Command::new(mysql.path())
        .args(["--no-defaults", "--protocol=TCP", "-h", "127.0.0.1"])
        .arg(format!("--port={}", database::MYSQL_PORT))
        .args(["-u", "root", "-N", "-B", "-D", db, "-e", sql])
        .output()
        .expect("mysql");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let plat = rexenv_lib::platform::current();
    let pid = std::process::id();
    let db_file = std::env::temp_dir().join(format!("rexenv-pull-into-{pid}.db"));
    let _ = std::fs::remove_file(&db_file);
    let conn = rexenv_lib::state::db::open(&db_file).unwrap();
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "pullinto");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let php = binaries::resolve_program(&*plat, "php", binaries::pins().php).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli).await.unwrap();
    let (mysql, _) = DbEngine::Mysql.sql_client_bins(&*plat, binaries::pins().mysql).await.expect("mysql client");
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
    common::require_ports_free(&[(PORT, "the fixture PHP server")]);

    let make = |domain: String| {
        let handle = handle.clone();
        async move {
            let snap = site_provision::site_provision_job(
                handle.clone(),
                handle.state::<AppState>(),
                handle.state::<site_provision::ProvisionJobs>(),
                NewSite {
                    name: domain.clone(),
                    domain,
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
            .expect("start a fixture site");
            for _ in 0..2400 {
                let st = site_provision::state_of(&handle.state::<site_provision::ProvisionJobs>(), &snap.id).unwrap();
                if st.status != "running" {
                    println!("{} settled {}", st.domain, st.status);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let state = handle.state::<AppState>();
            let conn = state.db.lock().unwrap();
            sites::get(&conn, snap.site_id.as_deref().unwrap()).unwrap().unwrap()
        }
    };
    let live = make(format!("lvl{pid}.rex")).await;
    let local = make(format!("lcl{pid}.rex")).await;

    let mut ok = true;
    let mut check = |good: bool, what: &str| {
        println!("{} {what}", if good { "ok  " } else { "FAIL" });
        ok &= good;
    };

    // The live site: plugin, a post and an upload only it has, a pairing.
    let live_root = live.served_root();
    let src = std::env::var_os("REXENV_SYNC_PLUGIN_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../companion/rexenv-sync"));
    copy_dir(&src, &live_root.join("wp-content/plugins/rexenv-sync"));
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["plugin", "activate", "rexenv-sync"]);
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["post", "create", "--post_title=LIVE-ONLY", "--post_status=publish"]);
    std::fs::create_dir_all(live_root.join("wp-content/uploads/2026")).unwrap();
    std::fs::write(live_root.join("wp-content/uploads/2026/rexsync-probe.txt"), "from live").unwrap();
    let key = sign::parse_key(wordpress::wp_run(&php, &wp, &live_root, &["eval", "echo Rexenv_Sync_Pairing::create();"]).unwrap_or_default().trim()).expect("a key");
    // The local site: a post only it has.
    let local_root = local.served_root();
    let _ = wordpress::wp_run(&php, &wp, &local_root, &["post", "create", "--post_title=LOCAL-ONLY", "--post_status=publish"]);

    let server = std::process::Command::new(&php)
        .args(["-S", &format!("127.0.0.1:{PORT}"), "-t", &live_root.to_string_lossy()])
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
    let scratch = std::env::temp_dir().join(format!("rexenv-pull-into-scratch-{pid}"));
    let platform = rexenv_lib::platform::current();
    let target = pull::LocalSite {
        platform: platform.as_ref(),
        engine: DbEngine::Mysql,
        client: &mysql,
        port: database::MYSQL_PORT,
        db_name: &local.db_name,
        domain: &local.domain,
        docroot: &local_root,
        php: &php,
        wp_phar: &wp,
        scratch: &scratch,
    };

    // 5 first: a failed pull (a key the site no longer knows) changes nothing.
    let stale = Client::new(sign::parse_key(&{
        let mut k = key.clone();
        k.key_id = "k_ffffffff".into();
        rexenv_lib::core::live_sync::sign::PROTOCOL.to_string() + ":" + &base64_key(&k)
    }).unwrap()).unwrap().with_base_url(&format!("http://127.0.0.1:{PORT}"));
    let before = query(&mysql, &local.db_name, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LOCAL-ONLY'");
    let mut sink = |l: &str| println!("  | {l}");
    let failed = pull::pull_into(&stale, &target, &pull::PullOptions::default(), &mut sink).await;
    let after = query(&mysql, &local.db_name, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LOCAL-ONLY'");
    check(failed.is_err() && before == "1" && after == "1", "5. a pull refused by the site leaves the local site as it was");

    let client = Client::new(key.clone()).unwrap().with_base_url(&format!("http://127.0.0.1:{PORT}"));
    match pull::pull_into(&client, &target, &pull::PullOptions::default(), &mut sink).await {
        Ok(r) => println!("pulled: {} tables, {} rows, {} files, {} replacements", r.tables, r.rows, r.files, r.replacements),
        Err(e) => check(false, &format!("the pull: {e}")),
    }
    let db = &local.db_name;
    check(
        query(&mysql, db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LIVE-ONLY'") == "1"
            && query(&mysql, db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LOCAL-ONLY'") == "0"
            && query(&mysql, db, "SELECT option_value FROM wp_options WHERE option_name='siteurl'") == format!("https://{}", local.domain),
        "1. the local database is the live one's, on the local domain",
    );
    let backup = format!("{db}_prepull");
    check(query(&mysql, &backup, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LOCAL-ONLY'") == "1", "2. the previous local tables are kept in _prepull");
    check(
        std::fs::read_to_string(local_root.join("wp-content/uploads/2026/rexsync-probe.txt")).ok().as_deref() == Some("from live")
            && query(&mysql, db, "SELECT COUNT(*) FROM wp_options WHERE option_name='rexsync_pairing'") == "0",
        "3. the live upload arrived; the pairing row did not",
    );
    let active = wordpress::wp_run(&php, &wp, &local_root, &["plugin", "list", "--status=active", "--field=name", "--skip-plugins"]).unwrap_or_default();
    check(!active.lines().any(|l| l.trim() == "rexenv-sync"), "4. the plugin that came along is not active locally");

    // Teardown: both sites' databases and rows, the backup, the scratch dir.
    {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        for s in [&live, &local] {
            let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &s.db_name);
            let _ = sites::teardown(&conn, state.platform.as_ref(), &s.id);
        }
        let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &backup);
        let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &format!("{db}_pull"));
    }
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::remove_file(&db_file);
    if ok {
        println!("live_sync_pull_into_check: ALL PASS");
        std::process::ExitCode::SUCCESS
    } else {
        println!("live_sync_pull_into_check: FAILED");
        std::process::ExitCode::FAILURE
    }
}

/// The key's base64 body for a (deliberately altered) pairing.
fn base64_key(k: &sign::PairingKey) -> String {
    use base64::Engine;
    let json = serde_json::json!({ "u": k.site_url, "k": k.key_id, "s": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&k.secret) });
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.to_string())
}
