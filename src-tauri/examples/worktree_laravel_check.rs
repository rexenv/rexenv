//! Live check: a worktree of a LARAVEL site's own repository (W10 of
//! `docs/PLAN-git-worktrees.md`, Shape B). Run:
//! `cargo run --example worktree_laravel_check`
//!
//! `worktree_site_check` proves Shape B on WordPress. This one proves the two
//! legs WordPress cannot reach:
//!   1. the job settles with config → deps → db → db_copy (no `urls`);
//!   2. the parent's ignored `.env` is copied and pointed at the child —
//!      `APP_URL` and `DB_DATABASE` — with every other key (APP_KEY) kept;
//!   3. `vendor/` is COPIED from the parent, because the branch's
//!      `composer.lock` is byte-identical — the job log says so, and the child
//!      boots `vendor/autoload.php`;
//!   4. the child's database holds the parent's migrated tables;
//!   5. a clean child deletes through git, folder and all; then the parent.
//!
//! Network tier: the parent is a real `laravel/laravel` created by the same job
//! the New Site dialog runs. The fixture manager adopts the database tier only
//! (the `git_site_provision_check` posture, ledger #805).

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::db::SqlClient;
use rexenv_lib::core::{database, sites, ssl};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tauri::{Listener, Manager};

mod common;

fn git_in(dir: &Path, args: &[&str]) {
    // Signing off: a machine whose global config signs every commit (the Dell
    // does, through gpg) would otherwise wait on a pinentry no SSH session has.
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "rexenv checks")
        .env("GIT_AUTHOR_EMAIL", "checks@rexenv.invalid")
        .env("GIT_COMMITTER_NAME", "rexenv checks")
        .env("GIT_COMMITTER_EMAIL", "checks@rexenv.invalid")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn query(mysql: &SqlClient, db: &str, sql: &str) -> String {
    let out = Command::new(mysql.path())
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
    let db_file = std::env::temp_dir().join("rexenv-worktree-laravel-check.db");
    let _ = std::fs::remove_file(&db_file);
    let conn = rexenv_lib::state::db::open(&db_file).unwrap();
    // A fixture sites folder, never the user's ~/rexenv/Sites (the `wp_plugins_check`
    // lesson: a check's leftovers there survive into the next run). `Worktrees/`
    // then lands beside THIS folder too.
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "wtlaravel");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let (mysql, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::pins().mysql)
        .await
        .expect("bundled MySQL client");
    let app = tauri::test::mock_app();
    app.manage(commands::repo::RepoJobs::default());
    app.manage(site_provision::ProvisionJobs::default());
    app.manage(commands::tunnels::Tunnels::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle().clone();
    let _engines = common::engines_as_found();
    {
        let state = handle.state::<AppState>();
        let mut mgr = state.services.lock().await;
        mgr.adopt_dbs(state.platform.as_ref());
    }
    let mut failures: Vec<String> = Vec::new();
    let mut created: Vec<String> = Vec::new();
    let wait_settled = |id: String| {
        let handle = handle.clone();
        async move {
            for _ in 0..2400 {
                let st = site_provision::state_of(&handle.state::<site_provision::ProvisionJobs>(), &id).unwrap();
                if st.status != "running" {
                    return st;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            panic!("job {id} never settled");
        }
    };
    let site_of = |id: &str| {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        sites::get(&conn, id).unwrap()
    };

    let parent_domain = format!("wtl{pid}.rex");
    println!("=== parent {parent_domain} (laravel/laravel) ===");
    let snap = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        NewSite {
            name: "Worktree Laravel".into(),
            domain: parent_domain.clone(),
            site_type: SiteType::Laravel,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        },
        None,
        None,
    )
    .await
    .expect("start the parent");
    created.extend(snap.site_id.clone());
    let fin = wait_settled(snap.id.clone()).await;
    println!("  parent settled {} — {:?}", fin.status, fin.summary.as_deref().or(fin.error.as_deref()));
    let Some(parent) = fin.site_id.as_deref().and_then(site_of) else {
        println!("FAIL: no parent row");
        return std::process::ExitCode::FAILURE;
    };
    if fin.status != "ok" {
        failures.push(format!("parent settled {}: {:?}", fin.status, fin.error));
        return finish(&handle, &mysql, &created, &db_file, failures);
    }
    let root = Path::new(&parent.path).to_path_buf();
    // Laravel's own .gitignore already leaves out vendor/ and .env.
    git_in(&root, &["init", "-q", "-b", "main"]);
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-q", "-m", "app"]);
    git_in(&root, &["checkout", "-q", "-b", "feature/lara"]);
    std::fs::write(root.join("routes/rexwt.txt"), "lara-feature\n").unwrap();
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-q", "-m", "feature"]);
    git_in(&root, &["checkout", "-q", "main"]);

    let req = commands::worktree::WorktreeRequest {
        parent_id: parent.id.clone(),
        asset_kind: None,
        asset_dir: None,
        branch: "feature/lara".into(),
        base: None,
        domain: None,
        skip_uploads: false,
    };
    let want = format!("feature-lara.{parent_domain}");
    println!("\n=== child {want} ===");
    let started = {
        let state = handle.state::<AppState>();
        let jobs = handle.state::<site_provision::ProvisionJobs>();
        commands::worktree::start(&handle, &state, &jobs, req)
    };
    let snap = match started {
        Ok(s) => s,
        Err(e) => {
            failures.push(format!("1: the child would not start: {e}"));
            return finish(&handle, &mysql, &created, &db_file, failures);
        }
    };
    created.extend(snap.site_id.clone());
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    {
        let l2 = lines.clone();
        handle.listen(site_provision::output_event(&snap.id), move |ev| {
            if let Ok(s) = serde_json::from_str::<String>(ev.payload()) {
                l2.lock().unwrap().push(s);
            }
        });
    }
    let fin = wait_settled(snap.id.clone()).await;
    println!("  child settled {} — {:?}", fin.status, fin.summary.as_deref().or(fin.error.as_deref()));
    let keys: Vec<&str> = fin.phases.iter().map(|p| p.key.as_str()).collect();
    if fin.status == "ok" && keys == ["prepare", "fetch", "config", "deps", "db", "db_copy", "serve"] {
        println!("1 ok — config → deps → db → db_copy, no urls");
    } else {
        for l in lines.lock().unwrap().iter() {
            println!("  | {l}");
        }
        failures.push(format!("1: settled {} ({:?}), phases {keys:?}", fin.status, fin.error));
    }
    let Some(child) = fin.site_id.as_deref().and_then(site_of) else {
        failures.push("no child row".into());
        return finish(&handle, &mysql, &created, &db_file, failures);
    };
    let folder = Path::new(&child.path).to_path_buf();
    let env_text = std::fs::read_to_string(folder.join(".env")).unwrap_or_default();
    let parent_env = std::fs::read_to_string(root.join(".env")).unwrap_or_default();
    let key_of = |t: &str, k: &str| t.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim_matches('"').to_string());
    if key_of(&env_text, "APP_URL").as_deref() == Some(format!("https://{want}").as_str())
        && key_of(&env_text, "DB_DATABASE").as_deref() == Some(child.db_name.as_str())
        && key_of(&env_text, "APP_KEY").is_some()
        && key_of(&env_text, "APP_KEY") == key_of(&parent_env, "APP_KEY")
        && key_of(&parent_env, "DB_DATABASE").as_deref() == Some(parent.db_name.as_str())
    {
        println!("2 ok — .env copied; APP_URL + DB_DATABASE are the child's, APP_KEY kept; the parent's untouched");
    } else {
        failures.push(format!("2: child .env APP_URL={:?} DB={:?}", key_of(&env_text, "APP_URL"), key_of(&env_text, "DB_DATABASE")));
    }
    let log = lines.lock().unwrap().join("\n");
    if folder.join("vendor/autoload.php").is_file() && log.contains("vendor/ copied") {
        println!("3 ok — vendor/ copied from the parent (same composer.lock)");
    } else {
        failures.push(format!("3: vendor/autoload.php={} log mentions copy={}", folder.join("vendor/autoload.php").is_file(), log.contains("vendor/ copied")));
    }
    let tables = query(&mysql, &child.db_name, "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'migrations'");
    if tables == "1" {
        println!("4 ok — the child's database holds the parent's migrated tables");
    } else {
        failures.push(format!("4: migrations table count {tables:?}"));
    }
    let del = commands::sites::delete_site(handle.state::<AppState>(), handle.state::<commands::tunnels::Tunnels>(), child.id.clone()).await;
    let del_parent = commands::sites::delete_site(handle.state::<AppState>(), handle.state::<commands::tunnels::Tunnels>(), parent.id.clone()).await;
    if matches!(del, Ok(true)) && !folder.exists() && matches!(del_parent, Ok(true)) {
        println!("5 ok — the child deleted through git, then the parent");
        if let Some(dir) = folder.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    } else {
        failures.push(format!("5: child {del:?} folder exists {} / parent {del_parent:?}", folder.exists()));
    }
    finish(&handle, &mysql, &created, &db_file, failures)
}

