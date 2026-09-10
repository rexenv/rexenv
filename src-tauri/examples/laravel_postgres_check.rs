//! Manual check: **can the PHP rexenv ships reach PostgreSQL at all?**
//! Run: `cargo run --example laravel_postgres_check`
//!
//! Laravel's `pgsql` connection and the Blank-PHP starter's `db.php` both go
//! through PDO, so step (b) of docs/PLAN-postgres-sites.md rests entirely on one
//! fact about a bundled binary — exactly the kind of fact only this layer can
//! establish. Measured 9 Sep 2026 on static-php.dev's builds, the answer was NO:
//!
//!   - `pg_connect()` (ext/pgsql) connects and queries;
//!   - the bundled `psql` connects (which is why every op in `core::postgres`
//!     works);
//!   - **`new PDO("pgsql:…")` accepts the socket and never sends its startup
//!     packet**, so the server closes it on `authentication_timeout` — 60s
//!     later on a stock cluster, as `SQLSTATE[08006] server closed the
//!     connection unexpectedly`. The build agrees: `pdo_pgsql` is not loaded,
//!     while `PDO::getAvailableDrivers()` advertises `pgsql`.
//!
//! `rexenv/runtimes` now builds 8.1-8.5 with the driver (release `php-8x-1`, 10
//! Sep 2026) and 7.4/8.0 still come from builds without it, so the answer is
//! per MINOR — and this check asks EVERY installed one, against what
//! `core::php::pdo_pgsql_supported` records. Written the way `wp_dns_check` is
//! (ledger #381): **it goes red on DISAGREEMENT with the record**, in either
//! direction, not on a bug appearing or going away. A minor that gains the
//! driver upstream is as much a finding as one that loses it.
//!
//! The cluster is started with `authentication_timeout=5` so measuring a STALL
//! (the failure shape on a minor without the driver) costs five seconds rather
//! than a minute.
//!
//! Fixture-owned: a sandbox app-data root (so `initdb` builds a THROWAWAY
//! cluster, never the app's), a `Reaped` guard for the server, and a probe
//! database dropped before the server is stopped.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{binaries, laravel, php};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

mod common;

const DB: &str = "rex_lvpg_check";

/// `php -r <code>` with a bounded wait, returning (ok, stdout+stderr).
fn php_says(php: &Path, code: &str) -> (bool, String) {
    let out = std::process::Command::new(php)
        .args(["-r", code])
        .output()
        .expect("run php");
    let mut said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        said.push_str(String::from_utf8_lossy(&out.stderr).trim());
    }
    (out.status.success(), said)
}

