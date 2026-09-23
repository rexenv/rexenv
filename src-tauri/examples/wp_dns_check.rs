//! Live check for the loopback-DNS mu-plugin (`core::wp_dns`).
//! Run: `cargo run --example wp_dns_check`
//!
//! # What this proves that the unit tests cannot
//!
//! The unit tests can only assert that the mu-plugin CONTAINS its guards. The
//! bug it fixes lives entirely outside Rust: the static-php.dev builds link
//! libcurl against **c-ares**, which reads `/etc/resolv.conf` alone and never
//! macOS split-DNS (`/etc/resolver/<tld>`). So inside rexenv's PHP,
//! `gethostbyname("x.rex")` answered `127.0.0.1` while `curl` on the same host
//! died with errno 6 — and WP-Cron, which spawns itself with exactly such a
//! request and never checks the result, stopped without a single log line.
//!
//! This check runs the REAL mu-plugin source under the REAL bundled PHP and
//! makes a REAL request, twice:
//!
//!   1. baseline (no plugin)  → expected: cURL error 6, the bug reproduced;
//!   2. plugin applied        → expected: HTTP 200, the fix working.
//!
//! Which of those two is expected comes from `core::wp_dns::resolver_for`, and
//! a DISAGREEMENT fails the run. It used to be a note — a threaded build printed
//! "the bug class is gone" and exited 0 — which `docs/TODO.md` called the signal
//! that this work was done. A line in a log nobody reads, on a run that passed,
//! is a hope and not a control.
//!
//! The gate is deliberately not "fail when this build is threaded": **7.4
//! already is** (measured 23 Aug 2026 — it is the build rexenv makes itself and
//! it never got static-php.dev's `--enable-cares`), so that rule would fail on
//! good news. It fails on DIVERGENCE from the record, which is news in either
//! direction — a minor that gained the threaded resolver means the mu-plugin is
//! dead weight for it, and one that gained c-ares means a machine is running the
//! bug while the workaround is believed unnecessary.
//!
//! # Fixture scope (the examples invariant)
//!
//! Writes ONLY into a temp dir this run created; serves its own listener on a
//! fixture port; spawns only a short-lived `php` child. It touches no site, no
//! app-data path, and no system file. It does depend on two things it does not
//! own and does not modify: the always-on rexenv DNS answering `*.rex`, and the
//! shared binary cache (the documented exception) for the `php` binary.

use rexenv_lib::core::{binaries, wp_dns};
use rexenv_lib::platform;
use std::io::{Read, Write};
use std::net::TcpListener;

/// Fixture port (docs/PORTS.md band for examples) — never a production port.
const FIXTURE_PORT: u16 = 18101;

/// A `.rex` host that belongs to no site: rexenv's DNS answers the whole TLD
/// with `127.0.0.1`, which is the point — the plugin must fix ANY rexenv host,
/// not one it was told about.
const PROBE_HOST: &str = "rexenv-dns-probe.rex";

/// The PHP harness: WordPress' `http_api_curl` hook, reduced to the one line
/// that matters — capture the callback the mu-plugin registers, then make the
/// same request with and without it.
const HARNESS: &str = r#"<?php
$GLOBALS['cb'] = null;
function add_action($hook, $fn, $prio = 10, $args = 1) {
    if ($hook === 'http_api_curl') { $GLOBALS['cb'] = $fn; }
}
require $argv[1];
$url = $argv[2];

$v = curl_version();
echo "ares=", (empty($v['ares']) ? '-' : $v['ares']), "\n";
echo "gethostbyname=", gethostbyname(parse_url($url, PHP_URL_HOST)), "\n";
echo "callback=", ($GLOBALS['cb'] ? 'yes' : 'no'), "\n";

// The negative branch: which /etc/resolver zones this may touch at all.
$cases = [
    'ours'    => "nameserver 127.0.0.1\nport 15353\n",
    'ours_v6' => "nameserver ::1\nport 15353\n",
    'vpn'     => "domain corp.internal\nnameserver 10.8.0.1\n",
    'lan'     => "nameserver 192.168.1.1\n",
    'empty'   => "",
];
foreach ($cases as $name => $conf) {
    echo "resolver_", $name, "=", rexenv_dns_resolver_is_loopback($conf) ? '1' : '0', "\n";
}
echo "public_host=", var_export(rexenv_dns_loopback_ip('example.com'), true), "\n";

foreach ([false, true] as $apply) {
    $ch = curl_init($url);
    curl_setopt_array($ch, [CURLOPT_RETURNTRANSFER => 1, CURLOPT_TIMEOUT => 10]);
    if ($apply && $GLOBALS['cb']) { ($GLOBALS['cb'])($ch, [], $url); }
    curl_exec($ch);
    echo ($apply ? 'plugin' : 'baseline'), "=", curl_errno($ch), ",",
        curl_getinfo($ch, CURLINFO_HTTP_CODE), ",", curl_error($ch), "\n";
}
"#;

