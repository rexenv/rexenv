fn main() {
    // The `rex` sidecar (bundle.externalBin) must EXIST before tauri_build
    // validates the config — otherwise a fresh clone's bare `cargo build` /
    // `cargo test` dies before compiling a line. `tauri dev`/`tauri build`
    // stage it via beforeDevCommand/beforeBuildCommand anyway; this covers
    // every other entry point. The cli crate has its own target dir, so the
    // nested cargo can't deadlock this build; after the first run it's a
    // cache hit.
    #[cfg(target_os = "macos")]
    if !std::path::Path::new("binaries/rex-aarch64-apple-darwin").exists() {
        let ok = std::process::Command::new("sh")
            .arg("../scripts/build-cli.sh")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            println!("cargo:warning=could not stage the rex CLI sidecar (scripts/build-cli.sh)");
        }
    }
    stamp_build_identity();
    tauri_build::build()
}

/// Stamp the git commit and build time into the binary.
///
/// A running app that can't say which source it was built from costs real time:
/// a user clicked Retry in a binary 40 minutes older than the fix and the
/// symptom looked like a logic bug rather than a stale build. Two env vars make
/// that question instant, in the app's About panel and in `rex version`.
///
/// Degrades honestly — a tarball with no git checkout stamps "unknown" rather
/// than failing the build.
fn stamp_build_identity() {
    // Rebuild the stamp when HEAD moves; harmless if .git isn't there.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");

    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    // A dirty tree is worth knowing: "does the binary match the commit" is the
    // actual question, and an uncommitted change makes the sha a half-truth.
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
    println!("cargo:rustc-env=REXENV_GIT_COMMIT={commit}");

    let built = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=REXENV_BUILT_AT={built}");
}
