//! QA P0-3 check: PHP `mail()` still reaches Mailpit after the pool is
//! restarted by a settings edit in an ADOPTED session.
//!
//! Since 4 Sep 2026 it holds BOTH halves of the catch-all: the `sendmail_path`
//! shim (WordPress's `mail()`) and `env[MAIL_*]` (Laravel, which never reads
//! php.ini for its transport — ledger #504). They travel as one value, so a
//! restart that kept one and lost the other is the failure worth catching.
//!
//! The bug: only `start_all` derived the Mailpit sendmail shim into the pools'
//! session state — a session that ADOPTED a running stack had `None`, so any
//! pool restart (settings edit, patch bump, watchdog respawn) rewrote that
//! minor's fpm config WITHOUT `php_admin_value[sendmail_path]` and its `mail()`
//! silently bypassed Mailpit (observed on a PHP 8.3 site). Replays the exact
//! sequence:
//!   1. manager A: provision a PHP 8.3 site + `start_all` (mail works);
//!   2. manager B (fresh, = app relaunch): `adopt_startup` the live stack;
//!   3. B applies a PHP setting for 8.3 → that pool restarts;
//!   4. the rewritten `php-fpm-8.3.conf` must STILL pin `sendmail_path`, and a
//!      `mail()` from the site must land in Mailpit.
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free):
//! `cargo run --example mail_adopt_settings_check`

use rexenv_lib::core::service_manager::{await_ready, Ports, ServiceManager};
use rexenv_lib::core::{binaries, mail, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::collections::HashMap;
use std::time::Duration;

mod common;

const HTTPS: u16 = 8443;
const MINOR: &str = "8.3";
const SUBJECT: &str = "adopted-restart mail test";

#[tokio::main]
async fn main() {
    // Deliberate real-stack control: this utility exists to adopt/stop the
    // shared stack. Without this, core::stack_guard skips adopted services.
    rexenv_lib::core::stack_guard::allow_real_stack_control();
    let plat = platform::current();
    let domain = "mailadopt.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-p0-3.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    // Fixture-owned docroot. `sites::provision` reads the `sites_dir` SETTING,
    // which falls back to a path derived from $HOME — so without this the site
    // lands in the user's real ~/rexenv/Sites and SURVIVES into the next run.
    // That is not untidy, it is the bug: two checks failed on their own
    // leftovers on 21 Aug 2026 (`wp_tools_check`, `wp_themes_check`).
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "mailadopt");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Mail Adopt".into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            php_version: MINOR.into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .unwrap();
    let all = sites::list(&conn).unwrap();
    let site = all.iter().find(|s| s.domain == domain).unwrap();
    std::fs::write(
        format!("{}/mailtest.php", site.path),
        format!(
            "<?php $ok = mail('catch@rexenv.test', '{SUBJECT}', 'after adopted restart'); \
             echo $ok ? 'SENT' : 'FAIL';"
        ),
    )
    .unwrap();

    // 1. Session A: start everything EXCEPT the edge (`start_core`) — the mail
    //    path (nginx → fpm → sendmail shim → Mailpit) never touches Caddy, and
    //    skipping the edge keeps this example away from a REAL KeepAlive edge
    //    daemon (adopt-reloading it would push this example's config into it).
    let ports = Ports { http: 8080, https: HTTPS, nginx: services::NGINX_HTTP_PORT };
    let mut a = ServiceManager::with_ports(ports.clone());
    let minors = vec![MINOR.to_string()];
    // Mirror real app boot: adopt rexenv-owned survivors (e.g. a MySQL that
    // outlived a quit app) so the port gates see them as ours.
    let pre = a.adopt_startup(&*plat, &all, true);
    println!("session A: adopted {pre} survivor(s), starting core stack…");
    match a.start_core(&*plat, &ca, &all, &minors, binaries::ADMINER_VERSION, true).await {
        Ok((_caddyfile, checks)) => await_ready(checks).await.expect("core stack ready"),
        Err(e) => {
            eprintln!("start_core failed: {e}");
            return;
        }
    }

    // 2. Session B: a fresh manager, as after an app relaunch — ADOPT, don't start.
    //    (A's handles are dropped without stopping anything, like a quit app.)
    std::mem::forget(a);
    let mut b = ServiceManager::with_ports(ports.clone());
    let adopted = b.adopt_startup(&*plat, &all, true);
    println!("session B: adopted {adopted} service(s)");
    assert!(adopted > 0, "nothing adopted — stack not up?");

    // 3. Settings edit for the 8.3 pool IN THE ADOPTED SESSION → pool restart.
    let mut settings: HashMap<String, Vec<(String, String)>> = HashMap::new();
    settings.insert(MINOR.into(), vec![("memory_limit".into(), "256M".into())]);
    let checks = b
        .apply_php_settings(&*plat, &ca, &all, settings, MINOR)
        .await
        .expect("apply_php_settings in the adopted session");
    // A skipped restart (pool not adopted — e.g. racing a previous run's
    // teardown) would pass 4a vacuously; require the restart actually happened.
    assert!(!checks.is_empty(), "PHP {MINOR} pool was not adopted, nothing restarted");
    await_ready(checks).await.expect("restarted pool ready");
    println!("session B: applied memory_limit=256M to PHP {MINOR} (pool restarted)");

    // 4a. The rewritten config must still pin the shim.
    let conf = plat.paths().config_dir().unwrap().join(format!("php-fpm-{MINOR}.conf"));
    let conf_text = std::fs::read_to_string(&conf).unwrap();
    assert!(
        conf_text.contains("php_admin_value[sendmail_path]"),
        "sendmail_path DROPPED from {} after the adopted-session restart",
        conf.display()
    );
    // BOTH halves, because the catch-all is one fact and a config carrying only
    // the shim looks entirely correct while every Laravel site on this pool
    // mails the real internet (ledger #504). Each key is checked, and QUOTED —
    // a bare `null` is `ERROR: empty value` and the pool would not have started.
    for (k, v) in mail::laravel_env() {
        assert!(
            conf_text.contains(&format!("env[{k}] = \"{v}\"")),
            "env[{k}] DROPPED from {} after the adopted-session restart",
            conf.display()
        );
    }
    assert!(conf_text.contains("memory_limit"), "new setting missing from config");
    println!("✓ {} keeps sendmail_path alongside the new setting", conf.display());

    // 4b. And mail() from the restarted pool still lands in Mailpit. Request
    //     the shared nginx DIRECTLY (Host-routed vhost) — the sendmail path
    //     is nginx → fpm → shim; the edge adds nothing here.
    let plain = reqwest::Client::new();
    let _ = plain.delete(format!("{}/api/v1/messages", mail::api_base())).send().await;
    let body = plain
        .get(format!("http://127.0.0.1:{}/mailtest.php", services::NGINX_HTTP_PORT))
        .header("Host", domain)
        .send()
        .await
        .expect("request mailtest.php via nginx")
        .text()
        .await
        .unwrap_or_default();
    assert!(body.contains("SENT"), "mail() did not report success: {body:?}");
    std::thread::sleep(Duration::from_millis(600));
    let listing = plain
        .get(format!("{}/api/v1/messages", mail::api_base()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(listing.contains(SUBJECT), "Mailpit did not capture {SUBJECT:?}");
    println!("✓ mail() from the restarted {MINOR} pool captured by Mailpit");

    b.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — sendmail routing survives adopted-session pool restarts.");
}
