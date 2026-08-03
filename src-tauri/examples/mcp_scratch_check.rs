//! Live check: the M2a **scratch** tools over a REAL MCP socket — the tier
//! boundary, the target screen, the clone loop, and a real `wp` execution.
//!
//!   cargo run --example mcp_scratch_check
//!
//! The companion to `mcp_socket_check` (which covers M1's read-only surface).
//! This one drives the EXECUTING registry, so what it is really checking is the
//! thing no unit test can: that the refusals hold when the call arrives as bytes
//! on a socket, through the real dispatch, against real files on disk.
//!
//! # What this proves, and what it deliberately does not
//!
//! Proves, end to end: a scratch tool aimed at the USER'S site is refused with
//! the ownership policy statement (#208); `wp_run` refuses an argv that names
//! its own target (#218); `scratch_add_package` refuses a blast-radius source
//! and a header-less one, then CLONES — with the source proven byte-identical
//! afterwards (#216/#217); `scratch_sync_package` re-reads only the RECORDED
//! source; a real `wp` child runs and its output is scrubbed of rexenv's own
//! paths (#201/#218); and every call lands in the feed while the scratch site's
//! TTL moves (#206/#207).
//!
//! Does NOT prove, and says so rather than implying otherwise:
//!
//! - **The reaper's delete and its skip-don't-stop leg (#215).** Deleting for
//!   real needs a provisioned site with a database to drop and, for the skip
//!   case, a LIVE public tunnel. Both are `network`/`system` territory — a
//!   sandbox-tier example that faked them would be asserting against its own
//!   fixture, not against `delete_site_owned`. #215 stays ◐ for those legs.
//! - **A `wp` command that BOOTS WordPress** (activation, options). This site's
//!   docroot is a real directory but not a WordPress install, so the wp child is
//!   exercised on a core command. What that leaves unproven is wp-cli's
//!   behaviour with WordPress loaded — not any rexenv guard, all of which sit
//!   before the child is spawned.
//!
//! # Fixture ownership (the invariant — `examples/common/mod.rs`)
//!
//! Sandboxed `Platform` + sandbox database + a socket under the sandbox root, so
//! nothing touches the app's own socket, config or sites. The plugin SOURCE has
//! to live outside the sandbox root — `validate_linked_docroot` refuses anything
//! under app-data, which is exactly the guard being tested — so it gets its own
//! temp tree and its own `Drop` guard. Nothing is provisioned and no service is
//! spawned: the only child process is a short, capped `wp` invocation.

use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use rexenv_lib::{core, mcp_server};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use tauri::Manager;

mod common;

/// A temp tree this example created, removed however the run ends.
///
/// A `Drop` guard rather than a line at the end of `main`: everything between
/// here and there is an `assert!` that can unwind straight past it, and a
/// half-cleaned run of THIS example would leave exactly the kind of orphan the
/// reaper exists to collect.
struct TempTree(PathBuf);
impl Drop for TempTree {
    fn drop(&mut self) {
        // Only ever the directory this example itself created, under the OS temp
        // dir — never a derived path, never a parent (the 24 Jul lesson).
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

/// One `tools/call`, returning `(is_error, text)` — the tool's own reply text,
/// which for a refusal is the policy statement an agent reads.
fn call(
    stream: &mut UnixStream,
    reader: &mut impl BufRead,
    id: u32,
    name: &str,
    args: Value,
) -> (bool, String) {
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": name, "arguments": args }
    });
    send(stream, &req.to_string());
    let v = read_reply(reader);
    let text = v["result"]["content"][0]["text"].as_str().unwrap_or("").to_string();
    (v["result"]["isError"].as_bool().unwrap_or(false), text)
}

