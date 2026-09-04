//! Live check: the agent mail surface's FILTER, over real messages in a real
//! Mailpit (ledger #227) — the leg `mcp_scratch_check` could not reach.
//!
//!   cargo run --example mcp_mail_check
//!
//! `mcp_scratch_check` proves the three states that decide whether the filter is
//! consulted at all (off / no docroot / stamp missing), and those are told apart
//! by a stat, so they need no mail catcher. What it cannot prove is the
//! predicate itself: that a real message carrying this scratch site's stamp
//! matches, and a real message that merely LOOKS like it does not.
//!
//! # The leg that matters is the near-miss, and it must fail loudly
//!
//! `admin@probe.scratch.rex` has the site's exact domain and a different local
//! part. `ends_with(domain)` — the plausible wrong predicate, and the one the L0
//! test plants — accepts it. So does anything reaching for the domain instead of
//! the address.
//!
//! The trap is that a broken filter and a correct one can BOTH return "no mail":
//! an empty result is also what a correctly filtered empty inbox looks like. So
//! this never infers from absence. Before asking the tool anything, it asserts
//! against Mailpit's own API that BOTH messages are really in the store — and if
//! they are not, it reports the fixture as broken and stops rather than reading
//! an empty list as a passing filter.
//!
//! # Why this brings its own Mailpit, and why that makes it `service` tier
//!
//! Borrowing the user's running Mailpit would mean planting messages in their
//! real store to prove rexenv can tell their mail from a scratch site's — a test
//! that contradicts the thing it is testing. So it starts its own under
//! `common::sandbox`, which puts the message database inside the sandbox. The
//! LISTEN PORTS are fixed constants, though (`mail::MAILPIT_SMTP_PORT` /
//! `MAILPIT_HTTP_PORT`), so this cannot run beside the user's stack: that is
//! exactly what the `service` tier means, and it is why this is its own example
//! rather than three more legs inside sandbox-tier `mcp_scratch_check`.
//!
//! # What a green run proves, and what it does not
//!
//! Proves: over the real socket, through the real dispatch, against messages a
//! real SMTP conversation put in a real Mailpit — the stamped message is
//! returned, the near-miss is not, and `mail_get` REFUSES the near-miss's id
//! without saying who it is really from.
//!
//! Does not prove: anything about Mailpit's own pagination or its search
//! endpoint (the list is read unfiltered and capped by us); anything about a
//! site that overrode its `From` header, which is the residual the tool's own
//! note states to every agent and which no test can remove.

mod common;

use rexenv_lib::core::{self, binaries, mail};
use rexenv_lib::mcp_server;
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tauri::Manager;

const DOMAIN: &str = "probe.scratch.rex";
/// Same domain, different local part — accepted by `ends_with(domain)`, which is
/// why it is the leg this example exists for.
const NEAR_MISS: &str = "admin@probe.scratch.rex";
const STAMPED_SUBJECT: &str = "rexenv-mailcheck-stamped";
const NEAR_MISS_SUBJECT: &str = "rexenv-mailcheck-nearmiss";

fn send_line(stream: &mut UnixStream, msg: &str) {
    stream.write_all(msg.as_bytes()).expect("write message");
    stream.write_all(b"\n").expect("write newline");
    stream.flush().expect("flush");
}

fn read_reply(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read reply line");
    serde_json::from_str(line.trim()).expect("reply is valid JSON-RPC")
}

fn call(
    stream: &mut UnixStream,
    reader: &mut impl BufRead,
    id: u32,
    name: &str,
    args: Value,
) -> (bool, String) {
    let req = json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": name, "arguments": args }
    });
    send_line(stream, &req.to_string());
    let v = read_reply(reader);
    let text = v["result"]["content"][0]["text"].as_str().unwrap_or("").to_string();
    (v["result"]["isError"].as_bool().unwrap_or(false), text)
}

/// Mailpit's pid, reachable from both cleanup paths.
///
/// A Drop guard alone is NOT enough here: `fail` ends the process with
/// `exit(1)`, and `exit` runs no destructors — so a failing leg would leave
/// Mailpit holding :11025 and :18025 and the NEXT run would fail to bind and
/// look like a different bug. Both paths go through `reap_mailpit`, which is
/// idempotent by swapping the pid out.
static MAILPIT_PID: AtomicU32 = AtomicU32::new(0);
/// The child itself, so liveness can be asked ("did it fail to bind and exit?")
/// rather than inferred from a port that somebody else may be answering.
static MAILPIT_CHILD: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

