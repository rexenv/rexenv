//! Live check: creating a site FROM A GIT REPOSITORY, end to end through the
//! REAL provisioning command (`docs/archive/PLAN-git-site-clone.md`, Stages 1–4).
//! Run: `cargo run --example git_site_provision_check`
//!
//! `git_site_clone_check` (sandbox tier) proves the clone/move/cleanup
//! mechanics against fixture repositories it builds itself. This one proves the
//! part that needs the world: real remotes, a real `composer install`, a real
//! database, and the actual phase driver — the same `site_provision_job` the
//! New Site dialog calls, on a `tauri::test::mock_app`.
//!
//! Four cases, one per shape the feature claims to support:
//!   1. **Laravel** from `laravel/laravel` — every phase in order, `.env` wired
//!      to THIS site's database with a real APP_KEY, `vendor/` installed,
//!      migrations actually in MySQL, `docroot_subdir = public`.
//!   2. **Blank PHP** from a repo with no `composer.json` — `deps` reports
//!      SKIPPED rather than failing, and no database is involved at all.
//!   3. **A clone that fails** (a ref that does not exist) — the job fails at
//!      the clone phase, `provisioned = 0`, the docroot is left EMPTY and no
//!      `.rexenv-clone-*` staging directory survives.
//!   4. **Bedrock** from `roots/bedrock` — the layout that was shipped marked
//!      UNVERIFIED (ledger #294). Composer owns core, `.env` owns the config. And (#35) which
//!      mu-plugins WordPress actually LOADS: a probe in the recorded `web/app` does, one in a
//!      hardcoded `web/wp-content` does not, and every `rexenv-*.php` rexenv wrote is in `app`.
//!
//! Edge safety: the fixture manager adopts ONLY the database tier
//! (`adopt_dbs`) — never the edge/nginx — so `is_running()` stays false and the
//! serve phase is SKIPPED. This example can never rebuild the real stack's
//! vhosts from its throwaway database — nor START the real stack: a stopped
//! stack is started only by the app (`stack_guard::may_control_real_stack`,
//! ledger #805; between 18 Sep and 9 Oct 2026 this run called `start_stack`).
//!
//! Cleanup: every site it creates is recorded AS CREATED and torn down from
//! that record — never a derived path (the 24 Jul Sites-folder incident).

use rexenv_lib::commands::{self, site_provision};
use rexenv_lib::core::{database, dotenv, sites, ssl};
use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Listener, Manager};

mod common;

/// The Laravel skeleton itself — the same shape every Laravel repository has
/// (artisan, public/index.php, composer.json, a `.env.example` whose DB block
/// is commented out and whose DB_CONNECTION says sqlite).
const LARAVEL_REPO: &str = "https://github.com/laravel/laravel";
/// A repository with no `composer.json` at all: the deps phase must SKIP.
const PLAIN_REPO: &str = "https://github.com/octocat/Hello-World";
/// Roots' Bedrock, unmodified.
const BEDROCK_REPO: &str = "https://github.com/roots/bedrock";

fn table_count(mysql: &Path, db: &str) -> usize {
    let out = std::process::Command::new(mysql)
        .args([
            "--protocol=TCP",
            "-h",
            "127.0.0.1",
            "-P",
            &database::MYSQL_PORT.to_string(),
            "-u",
            "root",
            "-N",
            "-B",
            "-e",
            &format!("SHOW TABLES FROM `{db}`"),
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).count()
        }
        _ => 0,
    }
}

fn staging_leftovers(sites_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(sites_dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(".rexenv-clone-"))
        .collect()
}

