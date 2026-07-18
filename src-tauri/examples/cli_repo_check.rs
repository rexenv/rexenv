//! Live check: the `rex repo` wave-1 dispatch arms against REAL state — a
//! throwaway site row (inserted directly, no provisioning) whose docroot is a
//! scratch dir with real git fixtures. Drives `cli_server::handle_request`
//! in-process (the socket layer is already proven by cli_socket_check), so it
//! never touches a running app's socket and starts no services.
//! Run: `cargo run --example cli_repo_check`

use rexenv_lib::cli_server;
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{
    MultisiteMode, ServiceStatus, Site, SiteDbEngine, SiteType, WebServer,
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

fn plugin_fixture(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("plugin.php"), "<?php\n/*\nPlugin Name: CLI Fixture\n*/\n").unwrap();
    git_in(dir, &["init", "-q"]);
    git_in(dir, &["add", "-A"]);
    git_in(dir, &["commit", "-qm", "init"]);
}

#[tokio::main]
async fn main() {
    let platform = rexenv_lib::platform::current();
    let conn = rexenv_lib::state::db::open_for_platform(platform.paths()).expect("open app db");
    let ca = rexenv_lib::core::ssl::load_or_create(platform.paths(), platform.permissions())
        .expect("load CA");

    let scratch = std::env::temp_dir().join(format!("rexenv-cli-repo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let docroot = scratch.join("public");
    let plugins = docroot.join("wp-content/plugins");
    std::fs::create_dir_all(&plugins).unwrap();

    // Throwaway site ROW (no provisioning, no services — repo arms only read
    // the row + the filesystem). Removed at the end.
    let site_id = uuid::Uuid::new_v4().to_string();
    let domain = format!("cli-repo-{}.rex", std::process::id());
    let now = rexenv_lib::state::store::db_now(&conn).expect("now");
    rexenv_lib::state::store::insert_site(
        &conn,
        &Site {
            id: site_id.clone(),
            name: "cli repo check".into(),
            domain: domain.clone(),
            site_type: SiteType::Wordpress,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            ssl: false,
            path: docroot.to_string_lossy().into_owned(),
            created_at: now,
            multisite: MultisiteMode::None,
            db_name: format!("wp_cli_repo_{}", std::process::id()),
            db_engine: SiteDbEngine::Mysql,
            xdebug: false,
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
    macro_rules! expect_ok {
        ($name:expr, $reply:expr) => {{
            let r = $reply;
            if r["ok"] != json!(true) {
                failures.push(format!("{}: {}", $name, r["error"]));
            }
            r["data"].clone()
        }};
    }

    // 1. Empty list.
    let d = expect_ok!("repo.list empty", ask("repo.list", json!({ "id": site_id })).await);
    if d["assets"].as_array().map(|a| a.len()) != Some(0) {
        failures.push(format!("expected empty asset list, got {}", d["assets"]));
    }

    // 2. Adopt a manual checkout.
    let fx = plugins.join("managed-fixture");
    plugin_fixture(&fx);
    git_in(&fx, &["checkout", "-qb", "feat"]);
    git_in(&fx, &["checkout", "-q", "-"]);
    let _ = expect_ok!(
        "repo.adopt",
        ask("repo.adopt", json!({ "id": site_id, "dir": "managed-fixture" })).await
    );
    let d = expect_ok!("repo.list", ask("repo.list", json!({ "id": site_id, "status": true })).await);
    let n = d["assets"].as_array().map(|a| a.len()).unwrap_or(0);
    if n != 1 || d["assets"][0]["source"] != json!("adopted") {
        failures.push(format!("adopted list wrong: {}", d["assets"]));
    }
    if d["statuses"]["plugin/managed-fixture"]["branch"].as_str().is_none() {
        failures.push("list --status missing live status".into());
    }

    // 3. Status: clean → dirty flip.
    std::fs::write(fx.join("new.txt"), "x").unwrap();
    let d = expect_ok!(
        "repo.status",
        ask("repo.status", json!({ "id": site_id, "dir": "managed-fixture" })).await
    );
    if d["untracked"].as_u64() != Some(1) {
        failures.push(format!("status untracked wrong: {d}"));
    }
    println!("status: branch={} untracked={}", d["branch"], d["untracked"]);

    // 4. Branches: current marked, feat listed.
    let d = expect_ok!(
        "repo.branches",
        ask("repo.branches", json!({ "id": site_id, "dir": "managed-fixture" })).await
    );
    let locals = d["local"].as_array().cloned().unwrap_or_default();
    if !locals.iter().any(|b| b == &json!("feat")) || d["current"].as_str().is_none() {
        failures.push(format!("branches wrong: {d}"));
    }

    // 5. Link an external checkout.
    let ext = scratch.join("elsewhere/linked-fx");
    plugin_fixture(&ext);
    let d = expect_ok!(
        "repo.link",
        ask(
            "repo.link",
            json!({ "id": site_id, "target": ext.to_string_lossy(), "name": "linked-fx" })
        )
        .await
    );
    if d["isGit"] != json!(true) {
        failures.push(format!("link result wrong: {d}"));
    }
    let is_link = std::fs::symlink_metadata(plugins.join("linked-fx"))
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if !is_link {
        failures.push("linked dir is not a symlink".into());
    }
    println!("linked: symlink on disk = {is_link}");

    // 6. Watch start → list → stop; no orphans against the fixture path.
    std::fs::write(
        fx.join("package.json"),
        r#"{"name":"fx","version":"1.0.0","scripts":{"watch":"node watch.js"}}"#,
    )
    .unwrap();
    std::fs::write(fx.join("watch.js"), "setInterval(()=>console.log('tick'),200);").unwrap();
    let d = expect_ok!(
        "repo.watch.start",
        ask(
            "repo.watch.start",
            json!({ "id": site_id, "dir": "managed-fixture", "script": "watch" })
        )
        .await
    );
    if d["status"] != json!("running") {
        failures.push(format!("watch start wrong: {d}"));
    }
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let d = expect_ok!(
        "repo.watch.list",
        ask("repo.watch.list", json!({ "id": site_id })).await
    );
    if d["watchers"].as_array().map(|w| w.len()) != Some(1) {
        failures.push(format!("watch list wrong: {d}"));
    }
    let _ = expect_ok!(
        "repo.watch.stop",
        ask("repo.watch.stop", json!({ "id": site_id, "dir": "managed-fixture" })).await
    );
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let survivors = std::process::Command::new("pgrep")
        .args(["-f", "managed-fixture"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if survivors {
        failures.push("watch orphans survive stop".into());
    }
    println!("watch: started, listed, stopped, orphans = {survivors}");

    // 7. Tools resolve (login-shell env).
    let d = expect_ok!("repo.tools", ask("repo.tools", json!({})).await);
    let ok_names: Vec<&str> = d["tools"]
        .as_array()
        .map(|t| {
            t.iter()
                .filter(|x| x["ok"] == json!(true))
                .filter_map(|x| x["name"].as_str())
                .collect()
        })
        .unwrap_or_default();
    println!("tools ok: {ok_names:?}");
    if !ok_names.contains(&"git") || !ok_names.contains(&"node") {
        failures.push(format!("tools wrong: {}", d["tools"]));
    }

    // 8. Unknown dir errors honestly (envelope ok:false, no panic).
    let r = ask("repo.status", json!({ "id": site_id, "dir": "nope" })).await;
    if r["ok"] != json!(false) {
        failures.push("missing-dir status did not error".into());
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
        println!("cli_repo_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