fn child_status() -> std::io::Result<Option<std::process::ExitStatus>> {
    let mut held = MAILPIT_CHILD.lock().expect("mailpit child lock");
    match held.as_mut() {
        Some(c) => c.try_wait(),
        None => Ok(None),
    }
}

struct MailpitGuard;

impl Drop for MailpitGuard {
    fn drop(&mut self) {
        reap_mailpit();
    }
}

fn reap_mailpit() {
    let pid = MAILPIT_PID.swap(0, Ordering::SeqCst);
    if pid != 0 {
        let plat = rexenv_lib::platform::current();
        let _ = mail::stop(&*plat, pid);
    }
    if let Some(mut c) = MAILPIT_CHILD.lock().expect("mailpit child lock").take() {
        let _ = c.wait(); // reap — no zombie
    }
}

fn fail(step: &str, why: &str) -> ! {
    reap_mailpit();
    eprintln!("\n✗ {step}\n  {why}\n");
    std::process::exit(1);
}

/// One SMTP conversation, hand-rolled: this needs to set an exact `From` header,
/// which is the whole subject of the check, and a client library would put its
/// own opinion between the fixture and the assertion.
fn smtp_send(from: &str, subject: &str) {
    let addr = format!("127.0.0.1:{}", mail::MAILPIT_SMTP_PORT);
    let mut sock = TcpStream::connect(&addr).unwrap_or_else(|e| {
        fail("FIXTURE BROKEN — no SMTP listener", &format!("connect {addr}: {e}"))
    });
    sock.set_read_timeout(Some(Duration::from_secs(10))).expect("read timeout");
    let mut reader = BufReader::new(sock.try_clone().expect("clone smtp stream"));
    let mut expect = |sock: &mut TcpStream, line: Option<&str>, want: char| {
        if let Some(l) = line {
            sock.write_all(l.as_bytes()).expect("smtp write");
            sock.write_all(b"\r\n").expect("smtp crlf");
        }
        let mut resp = String::new();
        reader.read_line(&mut resp).expect("smtp read");
        if !resp.starts_with(want) {
            fail(
                "FIXTURE BROKEN — SMTP refused",
                &format!("after {:?} Mailpit said {:?}", line.unwrap_or("<greeting>"), resp.trim()),
            );
        }
    };
    expect(&mut sock, None, '2'); // greeting
    expect(&mut sock, Some("HELO rexenv-check"), '2');
    expect(&mut sock, Some(&format!("MAIL FROM:<{from}>")), '2');
    expect(&mut sock, Some("RCPT TO:<catch@rexenv.test>"), '2');
    expect(&mut sock, Some("DATA"), '3');
    let body = format!(
        "From: {from}\r\nTo: catch@rexenv.test\r\nSubject: {subject}\r\n\r\nbody\r\n.",
    );
    expect(&mut sock, Some(&body), '2');
    expect(&mut sock, Some("QUIT"), '2');
}

