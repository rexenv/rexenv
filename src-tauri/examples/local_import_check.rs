//! Live check for the two halves of a Local import that rexenv can prove on its
//! own (`docs/archive/PLAN-local-import.md`). Run: `cargo run --example local_import_check`
//!
//! Network tier: it installs a real WordPress (`wp core download`). Everything
//! it creates lives in the sandbox — its own mysqld on a fixture port plays
//! Local's per-site server; nothing touches Local, the user's stack or app data.
//!
//!   A. **Sign-in over a SOCKET (#572).** The defaults file names a socket (in a
//!      folder with a space, as Local's is) and a TCP port where NOTHING listens.
//!      A `Ready` preflight therefore proves the socket carried the login; the
//!      TCP defaults file against the same dead port is the negative control.
//!   B. **The URL pass writes rexenv's copy through the override (#573).** A real
//!      WordPress is made to look like a copied Local site — `siteurl`/`home` on
//!      `http://ea.local`, a SERIALIZED option holding that URL, and a wp-config
//!      naming `localhost` / `root` / `root` / `local`, which signs in nowhere
//!      here (asserted: plain wp-cli fails). The pass must still reach the copy,
//!      move every URL to `https://ea.rex` with the serialized length repaired,
//!      leave wp-config byte-identical, and delete its override file. Then again
//!      on PHP 7.4, where a constant redefinition is a Notice rather than a Warning.
//!
//! NOT covered, by construction: Local's real mysqld (`skip-name-resolve`, the
//! socket where Local puts it) — `docs/PUBLISH-TESTING.md` §N, owner-run.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::dbcompat::{compat, Source, Target, Version};
use rexenv_lib::core::dbdump::{self, LiveCheck};
use rexenv_lib::core::dbimport::{ConfigSource, DbConnection, Driver};
use rexenv_lib::core::dbsource::{self, Identity, Probe, Vendor};
use rexenv_lib::core::{binaries, database, wordpress};
use std::path::Path;
use std::time::Duration;

mod common;
use common::Reaped;

const PORT: u16 = 13396;
/// Nothing listens here — the TCP half of the socket leg's defaults must be dead.
const DEAD_PORT: u16 = 1;
const COPY_DB: &str = "local_copy";

