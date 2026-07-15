//! Manual check: the per-site Xdebug toggle's moving parts (§8.2). Run:
//! `cargo run --example xdebug_pool_check`
//!
//! Proves the whole chain on the REAL binary cache, with throwaway processes
//! only (the running stack is untouched):
//!   1. `resolve_bundle("xdebug-<minor>")` — the shivammathur ghcr bottle
//!      downloads, verifies, relinks (no-op: system libs only), re-signs;
//!      audit `xdebug.so`'s load commands + signature.
//!   2. `assert_fpm_loads_xdebug` — the load-probe gate every debug-pool
//!      spawn runs (PHP treats a bad zend_extension as a WARNING, so this
//!      gate is what keeps a broken artifact from serving silently).
//!   3. A real DBGp handshake: the bundled PHP CLI with the .so in
//!      `xdebug.mode=debug` connects to a local listener and sends the
//!      protocol init packet — what an IDE sees on port 9003.
//!   4. A throwaway DEBUG pool via the production `start_fpm_xdebug` path on
//!      a probe port; the pool must accept FastCGI connections.

use rexenv_lib::core::{binaries, php, services};
use rexenv_lib::platform;
use std::io::Read;
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Probe port for the throwaway debug pool — NOT the real 99xx range, so a
/// running stack's own debug pools are never contended.
const POOL_PORT: u16 = 9998;
/// Local listener standing in for the IDE (not 9003 — an IDE may be running).
const DBGP_PORT: u16 = 19003;

fn load_commands_clean(path: &std::path::Path) -> bool {
    let out = Command::new("otool").arg("-L").arg(path).output().expect("otool");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .all(|d| d.starts_with("/usr/lib/") || d.starts_with("/System/") || d.starts_with("@loader_path/"))
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let mut ok = true;

    let minor = php::minor_of(binaries::PHP_VERSION);
    let patch = php::patch_for_minor(&minor).expect("default minor pinned");
    let (bundle, bundle_version) =
        binaries::xdebug_bundle_id(&minor).expect("default minor supports xdebug");

    println!("=== resolve_bundle({bundle} {bundle_version}) ===");
    let dir = binaries::resolve_bundle(&*plat, &bundle, bundle_version)
        .await
        .expect("resolve xdebug bundle");
    let so = dir.join("xdebug.so");
    println!("  published at {}", so.display());
    assert!(so.is_file(), "xdebug.so missing from the published bundle");

    println!("\n=== relink + signature audit ===");
    let clean = load_commands_clean(&so);
    let signed = Command::new("codesign")
        .args(["--verify", "--strict", &so.display().to_string()])
        .status()
        .expect("codesign")
        .success();
    println!("  loads-clean={clean} · codesign-verify={signed}");
    ok &= clean && signed;

    println!("\n=== load-probe gate (assert_fpm_loads_xdebug) ===");
    let fpm_bin = binaries::resolve(&*plat, "php-fpm", patch).await.expect("php-fpm");
    let gate = services::assert_fpm_loads_xdebug(&*plat, &fpm_bin, &so);
    println!("  php-fpm {patch} loads xdebug.so → {:?}", gate.as_ref().map(|_| "ok"));
    ok &= gate.is_ok();

    println!("\n=== DBGp handshake (what the IDE sees) ===");
    let php_bin = binaries::resolve(&*plat, "php", patch).await.expect("php cli");
    let listener = TcpListener::bind(("127.0.0.1", DBGP_PORT)).expect("bind dbgp listener");
    listener.set_nonblocking(false).unwrap();
    let handle = thread::spawn(move || {
        let (mut conn, _) = listener.accept().expect("accept dbgp");
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut buf = [0u8; 2048];
        let n = conn.read(&mut buf).unwrap_or(0);
        String::from_utf8_lossy(&buf[..n]).to_string()
    });
    let run = Command::new(&php_bin)
        .args([
            "-d", &format!("zend_extension={}", so.display()),
            "-d", "xdebug.mode=debug",
            "-d", "xdebug.start_with_request=yes",
            "-d", "xdebug.client_host=127.0.0.1",
            "-d", &format!("xdebug.client_port={DBGP_PORT}"),
            "-r", "echo 'ran';",
        ])
        .output()
        .expect("php cli");
    let init = handle.join().expect("dbgp thread");
    let dbgp_ok = init.contains("urn:debugger_protocol_v1") && init.contains("Xdebug");
    println!(
        "  script-ran={} · dbgp-init={dbgp_ok}",
        String::from_utf8_lossy(&run.stdout).contains("ran")
    );
    ok &= dbgp_ok;

    println!("\n=== throwaway DEBUG pool via start_fpm_xdebug on :{POOL_PORT} ===");
    let workdir = std::env::temp_dir().join("rexenv-xdebug-pool-check");
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).unwrap();
    let conf = workdir.join("fpm-debug.conf");
    std::fs::write(
        &conf,
        format!(
            "[global]\ndaemonize = no\nerror_log = {log}\n[www]\nlisten = 127.0.0.1:{POOL_PORT}\npm = static\npm.max_children = 2\n",
            log = workdir.join("fpm.log").display()
        ),
    )
    .unwrap();
    let mut pool =
        services::start_fpm_xdebug(&*plat, &fpm_bin, &conf, &so).expect("spawn debug pool");
    let mut accepting = false;
    for _ in 0..40 {
        if services::fpm_running(POOL_PORT) {
            accepting = true;
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    println!("  pid {} · accepting on :{POOL_PORT} = {accepting}", pool.id());
    ok &= accepting;

    let _ = pool.kill();
    let _ = pool.wait();
    let _ = std::fs::remove_dir_all(&workdir);

    if ok {
        println!("\nOK — xdebug.so pinned+published, gate passes, DBGp handshakes, debug pool serves.");
    } else {
        println!("\nFAILED — see the checks above.");
        std::process::exit(1);
    }
}
