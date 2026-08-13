//! Live-check (#307): prove the "Log in as" client-IP gate reads the LAST
//! `X-Forwarded-For` hop and not the first — by running the EXACT gate shipped in
//! `core::wp_login::MU_PLUGIN` through a real PHP interpreter. Read-only and
//! side-effect-free: it defines one PHP function and evaluates it over a fixed
//! matrix — spawns no services, binds no ports, touches no app state.
//!
//! Run: `cargo run --example wp_login_client_ip_check`
//! PHP is located as: `$REXENV_PHP_BIN` → a bundled `<app_data>/bin/php-*/php`
//! → `php` on PATH. If none is found the check SKIPS (exit 0) rather than lies.
//!
//! Background: Cloudflare APPENDS the connecting IP to `X-Forwarded-For` rather
//! than replacing it, so a request that sets the header arrives at the origin as
//! `127.0.0.1,<real IP>`. The gate took `explode(',', $xff)[0]` — the attacker's
//! entry — until 14 Aug 2026. The header shapes below marked "measured" are
//! verbatim from a real quick tunnel (CLAIM-LEDGER #307), not invented.
//!
//! The bug was found through the tunnel but is NOT tunnel-only: the edge binds
//! every interface, so a LAN caller sending `X-Forwarded-For: 127.0.0.1` passed
//! the gate with no tunnel and no Cloudflare anywhere in the request.

use rexenv_lib::core::wp_login::MU_PLUGIN;
use rexenv_lib::platform;
use std::path::PathBuf;
use std::process::Command;

