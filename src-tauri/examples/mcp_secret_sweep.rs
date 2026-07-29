//! Live check: the MCP secret-leak sweep — EVERY registered tool's output, run
//! against a fixture with planted secrets, asserted to carry none of them.
//!
//!   cargo run --example mcp_secret_sweep
//!
//! This is the HARNESS, not one tool's test: `mcp_server::sweep_tool_outputs`
//! enumerates `tools::registry()`, so a new content tool is swept by
//! construction — it can't be registered without a `sweep_args`, and it can't be
//! registered without appearing here. The fixture plants a site whose docroot
//! path and db_name are distinctive markers that the `Agent*` conversions DROP;
//! the site still appears in `list_sites` output by its domain, so the sweep is
//! not vacuous — it proves the drop on live output, not just on the type.
//!
//! Sandbox-owned: a throwaway `Platform` + fixture DB (the sandbox root is
//! removed on drop). The `site_status` probe touches the real loopback :443
//! read-only; its verdict varies with the machine, but its output never carries
//! a planted secret, which is all the sweep asserts.

use rexenv_lib::state::app::AppState;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use rexenv_lib::{core, mcp_server};
use tauri::Manager;

mod common;

/// Distinctive markers with no overlap with the site's domain, planted into the
/// two `Site` fields a tool must never leak.
const DOCROOT_SECRET: &str = "SWEEPSECRETDOCROOTaa11";
const DBNAME_SECRET: &str = "SWEEPSECRETDBNAMEbb22";

#[tokio::main]
async fn main() {
    let (plat, _sandbox) = common::sandbox("mcp_secret_sweep");
    let conn = rexenv_lib::state::db::open_for_platform(plat.paths()).expect("open sandbox db");
    let ca = core::ssl::load_or_create(plat.paths(), plat.permissions()).expect("load sandbox CA");

    let site = core::sites::create(
        &conn,
        NewSite {
            name: "Sweep Fixture".into(),
            domain: "sweep-fixture.rex".into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: SiteDbEngine::Mysql,
        },
    )
    .expect("create fixture site");
    conn.execute(
        "UPDATE sites SET path = ?1, db_name = ?2 WHERE id = ?3",
        rusqlite::params![DOCROOT_SECRET, DBNAME_SECRET, site.id],
    )
    .expect("plant the docroot path + db_name secrets");

    let app = tauri::test::mock_app();
    app.manage(AppState::new(conn, plat, ca));

    let outputs = mcp_server::sweep_tool_outputs(app.handle(), &site.id).await;
    assert!(!outputs.is_empty(), "the sweep must exercise the registered tools");

    let planted = [DOCROOT_SECRET, DBNAME_SECRET, "rexenv-ca-key.pem"];
    let mut list_sites_saw_the_site = false;
    for (tool, out) in &outputs {
        for secret in planted {
            assert!(
                !out.contains(secret),
                "tool `{tool}` leaked `{secret}` into agent output:\n{out}"
            );
        }
        if *tool == "list_sites" && out.contains("sweep-fixture.rex") {
            list_sites_saw_the_site = true;
        }
        println!("  ✓ {tool}: no docroot path / db name / CA path in output");
    }
    assert!(
        list_sites_saw_the_site,
        "list_sites must have INCLUDED the planted site (its domain) — else the sweep proved nothing"
    );

    println!(
        "✓ mcp_secret_sweep green — {} registered tool(s) exercised against a planted fixture; \
         the site appears by domain but no docroot path, db name, or CA path reached the output.",
        outputs.len()
    );
}
