//! Manual end-to-end check for the create-site flow (task 7.1).
//! Provisions a Blank-PHP site (DB row + docroot + cert), rebuilds the shared
//! nginx + Caddy configs from the DB, starts php-fpm + nginx + Caddy (high ports,
//! no root), and serves https://blankphp.test:8443 for ~15s. Probe:
//!   curl --resolve blankphp.test:8443:127.0.0.1 --cacert <ca> https://blankphp.test:8443/
//!   (issuer = rexenv Local CA; body = phpinfo HTML)

use rexenv_lib::core::{binaries, proxy, services, sites, ssl};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::thread;

mod common;
use std::time::Duration;

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT; // 18088
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    // Sandboxed: every path the app derives (config dir, nginx PREFIX and
    // therefore nginx.pid, run/, certs) lands in a throwaway root, so this
    // example cannot touch the running stack. See examples/common.
    let (plat, _sandbox) = common::sandbox("create_site_serve");
    let domain = "blankphp.test";

    // Isolated temp DB so the example is repeatable.
    let db_path = std::env::temp_dir().join("rexenv-7_1.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("open db");
    // Pin `sites_dir` into the sandbox: `sites::provision` reads the SETTING,
    // which falls back to the home directory, so without this the docroots land
    // in the user's real ~/rexenv/Sites (14 Aug 2026 sweep).
    common::pin_sites_dir(&conn, &*plat);

    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");

    // 1) create the site (DB row + docroot + index.php phpinfo + cert).
    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Blank PHP".into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
        },
    )
    .expect("provision site");
    println!("provisioned: {} -> {}", site.domain, site.path);

    // 2) rebuild shared configs from all sites.
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS)
        .expect("rebuild configs");

    // 3) resolve binaries + start services.
    let fpm_bin = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION).await.unwrap();
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let caddy_bin = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();

    let fpm_conf = services::write_fpm_config(&*plat, "8.3", services::PHP_FPM_PORT, None, &[]).unwrap();
    // Drop-GUARDED, all three. Rust does not kill a `Child` on drop, and the
    // readiness gates below exit by PANICKING — so between the spawn and the
    // teardown there is now an unwinding path that a raw `Child` would leak
    // through, holding :9783/:18088/:8443 and poisoning every later example in
    // the tier. Same fix, same reason, as `frankenphp_edge_serve` (48e5046).
    let mut fpm = common::OwnedService::new(
        services::start_fpm(&*plat, &fpm_bin, &fpm_conf).expect("fpm"),
        "php-fpm",
    );
    services::test_nginx_config(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)
        .expect("nginx -t");
    let mut nginx = common::OwnedService::new(
        services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)
            .expect("nginx"),
        "nginx",
    );
    let mut caddy = common::OwnedService::new(
        proxy::start(&*plat, &caddy_bin, &cfg.caddyfile).expect("caddy"),
        "caddy",
    );

    // Gate on the sockets, not the clock: all three spawn helpers return at
    // fork, not at bind. The READY line below advertises a URL for a human to
    // curl, so announcing it early is announcing a 502.
    common::await_listening(services::PHP_FPM_PORT, "php-fpm 8.3", None);
    common::await_listening(NGINX_PORT, "nginx", None);
    common::await_listening(CADDY_HTTPS, "the caddy edge", None);
    println!(
        "READY https://{domain}:{CADDY_HTTPS}  fpm={} nginx={}",
        services::fpm_running(services::PHP_FPM_PORT),
        services::nginx_running(NGINX_PORT),
    );

    thread::sleep(Duration::from_secs(15));

    // `stop()` is the guard's own idempotent shutdown — the same code its Drop
    // runs, so the happy path and the panic path tear down identically instead
    // of the happy path keeping a hand-written duplicate.
    caddy.stop();
    nginx.stop();
    fpm.stop();
    println!("stopped");
}
