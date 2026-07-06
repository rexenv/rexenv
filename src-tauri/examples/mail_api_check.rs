//! Phase-3 §2.3 check: the Mailpit HTTP-API client (list / detail / raw / search /
//! clear) parses real Mailpit responses. Starts Mailpit, injects two messages via
//! the sendmail shim, then exercises every `core::mail` API function.
//!
//! Run (ports 11025/18025 free): `cargo run --example mail_api_check`

use rexenv_lib::core::{binaries, mail};
use rexenv_lib::platform;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let bin = binaries::resolve(&*plat, "mailpit", binaries::MAILPIT_VERSION)
        .await
        .expect("resolve mailpit");
    let mut server = mail::start(&*plat, &bin).expect("start mailpit");
    for _ in 0..40 {
        if mail::running() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(mail::running(), "mailpit never came up");

    // Clean slate.
    mail::delete_all().await.expect("clear");
    assert_eq!(mail::list(None).await.unwrap().total, 0, "inbox not empty after clear");

    // Inject two messages through Mailpit's own sendmail shim.
    send(&bin, "alice@rexenv.test", "site@acme.test", "Welcome aboard", "<p>Hello <b>Alice</b></p>");
    send(&bin, "bob@rexenv.test", "noreply@portfolio.test", "Order shipped", "<p>On its way</p>");
    std::thread::sleep(Duration::from_millis(600));

    // list
    let list = mail::list(None).await.expect("list");
    println!("✓ list: total={} unread={}", list.total, list.unread);
    assert_eq!(list.total, 2, "expected 2 messages");
    assert!(list.messages.iter().any(|m| m.subject == "Welcome aboard"));

    // search
    let found = mail::list(Some("shipped")).await.expect("search");
    println!("✓ search 'shipped' → {} hit(s)", found.messages.len());
    assert!(
        found.messages.iter().all(|m| m.subject.contains("shipped") || m.subject.contains("Order")),
        "search returned unrelated results"
    );
    assert!(found.messages.iter().any(|m| m.subject == "Order shipped"));

    // detail (+ headers)
    let id = list.messages[0].id.clone();
    let d = mail::detail(&id).await.expect("detail");
    println!(
        "✓ detail: subject={:?} from={} htmlLen={} textLen={} headers={}",
        d.subject,
        d.from.address,
        d.html.len(),
        d.text.len(),
        d.headers.len()
    );
    assert!(!d.from.address.is_empty(), "from missing");
    assert!(!d.html.is_empty(), "html body missing");
    assert!(d.headers.iter().any(|h| h.name.eq_ignore_ascii_case("subject")), "subject header missing");

    // raw
    let raw = mail::raw(&id).await.expect("raw");
    println!("✓ raw: {} bytes", raw.len());
    assert!(raw.to_lowercase().contains("subject:"), "raw missing headers");

    // clear
    mail::delete_all().await.expect("clear");
    assert_eq!(mail::list(None).await.unwrap().total, 0, "inbox not empty after final clear");
    println!("✓ clear: inbox emptied");

    let _ = mail::stop(&*plat, server.id());
    let _ = server.wait();
    println!("\nALL GOOD — Mailpit API client lists, searches, previews, and clears.");
}

/// Pipe an HTML message to Mailpit's `sendmail` shim over the local SMTP.
fn send(bin: &std::path::Path, to: &str, from: &str, subject: &str, html: &str) {
    let msg = format!(
        "To: {to}\r\nFrom: {from}\r\nSubject: {subject}\r\nContent-Type: text/html\r\n\r\n{html}\r\n"
    );
    let mut child = Command::new(bin)
        .args(["sendmail", "-t", "-S", &format!("127.0.0.1:{}", mail::MAILPIT_SMTP_PORT)])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn sendmail");
    child.stdin.take().unwrap().write_all(msg.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success(), "sendmail failed");
}
