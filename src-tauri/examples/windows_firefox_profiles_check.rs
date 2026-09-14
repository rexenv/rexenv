//! W5, ledger #612: rexenv finds this user's Firefox profiles on Windows, and the `user.js` it writes
//! there is one Firefox itself reads.
//!
//! ```text
//! scripts/probes/windows-example.sh <host> windows_firefox_profiles_check
//! ```
//!
//! 1. The REAL root, read only: `CertTrustManager::firefox_profiles_root` answers
//!    `%APPDATA%\Mozilla\Firefox`, and `core::firefox::status` counts its profiles. Nothing there is
//!    written — `profiles.ini`, `installs.ini` and every profile's `user.js` are hashed before and
//!    after, and must not change.
//! 2. A fixture shaped like it: the real `profiles.ini` copied verbatim into
//!    `%TEMP%\rexenv firefox check\Mozilla\Firefox` (a SPACE in it), the profile folders it names
//!    created; `enable_in_profiles` writes each once, a re-run writes none, `status` counts them forced.
//! 3. Firefox reads it: the installed `firefox.exe`, headless, runs once on the fixture profile and
//!    once on a CONTROL profile with no `user.js`; at its normal shutdown Firefox saves every
//!    non-default pref to `prefs.js`. The pref must be saved `true` from ours, and absent from the
//!    control — so it came from rexenv's line, not from this Firefox's default.
//!
//! Fixture-owned: the fixture tree is removed at the end; a Firefox this check started is killed on
//! drop, panic path included. `demo` tier: Windows-only, needs Firefox installed; on macOS it prints
//! a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_firefox_profiles_check: skipped — a Windows check (ledger #612, W5)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use rexenv_lib::core::firefox;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode};
    use std::time::{Duration, Instant};

    const PREF_TRUE: &str = r#"user_pref("security.enterprise_roots.enabled", true);"#;

    /// A Firefox this check started, killed on drop.
    struct FirefoxRun(Child);

    impl Drop for FirefoxRun {
        fn drop(&mut self) {
            if let Ok(None) = self.0.try_wait() {
                println!("  · teardown: killing Firefox pid {}", self.0.id());
                let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &self.0.id().to_string()]).output();
                let _ = self.0.wait();
            }
        }
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_firefox_profiles_check");
        let fixture = std::env::temp_dir().join("rexenv firefox check");
        let _ = std::fs::remove_dir_all(&fixture);
        run(&mut check, &fixture);
        if let Err(e) = std::fs::remove_dir_all(&fixture) {
            println!("  · fixture not fully removed: {e}");
        }
        check.verdict()
    }

    /// A content hash of every file rexenv could touch under the real root.
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<u64>> {
        use std::hash::{Hash, Hasher};
        let mut files = vec![root.join("profiles.ini"), root.join("installs.ini")];
        files.extend(firefox::profiles(root).into_iter().map(|p| p.join("user.js")));
        files
            .into_iter()
            .map(|f| {
                let h = std::fs::read(&f).ok().map(|bytes| {
                    let mut s = std::collections::hash_map::DefaultHasher::new();
                    bytes.hash(&mut s);
                    s.finish()
                });
                (f, h)
            })
            .collect()
    }

    fn run(check: &mut Check, fixture: &Path) -> Option<()> {
        // ── 1. The real root, read only. ──
        let plat = rexenv_lib::platform::current();
        let real = plat.cert_trust().firefox_profiles_root();
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        let expected = Path::new(&appdata).join("Mozilla").join("Firefox");
        println!("  · firefox_profiles_root: {real:?}");
        check.is(
            "firefox_profiles_root answers %APPDATA%\\Mozilla\\Firefox",
            real.as_ref().is_some_and(|r| r.to_string_lossy().eq_ignore_ascii_case(&expected.to_string_lossy())),
            &format!("expected {}", expected.display()),
        );
        let real = real?;
        let before = snapshot(&real);
        let status = firefox::status(Some(&real));
        println!("  · real status: installed {}, profiles {}, forced {}", status.installed, status.profiles, status.forced);
        let real_profiles = firefox::profiles(&real);
        for p in &real_profiles {
            println!("  · real profile: {}", p.display());
        }
        check.is("status finds this user's Firefox and at least one profile", status.installed && status.profiles >= 1, "");
        check.is(
            "every real profile resolves to a folder under the root (IsRelative, forward slashes)",
            !real_profiles.is_empty() && real_profiles.iter().all(|p| p.starts_with(&real) && p.is_dir()),
            &format!("{real_profiles:?}"),
        );

        // ── 2. The fixture, shaped like the real root. ──
        let root = fixture.join("Mozilla").join("Firefox");
        std::fs::create_dir_all(&root).ok()?;
        // Copied as BYTES: the encoding is part of the shape (the Dell's is UTF-16LE).
        let bytes = std::fs::read(real.join("profiles.ini")).ok()?;
        std::fs::write(root.join("profiles.ini"), &bytes).ok()?;
        let encoding = match bytes.as_slice() {
            [0xFF, 0xFE, ..] => "UTF-16LE with BOM",
            [0xFE, 0xFF, ..] => "UTF-16BE with BOM",
            [0xEF, 0xBB, 0xBF, ..] => "UTF-8 with BOM",
            _ => "no BOM",
        };
        let ini = firefox::ini_text(&bytes);
        check.is("firefox::ini_text reads the real profiles.ini", ini.is_some(), encoding);
        let ini = ini?;
        println!("  · profiles.ini: {encoding}, CRLF line endings: {}", ini.contains("\r\n"));
        for line in ini.lines() {
            if let Some(p) = line.trim().strip_prefix("Path=") {
                std::fs::create_dir_all(root.join(p)).ok()?;
            }
        }
        let profiles = firefox::profiles(&root);
        check.is("the fixture yields as many profiles as the real root", profiles.len() == real_profiles.len(), &format!("{profiles:?}"));
        let written = firefox::enable_in_profiles(&root);
        check.is("enable_in_profiles writes every fixture profile once", matches!(written, Ok(n) if n == profiles.len()), &format!("{written:?}"));
        let again = firefox::enable_in_profiles(&root);
        check.is("a second run writes none", matches!(again, Ok(0)), &format!("{again:?}"));
        let fixture_status = firefox::status(Some(&root));
        check.is("status counts every fixture profile forced", fixture_status.forced == profiles.len(), &format!("{fixture_status:?}"));

        // ── 3. Firefox reads the user.js. ──
        let exe = [
            std::env::var("ProgramFiles").ok(),
            std::env::var("ProgramFiles(x86)").ok(),
            std::env::var("LOCALAPPDATA").ok(),
        ]
        .into_iter()
        .flatten()
        .map(|base| Path::new(&base).join("Mozilla Firefox").join("firefox.exe"))
        .find(|p| p.is_file());
        let Some(exe) = exe else {
            check.is("firefox.exe is installed", false, "not under Program Files or LocalAppData");
            return None;
        };
        println!("  · firefox.exe: {}", exe.display());
        let ours = profiles.first()?.clone();
        let control = fixture.join("control profile");
        std::fs::create_dir_all(&control).ok()?;
        let ours_prefs = headless_run(&exe, &ours, fixture);
        let control_prefs = headless_run(&exe, &control, fixture);
        let saved = |prefs: &Option<String>| prefs.as_deref().map(|p| p.lines().any(|l| l.trim() == PREF_TRUE));
        println!("  · prefs.js written: ours {}, control {}", ours_prefs.is_some(), control_prefs.is_some());
        check.is("Firefox saved the pref true in the profile rexenv wrote", saved(&ours_prefs) == Some(true), &format!("{ours_prefs:?}"));
        check.is(
            "and not in the control profile — it came from rexenv's user.js, not Firefox's default",
            saved(&control_prefs) == Some(false),
            &format!("{:?}", control_prefs.as_deref().map(|p| p.lines().filter(|l| l.contains("enterprise_roots")).collect::<Vec<_>>())),
        );

        // ── The real root untouched. ──
        let after = snapshot(&real);
        check.is("nothing under the real Firefox root changed", before == after, &format!("before {before:?}\nafter {after:?}"));
        Some(())
    }

    /// Run Firefox headless once on `profile` until it exits by itself (a screenshot of about:blank),
    /// then read the `prefs.js` its shutdown wrote.
    fn headless_run(exe: &Path, profile: &Path, fixture: &Path) -> Option<String> {
        let shot = fixture.join(format!("{}.png", profile.file_name()?.to_string_lossy()));
        let child = Command::new(exe)
            .args(["-headless", "-no-remote", "-wait-for-browser", "-profile"])
            .arg(profile)
            .arg("-screenshot")
            .arg(&shot)
            .arg("about:blank")
            .spawn();
        let mut run = match child {
            Ok(c) => FirefoxRun(c),
            Err(e) => {
                println!("  · firefox did not start: {e}");
                return None;
            }
        };
        let deadline = Instant::now() + Duration::from_secs(90);
        let t = Instant::now();
        while Instant::now() < deadline {
            if let Ok(Some(status)) = run.0.try_wait() {
                println!("  · firefox on {} exited {status} after {:.1} s", profile.display(), t.elapsed().as_secs_f64());
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        drop(run);
        std::fs::read_to_string(profile.join("prefs.js")).ok()
    }
}
