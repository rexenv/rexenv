//! Live check: stage, verify and SWAP a real install directory on Windows — on fixtures.
//! Run: `cargo run --example windows_app_bundle_swap_check`  (tier: sandbox, Windows only)
//!
//! # What this proves that L0 cannot
//!
//! `app_bundle_rules.rs` decides what paths and numbers MEAN, and the macOS host
//! proves that. What is unproven until something does this on a real Windows
//! filesystem is the half `platform/windows/app_bundle.rs` actually performs:
//!
//!   a real install directory on disk → zip its contents flat → extract through
//!   the guarded extractor → read the executable's OWN VERSIONINFO → check the PE
//!   machine → carry `uninstall.exe` across → rename the installed directory aside
//!   and the staged one in, while a process is running from it → sweep classifies
//!   the leftovers by the version INSIDE them → the previous one survives until it
//!   is told the new app is healthy.
//!
//! The Dell measured that Windows PERMITS the rename under a running process
//! (`docs/PLAN-windows-port.md` D5). This is the other half: that rexenv's own
//! code does it correctly, and — the part that matters more — that every failure
//! leaves the installed directory exactly as it was.
//!
//! # Fixture-owned, and that is the whole safety argument
//!
//! Everything happens inside `common::sandbox`'s root: a fixture parent folder, a
//! fixture install directory, and archives this example built. **No real install
//! is read, renamed or removed**: the swap is called with fixture facts, the guard
//! refuses to run at all if the sandbox root is not where it should be, and the
//! uninstall registry entry is only ever REWRITTEN if it already exists, never
//! created (`record_installed_version`), so a fixture swap on a developer's machine
//! leaves Apps & Features alone.
//!
//! The fixture executable is the REAL app binary from this build's `target`
//! directory — the only `.exe` around that carries the VERSIONINFO the verifier
//! reads — so its version is read back through PowerShell, an oracle that shares
//! no code with the crate, rather than assumed.

use rexenv_lib::core::app_update;
use rexenv_lib::platform::traits::{Arch, BundleFacts, InstallKind, StagedExpect};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod common;

/// What Windows itself says the file's product version and name are.
fn version_info_oracle(exe: &Path) -> Option<(String, String)> {
    let script = format!(
        "$v=(Get-Item '{}').VersionInfo; Write-Output ($v.ProductVersion + '|' + $v.ProductName)",
        exe.display()
    );
    let out = Command::new("powershell").args(["-NoProfile", "-Command", &script]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let (v, n) = s.split_once('|')?;
    Some((v.trim().to_string(), n.trim().to_string()))
}

/// Zip a directory's contents FLAT — the release archive's shape — optionally
/// under a wrapper directory, which is the shape the extractor must land wrong.
fn zip_dir(dir: &Path, out: &Path, wrapper: Option<&str>) -> std::io::Result<()> {
    let file = std::fs::File::create(out)?;
    let mut z = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let member = match wrapper {
            Some(w) => format!("{w}/{name}"),
            None => name,
        };
        z.start_file(member, opts).map_err(std::io::Error::other)?;
        let mut f = std::fs::File::open(&p)?;
        std::io::copy(&mut f, &mut z)?;
    }
    z.finish().map_err(std::io::Error::other)?;
    Ok(())
}

