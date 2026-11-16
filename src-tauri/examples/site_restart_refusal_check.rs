//! Live check (sandbox tier): `site restart`'s REFUSED outcome — ledger #444.
//!
//!   cargo run --example site_restart_refusal_check
//!
//! `restart_site_backend` has three outcomes. `Shared` is unit-proven and was run
//! live (3 Sep 2026); `Backend` was run live on Windows (7 Nov 2026). `Refused` —
//! an ADOPTED backend that a non-app process may not stop — had neither: the stack
//! guard is always open under `cfg(test)`, so no unit test can hold it, and the
//! real app is never refused, so no `rex site restart` against it can either. The
//! only process that meets the refusal is one like this: a live-check example,
//! which is exactly who the guard exists to stop.
//!
//! What is asked, against a real FrankenPHP on a fixture port:
//!
//!   1. A manager that ADOPTED the backend answers `Refused` — not `Backend`, not
//!      an error — and the backend is the SAME process, still answering.
//!   2. The backend is put back: a second restart is refused too. Dropping it from
//!      the map instead would make the next call see "nothing tracked", walk into
//!      the spawn path and report a port conflict about our own site.
//!   3. CONTROL, last, on the same manager: with the guard explicitly opened the
//!      restart is `Backend` and a DIFFERENT process answers. Without it, "refused"
//!      could not be told from "this manager can never restart anything".
//!
//! The control opens the guard for THIS process, which is safe only because every
//! path it can reach is fixture-owned: the `Platform` is `common::sandbox`, so the
//! ownership marker is the sandbox's app-data dir (the user's services carry a
//! different one and are never adopted), and the port is a fixture port outside
//! the `8200..8300` override range. `stop_all` is never called.

use rexenv_lib::core::service_manager::{ServiceManager, SiteRestartOutcome};
use rexenv_lib::core::{binaries, frankenphp, ports, sites, ssl, stack_guard};
use rexenv_lib::platform::traits::Platform;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::time::Duration;

mod common;

/// Outside the `8200..8300` override range, so a running stack's own backends
/// can never be the thing this adopts, refuses or restarts.
const PORT: u16 = 8378;
const DOMAIN: &str = "fprefuse.test";
const MARKER: &str = "REXENV-RESTART-REFUSAL-444";

/// Stops whatever fixture backend still holds [`PORT`] when the example ends —
/// including on a panic. The control leg's respawned backend belongs to the
/// manager, not to an [`common::OwnedService`], so nothing else would reap it.
/// Ownership is the fixture port AND the sandbox's own marker on the command
/// line: a process of the user's can satisfy neither.
struct FixtureBackend<'a> {
    plat: &'a dyn Platform,
    marker: String,
}

impl Drop for FixtureBackend<'_> {
    fn drop(&mut self) {
        if let Some(pid) = self.plat.supervisor().owned_master(PORT, &self.marker) {
            let _ = self.plat.supervisor().stop(pid);
            if !ports::wait_free(self.plat, PORT, ports::Proto::Tcp, 50, Duration::from_millis(100)) {
                eprintln!("!! fixture FrankenPHP (pid {pid}) still holds :{PORT} — stop it by hand");
            }
        }
    }
}

