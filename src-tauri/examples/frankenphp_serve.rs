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
use std::thread;
use std::time::Duration;

const PORT: u16 = frankenphp::FRANKENPHP_BASE_PORT; // 8200
const DOMAIN: &str = "fp.test";

#[tokio::main]
async fn main() {
    let plat = platform::current();

    // A throwaway docroot with a phpinfo() page.
    let docroot = std::env::temp_dir().join("rexenv-fp-docroot");
    std::fs::create_dir_all(&docroot).unwrap();
    std::fs::write(docroot.join("index.php"), "<?php phpinfo();\n").unwrap();

    if let Err(e) = ports::ensure_free(&*plat, PORT, ports::Proto::Tcp, "FrankenPHP") {
        eprintln!("port {PORT} busy: {e}");
        std::process::exit(1);
    }

    let bin = binaries::resolve(&*plat, "frankenphp", binaries::FRANKENPHP_VERSION).await.unwrap();
    let conf = frankenphp::write_config(&*plat, DOMAIN, &docroot, PORT, RewriteMode::Single, &[]).unwrap();
    let mut child = frankenphp::start(&*plat, &bin, DOMAIN, &conf, &[]).expect("start frankenphp");
    thread::sleep(Duration::from_millis(1500));

    let listening = frankenphp::running(PORT);
    let admin_bound = ports::is_listening(2019); // must be false — admin is off

    let code = Command::new("curl")
        .args(["-s", "-H", &format!("Host: {DOMAIN}"), "-o", "/dev/null", "-w", "%{http_code}",
               &format!("http://127.0.0.1:{PORT}/")])
        .output().expect("curl").stdout;
    let code = String::from_utf8_lossy(&code).trim().to_string();
    let body = Command::new("curl")
        .args(["-s", "-H", &format!("Host: {DOMAIN}"), &format!("http://127.0.0.1:{PORT}/")])
        .output().expect("curl").stdout;
    let body = String::from_utf8_lossy(&body);
    let php_ver = body.split("PHP Version ").nth(1)
        .and_then(|s| s.split('<').next()).map(|s| s.trim().to_string()).unwrap_or_default();

    println!("listening on :{PORT} = {listening}");
    println!("http={code}  embedded PHP reported = {php_ver}");
    println!(":2019 admin bound = {admin_bound}  (must be false — admin off)");

    // Cleanup.
    let _ = frankenphp::stop(&*plat, child.id());
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&docroot);

    let ok = listening && code == "200" && php_ver.starts_with("8.") && !admin_bound;
    if ok {
        println!("\nOK — FrankenPHP backend serves embedded PHP on a loopback port, no edge/admin.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}
