//! System developer-tool discovery for repo installs: git, node + a package
//! manager, composer. A DELIBERATE departure from the bundled-client rule
//! (§9): bundles exist so rexenv's own features never depend on the user's
//! machine — but a cloned repo IS the user's workflow, so builds run THEIR
//! toolchain (their node version manager, their ssh-agent), resolved from the
//! real login-shell environment (`ShellRunner::login_shell_env`), never from
//! our inherited launchd PATH. A missing tool is an honest error with a
//! copy-paste fix (ports.rs house style) — never a silent fallback. The one
//! planned fallback is composer (a pinned `.phar` on the site's bundled PHP,
//! the wp-cli model) — that lands with the install step, not here.

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};

// The login-shell env wire protocol (marker + parser) lives next to the
// ShellRunner trait — platform impls use it without importing core (the
// dependency stays one-way: core → platform). Re-exported here because
// devtools is the consumer-facing seam.
pub use crate::platform::traits::{parse_shell_env_output, ENV_MARKER};

/// A resolved system tool.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub path: PathBuf,
    /// First line of `<tool> --version`, None if the probe failed.
    pub version: Option<String>,
}

/// A variable's value from an env snapshot.
pub fn env_var<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Walk the snapshot's PATH for `name`, first hit wins (the shell's own
/// precedence — an nvm shim beats /usr/bin). Only `is_file` is checked:
/// exec-bit probing is OS-specific, and a non-executable PATH collision is
/// pathological — it surfaces at spawn with a clear OS error. How PATH is
/// read — its separator, its name's case, `git` being `git.exe` — is the
/// platform's (`platform::path_lookup`, ledger #628).
pub fn find_tool(env: &[(String, String)], name: &str) -> Option<PathBuf> {
    find_tool_by(crate::platform::path_lookup::current(), env, name, &|p| p.is_file())
}

/// [`find_tool`] under the given rules and file test — tests walk the Windows
/// rules on every host.
pub fn find_tool_by(
    rules: &crate::platform::path_lookup::PathLookup,
    env: &[(String, String)],
    name: &str,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    rules.candidates(env, name).into_iter().find(|p| is_file(p))
}

/// `find_tool` + a `--version` probe. None when the tool is absent.
pub fn find_optional(env: &[(String, String)], name: &str) -> Option<ToolInfo> {
    let path = find_tool(env, name)?;
    Some(ToolInfo { version: probe_version(&path), path })
}

/// `--version` probes must return fast — a hung binary (dead network mount,
/// broken shim) previously blocked `repo_tools`/`repo_probe` forever on a bare
/// `.output()`. ~100× a legit probe; on timeout the tool reports `version:
/// None` (present, version unknown) rather than a hard failure (B25).
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn probe_version(path: &Path) -> Option<String> {
    probe_version_with(path, PROBE_TIMEOUT)
}

