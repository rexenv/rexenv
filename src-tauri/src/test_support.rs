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

/// A symlink `dst` → `src`. Unsupported off unix: a Windows symlink needs
/// Developer Mode or elevation, and the tests calling this are about unix links.
pub(crate) fn symlink(
    src: impl AsRef<std::path::Path>,
    dst: impl AsRef<std::path::Path>,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
    }
    #[cfg(not(unix))]
    {
        let _ = (src, dst);
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "symlink fixtures are unix-only"))
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
