//! Manual end-to-end check for delete-site teardown (task 7.3).
//! Serves two sites, deletes one, rebuilds configs + reloads nginx/Caddy, and
//! self-verifies (over HTTPS, validated against our CA) that the deleted site no
//! longer serves while the other still does.

use rexenv_lib::core::{binaries, proxy, services, sites, ssl};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;

mod common;
use std::time::Duration;

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT;
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;

fn new_site(name: &str, domain: &str) -> NewSite {
    NewSite {
        name: name.into(),
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
    }
}

async fn probe(client: &reqwest::Client, host: &str) -> String {
    match client.get(format!("https://{host}:{CADDY_HTTPS}/")).send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            let phpinfo = body.contains("phpinfo()");
            format!("HTTP {code}{}", if phpinfo { " (phpinfo)" } else { "" })
        }
        Err(e) => format!("ERROR ({})", if e.is_connect() { "connect" } else { "request" }),
    }
}

#[tokio::main]
async fn main() {
    // Sandboxed: every path the app derives (config dir, nginx PREFIX and
    // therefore nginx.pid, run/, certs) lands in a throwaway root, so this
    // example cannot touch the running stack. See examples/common.
    let (plat, _sandbox) = common::sandbox("delete_site_serve");
    let db_path = std::env::temp_dir().join("rexenv-7_3.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("db");
    // Pin `sites_dir` into the sandbox: `sites::provision` reads the SETTING,
    // which falls back to the home directory, so without this the docroots land
    // in the user's real ~/rexenv/Sites (14 Aug 2026 sweep).
    common::pin_sites_dir(&conn, &*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");

    let del = sites::provision(&conn, &*plat, &ca, new_site("Del", "del.test")).unwrap();
    sites::provision(&conn, &*plat, &ca, new_site("Keep", "keep.test")).unwrap();

    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    let fpm_bin = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION).await.unwrap();
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let caddy_bin = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();

    let fpm_conf = services::write_fpm_config(&*plat, "8.3", services::PHP_FPM_PORT, None, &[]).unwrap();
    // Drop-GUARDED, all three: Rust does not kill a `Child` on drop, and between
    // these spawns and the teardown at the end sit three PANICKING readiness
    // gates and four `.unwrap()`s on the delete path. Any one of them unwinds
    // past a raw `Child` and leaves php-fpm :9783, nginx :18088 and caddy :8443
    // running — the shared production ports, which is why these are
    // `OwnedService` and not `Reaped`: `Reaped`'s name-keyed sweep would kill
    // the user's own nginx.
    let mut fpm = common::OwnedService::new(
        services::start_fpm(&*plat, &fpm_bin, &fpm_conf).unwrap(),
        "php-fpm",
    );
    let mut nginx = common::OwnedService::new(
        services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap(),
        "nginx",
    );
    let mut caddy = common::OwnedService::new(
        proxy::start(&*plat, &caddy_bin, &cfg.caddyfile).unwrap(),
        "caddy",
    );
    // Gate on the sockets, not the clock: all three spawn helpers return at
    // fork, not at bind (`common::await_listening`).
    common::await_listening(services::PHP_FPM_PORT, "php-fpm 8.3", None);
    common::await_listening(NGINX_PORT, "nginx", None);
    common::await_listening(CADDY_HTTPS, "the caddy edge", None);
    // ...and for it to ANSWER, before the BEFORE-delete baseline is taken. That
    // baseline is what the whole check rests on: if `keep.test` reads
    // `ERROR (connect)` because the request landed inside Caddy's load window,
    // the AFTER comparison still "passes" while proving nothing at all.
    common::await_answering(
        "keep.test",
        CADDY_HTTPS,
        &ca.cert_path,
        "the caddy edge answering HTTPS",
    );

    let addr: SocketAddr = format!("127.0.0.1:{CADDY_HTTPS}").parse().unwrap();
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve("del.test", addr)
        .resolve("keep.test", addr)
        .build()
        .unwrap();

    println!("BEFORE delete:");
    println!("  del.test  -> {}", probe(&client, "del.test").await);
    println!("  keep.test -> {}", probe(&client, "keep.test").await);

    // Delete del.test: DB row + cert + docroot, then rebuild configs + reload.
    let removed = sites::teardown(&conn, &*plat, &del.id).unwrap();
    let cfg2 = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    services::reload_nginx(&*plat, &nginx_bin, &cfg2.nginx_conf, &cfg2.nginx_prefix, NGINX_PORT).unwrap();
    proxy::reload(&*plat, &caddy_bin, &cfg2.caddyfile, false).unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;

    println!(
        "existed={} docroot_removed={}; remaining sites={:?}",
        removed.existed,
        removed.docroot_removed,
        sites::list(&conn).unwrap().iter().map(|s| s.domain.clone()).collect::<Vec<_>>()
    );
    println!("AFTER delete + reload:");
    println!("  del.test  -> {}", probe(&client, "del.test").await);
    println!("  keep.test -> {}", probe(&client, "keep.test").await);
    println!(
        "  del docroot exists={} cert exists={}",
        std::path::Path::new(&del.path).exists(),
        ssl::site_cert_dir(plat.paths(), "del.test").map(|d| d.exists()).unwrap_or(false),
    );

    // The guards' own idempotent shutdown, so the happy path and the panic path
    // tear down through the same code.
    caddy.stop();
    nginx.stop();
    fpm.stop();
    println!("stopped");
}
