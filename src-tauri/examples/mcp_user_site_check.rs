//! Live check: the PARITY tools over a REAL MCP socket — the switch, the grant
//! gate, a real change to the user's own site through the app's own command,
//! and a real delete gated by `destroy`.
//!
//!   cargo run --example mcp_user_site_check
//!
//! The companion to `mcp_scratch_check` (the executing registry) and
//! `mcp_socket_check` (the read registry). This one drives the THIRD registry
//! (`mcp_server/user_sites.rs`, `docs/PLAN-mcp-parity.md`), so what it checks
//! is the thing the unit tests' fake app cannot: that the refusals hold when
//! the call arrives as bytes on a socket through the real dispatch, that a
//! granted change really runs the app's own command against a real row, and
//! that a delete really removes a docroot on disk.
//!
//! # What this proves, and what it deliberately does not
//!
//! Proves, end to end: with the switch OFF every parity tool refuses by the
//! switch's own name and records no ask (#472/#473); with it ON and no grant,
//! the refusal names the Agent access dial and the level it needs (D15)
//! (#469/#471); a `manage` grant lets `site_configure` rename the row and set an
//! env var whose VALUE never comes back (#477); `site_delete` under that same
//! `manage` grant is refused naming `destroy` (#476); a session `destroy` grant
//! runs the app's full delete — the docroot is GONE from disk, the row is gone
//! (#476); a scratch site is refused by every parity tool with the scratch
//! tools named (#471); `site_info` names no path and `site_inspect_folder`
//! refuses app-data with the dialog's words (#479); every call lands in the feed
//! naming the site with the action as its summary (#206/#222); and the launch
//! sweep ends the session grant so the next delete is refused again (#468/#473).
//!
//! Does NOT prove, and says so rather than implying otherwise:
//!
//! - **`site_create`, `site_retry`, `site_restart` for real.** A provision
//!   downloads WordPress and starts the database engine; a restart reloads the
//!   web tier. All three are `stack` territory. Here the create tool is exercised
//!   only up to its refusals (a taken domain, the switch, the missing grant) —
//!   which are the parts that are rexenv's; the job itself is the app's and is
//!   proven where the app proves it. SMOKE §P2 is the human leg.
//! - **A database drop.** The fixture site records `db_created = 0`, so the
//!   app's delete skips the drop by provenance — the same rule a linked site's
//!   delete follows — and this example never needs an engine.
//!
//! # Fixture ownership (the invariant — `examples/common/mod.rs`)
//!
//! Sandboxed `Platform`, `sandbox_db` (which pins `sites_dir` INTO the sandbox)
//! and a socket under the sandbox root, so the delete this example performs
//! removes a docroot it created itself, under a Sites folder it created itself.
//! Nothing is provisioned and no service is spawned.

use rexenv_lib::commands::tunnels::Tunnels;
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use rexenv_lib::state::store;
use rexenv_lib::{core, mcp_server};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use tauri::Manager;

mod common;

struct TempTree(PathBuf);
impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn send(stream: &mut UnixStream, msg: &str) {
    stream.write_all(msg.as_bytes()).expect("write message");
    stream.write_all(b"\n").expect("write newline");
    stream.flush().expect("flush");
}

fn read_reply(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read reply line");
    serde_json::from_str(line.trim()).expect("reply is valid JSON-RPC")
}

fn call(stream: &mut UnixStream, reader: &mut impl BufRead, id: u32, name: &str, args: Value) -> (bool, String) {
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": args } });
    send(stream, &req.to_string());
    let v = read_reply(reader);
    let text = v["result"]["content"][0]["text"].as_str().unwrap_or("").to_string();
    (v["result"]["isError"].as_bool().unwrap_or(false), text)
}

