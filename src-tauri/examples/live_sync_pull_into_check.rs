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
    // The fixture's CUT: while `uploads/rexsync-cut/cut.json` names a route + param +
    // value, the live site answers 500 to that one request — how legs 15/16 interrupt a
    // pull and a push the way a host does. Under `uploads/rexsync-*`, so no pull lists it.
    std::fs::create_dir_all(live_root.join("wp-content/mu-plugins")).unwrap();
    std::fs::write(
        live_root.join("wp-content/mu-plugins/rexsync-cut.php"),
        "<?php\nadd_filter('rest_pre_dispatch', function ($result, $server, $request) {\n    $f = WP_CONTENT_DIR . '/uploads/rexsync-cut/cut.json';\n    if (!is_file($f)) { return $result; }\n    $c = json_decode((string) file_get_contents($f), true);\n    if (is_array($c) && $request->get_route() === $c['route'] && (string) $request->get_param($c['param']) === $c['value']) {\n        return new WP_Error('cut', 'fixture cut', array('status' => 500));\n    }\n    return $result;\n}, 10, 3);\n",
    )
    .unwrap();
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["post", "create", "--post_title=LIVE-ONLY", "--post_status=publish"]);
    std::fs::create_dir_all(live_root.join("wp-content/uploads/2026")).unwrap();
    std::fs::write(live_root.join("wp-content/uploads/2026/rexsync-probe.txt"), "from live").unwrap();
    // Two more for the incremental legs (12): one live will drop, one live will drop AFTER
    // the local copy edited it.
    std::fs::write(live_root.join("wp-content/uploads/2026/gone.txt"), "will go").unwrap();
    std::fs::write(live_root.join("wp-content/uploads/2026/local-edit.txt"), "from live").unwrap();
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
    let failed = pull::pull_into(&stale, &target, None, &pull::PullOptions::default(), &mut sink).await;
    let after = query(&mysql, &local.db_name, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LOCAL-ONLY'");
    check(failed.is_err() && before == "1" && after == "1", "5. a pull refused by the site leaves the local site as it was");

    let client = Client::new(key.clone()).unwrap().with_base_url(&format!("http://127.0.0.1:{PORT}"));
    // The base the pull RECORDS is the one leg 6 pushes against — exactly what the app
    // keeps (#841). Until 10 Oct 2026 leg 6 built its own base ("a moment ago"), and so
    // never saw that a real pull's base made every pulled file look edited locally.
    let mut pulled_base = match pull::pull_into(&client, &target, None, &pull::PullOptions::default(), &mut sink).await {
        Ok(r) => {
            println!("pulled: {} tables, {} rows, {} files, {} replacements", r.tables, r.rows, r.files, r.replacements);
            Some(r.base)
        }
        Err(e) => {
            check(false, &format!("the pull: {e}"));
            None
        }
    };
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

    // ── INCREMENTAL PULL + STATUS (14 Oct 2026) ─────────────────────────────
    use rexenv_lib::core::live_sync::{base, push, stamps, status};
    async fn status_now(client: &Client, target: &pull::LocalSite<'_>, b: Option<&base::SyncBase>) -> status::SyncStatus {
        let m = client.manifest().await.expect("manifest");
        let lf = client.list_files(&pull::DEFAULT_EXCLUDES, None).await.expect("files");
        let ls = stamps::local_stamps(target, &m.prefix).expect("local stamps");
        let lfiles: Vec<(String, i64)> = pull::walk_local(&target.docroot.join("wp-content")).unwrap().into_iter().map(|(p, _, m)| (p, m)).collect();
        status::judge("live", b, &m, &lf, &ls, &lfiles, base::now())
    }
    // 12. Straight after a pull the verdict is "up to date": nothing moved on either side.
    let s0 = status_now(&client, &target, pulled_base.as_ref()).await;
    check(s0.verdict == status::Verdict::UpToDate, &format!("12. right after a pull: up to date (got {:?}; behind {} ahead {}; files live {} local {})", s0.verdict, s0.tables_behind, s0.tables_ahead, s0.files.live_changed.len(), s0.files.local_changed.len()));
    // 12a. live moves ONE post + one file and drops two files; here a comment is added
    //      (a table live does not touch) and one of the dropped files is edited.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["post", "create", "--post_title=LIVE-SECOND", "--post_status=publish"]);
    std::fs::write(live_root.join("wp-content/uploads/2026/rexsync-probe.txt"), "from live v2").unwrap();
    std::fs::remove_file(live_root.join("wp-content/uploads/2026/gone.txt")).unwrap();
    std::fs::remove_file(live_root.join("wp-content/uploads/2026/local-edit.txt")).unwrap();
    let _ = wordpress::wp_run(&php, &wp, &local_root, &["comment", "create", "--comment_post_ID=1", "--comment_content=LOCALCOMMENT", "--comment_approved=1"]);
    std::fs::write(local_root.join("wp-content/uploads/2026/local-edit.txt"), "edited here").unwrap();
    let s1 = status_now(&client, &target, pulled_base.as_ref()).await;
    let posts_live = s1.tables.iter().find(|t| t.name == "wp_posts").map(|t| t.live);
    let comments_local = s1.tables.iter().find(|t| t.name == "wp_comments").map(|t| (t.local, t.live_owned));
    check(
        s1.verdict == status::Verdict::Diverged && posts_live == Some(status::Change::Changed) && comments_local == Some((status::Change::Changed, true))
            && s1.files.live_changed == vec!["uploads/2026/rexsync-probe.txt".to_string()] && s1.files.live_gone == vec!["uploads/2026/gone.txt".to_string(), "uploads/2026/local-edit.txt".to_string()]
            && s1.files.local_changed == vec!["uploads/2026/local-edit.txt".to_string()] && s1.files.both == vec!["uploads/2026/local-edit.txt".to_string()],
        &format!("12a. status: diverged — posts changed on live, comments (live-owned) changed here, the files on each side named ({:?}, posts {:?}, comments {:?}, files {:?})", s1.verdict, posts_live, comments_local, s1.files),
    );
    // 12b. the incremental pull: only what moved on live crosses the wire; the local
    //      comment survives; the dropped file goes; the edited one stays.
    let second = pull::pull_into(&client, &target, pulled_base.as_ref(), &pull::PullOptions::default(), &mut sink).await;
    match second {
        Ok(r) => {
            check(
                r.tables < 12 && r.tables_kept >= 5 && r.tables + r.tables_kept == 12 && r.files == 1 && r.files_kept > 100
                    && r.files_deleted == vec!["uploads/2026/gone.txt".to_string()] && r.files_kept_edited == vec!["uploads/2026/local-edit.txt".to_string()],
                &format!("12b. incremental pull: {} tables exported + {} kept, {} file fetched + {} kept, deleted {:?}, kept-edited {:?}", r.tables, r.tables_kept, r.files, r.files_kept, r.files_deleted, r.files_kept_edited),
            );
            check(
                query(&mysql, db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LIVE-SECOND'") == "1"
                    && query(&mysql, db, "SELECT COUNT(*) FROM wp_comments WHERE comment_content='LOCALCOMMENT'") == "1"
                    && std::fs::read_to_string(local_root.join("wp-content/uploads/2026/rexsync-probe.txt")).ok().as_deref() == Some("from live v2")
                    && !local_root.join("wp-content/uploads/2026/gone.txt").exists()
                    && std::fs::read_to_string(local_root.join("wp-content/uploads/2026/local-edit.txt")).ok().as_deref() == Some("edited here")
                    && !r.base.local_tables.is_empty() && r.base.kind == "pull",
                "12c. the live post arrived, the local comment survived, the file is live's v2, gone.txt is gone, the local edit kept; the base carries local stamps",
            );
            pulled_base = Some(r.base);
        }
        Err(e) => check(false, &format!("12b. the incremental pull: {e}")),
    }
    // 12d. After the incremental pull what was edited HERE before it is still ahead (#863):
    //      the local comment (live-owned: listed, not counted) and the kept-edited file.
    let s2 = status_now(&client, &target, pulled_base.as_ref()).await;
    let comments_after = s2.tables.iter().find(|t| t.name == "wp_comments").map(|t| t.local);
    check(
        s2.verdict == status::Verdict::Ahead && s2.tables_behind == 0 && s2.tables_ahead == 0 && s2.tables_ahead_live_owned == 1 && comments_after == Some(status::Change::Changed)
            && s2.files.local_changed == vec!["uploads/2026/local-edit.txt".to_string()] && s2.files.live_changed.is_empty() && s2.files.live_gone.is_empty(),
        &format!("12d. after the incremental pull: ahead by the kept-edited file only, the local comment still changed (got {:?}; behind {} ahead {} live-owned {}; comments {:?}; files live {:?} gone {:?} local {:?})", s2.verdict, s2.tables_behind, s2.tables_ahead, s2.tables_ahead_live_owned, comments_after, s2.files.live_changed, s2.files.live_gone, s2.files.local_changed),
    );

    // ── PUSH (L10): a local change goes to live; rollback; a conflict stops it. ──
    let _ = wordpress::wp_run(&php, &wp, &local_root, &["post", "create", "--post_title=PUSHED-FROM-LOCAL", "--post_status=publish"]);
    let fresh: base::SyncBase = pulled_base.clone().unwrap_or_default();
    tokio::time::sleep(Duration::from_millis(1100)).await; // the edit lands a second after the pull
    std::fs::write(local_root.join("wp-content/uploads/2026/from-local.txt"), "from local").unwrap();
    let opts = push::PushOptions { tables: None, files: true, override_items: vec![], skip: vec![] };
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
                    && r.tables.iter().any(|t| t == "wp_posts") && !r.tables.iter().any(|t| t == "wp_terms") && r.tables_unchanged > 0
                    && r.files == 2 && live_root.join("wp-content/uploads/2026/local-edit.txt").exists(),
                &format!("6. push: the local post and ONLY the changed files ({}: the new one and the one edited before the pull, #863) are on live, siteurl is LIVE's, the pairing row survived, users was not pushed; only the tables changed here went ({:?}, {} unchanged left out)", r.files, r.tables, r.tables_unchanged),
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
    // Named table: with none named a push sends what changed HERE since the base, and
    // nothing has — the conflict is the PLUGIN's verdict on the table asked for.
    let opts9 = push::PushOptions { tables: Some(vec!["wp_posts".into()]), files: false, override_items: vec![], skip: vec![] };
    let again = push::push_from(&client, &target, Some(&stale_base), &opts9, &mut sink).await;
    match again {
        Err(push::PushStop::Conflicts(list)) => check(
            list.iter().any(|t| t.ends_with("posts")) && query(&mysql, live_db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='PUSHED-FROM-LOCAL'") == "0",
            &format!("9. a table live changed since the base stops the push, nothing sent ({list:?})"),
        ),
        other => check(false, &format!("9. expected conflicts, got {}", match other { Ok(_) => "ok".into(), Err(push::PushStop::Failed(e)) => e.to_string(), Err(push::PushStop::Conflicts(c)) => format!("{c:?}") })),
    }
    // 10. Overriding the conflict pushes.
    let opts2 = push::PushOptions { tables: Some(vec!["wp_posts".into()]), files: false, override_items: vec!["wp_posts".into()], skip: vec![] };
    let over = push::push_from(&client, &target, Some(&stale_base), &opts2, &mut sink).await;
    let over_why = match &over { Ok(_) => "ok".to_string(), Err(push::PushStop::Conflicts(c)) => format!("conflicts {c:?}"), Err(push::PushStop::Failed(e)) => e.to_string() };
    check(over.is_ok() && query(&mysql, live_db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LIVE-CHANGED-SINCE'") == "0", &format!("10. overriding the conflict pushes that table (live's newer post is gone, as warned) — {over_why}"));

    // ── S3 (15 Oct 2026): per-item choice, anonymise, uploads on demand, resume ──
    // 13. `skip`: a conflict the person chose to keep live's version of is left out, and
    //     the rest of the push goes. Live's post (LIVE-CHANGED-SINCE) is gone after leg 10;
    //     make live move again, then push posts + a new file with posts skipped.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let after10 = match &over { Ok(r) => r.base.clone(), Err(_) => stale_base.clone() };
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["post", "create", "--post_title=LIVE-AGAIN", "--post_status=publish"]);
    std::fs::write(local_root.join("wp-content/uploads/2026/skip-leg.txt"), "from local, leg 13").unwrap();
    let opts13 = push::PushOptions { tables: Some(vec!["wp_posts".into()]), files: true, override_items: vec![], skip: vec!["wp_posts".into()] };
    let skipped = push::push_from(&client, &target, Some(&after10), &opts13, &mut sink).await;
    let skip_why = match &skipped { Ok(r) => format!("ok: tables {:?}, {} files", r.tables, r.files), Err(push::PushStop::Conflicts(c)) => format!("conflicts {c:?}"), Err(push::PushStop::Failed(e)) => e.to_string() };
    check(
        matches!(&skipped, Ok(r) if r.tables.is_empty() && r.files >= 1)
            && query(&mysql, live_db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='LIVE-AGAIN'") == "1"
            && std::fs::read_to_string(live_root.join("wp-content/uploads/2026/skip-leg.txt")).ok().as_deref() == Some("from local, leg 13"),
        &format!("13. a skipped conflict (wp_posts) is left out — live's newer post survives — and the file still went ({skip_why})"),
    );
    let base13 = match &skipped { Ok(r) => r.base.clone(), Err(_) => after10.clone() };

    // 14. Anonymise: a pull with the switch on scrubs the copy's users and comments before
    //     the swap; the base names them; a push of such a table is refused before the
    //     plugin is asked; a later pull WITHOUT the switch that replaces the table clears it.
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["user", "create", "pii-probe", "pii-probe@real-people.example", "--role=subscriber", "--first_name=Real", "--last_name=Person"]);
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["comment", "create", "--comment_post_ID=1", "--comment_content=PII", "--comment_author=Real Commenter", "--comment_author_email=commenter@real-people.example", "--comment_approved=1"]);
    let opts14 = pull::PullOptions { anonymise: true, uploads_on_demand: Some(true), ..Default::default() };
    let anon = pull::pull_into(&client, &target, Some(&base13), &opts14, &mut sink).await;
    let (anon_base, anon_why) = match &anon { Ok(r) => (Some(r.base.clone()), format!("anonymised {:?}", r.anonymised)), Err(e) => (None, e.to_string()) };
    let emails = query(&mysql, db, "SELECT COUNT(*) FROM wp_users WHERE user_email LIKE '%real-people.example'");
    let anon_emails = query(&mysql, db, "SELECT COUNT(*) FROM wp_users WHERE user_email LIKE 'user%@example.invalid'");
    let c_emails = query(&mysql, db, "SELECT COUNT(*) FROM wp_comments WHERE comment_author_email LIKE '%real-people.example'");
    let names = query(&mysql, db, "SELECT COUNT(*) FROM wp_usermeta WHERE meta_key='first_name' AND meta_value='Real'");
    check(
        anon.is_ok() && emails == "0" && anon_emails != "0" && c_emails == "0" && names == "0"
            && anon_base.as_ref().is_some_and(|b| b.anonymised.contains(&"wp_users".to_string()) && b.anonymised.contains(&"wp_comments".to_string()))
            && local_root.join("wp-content/mu-plugins/rexenv-live-uploads.php").is_file(),
        &format!("14. anonymise on pull: no real e-mail or name here (users {emails}/{anon_emails}, comments {c_emails}, names {names}), the base names the tables, the uploads-on-demand mu-plugin is in place ({anon_why})"),
    );
    let refused = push::push_from(&client, &target, anon_base.as_ref(), &push::PushOptions { tables: Some(vec!["wp_users".into()]), files: false, override_items: vec![], skip: vec![] }, &mut sink).await;
    check(
        matches!(&refused, Err(push::PushStop::Failed(e)) if e.to_string().contains("anonymised") && e.to_string().contains("never pushed"))
            && query(&mysql, live_db, "SELECT COUNT(*) FROM wp_users WHERE user_email='pii-probe@real-people.example'") == "1",
        &format!("14b. a push of an anonymised table is refused and live's users are untouched ({})", match &refused { Err(push::PushStop::Failed(e)) => e.to_string(), _ => "not refused".into() }),
    );
    // 14c. The uploads-on-demand mu-plugin names the live site; the switch off takes it out.
    let mu = std::fs::read_to_string(local_root.join("wp-content/mu-plugins/rexenv-live-uploads.php")).unwrap_or_default();
    let opts14c = pull::PullOptions { uploads_on_demand: Some(false), ..Default::default() };
    let off = pull::pull_into(&client, &target, anon_base.as_ref(), &opts14c, &mut sink).await;
    check(
        mu.contains(&format!("'https://{}/wp-content/uploads'", live.domain)) && off.is_ok() && !local_root.join("wp-content/mu-plugins/rexenv-live-uploads.php").exists()
            && off.as_ref().map(|r| r.base.anonymised.contains(&"wp_users".to_string())).unwrap_or(false),
        &format!("14c. the mu-plugin carried the live uploads URL; a pull with the switch off removes it; a kept (unchanged) anonymised table stays on the list ({:?})", off.as_ref().map(|r| r.base.anonymised.clone()).ok()),
    );
    let base14 = off.map(|r| r.base).ok().or(anon_base.clone());
    // 14d. live changes the users table; a pull WITHOUT anonymise replaces it → off the list.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["user", "create", "pii-two", "pii-two@real-people.example", "--role=subscriber"]);
    let plain = pull::pull_into(&client, &target, base14.as_ref(), &pull::PullOptions::default(), &mut sink).await;
    check(
        plain.as_ref().map(|r| !r.base.anonymised.contains(&"wp_users".to_string())).unwrap_or(false)
            && query(&mysql, db, "SELECT COUNT(*) FROM wp_users WHERE user_email='pii-two@real-people.example'") == "1",
        &format!("14d. a plain pull that replaced wp_users takes it off the anonymised list ({:?})", plain.as_ref().map(|r| r.base.anonymised.clone()).ok()),
    );
    let mut base15 = plain.map(|r| r.base).ok().or(base14);

    // 15. Resume: a pull cut off mid-way (live refuses one table) continues from what it
    //     staged; the staging database and the record are the proof.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["post", "create", "--post_title=RESUME-ONE", "--post_status=publish"]);
    let _ = wordpress::wp_run(&php, &wp, &live_root, &["option", "update", "blogdescription", "resume probe"]);
    // Two tables moved on live (options, then posts in export order). The fixture's cut
    // mu-plugin makes the site answer 500 to the export of wp_posts while the flag is
    // set: the pull dies with wp_options staged.
    let cut_flag = live_root.join("wp-content/uploads/rexsync-cut/cut.json");
    std::fs::create_dir_all(cut_flag.parent().unwrap()).unwrap();
    std::fs::write(&cut_flag, r#"{"route":"/rexenv-sync/v1/db/export","param":"table","value":"wp_posts"}"#).unwrap();
    let cut = pull::pull_into(&client, &target, base15.as_ref(), &pull::PullOptions::default(), &mut sink).await;
    let _ = std::fs::remove_file(&cut_flag);
    let progress_file = scratch.join(format!("pull-{db}.progress.json"));
    let staged_options = query(&mysql, &format!("{db}_pull"), "SELECT COUNT(*) FROM wp_options WHERE option_value='resume probe'");
    check(
        cut.is_err() && progress_file.is_file() && staged_options == "1",
        &format!("15. a pull cut off by the site keeps its staging copy and a record (options staged: {staged_options}; {})", cut.err().map(|e| e.to_string()).unwrap_or_default()),
    );
    let resumed = pull::pull_into(&client, &target, base15.as_ref(), &pull::PullOptions::default(), &mut sink).await;
    check(
        resumed.as_ref().map(|r| r.tables_resumed >= 1 && r.tables >= 1).unwrap_or(false)
            && query(&mysql, db, "SELECT COUNT(*) FROM wp_posts WHERE post_title='RESUME-ONE'") == "1"
            && query(&mysql, db, "SELECT option_value FROM wp_options WHERE option_name='blogdescription'") == "resume probe"
            && !progress_file.exists(),
        &format!("15b. the next pull reuses the staged table and exports the rest; the record is gone ({:?})", resumed.as_ref().map(|r| (r.tables_resumed, r.tables, r.tables_kept)).ok()),
    );
    base15 = resumed.map(|r| r.base).ok().or(base15);
    // 16. Resume a push: the site answers 500 to the SECOND file while the flag is set, so
    //     the push dies after the first file reached the quarantine; clear the flag and
    //     push again — the first file is not sent twice, and the swap happens.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    std::fs::write(local_root.join("wp-content/uploads/2026/a-first.txt"), "first file").unwrap();
    std::fs::write(local_root.join("wp-content/uploads/2026/b-second.txt"), "second file").unwrap();
    std::fs::write(&cut_flag, r#"{"route":"/rexenv-sync/v1/push/file","param":"path","value":"uploads/2026/b-second.txt"}"#).unwrap();
    let opts16 = push::PushOptions { tables: Some(vec![]), files: true, override_items: vec![], skip: vec![] };
    let died = push::push_from(&client, &target, base15.as_ref(), &opts16, &mut sink).await;
    let _ = std::fs::remove_file(&cut_flag);
    let record_file = scratch.join(format!("push-{db}.open.json"));
    let died_why = match &died { Ok(_) => "ok?!".to_string(), Err(push::PushStop::Conflicts(c)) => format!("conflicts {c:?}"), Err(push::PushStop::Failed(e)) => e.to_string() };
    check(matches!(&died, Err(push::PushStop::Failed(e)) if e.to_string().contains("continues from")) && record_file.is_file(), &format!("16. a push cut off by the site leaves the open push on live and a record ({died_why})"));
    let again = push::push_from(&client, &target, base15.as_ref(), &opts16, &mut sink).await;
    check(
        again.as_ref().map(|r| r.files_resumed >= 1 && r.files >= 2).unwrap_or(false)
            && std::fs::read_to_string(live_root.join("wp-content/uploads/2026/a-first.txt")).ok().as_deref() == Some("first file")
            && std::fs::read_to_string(live_root.join("wp-content/uploads/2026/b-second.txt")).ok().as_deref() == Some("second file")
            && !record_file.exists(),
        &format!("16b. the next push continues the open one — the first file not sent again — and swaps ({:?})", again.as_ref().map(|r| (r.files_resumed, r.files)).ok()),
    );

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
