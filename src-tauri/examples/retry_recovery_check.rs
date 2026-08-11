//! Regression check for the RECOVERY promise. Run:
//! `cargo run --example retry_recovery_check`
//!
//! We chose keep-the-site-and-Retry over rolling a failed import back, on the
//! grounds that Retry recovers. That only holds if it is true, and it wasn't:
//! a user's Retry failed exactly as the original import had, and only worked
//! after manually restarting nginx — a step nothing told them to take.
//!
//! So this asserts the substance of Retry at the level it actually broke: with
//! the stack in the broken state (an unusable `nginx.pid` while the master is
//! perfectly healthy), the serve phase's reload must recover on its own AND the
//! site must end up genuinely served.
//!
//! "Genuinely served" is the important part. When a reload fails, nginx keeps
//! its previously loaded config and answers an unknown Host from the DEFAULT
//! server — the first block for that listen address — so a 200 proves nothing.
//! This compares BODIES against another site to tell real serving from the
//! fallback, which is the mistake that nearly derailed the original diagnosis.
//!
//! Runs entirely in a sandbox (throwaway app-data root, fixture port), so the
//! user's stack is untouched.

use rexenv_lib::core::{binaries, services, sites, ssl};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::PathBuf;
use std::time::Duration;

mod common;
use common::Reaped;

const NGINX_PORT: u16 = 18097;
const FIRST: &str = "already-here.rex";
const SECOND: &str = "recovered.rex";

async fn body(host: &str) -> String {
    match reqwest::Client::new()
        .get(format!("http://127.0.0.1:{NGINX_PORT}/marker.txt"))
        .header("Host", host)
        .send()
        .await
    {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(e) => format!("ERROR: {e}"),
    }
}

fn project(root: &PathBuf, name: &str, marker: &str) -> String {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("marker.txt"), marker).unwrap();
    std::fs::write(dir.join("index.php"), "<?php echo 'x';").unwrap();
    dir.display().to_string()
}

#[tokio::main]
async fn main() {
    let (plat, sandbox) = common::sandbox("retry_recovery");
    let mut ok = true;

    let projects = std::env::temp_dir().join("rexenv-retry-recovery");
    let _ = std::fs::remove_dir_all(&projects);
    let first_path = project(&projects, "first", "FIRST-SITE");
    let second_path = project(&projects, "second", "SECOND-SITE");

    let conn = db::open(&sandbox.root().join("retry.db")).expect("db");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let mk = |domain: &str, path: &str| NewSite {
        name: domain.into(),
        domain: domain.into(),
        site_type: SiteType::Php,
        php_version: "8.3".into(),
        web_server: WebServer::Nginx,
        path: path.into(),
        db_engine: SiteDbEngine::Mysql,
        git_url: String::new(),
        git_ref: None,
    };

    println!("=== a site is already being served ===");
    sites::provision(&conn, &*plat, &ca, mk(FIRST, &first_path)).expect("first site");
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, 8081, 8444).unwrap();
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let mut nginx = Reaped::new(
        services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap(),
        NGINX_PORT,
        "nginx",
    );
    tokio::time::sleep(Duration::from_millis(800)).await;
    println!("  {FIRST} -> {}", body(FIRST).await.trim());

    println!("\n=== a second site is imported, and the pid file is unusable ===");
    // Exactly the state the user hit: nginx healthy, pid file empty.
    let pid_file = cfg.nginx_prefix.join("nginx.pid");
    let real_master = std::fs::read_to_string(&pid_file).unwrap_or_default().trim().to_string();
    std::fs::write(&pid_file, "").unwrap();
    println!("  master {real_master} alive; pid file emptied");

    sites::provision(&conn, &*plat, &ca, mk(SECOND, &second_path)).expect("second site");
    let cfg2 = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, 8081, 8444).unwrap();

    println!("\n=== the serve phase's reload must recover UNAIDED ===");
    let outcome = services::reload_nginx(
        &*plat,
        &nginx_bin,
        &cfg2.nginx_conf,
        &cfg2.nginx_prefix,
        NGINX_PORT,
    );
    println!("  reload -> {outcome:?}");
    ok &= matches!(outcome, Ok(services::ReloadOutcome::Reloaded));
    tokio::time::sleep(Duration::from_millis(500)).await;

    println!("\n=== and the site must be GENUINELY served ===");
    let (a, b) = (body(FIRST).await, body(SECOND).await);
    println!("  {FIRST:<18} -> {}", a.trim());
    println!("  {SECOND:<18} -> {}", b.trim());
    let genuinely_served = b.contains("SECOND-SITE");
    // The trap: a failed reload leaves nginx on its old config, where an unknown
    // Host is answered by the DEFAULT server — 200, with the FIRST site's body.
    let not_the_fallback = a.trim() != b.trim();
    println!("  serves its own content : {genuinely_served}");
    println!("  not the default server : {not_the_fallback}");
    ok &= genuinely_served && not_the_fallback;

    println!("\n=== the pid file is healed, so the next reload takes the normal path ===");
    let healed = std::fs::read_to_string(&pid_file).unwrap_or_default().trim().to_string();
    println!("  pid file now: [{healed}] (master {real_master})");
    ok &= !healed.is_empty() && healed == real_master;
    // Prove it: a second reload with no fallback available must still succeed.
    let again = services::reload_nginx(
        &*plat,
        &nginx_bin,
        &cfg2.nginx_conf,
        &cfg2.nginx_prefix,
        NGINX_PORT,
    );
    println!("  reload again -> {again:?}");
    ok &= matches!(again, Ok(services::ReloadOutcome::Reloaded));

    nginx.reap();
    let _ = std::fs::remove_dir_all(&projects);

    if ok {
        println!("\nOK — a site that failed to serve recovers on the reload alone, serves its OWN content, and the pid file heals.");
    } else {
        eprintln!("\nFAILED — Retry does not recover unaided; see above.");
        std::process::exit(1);
    }
}
