//! Live check: a **plugin worktree site** end to end through the REAL
//! provisioning job (W4/W5 of `docs/PLAN-git-worktrees.md`, Shape A). Run:
//! `cargo run --example worktree_site_check`
//!
//! What it builds, all recorded as created and torn down from that record:
//!   - a WordPress parent site, installed by the same job the New Site dialog
//!     runs (network tier: core is downloaded);
//!   - a git repository IN the parent's `wp-content/plugins/rexwt-probe`, with
//!     `main` and a `feature/probe` branch whose plugin file differs;
//!   - the worktree child, through `commands::worktree::start`.
//!
//! Proves:
//!   1. the job settles ok with the copy's own phases, in order;
//!   2. the child's domain is `feature-probe.<parent>` (§2.2's nested shape) and
//!      the relation row is recorded (Shape A, the plugin, the branch asked for);
//!   3. the child's plugin folder is a WORKTREE (a `.git` file) on
//!      `feature/probe`, while the parent's checkout still holds `main`;
//!   4. the child's `wp-config.php` names the child's own database, and that
//!      database is a copy — its `siteurl` and the Hello-world `guid` read
//!      `https://feature-probe.<parent>`, NEVER doubled
//!      (`feature-probe.feature-probe.…`, ledger #818's real-wp-cli leg), while
//!      the parent's still read the parent;
//!   5. deleting the parent is refused (it has a child, #815), and deleting the
//!      child is refused (its folder holds a worktree, #814) — before anything;
//!   6. Delete on a child with an UNCOMMITTED file is refused, naming the file —
//!      site, database and file untouched (W6);
//!   7. Delete on a clean child: git removes the worktree, the folder and the
//!      child's database go, and the BRANCH is kept;
//!   8. the parent then deletes.
//!
//! Edge safety: the fixture manager adopts the database tier only, so the
//! serve phase is SKIPPED and the real edge is never touched (the
//! `git_site_provision_check` posture, ledger #805).

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::db::SqlClient;
use rexenv_lib::core::{database, sites, ssl, worktree};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tauri::{Listener, Manager};

mod common;