#[tokio::main]
async fn main() {
    // 1) Fixture docroot — ours, created here, removed at the end.
    let dir = std::env::temp_dir().join(format!("rexenv-wp-dns-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");

    // 2) The mu-plugin, written by the PRODUCTION writer — not a copy.
    wp_dns::ensure(&dir, "wp-content").expect("write the mu-plugin");
    let plugin = dir.join("wp-content/mu-plugins/rexenv-dns.php");
    assert!(plugin.is_file(), "ensure() wrote nothing");
    let harness = dir.join("harness.php");
    std::fs::write(&harness, HARNESS).expect("write the harness");

    // 3) A listener of our own on a fixture port: two 200s, then done.
    let listener = TcpListener::bind(("127.0.0.1", FIXTURE_PORT))
        .unwrap_or_else(|e| panic!("bind 127.0.0.1:{FIXTURE_PORT}: {e}"));
    let server = std::thread::spawn(move || {
        for stream in listener.incoming().take(2) {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });

    // 4) The bundled PHP — the build the bug lives in.
    let plat = platform::current();
    let php = binaries::resolve(&*plat, "php", binaries::pins().php)
        .await
        .expect("resolve the bundled php");
    let url = format!("http://{PROBE_HOST}:{FIXTURE_PORT}/");
    let out = std::process::Command::new(&php)
        .arg(&harness)
        .arg(&plugin)
        .arg(&url)
        .output()
        .expect("run php");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    print!("{text}");
    if !out.stderr.is_empty() {
        eprintln!("php stderr: {}", String::from_utf8_lossy(&out.stderr));
    }

    let field = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .unwrap_or_default()
            .to_string()
    };
    let errno = |k: &str| -> i32 {
        field(k).split(',').next().unwrap_or("-1").parse().unwrap_or(-1)
    };
    let code = |k: &str| -> i32 {
        field(k).split(',').nth(1).unwrap_or("0").parse().unwrap_or(0)
    };

    let mut ok = true;
    let mut check = |cond: bool, what: &str| {
        println!("{} {what}", if cond { "PASS" } else { "FAIL" });
        ok &= cond;
    };

    check(field("callback") == "yes", "the mu-plugin registers an http_api_curl callback");
    // The system resolver must answer, or nothing below means anything: this is
    // the always-on DNS agent, not something this check owns.
    let resolved = field("gethostbyname");
    check(
        resolved == "127.0.0.1",
        &format!("rexenv DNS resolves {PROBE_HOST} via getaddrinfo (got {resolved:?}) — \
                  if this fails, the DNS agent is down, not the plugin"),
    );

    // The blast radius of the fix, in both directions: it must cover every
    // loopback-served zone and touch nothing else. A VPN's split-DNS rerouted
    // to 127.0.0.1 would break a working setup to fix a local one.
    check(field("resolver_ours") == "1", "a loopback-served zone (ours) is fixed");
    check(field("resolver_ours_v6") == "1", "an ::1-served zone is fixed too");
    for foreign in ["vpn", "lan", "empty"] {
        check(
            field(&format!("resolver_{foreign}")) == "0",
            &format!("a {foreign} resolver zone is left alone"),
        );
    }
    check(field("public_host") == "NULL", "a public host has no resolver file → untouched");

    // ── The resolver this build MEASURES as, against what rexenv RECORDS ────
    //
    // This branch used to be a note: on a threaded build it printed "the bug
    // class is gone" and exited 0 green. `docs/TODO.md` called that the signal
    // that the bug class was eliminated, which made it a hope rather than a
    // control — a line in a log nobody reads, on a run that passed.
    //
    // It is a GATE now, and the decision the row asked for. Not "fail when the
    // build is threaded" — 7.4 already is, so that would fail on good news —
    // but "fail when the build DISAGREES with `core::wp_dns::resolver_for`".
    // Divergence in either direction is news: a minor that gained the threaded
    // resolver means the mu-plugin can go for it, and one that gained c-ares
    // means a machine is running the workaround's bug with the workaround
    // believed unnecessary.
    let measured = if field("ares") == "-" {
        wp_dns::CurlResolver::Threaded
    } else {
        wp_dns::CurlResolver::Ares
    };
    let minor = binaries::pins().php
        .rsplit_once('.')
        .map(|(m, _)| m)
        .unwrap_or(binaries::pins().php);
    let recorded = wp_dns::resolver_for(minor);
    check(
        recorded == Some(measured),
        &format!(
            "PHP {minor}'s libcurl matches what core records: measured {measured:?}, \
             recorded {recorded:?} — if these disagree, fix `core::wp_dns::resolver_for` \
             and re-cost the c-ares row in docs/TODO.md; a build that turned threaded \
             means the mu-plugin is dead weight for this minor"
        ),
    );

    match measured {
        wp_dns::CurlResolver::Threaded => {
            println!(
                "NOTE: PHP {minor}'s libcurl uses the THREADED resolver — no bug class here, \
                 and the mu-plugin no-ops by its first guard."
            );
            check(code("baseline") == 200, "baseline request succeeds (no bug to fix)");
            check(code("plugin") == 200, "the plugin changes nothing on a threaded build");
        }
        wp_dns::CurlResolver::Ares => {
            check(
                errno("baseline") == 6,
                "BUG REPRODUCED: unaided cURL cannot resolve a .rex host (errno 6)",
            );
            check(code("plugin") == 200, "FIXED: with the mu-plugin the same request returns 200");
        }
    }

    // Drain the listener even if a request never arrived, then clean up OURS.
    let _ = std::net::TcpStream::connect(("127.0.0.1", FIXTURE_PORT));
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);

    println!("\n{}", if ok { "wp_dns_check: all green" } else { "wp_dns_check: FAILED" });
    if !ok {
        std::process::exit(1);
    }
}
