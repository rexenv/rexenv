//! Phase-1 real-:443 verification, part A (backgroundable — no prompt).
//! Starts MySQL + php-fpm + nginx, provisions + installs WordPress for
//! wpdemo.test, writes the :443 Caddyfile, then holds the services alive so a
//! separate foreground step can bind Caddy on :443 and verify.

use rexenv_lib::core::{binaries, database, services, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "wpdemo.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-p1-443.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let php_fpm = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION).await.unwrap();
    let nginx = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();

    // MySQL.
    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    database::initialize(&*plat, &mysql_base, &datadir).unwrap();
    let _mysqld = database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap();
    for _ in 0..30 {
        if database::mysql_running(database::MYSQL_PORT) { break; }
        std::thread::sleep(Duration::from_millis(500));
    }

    // Provision + install WordPress (single-site), URL on real :443 (no port).
    let site = sites::provision(&conn, &*plat, &ca, NewSite {
        name: "rexenv WP Demo".into(),
        domain: domain.into(),
        site_type: SiteType::Wordpress,
        php_version: "8.3".into(),
        web_server: WebServer::Nginx,
        path: String::new(),
    }).unwrap();
    wordpress::install_wordpress(&php, &wp, &wordpress::WpInstall {
        docroot: std::path::Path::new(&site.path),
        db_name: &wordpress::db_name_for(domain),
        db_host: &format!("127.0.0.1:{}", database::MYSQL_PORT),
        url: &format!("https://{domain}"),
        title: "rexenv WP Demo",
        admin_user: "admin",
        admin_password: "rexenv-admin-pw",
        admin_email: "admin@wpdemo.test",
        locale: "",
    }).unwrap();

    // PHP-FPM (nginx proxies .php here).
    let fpm_conf = services::write_fpm_config(&*plat, "8.3", services::PHP_FPM_PORT, None).unwrap();
    let _fpm = services::start_fpm(&*plat, &php_fpm, &fpm_conf).unwrap();

    // Configs for real :80/:443 (Caddy) + nginx on its internal port.
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, services::NGINX_HTTP_PORT, 80, 443).unwrap();
    let _nginx = services::start_nginx(&*plat, &nginx, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap();
    std::thread::sleep(Duration::from_millis(800));

    println!("READY domain={domain}");
    println!("CADDYFILE={}", cfg.caddyfile.display());
    println!("CA={}", ca.cert_path.display());
    println!(
        "nginx_running={} fpm_running={}",
        services::nginx_running(services::NGINX_HTTP_PORT),
        services::fpm_running(services::PHP_FPM_PORT)
    );

    // Hold the services alive for the foreground :443 step (then it's killed).
    std::thread::sleep(Duration::from_secs(240));
    std::mem::forget(_mysqld);
    std::mem::forget(_fpm);
    std::mem::forget(_nginx);
}
