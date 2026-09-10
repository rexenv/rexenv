//! Live check: the **site matrix** — every pinned PHP × {Blank PHP, WordPress,
//! Laravel} × {MySQL, MariaDB, PostgreSQL}, created, exercised and deleted
//! through the app's own provisioning job and delete command.
//! Run: `cargo run --example site_matrix_check`
//! Narrow a re-run: `MATRIX_PHP=8.1,8.5 MATRIX_TYPE=laravel MATRIX_ENGINE=postgres`
//!
//! # Why this exists
//!
//! Every PostgreSQL defect found so far was found by a real site on ONE
//! combination the per-part checks had not happened to pick (#550: the patch the
//! registry really selects; #551: delete handing psql MySQL's flags). The space
//! is small enough to walk entirely — 7 × 3 × 3 — so this walks it rather than
//! sampling the convenient corner.
//!
//! Per combination:
//!   * **refused combinations are asserted refused**, fast, by name — WordPress
//!     on PostgreSQL, and PostgreSQL on a PHP build without `pdo_pgsql`;
//!   * every other one must SETTLE `ok` (bounded — a hang is a failure);
//!   * the site's database exists in the engine the row names;
//!   * the site's code really talks to it: the Blank PHP page renders its seeded
//!     rows, WordPress answers `core is-installed` and counts its posts, Laravel's
//!     `.env` names the engine and its `migrations` table is in that cluster;
//!   * the front page is requested through the site's own PHP build (the PHP
//!     built-in server — the fixture app never owns nginx) and must be a 200 with
//!     no fatal error in the log;
//!   * `delete_site` completes, the database is gone and so is the folder.
//!
//! Fixture-owned: its own app database, a sites dir under the temp dir, domains
//! carrying this pid, cleanup from the run's OWN records. The engines are the real
//! ones on purpose — which server a site talks to is the thing under test.

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{binaries, database, php, postgres, sites, ssl, wordpress};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, Site, SiteDbEngine, SiteType, WebServer};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{Listener, Manager};

mod common;

fn wanted(var: &str, value: &str) -> bool {
    match std::env::var(var) {
        Ok(list) if !list.trim().is_empty() => list.split(',').any(|v| v.trim() == value),
        _ => true,
    }
}

fn engine_of(e: SiteDbEngine) -> DbEngine {
    match e {
        SiteDbEngine::Mysql => DbEngine::Mysql,
        SiteDbEngine::Mariadb => DbEngine::Mariadb,
        SiteDbEngine::Postgres => DbEngine::Postgres,
    }
}

fn type_key(t: SiteType) -> &'static str {
    match t {
        SiteType::Php => "php",
        SiteType::Wordpress => "wordpress",
        SiteType::Laravel => "laravel",
    }
}

fn engine_key(e: SiteDbEngine) -> &'static str {
    match e {
        SiteDbEngine::Mysql => "mysql",
        SiteDbEngine::Mariadb => "mariadb",
        SiteDbEngine::Postgres => "postgres",
    }
}

