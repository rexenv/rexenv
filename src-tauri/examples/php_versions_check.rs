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
use std::path::Path;
use std::process::Command;

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
