//! Phase-3 §1.2 check: creating a WordPress site (one-click install path).
//! Mirrors `commands::sites::create_site`'s WordPress branch using core fns:
//! provision → bring MySQL up → `wordpress::install_for_site` (admin account +
//! title + language from InstallOptions) → bring the shared stack up → verify the
//! site is browsable AND `/wp-login.php` (the wp-admin login) loads over HTTPS.
//!
//! Run (MySQL :13306 free): `cargo run --example wp_create_serve`

use rexenv_lib::core::wordpress::InstallOptions;
use rexenv_lib::core::{binaries, database, proxy, services, sites, ssl, wordpress};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;

mod common;
use std::time::Duration;

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT;
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    // Sandboxed: every path the app derives (config dir, nginx PREFIX and
    // therefore nginx.pid, run/, certs) lands in a throwaway root, so this
    // example cannot touch the running stack. See examples/common.
    let (plat, _sandbox) = common::sandbox("wp_create_serve");
    let domain = "wpcreate.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-3_1_2.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let php_fpm = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION).await.unwrap();
    let nginx = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let caddy = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();

    // MySQL (create_site does this via ServiceManager::ensure_db).
    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    database::initialize(&*plat, &mysql_base, &datadir).unwrap();
    let mut mysqld =
        database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap();
    for _ in 0..30 {
        if database::mysql_running(database::MYSQL_PORT) {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("mysql running={}", database::mysql_running(database::MYSQL_PORT));

    // 1) Provision (filesystem + cert + DB row) — like create_site.
    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "WP Create Check".into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(), db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
        },
    )
    .unwrap();

    // 2) One-click install via the new entry point (dialog fields → InstallOptions).
    let docroot = std::path::PathBuf::from(&site.path);
    let db_host = format!("127.0.0.1:{}", database::MYSQL_PORT);
    wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        &site.domain,
        &site.name,
        &site.db_name,
        &db_host,
        &mysql_base,
        &InstallOptions {
            admin_user: "owner".into(),
            admin_password: "rexenv-pw".into(),
            admin_email: "owner@wpcreate.test".into(),
            ..Default::default() // title→name, language→en_US
        },
    )
    .expect("install_for_site");

    // 3) Bring the shared stack up so the site is browsable.
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    let fpm_conf = services::write_fpm_config(&*plat, "8.3", services::PHP_FPM_PORT, None, &[]).unwrap();
    let mut fpm = services::start_fpm(&*plat, &php_fpm, &fpm_conf).unwrap();
    let mut ngx = services::start_nginx(&*plat, &nginx, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap();
    let mut cad = proxy::start(&*plat, &caddy, &cfg.caddyfile).unwrap();
    std::thread::sleep(Duration::from_millis(1200));

    // 4) Verify over HTTPS (validated against our CA): homepage + wp-admin login.
    let addr: SocketAddr = format!("127.0.0.1:{CADDY_HTTPS}").parse().unwrap();
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(domain, addr)
        .build()
        .unwrap();
    let base = format!("https://{domain}:{CADDY_HTTPS}");

    let home = client.get(format!("{base}/")).send().await.unwrap();
    let home_code = home.status().as_u16();
    let home_body = home.text().await.unwrap_or_default();
    println!("GET / -> {home_code}; title present: {}", home_body.contains("WP Create Check"));

    let login = client.get(format!("{base}/wp-login.php")).send().await.unwrap();
    let login_code = login.status().as_u16();
    let login_body = login.text().await.unwrap_or_default();
    let has_login_form =
        login_body.contains("name=\"log\"") && login_body.contains("name=\"pwd\"");
    println!("GET /wp-login.php -> {login_code}; login form present: {has_login_form}");

    let _ = proxy::stop(&*plat, cad.id()); let _ = cad.wait();
    let _ = services::stop(&*plat, ngx.id()); let _ = ngx.wait();
    let _ = services::stop(&*plat, fpm.id()); let _ = fpm.wait();
    let _ = database::stop(&*plat, mysqld.id()); let _ = mysqld.wait();

    let ok = home_code == 200 && home_body.contains("WP Create Check") && login_code == 200 && has_login_form;
    if ok {
        println!("\nOK — WordPress site created + installed; homepage + wp-admin login load over HTTPS.");
    } else {
        eprintln!("\nFAILED — home={home_code} login={login_code} form={has_login_form}");
        std::process::exit(1);
    }
}
