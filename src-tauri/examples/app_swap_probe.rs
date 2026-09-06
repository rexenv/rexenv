//! T0 of `docs/PLAN-self-update.md` — **a measurement, not a test.**
//!
//! # The question, and why it cannot be answered by reading
//!
//! Self-update replaces `/Applications/rexenv.app` with a verified copy of itself.
//! macOS App Management (Ventura and later) blocks an app bundle from being modified
//! by anything that is not signed by the same Team ID — but rexenv is **ad-hoc signed**
//! and has no Team ID at all, and Apple documents the rule only for signed apps with
//! one. No Apple, Eclectic Light, Sparkle or Tauri source found in the research pass
//! says what happens when an ad-hoc bundle renames ITSELF. Rust also maps EACCES and
//! EPERM to the same `PermissionDenied`, so an App-Management refusal and an ordinary
//! permission problem are indistinguishable from the error kind alone — which is one
//! reason the plugin's own updater turns a policy refusal into a root `rm -rf`.
//!
//! So this probe measures, on a throwaway bundle, what the plan's error handling has to
//! be designed against. `docs/PLAN-self-update.md` §6.5 lists seven outcomes (O1–O7) and
//! what each one changes; the run's job is to say which letter is true on this Mac.
//!
//! # Run it through the script, never by hand
//!
//! ```text
//! scripts/probes/app-swap-probe.sh              # unquarantined leg
//! scripts/probes/app-swap-probe.sh --quarantine # simulate a browser download
//! ```
//!
//! The script builds `/Applications/RexSwapProbe.app` around this binary, ad-hoc signs
//! it, launches it through LaunchServices (which is what a Finder double-click does),
//! captures the TCC log alongside, and cleans up. Running this example directly does
//! nothing but print that instruction: the measurement is only meaningful from inside a
//! real, launched `.app`.
//!
//! # Fixture ownership — the rule this file must not break
//!
//! Examples run against real app data and a real machine, so anything an example writes,
//! renames or deletes must be fixture-owned (`examples/common/mod.rs`). This probe
//! deliberately writes into `/Applications`, which is exactly where that rule matters
//! most, so every destructive call goes through [`fixture_path`], which refuses any path
//! that is not `/Applications/RexSwapProbe.app` or `/Applications/.rexswapprobe-*`.
//! A path containing `rexenv` aborts the process. The real bundle is never touched, and
//! this example is tier `demo` in `scripts/live-checks.sh` — never run in bulk.
//!
//! # What it does, in order (each step records its errno)
//!
//! 1. Who am I: exe path, bundle version, translocation, quarantine, owner.
//! 2. `mkdir` a staging dir in `/Applications` — the writability probe the real
//!    pre-flight uses (a failure here is `docs/PLAN-self-update.md` R5/O7).
//! 3. Build a v2 copy of the bundle in staging (copy, bump the plist, re-ad-hoc-sign).
//! 4. **rename-aside + rename-in**, then restore — the two-syscall shape.
//! 5. **`renamex_np(RENAME_SWAP)`** — the atomic shape the plan prefers. Left swapped
//!    on success, so the relaunch leg proves the new copy is what runs.
//! 6. If BOTH rename shapes are refused: **delete-then-create**, the Munki/Mysk shape,
//!    with the v1 copy kept as the restore source. This is the O3 branch and the only
//!    step that can leave the fixture without a bundle — it restores on failure.
//! 7. In-place writes into the installed bundle (open the executable for writing; create
//!    a file under `Contents/`) — the contrast case, expected to be refused.
//! 8. Spawn a detached waiter that opens the bundle once THIS pid is gone, then exit.
//!    That is the plan's relaunch shape (§6.2), and it is a measurement too: whether
//!    `open` after the parent exits starts the swapped copy.
//!
//! Run 2 (`relaunched`) appends which version actually started and whether it carries a
//! quarantine attribute. Everything is appended to the log file the script passes, since
//! a LaunchServices-launched app has nowhere to print.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("app_swap_probe measures macOS App Management behaviour; it is macOS-only.");
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    macos::run()
}

