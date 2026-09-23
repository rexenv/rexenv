//! Manual check for the FrankenPHP per-site backend (Phase 2 task 2.2).
//! Run: `cargo run --example frankenphp_serve`
//!
//! Writes a FrankenPHP config for one docroot, starts it on an internal loopback
//! HTTP port (embedded PHP, auto_https + admin OFF), curls it directly, and asserts:
//!   - HTTP 200 with phpinfo served by FrankenPHP's EMBEDDED PHP (not a php-fpm pool);
//!   - it did NOT bind the edge's admin port :2019 (proves `admin off`).
//!
//! No edge/TLS here — that's §2.3. Cleans up at the end.

use rexenv_lib::core::services::RewriteMode;
use rexenv_lib::core::{binaries, frankenphp, ports};
use rexenv_lib::platform;
use std::process::Command;

mod common;

const PORT: u16 = frankenphp::FRANKENPHP_BASE_PORT; // 8200
const DOT_SECRET: &str = "REXENV_FP_DOT_7a31";
const DOMAIN: &str = "fp.test";

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();

    // A throwaway docroot with a phpinfo() page.
    let docroot = std::env::temp_dir().join("rexenv-fp-docroot");
    std::fs::create_dir_all(&docroot).unwrap();
    std::fs::write(docroot.join("index.php"), "<?php phpinfo();\n").unwrap();
    // #103's FrankenPHP leg. The guard is in the generated config
    // (`@dot_root` / nested-dot matchers + `respond 404`, ordered before
    // `php_server`) and unit-tested as a string; this is the only place it is
    // exercised over the wire on this backend. Closes the last open leg on #103.
    std::fs::create_dir_all(docroot.join(".git")).unwrap();
    std::fs::create_dir_all(docroot.join(".hidden")).unwrap();
    std::fs::create_dir_all(docroot.join(".well-known")).unwrap();
    std::fs::write(docroot.join(".env"), format!("APP_KEY={DOT_SECRET}\n")).unwrap();
    std::fs::write(docroot.join(".git/config"), "[core]\n").unwrap();
    std::fs::write(docroot.join(".hidden/x.php"), "<?php echo 'DOTPHP-RAN';").unwrap();
    std::fs::write(docroot.join(".well-known/probe"), "well-known-ok").unwrap();

    if let Err(e) = ports::ensure_free(&*plat, PORT, ports::Proto::Tcp, "FrankenPHP") {
        eprintln!("port {PORT} busy: {e}");
        // `FAILURE` rather than `exit(1)`: exit runs no destructors (common/mod.rs,
        // the verdict contract).
        return std::process::ExitCode::FAILURE;
    }

    // Read :2019 BEFORE the spawn — see the assertion below.
    let admin_before = ports::is_listening(2019);
    if admin_before {
        println!("NOTE: something already holds :2019 (not ours) — the admin check reads the delta");
    }
    let bin = binaries::resolve(&*plat, "frankenphp", binaries::pins().frankenphp).await.unwrap();
    let conf = frankenphp::write_config(&*plat, DOMAIN, &docroot, PORT, RewriteMode::Single, &[], None).unwrap();
    // Drop-GUARDED: the readiness gate on the next line PANICS on timeout, and a
    // raw `Child` is not killed by an unwind — so the failing run would leave
    // FrankenPHP holding :8200 and the next run would fail its own
    // `ensure_free` above, blaming the port instead of the leak.
    let mut child = common::OwnedService::new(
        frankenphp::start(&*plat, &bin, DOMAIN, &conf, &[]).expect("start frankenphp"),
        "frankenphp",
    );
    // Gate on the sockets, not the clock (`common::await_listening` carries the
    // incident): every spawn helper here returns at fork, not at bind.
    common::await_listening(PORT, "frankenphp", None);

    let listening = frankenphp::running(PORT);
    // The TRANSITION, not the absolute. `is_listening(2019)` asks whether
    // ANYTHING holds Caddy's admin port — and a developer running their own
    // Caddy holds it all day, which failed this run for a reason that has
    // nothing to do with rexenv. What the claim is actually about is whether
    // OUR FrankenPHP opened it, so the reading is taken before the spawn and the
    // assertion is "we did not add one".
    let admin_bound = !admin_before && ports::is_listening(2019); // must be false — admin is off

    let code = Command::new("curl").args(["--max-time", "60"])
        .args(["-s", "-H", &format!("Host: {DOMAIN}"), "-o", "/dev/null", "-w", "%{http_code}",
               &format!("http://127.0.0.1:{PORT}/")])
        .output().expect("curl").stdout;
    let code = String::from_utf8_lossy(&code).trim().to_string();
    let body = Command::new("curl").args(["--max-time", "60"])
        .args(["-s", "-H", &format!("Host: {DOMAIN}"), &format!("http://127.0.0.1:{PORT}/")])
        .output().expect("curl").stdout;
    let body = String::from_utf8_lossy(&body);
    let php_ver = body.split("PHP Version ").nth(1)
        .and_then(|s| s.split('<').next()).map(|s| s.trim().to_string()).unwrap_or_default();

    println!("listening on :{PORT} = {listening}");
    println!("http={code}  embedded PHP reported = {php_ver}");
    println!(":2019 admin bound = {admin_bound}  (must be false — admin off)");

    // #103, FrankenPHP backend — the same four probes nginx and Apache run.
    // The body is checked for the SECRET's absence, not just the status: a 404
    // page that echoed the request would still serve it. And /.well-known/ must
    // still be 200, or "deny every dot path" would satisfy the three denials
    // while breaking ACME — the fix someone reaches for first.
    let get = |path: &str| -> String {
        let out = Command::new("curl").args(["--max-time", "60"])
            .args(["-s", "-i", "-H", &format!("Host: {DOMAIN}"),
                   &format!("http://127.0.0.1:{PORT}{path}")])
            .output().expect("curl").stdout;
        String::from_utf8_lossy(&out).to_string()
    };
    let mut dot_ok = true;
    for (path, what) in [
        ("/.env", "the .env"),
        ("/.git/config", "the .git config"),
        ("/.hidden/x.php", "a dot-dir .php"),
    ] {
        let r = get(path);
        let blocked = r.starts_with("HTTP/1.1 404");
        let leaked = r.contains(DOT_SECRET) || r.contains("DOTPHP-RAN");
        if !blocked || leaked {
            println!("  ✗ {what} at {path}: 404={blocked} leaked={leaked}");
        }
        dot_ok &= blocked && !leaked;
    }
    let wk = get("/.well-known/probe");
    let wk_ok = wk.starts_with("HTTP/1.1 200") && wk.contains("well-known-ok");
    println!("dotfiles-404={dot_ok} · well-known-still-200={wk_ok}");

    // Cleanup. `stop()` is the guard's own idempotent shutdown, so this path and
    // the panic path tear down through the same code.
    child.stop();
    let _ = std::fs::remove_dir_all(&docroot);

    let ok = listening && code == "200" && php_ver.starts_with("8.") && !admin_bound && dot_ok && wk_ok;
    if ok {
        println!("\nOK — FrankenPHP backend serves embedded PHP on a loopback port, no edge/admin.");
        std::process::ExitCode::SUCCESS
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::ExitCode::FAILURE
    }
}
