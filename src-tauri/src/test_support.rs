//! Test-only fixtures that need an OS-specific std extension — a symlink, a unix
//! file mode, a raw exit status.
//!
//! Kept OUT of `core/` on purpose. Ledger #163's scan refuses `std::os::unix` and
//! `cfg(unix)` in `core/`'s production lines, and a helper FILE reads as
//! production to that scan even when only tests call it. Tests across `core/` and
//! `commands/` used these extensions inline, which is what kept the `lib test`
//! unit from compiling for Windows (docs/PLAN-windows-port.md W1). Off unix these
//! compile; whether a symlink or mode test can RUN there is the Windows test
//! pass's question (W12), not this file's.

/// A symlink `dst` → `src`.
///
/// Windows makes one too, since 17 Sep 2026 — the first Windows test run turned 22
/// failures into one line: this helper returned `Unsupported`, and every test calling
/// it `.unwrap()`ed. Measured on the Dell the same day, a Windows symlink IS creatable
/// there (`SeCreateSymbolicLinkPrivilege` enabled under the elevated task the tests run
/// in; Developer Mode is off), so refusing was costing real coverage.
///
/// Two things Windows needs that unix does not, and both are load-bearing:
/// the call differs for a file and a directory (`symlink_file` vs `symlink_dir`, and the
/// wrong one produces a link that resolves to nothing), and a DANGLING target — which
/// several fixtures create on purpose — has no kind to read, so it is linked as a file,
/// which is what those fixtures mean. Without the privilege the error is
/// `PermissionDenied` from the OS rather than this function's own refusal; the caller
/// sees a real message either way instead of a fixture that silently did nothing.
pub(crate) fn symlink(
    src: impl AsRef<std::path::Path>,
    dst: impl AsRef<std::path::Path>,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
    }
    #[cfg(windows)]
    {
        let (src, dst) = (src.as_ref(), dst.as_ref());
        if src.is_dir() {
            std::os::windows::fs::symlink_dir(src, dst)
        } else {
            std::os::windows::fs::symlink_file(src, dst)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (src, dst);
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "symlink fixtures need unix or windows"))
    }
}

/// A child process that STAYS ALIVE for about half a minute — a stand-in for a
/// running pool master, where the test cares that the pid is live and nothing else.
///
/// Why a helper at all: the tests spawned `sleep 30`, and `sleep` is not a Windows
/// program. It is not even on the Dell's process PATH — only inside Git Bash's
/// `/usr/bin` (measured 17 Sep 2026), which is the nastiest version of the problem:
/// the tests PASS when the suite is started from Git Bash and panic in
/// `spawn().unwrap()` under a scheduled task or any other launcher. The command below
/// is in System32, so it does not depend on how the suite was started.
pub(crate) fn live_child() -> std::process::Child {
    #[cfg(unix)]
    {
        std::process::Command::new("sleep").arg("30").spawn().expect("spawn a live child")
    }
    #[cfg(windows)]
    {
        // `ping -n 31 127.0.0.1` waits a second between pings and is the standard
        // Windows "sleep" with no dependencies; the output goes nowhere.
        std::process::Command::new("cmd")
            .args(["/c", "ping -n 31 127.0.0.1 >nul"])
            .spawn()
            .expect("spawn a live child")
    }
}

/// A child that has ALREADY EXITED, for the "this pid is gone" half of the same tests.
/// Returns after it is reaped, so `try_wait` sees the exit immediately.
pub(crate) fn dead_child() -> std::process::Child {
    #[cfg(unix)]
    let mut child =
        std::process::Command::new("true").spawn().expect("spawn a child that exits at once");
    #[cfg(windows)]
    let mut child = std::process::Command::new("cmd")
        .args(["/c", "exit 0"])
        .spawn()
        .expect("spawn a child that exits at once");
    let _ = child.wait();
    child
}

