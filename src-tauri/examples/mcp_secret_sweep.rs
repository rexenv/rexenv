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
const DOCROOT_SECRET: &str = "SWEEPSECRETDOCROOTaa11"; // a distinctive path component
const DBNAME_SECRET: &str = "SWEEPSECRETDBNAMEbb22";
const LOGIN_TOKEN: &str = "TOKENSECRETee55ff66"; // a rexenv-issued login token, in a log
const COOKIE_SECRET: &str = "COOKIESECRETcc33"; // a Set-Cookie value, in a log
const BENIGN_MARKER: &str = "BENIGNMARKERdd44"; // benign log content that MUST survive

#[tokio::main]
async fn main() {
    let (plat, _sandbox) = common::sandbox("mcp_secret_sweep");
    let conn = rexenv_lib::state::db::open_for_platform(plat.paths()).expect("open sandbox db");
    let ca = core::ssl::load_or_create(plat.paths(), plat.permissions()).expect("load sandbox CA");

    // A real docroot (so tail_log can read a debug log) whose path carries the
    // distinctive marker, with a debug log holding a real login token, a
    // Set-Cookie, a REALISTIC absolute-path stack trace (the shape a WP fatal
    // actually logs — this is where the docroot/username leaked), and a benign
    // line that MUST come through.
    let docroot = plat.paths().app_data_dir().expect("data dir").join(DOCROOT_SECRET);
    std::fs::create_dir_all(docroot.join("wp-content")).expect("make fixture docroot");
    std::fs::write(
        docroot.join("wp-content/debug.log"),
        format!(
            "[29-Jul-2026] PHP Warning: {BENIGN_MARKER} in plugin.php on line 5\n\
             GET /wp-login.php?rexenv_login={LOGIN_TOKEN}&redir=1 HTTP/1.1\n\
             Set-Cookie: wordpress_logged_in={COOKIE_SECRET}; Path=/; HttpOnly\n\
             [29-Jul-2026] PHP Fatal error: boom in {}/wp-content/plugins/x.php on line 9\n",
            docroot.display()
        ),
    )
    .expect("write fixture debug log");

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
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
        },
    )
    .expect("create fixture site");
    conn.execute(
        "UPDATE sites SET path = ?1, db_name = ?2 WHERE id = ?3",
        rusqlite::params![docroot.to_string_lossy(), DBNAME_SECRET, site.id],
    )
    .expect("plant the docroot path + db_name secrets");

    let app = tauri::test::mock_app();
    app.manage(AppState::new(conn, plat, ca));

    let outputs = mcp_server::sweep_tool_outputs(app.handle(), &site.id).await;
    assert!(!outputs.is_empty(), "the sweep must exercise the registered tools");

    // EVERY tool's output, checked for EVERY planted secret. tail_log read the
    // token + cookie from the log; the scrubber must have removed them here.
    let planted = [DOCROOT_SECRET, DBNAME_SECRET, LOGIN_TOKEN, COOKIE_SECRET, "rexenv-ca-key.pem"];
    let mut list_sites_saw_the_site = false;
    let mut tail_log_kept_benign = false;
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
        // Non-vacuous for tail_log: it must have READ the log (benign line
        // present) — a scrubber that just emptied the log would pass the
        // absence checks while proving nothing.
        if *tool == "tail_log" && out.contains(BENIGN_MARKER) {
            tail_log_kept_benign = true;
        }
        println!("  ✓ {tool}: no docroot / db-name / login-token / cookie / CA path in output");
    }
    assert!(
        list_sites_saw_the_site,
        "list_sites must have INCLUDED the planted site (its domain) — else the sweep proved nothing"
    );
    assert!(
        tail_log_kept_benign,
        "tail_log must have RETURNED the log's benign content — else the scrubber's clean output is vacuous"
    );

    // The ERROR path (assembly-review coverage gap #5): drive the tools with a
    // bogus site_id so site_status/tail_log take their Err branch, and assert no
    // secret reaches the agent-facing error text either. Enumerated, so a future
    // tool's error path is swept too.
    let errs = mcp_server::sweep_tool_outputs(app.handle(), "no-such-site-id-zzzz").await;
    let mut saw_an_error = false;
    for (tool, out) in &errs {
        for secret in planted {
            assert!(!out.contains(secret), "tool `{tool}` leaked `{secret}` in its ERROR text:\n{out}");
        }
        if *tool != "list_sites" {
            saw_an_error = true; // site_status/tail_log error on a bogus id
        }
    }
    assert!(saw_an_error, "the error-path pass must actually take an Err branch");
    println!("  ✓ error path: tools erroring on a bogus site_id leak no secret");

    println!(
        "✓ mcp_secret_sweep green — {} tool(s) exercised on a planted fixture (Ok AND error paths): \
         the site appears by domain, the log's benign line + absolute stack-trace come through with \
         the docroot stripped, but no docroot, db name, login token, cookie, or CA path reached the \
         output.",
        outputs.len()
    );
}
