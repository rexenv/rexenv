//! Live check: the Windows relauncher waits for the process it was told about to
//! be GONE, and never for a stranger.
//! Run: `cargo run --example windows_app_relaunch_check`  (tier: sandbox, Windows only)
//!
//! # What this proves that L0 cannot
//!
//! The ordering IS the feature, for the same reason as on macOS
//! (`app_relaunch_check`): a relaunch that starts while the old process is still
//! holding the single-instance endpoint hands its launch to a process on its way
//! out. No unit test can observe it — it needs two real processes, a real pid and
//! the kernel's own word that one of them is gone (`WaitForSingleObject` on the
//! process handle here, kqueue `NOTE_EXIT` there). So this spawns a fixture
//! "parent", runs the REAL app binary in relauncher mode against it, and watches
//! WHEN the launch happens relative to that parent's death.
//!
//! # Fixture-owned
//!
//! The "parent" is a `ping -n 30` this example started and kills. The "bundle" the
//! relauncher starts is a fixture directory under the sandbox root whose
//! `rexenv.exe` is a COPY OF THIS EXAMPLE: launched with `REXENV_RELAUNCH_MARKER`
//! in its environment it appends a timestamp to that file and exits before touching
//! anything else. The relauncher inherits this process's environment and passes it
//! on, which is how the marker path reaches the fixture without an argument the
//! real relauncher would never pass. Nothing outside the sandbox root is written,
//! and no real rexenv process is signalled.

#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_imports))]

use rexenv_lib::core::app_update::{parse_relaunch_args, RelaunchArgs, RELAUNCH_FLAG};
use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

mod common;

const MARKER_ENV: &str = "REXENV_RELAUNCH_MARKER";

/// The `rexenv.exe` beside this example's directory, refused when it predates the
/// code the relauncher actually is — `cargo run --example` does not rebuild the
/// app binary, and a stale one would let this file agree with a change it never
/// executed.
fn app_binary() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("current exe: {e}"))?
        .parent()
        .and_then(Path::parent)
        .ok_or("exe has no target dir")?
        .join("rexenv.exe");
    if !exe.is_file() {
        return Err(format!(
            "windows_app_relaunch_check: the app binary is missing at {} — build it first: cargo build --bin rexenv",
            exe.display()
        ));
    }
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let newest = ["platform/windows/app_bundle.rs", "core/app_update.rs", "main.rs"]
        .iter()
        .filter_map(|f| mtime(&src.join(f)))
        .max();
    if let (Some(built), Some(edited)) = (mtime(&exe), newest) {
        if built < edited {
            return Err(format!(
                "windows_app_relaunch_check: REFUSING — {} is OLDER than the relauncher's source.\n\
                 Run: cargo build --bin rexenv && cargo run --example windows_app_relaunch_check",
                exe.display()
            ));
        }
    }
    Ok(exe)
}

fn spawn_parent() -> std::io::Result<std::process::Child> {
    Command::new("ping")
        .args(["-n", "30", "127.0.0.1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
}

fn marker_lines(marker: &Path) -> usize {
    std::fs::read_to_string(marker).map(|s| s.lines().count()).unwrap_or(0)
}

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

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> ExitCode {
    // ── The fixture half of this binary: launched by the relauncher, leave a mark ──
    if let Ok(marker) = std::env::var(MARKER_ENV) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let mut line = format!("{stamp}\n");
        if let Ok(existing) = std::fs::read_to_string(&marker) {
            line = existing + &line;
        }
        let _ = std::fs::write(&marker, line);
        return ExitCode::SUCCESS;
    }

    let (plat, _sandbox) = common::sandbox("winrelaunch");
    let mut checks = common::Check::new("windows_app_relaunch_check");
    let root = plat.paths().app_data_dir().expect("sandbox app data");
    let bundle = root.join("rexenv");
    let marker = root.join("marker.log");
    std::fs::create_dir_all(&bundle).expect("fixture bundle dir");
    // The fixture "app" is this example itself, under the name the relauncher starts.
    let me = std::env::current_exe().expect("current exe");
    std::fs::copy(&me, bundle.join("rexenv.exe")).expect("copy the fixture exe");
    // Inherited by the relauncher, and by the fixture it starts.
    std::env::set_var(MARKER_ENV, &marker);

    // ── 1. The argv contract, which the OLD build writes and the NEW one reads ──
    let argv: Vec<String> = vec![
        "C:\\Users\\x\\AppData\\Local\\rexenv\\rexenv.exe".into(),
        RELAUNCH_FLAG.into(),
        "4242".into(),
        "133000000000000000".into(),
        "C:\\Users\\x\\AppData\\Local\\rexenv".into(),
    ];
    checks.is(
        "the relaunch arguments round-trip through the flag the old build writes",
        parse_relaunch_args(&argv)
            == Some(RelaunchArgs {
                parent: 4242,
                parent_start: "133000000000000000".into(),
                bundle: "C:\\Users\\x\\AppData\\Local\\rexenv".into(),
            }),
        &format!("{:?}", parse_relaunch_args(&argv)),
    );

    // ── 2. THE ordering claim: the launch happens only after the parent dies ──
    let exe = match app_binary() {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    let mut parent = spawn_parent().expect("fixture parent");
    let pid = parent.id();
    let token = match rexenv_lib::platform::process_start_token(pid) {
        Some(t) => t,
        None => {
            checks.is("the fixture parent has a start token", false, "creation time unreadable");
            let _ = parent.kill();
            let _ = parent.wait();
            return checks.verdict();
        }
    };
    checks.is(
        "the start token is the creation time in FILETIME ticks (all digits, non-zero)",
        !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) && token != "0",
        &token,
    );
    let mut helper = Command::new(&exe)
        .arg(RELAUNCH_FLAG)
        .arg(pid.to_string())
        .arg(&token)
        .arg(&bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the relauncher");

    // A NEGATIVE claim needs a real window: well past the time a relauncher that
    // was not waiting at all would take to start the fixture.
    std::thread::sleep(Duration::from_secs(3));
    checks.is(
        "nothing is started while the parent is still alive",
        marker_lines(&marker) == 0,
        "the relauncher started the bundle before the process it waits for had exited",
    );

    let _ = parent.kill();
    let _ = parent.wait();
    checks.is(
        "the bundle is started once the parent is gone",
        wait_for_marker(&marker, 1, Duration::from_secs(20)),
        "the relauncher never started the bundle after its parent exited",
    );
    let _ = helper.wait();

    // ── 3. A recycled pid is not waited on ───────────────────────────────────
    let mut other = spawn_parent().expect("second fixture parent");
    let before = marker_lines(&marker);
    let mut wrong = Command::new(&exe)
        .arg(RELAUNCH_FLAG)
        .arg(other.id().to_string())
        .arg("1")
        .arg(&bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
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
         swapped directory — that needs an installed rexenv, which no Windows machine has\n\
         yet; SMOKE-TEST's Windows section once an installer has been installed."
    );
    checks.verdict()
}

/// The relauncher under test is Windows' (`platform::run_relauncher`), and so is the
/// creation-time token; the macOS counterpart is `app_relaunch_check`.
#[cfg(not(target_os = "windows"))]
fn main() -> ExitCode {
    eprintln!("windows_app_relaunch_check: skipped — Windows-only (relauncher)");
    ExitCode::SUCCESS
}