fn outcome_text(o: &SiteRestartOutcome) -> String {
    match o {
        SiteRestartOutcome::Backend { server, port } => format!("Backend {{ {server}, :{port} }}"),
        SiteRestartOutcome::Shared { php_minor, pool_port } => format!("Shared {{ {php_minor}, :{pool_port} }}"),
        SiteRestartOutcome::Refused { server } => format!("Refused {{ {server} }}"),
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::require_ports_free(&[(PORT, "this example's fixture FrankenPHP")]);

    let (plat, _sandbox) = common::sandbox("fprefuse");
    let mut check = common::Check::new("site_restart_refusal_check");
    let marker = plat.paths().app_data_dir().expect("sandbox app-data").display().to_string();
    let _sweep = FixtureBackend { plat: &*plat, marker: marker.clone() };

    // A real site row, so the manager is handed what the app hands it.
    let conn = common::sandbox_db(&*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("sandbox CA");
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: DOMAIN.into(),
            domain: DOMAIN.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Frankenphp,
            path: String::new(),
            db_engine: SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: false,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .expect("provision the fixture site");
    let mut site = sites::list(&conn)
        .expect("sites")
        .into_iter()
        .find(|s| s.domain == DOMAIN)
        .expect("the fixture site row");
    // The RECORDED port is what adoption and restart both read (B20 §4). Pinned to
    // the fixture port in memory: the allocator's own answer is the lowest free
    // port of the production override range.
    site.override_port = Some(PORT);
    let docroot = site.served_root();
    std::fs::create_dir_all(&docroot).expect("docroot");
    std::fs::write(docroot.join("index.php"), format!("<?php echo '{MARKER}';")).expect("index.php");

    // The "previous session": a backend this manager never spawned.
    let bin = binaries::resolve_program(&*plat, "frankenphp", binaries::pins().frankenphp)
        .await
        .expect("frankenphp binary (cached)");
    let log = frankenphp::log_path(&*plat, DOMAIN).expect("frankenphp log path");
    let conf = frankenphp::write_config(
        &*plat,
        DOMAIN,
        &docroot,
        PORT,
        sites::rewrite_mode_for(site.multisite),
        &[],
        None,
    )
    .expect("frankenphp config");
    let mut survivor = common::OwnedService::new(
        frankenphp::start(&*plat, &bin, DOMAIN, &conf, &[]).expect("start frankenphp"),
        "frankenphp",
    );
    common::await_http_answer(PORT, DOMAIN, "/index.php", "the fixture backend", Some(&log));
    let before = plat.supervisor().owned_master(PORT, &marker);
    check.is(
        "the fixture backend is ownable: fixture port + the sandbox's marker",
        before.is_some(),
        "owned_master found nothing — adoption below could not find it either",
    );

    // ── 1. Adopted, guard closed: refused, and nothing was stopped ───────────
    check.is(
        "this process starts with the stack guard CLOSED",
        !stack_guard::may_control_real_stack(),
        "the guard is already open — every leg below would be the control",
    );
    let mut mgr = ServiceManager::default();
    let adopted = mgr.adopt_startup(&*plat, std::slice::from_ref(&site), false);
    check.is("the manager adopted the backend", adopted >= 1, &format!("adopted {adopted}"));

    for attempt in ["first", "second"] {
        match mgr.restart_site_backend(&*plat, &site).await {
            Ok((outcome, checks)) => {
                check.is(
                    &format!("{attempt} restart of the adopted backend is Refused, naming the server"),
                    matches!(outcome, SiteRestartOutcome::Refused { server } if server == "FrankenPHP"),
                    &outcome_text(&outcome),
                );
                check.is(
                    &format!("{attempt} refusal hands back no readiness check"),
                    checks.is_empty(),
                    &format!("{} check(s)", checks.len()),
                );
            }
            Err(e) => check.is(
                &format!("{attempt} restart of the adopted backend is Refused, naming the server"),
                false,
                &format!("an error instead of the refusal: {e}"),
            ),
        }
        check.is(
            &format!("after the {attempt} refusal the SAME process holds the port"),
            before.is_some() && plat.supervisor().owned_master(PORT, &marker) == before,
            &format!("before {before:?}, now {:?}", plat.supervisor().owned_master(PORT, &marker)),
        );
        let body = common::http_get_bounded(PORT, DOMAIN, "/index.php", 20).unwrap_or_else(|why| why);
        check.is(&format!("…and it still answers after the {attempt} refusal"), body.contains(MARKER), &body);
    }

    // ── 2. Control: the guard opened, the same manager restarts it ───────────
    stack_guard::allow_real_stack_control();
    match mgr.restart_site_backend(&*plat, &site).await {
        Ok((outcome, _checks)) => {
            check.is(
                "control: with the guard open the restart is Backend on the recorded port",
                matches!(outcome, SiteRestartOutcome::Backend { server, port } if server == "FrankenPHP" && port == PORT),
                &outcome_text(&outcome),
            );
            common::await_http_answer(PORT, DOMAIN, "/index.php", "the restarted backend", Some(&log));
            let after = plat.supervisor().owned_master(PORT, &marker);
            check.is(
                "control: a DIFFERENT process answers now",
                after.is_some() && after != before,
                &format!("before {before:?}, after {after:?}"),
            );
            let body = common::http_get_bounded(PORT, DOMAIN, "/index.php", 20).unwrap_or_else(|why| why);
            check.is("control: the restarted backend serves the site", body.contains(MARKER), &body);
        }
        Err(e) => check.is(
            "control: with the guard open the restart is Backend on the recorded port",
            false,
            &format!("{e}\n  {}", common::log_tail(&log, 30)),
        ),
    }

    // Reap the original child's handle (the control stopped the process itself),
    // then the respawned backend through the port sweep's own ownership test.
    survivor.stop();
    drop(_sweep);
    check.is(
        "teardown: the fixture port is free again",
        ports::wait_free(&*plat, PORT, ports::Proto::Tcp, 50, Duration::from_millis(100)),
        "still listening",
    );
    check.verdict()
}
