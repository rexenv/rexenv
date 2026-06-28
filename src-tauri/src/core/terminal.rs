//! core::terminal — PTY-backed shell sessions for the built-in terminal (§4.1).
//!
//! Uses the cross-platform `portable-pty` crate (NOT a platform trait — it already
//! abstracts the OS PTY), so the same code drives macOS/Linux/Windows. A session
//! spawns the user's shell in a site's docroot with rexenv's bundled PHP + a `wp`
//! wrapper prepended to `PATH`, and streams output to a caller-supplied callback
//! (the command layer turns that into Tauri events; the headless example collects
//! it over a channel — so this module stays Tauri-free).

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How to launch a terminal session.
pub struct PtyConfig {
    /// Working directory the shell starts in (the site's docroot).
    pub cwd: PathBuf,
    /// The shell to run (e.g. the user's `$SHELL`).
    pub shell: String,
    /// Directories prepended to `PATH` (bundled PHP dir, the `wp` wrapper dir, …).
    pub path_prepend: Vec<PathBuf>,
    pub rows: u16,
    pub cols: u16,
}

/// A live PTY session. All fields are behind a `Mutex` so the session is
/// `Send + Sync` and can live in shared (Tauri-managed) state.
pub struct TerminalSession {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

impl TerminalSession {
    /// Open a PTY, spawn the shell, and start a reader thread that forwards each
    /// output chunk to `on_output`. The shell's `PATH` is set both in its
    /// environment AND re-prepended via an injected `export` (run after the
    /// user's rc files, which on macOS would otherwise reorder PATH and shadow
    /// the bundled PHP).
    pub fn open(cfg: PtyConfig, on_output: impl Fn(Vec<u8>) + Send + 'static) -> Result<Self> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows: cfg.rows, cols: cfg.cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| Error::Other(format!("openpty: {e}")))?;

        let new_path = prepend_path(&cfg.path_prepend);
        let mut cmd = CommandBuilder::new(&cfg.shell);
        cmd.cwd(&cfg.cwd);
        // Inherit the full parent environment, then override PATH + TERM.
        for (k, v) in std::env::vars() {
            cmd.env(k, v);
        }
        cmd.env("PATH", &new_path);
        cmd.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| Error::Other(format!("spawn shell: {e}")))?;
        // Drop the slave so the master sees EOF when the shell exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| Error::Other(format!("pty reader: {e}")))?;
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => on_output(buf[..n].to_vec()),
                }
            }
        });

        let mut writer = pair
            .master
            .take_writer()
            .map_err(|e| Error::Other(format!("pty writer: {e}")))?;
        // Re-prepend our PATH after rc runs (macOS path_helper / user rc reorders it).
        if !cfg.path_prepend.is_empty() {
            let _ = writeln!(writer, "export PATH=\"{}:$PATH\"", join_paths(&cfg.path_prepend));
            let _ = writer.flush();
        }

        Ok(Self {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
        })
    }

    /// Write bytes to the shell's stdin (keystrokes / pasted input).
    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut w = self.writer.lock().map_err(|_| Error::Other("pty writer poisoned".into()))?;
        w.write_all(data).map_err(|e| Error::Other(format!("pty write: {e}")))?;
        w.flush().map_err(|e| Error::Other(format!("pty flush: {e}")))
    }

    /// Resize the PTY (on terminal viewport changes).
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let m = self.master.lock().map_err(|_| Error::Other("pty master poisoned".into()))?;
        m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| Error::Other(format!("pty resize: {e}")))
    }

    /// Kill the shell process (closes the session).
    pub fn kill(&self) -> Result<()> {
        let mut c = self.child.lock().map_err(|_| Error::Other("pty child poisoned".into()))?;
        c.kill().map_err(|e| Error::Other(format!("pty kill: {e}")))
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }
}

/// Build the `PATH` env value with our dirs prepended to the current `PATH`.
fn prepend_path(dirs: &[PathBuf]) -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts = join_paths(dirs);
    if !current.is_empty() {
        if !parts.is_empty() {
            parts.push(':');
        }
        parts.push_str(&current);
    }
    parts
}

/// Join dirs with the path separator (no trailing separator).
fn join_paths(dirs: &[PathBuf]) -> String {
    dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(":")
}

/// Ensure a `wp` wrapper exists under app-data so the terminal exposes WP-CLI as
/// `wp` (runs the bundled PHP against `wp-cli.phar`, matching the Phase-1 512M
/// limit). Returns the directory holding it (to put on `PATH`). macOS/Linux only;
/// Windows would need a `.cmd` shim (deferred with the rest of the Windows port).
pub fn ensure_wp_wrapper(platform: &dyn Platform, php_bin: &Path, wp_phar: &Path) -> Result<PathBuf> {
    let dir = platform.paths().app_data_dir()?.join("terminal").join("bin");
    std::fs::create_dir_all(&dir)?;
    let wrapper = dir.join("wp");
    let script = format!(
        "#!/bin/sh\nexec \"{php}\" -d memory_limit=512M \"{phar}\" \"$@\"\n",
        php = php_bin.display(),
        phar = wp_phar.display(),
    );
    std::fs::write(&wrapper, script)?;
    platform.permissions().set_executable(&wrapper)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_and_prepend_paths() {
        let dirs = vec![PathBuf::from("/a b/bin"), PathBuf::from("/c/bin")];
        assert_eq!(join_paths(&dirs), "/a b/bin:/c/bin");
        let p = prepend_path(&dirs);
        assert!(p.starts_with("/a b/bin:/c/bin"));
        // Our dirs come before the inherited PATH.
        assert!(p.contains(&std::env::var("PATH").unwrap_or_default()) || std::env::var("PATH").is_err());
    }
}