fn git_in(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
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
    let db_file = std::env::temp_dir().join("rexenv-worktree-site-check.db");
    let _ = std::fs::remove_file(&db_file);
    let conn = rexenv_lib::state::db::open(&db_file).unwrap();
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
        let n = mgr.adopt_dbs(state.platform.as_ref());
        println!("adopted {n} running database engine(s)\n");
    }

    let mut failures: Vec<String> = Vec::new();
    let mut created: Vec<String> = Vec::new();
    let collect = |id: &str| {
        let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let l2 = lines.clone();
        handle.listen(site_provision::output_event(id), move |ev| {
            if let Ok(s) = serde_json::from_str::<String>(ev.payload()) {
                l2.lock().unwrap().push(s);
            }
        });
        lines
    };
    let wait_settled = |id: String| {
        let handle = handle.clone();
        async move {
            for _ in 0..2400 {
                let st =
                    site_provision::state_of(&handle.state::<site_provision::ProvisionJobs>(), &id)
                        .unwrap();
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

    // ── the parent: a real WordPress ────────────────────────────────────────
    let parent_domain = format!("wtp{pid}.rex");
    println!("=== parent {parent_domain} ===");
    let snap = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        NewSite {
            name: "Worktree Parent".into(),
            domain: parent_domain.clone(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
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
    }

    // ── the plugin repository in the parent ─────────────────────────────────
    let plugin = parent.served_root().join("wp-content/plugins/rexwt-probe");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("rexwt-probe.php"), "<?php\n/* Plugin Name: rexwt probe */\n// branch: main\n").unwrap();
    git_in(&plugin, &["init", "-q", "-b", "main"]);
    git_in(&plugin, &["add", "."]);
    git_in(&plugin, &["commit", "-q", "-m", "main"]);
    git_in(&plugin, &["checkout", "-q", "-b", "feature/probe"]);
    std::fs::write(plugin.join("rexwt-probe.php"), "<?php\n/* Plugin Name: rexwt probe */\n// branch: feature\n").unwrap();
    git_in(&plugin, &["commit", "-q", "-am", "feature"]);
    git_in(&plugin, &["checkout", "-q", "main"]);

    // ── the child ───────────────────────────────────────────────────────────
    let req = commands::worktree::WorktreeRequest {
        parent_id: parent.id.clone(),
        asset_kind: worktree::AssetKind::Plugin,
        asset_dir: "rexwt-probe".into(),
        branch: "feature/probe".into(),
        base: None,
        domain: None,
        skip_uploads: false,
    };
    let want_domain = format!("feature-probe.{parent_domain}");
    println!("\n=== child {want_domain} ===");
    let started = {
        let state = handle.state::<AppState>();
        let jobs = handle.state::<site_provision::ProvisionJobs>();
        commands::worktree::start(&handle, &state, &jobs, req)
    };
    let snap = match started {
        Ok(s) => s,
        Err(e) => {
            println!("FAIL: the child would not start: {e}");
            failures.push(format!("child start: {e}"));
            return finish(&handle, &mysql, &plugin, &created, &db_file, failures);
        }
    };
    created.extend(snap.site_id.clone());
    let lines = collect(&snap.id);
    let fin = wait_settled(snap.id.clone()).await;
    println!("  child settled {} — {:?}", fin.status, fin.summary.as_deref().or(fin.error.as_deref()));
    let log = lines.lock().unwrap().clone();
    if fin.status != "ok" {
        for l in &log {
            println!("  | {l}");
        }
        failures.push(format!("1: child settled {} ({:?})", fin.status, fin.error));
    }
    let keys: Vec<&str> = fin.phases.iter().map(|p| p.key.as_str()).collect();
    let want_keys = ["prepare", "fetch", "copy", "worktree", "deps", "db", "db_copy", "configure", "urls", "serve"];
    if keys != want_keys {
        failures.push(format!("1: phases {keys:?}, want {want_keys:?}"));
    } else {
        println!("1 ok — the copy's own phases, in order");
    }

    let Some(child) = fin.site_id.as_deref().and_then(site_of) else {
        failures.push("2: no child row".into());
        return finish(&handle, &mysql, &plugin, &created, &db_file, failures);
    };
    let rel = {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        rexenv_lib::state::store::get_site_worktree(&conn, &child.id).unwrap()
    };
    match &rel {
        Some(w) if child.domain == want_domain
            && w.parent_id == parent.id
            && w.asset_kind.as_deref() == Some("plugin")
            && w.asset_dir.as_deref() == Some("rexwt-probe")
            && w.branch == "feature/probe" =>
        {
            println!("2 ok — {} recorded as a plugin worktree of {}", child.domain, parent.domain)
        }
        other => failures.push(format!("2: domain {} / relation {other:?}", child.domain)),
    }

    let child_plugin = child.served_root().join("wp-content/plugins/rexwt-probe");
    let child_src = std::fs::read_to_string(child_plugin.join("rexwt-probe.php")).unwrap_or_default();
    let parent_src = std::fs::read_to_string(plugin.join("rexwt-probe.php")).unwrap_or_default();
    if child_plugin.join(".git").is_file() && child_src.contains("feature") && parent_src.contains("main") {
        println!("3 ok — the child's plugin is a worktree on feature/probe; the parent still on main");
    } else {
        failures.push(format!(
            "3: .git file={} child={child_src:?} parent={parent_src:?}",
            child_plugin.join(".git").is_file()
        ));
    }

    let config = std::fs::read_to_string(child.served_root().join("wp-config.php")).unwrap_or_default();
    let names_child_db = worktree::defines_constant(&config, "DB_NAME") && config.contains(&child.db_name);
    let siteurl = query(&mysql, &child.db_name, "SELECT option_value FROM wp_options WHERE option_name='siteurl'");
    let guid = query(&mysql, &child.db_name, "SELECT guid FROM wp_posts WHERE ID=1");
    let parent_url = query(&mysql, &parent.db_name, "SELECT option_value FROM wp_options WHERE option_name='siteurl'");
    let want_url = format!("https://{want_domain}");
    let doubled = format!("feature-probe.feature-probe.{parent_domain}");
    if names_child_db
        && siteurl == want_url
        && guid.starts_with(&want_url)
        && !guid.contains(&doubled)
        && parent_url == format!("https://{parent_domain}")
    {
        println!("4 ok — own database; siteurl {siteurl}; guid {guid}; parent untouched");
    } else {
        failures.push(format!(
            "4: db in config={names_child_db} siteurl={siteurl:?} guid={guid:?} parent={parent_url:?}"
        ));
    }

    {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let p = sites::delete_preflight(&conn, state.platform.as_ref(), &parent);
        let c = sites::delete_preflight(&conn, state.platform.as_ref(), &child);
        match (p, c) {
            (Err(pe), Err(ce)) if pe.to_string().contains(&child.domain) && ce.to_string().contains("git worktree remove") => {
                println!("5 ok — parent refused (has a child), child refused (holds a worktree)")
            }
            (p, c) => failures.push(format!("5: parent {p:?} / child {c:?}")),
        }
    }

    // ── 6–8. removal through Delete (W6) ────────────────────────────────────
    let delete = |id: String| {
        let handle = handle.clone();
        async move {
            commands::sites::delete_site(
                handle.state::<AppState>(),
                handle.state::<commands::tunnels::Tunnels>(),
                id,
            )
            .await
        }
    };
    let db_exists = |name: &str| {
        query(&mysql, "mysql", &format!("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME='{name}'"))
            == name
    };
    let stray = child_plugin.join("uncommitted.php");
    std::fs::write(&stray, "<?php // work in progress").unwrap();
    match delete(child.id.clone()).await {
        Err(e) if e.to_string().contains("uncommitted.php")
            && site_of(&child.id).is_some()
            && db_exists(&child.db_name)
            && stray.exists() =>
        {
            println!("6 ok — a dirty worktree refuses Delete, naming the file; site, DB and file intact")
        }
        other => failures.push(format!("6: dirty delete gave {other:?}")),
    }
    std::fs::remove_file(&stray).unwrap();
    match delete(child.id.clone()).await {
        Ok(true) => {
            let wts = String::from_utf8_lossy(
                &Command::new("git").args(["worktree", "list", "--porcelain"]).current_dir(&plugin).output().unwrap().stdout,
            )
            .matches("worktree ")
            .count();
            let branch = String::from_utf8_lossy(
                &Command::new("git").args(["branch", "--list", "feature/probe"]).current_dir(&plugin).output().unwrap().stdout,
            )
            .trim()
            .to_string();
            if site_of(&child.id).is_none()
                && !child.served_root().exists()
                && !db_exists(&child.db_name)
                && wts == 1
                && branch.contains("feature/probe")
            {
                println!("7 ok — a clean worktree child deletes: folder, DB and worktree gone; the branch kept");
            } else {
                failures.push(format!(
                    "7: row={} folder={} db={} worktrees={wts} branch={branch:?}",
                    site_of(&child.id).is_some(),
                    child.served_root().exists(),
                    db_exists(&child.db_name)
                ));
            }
        }
        other => failures.push(format!("7: clean delete gave {other:?}")),
    }
    match delete(parent.id.clone()).await {
        Ok(true) if site_of(&parent.id).is_none() => println!("8 ok — with its child gone, the parent deletes"),
        other => failures.push(format!("8: parent delete gave {other:?}")),
    }

    finish(&handle, &mysql, &plugin, &created, &db_file, failures)
}

/// Tear down ONLY this run's records: the child's worktree through git (run in
/// the fixture's own plugin repo), then each site's database and row.
fn finish(
    handle: &tauri::AppHandle<tauri::test::MockRuntime>,
    mysql: &SqlClient,
    plugin_repo: &Path,
    created: &[String],
    db_file: &Path,
    failures: Vec<String>,
) -> std::process::ExitCode {
    println!();
    let state = handle.state::<AppState>();
    // Children first: a parent with children is refused by design.
    for id in created.iter().rev() {
        let conn = state.db.lock().unwrap();
        let Ok(Some(site)) = sites::get(&conn, id) else { continue };
        let wt = rexenv_lib::state::store::get_site_worktree(&conn, id).ok().flatten();
        if let Some(w) = &wt {
            let out = Command::new("git")
                .args(["worktree", "remove", "--force", &w.worktree_path])
                .current_dir(plugin_repo)
                .output();
            println!("git worktree remove {} -> {:?}", w.worktree_path, out.map(|o| o.status));
        }
        let _ = rexenv_lib::core::db::DbEngine::Mysql.drop_database(
            mysql,
            database::MYSQL_PORT,
            &site.db_name,
        );
        match sites::teardown(&conn, state.platform.as_ref(), id) {
            Ok(t) if t.existed => println!("cleaned up {} (docroot removed = {})", site.domain, t.docroot_removed),
            other => println!("teardown {} -> {other:?}", site.domain),
        }
    }
    let _ = std::fs::remove_file(db_file);
    if failures.is_empty() {
        println!("worktree_site_check: ALL PASS");
        std::process::ExitCode::SUCCESS
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::ExitCode::FAILURE
    }
}
