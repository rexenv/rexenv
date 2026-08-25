//! Stamp the CLI with the commit it was built from.
//!
//! **`rex` and the app ship together, so "are these the same build?" is the
//! question a version number cannot answer.** Both carry `0.3.0` for a whole
//! release cycle, while the case that actually keeps happening is a rebuilt CLI
//! talking to an app still running an older binary — same version, different
//! commit. Measured 23 Aug 2026: `rex site relink` against an app built from
//! `460901c` returned "unknown command" and the version check stayed silent,
//! because both said 0.3.0.
//!
//! Deliberately the SAME shape as `src-tauri/build.rs` (short sha, `-dirty`
//! suffix, `unknown` when git cannot answer) so the two stamps are comparable
//! as strings. A different format here would make every comparison a mismatch.
fn main() {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    // A dirty tree is worth knowing: an uncommitted change makes the sha a
    // half-truth, and two `-dirty` builds are not evidence of being the same.
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    let commit = match sha {
        Some(s) if dirty => format!("{s}-dirty"),
        Some(s) => s,
        None => "unknown".to_string(),
    };
    println!("cargo:rustc-env=REX_GIT_COMMIT={commit}");

    // Build time, same shape as `src-tauri/build.rs`, so `rex version` can print
    // the app's stamp and the CLI's side by side and they read as one pair.
    let built = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=REX_BUILT_AT={built}");
    println!("cargo:rerun-if-changed=.git/HEAD");
}
