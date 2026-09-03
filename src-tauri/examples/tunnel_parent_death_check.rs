//! Live check: the parent-death guard ends a share when the app is SIGKILLed —
//! and refuses to signal anything it cannot still identify.
//!
//!   cargo run --example tunnel_parent_death_check      (sandbox tier)
//!
//! Not the same "guard" as `tunnel_guard_check.rs`, which pins this harness's own
//! reaper. This one exercises production code: `platform/macos/parent_death_guard.rs`,
//! re-executed through the app binary's `--tunnel-guard` mode.
//!
//! # What only a live run can show
//!
//! The decision the guard makes is `is_our_tunnel`, and that is L0-proven. What
//! is NOT provable in a unit test is the mechanism around it: that
//! `EVFILT_PROC`/`NOTE_EXIT` actually fires for a pid we did not fork, that it
//! fires on a SIGKILL (which runs no code in the dying process — the entire
//! reason this exists), and that the guard then signals the right process and
//! exits. That is three real processes and a real kernel, so it is an example.
//!
//! # Spawns nothing real
//!
//! No services, no ports, no app state. The stand-in for cloudflared is a shell
//! script this example writes into its OWN temp dir, named `cloudflared` and
//! given an argv that satisfies `is_our_tunnel` — the app-data marker, the
//! program name, and a `--http-host-header <domain>` pair for a domain no site
//! uses. The stand-in for rexenv is a `sleep`. Every pid spawned here is killed
//! on the way out, including on the failure paths.
//!
//! # The negative legs are the point
//!
//! A guard that kills whatever pid it was handed would pass a positive-only
//! check and be a loaded gun: pids are recycled, and the gap between the
//! parent's death and the guard waking is exactly when a recycled pid is
//! plausible. Leg 2 hands the guard a pid whose argv names a DIFFERENT domain
//! and requires it to survive. Leg 3 kills the child first and requires the
//! guard to exit on its own rather than sit on a dead pid — one process per
//! share, forever, would be a leak of its own.
//!
//! **Leg 2's first run failed for a fixture reason worth recording**: the
//! stand-in was `sh -c 'sleep 300' <argv…>`, which EXECS sleep, so `ps` reported
//! `sleep 300` — no marker, no `cloudflared`. The guard correctly refused to
//! kill it, and a positive leg written the same way would have "passed" while
//! proving nothing. The fixture has to look like production (a real argv), which
//! is the same lesson this project has now paid for three times.

mod common;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// A domain no real site can have, so a mis-identification cannot reach one.
const FIXTURE_DOMAIN: &str = "parent-death-fixture.rex";

/// Is one of OUR OWN children still running?
///
/// `kill -0` is the obvious probe and it is WRONG here — it succeeds on a
/// ZOMBIE, and every process this example spawns becomes one the moment it dies,
/// because nothing has waited on it yet. The first version of this file used it
/// and reported legs 1 and 3 as failures while the guard was working perfectly:
/// the same run passed by hand in a shell, which reaps its background jobs for
/// you. `try_wait` asks the parent's own bookkeeping instead, which is the only
/// thing that can tell a live child from an unreaped corpse.
fn alive(child: &mut Owned) -> bool {
    matches!(child.0.try_wait(), Ok(None))
}

/// A child THIS example owns, killed and waited for on drop — so an `expect`
/// that unwinds mid-leg (the first version's explicit `reap` calls sat after
/// the assertions, and a panic walked straight past them) cannot leave a
/// stand-in `sleep 300`, a stand-in tunnel, or a real guard process behind.
/// The `common::Reaped` fixture is for services on a fixture PORT; these
/// stand-ins listen on nothing, so the invariant is carried locally.
struct Owned(Child);

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Kill one of our children and WAIT for it, so it leaves no zombie behind to
/// confuse the next probe. Explicit where a leg needs the kill NOW ("the user
/// stops sharing"); the drop guard covers every other exit.
fn reap(child: &mut Owned) {
    let _ = child.0.kill();
    let _ = child.0.wait();
}

