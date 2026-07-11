//! Phase-1 goal check (task 9.2): one-click WordPress over HTTPS.
//! Starts MySQL + php-fpm + nginx + Caddy, provisions a WordPress site, installs
//! it via WP-CLI (single-site), and self-verifies the homepage over HTTPS
//! (validated against our local CA).

use rexenv_lib::core::{binaries, database, proxy, services, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::time::Duration;

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT;
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "wpdemo.test";
    let url = format!("https://{domain}:{CADDY_HTTPS}");

    let conn = {
        let p = std::env::temp_dir().join("rexenv-9_2.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    // Resolve everything we need.
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let php_fpm = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION).await.unwrap();
    let nginx = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let caddy = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();

    // MySQL.
    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    database::initialize(&*plat, &mysql_base, &datadir).unwrap();
    let mut mysqld = database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap();
    for _ in 0..30 {
        if database::mysql_running(database::MYSQL_PORT) { break; }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("mysql running={}", database::mysql_running(database::MYSQL_PORT));

    // Provision the WordPress site (docroot + cert + DB row).
    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "rexenv WP Demo".into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
        },
    )
    .unwrap();

    // Shared web stack.
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    let fpm_conf = services::write_fpm_config(&*plat, "8.3", services::PHP_FPM_PORT, None, &[]).unwrap();
    let mut fpm = services::start_fpm(&*plat, &php_fpm, &fpm_conf).unwrap();
    let mut ngx = services::start_nginx(&*plat, &nginx, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap();
    let mut cad = proxy::start(&*plat, &caddy, &cfg.caddyfile).unwrap();
    std::thread::sleep(Duration::from_millis(1200));

    // One-click WordPress install.
    println!("installing WordPress…");
    let docroot = std::path::PathBuf::from(&site.path);
    let db_name = wordpress::db_name_for(domain);
    wordpress::install_wordpress(
        &php,
        &wp,
        &wordpress::WpInstall {
            docroot: &docroot,
            db_name: &db_name,
            db_host: &format!("127.0.0.1:{}", database::MYSQL_PORT),
            mysql_basedir: &mysql_base,
            url: &url,
            title: "rexenv WP Demo",
            admin_user: "admin",
            admin_password: "rexenv-admin-pw",
            admin_email: "admin@wpdemo.test",
            locale: "",
        },
    )
    .expect("install wordpress");

    let siteurl = wordpress::wp_cli_checked(&php, &wp, &["option", "get", "siteurl", &format!("--path={}", docroot.display())], None)
        .unwrap_or_default();
    println!("wp siteurl = {}", siteurl.trim());

    // Self-verify the homepage over HTTPS, validated against our CA.
    let addr: SocketAddr = format!("127.0.0.1:{CADDY_HTTPS}").parse().unwrap();
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(domain, addr)
        .build()
        .unwrap();
    match client.get(format!("{url}/")).send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            println!("GET {url}/ -> HTTP {code}");
            println!("  title present: {}", body.contains("rexenv WP Demo"));
            println!("  wp-content present: {}", body.contains("wp-content"));
            println!("  WP generator: {}", body.contains("WordPress"));
        }
        Err(e) => println!("request error: {e}"),
    }

    let _ = proxy::stop(&*plat, cad.id()); let _ = cad.wait();
    let _ = services::stop(&*plat, ngx.id()); let _ = ngx.wait();
    let _ = services::stop(&*plat, fpm.id()); let _ = fpm.wait();
    let _ = database::stop(&*plat, mysqld.id()); let _ = mysqld.wait();
    println!("stopped");
}