/// Pull the client-IP gate verbatim from the shipped mu-plugin (the lines
/// strictly between the two `rexenv-client-ip-gate` markers), so this check can
/// never drift from what actually ships.
fn extract_gate() -> String {
    let mut lines = MU_PLUGIN.lines();
    for l in lines.by_ref() {
        if l.contains("rexenv-client-ip-gate:start") {
            break;
        }
    }
    let mut out = Vec::new();
    for l in lines {
        if l.contains("rexenv-client-ip-gate:end") {
            break;
        }
        out.push(l);
    }
    let gate = out.join("\n");
    // The markers could wander onto the wrong lines and still yield PHP that
    // runs; require the two statements the verdict actually depends on.
    assert!(
        gate.contains("$loopback") && gate.contains("HTTP_X_FORWARDED_FOR"),
        "could not extract the client-IP gate from MU_PLUGIN (markers moved?):\n{gate}"
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

struct Case {
    /// `None` = the header is absent entirely (not present-but-empty).
    xff: Option<&'static str>,
    remote: &'static str,
    allow: bool,
    why: &'static str,
}

/// `REMOTE_ADDR` as PHP sees it behind the shared nginx: always the edge's own
/// address, which is why the header is consulted at all.
const LOOPBACK: &str = "127.0.0.1";

fn matrix() -> Vec<Case> {
    vec![
        Case { xff: None, remote: LOOPBACK, allow: true,
               why: "local, no XFF at all" },
        Case { xff: Some("127.0.0.1"), remote: LOOPBACK, allow: true,
               why: "local through the edge — Caddy appended its loopback peer" },
        Case { xff: Some("203.0.113.7, 127.0.0.1"), remote: LOOPBACK, allow: true,
               why: "local client that sent its OWN XFF — DENIED before the fix (false deny)" },
        Case { xff: Some("127.0.0.1,103.209.197.170"), remote: LOOPBACK, allow: false,
               why: "measured: tunnel replay with a spoofed header — ALLOWED before the fix (#307)" },
        Case { xff: Some("103.209.197.170"), remote: LOOPBACK, allow: false,
               why: "measured: tunnel, caller sent nothing — denied before the fix too" },
        Case { xff: Some("127.0.0.1, 192.168.1.50"), remote: LOOPBACK, allow: false,
               why: "LAN caller spoofing loopback — ALLOWED before the fix, no tunnel involved" },
        Case { xff: Some("192.168.1.50"), remote: LOOPBACK, allow: false,
               why: "LAN caller, no spoof" },
        Case { xff: Some("127.0.0.1, 10.0.0.5, 127.0.0.1"), remote: LOOPBACK, allow: true,
               why: "last hop is still ours with junk in the middle" },
        Case { xff: Some("::1"), remote: "::1", allow: true,
               why: "IPv6 loopback" },
        Case { xff: Some("127.9.9.9"), remote: LOOPBACK, allow: true,
               why: "the 127./8 rule, unchanged by this fix" },
        Case { xff: Some("  ,  "), remote: LOOPBACK, allow: true,
               why: "every entry empty → falls back to REMOTE_ADDR" },
        Case { xff: Some(""), remote: "203.0.113.7", allow: false,
               why: "empty header, non-loopback REMOTE_ADDR → the fallback still denies" },
        Case { xff: None, remote: "203.0.113.7", allow: false,
               why: "no header, non-loopback REMOTE_ADDR" },
    ]
}

/// Properties of the MATRIX, not of the gate — checked before PHP runs.
///
/// A matrix can pass while proving nothing. These three make the degenerate
/// implementations impossible to satisfy, so the table stays honest even if
/// nobody ever re-runs the plants by hand:
///   - a gate that always allows, or always denies, fails on sheer counts;
///   - a gate reading the FIRST hop cannot produce this table, because allow and
///     deny cases share a leftmost entry;
///   - a gate reading only `REMOTE_ADDR` cannot either, for the same reason.
fn assert_matrix_discriminates(cases: &[Case]) {
    let allows = cases.iter().filter(|c| c.allow).count();
    let denies = cases.len() - allows;
    assert!(allows >= 4 && denies >= 4, "matrix is lopsided: {allows} allow / {denies} deny");

    let leftmost = |c: &Case| -> String {
        c.xff
            .map(|x| x.split(',').next().unwrap_or("").trim().to_string())
            .unwrap_or_default()
    };
    let shared_leftmost = cases
        .iter()
        .filter(|c| c.allow)
        .any(|a| cases.iter().any(|d| !d.allow && leftmost(d) == leftmost(a)));
    assert!(
        shared_leftmost,
        "no allow-case and deny-case share a leftmost XFF entry — a gate reading the \
         FIRST hop (the #307 bug) would pass this matrix"
    );

    let shared_remote = cases
        .iter()
        .filter(|c| c.allow)
        .any(|a| cases.iter().any(|d| !d.allow && d.remote == a.remote));
    assert!(
        shared_remote,
        "no allow-case and deny-case share a REMOTE_ADDR — a gate ignoring XFF \
         entirely would pass this matrix"
    );
}

fn main() {
    let cases = matrix();
    assert_matrix_discriminates(&cases);

    let Some(php) = find_php() else {
        eprintln!(
            "SKIP: no PHP interpreter found (set REXENV_PHP_BIN, start the app once so a \
             bundled PHP is cached, or put php on PATH). The gate was NOT exercised."
        );
        return; // exit 0 — skipped, not failed
    };

    // $_SERVER is a superglobal, so the extracted gate reads what the function
    // sets. Each case starts from a clean $_SERVER — no leakage between rows.
    let harness = format!(
        "<?php\nfunction rexenv_case($present, $xff, $remote) {{\n\
         $_SERVER = array('REMOTE_ADDR' => $remote);\n\
         if ($present) {{ $_SERVER['HTTP_X_FORWARDED_FOR'] = $xff; }}\n\
         {gate}\n\
         return $loopback;\n}}\n\
         foreach (array_slice($argv, 1) as $spec) {{\n\
         $p = explode('|', $spec, 3);\n\
         echo (rexenv_case($p[0] === '1', $p[1], $p[2]) ? 'ALLOW' : 'DENY'), \"\\n\";\n}}\n",
        gate = extract_gate()
    );
    let tmp = std::env::temp_dir().join("rexenv_wp_login_client_ip_check.php");
    std::fs::write(&tmp, &harness).expect("write harness");

    let mut cmd = Command::new(&php);
    cmd.arg(&tmp);
    for c in &cases {
        cmd.arg(format!(
            "{}|{}|{}",
            if c.xff.is_some() { 1 } else { 0 },
            c.xff.unwrap_or(""),
            c.remote
        ));
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

    println!("wp_login client-IP gate — proven through {}\n", php.display());
    let mut fails = 0;
    for (c, got) in cases.iter().zip(results.iter()) {
        let got_allow = *got == "ALLOW";
        let ok = got_allow == c.allow;
        if !ok {
            fails += 1;
        }
        println!(
            "  got={:5} expect={:5} {}  XFF={:<34} REMOTE_ADDR={:<12} {}",
            got,
            if c.allow { "ALLOW" } else { "DENY" },
            if ok { "ok  " } else { "FAIL" },
            c.xff.map(|x| format!("{x:?}")).unwrap_or_else(|| "(absent)".into()),
            c.remote,
            c.why
        );
    }
    if fails == 0 {
        println!(
            "\nALL PASS ({} cases) — the gate reads the last hop, and the matrix \
             discriminates against first-hop and REMOTE_ADDR-only reads.",
            cases.len()
        );
    } else {
        eprintln!("\n{fails} FAILURE(S) — the client-IP gate regressed (#307).");
        std::process::exit(1);
    }
}
