//! Live check: `proxy::edge_wire` tells the three states apart against REAL
//! sockets — the measurement the mapping rests on.
//!
//!   cargo run --example edge_wire_check
//!
//! # Why this cannot be a unit test
//!
//! The rule is "a refused connection proves nothing is listening; everything
//! else does not", and whether reqwest reports a given failure as a connect
//! error is reqwest's business. Reading its docs and believing them is how a
//! posture ends up resting on a third party's behaviour — the shape this
//! project has been burned by often enough to have a name for it. So this puts
//! three real listeners (and one deliberate absence) in front of the real probe.
//!
//! # The failure directions are not equal
//!
//! A false `NoAnswer` HIDES a blocker: the user is told the port is free while
//! something else answers every site. A false `Foreign` merely sends them
//! looking for a program that is not there. So the ambiguous cases must resolve
//! toward `Foreign`, and the leg that matters most here is the BARE TCP
//! LISTENER — something is squatting :443 without speaking TLS, which is a real
//! blocker and the shape most likely to be misfiled as "nothing there".
//!
//! # Fixture ports
//!
//! 18134 (bare TCP listener), 18135 (nothing — never bound), 18136 (a plaintext
//! responder). Claimed in `examples/common/mod.rs`. Nothing here binds :443; the
//! real edge is only READ, which is why this is sandbox tier.

mod common;

use rexenv_lib::core::proxy::{self, EdgeWire};
use std::io::Write;
use std::net::TcpListener;

/// Something is listening and never completes an exchange. The blocker shape
/// that must NOT read as an empty port.
const PORT_SILENT: u16 = 18134;
/// Nothing is bound here, ever. The only true `NoAnswer`.
const PORT_EMPTY: u16 = 18135;
/// A listener that answers the TCP connect and then sends plain HTTP at a
/// client expecting TLS — "something is there, and it is not us".
const PORT_NOT_TLS: u16 = 18136;

fn fail(step: &str, why: &str) -> ! {
    eprintln!("\n✗ {step}\n  {why}\n");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    // A listener that accepts and then holds the connection open, saying
    // nothing. This is Herd-shaped in the way that matters: the port is taken.
    let silent = TcpListener::bind(("127.0.0.1", PORT_SILENT)).unwrap_or_else(|e| {
        fail("FIXTURE", &format!("could not bind the silent listener on :{PORT_SILENT}: {e}"))
    });
    std::thread::spawn(move || {
        for stream in silent.incoming().flatten() {
            // Hold it, say nothing, and never close first.
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(30));
                drop(stream);
            });
        }
    });

    // A listener that speaks plain HTTP where TLS is expected.
    let plain = TcpListener::bind(("127.0.0.1", PORT_NOT_TLS)).unwrap_or_else(|e| {
        fail("FIXTURE", &format!("could not bind the plaintext listener on :{PORT_NOT_TLS}: {e}"))
    });
    std::thread::spawn(move || {
        for mut stream in plain.incoming().flatten() {
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        }
    });

    // Prove the empty port really is empty before trusting anything it says.
    if TcpListener::bind(("127.0.0.1", PORT_EMPTY)).is_err() {
        fail(
            "FIXTURE",
            &format!(
                ":{PORT_EMPTY} is in use, so the `NoAnswer` leg would be measuring somebody \
                 else's listener rather than an empty port."
            ),
        );
    }

    let host = rexenv_lib::core::adminer::ADMINER_HOST;

    // ── A · nothing listening → NoAnswer ────────────────────────────────────
    let empty = proxy::edge_wire(host, PORT_EMPTY).await;
    if empty != EdgeWire::NoAnswer {
        fail(
            "A — a refused connection is not reported as an empty port",
            &format!(
                "got {empty:?} on :{PORT_EMPTY}, where nothing is bound. If a refused connection \
                 does not read as NoAnswer, the state is unreachable and every caller is back to \
                 a boolean."
            ),
        );
    }
    println!("A ok — nothing listening → NoAnswer");

    // ── B · a bare TCP listener → Foreign (the leg that matters) ────────────
    let silent_state = proxy::edge_wire(host, PORT_SILENT).await;
    if silent_state != EdgeWire::Foreign {
        fail(
            "B — a squatting listener was filed as an empty port",
            &format!(
                "got {silent_state:?} on :{PORT_SILENT}, where a socket is bound and never \
                 answers. This is the failure direction that HIDES a blocker: the user is told \
                 the port is free while something else holds it. `edge_wire`'s listening \
                 check is wrong for this shape — an ambiguous result must resolve toward \
                 Foreign."
            ),
        );
    }
    println!("B ok — a listener that never speaks → Foreign, not NoAnswer");

    // ── C · something answering that is not us → Foreign ────────────────────
    let plain_state = proxy::edge_wire(host, PORT_NOT_TLS).await;
    if plain_state != EdgeWire::Foreign {
        fail(
            "C — a foreign responder was not reported as Foreign",
            &format!("got {plain_state:?} on :{PORT_NOT_TLS}, where a plaintext server answers."),
        );
    }
    println!("C ok — a non-TLS responder on the port → Foreign");

    // ── D · the boolean still means what it always meant ────────────────────
    //
    // The four existing callers ask the boolean. Both not-ours states must
    // collapse to false for them, or introducing the richer state changed
    // behaviour on paths that gate Start-all and login autostart.
    for (port, what) in
        [(PORT_EMPTY, "an empty port"), (PORT_SILENT, "a squatting listener"), (PORT_NOT_TLS, "a foreign responder")]
    {
        if proxy::edge_answers_as_ours(host, port).await {
            fail(
                "D — the boolean claimed our edge",
                &format!("`edge_answers_as_ours` returned true for {what} on :{port}"),
            );
        }
    }
    println!("D ok — the boolean is false for all three not-ours shapes");


    // ── E · the REAL edge, when it happens to be running ────────────────────
    //
    // Skipped rather than failed when the stack is down: this example is
    // sandbox-tier and must not require it. When it IS up, this is the only leg
    // that proves `Ours` is reachable at all — without it, a probe that always
    // said Foreign would pass every other leg here.
    let real = proxy::edge_wire(host, proxy::DEFAULT_HTTPS_PORT).await;
    match real {
        EdgeWire::Ours => println!("E ok — the running edge on :443 reads Ours"),
        EdgeWire::NoAnswer => {
            println!("E skipped — nothing on :{} (stack stopped)", proxy::DEFAULT_HTTPS_PORT)
        }
        EdgeWire::Foreign => println!(
            "E skipped — something else holds :{} (Herd, or another proxy)",
            proxy::DEFAULT_HTTPS_PORT
        ),
    }
    println!(
        "\n✓ edge_wire_check: refused → NoAnswer, squatting or answering → Foreign, and the \
         boolean four callers still ask is unchanged"
    );
}
