//! Live check: stage, verify and SWAP a real app bundle — on fixtures.
//! Run: `cargo run --example app_bundle_swap_check`  (tier: sandbox)
//!
//! # What this proves that L0 cannot
//!
//! `core::app_update`'s tests decide what the facts MEAN; none of them touches a
//! filesystem. What is unproven until something does this is the half the swap
//! actually performs:
//!
//!   a real `.app` on disk → tar it → extract it through the guarded extractor →
//!   read its `Info.plist` → check both Mach-Os → `renamex_np(RENAME_SWAP)` →
//!   the install path now holds the NEW bundle and the staging path the old one
//!   → sweep classifies them by the version INSIDE them → the previous one
//!   survives until it is told the new app is healthy.
//!
//! T0 measured that macOS PERMITS this on a real ad-hoc bundle in
//! `/Applications` (`docs/archive/PLAN-self-update.md` §T0). This is the other half:
//! that rexenv's own code does it correctly, and — the part that matters more —
//! that every failure leaves the installed bundle exactly as it was.
//!
//! # Fixture-owned, and that is the whole safety argument
//!
//! Everything happens inside `common::sandbox`'s root: a fixture "Applications"
//! directory, two fixture bundles with their own identifier, and an archive this
//! example built. **The real `/Applications/rexenv.app` is never read, never
//! renamed and never removed** — the swap is called with fixture paths, and a
//! guard below refuses to run at all if the sandbox root is not where it should
//! be. The fixture bundles are unsigned and single-arch, which is exactly why
//! `StagedExpect` takes its expectations as parameters rather than reading them
//! from a constant.

use rexenv_lib::core::{app_update, macho};
use rexenv_lib::platform::traits::{InstallKind, StagedExpect};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod common;

/// Build a minimal but REAL `.app`: an Info.plist with the four keys the
/// verifier reads, and a Mach-O executable (a copy of a system binary, so the
/// architecture check has something true to read).
fn make_bundle(at: &Path, version: &str, exe_name: &str, with_sidecar: bool) -> std::io::Result<()> {
    let macos = at.join("Contents/MacOS");
    std::fs::create_dir_all(&macos)?;
    std::fs::write(
        at.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key><string>dev.rexenv.swapfixture</string>
	<key>CFBundleExecutable</key><string>{exe_name}</string>
	<key>CFBundleShortVersionString</key><string>{version}</string>
	<key>CFBundleVersion</key><string>{version}</string>
</dict>
</plist>
"#
        ),
    )?;
    std::fs::copy("/bin/echo", macos.join(exe_name))?;
    if with_sidecar {
        std::fs::copy("/bin/echo", macos.join("rexfix"))?;
    }
    Ok(())
}

