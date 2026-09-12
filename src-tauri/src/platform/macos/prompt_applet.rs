//! macOS: the admin-password dialog, named and badged as rexenv.
//!
//! # Why a helper bundle at all
//!
//! macOS draws the admin dialog in SecurityAgent and names it after the process
//! that asked. Run through `/usr/bin/osascript`, that process is `osascript`: the
//! dialog read "osascript wants to make changes." over a generic lock, the kind
//! of prompt a careful user is right to refuse. Local's reads "Local Password
//! Prompt" over its own logo (owner report, 12 Sep 2026). SecurityAgent takes the
//! name and the badge from the asking process's BUNDLE, so the same AppleScript
//! is compiled into a small applet bundle called `rexenv` that carries our icon,
//! launched, and waited on.
//!
//! Measured on macOS 26.6.2 before any of this was written: a plain `osacompile`
//! applet renamed `rexenv` gets the name right and the badge WRONG — the applet's
//! `Assets.car` (named by `CFBundleIconName`) outranks `applet.icns`. With both
//! gone the lock carries our logo. `LSUIElement` keeps a Dock icon from appearing
//! for the few seconds the applet lives.
//!
//! # The script is compiled IN — root never reads a file
//!
//! The shell script is a string literal inside the applet's `main.scpt`, loaded
//! when the applet launches, before the dialog exists. After the user approves,
//! nothing is read from disk that a same-user process could have swapped. A
//! helper that instead reads its script from a path hands root whatever sits at
//! that path when the password is accepted. Swapping the whole bundle BEFORE
//! launch gains nothing new: any same-user process can already compile its own
//! applet named rexenv and ask for the password itself.
//!
//! # Results come back as a file
//!
//! `open -W` returns the applet's exit, not its output, so the applet writes
//! `ok\n<stdout>` or `error <number>\n<message>` to `result` beside itself. The
//! directory is per-process under the per-user `$TMPDIR`, created `0700`, and
//! removed after every run.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The name SecurityAgent prints in bold above the prompt.
const APPLET_NAME: &str = "rexenv";

/// What the privileged script did, as the applet reported it.
#[derive(Debug, PartialEq)]
pub(super) enum Outcome {
    /// The script ran as root and exited 0; its stdout, trailing newlines trimmed.
    Ran(String),
    /// AppleScript raised: `-128` is a dismissed dialog, a positive number is the
    /// script's own exit status.
    Failed { number: i64, message: String },
}

/// Why no [`Outcome`] came back.
#[derive(Debug)]
pub(super) enum RunError {
    /// The applet could not be built or launched, so no dialog was ever shown and
    /// asking another way cannot show a second one.
    NoDialog(String),
    /// The applet ran — a dialog may well have been answered — but wrote nothing
    /// readable. Never retried another way: that would be a second password prompt
    /// for a step the user may already have approved.
    NoResult(String),
}

/// A per-process working directory for one prompt. Its path is fixed by the pid,
/// so [`build`] may replace it wholesale: nothing else lives there.
pub(super) fn work_dir() -> PathBuf {
    std::env::temp_dir().join(format!("rexenv-prompt-{}", std::process::id()))
}

fn app_path(dir: &Path) -> PathBuf {
    dir.join(format!("{APPLET_NAME}.app"))
}

fn result_path(dir: &Path) -> PathBuf {
    dir.join("result")
}

