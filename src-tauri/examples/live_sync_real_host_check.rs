//! Live check against a REAL live WordPress (L14 of `docs/PLAN-wp-live-sync.md`):
//! the project's own `https://live-sync.rex.bd`, over real HTTPS. Run, singly:
//!
//! ```text
//! REXENV_LIVE_KEY_FILE=~/live-sync-key.txt cargo run --example live_sync_real_host_check
//! ```
//!
//! The key comes from a FILE (0600, made with `pbpaste > …`), never an argument or
//! an env value, so it never lands in shell history. Without the file: SKIPPED, exit 2.
//!
//! WRITES TO THE LIVE SITE, and undoes it: one probe post and one probe upload are
//! pushed, checked over public HTTPS, then rolled back and checked gone. Nothing
//! else on live is touched (the push names only the posts tables; a live change
//! since the pull stops it with nothing sent). Locally everything is a fixture:
//! a temp app database, a pinned sites dir, one throwaway WordPress site, dropped
//! at the end. No pairing file or base is written to the real app data.
//!
//! Proves, on a real host:
//!   1. a pull: the local copy is the live site, on the local domain, cron off,
//!      the plugin inactive locally, the pairing row not copied;
//!   2. right after the pull nothing reads as changed on live — tables or files
//!      (#841: the stamps are stable on a real MySQL, not only the fixture's);
//!   3. a push of one local post + one new file: only that file goes, the post is
//!      on live at the LIVE domain (public REST), the file is served over HTTPS;
//!   4. rollback: both are gone from live — from the site's own signed file list at
//!      once (4a), and from the public URL once the host's front cache lets go (4b).

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::live_sync::{client::Client, pull, push, sign};
use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::time::Duration;
use tauri::Manager;