/// A child that BURNS a core, standing in for a php-cgi parent retrying a spawn — the
/// churn breaker's subject (ledger #605). The caller owns it and must kill it: like
/// `yes`, neither form ends on its own.
pub(crate) fn spinning_child() -> std::process::Child {
    #[cfg(unix)]
    {
        std::process::Command::new("yes")
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn a spinning child")
    }
    #[cfg(windows)]
    {
        // `for /l %i in (1,0,2)` counts up from 1 by 0 — an infinite loop doing `rem`,
        // measured at 1.05 CPU seconds in 3 wall seconds on the Dell, which is the
        // quarter-core the breaker looks for and then some.
        std::process::Command::new("cmd")
            .args(["/c", "for /l %i in (1,0,2) do rem"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn a spinning child")
    }
}

/// A child that exits with `code`, for the "an ordinary failure is not a timeout" half
/// of the timeout tests. Returns the spawned child; the caller waits on it.
///
/// Measured on the Dell 17 Sep 2026: `cmd /c exit 3` yields exit code 3, so the Windows
/// arm carries the code the same way the unix one does.
pub(crate) fn exiting_child(code: i32) -> std::process::Child {
    #[cfg(unix)]
    {
        std::process::Command::new("/bin/sh")
            .args(["-c", &format!("exit {code}")])
            .spawn()
            .expect("spawn a child with a chosen exit code")
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/c", &format!("exit {code}")])
            .spawn()
            .expect("spawn a child with a chosen exit code")
    }
}

/// A child that writes one line to stdout and one to stderr, then exits 0 — the fixture
/// for "both streams are drained, and kept apart".
///
/// The Windows form is `echo out& echo err 1>&2`: the `&` separates commands inside one
/// `cmd /c`, and the redirect binds to the second one only (measured on the Dell).
pub(crate) fn two_stream_command() -> std::process::Command {
    #[cfg(unix)]
    {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "echo out; echo err 1>&2"]);
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/c", "echo out& echo err 1>&2"]);
        cmd
    }
}

/// A child that writes far past the ~64KB pipe buffer before exiting — the fixture that
/// caught a deadlock reported as a FAKE timeout (the old wait-then-read shape blocked the
/// child on a full pipe and never reached `try_wait`).
///
/// Both forms were measured for size rather than assumed: the Windows loop produces
/// 318,000 bytes in about two seconds on the Dell.
pub(crate) fn chatty_command() -> std::process::Command {
    #[cfg(unix)]
    {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "head -c 300000 /dev/zero | tr '\\0' 'x'; echo done"]);
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args([
            "/c",
            "for /l %i in (1,1,6000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ]);
        cmd
    }
}

/// A `Command` (not a spawned child) that stays alive for about half a minute — for the
/// timeout paths, which take a `Command` and own the spawn themselves.
pub(crate) fn stalling_command() -> std::process::Command {
    #[cfg(unix)]
    {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "sleep 30"]);
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/c", "ping -n 31 127.0.0.1 >nul"]);
        cmd
    }
}

/// A shell step as `(program, args)` — for `run_step_streamed`, which takes a program PATH
/// and an argv rather than a `Command`.
///
/// The two scripts are separate arguments because they are not translations of each other:
/// `sleep 0.2` has no `cmd` equivalent, and `ping -n 2` is the idiom that waits a second.
/// Passing both keeps each side readable instead of building one string with `cfg!`.
pub(crate) fn shell_step(unix: &str, windows: &str) -> (std::path::PathBuf, Vec<String>) {
    #[cfg(unix)]
    {
        let _ = windows;
        (std::path::PathBuf::from("/bin/sh"), vec!["-c".to_string(), unix.to_string()])
    }
    #[cfg(windows)]
    {
        let _ = unix;
        (std::path::PathBuf::from("cmd"), vec!["/c".to_string(), windows.to_string()])
    }
}

/// chmod `path` to `mode`. A no-op off unix.
pub(crate) fn set_mode(path: impl AsRef<std::path::Path>, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// The `ExitStatus` a captured `Output` carries for a process that exited with
/// `code`.
pub(crate) fn exit_status(code: i32) -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code << 8)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code as u32)
    }
}
