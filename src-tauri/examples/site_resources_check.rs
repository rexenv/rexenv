//! Live check for PART 3 per-site resources, against the RUNNING stack:
//!   (1) per-database disk sizes via the bundled mysql client (real numbers),
//!   (2) per-host activity: regenerate the nginx config (now with the rexenv
//!       `$host` log_format) + `nginx -s reload` — the same reload the app
//!       does on any site change — then hit a shared site over the edge and
//!       assert its requests show up in the 60s activity window.
//! Read-mostly: the only mutation is the config regen + reload (idempotent).

use rexenv_lib::core::{binaries, database, db::DbEngine, services, site_metrics, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let db_path = plat.paths().app_data_dir().unwrap().join("rexenv.db");
    let conn = db::open(&db_path).unwrap();
    let all = sites::list(&conn).unwrap();
    assert!(!all.is_empty(), "no sites — create one first");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    // (1) Real per-DB disk sizes.
    assert!(DbEngine::Mysql.running(), "MySQL must be running (Start all)");
    // The client via `cached_sql_client` — production's own status-poll path.
    // The previous version hand-built `bin_dir/mysql-<v>` (the TREE) and passed
    // it to db_sizes, which execs it: EACCES before any DB contact. A tenth
    // victim of the tree-vs-client class, found by the SqlClient type change —
    // the 14 Aug sweep grepped for mysql_client_bin and this file never called it.
    let client = DbEngine::Mysql
        .cached_sql_client(&*plat, binaries::MYSQL_VERSION)
        .expect("bundled MySQL client cached (Start all downloads it)");
    let sizes = database::db_sizes(&client, DbEngine::Mysql.port()).unwrap();
    println!("== db sizes ==");
    for (name, bytes) in &sizes {
        println!("{name:<30} {:>8.1} MB", *bytes as f64 / 1e6);
    }
    for s in all.iter().filter(|s| s.site_type == rexenv_lib::state::models::SiteType::Wordpress) {
        let dbn = wordpress::db_name_for(s.site_type, &s.domain);
        assert!(
            sizes.iter().any(|(n, b)| *n == dbn && *b > 0),
            "expected a non-empty database {dbn} for {}",
            s.domain
        );
    }

    // (2) Activity: roll out the rexenv log_format (config regen + reload —
    // exactly what the app does on any site change), then generate traffic.
    let cfg = sites::rebuild_configs_for(&all, &*plat, &ca, services::NGINX_HTTP_PORT, 80, 443, &Default::default(), &Default::default()).unwrap();
    let nginx_bin = plat
        .paths()
        .bin_dir()
        .unwrap()
        .join(format!("nginx-{}", binaries::NGINX_VERSION))
        .join("nginx");
    services::reload_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix, services::NGINX_HTTP_PORT).unwrap();
    // Let the old worker drain — a request raced right at the signal still
    // logs in the old format and wouldn't be attributed.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    println!("\nnginx reloaded with the rexenv log_format");

    let target = all.first().unwrap();
    let ca_pem = std::fs::read(plat.paths().app_data_dir().unwrap().join("ca").join("rexenv-ca.pem")).unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem).unwrap())
        .resolve(&target.domain, "127.0.0.1:443".parse().unwrap())
        .build()
        .unwrap();
    for _ in 0..5 {
        let r = client
            .get(format!("https://{}/", target.domain))
            .send()
            .await
            .expect("request shared site");
        assert!(r.status().is_success() || r.status().is_redirection());
    }

    let log = plat.paths().log_dir().unwrap().join("nginx-access.log");
    let act = site_metrics::activity_by_host(&log, time::OffsetDateTime::now_utc());
    println!("\n== activity (last 60s) ==");
    for (host, a) in &act {
        println!("{host:<24} {:>3} req  {:>8} bytes", a.requests, a.bytes);
    }
    let hit = act.get(&target.domain).copied().unwrap_or_default();
    assert!(
        hit.requests >= 5,
        "expected ≥5 requests for {} in the window, got {}",
        target.domain,
        hit.requests
    );

    println!("\nLIVE CHECK PASSED — real DB sizes + per-host activity attribution");
}