/// One scalar from the site's engine, asked of the CLUSTER with the same client
/// and flags the app uses — never of the site's own code, which is under test.
fn scalar(engine: DbEngine, client: &Path, database: &str, sql: &str) -> Result<String, String> {
    let mut cmd = Command::new(client);
    match engine {
        DbEngine::Postgres => {
            cmd.args(postgres::psql_base_args(engine.port(), database)).args(["-tA", "--command", sql]);
        }
        _ => {
            cmd.args(database::client_base_args(engine.port()))
                .args(["--batch", "--skip-column-names", database, "-e", sql]);
        }
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// GET `/` through the site's own PHP build. HTTPS is asserted on the request
/// (what the edge tells PHP in production) so WordPress does not canonical-
/// redirect to the https siteurl instead of rendering.
fn fetch_front_page(site_php: &Path, docroot: &Path, domain: &str, scratch: &Path) -> (u16, String, Vec<String>) {
    let prepend = scratch.join("https-prepend.php");
    std::fs::write(&prepend, "<?php $_SERVER['HTTPS'] = 'on';\n").unwrap();
    let log = scratch.join(format!("{domain}-php-errors.log"));
    let _ = std::fs::remove_file(&log);
    let port = free_port();
    let mut server = Command::new(site_php)
        .args([
            "-d", "display_errors=0",
            "-d", "log_errors=1",
            "-d", &format!("error_log={}", log.display()),
            "-d", &format!("auto_prepend_file={}", prepend.display()),
            "-S", &format!("127.0.0.1:{port}"),
            "-t", &docroot.to_string_lossy(),
        ])
        .current_dir(docroot)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn php -S");
    let up = Instant::now();
    while !rexenv_lib::core::ports::is_listening(port) && up.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let body_file = scratch.join(format!("{domain}-body.html"));
    let out = Command::new("curl")
        .args(["-s", "-o", &body_file.to_string_lossy(), "-w", "%{http_code}", "--max-time", "90"])
        .args(["-H", &format!("Host: {domain}")])
        .arg(format!("http://127.0.0.1:{port}/"))
        .output()
        .expect("curl");
    let _ = server.kill();
    let _ = server.wait();
    let code = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0);
    let body = std::fs::read_to_string(&body_file).unwrap_or_default();
    let errors = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    (code, body, errors)
}

/// Wait for a provision job to leave `running`, bounded: "never settles" is a
/// failure with a name, not a hung check.
async fn settle(
    handle: &tauri::AppHandle<tauri::test::MockRuntime>,
    job_id: &str,
) -> Option<site_provision::SiteProvisionState> {
    let deadline = Instant::now() + Duration::from_secs(900);
    loop {
        let st = site_provision::state_of(&handle.state::<site_provision::ProvisionJobs>(), job_id).ok()?;
        if st.status != "running" {
            return Some(st);
        }
        if Instant::now() > deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

struct Outcome {
    combo: String,
    verdict: String,
    pass: bool,
    notes: Vec<String>,
}

#[tokio::main]
async fn main() {
    let _engines = common::engines_as_found();
    let plat = rexenv_lib::platform::current();
    let pid = std::process::id();

    let scratch = std::env::temp_dir().join(format!("rexenv-matrix-{pid}"));
    let sites_dir = scratch.join("Sites");
    std::fs::create_dir_all(&sites_dir).unwrap();
    let conn = {
        let p = scratch.join("app.db");
        let c = rexenv_lib::state::db::open(&p).unwrap();
        rexenv_lib::state::store::set_setting(&c, "sites_dir", &sites_dir.to_string_lossy()).unwrap();
        c
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    let app = tauri::test::mock_app();
    app.manage(commands::repo::RepoJobs::default());
    app.manage(site_provision::ProvisionJobs::default());
    app.manage(commands::tunnels::Tunnels::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle().clone();
    {
        let state = handle.state::<AppState>();
        let mut mgr = state.services.lock().await;
        let n = mgr.adopt_dbs(state.platform.as_ref());
        println!("adopted {n} running database engine(s)");
    }

    let wp_phar = binaries::resolve_file(handle.state::<AppState>().platform.as_ref(), "wp-cli", binaries::WP_CLI_VERSION)
        .await
        .expect("wp-cli");

    let mut outcomes: Vec<Outcome> = Vec::new();
    let started_all = Instant::now();

    for version in binaries::PHP_VERSIONS {
        let minor = php::minor_of(version);
        if !wanted("MATRIX_PHP", &minor) {
            continue;
        }
        let patch = {
            let state = handle.state::<AppState>();
            let conn = state.db.lock().unwrap();
            php::effective_patch(&conn, &minor).ok().flatten().unwrap_or_else(|| version.to_string())
        };
        let site_php = binaries::resolve(handle.state::<AppState>().platform.as_ref(), "php", &patch)
            .await
            .expect("resolve the effective PHP");

        for site_type in [SiteType::Php, SiteType::Wordpress, SiteType::Laravel] {
            if !wanted("MATRIX_TYPE", type_key(site_type)) {
                continue;
            }
            for db_engine in [SiteDbEngine::Mysql, SiteDbEngine::Mariadb, SiteDbEngine::Postgres] {
                if !wanted("MATRIX_ENGINE", engine_key(db_engine)) {
                    continue;
                }
                let combo = format!("PHP {patch} · {} · {}", type_key(site_type), engine_key(db_engine));
                println!("\n━━ {combo}");
                let o = run_one(&handle, &minor, &patch, &site_php, &wp_phar, site_type, db_engine, pid, &scratch, combo).await;
                println!("   → {} {}", if o.pass { "PASS" } else { "FAIL" }, o.verdict);
                for n in &o.notes {
                    println!("     · {n}");
                }
                outcomes.push(o);
            }
        }
    }

    // Anything a failed delete left: only this run's rows.
    {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        if let Ok(left) = sites::list(&conn) {
            for s in left {
                println!("cleanup: tearing down {} left by a failed delete", s.domain);
                let _ = sites::teardown(&conn, state.platform.as_ref(), &s.id);
            }
        }
    }

    println!("\n══ site_matrix_check — {} combinations in {:?}", outcomes.len(), started_all.elapsed());
    for o in &outcomes {
        println!("{:4}  {:42} {}", if o.pass { "ok" } else { "FAIL" }, o.combo, o.verdict);
    }
    let failed = outcomes.iter().filter(|o| !o.pass).count();
    if failed == 0 {
        let _ = std::fs::remove_dir_all(&scratch);
        println!("\nsite_matrix_check: ALL PASS");
    } else {
        println!("\nsite_matrix_check: {failed} FAILED (scratch kept at {})", scratch.display());
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_one(
    handle: &tauri::AppHandle<tauri::test::MockRuntime>,
    minor: &str,
    patch: &str,
    site_php: &Path,
    wp_phar: &Path,
    site_type: SiteType,
    db_engine: SiteDbEngine,
    pid: u32,
    scratch: &Path,
    combo: String,
) -> Outcome {
    let mut notes = Vec::new();
    let fail = |combo: String, verdict: String, notes: Vec<String>| Outcome { combo, verdict, pass: false, notes };
    let domain = format!(
        "mx{pid}-{}-{}-{}.rex",
        minor.replace('.', ""),
        &type_key(site_type)[..2],
        &engine_key(db_engine)[..2]
    );
    let expect_refusal = db_engine == SiteDbEngine::Postgres
        && (site_type == SiteType::Wordpress || !php::pdo_pgsql_supported(patch));

    let new = NewSite {
        name: format!("Matrix {domain}"),
        domain: domain.clone(),
        site_type,
        php_version: minor.into(),
        web_server: WebServer::Nginx,
        path: String::new(),
        db_engine,
        git_url: String::new(),
        git_ref: None,
        git_migrate: true,
        git_build_assets: false,
        starter_db: site_type == SiteType::Php,
    };

    let t0 = Instant::now();
    let started = site_provision::site_provision_job(
        handle.clone(),
        handle.state::<AppState>(),
        handle.state::<site_provision::ProvisionJobs>(),
        new,
        None,
        None,
    )
    .await;
    let snap = match started {
        Err(e) if expect_refusal => {
            let m = e.to_string();
            let named = m.contains("PostgreSQL");
            let fast = t0.elapsed() < Duration::from_secs(5);
            // The first run of this check found every refused create leaving its
            // folder (and a Blank-PHP `index.php`) in the Sites directory.
            let left = scratch.join("Sites").join(&domain);
            if left.exists() {
                return fail(combo, format!("refused, but left {} behind", left.display()), notes);
            }
            return Outcome {
                combo,
                verdict: format!("refused as expected in {:?}, nothing left: {}", t0.elapsed(), m.lines().next().unwrap_or("")),
                pass: named && fast,
                notes,
            };
        }
        Err(e) => return fail(combo, format!("refused at start: {e}"), notes),
        Ok(s) => s,
    };
    let Some(site_id) = snap.site_id.clone() else {
        return fail(combo, "job started without a site row".into(), notes);
    };

    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    {
        let sink = lines.clone();
        handle.listen(site_provision::output_event(&snap.id), move |ev| {
            if let Ok(s) = serde_json::from_str::<String>(ev.payload()) {
                sink.lock().unwrap().push(s);
            }
        });
    }
    let deadline = Instant::now() + Duration::from_secs(900);
    let fin = loop {
        let st = site_provision::state_of(&handle.state::<site_provision::ProvisionJobs>(), &snap.id).unwrap();
        if st.status != "running" {
            break Some(st);
        }
        if Instant::now() > deadline {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };

    let mut verdict_problems: Vec<String> = Vec::new();
    if expect_refusal {
        verdict_problems.push("ACCEPTED a combination that must be refused".into());
    }

    // ── The user's way out, walked: Laravel on a PHP whose newest framework is
    // advisory-blocked fails NAMING that, then switching the site to a newer PHP
    // and pressing Retry must finish it. On 11 Sep 2026 the retry could not:
    // the failed create-project had left a skeleton with no vendor/, the job read
    // it as "already present", and every retry failed whatever PHP was chosen.
    let mut fin = fin;
    let mut php_owned = site_php.to_path_buf();
    let advisory_failed = site_type == SiteType::Laravel
        && matches!(&fin, Some(st) if st.status == "failed"
            && st.error.as_deref().is_some_and(|e| e.contains("security advisory")));
    if advisory_failed {
        let first = fin.as_ref().and_then(|s| s.error.clone()).unwrap_or_default();
        notes.push(format!("failed, named: {}", first.lines().next().unwrap_or("")));
        let newest = php::minor_of(binaries::PHP_VERSIONS.last().unwrap());
        {
            let state = handle.state::<AppState>();
            let conn = state.db.lock().unwrap();
            sites::set_php_version(&conn, &site_id, &newest).expect("switch the site's PHP");
        }
        match site_provision::site_provision_retry(
            handle.clone(),
            handle.state::<AppState>(),
            handle.state::<site_provision::ProvisionJobs>(),
            site_id.clone(),
        )
        .await
        {
            Ok(again) => {
                let sink = lines.clone();
                handle.listen(site_provision::output_event(&again.id), move |ev| {
                    if let Ok(s) = serde_json::from_str::<String>(ev.payload()) {
                        sink.lock().unwrap().push(s);
                    }
                });
                fin = settle(handle, &again.id).await;
                let ok = matches!(&fin, Some(st) if st.status == "ok");
                notes.push(format!("switched to PHP {newest} and retried → {}", if ok { "ok" } else { "NOT ok" }));
                if ok {
                    let state = handle.state::<AppState>();
                    let patch = {
                        let conn = state.db.lock().unwrap();
                        php::effective_patch(&conn, &newest).ok().flatten().unwrap_or(newest.clone())
                    };
                    php_owned = binaries::resolve(state.platform.as_ref(), "php", &patch).await.expect("newest PHP");
                }
            }
            Err(e) => verdict_problems.push(format!("site_provision_retry refused: {e}")),
        }
    }
    let site_php: &Path = &php_owned;
    let settled_ok = matches!(&fin, Some(st) if st.status == "ok");
    match &fin {
        Some(st) if st.status == "ok" => notes.push(format!("provisioned in {:?}", t0.elapsed())),
        // `error` is what the card shows the user — the sentence under test when
        // a combination cannot install. `summary` is only set on success.
        Some(st) => verdict_problems.push(format!("settled {} — {}", st.status, st.error.as_deref().unwrap_or("(no error text)"))),
        None => verdict_problems.push("never settled in 15 minutes".into()),
    }
    if !settled_ok {
        let l = lines.lock().unwrap();
        for line in l.iter().rev().take(20).collect::<Vec<_>>().into_iter().rev() {
            notes.push(format!("| {line}"));
        }
    }

    let site: Option<Site> = {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        sites::get(&conn, &site_id).ok().flatten()
    };
    let engine = engine_of(db_engine);
    let client = {
        let state = handle.state::<AppState>();
        let version = {
            let conn = state.db.lock().unwrap();
            engine.effective_version(&conn)
        };
        engine.sql_client_bins(state.platform.as_ref(), &version).await.ok().map(|(c, _)| c)
    };

    if let (true, Some(site), Some(client)) = (settled_ok, site.as_ref(), client.as_ref()) {
        let project = PathBuf::from(&site.path);
        if site.db_engine != db_engine {
            verdict_problems.push(format!("row says {:?}", site.db_engine));
        }
        let in_cluster = engine
            .db_sizes(client, engine.port())
            .map(|s| s.into_iter().any(|(n, _)| n == site.db_name))
            .unwrap_or(false);
        if !in_cluster {
            verdict_problems.push(format!("{} is not in the {engine:?} cluster", site.db_name));
        }

        let docroot = match site_type {
            SiteType::Laravel => project.join("public"),
            _ => project.clone(),
        };
        match site_type {
            SiteType::Php => {
                let sql = format!("SELECT count(*) FROM {}", rexenv_lib::core::starter::TABLE);
                let seeded = scalar(engine, client.path(), &site.db_name, &sql);
                notes.push(format!("seed rows in cluster → {seeded:?}"));
                if seeded.as_deref() != Ok("4") {
                    verdict_problems.push(format!("starter table not seeded: {seeded:?}"));
                }
            }
            SiteType::Wordpress => {
                let path = format!("--path={}", project.display());
                let run = |args: &[&str]| {
                    let mut full = args.to_vec();
                    full.push(&path);
                    wordpress::wp_cli(site_php, wp_phar, &full, None)
                        .map(|o| (o.status.success(), String::from_utf8_lossy(&o.stdout).trim().to_string(), String::from_utf8_lossy(&o.stderr).trim().to_string()))
                        .unwrap_or((false, String::new(), "spawn failed".into()))
                };
                let (inst, _, err) = run(&["core", "is-installed"]);
                if !inst {
                    verdict_problems.push(format!("wp core is-installed failed: {err}"));
                }
                let (_, url, _) = run(&["option", "get", "siteurl"]);
                if url != format!("https://{domain}") {
                    verdict_problems.push(format!("siteurl {url:?}"));
                }
                let posts = scalar(
                    engine,
                    client.path(),
                    &site.db_name,
                    "SELECT count(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name LIKE '%posts'",
                );
                notes.push(format!("wp posts tables in cluster → {posts:?}"));
                if posts.as_deref().map(|p| p.parse::<u32>().unwrap_or(0)) .unwrap_or(0) < 1 {
                    verdict_problems.push("no posts table in the cluster".into());
                }
            }
            SiteType::Laravel => {
                let env = std::fs::read_to_string(project.join(".env")).unwrap_or_default();
                let want_conn = if db_engine == SiteDbEngine::Postgres { "pgsql" } else { "mysql" };
                let has = |k: &str, v: &str| env.lines().any(|l| l.trim() == format!("{k}={v}"));
                if !has("DB_CONNECTION", want_conn) || !has("DB_PORT", &engine.port().to_string()) {
                    let got: Vec<&str> = env.lines().filter(|l| l.starts_with("DB_")).collect();
                    verdict_problems.push(format!(".env does not name the engine: {got:?}"));
                }
                let status = Command::new(site_php)
                    .args(["artisan", "migrate:status", "--no-interaction"])
                    .current_dir(&project)
                    .output();
                match status {
                    Ok(o) if o.status.success() => {}
                    Ok(o) => verdict_problems.push(format!(
                        "artisan migrate:status failed: {}",
                        String::from_utf8_lossy(&o.stderr).lines().chain(String::from_utf8_lossy(&o.stdout).lines()).take(3).collect::<Vec<_>>().join(" / ")
                    )),
                    Err(e) => verdict_problems.push(format!("artisan: {e}")),
                }
                let fw = Command::new(site_php)
                    .args(["artisan", "--version"])
                    .current_dir(&project)
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                notes.push(fw);
                let sql = match engine {
                    DbEngine::Postgres => "SELECT count(*) FROM information_schema.tables WHERE table_schema='public' AND table_name='migrations'",
                    _ => "SELECT count(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='migrations'",
                };
                let found = scalar(engine, client.path(), &site.db_name, sql);
                if found.as_deref() != Ok("1") {
                    verdict_problems.push(format!("migrations table not in the cluster: {found:?}"));
                }
            }
        }

        let (code, body, errors) = fetch_front_page(site_php, &docroot, &domain, scratch);
        notes.push(format!("GET / → {code} ({} bytes)", body.len()));
        let marker = match site_type {
            SiteType::Php => "Hello from your database",
            SiteType::Wordpress => "wp-content",
            SiteType::Laravel => "Laravel",
        };
        if code != 200 || !body.contains(marker) {
            let snippet: String = body.chars().take(300).collect();
            verdict_problems.push(format!("front page {code}, marker {marker:?} missing: {snippet:?}"));
        }
        // The ELEMENT, not the class name: `.err-text` is also in the page's
        // stylesheet, and matching that failed every healthy starter page.
        if site_type == SiteType::Php && body.contains("class=\"err-text\"") {
            verdict_problems.push("starter page rendered its database error".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for e in &errors {
            // Strip the timestamp so repeats collapse.
            let msg = e.split_once("] ").map(|(_, m)| m).unwrap_or(e).to_string();
            if msg.contains("Fatal error") {
                verdict_problems.push(format!("fatal on the front page: {msg}"));
            }
            if seen.insert(msg.clone()) && seen.len() <= 4 {
                notes.push(format!("php log: {}", msg.chars().take(220).collect::<String>()));
            }
        }
        if seen.len() > 4 {
            notes.push(format!("php log: … {} distinct lines in total", seen.len()));
        }
    }

    // Delete through the app's own command — every path, including a failed one.
    let db_name = site.as_ref().map(|s| s.db_name.clone());
    let folder = site.as_ref().map(|s| PathBuf::from(&s.path));
    let deleted = commands::sites::delete_site(
        handle.state::<AppState>(),
        handle.state::<commands::tunnels::Tunnels>(),
        site_id.clone(),
    )
    .await;
    if !matches!(deleted, Ok(true)) {
        verdict_problems.push(format!("delete_site → {deleted:?}"));
    }
    if let (Some(db_name), Some(client)) = (db_name, client.as_ref()) {
        let left = engine
            .db_sizes(client, engine.port())
            .map(|s| s.into_iter().any(|(n, _)| n == db_name))
            .unwrap_or(true);
        if left {
            verdict_problems.push(format!("{db_name} survived the delete"));
        }
    }
    if let Some(folder) = folder {
        if folder.starts_with(scratch) && folder.exists() {
            verdict_problems.push(format!("{} survived the delete", folder.display()));
        }
    }

    if verdict_problems.is_empty() {
        Outcome { combo, verdict: "created, served, deleted".into(), pass: true, notes }
    } else {
        fail(combo, verdict_problems.join("; "), notes)
    }
}
