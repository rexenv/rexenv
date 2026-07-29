//! Live check: the MCP server over the REAL socket path — handshake + the
//! `list_sites` read-only tool end to end.
//!
//!   cargo run --example mcp_socket_check
//!
//! Boots real app state (the way `cli_socket_check` does), binds the app's MCP
//! socket (guarding a live app), serves the real `mcp_server`, then drives a
//! SPEC-LITERAL MCP session over it — initialize, initialized, tools/list, one
//! tools/call to `list_sites`, ping. The bytes sent are written to match the MCP
//! spec's own message shapes, NOT round-tripped through our own types, so this
//! catches the our-encoder-agrees-with-our-decoder trap the plan (§8) warns
//! about. It does NOT prove the real Claude Code client — that is the manual
//! `claude mcp add rexenv -- rex mcp` step.
//!
//! Read-only by construction: `list_sites` reaches state only through `ReadCtx`,
//! reads the real database, and writes nothing.

use rexenv_lib::state::app::AppState;
use rexenv_lib::{cli_server, mcp_server};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use tauri::Manager;

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

#[tokio::main]
async fn main() {
    let platform = rexenv_lib::platform::current();
    // Default to the REAL socket path (guarding a live app); `REXENV_MCP_SOCKET`
    // overrides it for a temp path so this can run while the app holds the real
    // one — the CLI's `REXENV_CLI_SOCKET` test affordance, mirrored.
    let sock = std::env::var("REXENV_MCP_SOCKET").map(std::path::PathBuf::from).unwrap_or_else(
        |_| platform.paths().config_dir().expect("config dir").join(mcp_server::SOCKET_FILE),
    );

    // Refuse to steal a LIVE app's MCP socket — connect tells the truth; a stale
    // file from a crash refuses and is safe to replace (the cli_socket_check guard).
    if UnixStream::connect(&sock).is_ok() {
        panic!("the app's MCP socket is live at {} — quit the app first", sock.display());
    }

    // Boot real app state so `list_sites` reads the real database (read-only).
    let conn = rexenv_lib::state::db::open_for_platform(platform.paths()).expect("open app db");
    let ca = rexenv_lib::core::ssl::load_or_create(platform.paths(), platform.permissions())
        .expect("load CA");
    let app = tauri::test::mock_app();
    app.manage(AppState::new(conn, platform, ca));

    // The session records agent actions to the real DB; note the current max
    // feed id so we verify only the rows THIS run adds and remove only those.
    let before_id: i64 = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        conn.query_row("SELECT COALESCE(MAX(id),0) FROM agent_actions", [], |r| r.get(0)).unwrap()
    };

    // Bind with the SAME convention as the CLI socket (one convention, not two),
    // then serve the real M1 server against the app's state.
    let listener = cli_server::bind(&sock).expect("bind MCP socket");
    let mode = std::fs::metadata(&sock).expect("socket metadata").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "socket must be 0600");
    // A never-signalled shutdown (the toggle's live-disable path isn't under test
    // here) — keep the sender alive so `serve`'s accept loop stays up for the run.
    let (_mcp_shutdown, mcp_rx) = tokio::sync::watch::channel(true);
    tokio::spawn(mcp_server::serve(listener, app.handle().clone(), mcp_rx));

    let mut stream = UnixStream::connect(&sock).expect("connect to MCP socket");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream for reads"));

    // 1) initialize — the shape a real MCP client sends first.
    send(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"mcp_socket_check","version":"1"}}}"#,
    );
    let v = read_reply(&mut reader);
    assert_eq!(v["jsonrpc"], "2.0", "reply: {v}");
    assert_eq!(v["id"], 1, "reply: {v}");
    assert_eq!(v["result"]["serverInfo"]["name"], "rexenv", "reply: {v}");
    assert!(v["result"]["capabilities"]["tools"].is_object(), "must advertise tools: {v}");
    assert_eq!(v["result"]["protocolVersion"], "2025-11-25", "reply: {v}");
    println!("✓ initialize → rexenv {}", v["result"]["serverInfo"]["version"]);

    // 2) initialized notification — no reply must come back.
    send(&mut stream, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    // 3) tools/list — advertises list_sites with a schema.
    send(&mut stream, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let v = read_reply(&mut reader);
    let tools = v["result"]["tools"].as_array().expect("tools array");
    assert!(tools.iter().any(|t| t["name"] == "list_sites"), "list_sites advertised: {v}");
    println!("✓ tools/list → {} tool(s), incl. list_sites", tools.len());

    // 4) tools/call list_sites — the read-only tool, end to end.
    send(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_sites","arguments":{}}}"#,
    );
    let v = read_reply(&mut reader);
    assert!(v["result"]["isError"].is_null(), "list_sites must not error: {v}");
    let text = v["result"]["content"][0]["text"].as_str().expect("text content");
    let sites: Value = serde_json::from_str(text).expect("list_sites content is a JSON array");
    let arr = sites.as_array().expect("an array of sites");
    // The drop is structural, but spot-check live output too: never a docroot
    // path or db name, always a domain to reference the site by.
    for s in arr {
        assert!(s.get("path").is_none() && s.get("docroot").is_none(), "path leaked: {s}");
        assert!(s.get("dbName").is_none(), "db name leaked: {s}");
        assert!(s.get("domain").is_some(), "no domain to reference: {s}");
    }
    println!("✓ tools/call list_sites → {} site(s), no path/db-name in output", arr.len());

    // 5) tools/call site_status on the first site — the diagnostic, end to end.
    if let Some(first) = arr.first().and_then(|s| s["id"].as_str()) {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"site_status","arguments":{{"site_id":"{first}"}}}}}}"#
        );
        send(&mut stream, &req);
        let v = read_reply(&mut reader);
        assert!(v["result"]["isError"].is_null(), "site_status must not error: {v}");
        let text = v["result"]["content"][0]["text"].as_str().expect("text content");
        let status: Value = serde_json::from_str(text).expect("site_status is a JSON object");
        // Assert the SHAPE, not a specific verdict — the verdict reflects this
        // example's own (empty) manager, so it's honestly BackendDown here.
        // A specific verdict, who-resolves-it, and scope — never an internal path.
        assert!(status["verdict"].is_string(), "verdict present: {status}");
        assert!(status["resolution"].is_string(), "resolution present: {status}");
        assert!(status["detail"].as_str().unwrap().len() > 10, "detail present: {status}");
        assert!(status.get("path").is_none() && status.get("docroot").is_none(), "path leaked: {status}");
        println!("✓ tools/call site_status → verdict `{}`, resolution `{}`", status["verdict"], status["resolution"]);
    }

    // 6) tools/call tail_log on the first WordPress site — the log surface.
    if let Some(wp_id) = arr.iter().find(|s| s["type"] == "wordpress").and_then(|s| s["id"].as_str()) {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"tail_log","arguments":{{"site_id":"{wp_id}","lines":20}}}}}}"#
        );
        send(&mut stream, &req);
        let v = read_reply(&mut reader);
        assert!(v["result"]["isError"].is_null(), "tail_log must not error on a WP site: {v}");
        let text = v["result"]["content"][0]["text"].as_str().expect("text content");
        let tail: Value = serde_json::from_str(text).expect("tail_log is a JSON object");
        assert_eq!(tail["source"], "wp-debug", "tail: {tail}");
        assert!(tail["lines"].is_array(), "lines array: {tail}");
        let note = tail["note"].as_str().unwrap().to_ascii_lowercase();
        assert!(note.contains("not") && !note.contains("sanitis"), "honest note: {tail}");
        assert!(tail.get("path").is_none(), "path leaked: {tail}");
        println!("✓ tools/call tail_log → {} line(s), source `wp-debug`, honest note", tail["lines"].as_array().unwrap().len());
    }

    // 7) the non-happy-paths — an unknown tool and a malformed message. Both
    // must be answered AND recorded (the feed check below proves the recording).
    send(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"does_not_exist","arguments":{}}}"#,
    );
    assert_eq!(read_reply(&mut reader)["error"]["code"], -32602, "unknown tool errors");
    send(&mut stream, "this is not valid json");
    assert_eq!(read_reply(&mut reader)["error"]["code"], -32700, "malformed → parse error");
    println!("✓ unknown tool + malformed message answered");

    // 8) ping — an empty result, and NOT a feed entry (handshake, not an action).
    send(&mut stream, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#);
    let v = read_reply(&mut reader);
    assert_eq!(v["result"], json!({}), "reply: {v}");
    println!("✓ ping → {{}}");

    // The activity feed: read the rows THIS run created and verify the session
    // recorded every call — the happy path AND the non-happy-paths — attributed
    // to the client, with ping/initialize NOT logged. Then remove only those
    // rows (scoped by id — never the user's own feed).
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let mine: Vec<_> = mcp_server::feed::recent(&conn, 100)
            .unwrap()
            .into_iter()
            .filter(|a| a.id > before_id)
            .collect();
        let by_tool = |t: &str| mine.iter().find(|a| a.tool == t);
        use rexenv_lib::mcp_server::feed::Outcome;
        assert_eq!(by_tool("list_sites").map(|a| a.outcome), Some(Outcome::Ok), "{mine:?}");
        assert_eq!(by_tool("does_not_exist").map(|a| a.outcome), Some(Outcome::UnknownTool));
        assert_eq!(by_tool("(unparseable)").map(|a| a.outcome), Some(Outcome::BadRequest));
        assert!(mine.iter().all(|a| a.client == "mcp_socket_check"), "client attributed: {mine:?}");
        assert!(by_tool("ping").is_none() && by_tool("initialize").is_none(), "handshake not logged");
        assert!(mine.iter().any(|a| a.concerning), "the unknown-tool/bad-request rows are concerning");
        println!("✓ activity feed → {} row(s) recorded (incl. unknown-tool + bad-request), ping not logged", mine.len());
        conn.execute("DELETE FROM agent_actions WHERE id > ?1", [before_id]).unwrap();
    }

    let _ = std::fs::remove_file(&sock);
    println!(
        "✓ mcp_socket_check green — handshake + list_sites/site_status/tail_log + activity feed OK. \
         Real-client check: `claude mcp add rexenv -- rex mcp` (manual, §8)."
    );
}