fn sql(client: &Path, q: &str) -> Result<String, String> {
    let out = std::process::Command::new(client)
        .args(["--no-defaults", "--protocol=TCP", "--host=127.0.0.1"])
        .arg(format!("--port={PORT}"))
        .args(["--user=root", "-N", "-B", "-e", q])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn option(client: &Path, name: &str) -> String {
    sql(client, &format!("SELECT option_value FROM {COPY_DB}.wp_options WHERE option_name='{name}'"))
        .unwrap_or_else(|e| format!("<error: {e}>"))
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let (plat, sandbox) = common::sandbox("local_import_check");
    let mut check = common::Check::new("local_import_check");

    // ── the stand-in for Local's per-site mysqld ─────────────────────────────
    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION)
        .await
        .expect("mysql tree");
    let datadir = sandbox.root().join("mysql-data");
    database::initialize(&*plat, &basedir, &datadir).expect("init datadir");
    let sock_dir = sandbox.root().join("App Support");
    std::fs::create_dir_all(&sock_dir).unwrap();
    let socket = sock_dir.join("mysqld.sock");
    let _mysqld = Reaped::new(
        database::start(&*plat, &basedir, &datadir, PORT, &socket).expect("start mysqld"),
        PORT,
        "mysqld",
    );
    for _ in 0..150 {
        if database::mysql_running(PORT) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if !database::mysql_running(PORT) {
        check.is("the sandbox mysqld came up", false, "not listening");
        return check.verdict();
    }
    let (client, _) = DbEngine::Mysql
        .sql_client_bins(&*plat, binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");

    // ── A. sign-in over the socket (#572) ─────────────────────────────────────
    DbEngine::Mysql.create_database(&client, PORT, "local").expect("create `local`");
    let verdict = compat(
        &Source { vendor: Some(Vendor::Mysql), version: Version::parse(binaries::MYSQL_VERSION) },
        &Target { vendor: Vendor::Mysql, version: Version::parse(binaries::MYSQL_VERSION).unwrap() },
    );
    let cleared = dbdump::gate(None, &verdict, false).expect("gate clears");
    let conn = DbConnection {
        driver: Driver::MysqlFamily,
        host: "127.0.0.1".into(),
        port: DEAD_PORT,
        database: "local".into(),
        user: "root".into(),
        password: String::new(),
        table_prefix: Some("wp_".into()),
        source: ConfigSource::WpConfig { path: "/fixture/wp-config.php".into() },
    };
    let via_socket = dbdump::DefaultsFile::create_via_socket(&*plat, &sandbox.root().join("cnf-socket"), &conn, &socket)
        .expect("socket defaults file");
    let got = dbdump::preflight_live(&cleared, &client, &via_socket, "local");
    check.is(
        "a socket defaults file signs in although its TCP port is dead (#572)",
        matches!(got, Ok(LiveCheck::Ready { .. })),
        &format!("{got:?}"),
    );
    let via_tcp = dbdump::DefaultsFile::create(&*plat, &sandbox.root().join("cnf-tcp"), &conn).expect("tcp defaults file");
    let control = dbdump::preflight_live(&cleared, &client, &via_tcp, "local");
    check.is(
        "control: the TCP defaults file on the same dead port does NOT sign in",
        !matches!(control, Ok(LiveCheck::Ready { .. })),
        &format!("{control:?}"),
    );
    let id = dbsource::probe_socket(&socket);
    check.is(
        "the pre-auth probe identifies the server over its socket (Local refuses TCP before greeting)",
        matches!(&id, Probe::Listening(Identity::Handshake { vendor: Vendor::Mysql, .. })),
        &format!("{id:?}"),
    );

    // ── B. the URL pass on a copy whose wp-config signs in nowhere (#573) ─────
    let php8 = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.expect("php");
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.expect("wp-cli");
    let docroot = sandbox.root().join("Local Sites/ea/app/public");
    std::fs::create_dir_all(&docroot).unwrap();
    wordpress::install_for_site(
        &php8,
        &wp,
        &docroot,
        "ea.local",
        "EA",
        COPY_DB,
        &format!("127.0.0.1:{PORT}"),
        &client,
        &Default::default(),
    )
    .expect("install WordPress");

    let old = "http://ea.local";
    let serialized_old = format!("a:1:{{s:3:\"url\";s:{}:\"{old}\";}}", old.len());
    sql(
        client.path(),
        &format!(
            "UPDATE {COPY_DB}.wp_options SET option_value='{old}' WHERE option_name IN ('siteurl','home'); \
             INSERT INTO {COPY_DB}.wp_options (option_name, option_value, autoload) VALUES ('rex_probe', '{}', 'off');",
            serialized_old.replace('\'', "''")
        ),
    )
    .expect("make the copy look like Local's");

    // wp-config as Local writes it: the socket host, root/root, database `local`.
    let config = docroot.join("wp-config.php");
    let text = std::fs::read_to_string(&config).unwrap();
    let local_text = text
        .replace(&format!("'127.0.0.1:{PORT}'"), "'localhost'")
        .replace("'DB_PASSWORD', ''", "'DB_PASSWORD', 'root'")
        .replace(&format!("'{COPY_DB}'"), "'local'");
    std::fs::write(&config, &local_text).unwrap();
    check.is(
        "the fixture wp-config now names Local's shape (localhost, root/root, `local`)",
        local_text.contains("'localhost'") && local_text.contains("'DB_PASSWORD', 'root'") && !local_text.contains(COPY_DB),
        "wp config create's spelling changed — re-read the replacements",
    );
    let plain = wordpress::wp_run(&php8, &wp, &docroot, &["option", "get", "siteurl"]);
    check.is(
        "control: plain wp-cli with that wp-config reaches NO database",
        plain.is_err(),
        &format!("{plain:?}"),
    );

    let scratch = sandbox.root().join("db-imports");
    for (label, php, from, to) in [
        ("PHP 8.x", php8.clone(), "ea.local", "ea.rex"),
        ("PHP 7.4", binaries::resolve(&*plat, "php", "7.4.33").await.expect("php 7.4"), "ea.rex", "ea2.rex"),
    ] {
        let replaced =
            wordpress::rehome_urls_on_copy(&*plat, &php, &wp, &docroot, &scratch, PORT, COPY_DB, from, to, false);
        check.is(
            &format!("{label}: the URL pass reaches the copy through the override"),
            matches!(replaced, Ok(n) if n > 0),
            &format!("{replaced:?}"),
        );
        let want = format!("https://{to}");
        check.is(&format!("{label}: siteurl moved"), option(client.path(), "siteurl") == want, &option(client.path(), "siteurl"));
        check.is(&format!("{label}: home moved"), option(client.path(), "home") == want, &option(client.path(), "home"));
        let probe = option(client.path(), "rex_probe");
        check.is(
            &format!("{label}: the serialized option moved with its length repaired"),
            probe == format!("a:1:{{s:3:\"url\";s:{}:\"{want}\";}}", want.len()),
            &probe,
        );
        check.is(
            &format!("{label}: the override file is gone afterwards"),
            !scratch.join(".copy-db-override.php").exists(),
            "left behind",
        );
    }
    check.is(
        "the project's wp-config.php is byte-identical after both passes",
        std::fs::read_to_string(&config).unwrap() == local_text,
        "the URL pass wrote into the project",
    );

    check.verdict()
}
