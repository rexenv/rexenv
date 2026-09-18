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
    /// Working directory the shell starts in (the site's docroot, or a
    /// plugin/theme folder inside it — see `asset_cwd`).
    pub cwd: PathBuf,
    /// The shell to run (e.g. the user's `$SHELL`).
    pub shell: String,
    /// Directories prepended to `PATH` (bundled PHP dir, the `wp` wrapper dir, …).
    pub path_prepend: Vec<PathBuf>,
    /// Extra variables set on the shell, ON TOP of the inherited environment —
    /// today the mail catch-all's `MAIL_*` (`core::laravel::mail_env`), so
    /// `php artisan` typed in rexenv's own terminal reaches Mailpit the same way
    /// a page request does. Empty when the user has turned the catch-all off.
    ///
    /// Applied AFTER the inherited vars and before `PATH`/`TERM`, so a value
    /// here wins over whatever the parent process happened to be started with.
    pub env: Vec<(String, String)>,
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
        // LOGIN shell. A GUI app is launched by launchd, so its PATH is the bare
        // `/usr/bin:/bin:/usr/sbin:/sbin` — and a non-login zsh reads only
        // ~/.zshrc, never /etc/zprofile (path_helper → /etc/paths, /etc/paths.d)
        // nor ~/.zprofile (`brew shellenv`). That is where /usr/local/bin and
        // /opt/homebrew/bin come from, so without `-l` everything the developer
        // installed — `code`, `rex`, `git` from brew, nvm shims — is "command not
        // found" in our terminal while working fine in Terminal.app (which runs
        // `login -pf`). Unknown shells get no flag: a bad flag fails the spawn.
        for a in login_args(&cfg.shell) {
            cmd.arg(a);
        }
        cmd.cwd(&cfg.cwd);
        // Inherit the full parent environment, then override PATH + TERM.
        for (k, v) in std::env::vars() {
            cmd.env(k, v);
        }
        for (k, v) in &cfg.env {
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
        if let Some(line) = path_export_line(&cfg.path_prepend, std::env::consts::OS) {
            let _ = writeln!(writer, "{line}");
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

/// Flags that make `shell` a LOGIN shell, by shell family. Empty for anything we
/// do not recognise — an unknown shell handed an unknown flag would fail to spawn
/// (or worse, treat it as a script), and a terminal that opens with a short PATH
/// beats a terminal that does not open.
fn login_args(shell: &str) -> &'static [&'static str] {
    let name = Path::new(shell).file_name().and_then(|n| n.to_str()).unwrap_or("");
    match name {
        "zsh" | "bash" | "sh" | "ksh" | "dash" | "fish" | "csh" | "tcsh" => &["-l"],
        _ => &[],
    }
}

/// Build the `PATH` env value with our dirs prepended to the current `PATH`.
fn prepend_path(dirs: &[PathBuf]) -> String {
    prepend_path_on(dirs, &std::env::var("PATH").unwrap_or_default(), std::env::consts::OS)
}

/// The decision behind [`prepend_path`], with the current PATH and the OS passed in so
/// BOTH answers are measurable from either machine.
fn prepend_path_on(dirs: &[PathBuf], current: &str, os: &str) -> String {
    let mut parts = join_paths_on(dirs, os);
    if !current.is_empty() {
        if !parts.is_empty() {
            parts.push(path_list_sep(os));
        }
        parts.push_str(current);
    }
    parts
}

/// The line TYPED into the fresh shell to put our dirs back at the front of `PATH`,
/// or `None` where there is nothing to undo.
///
/// `PATH` is already correct on the spawned command (`cmd.env("PATH", …)`); this exists
/// only because a macOS login shell REORDERS it afterwards — `/etc/zprofile` runs
/// `path_helper`, which moves `/usr/bin` back in front of everything we prepended.
///
/// **Windows gets `None`, and that is the honest answer rather than a port.** There is no
/// path_helper there, so nothing undoes the env we set; and the line is POSIX `export`
/// syntax, so sending it to PowerShell would type a visible error into the user's prompt
/// to fix a problem that OS does not have (#688).
fn path_export_line(dirs: &[PathBuf], os: &str) -> Option<String> {
    if os == "windows" || dirs.is_empty() {
        return None;
    }
    // Escape the dirs for the DOUBLE-quoted context so a dir couldn't break the
    // export (B16 — consistency with the cli.rs/sh_quote quote discipline). The
    // trailing literal `:$PATH` stays outside the escape so it still interpolates.
    // Identity for real bin dirs (no `\"$` `).
    Some(format!("export PATH=\"{}:$PATH\"", dq_escape(&join_paths_on(dirs, os))))
}

/// Escape a string for embedding in a DOUBLE-quoted shell string: neutralize the
/// four chars active inside `"…"` (`\ " $ \``), backslash first so the escapes we
/// add aren't re-escaped. Identity for strings without them (every real bin dir),
/// so the exported PATH line is byte-identical for real inputs (B16).
fn dq_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

/// What separates entries in a PATH LIST on `os` — not the path separator. Windows uses
/// `;`, and a `:` there would make `C:\\php` and the rest one unusable entry, which is
/// how the terminal's php and wp dirs would have been lost even after the shell was
/// right (#688).
fn path_list_sep(os: &str) -> char {
    if os == "windows" {
        ';'
    } else {
        ':'
    }
}

/// Join dirs with the PATH-list separator for `os` (no trailing separator). There is no
/// host-reading wrapper: the two callers already carry an `os`, and a wrapper that reads
/// `std::env::consts::OS` would be dead code the bar denies — the third time that trap
/// has fired on this port (#657, #666, #668).
fn join_paths_on(dirs: &[PathBuf], os: &str) -> String {
    dirs.iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(&path_list_sep(os).to_string())
}

/// Where a terminal opened FROM a plugin/theme row starts: that asset's own
/// folder under the site's recorded content dir.
///
/// Reuses `repo::asset_dest` rather than joining `wp-content/plugins/<name>`
/// here — that is the ONE place the content-dir layout (`app` for Bedrock,
/// `content` for Radicle) and the folder-name validation live, and a second
/// copy of either is how a Bedrock site would get a shell in a directory that
/// does not exist.
///
/// A missing folder is an ERROR, not a silent fall back to the docroot: a
/// single-file plugin (`hello.php`) and a drop-in have no folder of their own,
/// and a shell that quietly opened somewhere else would read as "this IS the
/// plugin's directory".
pub fn asset_cwd(docroot: &Path, content_rel: &str, kind: &str, name: &str) -> Result<PathBuf> {
    let dir = crate::core::repo::asset_dest(docroot, content_rel, kind, name)?;
    if !dir.is_dir() {
        return Err(Error::Other(format!(
            "\"{name}\" has no folder of its own at {} — a single-file plugin or a \
             drop-in lives directly in the content dir, so there is nothing to open a \
             terminal in.",
            dir.display()
        )));
    }
    Ok(dir)
}

/// Ensure a `wp` wrapper exists under app-data so the terminal exposes WP-CLI as
/// `wp` (runs the bundled PHP against `wp-cli.phar`, matching the Phase-1 512M
/// limit). Returns the directory holding it (to put on `PATH`). macOS/Linux only;
/// Windows would need a `.cmd` shim (deferred with the rest of the Windows port).
pub fn ensure_wp_wrapper(platform: &dyn Platform, php_bin: &Path, wp_phar: &Path) -> Result<PathBuf> {
    let dir = platform.paths().app_data_dir()?.join("terminal").join("bin");
    std::fs::create_dir_all(&dir)?;
    let (name, script) = wp_wrapper_for(php_bin, wp_phar, std::env::consts::OS);
    let wrapper = dir.join(name);
    std::fs::write(&wrapper, script)?;
    platform.permissions().set_executable(&wrapper)?;
    Ok(dir)
}

/// The `wp` wrapper's FILE NAME and contents for `os`.
///
/// The name is the half that was wrong. An extensionless `wp` holding `#!/bin/sh` is a
/// command on macOS and a mystery file on Windows: typing `wp --version` in the app's own
/// terminal raised Windows' "How do you want to open this file?" picker — a list of
/// installed apps, one keystroke from opening wp-cli in Internet Explorer — and returned
/// nothing (seen on the Dell, 19 Sep 2026, #689). Windows runs a command because of its
/// EXTENSION, so the wrapper is `wp.cmd` there.
///
/// `%*` forwards the arguments the way `"$@"` does, and `@echo off` keeps the shell from
/// printing the wrapper's own line before every wp-cli run.
fn wp_wrapper_for(php_bin: &Path, wp_phar: &Path, os: &str) -> (&'static str, String) {
    if os == "windows" {
        return (
            "wp.cmd",
            format!(
                "@echo off\r\n\"{php}\" -d memory_limit=512M \"{phar}\" %*\r\n",
                php = php_bin.display(),
                phar = wp_phar.display(),
            ),
        );
    }
    (
        "wp",
        format!(
            "#!/bin/sh\nexec \"{php}\" -d memory_limit=512M \"{phar}\" \"$@\"\n",
            php = php_bin.display(),
            phar = wp_phar.display(),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dq_escape_is_identity_for_real_dirs_and_neutralizes_double_quote_chars() {
        // Real bin dirs (incl. spaces) are unchanged → the export PATH line is
        // byte-identical for every real input.
        for p in ["/a b/bin", "/App Support/dev.rexenv.rexenv/bin", "/c/bin"] {
            assert_eq!(dq_escape(p), p, "unchanged for real dirs");
        }
        // The four chars active inside "…" are neutralized so a crafted dir
        // couldn't break the export (B16).
        assert_eq!(dq_escape(r#"/x/a"b$c`d\e"#), r#"/x/a\"b\$c\`d\\e"#);
    }

    #[test]
    fn asset_cwd_follows_the_recorded_content_dir_and_refuses_a_folderless_asset() {
        let root = std::env::temp_dir().join(format!("rexenv-term-cwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // A Bedrock layout: the content dir is `app`, NOT wp-content. Joining a
        // hardcoded wp-content here would open a shell in a dead path.
        std::fs::create_dir_all(root.join("app/plugins/acme")).unwrap();
        std::fs::create_dir_all(root.join("app/themes/twenty")).unwrap();
        std::fs::write(root.join("app/plugins/hello.php"), "x").unwrap();

        assert_eq!(
            asset_cwd(&root, "app", "plugin", "acme").unwrap(),
            root.join("app/plugins/acme")
        );
        assert_eq!(
            asset_cwd(&root, "app", "theme", "twenty").unwrap(),
            root.join("app/themes/twenty")
        );
        // A single-file plugin has no folder: an error, never a silent shell in
        // the docroot that reads as the plugin's own directory.
        let err = asset_cwd(&root, "app", "plugin", "hello").unwrap_err().to_string();
        assert!(err.contains("no folder of its own"), "{err}");
        // The kind and the name are still validated by `asset_dest` (M7 class).
        assert!(asset_cwd(&root, "app", "mu-plugin", "acme").is_err());
        assert!(asset_cwd(&root, "app", "plugin", "../../etc").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn known_shells_are_launched_as_login_shells() {
        // The whole point: without `-l` the shell never reads /etc/zprofile
        // (path_helper) or ~/.zprofile (brew shellenv), so a GUI-launched app's
        // bare launchd PATH is all the terminal ever sees.
        for s in ["/bin/zsh", "/bin/bash", "/opt/homebrew/bin/fish", "/bin/sh"] {
            assert_eq!(login_args(s), &["-l"], "{s} must be a login shell");
        }
        // Unrecognised shells get no flag rather than a guessed one.
        assert!(login_args("/usr/local/bin/nu").is_empty());
        assert!(login_args("").is_empty());
    }

    /// **The terminal's PATH is joined and re-exported per OS, not per macOS.**
    ///
    /// Three layers of the same bug, each hiding the next, found by opening the Terminal tab
    /// on Windows (#688): the shell was `$SHELL` or `/bin/zsh`; the PATH list was joined with
    /// `:`, which on Windows makes `C:\php` and everything after it ONE unusable entry; and
    /// the re-prepend typed POSIX `export` syntax at whatever shell came up.
    /// **The `wp` wrapper is a command on this OS, not a file of the right shape.**
    ///
    /// An extensionless `wp` holding `#!/bin/sh` put wp-cli on PATH on macOS and put a
    /// MYSTERY FILE on PATH on Windows: `wp --version` in the app's own terminal raised the
    /// "How do you want to open this file?" picker — Internet Explorer among the choices —
    /// and returned nothing (#689). Windows decides what runs by EXTENSION.
    #[test]
    fn the_wp_wrapper_is_named_and_written_for_its_os() {
        let (php, phar) = (PathBuf::from("C:\\php\\php.exe"), PathBuf::from("C:\\wp\\wp-cli.phar"));
        let (name, script) = wp_wrapper_for(&php, &phar, "windows");
        assert_eq!(name, "wp.cmd", "Windows runs a command by its extension");
        assert!(script.starts_with("@echo off"), "no echoed wrapper line before every run");
        assert!(script.contains("%*"), "arguments forwarded: {script}");
        assert!(!script.contains("#!"), "a shebang means nothing here: {script}");

        let (name, script) = wp_wrapper_for(&php, &phar, "macos");
        assert_eq!(name, "wp");
        assert!(script.starts_with("#!/bin/sh"));
        assert!(script.contains("\"$@\""), "arguments forwarded: {script}");
    }

    #[test]
    fn the_path_list_and_its_re_export_follow_the_os() {
        let dirs = vec![PathBuf::from("C:\\php\\8.3"), PathBuf::from("C:\\rexenv\\bin")];
        assert_eq!(join_paths_on(&dirs, "windows"), "C:\\php\\8.3;C:\\rexenv\\bin");
        assert_eq!(join_paths_on(&dirs, "macos"), "C:\\php\\8.3:C:\\rexenv\\bin");
        assert_eq!(
            prepend_path_on(&dirs, "C:\\Windows", "windows"),
            "C:\\php\\8.3;C:\\rexenv\\bin;C:\\Windows"
        );
        // Nothing is typed at a PowerShell prompt: PATH is already right on the spawned
        // command, and only macOS's path_helper undoes it.
        assert_eq!(path_export_line(&dirs, "windows"), None);
        assert!(path_export_line(&dirs, "macos").expect("macos re-exports").starts_with("export PATH="));
        assert_eq!(path_export_line(&[], "macos"), None, "nothing to prepend, nothing to type");
    }

    #[test]
    fn join_and_prepend_paths() {
        let dirs = vec![PathBuf::from("/a b/bin"), PathBuf::from("/c/bin")];
        assert_eq!(join_paths_on(&dirs, "macos"), "/a b/bin:/c/bin");
        let p = prepend_path(&dirs);
        // Our dirs come first, then the inherited PATH verbatim. Exact
        // equality on both branches — the old `contains(unwrap_or_default())`
        // degenerated to `contains("")` with PATH unset and could never fail.
        match std::env::var("PATH") {
            Ok(inherited) if !inherited.is_empty() => {
                assert_eq!(p, format!("/a b/bin:/c/bin:{inherited}"));
            }
            _ => assert_eq!(p, "/a b/bin:/c/bin"),
        }
    }
}