/// Every file in `dir`, relative path → bytes. The clone's promise is about
/// bytes, so it is checked in bytes.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else if let Ok(bytes) = std::fs::read(&p) {
                out.push((p.strip_prefix(root).unwrap().to_path_buf(), bytes));
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[tokio::main]
async fn main() {
    let (plat, _sandbox) = common::sandbox("mcp_scratch_check");
    let sandbox_root = plat.paths().app_data_dir().expect("sandbox data dir");
    let real_bin = plat.paths().bin_dir().expect("bin dir");
    let conn = rexenv_lib::state::db::open_for_platform(plat.paths()).expect("open sandbox db");
    let ca = core::ssl::load_or_create(plat.paths(), plat.permissions()).expect("sandbox CA");

    // Pin the sites folder INSIDE the sandbox: nothing here provisions, but a
    // real path would let a future edit of this example write to the user's own
    // Sites folder without anyone noticing.
    let sites_dir = sandbox_root.join("Sites");
    std::fs::create_dir_all(&sites_dir).expect("sandbox sites dir");
    rexenv_lib::state::store::set_setting(&conn, "sites_dir", &sites_dir.to_string_lossy())
        .expect("pin the sandbox sites dir");

    // Two fixture sites: one the USER'S, one the agent's. Same suffix on both —
    // the user's is deliberately named `*.scratch.rex` so the refusal below is
    // proven to rest on the recorded origin, not on the domain.
    let mk = |domain: &str, name: &str| {
        core::sites::create(
            &conn,
            NewSite {
                name: name.into(),
                domain: domain.into(),
                site_type: SiteType::Wordpress,
                php_version: "8.3".into(),
                web_server: WebServer::Nginx,
                path: String::new(),
                db_engine: SiteDbEngine::Mysql,
            },
        )
        .expect("create fixture site")
    };
    let theirs = mk("mine.scratch.rex", "mine");
    let ours = mk("probe.scratch.rex", "probe");

    // The agent's site gets a REAL docroot (the clone has to land somewhere) and
    // the recorded facts that make it the agent's.
    let docroot = sites_dir.join("probe.scratch.rex");
    std::fs::create_dir_all(docroot.join("wp-content/plugins")).expect("fixture docroot");
    conn.execute(
        "UPDATE sites SET path = ?1, origin = 'agent', agent_client = 'mcp_scratch_check', \
         expires_at = datetime('now', '+1 hours'), docroot_managed = 1 WHERE id = ?2",
        rusqlite::params![docroot.to_string_lossy(), ours.id],
    )
    .expect("record the agent's site");
    let expiry_before: String = conn
        .query_row("SELECT expires_at FROM sites WHERE id = ?1", [&ours.id], |r| r.get(0))
        .expect("read the fixture expiry");

    // The plugin SOURCE — outside the sandbox root on purpose: app-data is one
    // of the blast-radius refusals, so a source living there would be rejected
    // by the very guard this example is here to exercise.
    let src_root = std::env::temp_dir().join(format!("rexenv-scratchsrc-{}", std::process::id()));
    let _src_guard = TempTree(src_root.clone());
    let src = src_root.join("acme-blocks");
    std::fs::create_dir_all(src.join("inc")).expect("fixture source");
    std::fs::write(src.join("acme-blocks.php"), b"<?php\n/**\n * Plugin Name: Acme Blocks\n */\n")
        .expect("plugin header");
    std::fs::write(src.join("inc/lib.php"), b"<?php // lib\n").expect("source file");
    // A binary-ish blob, so "byte-identical" is checked on bytes and not on text.
    std::fs::write(src.join("inc/logo.bin"), [0u8, 159, 146, 150, 255, 0, 1]).expect("blob");
    let source_before = snapshot(&src);
    // A tree with no plugin or theme header — refused, and told why.
    let headerless = src_root.join("not-a-plugin");
    std::fs::create_dir_all(&headerless).expect("headerless dir");
    std::fs::write(headerless.join("readme.txt"), b"nothing here\n").expect("readme");

    let app = tauri::test::mock_app();
    app.manage(AppState::new(conn, plat, ca));
    let before_id: i64 = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        conn.query_row("SELECT COALESCE(MAX(id),0) FROM agent_actions", [], |r| r.get(0)).unwrap()
    };

    // A socket under the sandbox root — never the app's own.
    let sock = sandbox_root.join(mcp_server::SOCKET_FILE);
    let listener = mcp_server::bind_socket(&sock).expect("bind the sandbox MCP socket");
    let (_shutdown, rx) = tokio::sync::watch::channel(true);
    tokio::spawn(mcp_server::serve(listener, app.handle().clone(), rx));

    let mut stream = UnixStream::connect(&sock).expect("connect");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    send(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"mcp_scratch_check","version":"1"}}}"#,
    );
    assert_eq!(read_reply(&mut reader)["result"]["serverInfo"]["name"], "rexenv");
    send(&mut stream, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    // 1) tools/list advertises BOTH registries — a registered tool no agent can
    //    call is the same as no tool at all.
    send(&mut stream, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let v = read_reply(&mut reader);
    let names: Vec<&str> =
        v["result"]["tools"].as_array().expect("tools").iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "list_sites", "site_status", "tail_log",
        "scratch_create_site", "scratch_delete_site", "scratch_add_package",
        "scratch_sync_package", "wp_run",
    ] {
        assert!(names.contains(&expected), "`{expected}` is not advertised: {names:?}");
    }
    println!("✓ tools/list → {} tool(s), both registries advertised", names.len());

    // 2) THE TIER BOUNDARY. Every executing tool, aimed at the USER'S site.
    //    Note the fixture's domain is `mine.scratch.rex`: if any of these ever
    //    passed, it would be because something started reading the NAME.
    for (id, tool, args) in [
        (10, "wp_run", serde_json::json!({ "site_id": theirs.id, "args": ["cli", "version"] })),
        (11, "scratch_delete_site", serde_json::json!({ "site_id": theirs.id })),
        (12, "scratch_add_package", serde_json::json!({ "site_id": theirs.id, "source": src.to_string_lossy() })),
        (13, "scratch_sync_package", serde_json::json!({ "site_id": theirs.id, "slug": "acme-blocks" })),
    ] {
        let (is_err, text) = call(&mut stream, &mut reader, id, tool, args);
        assert!(is_err, "`{tool}` on the USER'S site must be refused: {text}");
        assert!(text.contains("your own sites"), "`{tool}`: the ownership statement: {text}");
        assert!(!text.contains("no site with id"), "`{tool}`: must not read as not-found: {text}");
        assert!(text.contains("mine.scratch.rex"), "`{tool}`: names the site: {text}");
        // A refusal is output too: it must carry no path and no database name.
        assert!(!text.contains(&*docroot.to_string_lossy()), "`{tool}` leaked a docroot: {text}");
        assert!(!text.contains(&theirs.db_name), "`{tool}` leaked a db name: {text}");
    }
    println!("✓ tier boundary — all 4 executing tools refuse the user's own site, by RECORD");

    // 3) The target screen, over the wire, in the forms an agent would send.
    for (id, argv, must_name) in [
        (20, serde_json::json!(["plugin", "list", "--path=/tmp/elsewhere"]), "--path"),
        (21, serde_json::json!(["--url=https://other.rex", "option", "get", "home"]), "--url"),
        (22, serde_json::json!(["@prod", "plugin", "list"]), "@prod"),
        (23, serde_json::json!(["--ssh", "user@host", "cli", "version"]), "--ssh"),
    ] {
        let (is_err, text) =
            call(&mut stream, &mut reader, id, "wp_run", serde_json::json!({ "site_id": ours.id, "args": argv }));
        assert!(is_err, "an argv naming its own target must be refused: {text}");
        assert!(text.contains(must_name), "the refusal names `{must_name}`: {text}");
        assert!(text.contains("site_id"), "and the ONE way to say which site: {text}");
    }
    println!("✓ wp_run refuses --path / --url / @alias / --ssh, naming the parameter and site_id");

    // 4) scratch_add_package refuses before it reads: blast radius, then header.
    let home = directories::BaseDirs::new().expect("home").home_dir().to_path_buf();
    let (is_err, text) = call(
        &mut stream, &mut reader, 30, "scratch_add_package",
        serde_json::json!({ "site_id": ours.id, "source": home.to_string_lossy() }),
    );
    assert!(is_err && text.contains("too broad"), "$HOME must be refused: {text}");
    let (is_err, text) = call(
        &mut stream, &mut reader, 31, "scratch_add_package",
        serde_json::json!({ "site_id": ours.id, "source": headerless.to_string_lossy() }),
    );
    assert!(is_err, "a header-less tree must be refused: {text}");
    assert!(text.contains("Plugin Name") && text.contains("style.css"), "says what it looked for: {text}");
    println!("✓ scratch_add_package refuses $HOME (blast radius) and a header-less source");

    // 5) The clone, for real — and the SOURCE proven untouched afterwards.
    let (is_err, text) = call(
        &mut stream, &mut reader, 40, "scratch_add_package",
        serde_json::json!({ "site_id": ours.id, "source": src.to_string_lossy() }),
    );
    assert!(!is_err, "adding a real plugin must succeed: {text}");
    let added: Value = serde_json::from_str(&text).expect("add_package returns JSON");
    assert_eq!(added["slug"], "acme-blocks", "{added}");
    assert_eq!(added["kind"], "plugin", "kind is DERIVED from the header: {added}");
    let clone = docroot.join("wp-content/plugins/acme-blocks");
    assert!(clone.join("acme-blocks.php").is_file(), "the clone landed in the docroot");
    assert!(clone.join("inc/logo.bin").is_file(), "…including nested files");
    assert_eq!(snapshot(&src), source_before, "the clone WROTE to the source");

    // The direction, live: a write inside the copy must leave the source alone.
    std::fs::write(clone.join("acme-blocks.php"), b"<?php // the agent clobbered this\n")
        .expect("write inside the clone");
    std::fs::write(clone.join("new-file.php"), b"<?php\n").expect("create inside the clone");
    assert_eq!(snapshot(&src), source_before, "a write inside the clone reached the source");
    assert!(!src.join("new-file.php").exists(), "a file created in the clone appeared in the source");
    println!("✓ clone landed (header-derived kind), and writes inside it leave the source byte-identical");

    // 6) Sync re-reads the RECORDED source only — it cannot be redirected, and
    //    a real edit is reported as a real change.
    let (is_err, text) =
        call(&mut stream, &mut reader, 50, "scratch_sync_package", serde_json::json!({ "site_id": ours.id, "slug": "acme-blocks" }));
    assert!(!is_err, "sync must succeed: {text}");
    let synced: Value = serde_json::from_str(&text).expect("sync returns JSON");
    assert_eq!(synced["sourceHadChanged"], false, "nothing changed in the SOURCE: {synced}");
    assert!(
        synced["detail"].as_str().unwrap().contains("No changes were detected"),
        "the hedge, never 'unchanged': {synced}"
    );
    assert_eq!(
        std::fs::read(clone.join("acme-blocks.php")).unwrap(),
        std::fs::read(src.join("acme-blocks.php")).unwrap(),
        "the re-clone did not restore the clobbered file"
    );
    std::fs::write(src.join("inc/lib.php"), b"<?php // lib, edited and rather longer now\n")
        .expect("edit the source");
    let (_, text) =
        call(&mut stream, &mut reader, 51, "scratch_sync_package", serde_json::json!({ "site_id": ours.id, "slug": "acme-blocks" }));
    let synced: Value = serde_json::from_str(&text).expect("sync returns JSON");
    assert_eq!(synced["sourceHadChanged"], true, "a real edit must be detected: {synced}");
    let (is_err, text) =
        call(&mut stream, &mut reader, 52, "scratch_sync_package", serde_json::json!({ "site_id": ours.id, "slug": "never-added" }));
    assert!(is_err && text.contains("scratch_add_package"), "an unknown slug says what to do: {text}");
    println!("✓ sync re-clones from the RECORDED source, detects a real edit, hedges when it can't tell");

    // 7) A REAL wp child, and the scrub on its REAL output. `cli info` prints
    //    the phar and PHP paths, which live in rexenv's bin dir — the leak this
    //    is here to catch, arriving from a process rexenv did not write.
    let (is_err, text) = call(
        &mut stream, &mut reader, 60, "wp_run",
        serde_json::json!({ "site_id": ours.id, "args": ["cli", "info"] }),
    );
    assert!(!is_err, "wp_run must not fail at the TOOL level: {text}");
    let run: Value = serde_json::from_str(&text).expect("wp_run returns JSON");
    let streams = format!("{}{}", run["stdout"].as_str().unwrap_or(""), run["stderr"].as_str().unwrap_or(""));
    assert!(streams.contains("WP-CLI"), "a real wp child ran: {run}");
    assert!(run["exitCode"].is_number(), "the exit code travels: {run}");
    assert!(run["succeeded"].is_boolean(), "and whether it worked, named: {run}");
    // The scrub, on output rexenv did not author. Both directions asserted,
    // because the absence check alone would pass on a wp that printed no paths
    // at all: `cli info` DOES print them (the PHP binary under rexenv's bin dir,
    // the packages dir under $HOME), so the labels must be there in their place.
    // Proven by planting: with the scrub removed, this run fails on the raw
    // `/Users/<name>/Library/Application Support/…/bin/php-8.3.31/php`.
    let out = run["stdout"].as_str().unwrap_or("");
    assert!(out.contains("<rexenv-bin>"), "rexenv's bin dir was not labelled — did wp stop printing it? {run}");
    assert!(out.contains("<home>"), "a path under $HOME was not labelled: {run}");
    assert!(!text.contains(&*real_bin.to_string_lossy()), "rexenv's bin dir reached the agent: {text}");
    assert!(!text.contains(&*home.to_string_lossy()), "the OS home (and username) reached the agent: {text}");
    assert!(
        run["note"].as_str().unwrap_or("").contains("not sanitised"),
        "the reply states its own limit: {run}"
    );
    println!("✓ wp_run ran a real wp child; rexenv's own paths and the home dir are scrubbed from its output");

    // 8) Every call recorded, and using the site kept it alive (#206/#207).
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let mine: Vec<_> = mcp_server::feed::recent(&conn, 200)
            .unwrap()
            .into_iter()
            .filter(|a| a.id > before_id)
            .collect();
        for tool in ["wp_run", "scratch_add_package", "scratch_sync_package", "scratch_delete_site"] {
            assert!(mine.iter().any(|a| a.tool == tool), "`{tool}` is not in the feed: {mine:?}");
        }
        assert!(mine.iter().all(|a| a.client == "mcp_scratch_check"), "attributed: {mine:?}");
        assert!(
            mine.iter().any(|a| a.target_site.as_deref() == Some(ours.id.as_str())),
            "the site rexenv acted on is named: {mine:?}"
        );
        let expiry_after: String = conn
            .query_row("SELECT expires_at FROM sites WHERE id = ?1", [&ours.id], |r| r.get(0))
            .expect("read the expiry");
        assert!(expiry_after > expiry_before, "using a scratch site must push its expiry out");
        // The USER'S site was named by four refused calls and must have gained
        // no lifecycle state from any of them.
        let theirs_expiry: Option<String> = conn
            .query_row("SELECT expires_at FROM sites WHERE id = ?1", [&theirs.id], |r| r.get(0))
            .expect("read the user's expiry");
        assert!(theirs_expiry.is_none(), "a refused call gave the user's site an expiry: {theirs_expiry:?}");
    }
    println!("✓ every call recorded and attributed; the scratch TTL moved, the user's site gained none");

    println!(
        "✓ mcp_scratch_check green — the executing registry over a real socket: the user's own \
         site refused by RECORD by all four tools, target-naming argv refused, a real clone that \
         leaves the source byte-identical, a real wp child whose output is scrubbed of rexenv's \
         own paths. NOT covered here (and ◐ in the ledger): the reaper's real delete and its \
         skip-don't-stop leg, which need a provisioned site and a live tunnel."
    );
}
