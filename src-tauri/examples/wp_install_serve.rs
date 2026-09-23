//! Phase-1 goal check (task 9.2): one-click WordPress over HTTPS.
//! Starts MySQL + php-fpm + nginx + Caddy, provisions a WordPress site, installs
//! it via WP-CLI (single-site), and self-verifies the homepage over HTTPS
//! (validated against our local CA).

use rexenv_lib::core::{binaries, database, proxy, services, sites, ssl, wordpress};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;

mod common;

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT;
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    // Sandboxed: every path the app derives (config dir, nginx PREFIX and
    // therefore nginx.pid, run/, certs) lands in a throwaway root, so this
    // example cannot touch the running stack. See examples/common.
    // FIRST statement, before anything is created: `common::sandbox` makes the
    // PATHS throwaway and does NOTHING about ports. These are the production
    // ports, so beside a live stack this example's three readiness gates would
    // all be satisfied by the USER'S services — green, and measuring their
    // machine rather than rexenv's behaviour. The sibling `wp_create_serve`
    // already refuses this way; this one never did.
    common::require_ports_free(&[
        (CADDY_HTTPS, "this example's edge"),
        (CADDY_HTTP, "this example's HTTP edge"),
        (NGINX_PORT, "the SHARED nginx — the user's running stack"),
        (services::PHP_FPM_PORT, "a php-fpm pool"),
        (database::MYSQL_PORT, "MySQL"),
    ]);
    let (plat, _sandbox) = common::sandbox("wp_install_serve");
    let domain = "wpdemo.test";
    let url = format!("https://{domain}:{CADDY_HTTPS}");

    let conn = {
        let p = std::env::temp_dir().join("rexenv-9_2.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    // Pin `sites_dir` into the sandbox: `sites::provision` reads the SETTING,
    // which falls back to the home directory, so without this the docroots land
    // in the user's real ~/rexenv/Sites (14 Aug 2026 sweep).
    common::pin_sites_dir(&conn, &*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    // Resolve everything we need.
    let php = binaries::resolve(&*plat, "php", binaries::pins().php).await.unwrap();
    let php_fpm = binaries::resolve(&*plat, "php-fpm", binaries::pins().php).await.unwrap();
    let nginx = binaries::resolve(&*plat, "nginx", binaries::pins().nginx).await.unwrap();
    let caddy = binaries::resolve(&*plat, "caddy", binaries::pins().caddy).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::pins().mysql).await.unwrap();
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::pins().mysql)
        .await
        .expect("bundled MySQL client");

    // MySQL.
    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    database::initialize(&*plat, &mysql_base, &datadir).unwrap();
    // Drop-GUARDED, all four services below. Rust does not kill a `Child` on
    // drop, and this example now has PANICKING readiness gates between every
    // spawn and its teardown — so an unwind through a raw `Child` would leave
    // mysqld :13306, php-fpm :9783, nginx :18088 and caddy :8443 running, which
    // is every shared production port this tier uses.
    let mut mysqld = common::OwnedService::new(
        database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap(),
        "mysqld",
    );
    // `await_ready`, not the poll-then-carry-on loop this replaced. That loop fell
    // THROUGH after 15s, printed `mysql running=false`, and let the example continue
    // into `install_for_site` — so a dead engine was reported as one line of output in
    // the middle of a run that then failed on wp-cli's error instead of MySQL's.
    // `mysql_running` is a protocol check rather than a port listen, which is why this
    // is `await_ready` and not `await_listening`.
    common::await_ready("mysqld (accepting queries)", None, || {
        database::mysql_running(database::MYSQL_PORT)
    });

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
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .unwrap();

    // Shared web stack.
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    let fpm_conf = services::write_fpm_config(&*plat, "8.3", services::PHP_FPM_PORT, None, &[]).unwrap();
    let mut fpm = common::OwnedService::new(
        services::start_fpm(&*plat, &php_fpm, &fpm_conf).unwrap(),
        "php-fpm",
    );
    let mut ngx = common::OwnedService::new(
        services::start_nginx(&*plat, &nginx, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap(),
        "nginx",
    );
    let mut cad = common::OwnedService::new(
        proxy::start(&*plat, &caddy, &cfg.caddyfile).unwrap(),
        "caddy",
    );
    // Gate on the sockets, not the clock: all three spawn helpers return at
    // fork, not at bind (`common::await_listening`).
    common::await_listening(services::PHP_FPM_PORT, "php-fpm 8.3", None);
    common::await_listening(NGINX_PORT, "nginx", None);
    common::await_listening(CADDY_HTTPS, "the caddy edge", None);

    // One-click WordPress install.
    println!("installing WordPress…");
    let docroot = std::path::PathBuf::from(&site.path);
    let db_name = wordpress::db_name_for(SiteType::Wordpress, domain);
    wordpress::install_wordpress(
        &php,
        &wp,
        &wordpress::WpInstall {
            docroot: &docroot,
            db_name: &db_name,
            db_host: &format!("127.0.0.1:{}", database::MYSQL_PORT),
            db_client: &db_client,
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

    // The guards' own idempotent shutdown — same code on the happy path and the
    // panic path.
    cad.stop();
    ngx.stop();
    fpm.stop();
    mysqld.stop();
    println!("stopped");
}
