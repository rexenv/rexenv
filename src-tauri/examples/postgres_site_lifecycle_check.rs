//! Live check: a **PostgreSQL-backed site through the app's own path** — create,
//! then delete (`docs/archive/PLAN-postgres-sites.md`; ledger #550, #551).
//! Run: `cargo run --example postgres_site_lifecycle_check`
//!
//! # Why this exists, and why the other four checks did not catch what it does
//!
//! On 10 Sep 2026 PostgreSQL site support shipped with four live checks green
//! and 1,159 lib tests passing. The first real site found two defects in the
//! first two commands a user runs:
//!
//! 1. **create hung.** `pdo_pgsql` was recorded per MINOR — true of the artifacts
//!    rexenv BUILDS — while the machine ran a patch from the update manifest that
//!    rexenv did not build. `artisan migrate` then spun at 99% CPU for minutes.
//! 2. **delete failed.** `psql: unrecognized option '--no-defaults'` — MySQL's
//!    account-cleanup flags handed to psql, in a path whose SQL PostgreSQL could
//!    not have run either.
//!
//! `laravel_postgres_check` was RIGHT and still did not help: it asks every
//! installed binary directly, so it was correct about all seven while the app was
//! wrong about WHICH one it would run. **A check that interrogates the parts
//! cannot catch a wrong answer about which part is used.** So this one asks the
//! app instead — its registry, its provisioning job, its delete command.
//!
//! Proves:
//!   1. a PostgreSQL Laravel site provisions through `site_provision_job` and
//!      SETTLES — the shape of defect 1 is a job that never ends, so the verdict
//!      is bounded and a timeout is a failure, not a hang;
//!   2. the PHP it ran is the registry's EFFECTIVE patch, and that build really
//!      has `pdo_pgsql` — the exact conflation that shipped;
//!   3. Laravel's own `migrations` table is in the PostgreSQL database, asked of
//!      the cluster rather than of artisan's exit code;
//!   4. a minor WITHOUT the driver is refused at prepare, in under a second, with
//!      a message naming the build — the failure mode that used to be a hang;
//!   5. `delete_site` completes and the database is gone.
//!
//! Fixture-owned: its own app database and site rows, a fixture domain carrying
//! this process's pid, cleanup from the run's OWN records (never a derived
//! path — the 24 Jul Sites-folder incident). The engines are the REAL ones,
//! deliberately: which server a site talks to is the thing under test.

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{php, sites, ssl};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::time::{Duration, Instant};
use tauri::{Listener, Manager};

mod common;

