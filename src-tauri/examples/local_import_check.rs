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
//!   C. **A multisite NETWORK moves, then connects (#573, #581).** A real
//!      subdomain network (`multi.local` + `ea1.multi.local`, every stored URL on
//!      `http://`, a serialized subsite option, wp-config in Local's shape) is
//!      re-homed with `network = true`: `wp_blogs`, `wp_site`, `sitemeta` and both
//!      blogs' options must read `https://` on `multi.rex`. A control shows why the
//!      proof names the new host with `--url`. Then the connect plan (dedicated user,
//!      renamed database, network domain) is applied to the fixture wp-config and
//!      plain WordPress must boot the SUBSITE on its rexenv name with the login
//!      cookie scoped to `.multi.rex`.
//!
//! NOT covered, by construction: Local's real mysqld (`skip-name-resolve`, the
//! socket where Local puts it) — `docs/PUBLISH-TESTING.md` §N, owner-run.

use rexenv_lib::core::confedit::{self, RewritePlan};
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
    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::pins().mysql)
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
        .sql_client_bins(&*plat, binaries::pins().mysql)
        .await
        .expect("bundled MySQL client");

    // ── A. sign-in over the socket (#572) ─────────────────────────────────────
    DbEngine::Mysql.create_database(&client, PORT, "local").expect("create `local`");
    let verdict = compat(
        &Source { vendor: Some(Vendor::Mysql), version: Version::parse(binaries::pins().mysql) },
        &Target { vendor: Vendor::Mysql, version: Version::parse(binaries::pins().mysql).unwrap() },
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
    let id = dbsource::probe_socket(&*plat, &socket);
    check.is(
        "the pre-auth probe identifies the server over its socket (Local refuses TCP before greeting)",
        matches!(&id, Probe::Listening(Identity::Handshake { vendor: Vendor::Mysql, .. })),
        &format!("{id:?}"),
    );

    // ── B. the URL pass on a copy whose wp-config signs in nowhere (#573) ─────
    let php8 = binaries::resolve(&*plat, "php", binaries::pins().php).await.expect("php");
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli).await.expect("wp-cli");
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

    // ── C. a subdomain NETWORK: the URL pass, then the connect plan ───────────
    const NET_DB: &str = "local_net";
    let q = |s: &str| sql(client.path(), s).unwrap_or_else(|e| format!("<error: {e}>"));
    let net_root = sandbox.root().join("Local Sites/multi/app/public");
    std::fs::create_dir_all(&net_root).unwrap();
    wordpress::install_for_site(
        &php8,
        &wp,
        &net_root,
        "multi.local",
        "Multi",
        NET_DB,
        &format!("127.0.0.1:{PORT}"),
        &client,
        &Default::default(),
    )
    .expect("install the network's WordPress");
    wordpress::multisite_convert(&php8, &wp, &net_root, true).expect("convert to a subdomain network");
    let made = wordpress::wp_run(&php8, &wp, &net_root, &["site", "create", "--slug=ea1", "--title=EA1"]);
    check.is("fixture: the network gained a subsite", made.is_ok(), &format!("{made:?}"));

    // Local serves http:// — every stored URL says so, and one subsite option is serialized.
    let sub_old = "http://ea1.multi.local";
    let ser_old = format!("a:1:{{s:3:\"url\";s:{}:\"{sub_old}\";}}", sub_old.len());
    sql(
        client.path(),
        &format!(
            "UPDATE {NET_DB}.wp_options SET option_value='http://multi.local' WHERE option_name IN ('siteurl','home'); \
             UPDATE {NET_DB}.wp_2_options SET option_value='{sub_old}' WHERE option_name IN ('siteurl','home'); \
             UPDATE {NET_DB}.wp_sitemeta SET meta_value='http://multi.local/' WHERE meta_key='siteurl'; \
             INSERT INTO {NET_DB}.wp_2_options (option_name, option_value, autoload) VALUES ('rex_probe', '{}', 'off');",
            ser_old.replace('\'', "''")
        ),
    )
    .expect("make the network look like Local's");
    let blogs = format!("SELECT GROUP_CONCAT(domain ORDER BY blog_id) FROM {NET_DB}.wp_blogs");
    check.is("fixture: the blogs are multi.local and ea1.multi.local", q(&blogs) == "multi.local,ea1.multi.local", &q(&blogs));

    let net_config = net_root.join("wp-config.php");
    let net_local = std::fs::read_to_string(&net_config)
        .unwrap()
        .replace(&format!("'127.0.0.1:{PORT}'"), "'localhost'")
        .replace("'DB_PASSWORD', ''", "'DB_PASSWORD', 'root'")
        .replace(&format!("'{NET_DB}'"), "'local'");
    std::fs::write(&net_config, &net_local).unwrap();
    let dcs = rexenv_lib::core::phpconf::wp_define_str(&net_local, "DOMAIN_CURRENT_SITE");
    check.is(
        "fixture: the network's wp-config is Local-shaped with DOMAIN_CURRENT_SITE = multi.local",
        net_local.contains("'localhost'") && net_local.contains("'DB_PASSWORD', 'root'") && dcs.as_deref() == Ok("multi.local"),
        &format!("{dcs:?}"),
    );

    let net_scratch = sandbox.root().join("db-imports-net");
    let moved = wordpress::rehome_urls_on_copy(
        &*plat, &php8, &wp, &net_root, &net_scratch, PORT, NET_DB, "multi.local", "multi.rex", true,
    );
    check.is("network: the URL pass reaches and moves the copy", matches!(moved, Ok(n) if n > 0), &format!("{moved:?}"));
    let sub_new = "https://ea1.multi.rex";
    for (label, query, want) in [
        ("wp_blogs", blogs.clone(), "multi.rex,ea1.multi.rex".to_string()),
        ("wp_site", format!("SELECT domain FROM {NET_DB}.wp_site"), "multi.rex".to_string()),
        ("sitemeta siteurl", format!("SELECT meta_value FROM {NET_DB}.wp_sitemeta WHERE meta_key='siteurl'"), "https://multi.rex/".to_string()),
        ("the network's siteurl", format!("SELECT option_value FROM {NET_DB}.wp_options WHERE option_name='siteurl'"), "https://multi.rex".to_string()),
        ("the subsite's siteurl", format!("SELECT option_value FROM {NET_DB}.wp_2_options WHERE option_name='siteurl'"), sub_new.to_string()),
        ("the subsite's home", format!("SELECT option_value FROM {NET_DB}.wp_2_options WHERE option_name='home'"), sub_new.to_string()),
        (
            "the subsite's serialized option (length repaired)",
            format!("SELECT option_value FROM {NET_DB}.wp_2_options WHERE option_name='rex_probe'"),
            format!("a:1:{{s:3:\"url\";s:{}:\"{sub_new}\";}}", sub_new.len()),
        ),
    ] {
        let got = q(&query);
        check.is(&format!("network: {label} moved"), got == want, &got);
    }
    check.is("network: the override file is gone", !net_scratch.join(".copy-db-override.php").exists(), "left behind");
    check.is(
        "network: wp-config is byte-identical after the pass",
        std::fs::read_to_string(&net_config).unwrap() == net_local,
        "the URL pass wrote into the project",
    );

    // Control: the same override on the moved copy WITHOUT `--url` — wp-cli boots
    // the network from wp-config's DOMAIN_CURRENT_SITE (still multi.local) and
    // finds no such site, which is why the pass's proof names the new host.
    let bare = net_scratch.join("control-override.php");
    std::fs::write(&bare, wordpress::copy_db_override(PORT, NET_DB)).unwrap();
    let require = format!("--require={}", bare.display());
    let path = format!("--path={}", net_root.display());
    let unpinned = wordpress::wp_cli_checked(
        &php8,
        &wp,
        &["option", "get", "siteurl", "--skip-plugins", "--skip-themes", &require, &path],
        None,
    );
    check.is(
        "control: without --url, the moved network does not boot from wp-config's old DOMAIN_CURRENT_SITE",
        !matches!(&unpinned, Ok(s) if s.trim().ends_with("https://multi.rex")),
        &format!("{unpinned:?}"),
    );
    let _ = std::fs::remove_file(&bare);

    // The connect (#581): dedicated account holding the config's password (D1),
    // the renamed database (#574) and the network's domain, on the fixture file.
    sql(
        client.path(),
        &format!(
            "CREATE USER IF NOT EXISTS 'rex_multi'@'%' IDENTIFIED BY 'root'; \
             GRANT ALL PRIVILEGES ON {NET_DB}.* TO 'rex_multi'@'%';"
        ),
    )
    .expect("the dedicated account");
    let plan = RewritePlan::wp(&format!("127.0.0.1:{PORT}"), Some("rex_multi"))
        .and_then(|p| p.with_database(NET_DB))
        .and_then(|p| p.with_network_domain("multi.rex"))
        .expect("the connect plan");
    match confedit::rewrite(&net_local, &plan) {
        Err(refusal) => check.is("connect: the network's wp-config can be rewritten", false, &format!("{refusal:?}")),
        Ok(r) => {
            check.is(
                "connect: the diff moves DOMAIN_CURRENT_SITE to multi.rex",
                r.diff.iter().any(|d| d.sign == '+' && d.text.contains("DOMAIN_CURRENT_SITE") && d.text.contains("'multi.rex'")),
                &format!("{:?}", r.diff),
            );
            std::fs::write(&net_config, &r.new_content).unwrap();
            let booted = wordpress::wp_run(&php8, &wp, &net_root, &["option", "get", "siteurl", "--url=https://ea1.multi.rex/"]);
            check.is(
                "connect: plain WordPress boots the network's SUBSITE on its rexenv name",
                matches!(&booted, Ok(s) if s.trim() == sub_new),
                &format!("{booted:?}"),
            );
            let cookie = wordpress::wp_run(&php8, &wp, &net_root, &["eval", "echo COOKIE_DOMAIN;", "--url=https://multi.rex/"]);
            check.is(
                "connect: the login cookie is scoped to .multi.rex, not .multi.local",
                matches!(&cookie, Ok(s) if s.trim() == ".multi.rex"),
                &format!("{cookie:?}"),
            );
        }
    }

    check.verdict()
}
