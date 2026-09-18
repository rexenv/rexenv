//! Live check: the relauncher waits for the process it was told about to be
//! GONE, and never for a stranger.
//! Run: `cargo run --example app_relaunch_check`  (tier: sandbox)
//!
//! # What this proves that L0 cannot
//!
//! The ordering IS the feature. rexenv's single-instance lock is a unix socket
//! claimed before Tauri boots, and liveness is one `connect()` with no retry
//! (ledger #441) — so a relaunch that starts while the old process is still
//! listening hands its launch to a process on its way out, and both exit,
//! leaving no rexenv running at all. `AppHandle::restart` has exactly that shape
//! (it spawns the child, THEN exits), which is why this app does not use it.
//!
//! No unit test can observe that ordering: it needs two real processes, a real
//! pid, and the kernel's own notification that one of them is gone. So this
//! example spawns a fixture "parent", runs the real relauncher against it, and
//! watches WHEN the launch happens relative to that parent's death.
//!
//! # Fixture-owned
//!
//! The "parent" is a `sleep` this example started and kills. The "bundle" the
//! relauncher opens is a fixture `.app` under the sandbox root whose executable
//! writes a marker file and exits — so `open` really is exercised (through
//! LaunchServices, as in production) without launching rexenv or anything else
//! the developer is running. Nothing outside the sandbox root is written, and no
//! real rexenv process is signalled.

// Skipped on this host: `main` is a stub here, so every item below it is unreachable on
// purpose. Silence the dead-code / unused-import reds that fact produces under
// `clippy -D warnings` on the other OS (W12) -- and only there.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]

use rexenv_lib::core::app_update::{parse_relaunch_args, RelaunchArgs, RELAUNCH_FLAG};
use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

mod common;

/// A fixture `.app` whose executable appends to `marker` and exits. LaunchServices
/// needs a real bundle, so this is one — minimal, but real.
fn make_marker_app(at: &Path, marker: &Path) -> std::io::Result<()> {
    let macos = at.join("Contents/MacOS");
    std::fs::create_dir_all(&macos)?;
    std::fs::write(
        at.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key><string>dev.rexenv.relaunchfixture</string>
	<key>CFBundleExecutable</key><string>marker</string>
	<key>CFBundleShortVersionString</key><string>1.0.0</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>LSUIElement</key><true/>
</dict>
</plist>
"#,
    )?;
    let script = format!("#!/bin/sh\ndate +%s.%N >> '{}'\n", marker.display());
    let exe = macos.join("marker");
    std::fs::write(&exe, script)?;
    common::set_mode(&exe, 0o755)?;
    Ok(())
}

fn spawn_parent() -> std::io::Result<std::process::Child> {
    Command::new("/bin/sleep")
        .arg("30")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
}

/// The `rexenv` binary beside this example, refused when it predates the code
/// the relauncher actually is.
fn app_binary() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("current exe: {e}"))?
        .parent()
        .ok_or("exe has no parent")?
        .join("../rexenv")
        .canonicalize()
        .map_err(|_| {
            "app_relaunch_check: the app binary is missing — build it first: cargo build"
                .to_string()
        })?;
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let newest = ["platform/macos/relauncher.rs", "core/app_update.rs", "main.rs"]
        .iter()
        .filter_map(|f| mtime(&src.join(f)))
        .max();
    if let (Some(built), Some(edited)) = (mtime(&exe), newest) {
        if built < edited {
            return Err(format!(
                "app_relaunch_check: REFUSING — {} is OLDER than the relauncher's source.\n\
                 `cargo run --example` does not rebuild the app binary, so this run would test\n\
                 whatever was compiled last time. Run: cargo build && cargo run --example \
                 app_relaunch_check",
                exe.display()
            ));
        }
    }
    Ok(exe)
}

fn marker_lines(marker: &Path) -> usize {
    std::fs::read_to_string(marker).map(|s| s.lines().count()).unwrap_or(0)
}

