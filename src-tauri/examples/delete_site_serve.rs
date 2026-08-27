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
            starter_db: false,
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
    // Refuse BEFORE anything is created, not after it has half-run.
    //
    // Found 23 Aug 2026 by running the service tier against a live stack. This
    // example had no port guard: its nginx failed to take :18088 (the user's had
    // it), `await_listening(18088)` then passed AGAINST THE USER'S NGINX because
    // `ports::is_listening` connects, and the run carried on to print
    // `del.test -> HTTP 200` — a fixture reporting that its precondition holds
    // while reading a server it does not own. It died later on an unwrap of a
    // reload whose pid file was empty; had that reload happened to succeed it
    // could have reported green having proven nothing.
    //
    // `scripts/live-checks.sh` now refuses the whole tier with the stack up,
    // which covers the tier. This covers the example someone runs BY HAND, which
    // the runner cannot.
    common::require_ports_free(&[
        (NGINX_PORT, "the SHARED nginx — the user's running stack"),
        (CADDY_HTTPS, "a TLS listener on this example's edge port"),
        (services::PHP_FPM_PORT, "the shared php-fpm 8.3 pool"),
    ]);

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
    // `.unwrap()` here is deliberate, and the reason is not laziness.
    //
    // `docs/TODO.md` filed a sub-item to replace it — "a panic with a backtrace
    // is the wrong shape for 'something else owns this port'", which the error
    // text already says plainly. But the obvious replacement leaks: `fpm`,
    // `nginx` and `caddy` above are `OwnedService` guards that reap in `Drop`,
    // and `std::process::exit` does NOT run destructors. A panic UNWINDS (the
    // profile does not set `panic = "abort"`), so every guard still stops its
    // service. Swapping the panic for a tidy exit would trade a noisy backtrace
    // for three leaked processes on fixed ports — the exact leak class
    // `common::Reaped` exists to end.
    //
    // `common::require_ports_free` can use `process::exit` precisely because it
    // runs BEFORE anything is spawned. After that line, exiting is the unsafe
    // option. That ordering is the rule, not a detail of this file.
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
