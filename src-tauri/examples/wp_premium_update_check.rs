//! Live check: a PREMIUM plugin's update reaches the list (`core::wordpress`'s
//! premium-update context, #370).
//!
//! # The bug, and why a unit test cannot see it
//!
//! wp-admin listed BetterDocs Pro 3.9.0 → 4.1.0; rexenv's WordPress tab showed
//! no badge at all. Not a parsing bug: the vendors' updaters never REGISTER in
//! a wp-cli run. Every premium updater measured on a real site adds its
//! `pre_set_site_transient_update_plugins` filter behind a gate, and the gate is
//! a CAPABILITY check —
//!
//!     if ( ! current_user_can( 'manage_options' ) && ! $doing_cron ) { return; }
//!
//! — which a wp-cli run, having no user at all, never satisfies. WordPress then
//! builds its update data with every premium plugin missing, and rexenv reports
//! exactly what WordPress knows.
//!
//! The fix hands the CHECK (and the update itself) a `--require` file that grants
//! three capabilities to the process and defines `WP_ADMIN`. Nothing about that
//! is visible to a lib test: it is WordPress's own update pipeline, running
//! under a real wp-cli, deciding whether a filter registered.
//!
//! # What is planted
//!
//! Two fixture plugins on disk and one mu-plugin that injects an update for
//! each — the second one **behind the measured vendor gate**. That pairing is
//! the whole design: the ungated fixture proves the harness itself works, so a
//! failure on the gated one can only mean the capability grant did not arrive.
//!
//! Then the same question is asked twice:
//!   - `wp_run_raw` — the MCP raw runner, which by structural guarantee carries
//!     NO context (`the_premium_update_context_rides_only_the_update_paths`):
//!     the ungated update shows, the gated one does not. That is the old bug,
//!     reproduced on demand.
//!   - `plugin_list(check_updates: true)` — the app's own checked pass: BOTH
//!     show, with the version the vendor claims.
//!
//! Each measurement deletes the update transient first. Without that, WordPress
//! answers from the last check (12h timer) and the second reading would prove
//! only that the first one was cached.
//!
//! **Not covered here:** that a premium update INSTALLS. That needs a vendor's
//! real package URL and a real licence, so it is `docs/SMOKE-TEST.md`'s. What
//! this check does cover is the reason the install path was changed at all: the
//! package URL lives in the same filter's output, measured against a real
//! licensed site with `wp plugin update <paid-slug> --dry-run` — "No plugin
//! updates available" without the context, the real 3.9.0 → 4.1.0 row with it.
//!
//! Run (MySQL :13306 free, needs the internet): `cargo run --example wp_premium_update_check`

use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::path::Path;
use std::time::Duration;

mod common;

/// The version both fixtures claim to have available.
const OFFERED: &str = "9.9.9";

#[tokio::main]
async fn main() {
    // FIRST statement: this example starts a real mysqld on the production
    // port (the `wp_plugins_check` rule — a borrowed corpse fails the NEXT
    // example with an error naming nothing).
    common::require_ports_free(&[(database::MYSQL_PORT, "MySQL")]);

    let plat = platform::current();
    let domain = "wppremium.test";

    let (conn, _dbf) = common::fixture_db("wp_premium_update_check");
    // Fixture-owned docroot. `sites::provision` reads the `sites_dir` SETTING,
    // which falls back to a path derived from $HOME — so without this the site
    // lands in the user's real ~/rexenv/Sites and SURVIVES into the next run.
    // That is not untidy, it is the bug: two checks failed on their own
    // leftovers on 21 Aug 2026 (`wp_tools_check`, `wp_themes_check`).
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "wppremium");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");

    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    database::initialize(&*plat, &mysql_base, &datadir).unwrap();
    let mysqld =
        database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap();
    let mut mysqld = common::OwnedService::new(mysqld, "mysqld");
    for _ in 0..30 {
        if database::mysql_running(database::MYSQL_PORT) {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "WP Premium".into(),
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
    let docroot = std::path::PathBuf::from(&site.path);
    common::install_wp(&php, &wp, &docroot, domain, "WP Premium", &db_client);

    // ── Plant: two plugins, one mu-plugin, one of the two updates gated ──────
    // Everything written here is inside the docroot THIS example provisioned.
    plant_plugin(&docroot, "rexenv-free-fixture", "rexenv Free Fixture");
    plant_plugin(&docroot, "rexenv-paid-fixture", "rexenv Paid Fixture");
    plant_updater(&docroot);
    println!("planted: two 1.0.0 fixture plugins + an updater offering {OFFERED}");
    println!("         (the paid one behind `current_user_can( 'manage_options' )`)");

    // ── Leg 1: no context — the bug, reproduced ─────────────────────────────
    forget_the_update_check(&php, &wp, &docroot);
    let raw = raw_update_versions(&php, &wp, &docroot);
    let free_raw = raw.get("rexenv-free-fixture").cloned().unwrap_or_default();
    let paid_raw = raw.get("rexenv-paid-fixture").cloned().unwrap_or_default();
    assert_eq!(
        free_raw, OFFERED,
        "the ungated fixture did not offer an update through the raw runner — the plant \
         itself is broken, so leg 2 would prove nothing"
    );
    assert_eq!(
        paid_raw, "",
        "the GATED fixture offered an update with no capabilities granted — either wp-cli \
         now runs as a user, or the raw runner picked up the context it must never carry"
    );
    println!("✓ leg 1 (wp_run_raw, no context): free={free_raw:?}  paid={paid_raw:?} ← the bug");

    // ── Leg 2: the app's checked pass ───────────────────────────────────────
    forget_the_update_check(&php, &wp, &docroot);
    let list = wordpress::plugin_list(&php, &wp, &docroot, true).expect("checked plugin list");
    let row = |name: &str| {
        list.iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("{name} missing from the list entirely"))
    };
    let free = row("rexenv-free-fixture");
    let paid = row("rexenv-paid-fixture");
    assert_eq!(free.update, "available", "the ungated fixture lost its badge in the checked pass");
    assert_eq!(free.update_version, OFFERED);
    assert_eq!(
        paid.update, "available",
        "THE BUG IS BACK: the premium fixture's updater did not register, so its update is \
         invisible exactly as BetterDocs Pro's was"
    );
    assert_eq!(
        paid.update_version, OFFERED,
        "the badge appeared without the version the vendor offers — the arrow would be blank"
    );
    println!(
        "✓ leg 2 (plugin_list checked): free={} → {}  paid={} → {}",
        free.version, free.update_version, paid.version, paid.update_version
    );

    // ── Leg 3: the grant is scoped to the process, not written anywhere ──────
    // A capability grant that outlived the command would be a real defect, so
    // this asks the site itself, through a runner that carries no context.
    let out = wordpress::wp_run_raw(
        &php,
        &wp,
        &docroot,
        &["eval".into(), "echo current_user_can( 'manage_options' ) ? 'YES' : 'NO';".into()],
        Duration::from_secs(60),
    )
    .expect("eval");
    let answer = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(
        answer, "NO",
        "a later wp-cli run still has the capability — the grant escaped its process"
    );
    println!("✓ leg 3: a later run has no capabilities of its own (current_user_can → NO)");

    mysqld.stop();
    println!("\nALL GOOD — a premium plugin's update reaches the list, and only the paths that ask for it run with the grant.");
}