#[tokio::main]
async fn main() {
    // Provisioning starts engines through the app's own path and nothing here
    // owns them afterwards — this restores what it found (see the guard's doc).
    let _engines = common::engines_as_found();
    let plat = rexenv_lib::platform::current();
    let pid = std::process::id();
    let mut failures: Vec<String> = Vec::new();

    let conn = {
        let p = std::env::temp_dir().join(format!("rexenv-pglifecycle-{pid}.db"));
        let _ = std::fs::remove_file(&p);
        rexenv_lib::state::db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    let app = tauri::test::mock_app();
    app.manage(commands::repo::RepoJobs::default());
    app.manage(site_provision::ProvisionJobs::default());
    app.manage(commands::tunnels::Tunnels::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle().clone();

    // Database tier only — never the edge — so the serve phase is skipped and
    // this can never rebuild the real stack's vhosts from its own database.
    {
        let state = handle.state::<AppState>();
        let mut mgr = state.services.lock().await;
        let n = mgr.adopt_dbs(state.platform.as_ref());
        println!("adopted {n} running database engine(s)");
    }

    // The minor this machine's registry actually offers, and the patch it will
    // really run. Asked of the app, because that gap IS defect 1.
    // The registry's DEFAULT minor when it can reach PostgreSQL, else the newest
    // that can. Not "the first that can": that picked 8.1, where
    // `composer create-project laravel/laravel` resolves to Laravel 10 (13
    // requires ^8.3) whose framework releases are blocked by security
    // advisories — the install then fails for a reason that has nothing to do
    // with this check's subject. A live check must run the version a USER gets.
    let (minor, patch) = {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let can = |m: &str| {
            php::effective_patch(&conn, m)
                .ok()
                .flatten()
                .is_some_and(|p| php::pdo_pgsql_supported(&p))
        };
        let minor = php::default_minor(&conn)
            .ok()
            .flatten()
            .filter(|m| can(m))
            .or_else(|| {
                rexenv_lib::core::binaries::pins().php_versions
                    .iter()
                    .rev()
                    .map(|v| php::minor_of(v))
                    .find(|m| can(m))
            })
            .expect("some installed minor must be able to reach PostgreSQL");
        let patch = php::effective_patch(&conn, &minor).unwrap().unwrap();
        (minor, patch)
    };
    println!("PHP for the site: minor {minor} → effective patch {patch}");

    // Leg 2, first half: the record and the artifact must agree. This is the
    // conflation that shipped — the minor was judged by a build that was not
    // the one the registry selects.
    let php_bin = rexenv_lib::core::binaries::resolve(handle.state::<AppState>().platform.as_ref(), "php", &patch)
        .await
        .expect("the effective PHP resolves");
    let loaded = std::process::Command::new(&php_bin)
        .args(["-r", "echo extension_loaded('pdo_pgsql') ? 'yes' : 'no';"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes")
        .unwrap_or(false);
    println!("2. the effective build has pdo_pgsql → {loaded}");
    if !loaded {
        failures.push(format!(
            "the registry selects PHP {patch} and rexenv says it can reach PostgreSQL, but that \
             binary has no pdo_pgsql — this is exactly ledger #550"
        ));
    }

    let new_site = |domain: &str, php: &str| NewSite {
        name: format!("PG lifecycle {domain}"),
        domain: domain.into(),
        site_type: SiteType::Laravel,
        php_version: php.into(),
        web_server: WebServer::Nginx,
        path: String::new(),
        db_engine: SiteDbEngine::Postgres,
        git_url: String::new(),
        git_ref: None,
        git_migrate: true,
        git_build_assets: false,
        starter_db: false,
    };

    // ── 4. A minor WITHOUT the driver is REFUSED, fast ───────────────────────
    // Before the slow leg, because it is the cheap one and because its failure
    // mode used to be a four-minute hang rather than an error.
    let without = rexenv_lib::core::binaries::pins().php_versions
        .iter()
        .find(|v| !php::pdo_pgsql_supported(v))
        .map(|v| php::minor_of(v));
    if let Some(bad_minor) = without {
        let started = Instant::now();
        let refused = site_provision::site_provision_job(
            handle.clone(),
            handle.state::<AppState>(),
            handle.state::<site_provision::ProvisionJobs>(),
            new_site(&format!("pgrefuse-{pid}.rex"), &bad_minor),
            None,
            None,
        )
        .await;
        let took = started.elapsed();
        match &refused {
            Err(e) => {
                let m = e.to_string();
                println!("4. PHP {bad_minor} + PostgreSQL → refused in {took:?}: {m}");
                if !m.contains("PostgreSQL") || !m.contains("PDO") {
                    failures.push(format!("the refusal must name the reason: {m}"));
                }
                if took > Duration::from_secs(5) {
                    failures.push(format!("the refusal took {took:?} — it must not be a wait"));
                }
            }
            Ok(_) => failures.push(format!(
                "PHP {bad_minor} has no pdo_pgsql and a PostgreSQL site was ACCEPTED — the \
                 provision would hang, which is what #550 was"
            )),
        }
    } else {
        println!("4. skipped — every installed PHP can reach PostgreSQL on this machine");
    }

    // ── 1. Create, and SETTLE ────────────────────────────────────────────────
    let domain = format!("pglife-{pid}.rex");
    let snap = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        new_site(&domain, &minor),
        None,
        None,
    )
    .await
    .expect("start the PostgreSQL site job");
    let site_id = snap.site_id.clone().expect("a started job has a site row");

    // The job's own streamed lines, kept so a failure NAMES itself. Without this
    // the first red run of this check said only "settled failed@58", which is
    // the shape of report this whole feature has been fixing.
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    {
        let sink = lines.clone();
        handle.listen(site_provision::output_event(&snap.id), move |ev| {
            if let Ok(s) = serde_json::from_str::<String>(ev.payload()) {
                sink.lock().unwrap().push(s);
            }
        });
    }

    // Bounded on purpose: "never settles" is defect 1's whole shape, so a
    // timeout here is a FAILURE with a name, not a hung example.
    let deadline = Instant::now() + Duration::from_secs(900);
    let fin = loop {
        let st = site_provision::state_of(
            &handle.state::<site_provision::ProvisionJobs>(),
            &snap.id,
        )
        .unwrap();
        if st.status != "running" {
            break Some(st);
        }
        if Instant::now() > deadline {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    for l in lines.lock().unwrap().iter() {
        println!("  | {l}");
    }
    match &fin {
        Some(st) => {
            println!("1. job settled {}@{} — {:?}", st.status, st.pct, st.summary);
            if st.status != "ok" {
                failures.push(format!("the PostgreSQL site settled {} (want ok)", st.status));
            }
        }
        None => failures.push(
            "the PostgreSQL site never settled in 15 minutes — that is the #550 shape: a \
             missing driver busy-loops instead of failing"
                .into(),
        ),
    }

    // ── 3. Laravel's own table, in PostgreSQL, asked of the CLUSTER ─────────
    let db_name = {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        sites::get(&conn, &site_id).unwrap().map(|s| s.db_name)
    };
    if let Some(db_name) = db_name.clone() {
        let engine = DbEngine::Postgres;
        let state = handle.state::<AppState>();
        // The engine version the same way the app resolves it, without reaching
        // for a crate-private helper: the selection when there is one, else the
        // default pin.
        let version = {
            let conn = state.db.lock().unwrap();
            engine.effective_version(&conn)
        };
        let (client, _) = engine
            .sql_client_bins(state.platform.as_ref(), &version)
            .await
            .expect("psql");
        let out = std::process::Command::new(client.path())
            .args(rexenv_lib::core::postgres::psql_base_args(engine.port(), &db_name))
            .args([
                "-tA",
                "--command",
                "SELECT count(*) FROM information_schema.tables \
                 WHERE table_schema='public' AND table_name='migrations'",
            ])
            .output()
            .expect("run psql");
        let found = String::from_utf8_lossy(&out.stdout).trim().to_string();
        println!("3. migrations table in {db_name} → {found:?}");
        if found != "1" {
            failures.push(format!(
                "Laravel's migrations table is not in {db_name} — artisan exiting 0 against the \
                 wrong target looks identical from the app's side"
            ));
        }
    }

    // ── 5. Delete through the app's own command ──────────────────────────────
    // Defect 2 lived here and nowhere else: everything above was already green
    // on the day it shipped.
    let deleted = commands::sites::delete_site(
        handle.state::<AppState>(),
        handle.state::<commands::tunnels::Tunnels>(),
        site_id.clone(),
    )
    .await;
    match &deleted {
        Ok(true) => println!("5. delete_site → ok"),
        other => failures.push(format!(
            "deleting a PostgreSQL site failed: {other:?} — #551 was `psql: unrecognized \
             option '--no-defaults'` on exactly this call"
        )),
    }
    if let Some(db_name) = db_name {
        let engine = DbEngine::Postgres;
        let state = handle.state::<AppState>();
        // The engine version the same way the app resolves it, without reaching
        // for a crate-private helper: the selection when there is one, else the
        // default pin.
        let version = {
            let conn = state.db.lock().unwrap();
            engine.effective_version(&conn)
        };
        if let Ok((client, _)) = engine.sql_client_bins(state.platform.as_ref(), &version).await {
            let left = engine
                .db_sizes(&client, engine.port())
                .map(|s| s.into_iter().any(|(n, _)| n == db_name))
                .unwrap_or(true);
            println!("5. {db_name} still in the cluster → {left}");
            if left {
                failures.push(format!("{db_name} survived the delete"));
            }
        }
    }

    // Cleanup: only what this run recorded, and only if the delete did not
    // already do it — a failed delete must not leave a site behind.
    {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        if let Ok(Some(site)) = sites::get(&conn, &site_id) {
            let plat = rexenv_lib::platform::current();
            println!("cleanup: tearing down {} left by a failed delete", site.domain);
            let _ = sites::teardown(&conn, &*plat, &site_id);
        }
    }
    let _ = std::fs::remove_file(std::env::temp_dir().join(format!("rexenv-pglifecycle-{pid}.db")));

    println!();
    if failures.is_empty() {
        println!("postgres_site_lifecycle_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
