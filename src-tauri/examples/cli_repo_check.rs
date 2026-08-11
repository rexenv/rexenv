//! Live check: the `rex repo` wave-1 dispatch arms against REAL state — a
//! throwaway site row (inserted directly, no provisioning) whose docroot is a
//! scratch dir with real git fixtures. Drives `cli_server::handle_request`
//! in-process (the socket layer is already proven by cli_socket_check), so it
//! never touches a running app's socket and starts no services.
//! Run: `cargo run --example cli_repo_check`

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
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
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

    // ── wave 2: job-shaped arms ──────────────────────────────────────────

    // Local bare origin so pull/fetch/checkout/push run with ZERO network.
    let origin = scratch.join("origin.git");
    std::process::Command::new("git")
        .args(["init", "--bare", "-q"])
        .arg(&origin)
        .status()
        .expect("bare init");
    git_in(&fx, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git_in(&fx, &["add", "-A"]);
    git_in(&fx, &["commit", "-qm", "wave2 base"]);
    git_in(&fx, &["push", "-qu", "origin", "HEAD"]);
    // A second clone advances the remote so pull has something to do.
    let seed = scratch.join("seed");
    git_in(&scratch, &["clone", "-q", origin.to_str().unwrap(), "seed"]);
    std::fs::write(seed.join("advance.txt"), "x").unwrap();
    git_in(&seed, &["add", "-A"]);
    git_in(&seed, &["commit", "-qm", "advance"]);
    git_in(&seed, &["push", "-q"]);

    let cap = std::time::Duration::from_secs(120);
    // fetch → pull (file arrives) → local commit → push (visible at origin).
    for (op, extra) in [("fetch", json!({})), ("pull", json!({}))] {
        let mut args = json!({ "id": site_id, "dir": "managed-fixture", "op": op });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let r = tokio::time::timeout(cap, ask("repo.op", args)).await.expect("op in time");
        let d = expect_ok!(format!("repo.op {op}"), r);
        let ok = d["job"]["steps"][0]["status"] == json!("ok");
        if !ok {
            failures.push(format!("{op} step not ok: {}", d["job"]));
        }
        if op == "pull" && !fx.join("advance.txt").is_file() {
            failures.push("pull did not fast-forward the file in".into());
        }
        if d["log"].as_array().map(|l| l.is_empty()).unwrap_or(true) {
            failures.push(format!("{op} reply carried no job log"));
        }
    }
    std::fs::write(fx.join("pushed-from-cli.txt"), "x").unwrap();
    git_in(&fx, &["add", "-A"]);
    git_in(&fx, &["commit", "-qm", "from dispatch check"]);
    let r = tokio::time::timeout(
        cap,
        ask("repo.op", json!({ "id": site_id, "dir": "managed-fixture", "op": "push" })),
    )
    .await
    .expect("push in time");
    let d = expect_ok!("repo.op push", r);
    if d["job"]["steps"][0]["status"] != json!("ok") {
        failures.push(format!("push step not ok: {}", d["job"]));
    }
    git_in(&seed, &["fetch", "-q"]);
    let seen = std::process::Command::new("git")
        .args(["-C", seed.to_str().unwrap(), "cat-file", "-e"])
        .arg("origin/master:pushed-from-cli.txt")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !seen {
        failures.push("pushed commit not visible at origin".into());
    }
    println!("ops: fetch/pull/push ok (pull file arrived, push seen at origin = {seen})");

    // checkout the feat branch (created earlier).
    let r = tokio::time::timeout(
        cap,
        ask(
            "repo.op",
            json!({ "id": site_id, "dir": "managed-fixture", "op": "checkout", "ref": "feat" }),
        ),
    )
    .await
    .expect("checkout in time");
    let d = expect_ok!("repo.op checkout", r);
    if d["job"]["steps"][0]["status"] != json!("ok") {
        failures.push(format!("checkout step not ok: {}", d["job"]));
    }
    let st = expect_ok!(
        "post-checkout status",
        ask("repo.status", json!({ "id": site_id, "dir": "managed-fixture" })).await
    );
    if st["branch"] != json!("feat") {
        failures.push(format!("checkout landed on {}", st["branch"]));
    }
    println!("checkout: branch now {}", st["branch"]);
    git_in(&fx, &["checkout", "-q", "-"]); // back for the run test

    // repo.run: one-shot script to completion with the log in the reply.
    std::fs::write(
        fx.join("package.json"),
        r#"{"name":"fx","version":"1.0.0","scripts":{"watch":"node watch.js","oneshot":"node -e \"console.log('one-shot-ran')\""}}"#,
    )
    .unwrap();
    let r = tokio::time::timeout(
        cap,
        ask(
            "repo.run",
            json!({ "id": site_id, "dir": "managed-fixture", "script": "oneshot" }),
        ),
    )
    .await
    .expect("run in time");
    let d = expect_ok!("repo.run", r);
    let log_hit = d["log"]
        .as_array()
        .map(|l| l.iter().any(|x| x.as_str().unwrap_or("").contains("one-shot-ran")))
        .unwrap_or(false);
    if d["job"]["steps"][0]["status"] != json!("ok") || !log_hit {
        failures.push(format!("run wrong: {} log_hit={log_hit}", d["job"]));
    }
    println!("run: script output in completion log = {log_hit}");

    // repo.add against a real (tiny) public repo — the one networked step.
    let r = tokio::time::timeout(
        std::time::Duration::from_secs(180),
        ask(
            "repo.add",
            json!({
                "id": site_id,
                "url": "https://github.com/octocat/Hello-World.git",
                "name": "hello-cli"
            }),
        ),
    )
    .await
    .expect("add in time");
    let d = expect_ok!("repo.add", r);
    let detect_ok = d["job"]["steps"]
        .as_array()
        .map(|s| s.iter().any(|x| x["key"] == json!("detect") && x["status"] == json!("ok")))
        .unwrap_or(false);
    if !detect_ok || !plugins.join("hello-cli/.git").exists() {
        failures.push(format!("add wrong: {}", d["job"]));
    }
    println!("add: cloned + detected (dir on disk = {})", plugins.join("hello-cli").exists());

    // Linked delete THROUGH the guarded wp arm: unlink only, target intact.
    let r = ask("wp.plugin.delete", json!({ "id": site_id, "names": ["linked-fx"] })).await;
    if r["ok"] != json!(true) {
        failures.push(format!("wp.plugin.delete over link failed: {}", r["error"]));
    }
    let link_gone = std::fs::symlink_metadata(plugins.join("linked-fx")).is_err();
    let target_ok = ext.join("plugin.php").is_file() && ext.join(".git").exists();
    if !link_gone || !target_ok {
        failures.push(format!(
            "GUARD VIOLATION: link_gone={link_gone} target_intact={target_ok}"
        ));
    }
    println!("guarded delete: link gone = {link_gone}, target intact = {target_ok}");

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