/// A minimal but REAL plugin: WordPress lists what has a plugin header, and an
/// update row exists only for something installed.
fn plant_plugin(docroot: &Path, slug: &str, title: &str) {
    let dir = docroot.join("wp-content").join("plugins").join(slug);
    std::fs::create_dir_all(&dir).expect("fixture plugin dir");
    std::fs::write(
        dir.join(format!("{slug}.php")),
        format!("<?php\n/**\n * Plugin Name: {title}\n * Version: 1.0.0\n */\n"),
    )
    .expect("fixture plugin file");
}

/// The vendor shape, planted: one update offered unconditionally, one behind the
/// capability gate measured on a real site.
fn plant_updater(docroot: &Path) {
    let dir = docroot.join("wp-content").join("mu-plugins");
    std::fs::create_dir_all(&dir).expect("mu-plugins dir");
    std::fs::write(
        dir.join("rexenv-premium-fixture.php"),
        format!(
            r#"<?php
// rexenv live-check fixture (examples/wp_premium_update_check.rs). Not shipped.
add_filter( 'pre_set_site_transient_update_plugins', function ( $t ) {{
	if ( ! is_object( $t ) ) {{ $t = new stdClass(); }}
	if ( ! isset( $t->response ) || ! is_array( $t->response ) ) {{ $t->response = array(); }}

	$offer = function ( $slug ) {{
		return (object) array(
			'slug'        => $slug,
			'plugin'      => $slug . '/' . $slug . '.php',
			'new_version' => '{OFFERED}',
			'package'     => 'https://example.invalid/' . $slug . '.zip',
			'url'         => 'https://example.invalid/' . $slug,
		);
	}};

	// Always — the control.
	$t->response['rexenv-free-fixture/rexenv-free-fixture.php'] = $offer( 'rexenv-free-fixture' );

	// The measured premium gate, copied shape for shape.
	if ( current_user_can( 'manage_options' ) ) {{
		$t->response['rexenv-paid-fixture/rexenv-paid-fixture.php'] = $offer( 'rexenv-paid-fixture' );
	}}

	return $t;
}}, 99 );
"#
        ),
    )
    .expect("fixture mu-plugin");
}

/// Drop what WordPress remembers about the last update check.
///
/// Without this every reading after the first would answer from the stored
/// transient (12h timer), and the two legs would differ by their ORDER rather
/// than by the capability grant — a green run proving nothing.
fn forget_the_update_check(php: &Path, wp: &Path, docroot: &Path) {
    for key in ["_site_transient_update_plugins", "_site_transient_timeout_update_plugins"] {
        let _ = wordpress::wp_run_raw(
            php,
            wp,
            docroot,
            &["option".into(), "delete".into(), key.into()],
            Duration::from_secs(60),
        );
    }
}

/// `slug → update_version` through the RAW runner, which carries no context.
fn raw_update_versions(
    php: &Path,
    wp: &Path,
    docroot: &Path,
) -> std::collections::HashMap<String, String> {
    let out = wordpress::wp_run_raw(
        php,
        wp,
        docroot,
        &[
            "plugin".into(),
            "list".into(),
            "--fields=name,update_version".into(),
            "--format=json".into(),
        ],
        Duration::from_secs(300),
    )
    .expect("raw plugin list");
    assert!(
        out.status.success(),
        "raw plugin list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let start = stdout.find('[').expect("json array in raw list output");
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&stdout[start..]).expect("parse raw plugin list");
    rows.iter()
        .filter_map(|r| {
            let name = r.get("name")?.as_str()?.to_string();
            let v = r.get("update_version").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some((name, v))
        })
        .collect()
}
