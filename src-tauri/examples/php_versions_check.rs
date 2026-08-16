//! Manual check for the MULTI-VERSION PHP binary provider (Phase 2 task 1.1).
//! Run: `cargo run --example php_versions_check`
//!
//! For every version in `binaries::PHP_VERSIONS`, this downloads + verifies +
//! extracts + signs the pinned `php` (cli) and `php-fpm` builds under app-data
//! (reusing the Phase-1 BinaryProvider + prepare_binary path), then ASSERTS what
//! it got. Each version caches independently under `bin_dir/php-<version>/` and
//! `bin_dir/php-fpm-<version>/`.
//!
//! ## What changed here, and why it matters
//!
//! This used to PRINT `php -v`'s first line and check only the exit status. A
//! binary that ran fine but was the WRONG VERSION passed — so the check that
//! looks like it proves "the 8.1 pin really is 8.1" proved only "some php ran".
//! That is the shape this repo keeps getting bitten by: an assertion and the
//! claim it stands for pointing at different things. It matters more now that
//! PHP versions come from more than one source (`php_url`): a self-hosted
//! artifact is one filename typo away from being the wrong build, and the
//! checksum cannot catch that — it only proves the bytes are the ones we pinned,
//! not that we pinned the right ones.
//!
//! So each version now asserts:
//!   * `php -v` and `php-fpm -v` REPORT the pinned version,
//!   * the build is usable for WordPress — `mysqli` present, which is the whole
//!     reason `core/binaries.rs` pins the "bulk" builds rather than "common",
//!     a claim nothing checked until now,
//!   * the Mach-O matches the machine's own architecture.

use rexenv_lib::core::binaries;
use rexenv_lib::platform;
use rexenv_lib::platform::traits::Arch;
use std::path::Path;
use std::process::Command;

/// At least this many licence files must sit beside a binary rexenv BUILT.
///
/// A FLOOR, not the exact count (15 today): the set grows whenever the build
/// gains a statically linked dependency, and an exact assertion would be edited
/// to whatever was found rather than checked — which is how a guard becomes a
/// transcript of the current state.
const MIN_LICENCE_FILES: usize = 10;

/// Problems with the licence texts beside `bin`, or empty if there are none.
///
/// Returns empty for every build somebody ELSE distributes: asserting a
/// `licenses/` beside static-php.dev's 8.x would invent an obligation rexenv
/// does not have, and would fail on seven of the eight rows. The question of
/// who owes what is asked of `binaries::artifact_is_self_distributed`, so this example
/// cannot answer it differently from the code that acts on it.
///
/// L0 proves the pin exists and that a cache without the texts is stale. Only a
/// real resolve proves the archive fetches, unpacks where the code expects, and
/// survives the atomic publish — and the obligation is discharged by FILES next
/// to the binary, so files are what this looks at.
fn licence_problems(v: &str, bin: &Path, kind: &str, arch: Arch) -> Vec<String> {
    if !binaries::artifact_is_self_distributed(kind, v, arch) {
        return Vec::new();
    }
    let dir = bin.parent().unwrap().join(binaries::LICENSES_DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            return vec![format!(
                "{kind} {v}: rexenv built and distributes this interpreter, but there is no \
                 licences/ beside it ({}): {e}",
                dir.display()
            )]
        }
    };
    let names: Vec<String> =
        entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let mut problems = Vec::new();
    if names.len() < MIN_LICENCE_FILES {
        problems.push(format!(
            "{kind} {v}: {} licence file(s) in {}, expected at least {MIN_LICENCE_FILES}",
            names.len(),
            dir.display()
        ));
    }
    // The one text that is not optional: PHP's own.
    if !names.iter().any(|n| n.contains("PHP-3.01")) {
        problems.push(format!(
            "{kind} {v}: no PHP-3.01 licence beside the binary (found: {names:?}). \
             §2 wants the notice with the distribution."
        ));
    }
    if problems.is_empty() {
        println!("  licences: {} files incl. PHP-3.01", names.len());
    }
    problems
}

