//! Manual end-to-end check for the create-site flow (task 7.1).
//! Provisions a Blank-PHP site (DB row + docroot + cert), rebuilds the shared
//! nginx + Caddy configs from the DB, starts php-fpm + nginx + Caddy (high ports,
//! no root), and serves https://blankphp.test:8443 for ~15s. Probe:
//!   curl --resolve blankphp.test:8443:127.0.0.1 --cacert <ca> https://blankphp.test:8443/
//!   (issuer = rexenv Local CA; body = phpinfo HTML)

use rexenv_lib::core::{binaries, proxy, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::thread;
use std::time::Duration;

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT; // 18088
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "blankphp.test";

    // Isolated temp DB so the example is repeatable.
    let db_path = std::env::temp_dir().join("rexenv-7_1.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("open db");

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
    let mut fpm = services::start_fpm(&*plat, &fpm_bin, &fpm_conf).expect("fpm");
    services::test_nginx_config(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)
        .expect("nginx -t");
    let mut nginx = services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)
        .expect("nginx");
    let mut caddy = proxy::start(&*plat, &caddy_bin, &cfg.caddyfile).expect("caddy");

    thread::sleep(Duration::from_secs(1));
    println!(
        "READY https://{domain}:{CADDY_HTTPS}  fpm={} nginx={}",
        services::fpm_running(services::PHP_FPM_PORT),
        services::nginx_running(NGINX_PORT),
    );

    thread::sleep(Duration::from_secs(15));

    let _ = proxy::stop(&*plat, caddy.id());
    let _ = caddy.wait();
    let _ = services::stop(&*plat, nginx.id());
    let _ = nginx.wait();
    let _ = services::stop(&*plat, fpm.id());
    let _ = fpm.wait();
    println!("stopped");
}
