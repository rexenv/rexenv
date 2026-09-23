//! Manual check: the edge wire-probe is QUIET (answered at the edge — zero
//! nginx access-log lines) without weakening shadow detection (a foreign
//! no-marker TLS listener is still classified as NOT ours — the Herd class).
//!
//! 1. Regenerate configs (adds the adminer-block probe route) and reload the
//!    LIVE edge in place over its admin socket (config-shape reload, no force).
//! 2. Quiet: N probes against the real edge on :443 → all `true`, and the
//!    nginx access log grows by at most one line (a background tick from a
//!    still-running old-code app; N probe lines would show if probes leaked).
//! 3. Positive control: one old-style `GET /` through the edge MUST add an
//!    access-log line — proving the quiet is "probe never reaches nginx",
//!    not "logging broke / was turned off".
//! 4. Shadow simulation: the pinned nginx binary serving TLS 404s on
//!    127.0.0.1:18443 (exactly what Herd is — nginx, no marker, no
//!    `Server: Caddy`) → `edge_answers_as_ours` MUST return false.
//!
//! Steps 1–3 need the real stack's edge RUNNING (Start all in the app first);
//! they SKIP otherwise. Step 4 always runs.

use rexenv_lib::core::{adminer, binaries, proxy, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use std::io::Write;
use std::time::Duration;

const FOREIGN_PORT: u16 = 18443;

fn access_log_lines(path: &std::path::Path) -> usize {
    std::fs::read(path).map(|b| b.iter().filter(|c| **c == b'\n').count()).unwrap_or(0)
}

/// The probe's exact client shape, minus the marker check — used for the
/// positive control (an old-style `GET /` that must traverse nginx and log).
async fn get_via_loopback(host: &str, port: u16, path: &str) -> bool {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .resolve(host, std::net::SocketAddr::from(([127, 0, 0, 1], port)))
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    client.get(format!("https://{host}:{port}{path}")).send().await.is_ok()
}

#[tokio::main]
async fn main() {
    let plat = platform::current();

    // ── Steps 1–3: live edge required.
    if proxy::admin_alive(&*plat) {
        let conn = db::open_for_platform(plat.paths()).unwrap();
        let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
        let cfg = sites::rebuild_configs(&conn, &*plat, &ca, services::NGINX_HTTP_PORT, 80, 443)
            .unwrap();
        let caddyfile_text = std::fs::read_to_string(&cfg.caddyfile).unwrap();
        assert!(
            caddyfile_text.contains("respond @rexenv_probe 204"),
            "regenerated Caddyfile lacks the probe route"
        );
        let caddy = binaries::resolve(&*plat, "caddy", binaries::pins().caddy).await.unwrap();
        proxy::reload(&*plat, &caddy, &cfg.caddyfile, false).unwrap();
        assert!(proxy::admin_alive(&*plat), "edge must still answer after reload");
        println!("reloaded live edge with probe route ✓");

        let log = plat.paths().log_dir().unwrap().join("nginx-access.log");
        let before = access_log_lines(&log);

        // Quiet: 5 probes, all must classify the edge as ours, none may log.
        for i in 0..5 {
            assert!(
                proxy::edge_answers_as_ours(adminer::ADMINER_HOST, 443).await,
                "probe {i} did not classify the live edge as ours"
            );
        }
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let mid = access_log_lines(&log);
        assert!(
            mid.saturating_sub(before) <= 1,
            "5 probes leaked into the access log ({before} → {mid} lines; \
             at most 1 background tick from a running old-code app is tolerated)"
        );
        println!("quiet: 5 probes, access log {before} → {mid} lines ✓");

        // Positive control: an old-style GET / must traverse nginx and log —
        // the quiet above is the probe path, not broken logging.
        assert!(get_via_loopback(adminer::ADMINER_HOST, 443, "/").await);
        tokio::time::sleep(Duration::from_millis(800)).await;
        let after = access_log_lines(&log);
        assert!(
            after > mid,
            "control GET / did not appear in the access log ({mid} → {after}) — \
             is access logging off?"
        );
        println!("control: GET / logged ({mid} → {after} lines) ✓");
    } else {
        println!("SKIP quiet+control: no live edge on the admin socket — Start all in the app");
    }

    // ── Step 4: foreign no-marker TLS listener (the Herd class) must be
    // classified as NOT ours. Real nginx binary, TLS 404s, loopback-only.
    match std::net::TcpListener::bind(("127.0.0.1", FOREIGN_PORT)) {
        Ok(l) => drop(l),
        Err(e) => {
            println!("SKIP shadow check: port {FOREIGN_PORT} busy ({e})");
            return;
        }
    }
    let nginx = binaries::resolve(&*plat, "nginx", binaries::pins().nginx).await.unwrap();
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let cert =
        ssl::ensure_site_cert(plat.paths(), plat.permissions(), &ca, adminer::ADMINER_HOST)
            .unwrap();
    let prefix = std::env::temp_dir().join("rexenv-wire-probe-foreign");
    let _ = std::fs::remove_dir_all(&prefix);
    std::fs::create_dir_all(prefix.join("logs")).unwrap();
    let conf = prefix.join("nginx.conf");
    {
        // Quote all paths — app-data paths contain spaces.
        let mut f = std::fs::File::create(&conf).unwrap();
        write!(
            f,
            "worker_processes 1;\ndaemon off;\npid \"{pid}\";\n\
             error_log \"{err}\";\nevents {{}}\n\
             http {{\n  access_log off;\n  server {{\n    listen 127.0.0.1:{FOREIGN_PORT} ssl;\n\
    ssl_certificate \"{crt}\";\n    ssl_certificate_key \"{key}\";\n\
    return 404;\n  }}\n}}\n",
            pid = prefix.join("nginx.pid").display(),
            err = prefix.join("logs/error.log").display(),
            crt = cert.cert_path.display(),
            key = cert.key_path.display(),
        )
        .unwrap();
    }
    let mut child = std::process::Command::new(&nginx)
        .args(["-p", &prefix.display().to_string(), "-c", &conf.display().to_string()])
        .spawn()
        .unwrap();
    // Wait for the listener.
    let mut up = false;
    for _ in 0..30 {
        if std::net::TcpStream::connect(("127.0.0.1", FOREIGN_PORT)).is_ok() {
            up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(up, "foreign nginx did not start listening on {FOREIGN_PORT}");

    let verdict = proxy::edge_answers_as_ours(adminer::ADMINER_HOST, FOREIGN_PORT).await;

    // Graceful stop — `-s stop` signals the master so workers die with it
    // (a SIGKILLed master leaks workers that keep holding the port).
    let _ = std::process::Command::new(&nginx)
        .args(["-p", &prefix.display().to_string(), "-c", &conf.display().to_string(), "-s", "stop"])
        .status();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&prefix);

    assert!(
        !verdict,
        "a foreign no-marker TLS listener was classified as OURS — shadow detection broken"
    );
    println!("shadow: foreign nginx TLS 404 on :{FOREIGN_PORT} classified NOT ours ✓");
    println!("wire_probe_check PASS");
}
