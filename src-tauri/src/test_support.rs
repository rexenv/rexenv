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