#[tokio::main]
async fn main() {
    let (plat, _sandbox) = common::sandbox("mcp_user_site_check");
    let sandbox_root = plat.paths().app_data_dir().expect("sandbox data dir");
    let conn = common::sandbox_db(plat.as_ref());
    let ca = core::ssl::load_or_create(plat.paths(), plat.permissions()).expect("sandbox CA");
    let sites_dir = PathBuf::from(store::get_setting(&conn, "sites_dir").unwrap().expect("pinned sites dir"));

    // Two fixture sites: the USER'S (a blank PHP site with a real docroot the
    // delete will remove) and the agent's (a scratch row).
    let mk = |domain: &str, name: &str, ty: SiteType| {
        core::sites::create(
            &conn,
            NewSite {
                name: name.into(),
                domain: domain.into(),
                site_type: ty,
                php_version: "8.2".into(),
                web_server: WebServer::Nginx,
                path: String::new(),
                db_engine: SiteDbEngine::Mysql,
                git_url: String::new(),
                git_ref: None,
                git_migrate: true,
                git_build_assets: false,
                starter_db: false,
            },
        )
        .expect("create fixture site")
    };
    let mine = mk("mine.rex", "Mine", SiteType::Php);
    let theirs = mk("probe.scratch.rex", "probe", SiteType::Wordpress);
    let docroot = sites_dir.join("mine.rex");
    std::fs::create_dir_all(&docroot).expect("fixture docroot");
    std::fs::write(docroot.join("index.php"), b"<?php echo 'mine';\n").expect("index");
    // Provenance: our docroot (teardown removes it), NOT our database (the drop
    // is skipped, so no engine is needed).
    conn.execute(
        "UPDATE sites SET path = ?1, docroot_managed = 1, db_created = 0, provisioned = 1 WHERE id = ?2",
        rusqlite::params![docroot.to_string_lossy(), mine.id],
    )
    .expect("record the user's site");
    conn.execute(
        "UPDATE sites SET origin = 'agent', agent_client = 'mcp_user_site_check', \
         expires_at = datetime('now', '+1 hours'), docroot_managed = 1 WHERE id = ?1",
        [&theirs.id],
    )
    .expect("record the agent's site");
    let before_id: i64 = conn.query_row("SELECT COALESCE(MAX(id),0) FROM agent_actions", [], |r| r.get(0)).unwrap();

    let app = tauri::test::mock_app();
    app.manage(AppState::new(conn, plat, ca));
    app.manage(Tunnels::default());
    let state = app.state::<AppState>();

    let sock = sandbox_root.join(mcp_server::SOCKET_FILE);
    let listener = mcp_server::bind_socket(&sock).expect("bind the sandbox MCP socket");
    let (_shutdown, rx) = tokio::sync::watch::channel(true);
    tokio::spawn(mcp_server::serve(listener, app.handle().clone(), rx));

    let mut stream = UnixStream::connect(&sock).expect("connect");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    send(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"mcp_user_site_check","version":"1"}}}"#,
    );
    assert_eq!(read_reply(&mut reader)["result"]["serverInfo"]["name"], "rexenv");
    send(&mut stream, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    // 1) tools/list advertises all three registries.
    send(&mut stream, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let v = read_reply(&mut reader);
    let names: Vec<&str> = v["result"]["tools"].as_array().expect("tools").iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in ["list_sites", "site_info", "site_inspect_folder", "wp_org_search", "scratch_create_site", "site_create", "site_delete", "site_configure", "site_restart", "site_retry", "wp_info", "wp_plugin", "wp_theme", "wp_user", "wp_option", "wp_maintain", "wp_data", "wp_network", "site_wp_run", "site_logs", "mail_inbox", "site_artisan", "composer_link"] {
        assert!(names.contains(&expected), "`{expected}` is not advertised: {names:?}");
    }
    let destructive = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "site_delete").unwrap();
    assert_eq!(destructive["annotations"]["destructiveHint"], true, "site_delete must carry destructiveHint");
    println!("✓ tools/list → {} tool(s), all three registries advertised", names.len());

    let mut next = 10u32;
    let mut c = |name: &str, args: Value| {
        next += 1;
        call(&mut stream, &mut reader, next, name, args)
    };
    let asks = |state: &AppState| state.agent_site_requests.lock().unwrap().list().to_vec();


    // 2) The dial at Read (the default): a read is free, a change is refused
    //    naming the dial and the level it needs, and NO ask is recorded — asks
    //    are share's alone now (D15).
    let dial = |state: &AppState, level: core::agent_access::AccessLevel| {
        let conn = state.db.lock().unwrap();
        let mode = if level == core::agent_access::AccessLevel::Read { None } else { Some(core::agent_access::Mode::Always) };
        core::agent_access::set(&conn, level, mode).unwrap();
    };
    for (tool, args, needs) in [
        ("site_configure", json!({ "site_id": mine.id, "action": "rename", "name": "Renamed" }), "Changes"),
        ("site_delete", json!({ "site_id": mine.id }), "Full"),
        ("site_restart", json!({ "site_id": mine.id }), "Changes"),
        ("site_create", json!({ "name": "New", "domain": "new.rex", "type": "php", "php": "8.2" }), "Changes"),
    ] {
        let (err, text) = c(tool, args);
        assert!(err && text.contains("`Agent access`") && text.contains(needs), "{tool} at Read must name the dial and {needs}: {text}");
    }
    assert!(asks(&state).is_empty(), "the dial records no ask");
    let (err, text) = c("site_info", json!({ "site_id": mine.id }));
    assert!(!err, "a read is free at Read: {text}");
    println!("✓ dial at Read → reads free; every change refuses naming `Agent access` and its level; nothing asked");

    // 3) A SHAPE refusal (taken domain) is refused before the dial is consulted.
    let (err, text) = c("site_create", json!({ "name": "Dup", "domain": "mine.rex", "type": "php", "php": "8.2" }));
    assert!(err && text.contains("already reaches") && !text.contains("`Agent access`"), "{text}");
    println!("✓ a shape refusal comes before the dial");

    // 4) The dial at Changes: the rename really runs the app's command, and the
    //    row changes. An env var is set; its VALUE is not in the reply.
    dial(&state, core::agent_access::AccessLevel::Changes);
    let (err, text) = c("site_configure", json!({ "site_id": mine.id, "action": "rename", "name": "Renamed" }));
    assert!(!err, "rename with a manage grant: {text}");
    let reply: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(reply["name"], "Renamed");
    assert_eq!(reply["owner"], "user");
    {
        let conn = state.db.lock().unwrap();
        let name: String = conn.query_row("SELECT name FROM sites WHERE id = ?1", [&mine.id], |r| r.get(0)).unwrap();
        assert_eq!(name, "Renamed", "the app's own command ran against the real row");
    }
    let (err, text) = c("site_configure", json!({ "site_id": mine.id, "action": "env_set", "key": "API_KEY", "value": "s3cret-value" }));
    assert!(!err, "{text}");
    assert!(text.contains("API_KEY") && !text.contains("s3cret-value"), "an env VALUE left rexenv: {text}");
    println!("✓ dial at Changes → rename ran the app's command (row renamed); env set, value never returned");

    // 5) Delete at Changes: refused naming `destroy` and Full; docroot intact.
    let (err, text) = c("site_delete", json!({ "site_id": mine.id }));
    assert!(err && text.contains("`destroy`") && text.contains("Full"), "{text}");
    assert!(docroot.join("index.php").is_file(), "a refused delete touched the docroot");
    // The scratch site: every parity tool refuses it by name, whatever the dial says.
    dial(&state, core::agent_access::AccessLevel::Full);
    for (tool, args) in [
        ("site_configure", json!({ "site_id": theirs.id, "action": "rename", "name": "x" })),
        ("site_delete", json!({ "site_id": theirs.id })),
    ] {
        let (err, text) = c(tool, args);
        assert!(err && text.contains("scratch_delete_site"), "{tool} on a scratch site must name the scratch tools: {text}");
    }
    dial(&state, core::agent_access::AccessLevel::Changes);
    println!("✓ delete at Changes refused naming `destroy`/Full; a scratch site refused by every parity tool even at Full");

    // 6) site_info names no path; site_inspect_folder refuses app-data with the
    //    dialog's reason and classifies a real folder.
    let (err, text) = c("site_info", json!({ "site_id": mine.id }));
    assert!(!err, "{text}");
    let info: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(info["owner"], "user");
    assert_eq!(info["type"], "php");
    assert!(info["status"]["verdict"].is_string());
    assert!(!text.contains(&sandbox_root.display().to_string()) && !text.contains("/Users/"), "a path in site_info: {text}");
    let (err, text) = c("site_inspect_folder", json!({ "path": sandbox_root.display().to_string() }));
    assert!(err, "app-data must be refused: {text}");
    let probe = std::env::temp_dir().join(format!("rexenv-inspect-{}", std::process::id()));
    let _probe_guard = TempTree(probe.clone());
    std::fs::create_dir_all(&probe).unwrap();
    std::fs::write(probe.join("index.php"), b"<?php\n").unwrap();
    let (err, text) = c("site_inspect_folder", json!({ "path": probe.display().to_string() }));
    assert!(!err, "{text}");
    let found: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(found["type"], "php");
    assert_eq!(std::fs::read_dir(&probe).unwrap().count(), 1, "inspect created something");
    println!("✓ site_info carries no path; site_inspect_folder refuses app-data and classifies a real folder");

    // 6b) The P3 surfaces over the socket, on the grant from step 4 (manage ⊃
    //     read): a WordPress tool on a PHP site is refused on the row BEFORE any
    //     ask; on the scratch site it names wp_run; site_logs lists the site's
    //     sources with no path and refuses a key outside them; mail_inbox is
    //     refused by the mail switch's own label before any grant is consulted.
    let asks_before = asks(&state).len();
    let (err, text) = c("wp_plugin", json!({ "site_id": mine.id, "action": "list" }));
    assert!(err && text.contains("not WordPress"), "{text}");
    let (err, text) = c("wp_info", json!({ "site_id": theirs.id, "what": "info" }));
    assert!(err && text.contains("wp_run"), "{text}");
    // P7: the Laravel runner and the Composer link refuse on the ROW too — a
    // PHP site has no artisan and (here) no composer.json; the scratch site
    // is WordPress — with no ask recorded for any of them.
    let (err, text) = c("site_artisan", json!({ "site_id": mine.id, "args": ["about"] }));
    assert!(err && text.contains("not a Laravel site"), "{text}");
    let (err, text) = c("site_artisan", json!({ "site_id": theirs.id, "args": ["about"] }));
    assert!(err && text.contains("scratch"), "{text}");
    let (err, text) = c("composer_link", json!({ "site_id": mine.id, "source": sandbox_root.display().to_string() }));
    assert!(err && text.contains("no composer.json at its project root"), "{text}");
    assert_eq!(asks(&state).len(), asks_before, "row refusals ask for nothing");
    let (err, text) = c("site_logs", json!({ "site_id": mine.id }));
    assert!(!err, "site_logs under manage (⊃ read): {text}");
    let logs: Value = serde_json::from_str(&text).unwrap();
    let keys: Vec<String> = logs["result"]["sources"].as_array().unwrap().iter().map(|s| s["key"].as_str().unwrap().to_string()).collect();
    assert!(!keys.is_empty() && !text.contains(&sandbox_root.display().to_string()), "sources listed without paths: {text}");
    let (err, text) = c("site_logs", json!({ "site_id": mine.id, "source": "../../etc/passwd" }));
    assert!(err && text.contains("not one of"), "{text}");
    let (err, text) = c("site_logs", json!({ "site_id": mine.id, "source": keys[0] }));
    assert!(!err, "a real key tails (the file may be empty in the sandbox): {text}");
    let (err, text) = c("mail_inbox", json!({ "action": "list" }));
    assert!(err && text.contains("Let agents read scratch-site mail"), "{text}");
    assert_eq!(asks(&state).len(), asks_before, "the mail switch refuses before the grant is even asked for");
    println!("✓ wp/artisan/composer tools refuse a PHP/scratch site on the row; site_logs lists keys without paths and refuses a stray key; mail_inbox refuses by the mail switch's name");

    // 7) The dial at Full, for this session: the app's full delete runs — the
    //    docroot is gone from disk and the row is gone.
    {
        let conn = state.db.lock().unwrap();
        core::agent_access::set(&conn, core::agent_access::AccessLevel::Full, Some(core::agent_access::Mode::Session)).unwrap();
    }
    let (err, text) = c("site_delete", json!({ "site_id": mine.id }));
    assert!(!err, "delete with a destroy grant: {text}");
    assert!(!docroot.exists(), "the docroot must be gone from disk");
    {
        let conn = state.db.lock().unwrap();
        assert!(store::get_site(&conn, &mine.id).unwrap().is_none(), "the row must be gone");
    }
    println!("✓ dial at Full (session) → the app's delete removed the docroot and the row");

    // 8) The feed: every call landed, naming the site, with the action as the
    //    summary where there is one.
    {
        let conn = state.db.lock().unwrap();
        let rows: Vec<(String, Option<String>, Option<String>, String)> = conn
            .prepare("SELECT tool, target_site, args_summary, outcome FROM agent_actions WHERE id > ?1 ORDER BY id")
            .unwrap()
            .query_map([before_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(rows.iter().any(|(t, s, a, o)| t == "site_configure" && s.as_deref() == Some(mine.id.as_str()) && a.as_deref() == Some("rename") && o == "ok"), "{rows:?}");
        assert!(rows.iter().any(|(t, s, _, o)| t == "site_delete" && s.as_deref() == Some(mine.id.as_str()) && o == "ok"), "{rows:?}");
        assert!(rows.iter().any(|(t, _, _, o)| t == "site_delete" && o == "error"), "the refused deletes are recorded too: {rows:?}");
        assert!(rows.iter().all(|(_, _, a, _)| a.as_deref() != Some("s3cret-value")), "a value reached the feed");
    }
    // 9) The launch sweep ends a "this session" dial: back at Read.
    rexenv_lib::commands::mcp::end_session_grants_at_launch(&state);
    {
        let conn = state.db.lock().unwrap();
        let a = core::agent_access::current(&conn).unwrap();
        assert_eq!(a.level, core::agent_access::AccessLevel::Read, "a session-long dial survived the launch sweep");
    }
    println!("✓ feed complete (targets, verbs, refusals, no values); the launch sweep ended the session grant");

    println!("mcp_user_site_check: PASS");
}
