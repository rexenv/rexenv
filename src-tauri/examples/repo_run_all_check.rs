//! Live check: "Run all" sequencing (`run_offered_steps`, shared by the
//! panel's Run-all button and the CLI's `--install`) — the two honesty cases
//! that ARE the feature:
//!   1. STOP-ON-FAILURE: composer install succeeds, npm install FAILS (a
//!      deterministic `preinstall` that exits 1 — our own fixture, offline),
//!      build must read "skipped" (never "failed", never left "pending").
//!   2. CANCEL MID-STEP: npm install blocks on a `preinstall` sleep; cancel
//!      kills it → install "cancelled", build "skipped", nothing else ran.
//! Drives `cli_server::handle_request` in-process against a throwaway site
//! row (the cli_repo_check pattern) — real dispatch path, no services, no
//! running app touched. Composer resolves the bundled phar + pinned PHP from
//! the real binaries hub (cached on a dev machine).
//! Run: `cargo run --example repo_run_all_check`

use rexenv_lib::cli_server;
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{
    MultisiteMode, ServiceStatus, Site, SiteDbEngine, SiteOrigin, SiteType, WebServer,
};
use serde_json::{json, Value};
use std::path::Path;
use tauri::Manager;

fn git_in(dir: &Path, args: &[&str]) {
    let st = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=fx@rexenv.test", "-c", "user.name=fx"])
        .args(args)
        .output()
        .expect("git");
    assert!(st.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&st.stderr));
}

/// Plugin fixture: empty composer.json (install succeeds offline, no deps)
/// + package.json whose preinstall makes npm install fail or hang on demand.
fn fixture(dir: &Path, preinstall: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("plugin.php"), "<?php\n/*\nPlugin Name: RunAll Fixture\n*/\n").unwrap();
    std::fs::write(dir.join("composer.json"), "{}\n").unwrap();
    std::fs::write(
        dir.join("package.json"),
        format!(
            r#"{{ "name": "runall-fx", "version": "1.0.0",
  "scripts": {{ "preinstall": "{preinstall}", "build": "echo built" }} }}"#
        ),
    )
    .unwrap();
    git_in(dir, &["init", "-q"]);
    git_in(dir, &["add", "-A"]);
    git_in(dir, &["commit", "-qm", "init"]);
}

fn step_status(job: &Value, key: &str) -> String {
    job["steps"]
        .as_array()
        .and_then(|s| s.iter().find(|x| x["key"] == json!(key)))
        .and_then(|x| x["status"].as_str())
        .unwrap_or("<missing>")
        .to_string()
}

