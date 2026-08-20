//! Live check: the shared public-tunnel guard reaps from every path that ends a
//! run — `Drop`, a panic, and `process::exit` — and the explicit reap that
//! `fail()` needs is load-bearing rather than decorative.
//!
//!   cargo run --example tunnel_guard_check      (sandbox tier)
//!
//! Spawns no services, binds no ports, touches no app state: the stand-in for a
//! cloudflared is a `sleep` child, and the only other process is this example's
//! own binary re-run in four modes.
//!
//! # Why a self-spawning example rather than unit tests
//!
//! The three paths differ in what the RUNTIME does, not in what our code does.
//! `process::exit` skipping destructors cannot be observed from inside the
//! process that called it, and a panic hook's effect cannot be asserted by the
//! panicking thread. So each mode runs in a real child and the parent inspects
//! the corpse: is the stand-in pid still alive after the child is gone?
//!
//! # The control is the point
//!
//! `exit-bare` deliberately exits WITHOUT reaping and the stand-in MUST survive.
//! Without it the other three prove only "the guard was called somewhere" — and
//! the specific thing this file exists to pin is that `Drop` does NOT run on
//! `process::exit`, which is why `fail()` has to reap explicitly. That is the
//! bug that leaked a Mailpit once and a tunnel once; a test that could not
//! observe it would be agreeing with the fix rather than checking it.

mod common;

use std::process::Command;
use std::time::Duration;

const MODE_ENV: &str = "REXENV_TUNNEL_GUARD_MODE";

fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The child half: stand up a fake "tunnel", adopt it, then end this process the
/// way the mode says. Prints the stand-in pid on stdout so the parent can look
/// for its body afterwards.
fn run_child(mode: &str) {
    // stdout/stderr to null, NOT inherited. A child that inherits the pipe holds
    // its write end open, so the parent's `output()` blocks until the STAND-IN
    // exits rather than until this process does — the same inherited-pipe trap
    // `core::proxy::start_privileged` documents for the detached edge. First
    // version of this file hung for 300s on exactly that.
    // Not `wait()`ed on deliberately: the pid goes to `adopt_public_tunnel`, and
    // THAT guard owns the reaping through all three teardown paths. A `wait()`
    // here would block until the stand-in exits, which is the opposite of what
    // each mode is arranging to measure.
    #[allow(clippy::zombie_processes)]
    let child = Command::new("sleep")
        .arg("300")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the stand-in tunnel");
    let pid = child.id();
    println!("STANDIN_PID={pid}");
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let _guard = common::adopt_public_tunnel(pid, "https://example-stand-in.invalid");

    match mode {
        // Ordinary scope exit: fall out of this function and let `_guard` drop.
        // The first version wrote `process::exit(0)` here and the check FAILED —
        // correctly, because that is the skip-destructors path, which made this
        // mode a silent duplicate of the control. A mode that cannot fail
        // separately from another mode is not a mode.
        "drop" => {}
        // A panic: Drop runs while unwinding, and the hook covers the case where
        // the guard is no longer in scope.
        "panic" => panic!("deliberate panic — the guard must still reap"),
        // What `fail()` does: reap FIRST, then exit. This is the shape the
        // convention requires precisely because the next mode proves Drop is skipped.
        "exit-reaped" => {
            common::reap_public_tunnel();
            std::process::exit(1)
        }
        // CONTROL: exit without reaping. The stand-in MUST survive, or this file
        // is not observing the thing it claims to.
        "exit-bare" => std::process::exit(1),
        other => panic!("unknown mode {other}"),
    }
}

fn main() {
    if let Ok(mode) = std::env::var(MODE_ENV) {
        run_child(&mode);
        return;
    }

    // `drop` exits 0 through Drop; the guard runs on the way out of run_child's
    // frame. (exit(0) after `_guard` is in scope still unwinds nothing — so this
    // mode is really "the hook + Drop path"; see the control for the distinction.)
    let cases = [
        ("drop", false, "ordinary scope exit"),
        ("panic", false, "a panic mid-run"),
        ("exit-reaped", false, "fail() — reap, then process::exit"),
        ("exit-bare", true, "CONTROL: process::exit with no reap"),
    ];

    let exe = std::env::current_exe().expect("own binary");
    let mut ok = true;
    let mut strays: Vec<u32> = Vec::new();

    for (mode, expect_alive, what) in cases {
        let out = Command::new(&exe)
            .env(MODE_ENV, mode)
            .output()
            .expect("re-run self");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let Some(pid) = stdout
            .lines()
            .find_map(|l| l.strip_prefix("STANDIN_PID="))
            .and_then(|p| p.trim().parse::<u32>().ok())
        else {
            eprintln!("  ✗ {mode}: the child never reported a stand-in pid; this mode proved nothing");
            eprintln!("    stdout: {stdout}\n    stderr: {}", String::from_utf8_lossy(&out.stderr));
            ok = false;
            continue;
        };

        // The child is gone by now (output() waited). Give the reap a moment to
        // finish escalating before deciding.
        std::thread::sleep(Duration::from_millis(500));
        let alive = pid_alive(pid);
        let pass = alive == expect_alive;
        println!(
            "  {} {mode:<12} stand-in pid {pid} alive={alive} expected={expect_alive}  ({what})",
            if pass { "ok  " } else { "FAIL" }
        );
        if alive {
            strays.push(pid);
        }
        ok &= pass;
    }

    // The control deliberately leaks one; this example does not leave it behind.
    for pid in strays {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }

    if ok {
        println!(
            "\nOK — the guard reaps from Drop, from a panic, and from fail()'s explicit call;\n\
             and process::exit really does skip Drop, which is why that call has to be there."
        );
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}