fn entries(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<String> =
        rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect();
    v.sort();
    v
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let plat = rexenv_lib::platform::current();
    let pid = std::process::id();

    let db_file = std::env::temp_dir().join("rexenv-git-provision-check.db");
    let _ = std::fs::remove_file(&db_file);
    let conn = rexenv_lib::state::db::open(&db_file).unwrap();
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let (mysql, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::pins().mysql)
        .await
        .expect("bundled MySQL client");

    let app = tauri::test::mock_app();
    app.manage(commands::repo::RepoJobs::default());
    app.manage(site_provision::ProvisionJobs::default());
    app.manage(AppState::new(conn, plat, ca));
    let handle = app.handle().clone();

    // Was the engine already up? The answer decides whether stopping it at the
    // end is tidy-up or vandalism, and it has to be read BEFORE anything runs.
    //
    // Found the hard way on 21 Aug 2026: with the stack down — which is what the
    // network tier documents — provisioning STARTS MySQL through the app's own
    // path, this example adopted it, asserted its four cases, printed ALL PASS,
    // and exited leaving mysqld on :13306 against the REAL datadir. Thirteen
    // later examples in the same tier run then failed, every one of them naming
    // the port rather than the cause: `start_all failed: port 13306 is still
    // held by a leftover rexenv process`. One green run, thirteen red ones, and
    // the green one was the culprit.
    //
    // Adoption is deliberately not ownership — an adopted pid is nobody's child,
    // so no `Drop` reaches it (services outlive the app, by design). That is
    // correct for the product and is exactly why the example has to say what it
    // caused.
    let _engines = common::engines_as_found();

    // DB tier ONLY — see the module doc. Never the edge.
    {
        let state = handle.state::<AppState>();
        let mut mgr = state.services.lock().await;
        let n = mgr.adopt_dbs(state.platform.as_ref());
        println!("adopted {n} running database engine(s)\n");
    }

    let mut failures: Vec<String> = Vec::new();
    let created: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    // `ONLY=bedrock` runs one case. Each of these takes a real `composer
    // install`, so re-running all four to look at one of them is minutes of
    // somebody's afternoon (the `uireview.js` ONLY= precedent).
    let only = std::env::var("ONLY").unwrap_or_default();
    let want = |k: &str| only.is_empty() || only.contains(k);

    let cloning = |domain: &str, ty: SiteType, url: &str, git_ref: Option<&str>| NewSite {
        name: format!("Git Check {domain}"),
        domain: domain.into(),
        site_type: ty,
        php_version: "8.3".into(),
        web_server: WebServer::Nginx,
        path: String::new(),
        db_engine: SiteDbEngine::Mysql,
        git_url: url.into(),
        git_ref: git_ref.map(str::to_string),
        git_migrate: true,
        // Node is the developer's own toolchain and its phase is non-fatal by
        // design — proving that needs a machine WITHOUT node, which is a
        // SMOKE-TEST item, not something to make every run of this depend on.
        git_build_assets: false,
        starter_db: false,
    };

    let collect = |id: &str| {
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let l2 = lines.clone();
        handle.listen(site_provision::output_event(id), move |ev| {
            if let Ok(s) = serde_json::from_str::<String>(ev.payload()) {
                l2.lock().unwrap().push(s);
            }
        });
        lines
    };
    let wait_settled = |id: String| {
        let handle = handle.clone();
        async move {
            // Composer on a cold cache is minutes, not seconds.
            for _ in 0..2400 {
                let st =
                    site_provision::state_of(&handle.state::<site_provision::ProvisionJobs>(), &id)
                        .unwrap();
                if st.status != "running" {
                    return st;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            panic!("job {id} never settled");
        }
    };
    let site_of = |id: &str| {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        sites::get(&conn, id).unwrap()
    };
    let start = |site: NewSite| {
        let handle = handle.clone();
        let created = created.clone();
        async move {
            let snap = site_provision::site_provision_job(
                handle.clone(),
                handle.state::<AppState>(),
                handle.state::<site_provision::ProvisionJobs>(),
                site,
                None,
                None,
            )
            .await
            .expect("start the provision job");
            created.lock().unwrap().extend(snap.site_id.clone());
            snap
        }
    };
    /// Phase markers must appear in THIS order — our own boundaries, never
    /// parsed from a subprocess.
    fn markers_in_order(lines: &[String], want: &[&str], tag: &str, failures: &mut Vec<String>) {
        let mut at = 0usize;
        for m in want {
            match lines.iter().skip(at).position(|l| l.starts_with(m)) {
                Some(i) => at += i + 1,
                None => failures.push(format!("{tag}: marker {m:?} missing or out of order")),
            }
        }
    }

    // ── 1. Laravel from a real repository ────────────────────────────────
    if want("laravel") {
    println!("=== 1. Laravel from {LARAVEL_REPO} ===");
    let domain_a = format!("gitlv-{pid}.rex");
    let snap = start(cloning(&domain_a, SiteType::Laravel, LARAVEL_REPO, None)).await;
    let lines_a = collect(&snap.id);
    let fin = wait_settled(snap.id.clone()).await;
    let ls = lines_a.lock().unwrap().clone();
    println!("  status={} pct={} summary={:?}", fin.status, fin.pct, fin.summary);
    if fin.status != "ok" || fin.pct != 100 {
        for l in &ls {
            println!("  | {l}");
        }
        failures.push(format!("Laravel: settled {}@{} (want ok@100)", fin.status, fin.pct));
    }
    markers_in_order(
        &ls,
        &[
            "── cloning the repository",
            "── starting database",
            "── creating database + .env",
            "── installing dependencies",
            "── app key + migrations",
            "── starting to serve",
        ],
        "Laravel",
        &mut failures,
    );
    if let Some(site) = fin.site_id.as_deref().and_then(site_of) {
        let project = Path::new(&site.path);
        if !site.provisioned {
            failures.push("Laravel: provisioned flag not set".into());
        }
        if site.docroot_subdir != "public" {
            failures.push(format!("Laravel: docroot_subdir = {:?}", site.docroot_subdir));
        }
        if site.git_url.as_deref() != Some("https://github.com/laravel/laravel") {
            failures.push(format!("Laravel: git_url = {:?}", site.git_url));
        }
        // The name a developer reads in Adminer/TablePlus and in `.env` — a
        // Laravel app is `lv_`, never WordPress's `wp_` (13 Aug 2026).
        if !site.db_name.starts_with("lv_") {
            failures.push(format!("Laravel: db_name = {} (want lv_…)", site.db_name));
        }
        if !project.join("vendor/autoload.php").is_file() {
            failures.push("Laravel: composer install did not produce vendor/autoload.php".into());
        }
        // `.env` really points at THIS site's database, and the app key is real.
        let env_text = std::fs::read_to_string(project.join(".env")).unwrap_or_default();
        for want in [
            format!("DB_DATABASE={}", site.db_name),
            format!("APP_URL=https://{}", site.domain),
            "DB_CONNECTION=mysql".to_string(),
        ] {
            if !env_text.contains(&want) {
                failures.push(format!("Laravel .env missing {want}"));
            }
        }
        if env_text.contains("sqlite") {
            failures.push("Laravel .env still mentions sqlite".into());
        }
        match dotenv::value_of(&env_text, "APP_KEY") {
            Some(k) if k.starts_with("base64:") && k.len() > 20 => {
                println!("  APP_KEY generated ({} chars)", k.len())
            }
            other => failures.push(format!("Laravel APP_KEY = {other:?}")),
        }
        // The migrations are the point: an empty database here means the app
        // talks to a file the Databases screen never shows.
        let tables = table_count(mysql.path(), &site.db_name);
        println!("  {} has {tables} tables", site.db_name);
        if tables == 0 {
            failures.push(format!("Laravel: {} has no tables — migrations did not land", site.db_name));
        }
        // `.env` sits ABOVE what the web server serves.
        if site.served_root() != project.join("public") {
            failures.push(format!("Laravel: served_root = {:?}", site.served_root()));
        }
    } else {
        failures.push("Laravel: no site row".into());
    }

    }

    // ── 2. Blank PHP, a repository with no composer.json ─────────────────
    if want("php") {
    println!("\n=== 2. Blank PHP from {PLAIN_REPO} (no composer.json) ===");
    let domain_b = format!("gitphp-{pid}.rex");
    let snap = start(cloning(&domain_b, SiteType::Php, PLAIN_REPO, None)).await;
    let lines_b = collect(&snap.id);
    let fin = wait_settled(snap.id.clone()).await;
    let ls = lines_b.lock().unwrap().clone();
    println!("  status={} pct={} summary={:?}", fin.status, fin.pct, fin.summary);
    for l in &ls {
        println!("  | {l}");
    }
    if fin.status != "ok" {
        failures.push(format!("Blank PHP: settled {} (want ok)", fin.status));
    }
    if !ls.iter().any(|l| l.contains("no composer.json in this repository")) {
        failures.push("Blank PHP: the deps phase must SKIP with a note, not fail".into());
    }
    let deps_phase = fin.phases.iter().find(|p| p.key == "deps");
    match deps_phase.map(|p| p.status.as_str()) {
        Some("skipped") => {}
        other => failures.push(format!("Blank PHP: deps phase status = {other:?}")),
    }
    if fin.phases.iter().any(|p| p.key == "db") {
        failures.push("Blank PHP: a site with no database must not get a db phase".into());
    }

    }

    // ── 3. A clone that fails leaves nothing behind ──────────────────────
    if want("badref") {
    println!("\n=== 3. a ref that does not exist ===");
    let domain_c = format!("gitbad-{pid}.rex");
    let snap =
        start(cloning(&domain_c, SiteType::Laravel, LARAVEL_REPO, Some("no-such-branch"))).await;
    let fin = wait_settled(snap.id.clone()).await;
    println!("  status={} error={:?}", fin.status, fin.error);
    if fin.status != "failed" {
        failures.push(format!("bad ref: settled {} (want failed)", fin.status));
    }
    if let Some(site) = fin.site_id.as_deref().and_then(site_of) {
        if site.provisioned {
            failures.push("bad ref: provisioned must stay 0".into());
        }
        let docroot = Path::new(&site.path);
        if !entries(docroot).is_empty() {
            failures.push(format!("bad ref: docroot not empty — {:?}", entries(docroot)));
        }
        if let Some(parent) = docroot.parent() {
            let left = staging_leftovers(parent);
            if !left.is_empty() {
                failures.push(format!("bad ref: staging survived — {left:?}"));
            }
        }
        // The row still remembers the repository, which is what makes Retry
        // possible after an app restart.
        if site.git_url.is_none() {
            failures.push("bad ref: the row must still record the repository for Retry".into());
        }
    }

    }

    // ── 4. Bedrock — the layout shipped marked UNVERIFIED ────────────────
    if want("bedrock") {
    println!("\n=== 4. WordPress (Bedrock) from {BEDROCK_REPO} ===");
    let domain_d = format!("gitbr-{pid}.rex");
    let snap = start(cloning(&domain_d, SiteType::Wordpress, BEDROCK_REPO, None)).await;
    let lines_d = collect(&snap.id);
    let fin = wait_settled(snap.id.clone()).await;
    let ls = lines_d.lock().unwrap().clone();
    println!("  status={} pct={} summary={:?}", fin.status, fin.pct, fin.summary);
    for l in &ls {
        println!("  | {l}");
    }
    if fin.status != "ok" {
        failures.push(format!("Bedrock: settled {} (want ok) — {:?}", fin.status, fin.error));
    }
    if !ls.iter().any(|l| l.contains("installs WordPress core through Composer")) {
        failures.push("Bedrock: core_download must report SKIPPED with its reason".into());
    }
    if let Some(site) = fin.site_id.as_deref().and_then(site_of) {
        let project = Path::new(&site.path);
        if site.docroot_subdir != "web" {
            failures.push(format!("Bedrock: docroot_subdir = {:?}", site.docroot_subdir));
        }
        if site.content_dir_rel() != "app" {
            failures.push(format!("Bedrock: content dir = {:?}", site.content_dir_rel()));
        }
        // Composer owns core: exactly one WordPress, at web/wp.
        if !project.join("web/wp/wp-load.php").is_file() {
            failures.push("Bedrock: composer did not install core at web/wp".into());
        }
        if project.join("web/wp-load.php").exists() {
            failures.push("Bedrock: a SECOND WordPress was downloaded into web/".into());
        }
        // The repository's own stub, not a stock wp-config.
        let cfg = std::fs::read_to_string(project.join("web/wp-config.php")).unwrap_or_default();
        if !cfg.contains("application.php") {
            failures.push("Bedrock: web/wp-config.php was overwritten with a stock one".into());
        }
        // `.env` wired, one line per key, all eight salts real.
        let env_text = std::fs::read_to_string(project.join(".env")).unwrap_or_default();
        for want in [format!("DB_NAME={}", site.db_name), format!("WP_HOME=https://{}", site.domain)]
        {
            if !env_text.contains(&want) {
                failures.push(format!("Bedrock .env missing {want}"));
            }
        }
        if !env_text.contains("WP_SITEURL=${WP_HOME}/wp") {
            failures.push("Bedrock .env lost the ${WP_HOME}/wp convention".into());
        }
        for key in rexenv_lib::core::wordpress::SALT_KEYS {
            let n = env_text.lines().filter(|l| l.starts_with(&format!("{key}="))).count();
            if n != 1 {
                failures.push(format!("Bedrock .env: {key} appears {n} times"));
            }
            if dotenv::is_blank(&env_text, key) {
                failures.push(format!("Bedrock .env: {key} left unset"));
            }
        }
        // WordPress actually installed into the site's database.
        let tables = table_count(mysql.path(), &site.db_name);
        println!("  {} has {tables} tables", site.db_name);
        if tables == 0 {
            failures.push(format!("Bedrock: {} has no tables — wp core install did not run", site.db_name));
        }
        if site.served_root() != project.join("web") {
            failures.push(format!("Bedrock: served_root = {:?}", site.served_root()));
        }
        // ── #35 as a committed leg: which mu-plugins does WordPress LOAD? ──
        // Two identical probes: one in the content dir rexenv recorded (`web/app`), one where a
        // hardcoded `wp-content` would have put it (`web/wp-content`). Asked of WordPress itself
        // (`wp_get_mu_plugins()` lists WPMU_PLUGIN_DIR, the only place it loads them from). The app
        // probe is the CONTROL: without it loading, the stray's absence proves nothing (the 24 Aug
        // run's first attempt read a failed request as the premise holding).
        let app_mu = project.join("web/app/mu-plugins");
        let stray_mu = project.join("web/wp-content/mu-plugins");
        let _ = std::fs::create_dir_all(&app_mu);
        let _ = std::fs::create_dir_all(&stray_mu);
        let probe = "<?php // rexenv #35 probe\n";
        let _ = std::fs::write(app_mu.join("rexenv-probe-app.php"), probe);
        let _ = std::fs::write(stray_mu.join("rexenv-probe-stray.php"), probe);
        let state = handle.state::<AppState>();
        let plat = state.platform.as_ref();
        let pins = rexenv_lib::core::binaries::pins();
        let php = rexenv_lib::core::binaries::resolve_program(plat, "php", pins.php).await;
        let phar = rexenv_lib::core::binaries::resolve_file(plat, "wp-cli", pins.wp_cli).await;
        match (php, phar) {
            (Ok(php), Ok(phar)) => {
                let eval = r#"echo implode("\n", array_map("basename", wp_get_mu_plugins()));"#;
                let out = rexenv_lib::core::wordpress::wp_cli(&php, &phar, &["eval", eval], Some(project));
                let loaded: Vec<String> = out
                    .as_ref()
                    .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).collect())
                    .unwrap_or_default();
                println!("  mu-plugins WordPress loads: {loaded:?}");
                if !loaded.iter().any(|m| m == "rexenv-probe-app.php") {
                    failures.push(format!(
                        "Bedrock #35: the probe in web/app/mu-plugins did not load — the check proves nothing ({:?})",
                        out.as_ref().map(|o| String::from_utf8_lossy(&o.stderr).into_owned())
                    ));
                }
                if loaded.iter().any(|m| m == "rexenv-probe-stray.php") {
                    failures.push("Bedrock #35: a mu-plugin in web/wp-content loaded — the content dir is not app".into());
                }
                // rexenv's OWN mu-plugins: written to the recorded dir, none to the stray, each loaded.
                let ours = |dir: &Path| -> Vec<String> {
                    std::fs::read_dir(dir)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .filter(|n| n.starts_with("rexenv-") && !n.starts_with("rexenv-probe-"))
                        .collect()
                };
                let in_app = ours(&app_mu);
                let in_stray = ours(&stray_mu);
                println!("  rexenv's own mu-plugins: web/app {in_app:?}, web/wp-content {in_stray:?}");
                if !in_stray.is_empty() {
                    failures.push(format!("Bedrock #35: rexenv wrote {in_stray:?} into web/wp-content, which never loads"));
                }
                for m in &in_app {
                    if !loaded.contains(m) {
                        failures.push(format!("Bedrock #35: rexenv's {m} sits in web/app/mu-plugins but WordPress did not load it"));
                    }
                }
            }
            (php, phar) => failures.push(format!("Bedrock #35: no PHP/wp-cli to ask WordPress with: {:?} / {:?}", php.err(), phar.err())),
        }
    } else {
        failures.push("Bedrock: no site row".into());
    }

    }

    // ── Cleanup: ONLY this run's own records ─────────────────────────────
    println!();
    let ids = created.lock().unwrap().clone();
    for id in ids {
        let state = handle.state::<AppState>();
        let conn = state.db.lock().unwrap();
        if let Ok(Some(site)) = sites::get(&conn, &id) {
            let _ = rexenv_lib::core::db::DbEngine::Mysql.drop_database(&mysql, database::MYSQL_PORT, &site.db_name);
            let plat = rexenv_lib::platform::current();
            match sites::teardown(&conn, &*plat, &id) {
                Ok(t) if t.existed => {
                    println!("cleaned up {} (docroot removed = {})", site.domain, t.docroot_removed)
                }
                other => println!("teardown {} -> {other:?}", site.domain),
            }
        }
    }
    let _ = std::fs::remove_file(&db_file);

    println!();
    if failures.is_empty() {
        println!("git_site_provision_check: ALL PASS");
        std::process::ExitCode::SUCCESS
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::ExitCode::FAILURE
    }
}