fn strays(parent: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(parent)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".rexenv-update-")))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::main]
async fn main() -> ExitCode {
    if !cfg!(target_os = "windows") {
        eprintln!("windows_app_bundle_swap_check: Windows only — the install directory and VERSIONINFO do not exist here");
        return ExitCode::FAILURE;
    }
    let (plat, _sandbox) = common::sandbox("winappswap");
    let mut checks = common::Check::new("windows_app_bundle_swap_check");
    let bundles = plat.app_bundle();

    let root = plat.paths().app_data_dir().expect("sandbox app data");
    let parent = root.join("Programs");
    if !parent.to_string_lossy().contains("rexenv-sandbox-") {
        eprintln!("refusing: the fixture parent is not inside a sandbox root");
        return ExitCode::FAILURE;
    }
    std::fs::create_dir_all(&parent).expect("fixture parent");

    // The real app binary of THIS build: examples live in target\<profile>\examples,
    // the app one level up. It is the only executable around with VERSIONINFO.
    let me = std::env::current_exe().expect("current exe");
    let profile_dir = me.parent().and_then(Path::parent).expect("target profile dir").to_path_buf();
    let real_exe = profile_dir.join("rexenv.exe");
    let real_rex = profile_dir.join("rex.exe");
    if !real_exe.is_file() {
        eprintln!("no {} — build the app in this profile first (cargo build --bin rexenv)", real_exe.display());
        return ExitCode::FAILURE;
    }
    let Some((real_version, real_product)) = version_info_oracle(&real_exe) else {
        eprintln!("PowerShell could not read {}'s VersionInfo", real_exe.display());
        return ExitCode::FAILURE;
    };
    println!("fixture executable: {} — {real_product} {real_version} (per PowerShell)", real_exe.display());
    let sidecar_src = if real_rex.is_file() { real_rex.clone() } else { real_exe.clone() };

    // ── 0. Facts about the REAL dev binary: a cargo target dir is a dev build ──
    let dev = bundles.facts(&real_exe).expect("facts of the dev build");
    checks.is(
        "the dev build's own binary is classified DevBuild and refused before any download",
        dev.kind == InstallKind::DevBuild
            && matches!(app_update::preflight(&dev, 1), Err(app_update::Refusal::NotABundle)),
        &format!("{:?}", dev.kind),
    );
    checks.is(
        "and its facts are real: writable, owned by this account, on a writable volume, no symlink on the way",
        dev.parent_writable && dev.owned_by_me && !dev.read_only && dev.canonical && dev.free_parent_bytes > 0,
        &format!("{dev:?}"),
    );

    // ── the fixture install directory (v1), and a v2 build to update it with ──
    let installed = parent.join("rexenv");
    std::fs::create_dir_all(&installed).expect("installed dir");
    std::fs::copy(&real_exe, installed.join("rexenv.exe")).expect("v1 exe");
    // v1's sidecar is a copy of the system's `ping.exe`: a real x64 PE that can be
    // told to stay alive for a minute, so the swap below happens under a process
    // that is genuinely executing from the installed directory. Not `timeout.exe`:
    // with stdin redirected it prints "Input redirection is not supported" and
    // exits at once, which made this leg fail for a fixture reason on the first
    // Dell run (19 Sep 2026) while the swap itself had worked.
    let waiter = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()))
        .join("System32")
        .join("ping.exe");
    std::fs::copy(&waiter, installed.join("rex.exe")).expect("v1 sidecar (ping.exe)");
    // The installer's uninstaller: NOT in any update archive, carried across.
    std::fs::copy(&sidecar_src, installed.join("uninstall.exe")).expect("v1 uninstaller");
    std::fs::write(installed.join("marker.txt"), "v1").expect("v1 marker");

    let build = root.join("build");
    std::fs::create_dir_all(&build).expect("build dir");
    std::fs::copy(&real_exe, build.join("rexenv.exe")).expect("v2 exe");
    std::fs::copy(&sidecar_src, build.join("rex.exe")).expect("v2 sidecar");
    std::fs::write(build.join("marker.txt"), "v2").expect("v2 marker");
    let archive = root.join("rexenv_update_x64.zip");
    zip_dir(&build, &archive, None).expect("fixture archive");

    // Facts as the ordinary per-user install has them, over fixture paths. The
    // kind is stated because `%LOCALAPPDATA%\rexenv` is the user's real place and
    // a fixture must never sit there.
    let facts = BundleFacts {
        bundle: installed.clone(),
        parent: parent.clone(),
        kind: InstallKind::ProgramsPerUser,
        homebrew: false,
        parent_writable: true,
        owned_by_me: true,
        read_only: false,
        canonical: true,
        free_parent_bytes: 10_000_000_000,
    };
    checks.is(
        "a per-user install passes preflight",
        app_update::preflight(&facts, 1).is_ok(),
        &format!("{:?}", app_update::preflight(&facts, 1)),
    );
    let expect = |version: &str| StagedExpect {
        version: version.into(),
        identifier: real_product.clone(),
        executable: "rexenv.exe".into(),
        archs: vec![Arch::X86_64],
        required_binaries: vec!["rex.exe".into()],
        codesign: false,
    };
    checks.is(
        "what the swap expects on Windows is what the build stamps (ProductName)",
        app_update::staged_expect_on(&real_version, "windows").identifier == real_product,
        &format!("{:?} vs {real_product:?}", app_update::staged_expect_on(&real_version, "windows").identifier),
    );

    // ── 1. Every refusal leaves v1 byte-identical ────────────────────────────
    let before = std::fs::read(installed.join("rexenv.exe")).expect("read v1");
    let wrong = bundles.stage(&facts, &archive, &expect("9.9.9"));
    checks.is(
        "an archive whose executable disagrees with the signed version is refused",
        wrong.is_err(),
        "a stale or swapped archive would have been installed",
    );
    if let Err(e) = &wrong {
        let m = e.to_string();
        checks.is(
            "the refusal names what the download actually said, read off VERSIONINFO",
            m.contains(&real_version) && m.contains("9.9.9"),
            &m,
        );
    }
    let mut wrong_product = expect(&real_version);
    wrong_product.identifier = "not-rexenv".into();
    checks.is(
        "an executable whose ProductName is not rexenv's is refused",
        bundles.stage(&facts, &archive, &wrong_product).is_err(),
        "somebody else's build would have been installed under rexenv's name",
    );
    let mut missing = expect(&real_version);
    missing.required_binaries = vec!["rexfix.exe".into()];
    checks.is(
        "an archive missing a required binary is refused",
        bundles.stage(&facts, &archive, &missing).is_err(),
        "a build without the rex sidecar would silently break every open terminal",
    );
    let wrapped = root.join("wrapped.zip");
    zip_dir(&build, &wrapped, Some("rexenv")).expect("wrapped archive");
    checks.is(
        "an archive with a wrapper directory is refused (the extractor strips nothing)",
        bundles.stage(&facts, &wrapped, &expect(&real_version)).is_err(),
        "the executable would have landed one level too deep and the swap would install an empty directory",
    );
    // A file with no VERSIONINFO at all in the executable's place.
    let noinfo = root.join("noinfo");
    std::fs::create_dir_all(&noinfo).unwrap();
    std::fs::write(noinfo.join("rexenv.exe"), b"MZ this is not a program").unwrap();
    std::fs::copy(&sidecar_src, noinfo.join("rex.exe")).unwrap();
    let noinfo_zip = root.join("noinfo.zip");
    zip_dir(&noinfo, &noinfo_zip, None).unwrap();
    let r = bundles.stage(&facts, &noinfo_zip, &expect(&real_version));
    checks.is(
        "an executable that carries no version information is refused, and says so",
        r.as_ref().err().is_some_and(|e| e.to_string().contains("version information")),
        &format!("{r:?}"),
    );
    checks.is(
        "every refusal above left the installed directory byte-identical",
        std::fs::read(installed.join("rexenv.exe")).ok().as_deref() == Some(&before[..])
            && std::fs::read_to_string(installed.join("marker.txt")).as_deref().ok() == Some("v1"),
        "the installed directory changed while nothing was supposed to happen",
    );
    checks.is(
        "a failed stage removes its own staging directory",
        strays(&parent).is_empty(),
        &format!("left behind: {:?}", strays(&parent)),
    );

    // ── 2. The real thing: stage, verify, swap — with a process RUNNING from v1 ──
    //
    // The measured fact this rests on: a running .exe cannot be deleted or
    // overwritten, but its directory can be renamed. So the swap happens while
    // something executes from the installed copy, exactly as the app does it to
    // itself. It is killed at the end whatever happens.
    let mut running = Command::new(installed.join("rex.exe"))
        .args(["-n", "60", "127.0.0.1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok();
    // Long enough for a process that is going to exit at once to have done so:
    // the check below must catch a dead waiter HERE, not as a false "the rename
    // killed it" after the swap.
    std::thread::sleep(std::time::Duration::from_millis(500));
    checks.is(
        "a process is running from the installed directory before the swap",
        running.as_mut().map(|c| c.try_wait().ok().flatten().is_none()).unwrap_or(false),
        "timeout.exe did not start, so the swap-under-a-running-process leg would be vacuous",
    );
    let staged = match bundles.stage(&facts, &archive, &expect(&real_version)) {
        Ok(s) => s,
        Err(e) => {
            checks.is("the correct archive stages", false, &e.to_string());
            if let Some(mut c) = running {
                let _ = c.kill();
            }
            return checks.verdict();
        }
    };
    checks.is(
        "the staged directory is a sibling of the installed one",
        staged.stage_dir.parent() == Some(parent.as_path()),
        &format!("{:?}", staged.path),
    );
    checks.is(
        "the installed uninstaller was carried into the staged directory",
        staged.path.join("uninstall.exe").is_file(),
        "after the swap Apps & Features could not remove rexenv",
    );

    let receipt = match bundles.swap(&installed, &staged) {
        Ok(r) => r,
        Err(e) => {
            checks.is("the swap succeeds on a fixture directory", false, &e.to_string());
            if let Some(mut c) = running {
                let _ = c.kill();
            }
            return checks.verdict();
        }
    };
    println!("swap method: {:?}", receipt.method);
    checks.is(
        "after the swap the install path holds the NEW build (its marker says v2)",
        std::fs::read_to_string(installed.join("marker.txt")).as_deref().ok() == Some("v2"),
        &format!("{:?}", std::fs::read_to_string(installed.join("marker.txt"))),
    );
    checks.is(
        "and the uninstaller is still there",
        installed.join("uninstall.exe").is_file(),
        "the swapped-in directory lost uninstall.exe",
    );
    checks.is(
        "the previous directory survives, and the receipt says where",
        receipt.previous.is_dir()
            && std::fs::read_to_string(receipt.previous.join("marker.txt")).as_deref().ok() == Some("v1"),
        &format!("{:?}", receipt.previous),
    );
    let still_running = running.as_mut().map(|c| c.try_wait().ok().flatten().is_none()).unwrap_or(false);
    checks.is(
        "the process that was running from the previous directory is still running",
        still_running,
        "the rename killed the running process — the app would die mid-update",
    );
    if let Some(mut c) = running.take() {
        let _ = c.kill();
        let _ = c.wait();
    }

    // ── 3. The sweep classifies by the version INSIDE, keeps the previous one ──
    //
    // Both fixtures carry the SAME real version, so "older than me" is asked
    // from a build that claims to be newer than both: the sweep's parameter is
    // the running version, and that is exactly what a fresh build would pass.
    let kept = bundles.sweep_leftovers(&parent, "99.0.0", false).expect("sweep");
    checks.is(
        "a sweep before the new app is healthy KEEPS the previous directory",
        kept.iter().any(|l| !l.deleted && l.version.as_deref() == Some(real_version.as_str()))
            && receipt.previous.is_dir(),
        &format!("{kept:?}"),
    );
    checks.is(
        "the sweep read the version out of the executable rather than trusting a marker",
        kept.iter().all(|l| l.version.is_some()),
        &format!("{kept:?}"),
    );
    let swept = bundles.sweep_leftovers(&parent, "99.0.0", true).expect("sweep");
    checks.is(
        "once the new app is healthy the previous directory is removed",
        swept.iter().all(|l| l.deleted) && !receipt.previous.exists(),
        &format!("{swept:?}"),
    );
    // A leftover NEWER than (or equal to) me is an interrupted apply, not a
    // previous build: it goes even when the previous one would be kept.
    let orphan = parent.join(".rexenv-update-999999");
    std::fs::create_dir_all(orphan.join("rexenv")).unwrap();
    std::fs::copy(&real_exe, orphan.join("rexenv").join("rexenv.exe")).unwrap();
    let mixed = bundles.sweep_leftovers(&parent, "0.0.1", false).expect("sweep");
    checks.is(
        "a leftover newer than the running build is deleted even before the app is healthy",
        mixed.iter().any(|l| l.deleted) && !orphan.exists(),
        &format!("{mixed:?}"),
    );
    checks.is(
        "and the installed directory is still the new one afterwards",
        std::fs::read_to_string(installed.join("marker.txt")).as_deref().ok() == Some("v2"),
        "the sweep removed the wrong thing",
    );

    println!(
        "\nNOT covered here: the swap against the REAL %LOCALAPPDATA%\\rexenv (no installed\n\
         copy exists on any machine yet), the relaunch, and the uninstall entry's\n\
         DisplayVersion rewrite (only rewritten when the entry exists, which a fixture's\n\
         never does) — SMOKE-TEST's Windows section once an installer has been installed."
    );
    checks.verdict()
}
