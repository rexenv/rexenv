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
        /// Should be reachable and is not yet — with a `docs/TODO.md` row. Unused
        /// since 11 Sep 2026, when the last of the first thirty closed (#564); kept
        /// because it is the honest answer for the next command that arrives
        /// before its tool does, and removing it would make "not yet" unsayable.
        #[allow(dead_code)]
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
        ("app_update_restart", Never(UPDATE_FLOW)),
        // ── valet_import: the banner's generation-keyed feed (#807); `valet_import` action
        //    `drift` reaches the same `drifted_takeover_records` (it reports the TLDs).
        ("resolver_drift_records", Tool("valet_import")),
        // ── blueprints
        ("list_blueprints", Tool("blueprints_list")),
        // ── database
        ("databases_status", Tool("stack_status")),
        ("db_engine_versions", Tool("stack_status")),
        ("db_engine_refusals", Tool("stack_status")),
        // ── logs
        ("log_targets", Tool("site_logs")),
        ("tail_log", Tool("site_logs")),
        ("wp_debug_log_status", Tool("site_logs")),
        ("wp_debug_log_tail", Tool("tail_log")),
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
        ("frankenphp_embedded_php", Tool("stack_status")),
        // ── repo
        ("repo_run_offered_steps", Tool("repo")),
        // ── services
        ("services_status", Tool("stack_status")),
        ("runtime_problem", Tool("stack_status")),
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
        ("offered_web_servers", Never("the New Site dialog's option list. An agent does not pick from a menu: it names a server and core refuses an unavailable one at create time, with the same sentence")),
        ("offered_db_engines", Never("the New Site dialog's other option list, and the same reasoning: an agent names an engine and core refuses one with no pin on this platform, by name")),
        ("site_domains", Tool("site_info")),
        ("site_cert_info", Tool("site_info")),
        ("scratch_packages", Tool("site_info")),
        ("inspect_linked_folder", Tool("site_inspect_folder")),
        ("create_site", Tool("site_create")),
        ("delete_site", Tool("site_delete")),
        ("set_site_enabled", Tool("site_configure")),
        ("set_all_sites_enabled", Tool("stack")),
        ("keep_site", Never("Keep is the person's act by definition (#213)")),
        // Without the database size: that runs a client per engine, and the read
        // tools run nothing (`commands::sites::resources_of`'s flag; db_query asks).
        ("sites_resources", Tool("site_info")),
        // ── system
        ("global_status", Tool("stack_status")),
        ("dns_status", Tool("stack_status")),
        ("cli_status", Tool("stack_status")),
        ("app_info", Tool("stack_status")),
        ("legacy_notice", Never("UI copy: the one onboarding sentence a macOS 13/14 host sees — what the tier refuses reaches an agent as the refusal itself, on the call it refuses")),
        ("platform_words", Never("UI copy: the words a button or a hint uses for this OS's own things (W7 S7) — an agent reads the same words inside the errors that carry them")),
        ("autostart_status", Tool("stack_status")),
        ("init_error", Never("when init fails there is no AppState, and the MCP server answers every call \"rexenv is still starting\" — an agent can never observe a value to read")),
        ("startup_notices", Never("reading DRAINS the queue the person's screen toasts from — an agent read would take the notice from them; the same facts are in rexenv.log, which site_logs reads")),
        ("unresolvable_tlds", Tool("stack_status")),
        ("firefox_trust_status", Tool("stack_status")),
        ("open_external", Never("an arbitrary URL opener (parity §4.4); `open` is site-scoped")),
        ("cli_install", Never(AMPLIFIER)),
        ("system_setup", Never(AMPLIFIER)),
        ("uninstall_system", Never(AMPLIFIER)),
        ("regenerate_certs", Never(AMPLIFIER)),
        ("trust_local_ca", Never(AMPLIFIER)),
        ("trust_ca_in_firefox", Never(AMPLIFIER)),
        ("allow_tlds_in_firefox", Never("writes the person's own browser profiles (user.js); it rides the resolver install and the CA trust already, and firefox_trust_status's counts reach stack_status")),
        // ── terminal
        ("terminal_open", Never(PTY)),
        ("terminal_write", Never(PTY)),
        ("terminal_resize", Never(PTY)),
        ("terminal_close", Never(PTY)),
        // ── wordpress
        ("wp_org_search_plugins", Tool("wp_org_search")),
        ("wp_org_search_themes", Tool("wp_org_search")),
        ("wp_org_plugin_icons", Never("icon URLs for the app's list rows; wp_org_search returns the plugins themselves")),
        ("wp_default_creds", Never("credentials (parity §4.2)")),
        // The banner's question, answered more widely: `checksums` names EVERY missing core file,
        // the long-named casualties included, and wp_maintain's `core_reinstall` is the repair.
        ("wp_core_cut_names", Tool("wp_info")),
        // ── worktree (docs/PLAN-git-worktrees.md) — `worktree_children/preview/remove`
        //    are called by name from the MCP source; these two are reached otherwise:
        ("worktree_create", Tool("worktree")),
        ("worktree_of", Tool("worktree")),
        ("worktree_relations", Never("the Sites list's grouping rows; an agent reads the same through the worktree action's list")),
        // ── live_sync (docs/PLAN-wp-live-sync.md §2.9: pull is Full-level agent work LATER —
        //    S4 adds `live` status/diff/pull; pairing takes a pasted secret, and push/rollback
        //    are never tools, owner ruling 9 Oct 2026)
        ("live_sync_pair", Never("the pasted key is a secret the person holds; an agent that could pair a site could point it at any site it chose (plan invariant #3)")),
        ("live_sync_pairing", Gap("S4 of docs/PLAN-wp-live-sync.md adds the `live` tool's status (Read); TODO row WordPress live ↔ local sync")),
        ("live_sync_unpair", Never("undoing the person's pairing; the person does it in the Live tab")),
        ("live_sync_pull", Gap("S4 of docs/PLAN-wp-live-sync.md adds the `live` tool's pull (Full — it replaces the local database); TODO row WordPress live ↔ local sync")),
        ("live_sync_job", Never(JOBS)),
        ("live_sync_active", Never(JOBS)),
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
