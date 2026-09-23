//! Phase-3 §2.3 check: the Mailpit HTTP-API client (list / detail / raw / search /
//! unread filter / mark-all-read / clear) parses real Mailpit responses. Starts
//! Mailpit, injects two messages via the sendmail shim, then exercises every
//! `core::mail` API function.
//!
//! The unread legs run AFTER `detail`, deliberately: previewing a message is
//! what marks it read in Mailpit, so at that point exactly one of the two is
//! unread. Asserted on a fresh inbox (all unread) or a swept one (none), a
//! filter that returns everything would pass without filtering anything.
//!
//! Run (ports 11025/18025 free): `cargo run --example mail_api_check`

#[path = "common/mod.rs"]
mod common;

use rexenv_lib::core::{binaries, mail};
use rexenv_lib::platform;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();
    let bin = binaries::resolve(&*plat, "mailpit", binaries::pins().mailpit)
        .await
        .expect("resolve mailpit");
    let mut server = common::OwnedService::new(mail::start(&*plat, &bin).expect("start mailpit"), "mailpit");
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

    // The unread filter + Mark all read, in the ONE order that can prove both:
    // reading a message is a SIDE EFFECT of `detail` above, so by now exactly
    // one of the two is read. A filter asserted on a fresh inbox (everything
    // unread) or a swept one (nothing unread) would pass while filtering
    // nothing at all.
    let unread = mail::list(mail::search_query(None, true).as_deref()).await.expect("unread list");
    println!("✓ is:unread → {} of {} message(s)", unread.messages.len(), list.total);
    assert_eq!(unread.messages.len(), 1, "the unread filter did not narrow the inbox");
    assert!(unread.messages.iter().all(|m| !m.read), "a READ message came back from is:unread");
    assert!(unread.messages.iter().all(|m| m.id != id), "the message just previewed is still unread");

    // Composed with a search: both terms must apply. A filter that REPLACED the
    // search would return the unread message even when it doesn't match the
    // text — and would look like it worked, because unread mail did appear.
    let other_subject = &unread.messages[0].subject.clone();
    let both = mail::list(mail::search_query(Some("Welcome"), true).as_deref())
        .await
        .expect("search+unread");
    println!("✓ 'Welcome is:unread' → {} hit(s) (unread subject is {other_subject:?})", both.messages.len());
    assert!(
        both.messages.iter().all(|m| !m.read && m.subject.contains("Welcome")),
        "search + unread did not compose — one of the two terms was dropped"
    );

    // Mark all read: every message, no ids, and the unread filter goes empty.
    mail::mark_all_read().await.expect("mark all read");
    let after = mail::list(None).await.expect("list after mark-all");
    println!("✓ mark all read: unread={} (was {})", after.unread, list.unread);
    assert_eq!(after.unread, 0, "unread count survived Mark all read");
    assert!(after.messages.iter().all(|m| m.read), "a message stayed unread after Mark all read");
    assert_eq!(after.total, list.total, "MARK ALL READ DELETED MESSAGES — it must only flip a flag");
    let none_unread = mail::list(mail::search_query(None, true).as_deref()).await.expect("unread after");
    assert!(none_unread.messages.is_empty(), "is:unread still returns messages after Mark all read");

    // raw
    let raw = mail::raw(&id).await.expect("raw");
    println!("✓ raw: {} bytes", raw.len());
    assert!(raw.to_lowercase().contains("subject:"), "raw missing headers");

    // clear
    mail::delete_all().await.expect("clear");
    assert_eq!(mail::list(None).await.unwrap().total, 0, "inbox not empty after final clear");
    println!("✓ clear: inbox emptied");

    server.stop();
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