/// [`probe_version`] with an injectable bound (tests use a short one so the
/// hung-binary case doesn't wait out the real 10s).
fn probe_version_with(path: &Path, timeout: std::time::Duration) -> Option<String> {
    let mut cmd = crate::platform::command(path);
    cmd.arg("--version");
    // The drain-on-threads runner (B25 stage 2): kills on expiry, reaps, and a
    // chatty tool can't fake-timeout on a full pipe.
    let out = crate::core::wordpress::run_with_timeout(cmd, timeout, "version probe").ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let line = line.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// System git, preflighted so a missing Xcode CLT becomes an honest error
/// instead of a surprise GUI dialog (`ShellRunner::git_preflight`).
pub fn resolve_git(platform: &dyn Platform, env: &[(String, String)]) -> Result<ToolInfo> {
    platform.shell().git_preflight()?;
    // The fix names this OS's own tools (`platform::words`, ledger #626): a Windows screen must not say Xcode.
    let path = find_tool(env, "git").ok_or_else(|| {
        Error::Other(format!(
            "git not found in your shell environment. {}",
            crate::platform::words::current().git_install
        ))
    })?;
    Ok(ToolInfo { version: probe_version(&path), path })
}

/// System Node.js. The error names WHERE we looked — the user's login-shell
/// PATH — so "but it works in my terminal" has an answer.
pub fn resolve_node(env: &[(String, String)]) -> Result<ToolInfo> {
    let path = find_tool(env, "node").ok_or_else(|| {
        Error::Other(format!(
            "Node.js not found in your shell environment (rexenv resolves \
             tools through your login shell's PATH, so nvm/fnm installs are \
             seen). Install Node, then hit Re-detect:\n{}",
            crate::platform::words::current().node_install
        ))
    })?;
    Ok(ToolInfo { version: probe_version(&path), path })
}

/// A JS package manager by name (npm/pnpm/yarn/bun). npm ships with Node;
/// pnpm/yarn are usually corepack shims, so the miss suggests exactly that.
pub fn resolve_package_manager(env: &[(String, String)], name: &str) -> Result<ToolInfo> {
    if let Some(tool) = find_optional(env, name) {
        return Ok(tool);
    }
    let words = crate::platform::words::current();
    let fix = match name {
        "npm" => format!("npm ships with Node.js — install Node, then hit Re-detect:\n{}", words.node_install),
        "pnpm" | "yarn" => "this repo pins it via package.json's packageManager field. \
             corepack (ships with Node) provides it — enable once, then hit \
             Re-detect:\n$ corepack enable"
            .to_string(),
        "bun" => format!("this repo uses Bun. Install it, then hit Re-detect:\n{}", words.bun_install),
        _ => "install it, then hit Re-detect.".to_string(),
    };
    Err(Error::Other(format!(
        "{name} not found in your shell environment — {fix}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_parse_survives_rc_noise_markers_and_multiline_values() {
        let raw = format!(
            "Welcome to zsh!\nmotd says PATH=/evil\n\0{ENV_MARKER}\0PATH=/opt/homebrew/bin:/usr/bin\0MULTI=line one\nline two\0BAD ENTRY\0_UND=ok\09LEAD=no\0",
        );
        let env = parse_shell_env_output(raw.as_bytes());
        assert_eq!(env_var(&env, "PATH"), Some("/opt/homebrew/bin:/usr/bin"));
        // NUL separation: a value with a newline is ONE entry (the point of env -0).
        assert_eq!(env_var(&env, "MULTI"), Some("line one\nline two"));
        assert_eq!(env_var(&env, "_UND"), Some("ok"));
        // Junk filtered: no '=', digit-leading keys, pre-marker noise.
        assert_eq!(env.iter().filter(|(k, _)| k == "9LEAD").count(), 0);
        assert!(env_var(&env, "PATH") != Some("/evil"));
        // Defensive: no marker at all → still parses the buffer.
        let bare = parse_shell_env_output(b"A=1\0B=2\0");
        assert_eq!(env_var(&bare, "B"), Some("2"));
    }

    #[test]
    #[cfg(unix)] // runs `#!/bin/sh` scripts
    fn probe_version_reports_a_tool_and_bounds_a_hung_one() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rexenv-probe-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        // A normal tool answers --version instantly → Some(first line).
        let ok = dir.join("goodtool");
        std::fs::write(&ok, "#!/bin/sh\necho tool 1.2.3\n").unwrap();
        std::fs::set_permissions(&ok, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(probe_version(&ok).as_deref(), Some("tool 1.2.3"));

        // A hung tool (dead mount / broken shim shape) must come back None
        // within the bound — previously a bare .output() blocked repo_tools
        // forever (B25). Short injectable bound so the test doesn't wait 10s.
        let hung = dir.join("hungtool");
        std::fs::write(&hung, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&hung, std::fs::Permissions::from_mode(0o755)).unwrap();
        let start = std::time::Instant::now();
        assert_eq!(probe_version_with(&hung, std::time::Duration::from_millis(300)), None);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "bounded, not blocked: {:?}",
            start.elapsed()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_tool_walks_path_in_shell_precedence_order() {
        // Driven through the UNIX rules by name, the way the Windows-shape test beside
        // this one drives `WINDOWS`. `find_tool` reads the HOST's rules, so a `:`-joined
        // PATH and an extensionless `node` are a unix PATH asserted on every machine —
        // on Windows the separator is `;` and a bare name needs PATHEXT, so this failed
        // there while saying nothing about the precedence it is named for (W12).
        //
        // **The directories are synthetic, and that is the second half of the same
        // lesson.** The first fix drove the UNIX rules but still built the PATH from real
        // temp dirs, so on Windows the value read `C:\...\first:C:\...\second` and
        // `split(':')` cut the DRIVE LETTER off every entry — the run reported
        // `\Users\DELL\...\second\node` where `C:\Users\DELL\...` was expected. A fixture
        // for unix rules must carry no Windows path, and `find_tool_by` takes an injected
        // `is_file`, so no real file is needed to prove a lookup ORDER.
        use crate::platform::path_lookup::UNIX;
        let first = Path::new("/fx/first");
        let second = Path::new("/fx/second");
        let path_val = format!("{}{}{}", first.display(), UNIX.separator, second.display());
        let env = vec![("PATH".to_string(), path_val)];
        // What "exists as a file" means here, set by the test rather than the disk.
        let present = |paths: Vec<PathBuf>| {
            move |p: &Path| -> bool { paths.iter().any(|q| q == p) }
        };

        // Only `second` has node.
        let real = present(vec![second.join("node")]);
        assert_eq!(find_tool_by(&UNIX, &env, "node", &real), Some(second.join("node")));
        assert_eq!(find_tool_by(&UNIX, &env, "pnpm", &real), None);
        // First hit wins once `first` gains one (an nvm shim beats /usr/bin).
        let real = present(vec![first.join("node"), second.join("node")]);
        assert_eq!(find_tool_by(&UNIX, &env, "node", &real), Some(first.join("node")));
        // A directory named like the tool is not a hit: `is_file` is false for it.
        let real = present(vec![]);
        assert_eq!(find_tool_by(&UNIX, &env, "git", &real), None);
    }

    /// The Dell's own shape, 15 Sep 2026: the key spelled `Path`, a drive letter in every directory,
    /// `git.exe`, `npm.cmd` and `composer.bat` — the machine where "From Git" said git was missing.
    #[test]
    fn the_windows_rules_find_git_node_and_npm_where_git_for_windows_and_node_put_them() {
        use crate::platform::path_lookup::WINDOWS;
        let env = vec![
            (
                "Path".to_string(),
                r"C:\WINDOWS\system32;C:\composer;C:\Program Files\nodejs\;C:\Program Files\Git\cmd;C:\Users\DELL\AppData\Roaming\npm"
                    .to_string(),
            ),
            ("PATHEXT".to_string(), ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.PY;.PYW;.CPL".to_string()),
        ];
        let git = Path::new(r"C:\Program Files\Git\cmd").join("git.exe");
        let node = Path::new(r"C:\Program Files\nodejs\").join("node.exe");
        let npm = Path::new(r"C:\Program Files\nodejs\").join("npm.cmd");
        let composer = Path::new(r"C:\composer").join("composer.bat");
        let on_disk = [git.clone(), node.clone(), npm.clone(), composer.clone()];
        let is_file = |p: &Path| on_disk.iter().any(|f| f == p);
        assert_eq!(find_tool_by(&WINDOWS, &env, "git", &is_file), Some(git));
        assert_eq!(find_tool_by(&WINDOWS, &env, "node", &is_file), Some(node));
        assert_eq!(find_tool_by(&WINDOWS, &env, "npm", &is_file), Some(npm));
        assert_eq!(find_tool_by(&WINDOWS, &env, "composer", &is_file), Some(composer));
        assert_eq!(find_tool_by(&WINDOWS, &env, "pnpm", &is_file), None);
    }

    #[test]
    fn missing_tools_error_with_a_copy_paste_fix() {
        let empty: Vec<(String, String)> = vec![("PATH".into(), "/nonexistent-x".into())];
        let node = resolve_node(&empty).unwrap_err().to_string();
        assert!(node.contains("login shell"), "{node}");
        // A command line, not a `$` prompt: macOS writes `$ brew install node` and
        // Windows `> winget install …`, and both are the platform's own words. Asserting
        // the unix prompt character made this fail on Windows for the one thing the test
        // is NOT about (W12) — the claim is that the last line is something to paste.
        let pasteable = |s: &str| {
            let last = s.lines().last().unwrap_or_default().to_string();
            last.starts_with("$ ") || last.starts_with("> ")
        };
        assert!(pasteable(&node), "{node}");
        let pnpm = resolve_package_manager(&empty, "pnpm").unwrap_err().to_string();
        assert!(pnpm.contains("corepack"), "{pnpm}");
        assert!(pasteable(&pnpm) && pnpm.contains("corepack enable"), "{pnpm}");
        let npm = resolve_package_manager(&empty, "npm").unwrap_err().to_string();
        let words = crate::platform::words::current();
        assert_eq!(npm.lines().last().unwrap(), words.node_install, "{npm}");
        assert_eq!(node.lines().last().unwrap(), words.node_install, "{node}");
        let bun = resolve_package_manager(&empty, "bun").unwrap_err().to_string();
        assert_eq!(bun.lines().last().unwrap(), words.bun_install, "{bun}");
    }
}
