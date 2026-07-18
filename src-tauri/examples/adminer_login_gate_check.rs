//! Live-check (B1): prove the Adminer passwordless-login gate rejects
//! non-loopback hosts while still accepting every real loopback form — by
//! running the EXACT gate shipped in `core::adminer::WRAPPER_INDEX_PHP` through a
//! real PHP interpreter. Read-only and side-effect-free: it defines one PHP
//! function and evaluates it over a fixed matrix — spawns no services, binds no
//! ports, touches no app state.
//!
//! Run: `cargo run --example adminer_login_gate_check`
//! PHP is located as: `$REXENV_PHP_BIN` → a bundled `<app_data>/bin/php-*/php`
//! → `php` on PATH. If none is found the check SKIPS (exit 0) rather than lies.
//!
//! Background: `\Adminer\SERVER` is request-controlled, so the gate MUST match the
//! target host against a loopback allow-list EXACTLY. The old prefix test
//! (`strpos(SERVER, '127.0.0.1') === 0`) accepted `127.0.0.1.evil.com` and handed
//! a passwordless session to a REMOTE server (→ `LOAD DATA LOCAL INFILE` file read).

use rexenv_lib::core::adminer::WRAPPER_INDEX_PHP;
use rexenv_lib::platform;
use std::path::PathBuf;
use std::process::Command;

/// Pull the loopback-gate function verbatim from the shipped wrapper (the lines
/// strictly between the two `rexenv-loopback-gate` markers), so this check can
/// never drift from what actually ships.
fn extract_gate() -> String {
    let mut lines = WRAPPER_INDEX_PHP.lines();
    for l in lines.by_ref() {
        if l.contains("rexenv-loopback-gate:start") {
            break;
        }
    }
    let mut out = Vec::new();
    for l in lines {
        if l.contains("rexenv-loopback-gate:end") {
            break;
        }
        out.push(l);
    }
    let gate = out.join("\n");
    assert!(
        gate.contains("function rexenv_is_loopback("),
        "could not extract the loopback gate from WRAPPER_INDEX_PHP (markers moved?)"
    );
    gate
}

/// A PHP interpreter to prove the gate with. Prefers a bundled rexenv PHP (the
/// runtime family this actually runs under in production).
fn find_php() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("REXENV_PHP_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(bin) = platform::current().paths().app_data_dir().map(|d| d.join("bin")) {
        if let Ok(rd) = std::fs::read_dir(&bin) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("php-") {
                    let cand = e.path().join("php");
                    if cand.exists() {
                        return Some(cand);
                    }
                }
            }
        }
    }
    if Command::new("php")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some(PathBuf::from("php"));
    }
    None
}

fn main() {
    // Every host rexenv itself puts in a deep-link (127.0.0.1:<port>) plus the
    // other genuine loopback forms a manual login might use.
    let allow = [
        "127.0.0.1",
        "127.0.0.1:13306",
        "127.0.0.1:13307",
        "127.0.0.1:15432",
        "localhost",
        "localhost:13306",
        "localhost:/tmp/mysql.sock",
        "LOCALHOST",
        "::1",
        "[::1]",
        "[::1]:13306",
    ];
    // The B1 bypass vectors + plain non-loopback hosts — all must be refused.
    let deny = [
        "127.0.0.1.evil.com",
        "127.0.0.1.evil.com:3306",
        "localhost.evil.com",
        "evil.com",
        "127.0.0.1@evil.com",
        "",
        "10.0.0.5",
        "127.0.0.1x",
        "foo127.0.0.1",
        "127.0.0.1.evil.com:3306/x",
    ];

    let Some(php) = find_php() else {
        eprintln!(
            "SKIP: no PHP interpreter found (set REXENV_PHP_BIN, start the app once so a \
             bundled PHP is cached, or put php on PATH). The gate was NOT exercised."
        );
        return; // exit 0 — skipped, not failed
    };

    let harness = format!(
        "<?php\n{gate}\nforeach (array_slice($argv, 1) as $s) {{ echo (rexenv_is_loopback($s) ? \
         'ALLOW' : 'DENY'), \"\\n\"; }}\n",
        gate = extract_gate()
    );
    let tmp = std::env::temp_dir().join("rexenv_adminer_gate_check.php");
    std::fs::write(&tmp, &harness).expect("write harness");

    let mut cases: Vec<(&str, bool)> = Vec::new();
    for c in allow {
        cases.push((c, true));
    }
    for c in deny {
        cases.push((c, false));
    }

    let mut cmd = Command::new(&php);
    cmd.arg(&tmp);
    for (c, _) in &cases {
        cmd.arg(c);
    }
    let out = cmd.output().expect("run php");
    let _ = std::fs::remove_file(&tmp);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let results: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        results.len(),
        cases.len(),
        "php returned {} lines for {} cases; stderr:\n{}\nstdout:\n{}",
        results.len(),
        cases.len(),
        String::from_utf8_lossy(&out.stderr),
        stdout
    );

    println!("Adminer loopback gate — proven through {}\n", php.display());
    let mut fails = 0;
    for ((server, expect_allow), got) in cases.iter().zip(results.iter()) {
        let got_allow = *got == "ALLOW";
        let ok = got_allow == *expect_allow;
        if !ok {
            fails += 1;
        }
        println!(
            "  got={:5} expect={:5} {}  {:?}",
            got,
            if *expect_allow { "ALLOW" } else { "DENY" },
            if ok { "ok  " } else { "FAIL" },
            server
        );
    }
    if fails == 0 {
        println!("\nALL PASS ({} cases) — exact-match gate holds.", cases.len());
    } else {
        eprintln!("\n{fails} FAILURE(S) — the loopback gate regressed.");
        std::process::exit(1);
    }
}
