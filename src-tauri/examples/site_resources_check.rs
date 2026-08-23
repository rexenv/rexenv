//! Live check for PART 3 per-site resources, against the RUNNING stack:
//!   (1) per-database disk sizes via the bundled mysql client (real numbers),
//!   (2) per-host activity: regenerate the nginx config (now with the rexenv
//!       `$host` log_format) + `nginx -s reload` — the same reload the app
//!       does on any site change — then hit a shared site over the edge and
//!       assert its requests show up in the 60s activity window.
//! Read-mostly: the only mutation is the config regen + reload (idempotent).

use rexenv_lib::core::{binaries, database, db::DbEngine, services, site_metrics, sites, ssl};
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
    // The site's RECORDED database name, never a re-derivation from the domain.
    //
    // `models::Site::db_name`'s own doc says it: derived once at creation and
    // stored, "never re-derived". This loop re-derived it with
    // `wordpress::db_name_for(site_type, domain)` and asserted the result exists
    // — which is true only for sites rexenv CREATED. An IMPORTED site keeps its
    // original database (that is the point of the import: it connects to what
    // was already there), so `photocontest.test` records `photocontest` while
    // the derivation demands `wp_photocontest_test`, and the check failed
    // against a site that was completely healthy — 41 tables, 6 MB.
    //
    // Found 24 Aug 2026 on a machine with five Valet-imported sites. It would
    // fail on ANY install with an imported site, which is the entire cohort the
    // Valet/Herd migration exists for. Textbook recorded-vs-derived
    // (`docs/TESTING.md` §3.1/§3.5).
    // What this can prove, and what it cannot.
    //
    // It CAN prove `db_sizes` reads real databases and that a site's RECORDED
    // name is the one to look them up by. It CANNOT prove every WordPress site
    // has a database: a site imported without one, a half-provisioned site, or
    // one whose database was dropped by hand are all legitimate states on a real
    // machine, and this example runs against the user's real app data.
    //
    // The old loop asserted the second thing, and got the first one wrong on the
    // way: it re-derived the name with `wordpress::db_name_for(site_type,
    // domain)` — which `models::Site::db_name`'s own doc forbids ("derived ONCE
    // at creation and stored, never re-derived") — so it demanded
    // `wp_photocontest_test` from a site recording `photocontest` and failed
    // against a completely healthy 41-table, 6 MB database. That failure was not
    // rare: it fires on ANY install with an imported site, which is the entire
    // cohort the Valet/Herd migration exists for.
    let wp: Vec<_> = all
        .iter()
        .filter(|s| s.site_type == rexenv_lib::state::models::SiteType::Wordpress)
        .collect();
    let mut matched = 0;
    let mut absent = Vec::new();
    for s in &wp {
        assert!(!s.db_name.is_empty(), "{} is a WordPress site with no recorded database name", s.domain);
        match sizes.iter().find(|(n, _)| *n == s.db_name) {
            Some((_, bytes)) => {
                assert!(*bytes > 0, "{} has database {} and it is EMPTY", s.domain, s.db_name);
                matched += 1;
            }
            // Reported, never asserted — and reported by NAME so it is not a
            // silent skip. A number here is the honest limit of an example
            // reading somebody else's data.
            None => absent.push(format!("{} → {}", s.domain, s.db_name)),
        }
    }
    if !absent.is_empty() {
        println!("NOTE: {} WordPress site(s) have no database on this machine:", absent.len());
        for a in &absent {
            println!("  - {a}");
        }
        println!("  (imported without one, half-provisioned, or dropped by hand — all legitimate)");
    }
    // The landmark: a run that matched nothing proves nothing, and would sail
    // past every assertion above by having no site to check.
    assert!(
        matched > 0,
        "no WordPress site's recorded database was found among {} sizes — db_sizes or the \
         recorded names are wrong, not the machine",
        sizes.len()
    );
    println!("✓ {matched} of {} WordPress sites resolved by RECORDED db_name", wp.len());

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
