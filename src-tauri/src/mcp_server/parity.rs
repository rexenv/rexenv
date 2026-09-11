//! MCP parity, kept true by a test instead of by a table in a plan.
//!
//! Parity (`docs/archive/PLAN-mcp-parity.md`, 3 Sep 2026) set out to make every
//! app function drivable by an agent, and its coverage matrix listed all 231
//! commands of the day. Nothing then held the matrix to the tree: by 11 Sep 2026
//! fifteen commands had been added — app self-update, per-site start/stop, the
//! mail catch-all switch, per-TLD resolver status, an external terminal — and
//! nobody had asked of any of them whether an agent could reach it. Asked by
//! hand that day, the answer was "most, not all", with thirty gaps no document
//! named.
//!
//! So the question is now asked of every registered command, every build:
//!
//!   * a command the MCP source reaches (`commands::<module>::<name>`, as a call
//!     or as a function value) needs NO row — that is derived, not typed;
//!   * every other command needs a row here, and the row is one of three
//!     rulings: which tool reaches the same core function, why it must NEVER be
//!     a tool, or that it is a known GAP;
//!   * a row for a command that no longer exists, or that the MCP source now
//!     reaches directly, fails — a stale ruling reads as a decision someone
//!     still stands behind.
//!
//! A new command therefore cannot ship without someone writing down what an
//! agent may do with it. "Gap" is an honest answer; silence is not.
//!
//! The whole file is a `#[cfg(test)]` module, not `#![cfg(test)]`: the tree's
//! source guards strip test code by that attribute (`copy_scan::production_lines`),
//! and this table names commands — `app_update`'s apply among them — that those
//! guards forbid production MCP code to reach.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// What an agent can do with an app command the MCP source does not call.
    #[derive(Clone, Copy, Debug)]
    enum Disposition {
        /// Reachable: the named tool reads or does the same thing through core.
        Tool(&'static str),
        /// Never a tool, and why — the reason is the decision.
        Never(&'static str),
        /// Should be reachable and is not yet. `docs/TODO.md` carries the list.
        Gap(&'static str),
    }
    use Disposition::*;

    const UPDATE_FLOW: &str = "the app-update flow is the person's: installing replaces the process that \
                               enforces the agent-access dial (ledger #530); stack_status shows the offer";
    const JOBS: &str = "job_status/job_cancel are NOT built, as a decision (parity L9): the creating tool \
                        blocks with progress and returns the outcome, and a half-built site is site_delete";
    const PTY: &str = "a PTY is a shell (parity §4.4)";
    const AMPLIFIER: &str = "CA trust, install and uninstall are the amplifiers (parity §2.2)";

    /// Every registered command the MCP source does not reach, with its ruling.
    /// Grouped by `commands::` module.
    const DISPOSITIONS: &[(&str, Disposition)] = &[
        // ── app_update
        ("app_update_state", Tool("stack_status")),
        ("app_update_check", Never(UPDATE_FLOW)),
        ("app_update_readiness", Never(UPDATE_FLOW)),
        ("app_update_set_auto_check", Never(UPDATE_FLOW)),
        ("app_update_skip", Never(UPDATE_FLOW)),
        ("app_update_apply", Never(UPDATE_FLOW)),
        // ── blueprints
        ("list_blueprints", Tool("blueprints_list")),
        // ── database
        ("databases_status", Tool("stack_status")),
        ("db_engine_versions", Gap("which version of each engine is installed and selected — no read tool carries it")),
        ("set_db_engine_version", Gap("switch an engine's version — parity §4.3 planned it under `stack` (system); not built")),
        ("adminer_status", Gap("whether Adminer is installed and at which version — no read tool carries it")),
        ("adminer_update_check", Gap("parity §4.3 planned it under `stack` (system); not built")),
        ("adminer_update_apply", Gap("parity §4.3 planned it under `stack` (system); not built")),
        ("adminer_set_theme", Gap("cosmetic; parity §4.3 planned it as manage; not built")),
        // ── downloads
        ("core_binaries_plan", Gap("which binaries are cached and which a start would fetch — no read tool carries it")),
        ("downloads_state", Gap("in-flight downloads — no read tool carries them")),
        ("prefetch_core_binaries", Gap("parity §4.3 planned a `downloads` tool (manage); not built")),
        ("retry_download", Gap("parity §4.3 planned a `downloads` tool (manage); not built")),
        // ── logs
        ("log_targets", Tool("site_logs")),
        ("tail_log", Tool("site_logs")),
        ("wp_debug_log_status", Tool("site_logs")),
        ("wp_debug_log_tail", Tool("tail_log")),
        ("log_clear", Gap("parity §4.3 planned `log_clear` (destroy); not built")),
        ("wp_debug_log_clear", Gap("parity §4.3 planned `log_clear` (destroy); not built")),
        ("log_download", Never("returns a path for a Save dialog; an agent reads the log with site_logs")),
        ("wp_debug_log_download", Never("returns a path for a Save dialog; an agent reads the log with tail_log")),
        // ── mail
        ("mailpit_status", Tool("stack_status")),
        ("mail_catch_all", Tool("stack_status")),
        // ── mcp
        ("agent_activity", Tool("agent_activity")),
        ("mcp_status", Never("an agent talking to this server already has the answer; the switch is the person's (D16)")),
        ("agent_access_get", Never("a refusal names the level it needs; the dial is the person's to read and set (D15)")),
        ("agent_access_set", Never("an agent raising its own access level is self-granting (D15)")),
        ("mcp_set_enabled", Never("an agent switching the MCP server on or off is self-granting (D16)")),
        ("agent_activity_clear", Never("an agent erasing the feed of what agents did is audit-erasing (D16)")),
        // ── php
        ("list_php_versions", Tool("stack_status")),
        ("get_php_settings", Tool("php_settings")),
        ("frankenphp_embedded_php", Gap("which PHP FrankenPHP embeds — no read tool carries it")),
        // ── repo
        ("repo_run_offered_steps", Tool("repo")),
        ("repo_watch_log", Gap("a watch's own log — `repo` lists watches, not what they printed")),
        // ── services
        ("services_status", Tool("stack_status")),
        // ── settings
        ("get_setting", Tool("settings_get")),
        ("default_tld", Tool("stack_status")),
        // `sites_dir` is a gated setter, and `settings_access::cli_access` makes every
        // gated key ReadWrite — so `settings_get` reads it.
        ("sites_folder", Tool("settings_get")),
        // ── site_provision
        ("site_provision_job", Never(JOBS)),
        ("site_provision_active", Never(JOBS)),
        ("site_provision_cancel", Never(JOBS)),
        // ── sites
        ("list_sites", Tool("list_sites")),
        ("all_site_domains", Tool("list_sites")),
        ("sites_serving", Tool("site_status")),
        ("site_domains", Tool("site_info")),
        ("site_cert_info", Tool("site_info")),
        ("scratch_packages", Tool("site_info")),
        ("inspect_linked_folder", Tool("site_inspect_folder")),
        ("create_site", Tool("site_create")),
        ("delete_site", Tool("site_delete")),
        ("set_site_enabled", Tool("site_configure")),
        ("set_all_sites_enabled", Tool("stack")),
        ("keep_site", Never("Keep is the person's act by definition (#213)")),
        ("sites_resources", Gap("per-site CPU and memory — no read tool carries it")),
        // ── system
        ("global_status", Tool("stack_status")),
        ("dns_status", Tool("stack_status")),
        ("cli_status", Tool("stack_status")),
        ("app_info", Gap("the app's version and build — no read tool carries it")),
        ("autostart_status", Gap("start-at-login state — parity §4.4 planned it under stack_status; not built")),
        ("set_autostart", Gap("parity §4.4 planned `settings(action: autostart)` (system); not built")),
        ("init_error", Gap("why the app failed to start its stack — no read tool carries it")),
        ("startup_notices", Gap("the notices the app raised at launch — no read tool carries them")),
        ("unresolvable_tlds", Gap("sites on a TLD this machine cannot resolve — no read tool carries them")),
        ("firefox_trust_status", Gap("whether Firefox trusts the local CA — no read tool carries it")),
        ("setup_edge_conflict", Gap("parity §4.4 planned `stack(action: resolve_edge_conflict)` (system); not built")),
        ("open_external", Never("an arbitrary URL opener (parity §4.4); `open` is site-scoped")),
        ("cli_install", Never(AMPLIFIER)),
        ("system_setup", Never(AMPLIFIER)),
        ("uninstall_system", Never(AMPLIFIER)),
        ("regenerate_certs", Never(AMPLIFIER)),
        ("trust_local_ca", Never(AMPLIFIER)),
        ("trust_ca_in_firefox", Never(AMPLIFIER)),
        // ── terminal
        ("terminal_open", Never(PTY)),
        ("terminal_write", Never(PTY)),
        ("terminal_resize", Never(PTY)),
        ("terminal_close", Never(PTY)),
        // ── wordpress
        ("wp_core_versions", Gap("parity §4.2 folded it into `wp_info`; its `what` enum does not offer it")),
        ("wp_cli_packages", Gap("parity §4.2 folded it into `wp_info`; its `what` enum does not offer it")),
        ("wp_org_search_plugins", Tool("wp_org_search")),
        ("wp_org_search_themes", Tool("wp_org_search")),
        ("wp_org_plugin_icons", Never("icon URLs for the app's list rows; wp_org_search returns the plugins themselves")),
        ("wp_default_creds", Never("credentials (parity §4.2)")),
        // ── wp_install
        ("wp_install_job", Never(JOBS)),
        ("wp_install_active", Never(JOBS)),
        ("wp_install_cancel", Never(JOBS)),
    ];

    fn root() -> &'static std::path::Path {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    /// The app's whole IPC surface, read the way `copy_scan` reads it.
    fn registered_commands() -> BTreeSet<String> {
        let lib = std::fs::read_to_string(root().join("src/lib.rs")).expect("lib.rs");
        let handler = lib
            .split("tauri::generate_handler![")
            .nth(1)
            .and_then(|b| b.split("])").next())
            .expect("the invoke_handler list");
        handler
            .lines()
            .filter_map(|line| {
                let t = line.trim().trim_end_matches(',');
                t.strip_prefix("commands::").and_then(|r| r.rsplit("::").next()).map(str::to_string)
            })
            .filter(|n| !n.is_empty() && !n.contains(' '))
            .collect()
    }

    /// Every `commands::<module>::<name>` the MCP server's source names — as a
    /// call or as a function value (`.map(crate::commands::system::list_browsers)`).
    fn reached_from_mcp_source() -> BTreeSet<String> {
        let mut src = std::fs::read_to_string(root().join("src/mcp_server.rs")).expect("mcp_server.rs");
        for e in std::fs::read_dir(root().join("src/mcp_server")).expect("mcp_server/").flatten() {
            // This file names every command in its table; counting those as
            // reached would make the guard pass by reading its own rulings.
            if e.file_name() == "parity.rs" {
                continue;
            }
            src.push_str(&std::fs::read_to_string(e.path()).unwrap_or_default());
        }
        let mut out = BTreeSet::new();
        for piece in src.split("commands::").skip(1) {
            let Some((_module, rest)) = piece.split_once("::") else { continue };
            let name: String =
                rest.chars().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_').collect();
            if !name.is_empty() {
                out.insert(name);
            }
        }
        out
    }

    fn tool_names() -> BTreeSet<&'static str> {
        super::super::tools::registry()
            .iter()
            .map(|t| t.name)
            .chain(super::super::scratch::registry().iter().map(|t| t.name))
            .chain(super::super::user_sites::registry().iter().map(|t| t.name))
            .collect()
    }

    /// **Every registered command is either reached by the MCP server or
    /// carries a written ruling — a tool that reaches it, a reason it never
    /// will, or a named gap.** See the module doc for why this replaced a
    /// table in a plan.
    #[test]
    fn every_command_is_reachable_by_an_agent_or_ruled_on_by_name() {
        let registered = registered_commands();
        let reached = reached_from_mcp_source();
        let tools = tool_names();
        // Floors: a scan that silently finds nothing would pass every check below.
        assert!(registered.len() > 200, "only {} commands parsed — the handler scan is broken", registered.len());
        assert!(registered.intersection(&reached).count() > 100, "the MCP source scan found almost nothing");
        assert!(tools.len() >= 45, "only {} tools enumerated — the registry scan is broken", tools.len());

        let mut ruled: BTreeSet<&str> = BTreeSet::new();
        for &(name, disposition) in DISPOSITIONS {
            assert!(ruled.insert(name), "`{name}` is ruled on twice");
            assert!(registered.contains(name), "`{name}` has a ruling but is no longer a registered command — delete the row");
            assert!(
                !reached.contains(name),
                "`{name}` is now reached by the MCP source directly — delete its row ({disposition:?}); the derivation covers it"
            );
            match disposition {
                Tool(tool) => assert!(tools.contains(&tool), "`{name}` says tool `{tool}` reaches it, and there is no such tool"),
                Never(why) | Gap(why) => assert!(why.len() > 20, "`{name}`'s ruling needs a real reason, not {why:?}"),
            }
        }

        let unruled: Vec<&String> =
            registered.iter().filter(|c| !reached.contains(*c) && !ruled.contains(&c.as_str())).collect();
        assert!(
            unruled.is_empty(),
            "these commands are registered, the MCP server does not reach them, and nobody has said what an agent \
             may do with them: {unruled:?}. Add a row to DISPOSITIONS in src-tauri/src/mcp_server/parity.rs — \
             Tool(\"…\") if a tool reaches the same core function, Never(\"why\") if it must not be a tool, or \
             Gap(\"…\") and a docs/TODO.md row if it should be and is not yet."
        );
    }
}
