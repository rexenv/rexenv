//! Ledger #190's L1 half: **deleting a shared site kills its tunnel FIRST** —
//! through the REAL `delete_site` command on a `tauri::test::mock_app`, with a
//! REAL cloudflared holding a real public URL.
//!
//!   cargo run --example tunnel_delete_order_check      (network tier)
//!
//! The claim is an ORDERING, so the proof is an observer, not an after-check:
//! a sampling thread watches `(cloudflared alive, docroot exists)` at ~5 ms
//! through the whole delete, and the invariant is that NO sample ever shows
//! the site's content gone while its tunnel still runs — the window where a
//! public URL would be pointing at a half-deleted site. `stop_for_domain`
//! WAITS for the process to die (`stop_pid` polls to SIGKILL), which is what
//! makes the ordering real rather than fired-and-hoped; this leg is what
//! notices if that wait is ever removed.
//!
//! Fixture shape:
//! - `common::sandbox` + `common::sandbox_db` — the provisioned docroot lands
//!   in the SANDBOX sites dir, never `~/rexenv/Sites`.
//! - A Blank-PHP nginx site: no database, no engine spawn, so the delete's
//!   fast path runs and the ordering window is at its narrowest (hardest for
//!   the observer, which is the honest direction).
//! - No fixture services and no ports bound: cloudflared is outbound-only and
//!   the site is never served — the tunnel's health stays Unverified, which is
//!   irrelevant to kill ordering.
//! - `common::adopt_public_tunnel` guards the public URL on every exit path.
//!
//! What this does NOT cover (stated): the mirrored-user drop is #190's OTHER
//! half, already L0-proven by record; it needs an imported site + engine and
//! is out of this fixture's scope.

mod common;

use rexenv_lib::commands;
use rexenv_lib::core::{sites, ssl, tunnels};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Manager;

const DOMAIN: &str = "delorder.test";

/// Alive means RUNNING — `ps` state, zombie counts as dead (the same
/// measurement lesson as `common::pid_alive`, 15 Aug 2026).
fn running(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "state="])
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            let s = s.trim();
            !s.is_empty() && !s.starts_with('Z')
        })
        .unwrap_or(false)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut checks = common::Check::new("tunnel_delete_order_check");

    let (plat, sandbox) = common::sandbox("tdo");
    let conn = common::sandbox_db(&*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("CA");

    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Del Order".into(),
            domain: DOMAIN.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
        },
    )
    .expect("provision the fixture site");
    let docroot = std::path::PathBuf::from(&site.path);
    checks.is("fixture site provisioned (docroot exists)", docroot.exists(), "no docroot");

    let app = tauri::test::mock_app();
    app.manage(commands::site_provision::ProvisionJobs::default());
    app.manage(commands::tunnels::Tunnels::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle();

    // ── start the tunnel through the REAL command path ──────────────────────
    let info = commands::tunnels::start_tunnel(
        handle.clone(),
        handle.state(),
        handle.state(),
        handle.state(),
        site.id.clone(),
    )
    .await
    .expect("start_tunnel");
    checks.is("start_tunnel claims the share", info.running, "not running");

    // The pid from the CLAIM ROW (the exit hook's own source of truth).
    let mut pid = 0u32;
    for _ in 0..100 {
        let st = handle.state::<AppState>();
        let rows = {
            let c = st.db.lock().expect("db lock");
            rexenv_lib::state::store::list_tunnels(&c).expect("tunnel rows")
        };
        if let Some(r) = rows.iter().find(|r| r.domain == DOMAIN) {
            if r.pid != tunnels::PID_PENDING && r.pid != 0 {
                pid = r.pid;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    checks.is("cloudflared pid recorded on the claim", pid != 0, "pid never landed");
    if pid == 0 {
        return checks.verdict();
    }
    let _guard = common::adopt_public_tunnel(pid, "(URL pending)");
    // The public URL, so a leaked tunnel is nameable — and so the fixture is
    // provably the real thing, not a cloudflared that failed to register.
    let mut url = String::new();
    for _ in 0..90 {
        if let Some(u) = {
            let st = handle.state::<AppState>();
            let plat_ref = st.platform.as_ref();
            tunnels::read_url(plat_ref, DOMAIN)
        } {
            url = u;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    common::note_public_tunnel_url(&url);
    checks.is(
        "cloudflared registered a real public URL",
        url.contains("trycloudflare.com"),
        &format!("no URL after 45s (got {url:?}) — the fixture never became a real tunnel"),
    );

    // ── the observer: no sample may show content-gone + tunnel-alive ────────
    let stop = Arc::new(AtomicBool::new(false));
    let violations: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    let sampler = {
        let stop = stop.clone();
        let violations = violations.clone();
        let docroot = docroot.clone();
        std::thread::spawn(move || {
            let mut samples = 0u32;
            while !stop.load(Ordering::SeqCst) {
                let tunnel_alive = running(pid);
                let content_gone = !docroot.exists();
                if content_gone && tunnel_alive {
                    violations.lock().unwrap().push(format!(
                        "sample {samples}: docroot gone while cloudflared {pid} still runs — \
                         the public URL outlived the site"
                    ));
                }
                samples += 1;
                std::thread::sleep(Duration::from_millis(5));
            }
            samples
        })
    };

    // ── the REAL delete ─────────────────────────────────────────────────────
    let t0 = Instant::now();
    let existed = commands::sites::delete_site(handle.state(), handle.state(), site.id.clone())
        .await
        .expect("delete_site");
    let took = t0.elapsed();
    stop.store(true, Ordering::SeqCst);
    let samples = sampler.join().expect("sampler");

    checks.is("delete_site reports the site existed", existed, "false");
    checks.is("docroot removed by teardown", !docroot.exists(), "still there");
    checks.is(
        "cloudflared is dead when delete returns",
        !running(pid),
        "the tunnel survived its site's deletion",
    );
    // The observer must have actually observed: a delete faster than one
    // sample proves nothing about ordering.
    checks.is(
        &format!("the observer sampled through the delete ({samples} samples over {took:?})"),
        samples >= 3,
        "too few samples — the observation window is vacuous",
    );
    let v = violations.lock().unwrap();
    checks.is(
        "NO sample saw the content gone while the tunnel ran (kill-first ordering)",
        v.is_empty(),
        &v.join("; "),
    );

    drop(sandbox);
    checks.verdict()
}
