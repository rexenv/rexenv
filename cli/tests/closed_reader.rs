//! Ledger #767 — the built `rex`, run against a pipe whose reader is already gone.
//!
//! `rex status | head -1` on the 22.04 VM (30 Sep 2026) ended in `thread 'main' panicked …
//! failed printing to stdout: Broken pipe (os error 32)`: std's print macros panic when the
//! write fails, and a reader that stopped listening is a write failure. The lib tests prove the
//! classification and that every print goes through `emit`; this one proves the binary — the
//! read end is dropped BEFORE the child starts, so the first write hits a closed pipe on every
//! OS, with no race against a reader that might still be there.

use std::process::{Command, Stdio};

#[test]
fn a_reader_that_left_ends_rex_quietly_with_exit_0() {
    let (reader, writer) = std::io::pipe().expect("an anonymous pipe");
    drop(reader);
    // `rex -h` is native output — no app needed, and the one write it makes is the USAGE text.
    let output = Command::new(env!("CARGO_BIN_EXE_rex"))
        .arg("-h")
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .expect("rex runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "a closed reader is not rex's failure: {:?}\n{stderr}", output.status);
    assert!(stderr.is_empty(), "nothing is said about a reader that left:\n{stderr}");
}
