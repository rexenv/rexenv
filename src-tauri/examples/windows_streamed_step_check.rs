//! W4: Composer through the site's PHP on Windows — the streamed step and the user's environment
//! it runs with (ledger #609, owner rulings 14 Sep 2026).
//!
//! ```text
//! scripts/probes/windows-example.sh dell@<host> windows_streamed_step_check
//! ```
//!
//! What it proves there:
//!
//! 1. `login_shell_env` reads the registry FRESH: a variable written to `HKCU\Environment` after
//!    this process started is in it (and not in this process's own environment); `SystemRoot`,
//!    `USERPROFILE` and a `Path` with System32 are there.
//! 2. `run_step_streamed` streams a step's lines, and the step sees ONLY the environment it was
//!    handed — a variable set in this process after the snapshot does not reach it.
//! 3. Cancelling a step ends the step AND the grandchild it started (the kill-on-close job, not a
//!    pid), within seconds.
//! 4. The idle limit kills a silent step's whole tree the same way.
//! 5. A process that starts a step and then EXITS without stopping it — rexenv quitting or
//!    crashing — takes the step and its grandchild with it.
//! 6. `laravel::create_project` — `composer create-project laravel/laravel` run by the pinned
//!    Composer phar through the site's `php.exe` — produces an installed Laravel app.
//!
//! Fixture-owned: everything under `%TEMP%\rexenv streamed step check` (a SPACE in it), removed at
//! the end, with Composer's home and cache pointed inside it; the one registry value
//! `HKCU\Environment\REXENV_STEP_PROBE`, deleted on every exit path by a guard. The binary cache is
//! the documented exception. Needs the network (Laravel). `demo` tier: Windows-only; on macOS it
//! prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_streamed_step_check: skipped — a Windows live check (ledger #609)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check};
    use rexenv_lib::core::repo::{run_step_streamed, CancelToken};
    use rexenv_lib::core::{binaries, laravel};
    use std::io::BufRead;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const PROBE_VAR: &str = "REXENV_STEP_PROBE";
    const PROBE_VALUE: &str = "fresh from the registry";
    /// A step that starts a grandchild, says its pid, and waits.
    const GRANDCHILD: &str = "$p = proc_open([PHP_BINARY, '-r', 'sleep(120);'], [], $pipes); \
                              $s = proc_get_status($p); echo 'grandchild=' . $s['pid'] . PHP_EOL; flush(); sleep(120);";

    /// Deletes the fixture's registry value on every exit path.
    struct RegistryVar;
    impl Drop for RegistryVar {
        fn drop(&mut self) {
            let _ = Command::new("reg").args(["delete", r"HKCU\Environment", "/v", PROBE_VAR, "/f"]).output();
        }
    }

    pub async fn main() -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("orphan") {
            return orphan(&args);
        }
        let mut check = Check::new("windows_streamed_step_check");
        let root = std::env::temp_dir().join("rexenv streamed step check");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let plat = common::sandbox_platform_at(root.join("app"));
        let sup = plat.supervisor();

        // ── 1. The environment, read fresh. ──
        let _guard = RegistryVar;
        let written = Command::new("reg")
            .args(["add", r"HKCU\Environment", "/v", PROBE_VAR, "/t", "REG_SZ", "/d", PROBE_VALUE, "/f"])
            .output();
        check.is("the fixture value is written to HKCU\\Environment", written.as_ref().is_ok_and(|o| o.status.success()), &format!("{written:?}"));
        let env = match plat.shell().login_shell_env() {
            Ok(env) => env,
            Err(e) => {
                check.is("login_shell_env answers", false, &e.to_string());
                return finish(check, &root);
            }
        };
        let get = |name: &str| env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
        check.is(
            "a variable written after this process started is in the environment (read fresh)",
            get(PROBE_VAR).as_deref() == Some(PROBE_VALUE) && std::env::var(PROBE_VAR).is_err(),
            &format!("{:?} (own: {:?})", get(PROBE_VAR), std::env::var(PROBE_VAR)),
        );
        check.is("logon-only variables are kept", get("SystemRoot").is_some() && get("USERPROFILE").is_some(), "missing");
        check.is(
            "the Path carries System32",
            get("Path").is_some_and(|p| p.to_ascii_lowercase().contains(r"\system32")),
            &format!("{:?}", get("Path")),
        );

        let php = match binaries::resolve_program(&*plat, "php", binaries::pins().php).await {
            Ok(p) => p,
            Err(e) => {
                check.is("the site's PHP resolves", false, &e.to_string());
                return finish(check, &root);
            }
        };

        // ── 2. Output, and only the handed environment. ──
        std::env::set_var("REXENV_NOT_IN_SNAPSHOT", "leaked"); // after the snapshot was taken
        let mut lines = Vec::new();
        let script = format!(
            "echo getenv('{PROBE_VAR}'), PHP_EOL, getenv('REXENV_NOT_IN_SNAPSHOT') === false ? 'absent' : 'present', PHP_EOL;"
        );
        let r = run_step_streamed(sup, &php, &["-r".into(), script], &root, &env, &CancelToken::new(), &mut |l| lines.push(l.to_string()), None);
        check.is("a step streams its lines and exits 0", r.as_ref().is_ok_and(|r| r.ok) && lines.iter().any(|l| l == PROBE_VALUE), &format!("{r:?} {lines:?}"));
        check.is("the step sees only the environment it was handed", lines.iter().any(|l| l == "absent"), &format!("{lines:?}"));

        // ── 3. Cancel ends the tree. ──
        let (step, grandchild) = (Arc::new(Mutex::new(None::<u32>)), Arc::new(Mutex::new(None::<u32>)));
        let cancel = CancelToken::new();
        let watcher = {
            let (step, grandchild, cancel) = (step.clone(), grandchild.clone(), cancel.clone());
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(30);
                while Instant::now() < deadline && grandchild.lock().unwrap().is_none() {
                    std::thread::sleep(Duration::from_millis(100));
                }
                *step.lock().unwrap() = cancel.current_pgid();
                let platform = rexenv_lib::platform::current();
                cancel.cancel(platform.supervisor());
            })
        };
        let started = Instant::now();
        let r = run_step_streamed(sup, &php, &["-r".into(), GRANDCHILD.into()], &root, &env, &cancel, &mut |l| {
            if let Some(pid) = l.strip_prefix("grandchild=") {
                *grandchild.lock().unwrap() = pid.trim().parse().ok();
            }
        }, None);
        let _ = watcher.join();
        let (s, g) = (*step.lock().unwrap(), *grandchild.lock().unwrap());
        println!("  · cancelled step {s:?}, grandchild {g:?}, after {:.1} s", started.elapsed().as_secs_f64());
        check.is("cancel: the step reports cancelled within seconds", r.as_ref().is_ok_and(|r| r.cancelled) && started.elapsed() < Duration::from_secs(20), &format!("{r:?}"));
        std::thread::sleep(Duration::from_millis(500));
        check.is("cancel: the step and its grandchild are gone", both_gone(&*plat, s, g), &format!("step {s:?} grandchild {g:?}"));

        // ── 4. The idle limit ends the tree. ──
        let grandchild = Arc::new(Mutex::new(None::<u32>));
        let started = Instant::now();
        let r = run_step_streamed(sup, &php, &["-r".into(), GRANDCHILD.into()], &root, &env, &CancelToken::new(), &mut |l| {
            if let Some(pid) = l.strip_prefix("grandchild=") {
                *grandchild.lock().unwrap() = pid.trim().parse().ok();
            }
        }, Some(Duration::from_secs(3)));
        let g = *grandchild.lock().unwrap();
        check.is(
            "idle: a silent step is killed as stalled",
            r.as_ref().is_ok_and(|r| !r.ok && !r.cancelled && r.tail.iter().any(|l| l.contains("killed as stalled"))) && started.elapsed() < Duration::from_secs(20),
            &format!("{r:?}"),
        );
        std::thread::sleep(Duration::from_millis(500));
        check.is("idle: its grandchild is gone too", both_gone(&*plat, None, g), &format!("grandchild {g:?}"));

        // ── 5. A launcher that exits without stopping its step takes the step with it. ──
        let pids_file = root.join("orphan-pids.txt");
        let ran = Command::new(std::env::current_exe().unwrap())
            .args(["orphan", &pids_file.display().to_string(), &php.display().to_string()])
            .status();
        std::thread::sleep(Duration::from_secs(2));
        let pids: Vec<u32> = std::fs::read_to_string(&pids_file).unwrap_or_default().split_whitespace().filter_map(|p| p.parse().ok()).collect();
        println!("  · the launcher exited ({ran:?}) leaving step/grandchild {pids:?}");
        check.is(
            "quit: the step and its grandchild die with the process that started them",
            pids.len() == 2 && pids[1] != 0 && both_gone(&*plat, Some(pids[0]), Some(pids[1])),
            &format!("{pids:?}"),
        );

        // ── 6. Composer through the site's PHP. ──
        match binaries::resolve_file(&*plat, "composer", binaries::pins().composer).await {
            Ok(composer) => {
                let project = root.join("laravel app");
                std::fs::create_dir_all(&project).unwrap();
                let mut env6 = env.clone();
                env6.push(("COMPOSER_HOME".into(), root.join("composer-home").display().to_string()));
                env6.push(("COMPOSER_CACHE_DIR".into(), root.join("composer-cache").display().to_string()));
                let started = Instant::now();
                let mut count = 0usize;
                let mut tail: Vec<String> = Vec::new();
                let created = laravel::create_project(sup, &php, &composer, &project, &env6, &CancelToken::new(), &mut |l| {
                    count += 1;
                    tail.push(l.to_string());
                    if tail.len() > 20 {
                        tail.remove(0);
                    }
                });
                println!("  · create-project: {count} lines in {:.0} s", started.elapsed().as_secs_f64());
                let ok = created.is_ok() && laravel::is_installed(&project);
                check.is(
                    "composer create-project laravel/laravel runs through the site's php.exe and installs the app",
                    ok,
                    &format!("{:?} — last lines:\n{}", created.err(), tail.join("\n")),
                );
            }
            Err(e) => check.is("the Composer phar resolves", false, &e.to_string()),
        }

        finish(check, &root)
    }

    fn finish(check: Check, root: &Path) -> ExitCode {
        if let Err(e) = std::fs::remove_dir_all(root) {
            println!("  · fixture not fully removed: {e}");
        }
        check.verdict()
    }

    fn both_gone(plat: &dyn rexenv_lib::platform::traits::Platform, step: Option<u32>, grandchild: Option<u32>) -> bool {
        let sup = plat.supervisor();
        grandchild.is_some() && [step, grandchild].into_iter().flatten().all(|pid| !sup.pid_alive(pid))
    }

    /// Child mode: start a step, record its pid and its grandchild's, and exit WITHOUT stopping it.
    fn orphan(args: &[String]) -> ExitCode {
        let (Some(file), Some(php)) = (args.get(2), args.get(3)) else { return ExitCode::FAILURE };
        let plat = rexenv_lib::platform::current();
        let env = plat.shell().login_shell_env().unwrap_or_default();
        let Ok(mut child) = plat.supervisor().spawn_streamed(&PathBuf::from(php), &["-r".into(), GRANDCHILD.into()], &std::env::temp_dir(), &env) else {
            return ExitCode::FAILURE;
        };
        let mut line = String::new();
        if let Some(stdout) = child.stdout.take() {
            let _ = std::io::BufReader::new(stdout).read_line(&mut line);
        }
        let grandchild = line.trim().strip_prefix("grandchild=").unwrap_or("0").to_string();
        let _ = std::fs::write(file, format!("{} {grandchild}", child.id()));
        std::process::exit(0)
    }
}