#[tokio::main]
async fn main() {
    // `sandbox` redirects paths and says nothing about ports — refuse rather
    // than talk to the user's own PostgreSQL.
    common::require_ports_free(&[(DbEngine::Postgres.port(), "PostgreSQL")]);
    let (plat, _guard) = common::sandbox("lvpg");
    let engine = DbEngine::Postgres;
    let port = engine.port();
    let mut ok = true;

    let (client, _dump) = engine
        .sql_client_bins(&*plat, engine.default_version())
        .await
        .expect("psql");

    println!("=== a throwaway cluster (authentication_timeout=5) ===");
    let datadir = engine.data_dir(&*plat, engine.default_version()).expect("datadir");
    let basedir = binaries::resolve_dir(&*plat, "postgres", engine.default_version())
        .await
        .expect("postgres tree");
    rexenv_lib::core::postgres::initialize(&*plat, &basedir, &datadir).expect("initdb");
    let child = plat
        .supervisor()
        .spawn_logged(
            &rexenv_lib::core::postgres::postgres_bin(&basedir),
            &[
                "-D".to_string(),
                datadir.display().to_string(),
                "-p".to_string(),
                port.to_string(),
                "-c".to_string(),
                "listen_addresses=127.0.0.1".to_string(),
                "-c".to_string(),
                "unix_socket_directories=".to_string(),
                // The whole reason this example can be run in seconds: the
                // stall it measures ends at the server's auth timeout.
                "-c".to_string(),
                "authentication_timeout=5".to_string(),
            ],
            &plat.paths().log_dir().expect("log dir").join("postgres-stdout.log"),
        )
        .expect("start postgres");
    let mut server = common::Reaped::new(child, port, "postgres");
    for _ in 0..60 {
        if engine.running() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    println!("  pid {} · listening on :{port} = {}", server.id(), engine.running());
    ok &= engine.running();
    ok &= engine.create_database(&client, port, DB).is_ok();

    // Every PHP the app can run, not just the default: the record is a claim
    // about the RUNTIME, and a version that differs is the interesting case.
    println!("\n=== what each bundled PHP says about pdo_pgsql ===");
    let mut phps = Vec::new();
    for v in binaries::PHP_VERSIONS {
        match binaries::resolve(&*plat, "php", v).await {
            Ok(bin) => phps.push((*v, bin)),
            Err(e) => {
                eprintln!("  {v}: could not resolve ({e})");
                ok = false;
            }
        }
    }
    for (v, bin) in &phps {
        let (_, said) = php_says(
            bin,
            "echo extension_loaded('pdo_pgsql') ? 'loaded' : 'absent', '|', \
             in_array('pgsql', PDO::getAvailableDrivers()) ? 'advertised' : 'not-advertised';",
        );
        let loaded = said.starts_with("loaded");
        let minor = php::minor_of(v);
        let recorded = php::pdo_pgsql_supported(&minor);
        println!("  php {v}: {said}  (recorded: {recorded})");
        // Disagreement in EITHER direction is the failure. A minor that quietly
        // gains the driver matters as much as one that loses it: the first means
        // rexenv is refusing sites it could allow, the second means it is
        // allowing sites that will stall.
        if loaded != recorded {
            eprintln!(
                "    ✗ php {v} disagrees with core::php::pdo_pgsql_supported(\"{minor}\") = {recorded}"
            );
            ok = false;
        }
    }

    println!("\n=== and what a real connection does (default PHP) ===");
    let (v, php_bin) = phps
        .iter()
        .find(|(v, _)| *v == binaries::PHP_VERSION)
        .expect("the default PHP is in PHP_VERSIONS");

    // ext/pgsql is the control: it proves the SERVER is reachable, so a PDO
    // failure below is about the driver rather than about the cluster.
    let (ext_ok, ext_said) = php_says(
        php_bin,
        &format!(
            "$c = @pg_connect('host=127.0.0.1 port={port} dbname={DB} user=postgres'); \
             echo $c ? pg_fetch_result(pg_query($c, 'SELECT 1'), 0, 0) : 'no-connection';"
        ),
    );
    println!("  php {v} · pg_connect → {ext_said}");
    ok &= ext_ok && ext_said == "1";

    let started = Instant::now();
    let (_, pdo_said) = php_says(
        php_bin,
        &format!(
            "try {{ new PDO('pgsql:host=127.0.0.1;port={port};dbname={DB}', 'postgres', ''); \
             echo 'connected'; }} catch (Throwable $e) {{ \
             echo 'refused: ', str_replace(\"\\n\", ' ', substr($e->getMessage(), 0, 90)); }}"
        ),
    );
    let waited = started.elapsed();
    println!("  php {v} · PDO pgsql → {pdo_said}  ({}s)", waited.as_secs());
    let connected = pdo_said.starts_with("connected");
    let recorded = php::pdo_pgsql_supported(&php::minor_of(v));
    if connected != recorded {
        eprintln!("    ✗ the connection disagrees with the record for this minor ({recorded})");
        ok = false;
    }
    // And the FAILURE SHAPE where there is one: a stall the server ends, not a
    // refusal the client raises. That is the half that makes a developer blame
    // PostgreSQL, and the reason the refusal at site-create names PHP instead.
    if !recorded {
        let stalled = waited >= Duration::from_secs(4) && pdo_said.contains("server closed");
        println!("  the failure is a stall the SERVER ends: {stalled}");
        ok &= stalled;
    }

    // The whole point, on a minor that HAS the driver: a Laravel-shaped session
    // — connect, DDL, write, read back — not just a successful handshake.
    if recorded {
        let (work_ok, said) = php_says(
            php_bin,
            &format!(
                "$p = new PDO('pgsql:host=127.0.0.1;port={port};dbname={DB}', 'postgres', '', \
                 [PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION]); \
                 $p->exec('CREATE TABLE IF NOT EXISTS probe (id serial PRIMARY KEY, note text)'); \
                 $s = $p->prepare('INSERT INTO probe (note) VALUES (?)'); \
                 $s->execute(['from pdo']); \
                 echo $p->query('SELECT note FROM probe ORDER BY id DESC LIMIT 1')->fetchColumn();"
            ),
        );
        println!("  php {v} · PDO create+insert+select → {said}");
        ok &= work_ok && said == "from pdo";
    }

    // What provisioning would write, whatever the runtime does — the values are
    // rexenv's own and are settled (ledger #546).
    let settings = laravel::DbSettings::for_engine(engine, DB.to_string());
    println!(
        "\n  .env would say DB_CONNECTION={} DB_PORT={} DB_USERNAME={}",
        settings.connection, settings.port, settings.username
    );
    ok &= settings.connection == "pgsql" && settings.port == port && settings.username == "postgres";

    let _ = engine.drop_database(&client, port, DB);
    server.reap();

    println!(
        "\n{}",
        if ok {
            if php::pdo_pgsql_supported(&php::minor_of(binaries::PHP_VERSION)) {
                "laravel on postgres: all green"
            } else {
                "laravel on postgres: the gap is still real — recorded, refused at create, unchanged"
            }
        } else {
            "laravel on postgres: FAILURES above"
        }
    );
    if !ok {
        std::process::exit(1);
    }
}
