//! The Linux self-update's two swaps, on fixtures (docs/PLAN-linux-port.md L7):
//!
//! - **AppImage**: a fixture "image" (a script that answers `--print-version`) staged beside
//!   itself, refused when its version disagrees with the signed one, then exchanged in place
//!   (`renameat2(RENAME_EXCHANGE)`); the previous copy stays in the stage dir and the sweep
//!   keeps it until told otherwise.
//! - **`.deb`**: a fixture package built with `dpkg-deb` under the name `rexenv-swapfixture`
//!   whose files are `usr/bin/rexenv-swapfixture` and `usr/bin/rex-swapfixture` (never the
//!   real names — this runs beside an installed rexenv), refused for a wrong version or a
//!   missing sidecar, then installed with the SAME command the platform's `swap` hands to
//!   `pkexec` — `sudo -n` stands in for the polkit prompt a terminal cannot show — and
//!   removed again.
//!
//! Linux only, `system` tier (root for the package leg). The fixture roots are the sandbox's;
//! the package is the fixture's own name and `dpkg -r` removes it on every exit.
//!
//!   cargo run --example linux_app_swap_check      # on Ubuntu with sudo

#[cfg(target_os = "linux")]
mod common;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("linux_app_swap_check: skipped — a Linux check (docs/PLAN-linux-port.md L7)");
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    use rexenv_lib::core::app_update;
    use rexenv_lib::platform::traits::{InstallKind, StagedExpect, SwapMethod};
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, ExitCode};

    let (plat, _sandbox) = common::sandbox("linuxswap");
    let mut checks = common::Check::new("linux_app_swap_check");
    let bundles = plat.app_bundle();
    let root = plat.paths().app_data_dir().expect("sandbox app data");
    if !root.to_string_lossy().contains("rexenv-sandbox-") {
        eprintln!("refusing: the fixture root is not inside a sandbox root");
        return ExitCode::FAILURE;
    }

    // ── AppImage leg ──────────────────────────────────────────────────────────
    let apps = root.join("Apps");
    std::fs::create_dir_all(&apps).expect("fixture Apps");
    let image = apps.join("Fixture.AppImage");
    let write_image = |at: &std::path::Path, version: &str| {
        std::fs::write(at, format!("#!/bin/sh\n[ \"$1\" = \"--print-version\" ] && echo {version}\n")).expect("fixture image");
        std::fs::set_permissions(at, PermissionsExt::from_mode(0o755)).expect("mode");
    };
    write_image(&image, "1.0.0");
    let download = root.join("download.AppImage");
    write_image(&download, "2.0.0");
    let expect = |v: &str| StagedExpect {
        version: v.into(),
        identifier: "rexenv-swapfixture".into(),
        executable: "rexenv-swapfixture".into(),
        archs: vec![],
        required_binaries: vec!["rex-swapfixture".into()],
        codesign: false,
    };
    // `$APPIMAGE` is how the platform tells an AppImage from a bare binary.
    std::env::set_var("APPIMAGE", &image);
    let facts = bundles.facts(std::path::Path::new("/tmp/.mount_fixture/usr/bin/rexenv")).expect("facts");
    checks.is("an $APPIMAGE install is a PortableFile whose bundle is the image", facts.kind == InstallKind::PortableFile && facts.bundle == image, &format!("{facts:?}"));
    checks.is("the image's folder is writable and owned, so no prompt", facts.parent_writable && facts.owned_by_me && app_update::preflight(&facts, 10).is_ok(), &format!("{facts:?}"));
    let wrong = bundles.stage(&facts, &download, &expect("9.9.9"));
    checks.is("a staged image whose --print-version disagrees is refused", wrong.as_ref().err().is_some_and(|e| e.to_string().contains("2.0.0") && e.to_string().contains("9.9.9")), &format!("{:?}", wrong.as_ref().err().map(|e| e.to_string())));
    checks.is("a refused stage leaves no stage dir", !std::fs::read_dir(&apps).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(".rexenv-update-")), "");
    let staged = bundles.stage(&facts, &download, &expect("2.0.0")).expect("stage v2");
    let receipt = bundles.swap(&image, &staged).expect("swap");
    let now = Command::new(&image).arg("--print-version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    checks.is("after the exchange the install path answers 2.0.0", now == "2.0.0", &now);
    let prev = Command::new(&receipt.previous).arg("--print-version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    checks.is("the previous image is where the receipt says, still 1.0.0", prev == "1.0.0" && receipt.method == SwapMethod::AtomicSwap, &format!("{prev} {:?}", receipt.method));
    let kept = bundles.sweep_leftovers(&apps, "2.0.0", false).expect("sweep");
    checks.is("the sweep keeps the previous copy until told the new build is healthy", kept.iter().any(|l| l.version.as_deref() == Some("1.0.0") && !l.deleted), &format!("{kept:?}"));
    let gone = bundles.sweep_leftovers(&apps, "2.0.0", true).expect("sweep");
    checks.is("…and removes it when told", gone.iter().all(|l| l.deleted) && !staged.stage_dir.exists(), &format!("{gone:?}"));
    std::env::remove_var("APPIMAGE");

    // ── .deb leg ──────────────────────────────────────────────────────────────
    let build = root.join("debbuild");
    let mk_deb = |version: &str, with_rex: bool| -> std::path::PathBuf {
        let tree = build.join(format!("tree-{version}-{with_rex}"));
        let _ = std::fs::remove_dir_all(&tree);
        std::fs::create_dir_all(tree.join("DEBIAN")).unwrap();
        std::fs::create_dir_all(tree.join("usr/bin")).unwrap();
        let arch = String::from_utf8_lossy(&Command::new("dpkg").arg("--print-architecture").output().unwrap().stdout).trim().to_string();
        std::fs::write(tree.join("DEBIAN/control"), format!("Package: rexenv-swapfixture\nVersion: {version}\nArchitecture: {arch}\nMaintainer: rexenv <fixture@rexenv.dev>\nDescription: rexenv swap fixture (safe to remove)\n")).unwrap();
        // Only fixture-named members: the listing check asks for what `expect` names, and the
        // package must never own a path the real rexenv package owns.
        write_image(&tree.join("usr/bin/rexenv-swapfixture"), version);
        if with_rex {
            std::fs::write(tree.join("usr/bin/rex-swapfixture"), "#!/bin/sh\n").unwrap();
        }
        let out = build.join(format!("rexenv-swapfixture_{version}_{}.deb", if with_rex { "full" } else { "norex" }));
        let ok = Command::new("dpkg-deb").args(["--root-owner-group", "-b"]).arg(&tree).arg(&out).output().map(|o| o.status.success()).unwrap_or(false);
        assert!(ok, "dpkg-deb could not build the fixture package");
        out
    };
    let deb_v2 = mk_deb("2.0.0", true);
    let deb_norex = mk_deb("2.0.0", false);
    let pkg_facts = rexenv_lib::platform::traits::BundleFacts {
        bundle: "/usr/bin/rexenv".into(),
        parent: root.join("updates"),
        kind: InstallKind::SystemPackage,
        homebrew: false,
        parent_writable: true,
        owned_by_me: true,
        read_only: false,
        canonical: true,
        free_parent_bytes: u64::MAX,
    };
    std::fs::create_dir_all(&pkg_facts.parent).unwrap();
    let r = bundles.stage(&pkg_facts, &deb_v2, &expect("9.9.9"));
    checks.is("a staged package whose control Version disagrees is refused", r.as_ref().err().is_some_and(|e| e.to_string().contains("says it is 2.0.0")), &format!("{:?}", r.as_ref().err().map(|e| e.to_string())));
    let r = bundles.stage(&pkg_facts, &deb_norex, &expect("2.0.0"));
    checks.is("a staged package without the rex sidecar is refused", r.as_ref().err().is_some_and(|e| e.to_string().contains("missing usr/bin/rex-swapfixture")), &format!("{:?}", r.as_ref().err().map(|e| e.to_string())));
    let staged_deb = bundles.stage(&pkg_facts, &deb_v2, &expect("2.0.0")).expect("stage deb");
    checks.is("the verified package sits in the user's staging folder", staged_deb.path.starts_with(&pkg_facts.parent) && staged_deb.path.extension().is_some_and(|e| e == "deb"), &staged_deb.path.display().to_string());

    // The real swap would `pkexec` this; a terminal has no agent, so the leg runs the SAME
    // command the platform builds, through sudo, against a package that owns only its own
    // files. It is removed again whatever the verdict.
    let install = Command::new("sudo").args(["-n", "/bin/sh", "-c"]).arg(format!("/usr/bin/dpkg -i '{}'", staged_deb.path.display())).output().expect("sudo");
    checks.is("dpkg -i installs the staged package (the command the polkit step runs)", install.status.success(), &String::from_utf8_lossy(&install.stderr));
    let v = Command::new("/usr/bin/rexenv-swapfixture").arg("--print-version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    checks.is("the installed file answers the new version", v == "2.0.0", &v);
    let removed = Command::new("sudo").args(["-n", "/usr/bin/dpkg", "-r", "rexenv-swapfixture"]).output().map(|o| o.status.success()).unwrap_or(false);
    checks.is("the fixture package is removed again", removed && !std::path::Path::new("/usr/bin/rexenv-swapfixture").exists(), "");
    let real = Command::new("dpkg").args(["-s", "rexenv"]).output().map(|o| o.status.success()).unwrap_or(false);
    checks.is("the real rexenv package, where installed, is untouched", !real || std::path::Path::new("/usr/bin/rexenv").exists(), "");
    let swept = bundles.sweep_leftovers(&pkg_facts.parent, "2.0.0", true).expect("sweep");
    checks.is("the staged package is swept once the new build is healthy", swept.iter().any(|l| l.version.as_deref() == Some("2.0.0") && l.deleted), &format!("{swept:?}"));
    checks.verdict()
}