/// First line of `<bin> -v`, or an error string.
fn version_line(bin: &Path) -> Result<String, String> {
    let out = Command::new(bin)
        .arg("-v")
        .output()
        .map_err(|e| format!("spawn failed: {e}"))?;
    if !out.status.success() {
        return Err(format!("-v exited {}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string())
}

fn main_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let mut failures: Vec<String> = Vec::new();
    let mut fail = |msg: String| {
        eprintln!("  ✗ {msg}");
        failures.push(msg);
    };

    for v in binaries::PHP_VERSIONS {
        println!("=== PHP {v} ===");

        // CLI: resolve, then prove it is THIS version and can serve WordPress.
        match binaries::resolve(&*plat, "php", v).await {
            Ok(path) => {
                println!("  php     cached at: {}", path.display());
                match version_line(&path) {
                    Ok(line) => {
                        println!("  {line}");
                        // The assertion the old check was missing.
                        if !line.contains(&format!("PHP {v}")) {
                            fail(format!("php {v}: reports `{line}` — not the pinned version"));
                        }
                    }
                    Err(e) => fail(format!("php {v}: {e}")),
                }

                // mysqli is WordPress's hard requirement and the stated reason
                // for pinning "bulk" over "common". Nothing checked it before.
                match Command::new(&path).arg("-m").output() {
                    Ok(out) => {
                        let mods = String::from_utf8_lossy(&out.stdout).to_lowercase();
                        for needed in ["mysqli", "curl", "mbstring", "openssl"] {
                            if !mods.lines().any(|l| l.trim() == needed) {
                                fail(format!("php {v}: `{needed}` missing from php -m"));
                            }
                        }
                    }
                    Err(e) => fail(format!("php {v}: php -m failed: {e}")),
                }

                // A binary for the other arch would run under Rosetta (or not at
                // all) and be a slow mystery rather than a loud failure.
                match Command::new("file").arg(&path).output() {
                    Ok(out) => {
                        let desc = String::from_utf8_lossy(&out.stdout);
                        if !desc.contains(main_arch()) {
                            fail(format!("php {v}: not {} — `{}`", main_arch(), desc.trim()));
                        }
                    }
                    Err(e) => fail(format!("php {v}: file(1) failed: {e}")),
                }

                for p in licence_problems(v, &path, "php", plat.binaries().arch()) {
                    fail(p);
                }
            }
            Err(e) => fail(format!("php {v}: resolve failed: {e}")),
        }

        // FPM: the same version, proven separately — cli and fpm are DISTINCT
        // artifacts with distinct checksums, so one being right says nothing
        // about the other.
        match binaries::resolve(&*plat, "php-fpm", v).await {
            Ok(path) => {
                println!("  php-fpm cached at: {}", path.display());
                match version_line(&path) {
                    Ok(line) => {
                        println!("  {line}");
                        if !line.contains(&format!("PHP {v}")) {
                            fail(format!("php-fpm {v}: reports `{line}` — not the pinned version"));
                        }
                    }
                    Err(e) => fail(format!("php-fpm {v}: {e}")),
                }
                // Separately, for the same reason the version is: `php-fpm-7.4.33`
                // is its own cache dir published by its own resolve, so the cli's
                // licences say nothing about it.
                for p in licence_problems(v, &path, "php-fpm", plat.binaries().arch()) {
                    fail(p);
                }
            }
            Err(e) => fail(format!("php-fpm {v}: resolve failed: {e}")),
        }
        println!();
    }

    if failures.is_empty() {
        println!(
            "OK — all {} PHP versions resolved, reported their pinned version, carry mysqli, and match {}.",
            binaries::PHP_VERSIONS.len(),
            main_arch()
        );
    } else {
        eprintln!("\nFAILED — {} problem(s):", failures.len());
        for f in &failures {
            eprintln!("  - {f}");
        }
        std::process::exit(1);
    }
}