/// Tar a bundle the way the release script will: no AppleDouble entries, and
/// exactly one top-level `<name>.app/` member.
fn tar_bundle(parent: &Path, name: &str, out: &Path) -> bool {
    Command::new("/usr/bin/tar")
        .env("COPYFILE_DISABLE", "1")
        .arg("-C")
        .arg(parent)
        .args(["-czf"])
        .arg(out)
        .arg(name)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn version_of(app: &Path) -> Option<String> {
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let at = plist.find("<key>CFBundleShortVersionString</key>")?;
    let rest = &plist[at..];
    let open = rest.find("<string>")? + 8;
    let close = rest[open..].find("</string>")?;
    Some(rest[open..open + close].to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _sandbox) = common::sandbox("appswap");
    let mut checks = common::Check::new("app_bundle_swap_check");
    let bundles = plat.app_bundle();

    let root = plat.paths().app_data_dir().expect("sandbox app data");
    // A fixture "Applications" folder. Every path below is under it, and the
    // guard makes that a fact rather than an intention.
    let apps = root.join("Applications");
    if !apps.to_string_lossy().contains("rexenv-sandbox-") {
        eprintln!("refusing: the fixture parent is not inside a sandbox root");
        return ExitCode::FAILURE;
    }
    std::fs::create_dir_all(&apps).expect("fixture Applications");

    let installed = apps.join("Fixture.app");
    let build = root.join("build");
    std::fs::create_dir_all(&build).expect("build dir");

    make_bundle(&installed, "1.0.0", "fixture", true).expect("v1 bundle");
    make_bundle(&build.join("Fixture.app"), "2.0.0", "fixture", true).expect("v2 bundle");
    let archive = root.join("fixture-2.0.0.app.tar.gz");
    if !tar_bundle(&build, "Fixture.app", &archive) {
        eprintln!("could not build the fixture archive");
        return ExitCode::FAILURE;
    }

    // The fixture executable is whatever `/bin/echo` is on this Mac; the
    // expectation is read from it rather than assumed, which is the point of
    // `StagedExpect` taking parameters.
    let fixture_archs = macho::archs(&installed.join("Contents/MacOS/fixture")).unwrap_or_default();
    let expect = |version: &str| StagedExpect {
        version: version.into(),
        identifier: "dev.rexenv.swapfixture".into(),
        executable: "fixture".into(),
        archs: fixture_archs.clone(),
        required_binaries: vec!["rexfix".into()],
        codesign: false, // the fixtures were never signed; the real one asserts it
    };

    // ── 1. Facts about a bundle that is NOT in an Applications folder ────────
    let exe = installed.join("Contents/MacOS/fixture");
    let facts = bundles.facts(&exe).expect("facts");
    checks.is(
        "a bundle outside /Applications is seen as Elsewhere, and refused before any download",
        facts.kind == InstallKind::Elsewhere
            && matches!(app_update::preflight(&facts, 1), Err(app_update::Refusal::NotInApplications { .. })),
        &format!("{:?}", facts.kind),
    );
    checks.is(
        "the facts name the bundle and its parent, not the executable",
        facts.bundle == installed && facts.parent == apps,
        &format!("{:?} / {:?}", facts.bundle, facts.parent),
    );
    checks.is(
        "the fixture volume is writable and owned by this user, so nothing here needs a prompt",
        facts.parent_writable && facts.owned_by_me && !facts.read_only,
        &format!("{facts:?}"),
    );

    // ── 2. A staged tree that is NOT what was promised leaves v1 alone ───────
    //
    // The most important check in this file: the installed bundle must survive
    // every failure path byte for byte.
    let before = std::fs::read(installed.join("Contents/Info.plist")).expect("read v1");
    let wrong_version = bundles.stage(&facts, &archive, &expect("9.9.9"));
    checks.is(
        "a staged bundle whose Info.plist disagrees with the signed version is refused",
        wrong_version.is_err(),
        "a stale or swapped archive would have been installed",
    );
    if let Err(e) = &wrong_version {
        let m = e.to_string();
        checks.is(
            "the refusal names what the download actually said",
            m.contains("2.0.0") && m.contains("9.9.9"),
            &m,
        );
    }
    let mut missing = expect("2.0.0");
    missing.required_binaries = vec!["rex".into()];
    checks.is(
        "a staged bundle missing a required binary is refused",
        bundles.stage(&facts, &archive, &missing).is_err(),
        "a bundle without the rex sidecar would silently break every open terminal",
    );
    // The slice check, on a bundle that really is thin. `/bin/echo` is universal
    // on this Mac, so the fixture is thinned deliberately rather than the leg
    // being skipped: "installing a thin build" is the failure that leaves half
    // the userbase unable to launch rexenv at all, and a check that only runs on
    // some machines is a check nobody can rely on.
    let thin_dir = root.join("thin");
    std::fs::create_dir_all(&thin_dir).expect("thin dir");
    make_bundle(&thin_dir.join("Fixture.app"), "2.0.0", "fixture", true).expect("thin bundle");
    // Ask lipo what it calls the slices rather than guessing: `/bin/echo` on this
    // Mac is `x86_64 arm64e`, and thinning to "arm64" fails on a file that has
    // arm64e. The name has to come from the tool that will be given it.
    let slice = Command::new("/usr/bin/lipo")
        .args(["-archs"])
        .arg(installed.join("Contents/MacOS/fixture"))
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().unwrap_or("").to_string())
        .unwrap_or_default();
    let thinned = !slice.is_empty()
        && Command::new("/usr/bin/lipo")
            .arg(installed.join("Contents/MacOS/fixture"))
            .args(["-thin", &slice, "-output"])
            .arg(thin_dir.join("Fixture.app/Contents/MacOS/fixture"))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    let thin_archive = root.join("fixture-thin.app.tar.gz");
    if thinned && tar_bundle(&thin_dir, "Fixture.app", &thin_archive) {
        let mut both = expect("2.0.0");
        both.archs = vec![
            rexenv_lib::platform::traits::Arch::Arm64,
            rexenv_lib::platform::traits::Arch::X86_64,
        ];
        let refused = bundles.stage(&facts, &thin_archive, &both);
        checks.is(
            "a thin bundle is refused when both architectures are required",
            refused.is_err(),
            "installing a thin build leaves half the userbase unable to launch",
        );
        if let Err(e) = &refused {
            checks.is(
                "and the refusal says which architecture is missing",
                e.to_string().contains("missing"),
                &e.to_string(),
            );
        }
        // Anti-vacuity: the SAME thin bundle stages fine when only the
        // architecture it actually has is required, so the refusal above came
        // from the slice check and not from something else about the fixture.
        let mut one = expect("2.0.0");
        one.archs = macho::archs(&thin_dir.join("Fixture.app/Contents/MacOS/fixture"))
            .unwrap_or_default();
        let ok = bundles.stage(&facts, &thin_archive, &one);
        checks.is(
            "the same bundle stages when only the architecture it has is required",
            ok.is_ok(),
            "the thin leg above may be passing for the wrong reason",
        );
        // That one SUCCEEDED, so it legitimately left a staging directory
        // behind — remove it here so the "a failure cleans up after itself"
        // check below is measuring failures and nothing else.
        if let Ok(s) = ok {
            let _ = std::fs::remove_dir_all(&s.stage_dir);
        }
    } else {
        checks.is("the thin fixture could be built", false, "lipo or tar failed");
    }
    checks.is(
        "every refusal above left the installed bundle byte-identical",
        std::fs::read(installed.join("Contents/Info.plist")).ok().as_deref() == Some(&before[..]),
        "the installed bundle changed while nothing was supposed to happen",
    );
    let strays: Vec<PathBuf> = std::fs::read_dir(&apps)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".rexenv-update-")))
        .collect();
    checks.is(
        "a failed stage removes its own staging directory",
        strays.is_empty(),
        &format!("left behind: {strays:?}"),
    );

    // ── 3. The real thing: stage, verify, swap ───────────────────────────────
    let staged = match bundles.stage(&facts, &archive, &expect("2.0.0")) {
        Ok(s) => s,
        Err(e) => {
            checks.is("the correct archive stages", false, &e.to_string());
            return checks.verdict();
        }
    };
    checks.is(
        "the staged bundle is a sibling of the installed one, so a cross-device rename is impossible",
        staged.path.parent().and_then(|p| p.parent()) == Some(apps.as_path()),
        &format!("{:?}", staged.path),
    );
    checks.is(
        "the staging directory is dot-prefixed, so Finder and Spotlight skip it",
        staged
            .stage_dir
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.')),
        &format!("{:?}", staged.stage_dir),
    );

    let receipt = match bundles.swap(&installed, &staged) {
        Ok(r) => r,
        Err(e) => {
            checks.is("the swap succeeds on a fixture bundle", false, &e.to_string());
            return checks.verdict();
        }
    };
    checks.is(
        "after the swap the install path holds the NEW version",
        version_of(&installed).as_deref() == Some("2.0.0"),
        &format!("{:?}", version_of(&installed)),
    );
    checks.is(
        "the previous bundle survives, and the receipt says where",
        receipt.previous.is_dir() && version_of(&receipt.previous).as_deref() == Some("1.0.0"),
        &format!("{:?} → {:?}", receipt.previous, version_of(&receipt.previous)),
    );
    println!("swap method: {:?}", receipt.method);

    // ── 4. The sweep classifies by VERSION, and keeps the previous one ───────
    let kept = bundles.sweep_leftovers(&apps, "2.0.0", false).expect("sweep");
    checks.is(
        "a sweep before the new app is healthy KEEPS the previous bundle",
        kept.iter().any(|l| !l.deleted && l.version.as_deref() == Some("1.0.0"))
            && receipt.previous.is_dir(),
        &format!("{kept:?}"),
    );
    checks.is(
        "the sweep read the version out of the bundle rather than trusting a marker",
        kept.iter().all(|l| l.version.is_some()),
        &format!("{kept:?}"),
    );
    let swept = bundles.sweep_leftovers(&apps, "2.0.0", true).expect("sweep");
    checks.is(
        "once the new app is healthy the previous bundle is removed",
        swept.iter().all(|l| l.deleted) && !receipt.previous.exists(),
        &format!("{swept:?}"),
    );
    checks.is(
        "and the installed bundle is still the new one afterwards",
        version_of(&installed).as_deref() == Some("2.0.0"),
        "the sweep removed the wrong thing",
    );

    // A leftover NEWER than me is an interrupted apply, not a previous bundle:
    // it goes even when the previous one would be kept.
    let orphan = apps.join(".rexenv-update-999999");
    std::fs::create_dir_all(&orphan).unwrap();
    make_bundle(&orphan.join("Fixture.app"), "3.0.0", "fixture", true).unwrap();
    let mixed = bundles.sweep_leftovers(&apps, "2.0.0", false).expect("sweep");
    checks.is(
        "a leftover newer than the running build is deleted even before the app is healthy",
        mixed.iter().any(|l| l.version.as_deref() == Some("3.0.0") && l.deleted)
            && !orphan.exists(),
        &format!("{mixed:?}"),
    );

    println!(
        "\nNOT covered here: the swap against the REAL /Applications bundle under App\n\
         Management (measured once by the T0 probe, re-measured per macOS major), the\n\
         relaunch (T4), and Gatekeeper on the replaced bundle — SMOKE-TEST §In-app\n\
         self-update and PUBLISH-TESTING §M."
    );
    checks.verdict()
}
