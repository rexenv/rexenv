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
//! Also: the console's PALETTE. Adminer picks its scheme from what the
//! wrapper's `css()` returns, so the only proof is the head it EMITS — run in a
//! throwaway docroot this example creates, never the live console.
//!
//! Run: `cargo run --example adminer_check`

use rexenv_lib::core::{adminer, binaries};
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();

    let php = binaries::resolve(&*plat, "php", binaries::pins().php).await.expect("php");
    let adminer = binaries::resolve_file(&*plat, "adminer", binaries::pins().adminer)
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

    // ── The console's PALETTE follows rexenv, not the OS ────────────────────
    //
    // Adminer decides its scheme from what the wrapper's `css()` returns, so
    // this is only provable by RUNNING it: the head it emits is the whole
    // answer, and no amount of reading the wrapper says what Adminer did with
    // it. Three states, in a throwaway docroot this run creates — never the
    // live console, which is serving.
    let dir = std::env::temp_dir().join(format!("rexenv-adminer-theme-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("throwaway docroot");
    std::fs::copy(&adminer, dir.join(".adminer.php")).expect("stage adminer");
    std::fs::write(dir.join("index.php"), adminer::wrapper_index_php()).expect("stage wrapper");
    std::fs::write(dir.join("rexenv-theme.css"), "/* checked by adminer_check */\n").unwrap();

    let head_for = |want: Option<&str>| -> String {
        let marker = dir.join(".rexenv-theme");
        match want {
            Some(t) => std::fs::write(&marker, t).unwrap(),
            None => {
                let _ = std::fs::remove_file(&marker);
            }
        }
        let out = std::process::Command::new(&php)
            .arg("-d")
            .arg("error_reporting=0")
            .arg("index.php")
            .current_dir(&dir)
            .output()
            .expect("run the console");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // The media query is the tell: WITH it, the browser decides; without it,
    // Adminer has been told. `color-scheme` is the second half — it is what
    // makes form controls and scrollbars follow too.
    let dark_media = "media='(prefers-color-scheme: dark)'";

    let os_default = head_for(None);
    assert!(
        os_default.contains(dark_media) && os_default.contains("content='light dark'"),
        "no theme file must leave Adminer's OWN behaviour untouched — it did not:\n{os_default:.400}"
    );

    let dark = head_for(Some("dark"));
    assert!(
        dark.contains("content='dark'") && !dark.contains(dark_media),
        "dark: the console still asks the OS instead of taking rexenv's answer:\n{dark:.400}"
    );
    assert!(dark.contains("dark.css"), "dark: the dark stylesheet is not loaded at all");

    let light = head_for(Some("light"));
    assert!(
        light.contains("content='light'"),
        "light: the console did not commit to light:\n{light:.400}"
    );
    assert!(
        !light.contains("dark.css"),
        "light: the dark stylesheet is still linked — the OS can still win:\n{light:.400}"
    );

    // Garbage in the file must degrade to Adminer's own behaviour, never to a
    // console with no stylesheet: a half-written file is a real state (the
    // writer renames, but nothing stops a user editing it).
    let junk = head_for(Some("moonlight"));
    assert!(
        junk.contains(dark_media) && junk.contains("content='light dark'"),
        "an unreadable theme file must fall back to Adminer's default:\n{junk:.400}"
    );
    println!("✓ palette follows rexenv: dark forces dark, light drops dark.css, junk falls back");

    // Fixture-owned, by name.
    let _ = std::fs::remove_dir_all(&dir);

    println!(
        "\nALL GOOD — Adminer resolves via resolve_file, passes php -l, binds to the wrapper, and takes its palette from rexenv."
    );
}
