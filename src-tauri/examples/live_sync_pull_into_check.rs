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
//!   4. the plugin that came along is not active locally, and WP-Cron is off
//!      (`DISABLE_WP_CRON`, Q8b — a pulled shop never renews subscriptions here);
//!   5. a pull that fails before the swap (a key the site no longer knows) leaves
//!      the local site exactly as it was;
//!   6. (L10, push) a local post and the one file changed since the base reach
//!      live with LIVE's siteurl and the pairing row intact, users untouched;
//!   7. the backup holds live's previous posts;
//!   8. rollback removes both;
//!   9. a table live changed since the base STOPS the push with nothing sent;
//!  10. overriding it pushes;
//!  11. (#837) an export stream that reaches for another database, the `mysql`
//!      schema or a new user fails under the scoped import user — nothing it
//!      named exists afterwards — while a plain table import under it succeeds.
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
    let (mysql, mysqldump) = DbEngine::Mysql.sql_client_bins(&*plat, binaries::pins().mysql).await.expect("mysql client");
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
        dump: mysqldump.clone(),
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
    let cron = wordpress::wp_run(&php, &wp, &local_root, &["config", "get", "DISABLE_WP_CRON", "--type=constant"]).unwrap_or_default();
    check(cron.trim() == "1" || cron.trim().eq_ignore_ascii_case("true"), &format!("4b. WP-Cron is off in the pulled copy's wp-config.php (DISABLE_WP_CRON = {:?})", cron.trim()));

    // ── PUSH (L10): a local change goes to live; rollback; a conflict stops it. ──
    use rexenv_lib::core::live_sync::{base, push};
    let _ = wordpress::wp_run(&php, &wp, &local_root, &["post", "create", "--post_title=PUSHED-FROM-LOCAL", "--post_status=publish"]);
    let prior = base::SyncBase { tables: Default::default(), files: Default::default(), at: 0 };
    // The base a real pull records: the client's report carries it; here the pull above
    // did not keep it, so take a fresh one the same way the command does.
    let m = client.manifest().await.unwrap();
    let fresh = base::SyncBase {
        tables: m.tables.iter().map(|t| (t.name.clone(), t.checksum.clone())).collect(),
        files: client.list_files(&pull::DEFAULT_EXCLUDES, None).await.unwrap().into_iter().map(|f| (f.path, format!("{}:{}", f.size, f.mtime))).collect(),
        // The time of "the last sync" — a moment ago, so only files written after it
        // count as changed locally.
        at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64 - 1,
    };
    let _ = prior;
    tokio::time::sleep(Duration::from_millis(1100)).await; // a second later than `at`
    std::fs::write(local_root.join("wp-content/uploads/2026/from-local.txt"), "from local").unwrap();
    let opts = push::PushOptions { tables: None, files: true, override_items: vec![] };
    let pushed = push::push_from(&client, &target, Some(&fresh), &opts, &mut sink).await;
    let live_db = &live.db_name;
    match &pushed {
        Ok(r) => {
            check(
                query(&mysql, live_db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='PUSHED-FROM-LOCAL'") == "1"
                    && query(&mysql, live_db, "SELECT option_value FROM wp_options WHERE option_name='siteurl'") == format!("https://{}", live.domain)
                    && std::fs::read_to_string(live_root.join("wp-content/uploads/2026/from-local.txt")).ok().as_deref() == Some("from local")
                    && query(&mysql, live_db, "SELECT COUNT(*) FROM wp_options WHERE option_name='rexsync_pairing'") == "1"
                    && !r.tables.iter().any(|t| t.ends_with("users"))
                    && r.files == 1,
                &format!("6. push: the local post and ONLY the changed file ({}) are on live, siteurl is LIVE's, the pairing row survived, users was not pushed", r.files),
            );
            check(query(&mysql, live_db, "SELECT COUNT(*) FROM rxbak_wp_posts WHERE post_title='PUSHED-FROM-LOCAL'") == "0", "7. the backup holds live's previous posts");
            match client.push_rollback(&r.backup_id).await {
                Ok(()) => check(
                    query(&mysql, live_db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='PUSHED-FROM-LOCAL'") == "0"
                        && !live_root.join("wp-content/uploads/2026/from-local.txt").exists(),
                    "8. rollback: the pushed post and file are gone from live",
                ),
                Err(e) => check(false, &format!("8. rollback: {e}")),
            }
        }
        Err(push::PushStop::Conflicts(c)) => check(false, &format!("6. push stopped on conflicts {c:?}")),
        Err(push::PushStop::Failed(e)) => check(false, &format!("6. push: {e}")),
    }
    // 9. A conflict: live changed since the base → the push stops BEFORE anything is sent.
    match wordpress::wp_run(&php, &wp, &live_root, &["post", "create", "--post_title=LIVE-CHANGED-SINCE", "--post_status=publish"]) {
        Ok(o) => println!("  live post create: {o}"),
        Err(e) => println!("  live post create FAILED: {e}"),
    }
    let stale_base = fresh.clone();
    let again = push::push_from(&client, &target, Some(&stale_base), &opts, &mut sink).await;
    match again {
        Err(push::PushStop::Conflicts(list)) => check(
            list.iter().any(|t| t.ends_with("posts")) && query(&mysql, live_db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='PUSHED-FROM-LOCAL'") == "0",
            &format!("9. a table live changed since the base stops the push, nothing sent ({list:?})"),
        ),
        other => check(false, &format!("9. expected conflicts, got {}", match other { Ok(_) => "ok".into(), Err(push::PushStop::Failed(e)) => e.to_string(), Err(push::PushStop::Conflicts(c)) => format!("{c:?}") })),
    }
    // 10. Overriding the conflict pushes.
    let opts2 = push::PushOptions { tables: Some(vec!["wp_posts".into()]), files: false, override_items: vec!["wp_posts".into()] };
    let over = push::push_from(&client, &target, Some(&stale_base), &opts2, &mut sink).await;
    let over_why = match &over { Ok(_) => "ok".to_string(), Err(push::PushStop::Conflicts(c)) => format!("conflicts {c:?}"), Err(push::PushStop::Failed(e)) => e.to_string() };
    check(over.is_ok() && query(&mysql, live_db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LIVE-CHANGED-SINCE'") == "0", &format!("10. overriding the conflict pushes that table (live's newer post is gone, as warned) — {over_why}"));

    // 11. The scoped import: a hostile stream cannot leave the staging database.
    {
        let stage = format!("{db}_hostile_probe_stage");
        let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &stage);
        DbEngine::Mysql.create_database(&mysql, database::MYSQL_PORT, &stage).unwrap();
        std::fs::create_dir_all(&scratch).unwrap();
        let hostile = scratch.join("hostile.sql");
        let mut outcomes = Vec::new();
        for (what, sql) in [
            ("another database", format!("CREATE DATABASE `{db}_hostile_probe`;")),
            ("the mysql schema", "SELECT user FROM mysql.user;".to_string()),
            ("a new user", "CREATE USER 'rexhostile'@'127.0.0.1' IDENTIFIED BY 'x';".to_string()),
            ("a sibling site's table", format!("DROP TABLE `{}`.wp_options;", local.db_name)),
        ] {
            std::fs::write(&hostile, sql).unwrap();
            outcomes.push((what, DbEngine::Mysql.import_from_file_scoped(&mysql, database::MYSQL_PORT, &stage, &hostile).is_err()));
        }
        std::fs::write(&hostile, "CREATE TABLE probe (id INT); INSERT INTO probe VALUES (1);").unwrap();
        let plain_ok = DbEngine::Mysql.import_from_file_scoped(&mysql, database::MYSQL_PORT, &stage, &hostile).is_ok();
        let probe_db = query(&mysql, "mysql", &format!("SELECT COUNT(*) FROM information_schema.schemata WHERE schema_name='{db}_hostile_probe'"));
        let probe_user = query(&mysql, "mysql", "SELECT COUNT(*) FROM mysql.user WHERE user='rexhostile'");
        let sibling = query(&mysql, &local.db_name, "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='wp_options'");
        let leftover_users = query(&mysql, "mysql", "SELECT COUNT(*) FROM mysql.user WHERE user LIKE 'rexpull_%'");
        check(
            outcomes.iter().all(|(_, refused)| *refused) && plain_ok && probe_db == "0" && probe_user == "0" && sibling == "1" && leftover_users == "0",
            &format!("11. the scoped import refuses {:?}; a plain table imports; no probe db/user, the sibling table intact, no import user left behind (db={probe_db} user={probe_user} sibling={sibling} leftover={leftover_users})", outcomes),
        );
        let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &stage);
        let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &format!("{db}_hostile_probe"));
        // A PLANTED run (the grant widened) lets the probes through: clean what they would make.
        let _ = query(&mysql, "mysql", "DROP USER IF EXISTS 'rexhostile'@'127.0.0.1'");
        let _ = std::fs::remove_file(&hostile);
    }

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