#[cfg(target_os = "macos")]
mod macos {
    use std::fmt::Write as _;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode};

    /// The ONLY bundle this probe may touch.
    const APP: &str = "/Applications/RexSwapProbe.app";
    /// The ONLY other prefix it may create or remove under.
    const STAGE_PREFIX: &str = "/Applications/.rexswapprobe-stage-";
    /// The version baked into the installed copy by the script.
    const V1: &str = "1.0.0";
    /// The version given to the staged copy, so a relaunch can say which one ran.
    const V2: &str = "2.0.0";

    /// Refuse any path outside the fixture. Returns the path so calls read as
    /// `remove(fixture_path(&p)?)` and cannot forget the check.
    ///
    /// This is the whole safety argument of a probe that writes into `/Applications`:
    /// a derived path (a `parent()` one level too far, a glob that matched more than it
    /// meant) is how an example once deleted a real Sites folder.
    fn fixture_path(p: &Path) -> std::io::Result<&Path> {
        let s = p.to_string_lossy();
        if s.to_lowercase().contains("rexenv.app") || s.to_lowercase().contains("/rexenv/") {
            eprintln!("app_swap_probe: refusing to touch a path naming rexenv: {s}");
            std::process::exit(2);
        }
        if s == APP || s.starts_with(&format!("{APP}/")) || s.starts_with(STAGE_PREFIX) {
            return Ok(p);
        }
        Err(std::io::Error::other(format!("path outside the fixture: {s}")))
    }

    /// `errno` as a short, greppable token: `ok`, or `EPERM(1)`.
    fn outcome(r: std::io::Result<()>) -> String {
        match r {
            Ok(()) => "ok".to_string(),
            Err(e) => match e.raw_os_error() {
                Some(n) => format!("{}({n})", errno_name(n)),
                None => format!("err({e})"),
            },
        }
    }

    fn errno_name(n: i32) -> &'static str {
        match n {
            libc::EPERM => "EPERM",
            libc::EACCES => "EACCES",
            libc::EXDEV => "EXDEV",
            libc::EROFS => "EROFS",
            libc::ENOTSUP => "ENOTSUP",
            libc::EINVAL => "EINVAL",
            libc::ENOENT => "ENOENT",
            libc::ENOTEMPTY => "ENOTEMPTY",
            libc::EBUSY => "EBUSY",
            libc::ETXTBSY => "ETXTBSY",
            _ => "errno",
        }
    }

    /// `renamex_np(from, to, RENAME_SWAP)` — the atomic directory swap the plan wants.
    /// APFS supports it; a filesystem that does not answers ENOTSUP.
    fn rename_swap(from: &Path, to: &Path) -> std::io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        fixture_path(from)?;
        fixture_path(to)?;
        let a = CString::new(from.as_os_str().as_bytes())?;
        let b = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: two valid NUL-terminated paths, a documented flag, return checked.
        let rc = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    fn rename_checked(from: &Path, to: &Path) -> std::io::Result<()> {
        fixture_path(from)?;
        fixture_path(to)?;
        std::fs::rename(from, to)
    }

    fn remove_tree(p: &Path) -> std::io::Result<()> {
        fixture_path(p)?;
        std::fs::remove_dir_all(p)
    }

    /// A log line that both a human and the script can read. `RESULT k=v` lines are what
    /// the script's summary greps for.
    struct Log {
        path: PathBuf,
        buf: String,
    }

    impl Log {
        fn new(path: PathBuf) -> Self {
            Self { path, buf: String::new() }
        }
        fn line(&mut self, s: impl AsRef<str>) {
            let _ = writeln!(self.buf, "{}", s.as_ref());
            self.flush();
        }
        fn result(&mut self, key: &str, value: impl AsRef<str>) {
            let _ = writeln!(self.buf, "RESULT {key}={}", value.as_ref());
            self.flush();
        }
        fn flush(&mut self) {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
                let _ = f.write_all(self.buf.as_bytes());
            }
            print!("{}", self.buf);
            self.buf.clear();
        }
    }

    fn plist_version(bundle: &Path) -> String {
        let out = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleShortVersionString"])
            .arg(bundle.join("Contents/Info.plist"))
            .output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => "unknown".to_string(),
        }
    }

    fn quarantine(path: &Path) -> String {
        let out = Command::new("/usr/bin/xattr")
            .args(["-p", "com.apple.quarantine"])
            .arg(path)
            .output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => "none".to_string(),
        }
    }

    fn codesign_ok(path: &Path) -> bool {
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn run() -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        let mode = args.get(1).map(String::as_str).unwrap_or("");
        let log_path = args.get(2).map(PathBuf::from);

        let Some(log_path) = log_path else {
            eprintln!(
                "app_swap_probe is driven by its script — it measures nothing outside a launched .app.\n\n    \
                 scripts/probes/app-swap-probe.sh              # unquarantined leg\n    \
                 scripts/probes/app-swap-probe.sh --quarantine # simulate a browser download\n"
            );
            return ExitCode::FAILURE;
        };
        let mut log = Log::new(log_path);

        match mode {
            "measure" => measure(&mut log),
            "relaunched" => relaunched(&mut log),
            other => {
                log.line(format!("app_swap_probe: unknown mode {other:?}"));
                ExitCode::FAILURE
            }
        }
    }

    /// Run 2: the copy that LaunchServices started after the swap. Its only job is to
    /// say which version is now at the path and whether macOS attached a quarantine
    /// attribute to a bundle the probe wrote itself.
    fn relaunched(log: &mut Log) -> ExitCode {
        let exe = std::env::current_exe().unwrap_or_default();
        let bundle = PathBuf::from(APP);
        log.line("--- run 2: relaunched by `open` after the parent exited ---");
        log.result("relaunch_started", "yes");
        log.result("relaunch_version", plist_version(&bundle));
        log.result("relaunch_quarantine", quarantine(&bundle));
        log.result("relaunch_exe", exe.display().to_string());
        log.result("relaunch_translocated", exe.to_string_lossy().contains("/AppTranslocation/").to_string());
        log.line("DONE run=2");
        ExitCode::SUCCESS
    }

    /// Run 1: every measurement, in the order `docs/PLAN-self-update.md` §6.2 performs
    /// them, so a refusal is recorded at the same point the real code would meet it.
    fn measure(log: &mut Log) -> ExitCode {
        let exe = std::env::current_exe().unwrap_or_default();
        let bundle = PathBuf::from(APP);

        log.line("=== app_swap_probe run 1 (measure) ===");
        log.line(format!("when: {}", now()));
        log.result("os", os_version());
        log.result("exe", exe.display().to_string());
        log.result("bundle_version", plist_version(&bundle));
        log.result("bundle_quarantine", quarantine(&bundle));
        log.result("translocated", exe.to_string_lossy().contains("/AppTranslocation/").to_string());
        // SAFETY: getuid cannot fail and takes no arguments.
        log.result("uid", unsafe { libc::getuid() }.to_string());
        log.result("bundle_exists", bundle.exists().to_string());
        log.result("codesign_installed", codesign_ok(&bundle).to_string());
        // The script builds v1; if the installed copy already says v2 a previous run was
        // interrupted mid-swap, and every version comparison below would read backwards.
        log.result("start_state_is_v1", (plist_version(&bundle) == V1).to_string());

        if !exe.starts_with(APP) {
            log.line(
                "REFUSING: this binary is not running from /Applications/RexSwapProbe.app. \
                 The measurement only means something from inside a launched bundle — \
                 run scripts/probes/app-swap-probe.sh.",
            );
            log.line("DONE run=1");
            return ExitCode::FAILURE;
        }
        if exe.to_string_lossy().contains("/AppTranslocation/") {
            // Not a failure of the probe — it is one of the answers. Translocation makes
            // the bundle read-only and is exactly why the real pre-flight refuses.
            log.line(
                "NOTE: macOS translocated this launch (read-only copy). The swap cannot be \
                 measured from here; re-run without --quarantine, or move the app in Finder.",
            );
        }

        // ── 2. The staging directory: the first write, and the writability probe ──
        let stage = PathBuf::from(format!("{STAGE_PREFIX}{}", std::process::id()));
        let mk = fixture_path(&stage).and_then(std::fs::create_dir);
        log.result("stage_mkdir", outcome(mk.map(|_| ())));
        if !stage.is_dir() {
            log.line("stopping: no staging directory, so nothing else can be measured safely.");
            log.line("DONE run=1");
            return ExitCode::FAILURE;
        }

        // ── 3. The v2 copy: what a real update would stage ──
        let staged = stage.join("new.app");
        let previous = stage.join("previous.app");
        let cp = Command::new("/bin/cp").arg("-R").arg(&bundle).arg(&staged).status();
        log.result("stage_copy", match cp {
            Ok(s) if s.success() => "ok".into(),
            Ok(s) => format!("cp exit {:?}", s.code()),
            Err(e) => format!("err({e})"),
        });
        let bump = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", &format!("Set :CFBundleShortVersionString {V2}")])
            .arg(staged.join("Contents/Info.plist"))
            .status();
        log.result("stage_bump_version", match bump {
            Ok(s) if s.success() => "ok".into(),
            Ok(s) => format!("PlistBuddy exit {:?}", s.code()),
            Err(e) => format!("err({e})"),
        });
        // Editing the plist breaks the seal, so re-sign — the same ad-hoc identity the
        // real bundle carries, which is the whole point of the probe.
        let sign = Command::new("/usr/bin/codesign").args(["-f", "-s", "-"]).arg(&staged).status();
        log.result("stage_codesign", match sign {
            Ok(s) if s.success() => "ok".into(),
            Ok(s) => format!("codesign exit {:?}", s.code()),
            Err(e) => format!("err({e})"),
        });
        log.result("stage_codesign_verify", codesign_ok(&staged).to_string());
        log.result("stage_version", plist_version(&staged));

        // ── 4. The two-syscall shape: rename aside, rename in, then put it all back ──
        let aside = rename_checked(&bundle, &previous);
        log.result("rename_aside", outcome(aside.as_ref().map(|_| ()).map_err(clone_err)));
        let mut installed_is_v2 = false;
        if aside.is_ok() {
            let inward = rename_checked(&staged, &bundle);
            log.result("rename_in", outcome(inward.as_ref().map(|_| ()).map_err(clone_err)));
            if inward.is_ok() {
                log.result("rename_pair_installed_version", plist_version(&bundle));
                // Restore, so the atomic shape below is measured from the same start
                // state — the pair is a measurement here, not the shape being adopted.
                let back_out = rename_checked(&bundle, &staged);
                log.result("rename_restore_out", outcome(back_out.map(|_| ())));
                let back_in = rename_checked(&previous, &bundle);
                log.result("rename_restore_in", outcome(back_in.map(|_| ())));
            } else {
                // The second rename failed: put the original back where it belongs.
                let back = rename_checked(&previous, &bundle);
                log.result("rename_restore_after_failure", outcome(back.map(|_| ())));
            }
        }
        log.result("installed_version_after_step4", plist_version(&bundle));

        // ── 5. The atomic shape the plan prefers ──
        let swap = rename_swap(&bundle, &staged);
        log.result("renamex_np_swap", outcome(swap.as_ref().map(|_| ()).map_err(clone_err)));
        if swap.is_ok() {
            installed_is_v2 = true;
            log.result("swap_installed_version", plist_version(&bundle));
            log.result("swap_staged_version", plist_version(&staged));
            log.result("swap_installed_codesign", codesign_ok(&bundle).to_string());
        }

        // ── 6. O3 only: both rename shapes refused → the delete-then-create shape ──
        if !installed_is_v2 {
            log.line(
                "both rename shapes failed — measuring the delete-then-create shape \
                 (Munki/Mysk), which is outcome O3's fallback.",
            );
            let keep = Command::new("/bin/cp").arg("-R").arg(&bundle).arg(&previous).status();
            log.result("o3_backup_copy", match keep {
                Ok(s) if s.success() => "ok".into(),
                Ok(s) => format!("cp exit {:?}", s.code()),
                Err(e) => format!("err({e})"),
            });
            if previous.is_dir() {
                let rm = remove_tree(&bundle);
                log.result("o3_remove_installed", outcome(rm.map(|_| ())));
                if !bundle.exists() {
                    let inward = rename_checked(&staged, &bundle);
                    log.result("o3_rename_in", outcome(inward.as_ref().map(|_| ()).map_err(clone_err)));
                    if inward.is_ok() {
                        installed_is_v2 = true;
                    } else {
                        let back = rename_checked(&previous, &bundle);
                        log.result("o3_restore", outcome(back.map(|_| ())));
                    }
                }
            }
        }

        // ── 7. The contrast case: writing INSIDE a launched bundle ──
        let exe_write = std::fs::OpenOptions::new().write(true).open(&exe).map(|_| ());
        log.result("inplace_open_executable_for_write", outcome(exe_write));
        let inside = bundle.join("Contents/Resources/inplace-probe.txt");
        let create = fixture_path(&inside).and_then(|p| std::fs::write(p, b"probe\n"));
        log.result("inplace_create_file", outcome(create.map(|_| ())));
        if inside.exists() {
            let _ = fixture_path(&inside).and_then(std::fs::remove_file);
        }

        log.result("installed_version_final", plist_version(&bundle));
        log.result("installed_is_new_copy", installed_is_v2.to_string());

        // ── 8. The relaunch shape: wait for THIS pid to be gone, then `open` ──
        //
        // Not a convenience: `open` while we are still running would activate this
        // process rather than start the swapped copy, which is precisely why the plan's
        // helper waits for NOTE_EXIT before opening. The shell loop is the same idea in
        // its cheapest form.
        if installed_is_v2 {
            let pid = std::process::id();
            let script = format!(
                "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; sleep 0.5; \
                 /usr/bin/open '{APP}' --args relaunched '{log}'",
                log = log.path.display()
            );
            let spawned = Command::new("/bin/sh").arg("-c").arg(script).spawn();
            log.result("relaunch_helper_spawned", match spawned {
                Ok(_) => "ok".into(),
                Err(e) => format!("err({e})"),
            });
        } else {
            log.result("relaunch_helper_spawned", "skipped (nothing was swapped in)");
            log.line(
                "No shape replaced the bundle, so there is nothing to relaunch. That is \
                 outcome O4 unless the errnos above say otherwise.",
            );
        }

        log.line("DONE run=1");
        ExitCode::SUCCESS
    }

    /// `std::io::Error` is not `Clone`; keep the errno, which is all the log wants.
    fn clone_err(e: &std::io::Error) -> std::io::Error {
        match e.raw_os_error() {
            Some(n) => std::io::Error::from_raw_os_error(n),
            None => std::io::Error::other(e.to_string()),
        }
    }

    fn os_version() -> String {
        Command::new("/usr/bin/sw_vers")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_else(|_| "unknown".into())
    }

    fn now() -> String {
        Command::new("/bin/date")
            .arg("+%Y-%m-%dT%H:%M:%S%z")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unknown".into())
    }
}