mod common;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let key_file = std::env::var_os("REXENV_LIVE_KEY_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dirs_home().join("live-sync-key.txt"));
    let Ok(raw) = std::fs::read_to_string(&key_file) else {
        println!("live_sync_real_host_check: SKIPPED — no key file at {} (paste the plugin's key into it, chmod 600)", key_file.display());
        return std::process::ExitCode::from(2);
    };
    let key = match sign::parse_key(raw.trim()) {
        Ok(k) => k,
        Err(e) => {
            println!("live_sync_real_host_check: the key file does not hold a key: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let live_url = key.site_url.clone();
    let live_host = pull::host_of(&live_url);
    println!("live site: {live_url}");

    let plat = rexenv_lib::platform::current();
    let pid = std::process::id();
    let db_file = std::env::temp_dir().join(format!("rexenv-real-host-{pid}.db"));
    let _ = std::fs::remove_file(&db_file);
    let conn = rexenv_lib::state::db::open(&db_file).unwrap();
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "realhost");
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

    // The local fixture site.
    let domain = format!("l14{pid}.rex");
    let snap = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        NewSite {
            name: domain.clone(),
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
    for _ in 0..2400 {
        let st = site_provision::state_of(&handle.state::<site_provision::ProvisionJobs>(), &snap.id).unwrap();
        if st.status != "running" {
            println!("{} settled {}", st.domain, st.status);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let local = {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        sites::get(&conn, snap.site_id.as_deref().unwrap()).unwrap().unwrap()
    };
    let root = local.served_root();
    let scratch = std::env::temp_dir().join(format!("rexenv-real-host-scratch-{pid}"));
    let platform = rexenv_lib::platform::current();
    let target = pull::LocalSite {
        platform: platform.as_ref(),
        engine: DbEngine::Mysql,
        client: &mysql,
        dump: mysqldump.clone(),
        port: database::MYSQL_PORT,
        db_name: &local.db_name,
        domain: &local.domain,
        docroot: &root,
        php: &php,
        wp_phar: &wp,
        scratch: &scratch,
    };

    let mut ok = true;
    let mut check = |good: bool, what: &str| {
        println!("{} {what}", if good { "ok  " } else { "FAIL" });
        ok &= good;
    };
    let mut sink = |l: &str| println!("  | {l}");
    let client = Client::new(key).unwrap();
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap();

    // 1. Pull.
    let pulled = pull::pull_into(&client, &target, &pull::PullOptions::default(), &mut sink).await;
    let base = match &pulled {
        Ok(r) => {
            println!("pulled: {} tables, {} rows, {} files", r.tables, r.rows, r.files);
            Some(r.base.clone())
        }
        Err(e) => {
            check(false, &format!("1. the pull: {e}"));
            None
        }
    };
    if let Some(base) = base {
        let siteurl = wordpress::wp_run(&php, &wp, &root, &["option", "get", "siteurl"]).unwrap_or_default();
        let posts = wordpress::wp_run(&php, &wp, &root, &["post", "list", "--post_type=post", "--format=count"]).unwrap_or_default();
        let cron = wordpress::wp_run(&php, &wp, &root, &["config", "get", "DISABLE_WP_CRON", "--type=constant"]).unwrap_or_default();
        let active = wordpress::wp_run(&php, &wp, &root, &["plugin", "list", "--status=active", "--field=name", "--skip-plugins"]).unwrap_or_default();
        let pairing = wordpress::wp_run(&php, &wp, &root, &["option", "get", "rexsync_pairing"]).is_ok();
        check(
            siteurl.trim() == format!("https://{domain}") && posts.trim().parse::<u32>().unwrap_or(0) >= 1 && cron.trim() == "1" && !active.lines().any(|l| l.trim() == "rexenv-sync") && !pairing,
            &format!("1. the local copy is the live site at https://{domain} ({} posts), cron off, plugin inactive, no pairing row", posts.trim()),
        );

        // 2. Nothing reads as changed on live right after the pull.
        let m = client.manifest().await.unwrap();
        let tables_moved: Vec<&str> = m.tables.iter().filter(|t| base.tables.get(&t.name) != Some(&t.checksum)).map(|t| t.name.as_str()).collect();
        let files_now = client.list_files(&pull::DEFAULT_EXCLUDES, None).await.unwrap();
        let files_moved: Vec<String> = files_now.iter().filter(|f| base.files.get(&f.path) != Some(&format!("{}:{}", f.size, f.mtime))).map(|f| f.path.clone()).collect();
        check(tables_moved.is_empty() && files_moved.is_empty(), &format!("2. right after the pull nothing reads as changed on live (tables {tables_moved:?}, files {files_moved:?})"));

        // 3. Push one probe post + one probe upload.
        let title = format!("rexenv-l14-probe-{pid}");
        let file_rel = format!("uploads/{title}.txt");
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let _ = wordpress::wp_run(&php, &wp, &root, &["post", "create", &format!("--post_title={title}"), "--post_status=publish"]);
        std::fs::create_dir_all(root.join("wp-content/uploads")).unwrap();
        std::fs::write(root.join("wp-content").join(&file_rel), "pushed by rexenv's L14 check").unwrap();
        let tables = vec![format!("{}posts", m.prefix), format!("{}postmeta", m.prefix)];
        let opts = push::PushOptions { tables: Some(tables), files: true, override_items: vec![] };
        let search = format!("{live_url}/?rest_route=/wp/v2/posts&search={title}");
        let file_url = format!("{live_url}/wp-content/{file_rel}");
        match push::push_from(&client, &target, Some(&base), &opts, &mut sink).await {
            Ok(r) => {
                let found: serde_json::Value = http.get(&search).send().await.unwrap().json().await.unwrap_or_default();
                let link = found.get(0).and_then(|p| p.get("link")).and_then(|l| l.as_str()).unwrap_or("").to_string();
                let served = http.get(&file_url).send().await.map(|r| r.status().as_u16()).unwrap_or(0);
                check(
                    r.files == 1 && link.starts_with(&format!("https://{live_host}")) && served == 200,
                    &format!("3. push: {} file(s) sent; the post is on live at {link:?}; the file answers {served}", r.files),
                );
                // 4. Roll back.
                match client.push_rollback(&r.backup_id).await {
                    Ok(()) => {
                        // 4a. The plugin's own view of the disk (signed `/files/list`) is the
                        // authority: the probe file is gone from it the moment rollback answers.
                        let listed = client.list_files(&pull::DEFAULT_EXCLUDES, None).await.map(|fs| fs.iter().any(|f| f.path == file_rel)).unwrap_or(true);
                        let found: serde_json::Value = http.get(&search).send().await.unwrap().json().await.unwrap_or_default();
                        let gone = found.as_array().map(|a| a.is_empty()).unwrap_or(false);
                        check(gone && !listed, &format!("4a. rollback {}: the post is gone from live ({gone}) and the file is gone from the site's own file list ({})", r.backup_id, !listed));
                        // 4b. What the public sees, on a FRESH connection each time. On
                        // live-sync.rex.bd (10 Oct 2026) a reused keep-alive connection kept getting
                        // 200 for 2+ minutes after the unlink — one server worker's open-file cache —
                        // while a new connection got 404 at once; a cache-buster query did not help.
                        // A visitor opens their own connection, so that is what is asked here.
                        let fresh = reqwest::Client::builder().timeout(Duration::from_secs(30)).pool_max_idle_per_host(0).build().unwrap();
                        let started = std::time::Instant::now();
                        let mut served = 0u16;
                        while started.elapsed() < Duration::from_secs(120) {
                            served = fresh.get(&file_url).send().await.map(|r| r.status().as_u16()).unwrap_or(0);
                            if served == 404 {
                                break;
                            }
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                        check(served == 404, &format!("4b. the public URL answers 404 within 120 s (took {} s, fresh connections)", started.elapsed().as_secs()));
                    }
                    Err(e) => check(false, &format!("4. rollback {}: {e} — ROLL BACK BY HAND on the live site", r.backup_id)),
                }
            }
            Err(push::PushStop::Conflicts(c)) => check(false, &format!("3. the push stopped on conflicts {c:?} — nothing was sent")),
            Err(push::PushStop::Failed(e)) => check(false, &format!("3. the push failed: {e} (the plugin aborts a half push)")),
        }
    }

    // Teardown: the fixture site and its databases, the scratch dir, the temp app db.
    {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &local.db_name);
        let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &format!("{}_prepull", local.db_name));
        let _ = DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &format!("{}_push", local.db_name));
        let _ = sites::teardown(&conn, state.platform.as_ref(), &local.id);
    }
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::remove_file(&db_file);
    if ok {
        println!("live_sync_real_host_check: ALL PASS ({live_host})");
        std::process::ExitCode::SUCCESS
    } else {
        println!("live_sync_real_host_check: FAILED ({live_host})");
        std::process::ExitCode::FAILURE
    }
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from).unwrap_or_default()
}