/// Tear down ONLY this run's records that are still there.
fn finish(
    handle: &tauri::AppHandle<tauri::test::MockRuntime>,
    mysql: &SqlClient,
    created: &[String],
    db_file: &Path,
    failures: Vec<String>,
) -> std::process::ExitCode {
    let state = handle.state::<AppState>();
    let parent_root = {
        let conn = state.db.lock().unwrap();
        created.first().and_then(|id| sites::get(&conn, id).ok().flatten()).map(|s| std::path::PathBuf::from(s.path))
    };
    for id in created.iter().rev() {
        let conn = state.db.lock().unwrap();
        let Ok(Some(site)) = sites::get(&conn, id) else { continue };
        if let (Ok(Some(w)), Some(root)) = (rexenv_lib::state::store::get_site_worktree(&conn, id), &parent_root) {
            let _ = Command::new("git").args(["worktree", "remove", "--force", &w.worktree_path]).current_dir(root).output();
        }
        let _ = rexenv_lib::core::db::DbEngine::Mysql.drop_database(mysql, database::MYSQL_PORT, &site.db_name);
        match sites::teardown(&conn, state.platform.as_ref(), id) {
            Ok(t) if t.existed => println!("cleaned up {} (docroot removed = {})", site.domain, t.docroot_removed),
            other => println!("teardown {} -> {other:?}", site.domain),
        }
    }
    let _ = std::fs::remove_file(db_file);
    if failures.is_empty() {
        println!("worktree_laravel_check: ALL PASS");
        std::process::ExitCode::SUCCESS
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::ExitCode::FAILURE
    }
}