/// An AppleScript double-quoted string literal holding `s`.
fn applescript_literal(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The applet's AppleScript: run `script` as root behind a dialog that says
/// `prompt`, and write the outcome to `result`. `without altering line endings`
/// keeps stdout's `\n` (the default turns them into `\r`).
pub(super) fn applet_source(script: &str, prompt: &str, result: &Path) -> String {
    let result = applescript_literal(&result.to_string_lossy());
    let script = applescript_literal(script);
    let prompt = applescript_literal(prompt);
    format!(
        "on writeResult(t)
\tset f to open for access (POSIX file {result}) with write permission
\tset eof f to 0
\twrite t to f as «class utf8»
\tclose access f
end writeResult
try
\tset r to do shell script {script} with prompt {prompt} with administrator privileges without altering line endings
\twriteResult(\"ok\" & linefeed & r)
on error errMsg number errNum
\twriteResult(\"error \" & errNum & linefeed & errMsg)
end try
"
    )
}

/// Read what the applet wrote. `None` for anything that is not one of the two
/// shapes [`applet_source`] produces.
pub(super) fn parse_result(raw: &str) -> Option<Outcome> {
    let (head, body) = raw.split_once('\n').unwrap_or((raw, ""));
    if head == "ok" {
        return Some(Outcome::Ran(body.trim_end_matches(['\n', '\r']).to_string()));
    }
    let number = head.strip_prefix("error ")?.trim().parse().ok()?;
    Some(Outcome::Failed { number, message: body.trim().to_string() })
}

/// Our icon: the running bundle's `Resources/icon.icns`, or in a debug build the
/// source tree's copy (`cargo run` has no bundle). `None` leaves the applet's own
/// badge — the name is still right.
pub(super) fn icon_source() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundled = exe.parent()?.parent()?.join("Resources/icon.icns");
    if bundled.is_file() {
        return Some(bundled);
    }
    #[cfg(debug_assertions)]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/icon.icns");
        if dev.is_file() {
            return Some(dev);
        }
    }
    None
}

fn tool(program: &str, args: &[&std::ffi::OsStr]) -> Result<()> {
    let out = Command::new(program).args(args).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Remove whatever sits at `path` without following it: a symlink planted there
/// loses the link, never its target's contents.
pub(super) fn remove(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => Ok(std::fs::remove_dir_all(path)?),
        Ok(_) => Ok(std::fs::remove_file(path)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Build the applet for `script` in `dir`, replacing anything already there.
/// `prompt` is the sentence the dialog shows under the name.
pub(super) fn build(dir: &Path, script: &str, prompt: &str, icon: Option<&Path>) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    remove(dir)?;
    std::fs::DirBuilder::new().mode(0o700).create(dir)?;

    // The source holds the root script, so it is 0600 and gone once compiled.
    let source = dir.join("prompt.applescript");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&source)?
        .write_all(applet_source(script, prompt, &result_path(dir)).as_bytes())?;
    let app = app_path(dir);
    let compiled = tool("/usr/bin/osacompile", &["-o".as_ref(), app.as_os_str(), source.as_os_str()]);
    std::fs::remove_file(&source)?;
    compiled?;

    let contents = app.join("Contents");
    remove(&contents.join("Resources/Assets.car"))?;
    let plist = contents.join("Info.plist");
    let plist = plist.as_os_str();
    // Absent on some macOS versions' applet template — nothing to remove then.
    let _ = tool("/usr/bin/plutil", &["-remove".as_ref(), "CFBundleIconName".as_ref(), plist]);
    for (key, kind, value) in [
        ("CFBundleName", "-string", APPLET_NAME),
        ("CFBundleDisplayName", "-string", APPLET_NAME),
        ("CFBundleIdentifier", "-string", "dev.rexenv.rexenv.prompt"),
        ("LSUIElement", "-bool", "YES"),
    ] {
        tool("/usr/bin/plutil", &["-replace".as_ref(), key.as_ref(), kind.as_ref(), value.as_ref(), plist])?;
    }
    if let Some(icon) = icon {
        std::fs::copy(icon, contents.join("Resources/applet.icns"))?;
    }
    // The edits above broke the template's seal; sign last, ad hoc.
    tool("/usr/bin/codesign", &["--force".as_ref(), "-s".as_ref(), "-".as_ref(), app.as_os_str()])
}

/// Launch the applet built in `dir`, wait for it to quit, and read its result.
pub(super) fn run(dir: &Path) -> std::result::Result<Outcome, RunError> {
    let app = app_path(dir);
    let launched = Command::new("/usr/bin/open")
        .args(["-n", "-W"])
        .arg(&app)
        .output()
        .map_err(|e| RunError::NoDialog(e.to_string()))?;
    let result = std::fs::read_to_string(result_path(dir));
    if !launched.status.success() && result.is_err() {
        return Err(RunError::NoDialog(
            String::from_utf8_lossy(&launched.stderr).trim().to_string(),
        ));
    }
    let raw = result.map_err(|e| RunError::NoResult(e.to_string()))?;
    parse_result(&raw).ok_or_else(|| RunError::NoResult(format!("unreadable result: {raw:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rexenv-prompt-test-{tag}-{}", std::process::id()))
    }

    #[test]
    fn the_script_is_one_escaped_literal_run_as_root_with_its_line_endings_kept() {
        let src = applet_source(
            "echo \"hi\" \\ there\nid -u",
            "rexenv wants to add a \"quoted\" thing.",
            Path::new("/Users/me/Library/Application Support/x/result"),
        );
        assert!(
            src.contains(
                "do shell script \"echo \\\"hi\\\" \\\\ there\nid -u\" with prompt \"rexenv wants to add a \\\"quoted\\\" thing.\" with administrator privileges without altering line endings"
            ),
            "{src}"
        );
        assert!(src.contains("POSIX file \"/Users/me/Library/Application Support/x/result\""));
        // The only file the applet opens is the one it WRITES its result to.
        assert_eq!(src.matches("open for access").count(), 1, "{src}");
        assert!(src.contains("with write permission"));
    }

    #[test]
    fn results_parse_to_what_ran_or_why_not_and_nothing_else() {
        assert_eq!(parse_result("ok\nroot\n"), Some(Outcome::Ran("root".into())));
        assert_eq!(parse_result("ok\na\nb"), Some(Outcome::Ran("a\nb".into())));
        assert_eq!(parse_result("ok\n"), Some(Outcome::Ran(String::new())));
        assert_eq!(parse_result("ok"), Some(Outcome::Ran(String::new())));
        assert_eq!(
            parse_result("error -128\nUser canceled."),
            Some(Outcome::Failed { number: -128, message: "User canceled.".into() })
        );
        assert_eq!(
            parse_result("error 1\nrm: /x: Permission denied\n"),
            Some(Outcome::Failed { number: 1, message: "rm: /x: Permission denied".into() })
        );
        for junk in ["", "okay\nroot", "error\nx", "error abc\nx", "root"] {
            assert_eq!(parse_result(junk), None, "{junk:?}");
        }
    }

    fn plist_value(app: &Path, key: &str) -> Option<String> {
        let out = Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(app.join("Contents/Info.plist"))
            .output()
            .unwrap();
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// The bundle the owner saw in the measured dialog, built by the real tools:
    /// named rexenv, badged with our icon (asset catalog gone), no Dock icon, a
    /// valid signature, the script compiled into `main.scpt` and no source file
    /// left beside it. Nothing is launched — no dialog.
    #[test]
    fn a_built_applet_is_named_rexenv_badged_with_our_icon_and_holds_the_script_inside() {
        let dir = scratch("build");
        let icon = Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/icon.icns");
        build(&dir, "echo rexenv-prompt-marker", "rexenv wants to prove this.", Some(&icon)).expect("build");
        let app = app_path(&dir);
        let res = app.join("Contents/Resources");

        assert_eq!(plist_value(&app, "CFBundleName").as_deref(), Some("rexenv"));
        assert_eq!(plist_value(&app, "CFBundleDisplayName").as_deref(), Some("rexenv"));
        assert_eq!(plist_value(&app, "LSUIElement").as_deref(), Some("true"));
        assert_eq!(plist_value(&app, "CFBundleIconName"), None, "the asset catalog would outrank our icon");
        assert!(!res.join("Assets.car").exists(), "Assets.car outranks applet.icns");
        assert_eq!(std::fs::read(res.join("applet.icns")).unwrap(), std::fs::read(&icon).unwrap());
        assert!(
            Command::new("/usr/bin/codesign").arg("-v").arg(&app).status().unwrap().success(),
            "the edited bundle must carry a valid signature"
        );

        let decompiled = Command::new("/usr/bin/osadecompile")
            .arg(res.join("Scripts/main.scpt"))
            .output()
            .unwrap();
        let decompiled = String::from_utf8_lossy(&decompiled.stdout);
        assert!(
            decompiled.contains(
                "do shell script \"echo rexenv-prompt-marker\" with prompt \"rexenv wants to prove this.\""
            ),
            "{decompiled}"
        );

        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["rexenv.app"], "the root script's source must not outlive the compile");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);

        remove(&dir).unwrap();
        assert!(!dir.exists());
    }

    /// `build` replaces its directory; a symlink planted at that path must lose the
    /// LINK, never the contents of what it points to.
    #[test]
    fn a_symlink_at_the_work_dir_is_removed_without_touching_its_target() {
        let dir = scratch("link");
        let target = scratch("link-target");
        let _ = remove(&dir);
        let _ = remove(&target);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep"), "theirs").unwrap();
        std::os::unix::fs::symlink(&target, &dir).unwrap();

        remove(&dir).unwrap();

        assert!(std::fs::symlink_metadata(&dir).is_err(), "the link is gone");
        assert_eq!(std::fs::read_to_string(target.join("keep")).unwrap(), "theirs");
        remove(&target).unwrap();
    }
}