/// Wait up to `limit` for the marker to gain a line.
fn wait_for_marker(marker: &Path, want: usize, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if marker_lines(marker) >= want {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _sandbox) = common::sandbox("relaunch");
    let mut checks = common::Check::new("app_relaunch_check");
    let root = plat.paths().app_data_dir().expect("sandbox app data");
    let bundle = root.join("Marker.app");
    let marker = root.join("marker.log");
    make_marker_app(&bundle, &marker).expect("fixture bundle");

    // ── 1. The argv contract, which the OLD build writes and the NEW one reads ──
    let argv: Vec<String> = vec![
        "/Applications/rexenv.app/Contents/MacOS/rexenv".into(),
        RELAUNCH_FLAG.into(),
        "4242".into(),
        "TOKEN".into(),
        "/Applications/rexenv.app".into(),
    ];
    checks.is(
        "the relaunch arguments round-trip through the flag the old build writes",
        parse_relaunch_args(&argv)
            == Some(RelaunchArgs {
                parent: 4242,
                parent_start: "TOKEN".into(),
                bundle: "/Applications/rexenv.app".into(),
            }),
        &format!("{:?}", parse_relaunch_args(&argv)),
    );
    checks.is(
        "a truncated or zero-pid invocation parses to nothing rather than a default",
        parse_relaunch_args(&argv[..3]).is_none()
            && parse_relaunch_args(&[
                "x".into(),
                RELAUNCH_FLAG.into(),
                "0".into(),
                "T".into(),
                "/tmp/x.app".into(),
            ])
            .is_none()
            && parse_relaunch_args(&["rexenv".into()]).is_none(),
        "a malformed relaunch must not open anything",
    );

    // ── 2. THE ordering claim: the launch happens only after the parent dies ──
    //
    // The relauncher runs from the APP BINARY, not from this example: the flag is
    // dispatched in `main.rs` before Tauri boots, and testing anything else would
    // be testing a copy. `cargo run --example` does NOT rebuild that binary, so a
    // stale one would let this file agree with a change it never executed — the
    // failure `tunnel_parent_death_check` records, where both plants came back
    // green against yesterday's build. Staleness is therefore a refusal.
    //
    // Resolved BEFORE the fixture parent is spawned, so the refusal path cannot
    // leave a process behind.
    let exe = match app_binary() {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    let mut parent = spawn_parent().expect("fixture parent");
    let pid = parent.id();
    let token = rexenv_lib::platform::process_start_token(pid)
        .expect("the fixture parent has a start token");
    let mut helper = Command::new(&exe)
        .arg(RELAUNCH_FLAG)
        .arg(pid.to_string())
        .arg(&token)
        .arg(&bundle)
        .spawn()
        .expect("spawn the relauncher");

    // Give it well past the time it would take to open the bundle if it were
    // not waiting at all. The claim is a NEGATIVE, so it needs a real window.
    std::thread::sleep(Duration::from_secs(3));
    checks.is(
        "nothing is opened while the parent is still alive",
        marker_lines(&marker) == 0,
        "the relauncher opened the bundle before the process it waits for had exited — \
         this is the single-instance race AppHandle::restart creates",
    );

    let _ = parent.kill();
    let _ = parent.wait();
    checks.is(
        "the bundle is opened once the parent is gone",
        wait_for_marker(&marker, 1, Duration::from_secs(20)),
        "the relauncher never opened the bundle after its parent exited",
    );
    // Reaped rather than dropped: `Child::drop` neither kills nor waits, and a
    // dropped handle leaves a zombie for the life of this process.
    let _ = helper.wait();

    // ── 3. A recycled pid is not waited on ───────────────────────────────────
    //
    // Between rexenv reading its own pid and the helper registering for its
    // death, the kernel may recycle that number. Registering against a stranger
    // would park the relaunch on someone else's lifetime — so a start token that
    // does not match reads as "already gone", and the helper proceeds.
    let mut other = spawn_parent().expect("second fixture parent");
    let before = marker_lines(&marker);
    let mut wrong = Command::new(&exe)
        .arg(RELAUNCH_FLAG)
        .arg(other.id().to_string())
        .arg("not-the-token-this-pid-wears")
        .arg(&bundle)
        .spawn()
        .expect("spawn the relauncher with a wrong token");
    checks.is(
        "a pid whose start token does not match is treated as already gone",
        wait_for_marker(&marker, before + 1, Duration::from_secs(20)),
        "the relauncher waited on a process it could not identify",
    );
    let _ = wrong.wait();
    checks.is(
        "and it did not wait for that unrelated process to exit",
        other.try_wait().ok().flatten().is_none(),
        "the stranger exited on its own, so the leg above proves nothing",
    );
    let _ = other.kill();
    let _ = other.wait();

    println!(
        "\nNOT covered here: the real app quitting through the quit gate and reopening on a\n\
         swapped bundle — that needs a real rexenv and a real /Applications, and it is\n\
         SMOKE-TEST §In-app self-update / PUBLISH-TESTING §M."
    );
    checks.verdict()
}

/// The relauncher is macOS's (`platform::run_relauncher`), and so is the process
/// start-time token this check identifies the parent by — skipped elsewhere
/// until the Windows `AppBundle` exists (docs/PLAN-windows-port.md W11).
#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    eprintln!("app_relaunch_check: skipped — macOS-only (relauncher)");
    ExitCode::SUCCESS
}