#[tokio::main]
async fn main() {
    let (plat, _sandbox) = common::sandbox("mcp_mail_check");
    let sandbox_root = plat.paths().app_data_dir().expect("sandbox data dir");
    let conn = rexenv_lib::state::db::open_for_platform(plat.paths()).expect("open sandbox db");
    let ca = core::ssl::load_or_create(plat.paths(), plat.permissions()).expect("sandbox CA");

    let sites_dir = sandbox_root.join("Sites");
    std::fs::create_dir_all(&sites_dir).expect("sandbox sites dir");
    rexenv_lib::state::store::set_setting(&conn, "sites_dir", &sites_dir.to_string_lossy())
        .expect("pin the sandbox sites dir");

    let site = core::sites::create(
        &conn,
        NewSite {
            name: "probe".into(),
            domain: DOMAIN.into(),
            site_type: SiteType::Wordpress,
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
    .expect("create the fixture site");
    let docroot = sites_dir.join(DOMAIN);
    std::fs::create_dir_all(docroot.join("wp-content")).expect("fixture docroot");
    conn.execute(
        "UPDATE sites SET path = ?1, origin = 'agent', agent_client = 'mcp_mail_check', \
         expires_at = datetime('now', '+1 hours'), docroot_managed = 1 WHERE id = ?2",
        rusqlite::params![docroot.to_string_lossy(), site.id],
    )
    .expect("record the agent's site");

    // Ports are the gap `common::sandbox` does not close (see its module doc).
    // THE hazard of this example, and it fails in the direction that looks like
    // success: if the user's stack is up, our `mail::start` cannot bind and
    // exits, `mail::running()` then sees THEIR Mailpit on the fixed port, and
    // every leg below proceeds — planting two messages in a real store and
    // proving the filter against the user's own mail.
    common::require_ports_free(&[
        (mail::MAILPIT_HTTP_PORT, "Mailpit's HTTP API — the legs below would read a real inbox"),
        (mail::MAILPIT_SMTP_PORT, "Mailpit's SMTP port — the legs below would plant test mail in it"),
    ]);

    // ── Mailpit, ours, reaped however this ends ─────────────────────────────
    let bin = binaries::resolve(&*plat, "mailpit", binaries::MAILPIT_VERSION)
        .await
        .expect("resolve mailpit");
    // Not `common::Reaped`: that guard sweeps its port, and its contract is a
    // FIXTURE port precisely so it can never be aimed at one the running stack
    // owns — which Mailpit's fixed :18025 is. Mailpit is also a single Go
    // process, so the forking-service worker leak the sweep exists for cannot
    // happen here. Terminating the exact child we spawned is the whole job.
    let child = mail::start(&*plat, &bin).expect("start mailpit");
    MAILPIT_PID.store(child.id(), Ordering::SeqCst);
    *MAILPIT_CHILD.lock().expect("mailpit child lock") = Some(child);
    let _reaped = MailpitGuard;
    let mut up = false;
    for _ in 0..40 {
        if mail::running() {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if !up {
        fail(
            "FIXTURE BROKEN — Mailpit never came up",
            &format!(
                "nothing is answering on :{}. If the user's stack is running it already holds \
                 these ports — this is a `service`-tier check, so stop the stack first.",
                mail::MAILPIT_HTTP_PORT
            ),
        );
    }
    // …and it is OURS: a child that failed to bind has already exited, and the
    // port check above would not catch a Mailpit that started between the two.
    if let Ok(Some(status)) = child_status() {
        fail(
            "FIXTURE BROKEN — our Mailpit exited",
            &format!(
                "it stopped with {status:?} while something else answers on :{}. Whatever is                  serving that port is not ours, and nothing below may write to it.",
                mail::MAILPIT_HTTP_PORT
            ),
        );
    }
    // The decisive one: our store is brand new, so it is empty. A non-empty
    // inbox here means we are talking to a message store somebody else owns.
    let existing = mail::list(None).await.expect("read mailpit").messages.len();
    if existing != 0 {
        fail(
            "REFUSING TO CONTINUE — the inbox is not ours",
            &format!(
                "{existing} message(s) are already here, but this run's Mailpit database was                  created seconds ago inside the sandbox. Something else is serving :{} and the                  legs below would send test mail into it.",
                mail::MAILPIT_HTTP_PORT
            ),
        );
    }
    println!("✓ our own Mailpit is up on :{} with an empty store", mail::MAILPIT_HTTP_PORT);

    // ── The two messages ────────────────────────────────────────────────────
    let stamp = core::wp_mailtag::stamp_for(DOMAIN);
    smtp_send(&stamp, STAMPED_SUBJECT);
    smtp_send(NEAR_MISS, NEAR_MISS_SUBJECT);

    // BOTH must really be in the store before anything is asked of the tool.
    // Skipping this is how "the filter works" and "the fixture never sent
    // anything" become the same green result: an empty list is also what a
    // correctly filtered empty inbox looks like.
    let mut raw = None;
    for _ in 0..40 {
        let inbox = mail::list(None).await.expect("read mailpit");
        if inbox.messages.len() >= 2 {
            raw = Some(inbox);
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let Some(raw) = raw else {
        fail(
            "FIXTURE BROKEN — the two messages are not in Mailpit",
            "the filter legs below would then be reading an empty inbox, and an empty result is \
             indistinguishable from a filter that works. Nothing here proves anything until both \
             messages exist.",
        );
    };
    let has = |addr: &str| raw.messages.iter().any(|m| m.from.address.eq_ignore_ascii_case(addr));
    if !has(&stamp) || !has(NEAR_MISS) {
        fail(
            "FIXTURE BROKEN — Mailpit does not hold both senders",
            &format!(
                "stamped={} near-miss={}. The near-miss is the leg this example exists for; \
                 without it in the store, a filter that returns one message proves nothing.",
                has(&stamp),
                has(NEAR_MISS)
            ),
        );
    }
    let near_miss_id = raw
        .messages
        .iter()
        .find(|m| m.from.address.eq_ignore_ascii_case(NEAR_MISS))
        .map(|m| m.id.clone())
        .expect("the near-miss id");
    println!("✓ both messages are really in the store (stamped + {NEAR_MISS})");

    // ── The socket, the real dispatch ───────────────────────────────────────
    let app = tauri::test::mock_app();
    app.manage(AppState::new(conn, plat, ca));
    {
        // D16: the stamp rides the endpoint — the same sync the enable path runs.
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        rexenv_lib::commands::mcp::sync_scratch_mail_stamps(&conn, true);
    }

    let sock = sandbox_root.join(mcp_server::SOCKET_FILE);
    let listener = mcp_server::bind_socket(&sock).expect("bind the sandbox MCP socket");
    let (_shutdown, rx) = tokio::sync::watch::channel(true);
    tokio::spawn(mcp_server::serve(listener, app.handle().clone(), rx));

    let mut stream = UnixStream::connect(&sock).expect("connect");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    send_line(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"mcp_mail_check","version":"1"}}}"#,
    );
    let _ = read_reply(&mut reader);
    send_line(&mut stream, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    // ── A · the list returns the stamped message and NOT the near-miss ──────
    let (err, text) = call(&mut stream, &mut reader, 2, "mail_list", json!({ "site_id": site.id }));
    if err {
        fail("A — mail_list refused", &text);
    }
    let listed: Value = serde_json::from_str(&text).expect("mail_list returns JSON");
    let messages = listed["messages"].as_array().cloned().unwrap_or_default();
    let listed_from: Vec<String> = messages
        .iter()
        .filter_map(|m| m["from"].as_str().map(str::to_string))
        .collect();
    if !listed_from.iter().any(|f| f.eq_ignore_ascii_case(&stamp)) {
        fail(
            "A — the site's OWN message was filtered out",
            &format!(
                "the stamped send is in Mailpit (asserted above) but did not come back. An agent \
                 would report the feature under test as broken.\n  returned: {listed_from:?}"
            ),
        );
    }
    if listed_from.iter().any(|f| f.eq_ignore_ascii_case(NEAR_MISS)) {
        fail(
            "A — the near-miss was returned as this site's mail",
            &format!(
                "`{NEAR_MISS}` shares the domain and nothing else. A predicate reaching for the \
                 DOMAIN rather than the address accepts it, and `mail_get` then becomes a read of \
                 any message in the user's inbox behind an id an agent can enumerate (#227).\n  \
                 returned: {listed_from:?}"
            ),
        );
    }
    if listed_from.len() != 1 {
        fail(
            "A — the list is not exactly this site's mail",
            &format!("expected one message, got {listed_from:?}"),
        );
    }
    println!("✓ A — the list returns the stamped message and refuses the near-miss");

    // ── B · the GATE re-proves it, on an id the agent supplies ──────────────
    //
    // The filter and the gate share one predicate precisely so this cannot
    // disagree with A. Asked anyway, over the socket: an agent does not have to
    // get its id from a filtered list.
    let (err, text) = call(
        &mut stream,
        &mut reader,
        3,
        "mail_get",
        json!({ "site_id": site.id, "message_id": near_miss_id }),
    );
    if !err {
        fail(
            "B — mail_get returned a message that is not this site's",
            &format!(
                "the id came straight from Mailpit, not from a filtered list — which is exactly \
                 how an agent would reach the user's own mail.\n  returned: {text}"
            ),
        );
    }
    // …and the refusal does not answer the question it is refusing.
    if text.contains(NEAR_MISS) || text.to_lowercase().contains("admin@") {
        fail(
            "B — the refusal named who the message is really from",
            &format!("that hands over the fact being withheld.\n  said: {text}"),
        );
    }
    println!("✓ B — mail_get refuses the near-miss without naming its sender");

    // ── C · the site's own message reads back in full ───────────────────────
    let mine = messages[0]["id"].as_str().expect("a message id");
    let (err, text) =
        call(&mut stream, &mut reader, 4, "mail_get", json!({ "site_id": site.id, "message_id": mine }));
    if err {
        fail("C — mail_get refused this site's OWN message", &text);
    }
    if !text.contains(STAMPED_SUBJECT) {
        fail(
            "C — the fetched message is not the one that was sent",
            &format!("expected the subject {STAMPED_SUBJECT:?}; got: {text}"),
        );
    }
    println!("✓ C — the site's own message reads back in full");

    println!(
        "\n✓ mcp_mail_check: over real messages, the stamp matches, the same domain with a \
         different local part does not, and the gate holds on an id the agent chose"
    );
}