#[tokio::main]
async fn main() {
    let platform = rexenv_lib::platform::current();
    let conn = rexenv_lib::state::db::open_for_platform(platform.paths()).expect("open app db");
    let ca = rexenv_lib::core::ssl::load_or_create(platform.paths(), platform.permissions())
        .expect("load CA");

    let scratch = std::env::temp_dir().join(format!("rexenv-runall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let docroot = scratch.join("public");
    let plugins = docroot.join("wp-content/plugins");
    std::fs::create_dir_all(&plugins).unwrap();

    let site_id = uuid::Uuid::new_v4().to_string();
    let domain = format!("runall-{}.rex", std::process::id());
    let now = rexenv_lib::state::store::db_now(&conn).expect("now");
    rexenv_lib::state::store::insert_site(
        &conn,
        &Site {
            id: site_id.clone(),
            name: "run-all check".into(),
            domain,
            site_type: SiteType::Wordpress,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            ssl: false,
            path: docroot.to_string_lossy().into_owned(),
            created_at: now,
            multisite: MultisiteMode::None,
            db_name: format!("wp_runall_{}", std::process::id()),
            db_engine: SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed: Some(true),
            db_created: None,
            content_dir: None,
            mu_dir_created: None,
            origin: SiteOrigin::User,
            agent_client: None,
            expires_at: None,
            docroot_subdir: String::new(),
        },
    )
    .expect("insert site row");

    let app = tauri::test::mock_app();
    app.manage(rexenv_lib::commands::repo::RepoJobs::default());
    app.manage(rexenv_lib::commands::repo::RepoWatches::default());
    app.manage(AppState::new(conn, platform, ca));

    let ask = |cmd: &str, args: Value| {
        let line = json!({ "cmd": cmd, "args": args }).to_string();
        let handle = app.handle().clone();
        async move {
            let reply = cli_server::handle_request(&handle, line).await;
            serde_json::from_str::<Value>(&reply).expect("valid envelope")
        }
    };
    let mut failures: Vec<String> = Vec::new();

    // ── 1. STOP-ON-FAILURE ────────────────────────────────────────────────
    // preinstall exits 1 → npm install fails deterministically, offline.
    // (npm scripts run via sh — no quotes, the JSON must stay valid.)
    fixture(&plugins.join("fail-fx"), "exit 1");
    let r = ask("repo.check", json!({ "id": site_id, "dir": "fail-fx", "install": true })).await;
    if r["ok"] != json!(true) {
        failures.push(format!("repo.check --install failed outright: {}", r["error"]));
    }
    let job = &r["data"]["job"];
    let (chk, com, ins, bld) = (
        step_status(job, "check"),
        step_status(job, "composer"),
        step_status(job, "install"),
        step_status(job, "build"),
    );
    println!("fail case:   check={chk} composer={com} install={ins} build={bld}");
    if chk != "ok" || com != "ok" {
        failures.push(format!("fail case: check/composer should be ok, got {chk}/{com}"));
    }
    if ins != "failed" {
        failures.push(format!("fail case: install should be failed, got {ins}"));
    }
    if bld != "skipped" {
        failures.push(format!(
            "fail case: build after a failed install must be SKIPPED (not failed/pending), got {bld}"
        ));
    }
    // The fingerprint marker must reflect reality: composer succeeded →
    // recorded; npm failed → NOT recorded.
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().expect("db lock");
        let fps =
            rexenv_lib::state::store::get_git_asset_fps(&conn, &site_id, "plugin", "fail-fx")
                .expect("fps");
        // No provenance row exists for this unadopted fixture — both None is
        // also acceptable; what's FORBIDDEN is a node fp after a failed install.
        if fps.1.is_some() {
            failures.push(format!("fail case: node fp recorded despite failed install: {fps:?}"));
        }
    }

    // ── 2. CANCEL MID-STEP ────────────────────────────────────────────────
    // preinstall sleeps → install blocks; we cancel while it runs.
    fixture(&plugins.join("cancel-fx"), "sleep 300");
    let ask_cancel_case =
        ask("repo.check", json!({ "id": site_id, "dir": "cancel-fx", "install": true }));
    let runner = tokio::spawn(ask_cancel_case);
    // Wait until the install step is actually RUNNING, then cancel its job.
    let mut cancelled = false;
    for _ in 0..600 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let jobs = app.state::<rexenv_lib::commands::repo::RepoJobs>();
        let snap = rexenv_lib::commands::repo::repo_site_jobs(jobs, site_id.clone(), "plugin".into())
            .await
            .unwrap_or_default();
        let Some(job) = snap.iter().find(|j| j.dir_name == "cancel-fx" && j.op == "check") else {
            continue;
        };
        let installing =
            job.steps.iter().any(|s| s.key == "install" && s.status == "running");
        if installing {
            let state = app.state::<AppState>();
            let jobs = app.state::<rexenv_lib::commands::repo::RepoJobs>();
            rexenv_lib::commands::repo::repo_cancel(state, jobs, job.id.clone())
                .await
                .expect("cancel");
            cancelled = true;
            break;
        }
    }
    if !cancelled {
        failures.push("cancel case: install step never reached running".into());
    }
    let r = runner.await.expect("join");
    let job = &r["data"]["job"];
    let (com, ins, bld) = (
        step_status(job, "composer"),
        step_status(job, "install"),
        step_status(job, "build"),
    );
    println!("cancel case: composer={com} install={ins} build={bld}");
    if com != "ok" {
        failures.push(format!("cancel case: composer should have completed ok, got {com}"));
    }
    if ins != "cancelled" {
        failures.push(format!("cancel case: install should be cancelled, got {ins}"));
    }
    if bld != "skipped" {
        failures.push(format!("cancel case: build must be skipped, got {bld}"));
    }

    // Cleanup: site row + scratch.
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().expect("db lock");
        let _ = rexenv_lib::state::store::delete_site(&conn, &site_id);
    }
    let _ = std::fs::remove_dir_all(&scratch);
    println!();
    if failures.is_empty() {
        println!("repo_run_all_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