/// SIGKILL a pid we do not own the handle for (used only on the stand-in parent
/// when we want the guard, not us, to observe the death).
fn sigkill(pid: u32) {
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

/// A stand-in rexenv: something to SIGKILL.
fn stand_in_parent() -> Owned {
    Owned(Command::new("/bin/sh")
        .args(["-c", "sleep 300"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn stand-in parent"))
}

/// A stand-in cloudflared whose ARGV is what the guard identifies on. It must
/// carry the app-data marker, the program name, and the host-header pair — a
/// process that merely sleeps is not the thing under test.
fn stand_in_tunnel(script: &PathBuf, marker: &str, domain: &str) -> Owned {
    Owned(Command::new(script)
        .args([
            "tunnel",
            "--no-autoupdate",
            "--url",
            "http://127.0.0.1:18088",
            "--http-host-header",
            domain,
            "--marker",
            marker,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn stand-in tunnel"))
}

fn main() {
    let plat = rexenv_lib::platform::current();
    let marker = plat
        .paths()
        .app_data_dir()
        .expect("app data dir")
        .display()
        .to_string();

    // Fixture-owned temp dir: everything this example writes lives here.
    let dir = std::env::temp_dir().join(format!("rexenv-parent-death-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let script = dir.join("cloudflared");
    std::fs::write(&script, "#!/bin/sh\nsleep 300\n").expect("write stand-in");
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
    }
    std::fs::set_permissions(&script, perms).unwrap();

    // The guard runs from the APP BINARY, which `cargo run --example` does not
    // rebuild — it builds this example and links the library, and the `rexenv`
    // bin target is left exactly as it was. So an edit to the guard is invisible
    // here unless someone remembers `cargo build`, and the first run of this
    // file proved how that ends: BOTH plants (never signal, and kill without
    // identifying) came back ALL PASS against a stale binary. A check that can
    // agree with a change it never executed is worse than no check.
    //
    // So staleness is a REFUSAL, not a note. Same family as `cli_socket_check`'s
    // "unknown command … newer than the running app".
    let exe = std::env::current_exe()
        .expect("current exe")
        .parent()
        .expect("exe dir")
        .join("../rexenv")
        .canonicalize()
        .expect("the app binary is missing — build it first: cargo build");
    // Compared against the guard's SOURCE, not against this example's binary:
    // `cargo run --example` relinks the example last every time, so "newer than
    // me" is always true and would refuse every run. What matters is whether the
    // binary predates the code it is supposed to be running.
    let mtime = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let newest_source = ["platform/macos/parent_death_guard.rs", "core/tunnels.rs"]
        .iter()
        .filter_map(|f| mtime(&src.join(f)))
        .max();
    if let (Some(built), Some(edited)) = (mtime(&exe), newest_source) {
        if built < edited {
            eprintln!(
                "tunnel_parent_death_check: REFUSING — {} is OLDER than the guard's source.\n\
                 `cargo run --example` does not rebuild the app binary, so this run would test\n\
                 whatever was compiled last time. Run: cargo build && cargo run --example \
                 tunnel_parent_death_check",
                exe.display()
            );
            std::process::exit(2);
        }
    }

    let mut failures: Vec<String> = Vec::new();
    let guard = |parent: u32, child: u32, domain: &str| -> Owned {
        Owned(
            Command::new(&exe)
                .args(["--tunnel-guard", &parent.to_string(), &child.to_string(), domain])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn guard"),
        )
    };

    // ── Leg 1: the app is SIGKILLed → the share dies with it ──────────────────
    {
        println!("leg 1: parent SIGKILLed — the share must not survive it");
        let mut parent = stand_in_parent();
        let mut tunnel = stand_in_tunnel(&script, &marker, FIXTURE_DOMAIN);
        let (p, c) = (parent.0.id(), tunnel.0.id());
        let mut g = guard(p, c, FIXTURE_DOMAIN);
        std::thread::sleep(Duration::from_millis(800));
        sigkill(p); // SIGKILL: the dying app runs NONE of its own shutdown code
        std::thread::sleep(Duration::from_millis(3500));
        if alive(&mut tunnel) {
            failures.push(
                "the share outlived a SIGKILLed app — this is the window the guard exists to \
                 close, and without it the site stays public until the next launch"
                    .into(),
            );
        } else {
            println!("  ✓ stand-in tunnel stopped");
        }
        reap(&mut tunnel);
        reap(&mut parent);
        reap(&mut g);
    }

    // ── Leg 2: an unidentifiable pid is never signalled ───────────────────────
    {
        println!("leg 2: the pid's argv names ANOTHER domain — it must survive");
        let mut parent = stand_in_parent();
        let mut tunnel = stand_in_tunnel(&script, &marker, "someone-elses.rex");
        let (p, c) = (parent.0.id(), tunnel.0.id());
        let mut g = guard(p, c, FIXTURE_DOMAIN);
        std::thread::sleep(Duration::from_millis(800));
        sigkill(p);
        std::thread::sleep(Duration::from_millis(3500));
        if !alive(&mut tunnel) {
            failures.push(
                "the guard killed a pid it could NOT identify as this share. Pids are recycled, \
                 and the gap between the parent dying and the guard waking is exactly when the \
                 number belongs to someone else — never kill on a bare pid"
                    .into(),
            );
        } else {
            println!("  ✓ untouched");
        }
        reap(&mut tunnel);
        reap(&mut parent);
        reap(&mut g);
    }

    // ── Leg 3: the ordinary stop reaps the guard ──────────────────────────────
    {
        println!("leg 3: the share ends first — the guard must exit on its own");
        let mut parent = stand_in_parent();
        let mut tunnel = stand_in_tunnel(&script, &marker, FIXTURE_DOMAIN);
        let (p, c) = (parent.0.id(), tunnel.0.id());
        let mut g = guard(p, c, FIXTURE_DOMAIN);
        std::thread::sleep(Duration::from_millis(800));
        reap(&mut tunnel); // the user stops sharing
        std::thread::sleep(Duration::from_millis(1500));
        if alive(&mut g) {
            failures.push(
                "the guard outlived the share it guards — one leftover process per share, \
                 forever, sitting on a pid that can be recycled underneath it"
                    .into(),
            );
        } else {
            println!("  ✓ guard exited");
        }
        if !alive(&mut parent) {
            failures.push("the guard killed the PARENT — it must only ever signal the child".into());
        }
        reap(&mut parent);
        reap(&mut g);
        let _ = (p, c);
    }

    let _ = std::fs::remove_dir_all(&dir);
    if failures.is_empty() {
        println!("\ntunnel_parent_death_check: ALL PASS");
    } else {
        eprintln!("\ntunnel_parent_death_check: FAILED");
        for f in &failures {
            eprintln!("  - {f}");
        }
        std::process::exit(1);
    }
}
