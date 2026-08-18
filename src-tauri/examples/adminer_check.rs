//! Phase-3 §5.1 check: Adminer binary provider. Resolves `adminer.php` via
//! `BinaryProvider::resolve_file` (download + checksum-pin, no chmod/codesign —
//! it's a PHP script), then `php -l` on it from the bundled PHP passes — and
//! then the thing `php -l` cannot see: that this build still BINDS to rexenv's
//! wrapper (`adminer::verify_pair`).
//!
//! A syntactically perfect Adminer whose base class moved is a console with no
//! login gate and no frame protection, serving happily. Lint says nothing about
//! it; only running it does.
//!
//! Run: `cargo run --example adminer_check`

use rexenv_lib::core::{adminer, binaries};
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();

    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.expect("php");
    let adminer = binaries::resolve_file(&*plat, "adminer", binaries::ADMINER_VERSION)
        .await
        .expect("resolve adminer");
    println!("✓ resolved {}", adminer.display());
    assert!(adminer.file_name().unwrap() == "adminer.php", "cached as adminer.php");

    // It's a script, not a Mach-O — resolve_file must NOT have made it executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&adminer).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0, "adminer.php should not be executable (mode {mode:o})");
        println!("✓ not marked executable (mode {:o})", mode & 0o777);
    }

    // `php -l` (lint) from the bundled PHP passes.
    let out = std::process::Command::new(&php).arg("-l").arg(&adminer).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("php -l: {}", stdout.trim());
    assert!(out.status.success(), "php -l failed: {stderr}");
    assert!(stdout.contains("No syntax errors"), "unexpected lint output: {stdout}{stderr}");

    // The binding. `php -l` proves the file parses; this proves rexenv's
    // security controls still have something to hang on.
    adminer::verify_pair(&php, &adminer).expect("the pinned Adminer must bind to the wrapper");
    println!("✓ binds to rexenv's wrapper (class, four overrides, nonce())");

    println!(
        "\nALL GOOD — Adminer resolves via resolve_file, passes php -l, and binds to the wrapper."
    );
}
