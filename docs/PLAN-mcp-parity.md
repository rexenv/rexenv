# MCP parity — every rexenv function drivable by an agent

**Status: PLANNED 3 Sep 2026; D8–D13 ALL SETTLED by the owner the same day, on the recommendations (§7); P1 (the foundation) SHIPPED the same day in six commits, ledger #468–#474 — see `docs/TODO.md` for the per-task evidence. Next: P2, the site lifecycle.** This is the TODO list for the MCP
server after every milestone in `docs/PLAN-mcp-server.md` shipped (M1, M2a, M2b, M3).
It has two halves, and they are different kinds of work:

- **§1 — what the shipped plan still owes.** Leftovers audited against the tree on
  3 Sep 2026: human gates never recorded as run, one open decision, three 🔨 live
  legs, two deferred-with-trigger items whose trigger has fired, and three docs that
  still said M3 was unbuilt nine days after it shipped (fixed in this commit).
- **§2–§6 — parity.** The owner's brief, verbatim in spirit: *"any work the rexenv
  app can do should be doable through MCP — not scratch subdomains only; a real
  WordPress, blank PHP or Laravel site like the Sites page makes, and switching its
  PHP or web server, all of it."* That reverses two rulings the shipped plan made
  (§2.1), so the reversal is stated as a decision, not slipped in as a feature.

**How the coverage was measured.** The app's whole surface is the
`generate_handler!` list in `lib.rs` — **231 Tauri commands across 21 modules**
(`scripts/doc-counts.sh` is where a number like this belongs; this one is a
snapshot for the audit, not a claim to keep). Twelve are MCP tools today. §4 walks
every module and files each command as *already a tool*, *expose*, *expose behind a
grant*, *read-only*, or *never*, with the reason. The CLI already proved that the
app's commands can be driven from a second mouth with no parallel implementation
(`docs/CLI-ROADMAP.md` — every dispatch arm, ledger #57) — MCP parity is the same
exercise with a consent model in front of it.

---

## 0. The one-sentence shape

Today an agent can **see** every site and **change** only the ones it made. Parity
adds a **third registry** of tools that act on the USER's own sites and on the stack,
each reachable only through a **recorded, scoped, expiring grant the user gives in
the app** — the exact machinery M3 built for `db_query`, generalised from one
resource (a database, read-only) to a small set of scopes — so the guarantee becomes:
*an agent changes what you made only where you said it could, for as long as you
said, and you can see and revoke every such permission in one place.*

## 1. What the shipped plan still owes (audited 3 Sep 2026)

Ordered by what it costs to leave open. Each row says where the evidence is.

| # | Item | Where | Status |
|---|---|---|---|
| L1 | **Three docs said M3 was unbuilt.** `PLAN-mcp-server.md`'s header ("nothing of it is in the tree"), `ARCHITECTURE.md` §8.3 first bullet ("not built — nothing of it is in the tree"), and the `CLAUDE.md` router row ("M3 (DB access) unbuilt") — while `docs/TODO.md` records M3 shipped 24–25 Aug in seven commits and its gate run. The TODO row itself says the header was "corrected twice"; the file did not carry the second correction. | this commit | ✅ fixed 3 Sep 2026 — all three now say M3 shipped and point here for what is left |
| L2 | **Human gates not recorded as run** (`SMOKE-TEST.md` §MCP): step 1 literally (a never-enabled launch), 9 (Keep), **11 ⚠HOLD** (no admin prompt — an observation, not a check), 13's eyes-only copy, **8's model half** (the mechanical half passed by a hand-rolled client; "survives contact with a model that wants to help" has not), 16 and 17 (M3 eyes-only), and 21's real-site arm. | `TODO.md` "The M1/M2a/M2b human gates" row | ◐ owner's run — needs a packaged build + a real Claude Code session |
| L3 | **D2 — `wp_login_url`, scratch-only.** The one open decision; "recommend include, drop without argument if it reads as surface for surface's sake". | `PLAN-mcp-server.md` §9 D2 | folds into §4 (`site_login_url`: scratch T0, real behind the `manage` scope) — decide there, once |
| L4 | **🔨 live legs**: #215 (reaper delete + skip-don't-stop with a live tunnel), #219 (packaged-webview eyeball of the enable-moment paragraph at both widths), #401/#403 ◐ (the `db_query` gate's and the delete-drops-accounts row's unplanted halves). | `CLAIM-LEDGER.md` | 🔨 |
| L5 | **In-band "rexenv stopped" error in `rex mcp` — deferred WITH A TRIGGER, and the trigger fired at M2.** §2.3 said the dumb pipe is fine while there are no tools, and that a mid-session app quit becomes a *model-facing* failure the day tools land. Nine executing tools have landed; `cli/src/main.rs::run_mcp_bridge` is still the dumb byte pipe. A model whose `scratch_create_site` dies with a bare EOF will guess. | `PLAN-mcp-server.md` §2.3; `cli/src/main.rs:210-230` | ⏳ build — §5 P6 |
| L6 | **Tool annotations never emitted.** §2.4 says tools carry `readOnlyHint`/`destructiveHint`; `tools_list_result` emits name/description/inputSchema only (grep: no `Hint` in `mcp_server.rs`). Harmless today (12 tools, obvious names); load-bearing at parity, where a client's confirm-before-destructive UX reads exactly these. | `mcp_server/tools.rs::tools_list_result` | ⏳ — §5 P6 |
| L7 | **TTL and cap are compile-time constants** (`MAX_SCRATCH_SITES = 5`, 24 h) where §4.2/§4.3 said settings. `ARCHITECTURE.md` §8.3 records the divergence. Not a defect; a parity agent that runs a matrix across five PHP minors on five plugins will hit the cap. | `core/scratch.rs:77` | ✅ P4.3, 3 Sep 2026 (#488) — `scratch_cap` / `scratch_ttl_hours`, gated setters, the constants as defaults |
| L8 | **Laravel headline (M-later trio)**: scratch Laravel skeleton, `php_artisan` runner, Composer path-repo link. Ranked in §5.1/§7.3 of the old plan, never started. Parity makes the first one moot in the scratch form (a real Laravel site is creatable, §4) but the runner and the path-repo link are still the Laravel dev loop. | `PLAN-mcp-server.md` §5.1 | ⏳ — §5 P7 |
| L9 | **Progress for long tool calls.** The CLI got streaming 2 Sep 2026 (#447); MCP still blocks the whole call (`state_of` poll) — fine for a 60 s scratch create, not for a git-cloned Laravel site with `composer install` + asset build. | `PLAN-mcp-server.md` §2.5 | ⏳ — §5 P6 |
| L10 | Windows/Linux socket path + named pipe. | Phase 4 | out of scope, unchanged |

## 2. What parity reverses, and what it keeps

### 2.1 Two rulings reopened — by the owner's brief, recorded as such

1. **D3 "real-site mutation: left unpromised, may ship never"** → **reopened.** The
   brief asks for exactly this. D3's own text said the consent machinery "still lands
   in M3, so it is an extension not a rework" — that is what makes this buildable:
   `agent_db_grants` + `GrantRequests` + the Settings → AI agents prompt + auto-allow
   are the consent path, and parity generalises them (§3).
2. **§3.3 "Real-site create/delete/rename/move; linked-site creation — a human
   flow; agents get scratch sites only"** → **reopened**, with the linked-site and
   git-clone halves kept behind a stricter scope than the rest (§3.2), because
   `PLAN-git-site-clone.md` §2.7 ("agents may not clone — executing third-party
   code chosen by a model with no click") and `validate_linked_docroot` are about
   *what runs*, not *whose site it is*. The grant IS the click.

### 2.2 What does not move

- **T2 stays T2 for the amplifiers**: CA operations (`regenerate_certs`,
  `trust_local_ca`, `trust_ca_in_firefox`), `system_setup`, `uninstall_system`,
  `cli_install`, the terminal (a PTY is a shell; the agent has its own), the MCP
  toggles and grant commands themselves (an agent granting itself is the thing
  consent exists to prevent), `list_site_env` VALUES and `wp_default_creds` (secrets
  by definition), `open_external` (an arbitrary-URL opener). §4 marks each.
- **Ownership stays a recorded column.** `origin` decides which registry a site is
  reachable from; nothing here derives it from a name.
- **Scratch tools do not promote** (#223). A parity tool acting on a user's site
  needs no promotion (it is already the user's); a parity tool acting on a scratch
  site is refused — use the scratch tool. One site, one registry.
- **The honest-guarantee discipline**: the enable-moment copy is held to the
  registry by a guard (#209/#211) and it WILL fire when the third registry lands,
  because "an agent cannot change or delete your own sites" stops being
  unconditionally true. §6 drafts the replacement FIRST, the way §6.0 did for M2.
- **No SDK, no TCP, no autostart, two-then-three disjoint registries, the one
  scrubber, the sweep over every registered tool, the feed complete by
  construction.** Every parity tool inherits all of it by living in a registry.

## 3. The consent model — one generalisation, not a new mechanism

### 3.1 Grants, generalised from `agent_db_grants`

| | M3 today | Parity |
|---|---|---|
| Table | `agent_db_grants(site_id, client, db_user, granted_at, expires_at, revoked_at)` | `agent_site_grants(site_id NULLABLE, client, scope, granted_at, expires_at, revoked_at, auto_granted)` — v43+ (**check `MIGRATIONS` length at build time; do not write the number down here**). `site_id NULL` = a stack-level scope (services, PHP versions, settings). `agent_db_grants` stays as-is: the DB grant provisions an account, which a scope row does not |
| Ask | recorded on `db_query`'s REFUSAL path, in memory, session-scoped | same shape: every refused parity call records `(client, site, scope)` in `GrantRequests`; the Settings card lists them; there is no "request access" tool |
| Answer | Allow for 7 days / Don't allow | Allow for 7 days / Allow for this session / Don't allow — the middle option is new and is what a dev loop actually wants |
| Auto-allow | `AppState`, dies with the process, DB prompt only | per-scope, same lifetime rule; `destroy` and `system` may NOT be auto-allowed (a standing yes to "delete a site" is not a convenience) |
| Witness | `ScratchSite` (one constructor, reads the row) | `Granted<Scope>` — one constructor, reads the grant row AND the site row, re-asserted before each destructive step exactly as `still_the_agents` does |
| Where the gate runs | `core::agent_db::authorize`, pure, before anything opens | `core::agent_grants::authorize(client, site, scope) -> Decision`, pure, before anything opens; the CLI and UI inherit it because it lives in core |

### 3.2 The scopes — five, closed, ranked by blast radius

| Scope | Covers | Why its own scope |
|---|---|---|
| `read` | wp-cli reads on a real site (`wp_info`, plugin/theme/user/option lists, cron events, core versions…), every log source (scrubbed), the user's Mailpit inbox (behind the existing mail toggle) | Boots WordPress = runs the site's code as the user, and reads real content — the M3 dialog's "password hashes and tokens" sentence applies verbatim. NOT the same as M1's read tools, which never run site code |
| `manage` | Non-destructive mutation: PHP/web-server/Xdebug switches, env set, rename, domain add/remove/change, move/relink, restart, retry, cert regenerate; WP plugin/theme install/activate/update, user create/role, option update, debug/maintenance/permalink set, cache/rewrite/transient flush, cron run, language switch, multisite convert, network site create, login URL; DB engine start/stop, PHP version install, prefetch | Reversible or additive. The dialog says "can change how the site is served and what is installed in it; cannot delete it or its data" |
| `destroy` | `delete_site`, `wp_db_import`, `wp_site_reset`, plugin/theme/user/network-site delete, `wp_core_switch_version`, `wp_search_replace` (not dry-run), `mailpit_clear`/`delete`, `log_clear`, blueprint delete, `db_import_delete_leftover`, `rewrite_apply`/`revert` | Loses data or user work. Never auto-allowed; the dialog names what is lost; every tool carries `destructiveHint` |
| `run` | Raw `wp_run` on a REAL site, `php_artisan`, repo scripts/steps/`repo_add`/`git_op`/`watch`, `db_import_start`, `valet_import_run`, git-sourced and linked-folder site creation | Executes code the agent chose, on the user's site or from the network — D1's "real-site raw-wp refusal stays permanent" and git-clone §2.7 both live here, as a scope the user grants knowingly rather than a permanent no |
| `system` | `start_services`/`stop_services`, `set_default_tld`, `repair_resolver`/`remove_resolver`, `resolver_take_over`/`hand_back`, `set_autostart`, `set_default_php_version`, `set_setting` (through `settings_access`, same allow-list as the CLI), `set_db_engine_version`, `php_update_apply`/`adminer_update_apply` | Machine-wide. The ones that reach `run_privileged` ALSO prompt macOS — that dialog is a second consent, and the tool's description says so. "Never make the prompt routine" (§3.1) is kept by the grant expiring and by refusing auto-allow |

**Why not per-call dialogs (§3.4's original T1 shape)?** A dev loop is dozens of
calls; a 120 s native dialog per call is the prompt-fatigue amplifier the old plan
warned about, and a fatigued user clicks Allow. A scoped, expiring, visible,
revocable grant is what M3 already proved works with a real client on a real site
(§M3 gate, 25 Aug). Per-call remains the shape for `destroy` when the owner wants
it (D9).

### 3.3 The sub-toggle: "Let agents manage my own sites"

Default OFF, independent of the master and mail toggles, same pattern as
`mcp_mail_enabled`: the third registry is REGISTERED regardless (so `tools/list` is
stable and the sweep covers it), and every call is refused BY NAME pointing at the
toggle while it is off. Its copy is on the must-say list (§6). While it is off,
today's guarantee sentence stays exactly true — which is what lets a user who never
turns it on keep reading the paragraph they enabled MCP under.

## 4. Coverage matrix — 231 commands, by module

Legend: **✅ tool** = exists today · **➕ T0** = expose unattended (user-level, no
site of the user's touched or read-only rexenv state) · **🔑 read/manage/destroy/
run/system** = expose behind that scope (§3.2) · **🚫** = never a tool, reason given.
"Grouped as" names the proposed tool — parity does NOT mean 200 flat tools (§5 P6:
some clients cap the per-server tool count, and every description costs the model
context on every turn). The proposed surface is **~40 tools**, most taking an
`action` enum.

### 4.1 sites (26) + site_provision (4) + wp_install (3) + scratch commands

| Command | Disposition | Grouped as / note |
|---|---|---|
| `list_sites` | ✅ tool | extend the view with `origin`, `docroot_subdir`, aliases (`site_domains`), `xdebug` |
| `sites_serving`, `sites_resources`, `site_cert_info`, `site_domains`, `all_site_domains` | ➕ T0 | fold into `site_status` (serving chain already there) + `site_info(site_id)` |
| `scratch_packages` | ➕ T0 | into `site_info` for scratch rows |
| `inspect_linked_folder` | ➕ T0 | `site_inspect_folder(path)` — runs `validate_linked_docroot` + `detect_project`, executes nothing; the same blast-radius refusals as `scratch_add_package` |
| **`create_site`** (WordPress / PHP / Laravel; new docroot) | 🔑 manage (stack-level grant, `site_id NULL`) | **`site_create`** — `NewSite` minus `path`/`git_url`, plus `wp: InstallOptions`, `multisite`, `blueprint`, `starter_db`. Runs the SAME provision job (`SiteCreator` erasure from #211), `Ownership::User`, `ResolverPrompt::Never` **unless the grant is `system`** — a `.rex` site never prompts; a new TLD is a resolver write and refuses with "grant system or add the TLD in rexenv". `admin_password` generated + returned once, never echoed to the feed |
| `create_site` with `path` (link an existing folder) | 🔑 run | `site_link(path, …)` — serves user code the agent pointed at; `validate_linked_docroot` verbatim; the dialog names the folder |
| `create_site` with `git_url` (clone) | 🔑 run | `site_create_from_git(url, ref, type, migrate, build_assets)` — reverses git-clone §2.7 for the granted case only; the dialog quotes the URL; `git_build_assets` stays default-false |
| `delete_site` | 🔑 destroy | `site_delete` — the app's full delete path (provenance, tunnels `refuse_if_shared`, agent accounts dropped #403) |
| `rename_site`, `change_site_domain`, `add_site_domain`, `remove_site_domain` | 🔑 manage | `site_configure(action: rename\|domain\|add_domain\|remove_domain)` |
| `set_site_php_version`, `set_site_web_server`, `set_site_xdebug` | 🔑 manage | `site_configure(action: php\|server\|xdebug)` — **on a user site these call the app command WITH `promote_if_scratch`**, which is a no-op for `origin='user'`; on a scratch site refused ("use set_php_version") so #223's cap-bypass proof stays true |
| `set_site_env` | 🔑 manage | `site_configure(action: env_set\|env_unset)` — write-only: **values are never read back** (`list_site_env` 🚫 below); the reply says "set" and the key |
| `list_site_env` | 🚫 values / ➕ keys | `site_info` lists KEY NAMES only. Users park real secrets there (old plan §3.5) |
| `move_site_docroot`, `relink_site_docroot` | 🔑 manage | `site_configure(action: move\|relink)` — the app's preflights; relink path blast-radius validated |
| `regenerate_site_cert` | 🔑 manage | `site_configure(action: regenerate_cert)` |
| `restart_site` | 🔑 manage | `site_restart(site_id, pool: bool)` — the three honest outcomes from #444, verbatim |
| `keep_site` | 🚫 | Keep is the user's act by definition (#213) |
| `site_provision_job`, `site_provision_active` | ➕ T0 | `job_status(job_id)` — one tool for every job registry (§5 P6) |
| `site_provision_cancel`, `site_provision_retry` | 🔑 manage (T0 on the agent's own scratch site) | `job_cancel`, `site_retry` |
| `wp_install_job/active/cancel` | same as provision | same `job_*` tools |
| `scratch_*` (existing) | ✅ | unchanged |

### 4.2 wordpress (61)

Every one of these runs wp-cli against a real site, i.e. boots the user's code.
Group into **eight tools with an `action` enum**, all refusing on a scratch site
(use `wp_run` there):

| Tool | Actions (← commands) | Scope |
|---|---|---|
| `wp_info` | `wp_info`, `wp_debug_get`, `wp_debug_flag_get`, `wp_maintenance_get`, `wp_permalink_get`, `wp_core_versions`, `wp_languages`, `wp_cli_packages`, `wp_primary_admin`, `wp_options` (names + values — the same sensitivity as `db_query`, said in the `read` dialog) | 🔑 read |
| `wp_plugin` | `list` ← `wp_plugins`; `install/activate/deactivate/update` (+ `_network` variants) | 🔑 read for `list`, manage otherwise; `delete` → **destroy** |
| `wp_theme` | `list`, `install/activate/update`, `enable_network/disable_network`, `themes_network_enabled` | as above; `delete` → destroy |
| `wp_user` | `list`, `create` (password generated + returned once), `set_role`, `login_url` (D2 — real sites here, scratch T0), `super_admins`, `super_admin_add` | manage; `delete` (with the #446 required fork) and `set_password` → **destroy** (a password reset locks a human out) |
| `wp_option` | `update` ← `wp_option_update`; `debug_set`, `debug_flag_set`, `maintenance_set`, `permalink_set`, `switch_language` | manage |
| `wp_maintain` | `cache_flush`, `rewrite_flush`, `transient_delete_all`, `cron_events`(read), `cron_run_due`, `cron_run_hook`, `core_verify_checksums`(read), `checksum_cleanup`, `core_update`, `core_reinstall` | manage; `core_switch_version` → destroy |
| `wp_data` | `db_export` (reply = the path under `~/Downloads`, same contract as the CLI), `content_export`, `search_replace --dry-run` | manage; `db_import`, `site_reset`, `search_replace` live → **destroy** |
| `wp_network` | `multisite_convert`, `network_sites`, `network_site_create` | manage; `network_site_delete` → destroy |
| `wp_org_search_plugins/themes`, `wp_org_plugin_icons` | ➕ T0 | `wp_org_search(kind, query)` — network read, no site |
| `wp_admin_login_url` | 🔑 manage (real) / T0 (scratch) | D2 settled here: **include**, as `wp_user(action: login_url)`; the URL is one-time and is NOT written to the feed's `args_summary` |
| `wp_default_creds` | 🚫 | credentials |
| `wp_run` on a REAL site | 🔑 run | the existing tool, third registry arm, same `WP_TARGET_PARAMS` refusal. D1's permanent refusal becomes "refused unless the user granted `run`" |

### 4.3 services (4) · php (8) · database (9) · mail (9) · logs (8) · downloads (4)

| Command | Disposition | Grouped as / note |
|---|---|---|
| `services_status`, `databases_status`, `db_engine_versions`, `mailpit_status`, `downloads_state`, `core_binaries_plan`, `list_php_versions`, `frankenphp_embedded_php`, `get_php_settings`, `php_update_check`, `adminer_status`, `adminer_update_check` | ➕ T0 | `stack_status()` (the `rex doctor` composite + these), `php_versions()`, `php_settings(minor)` |
| `start_services`, `stop_services` | 🔑 system | `stack(action: start\|stop)` — reaches `run_privileged`; **the macOS dialog is the second consent** and the description says so. Reverses old §3.3 row 1 for the granted case. The ledger row "no registered tool maps to a privileged-reachable command" must be RE-SCOPED to the first two registries in the same commit, or this tool does not ship (D10) |
| `restart_web_service` | 🔑 manage (stack-level) | `stack(action: restart, service)` — #445's rules verbatim (edge = reload) |
| `start_database`, `stop_database`, `start_mail`, `stop_mail` | 🔑 manage (stack-level) | `stack(action: start\|stop, service: mysql\|postgres\|mailpit)` — user-level, no prompt |
| `set_db_engine_version`, `set_default_php_version`, `php_update_apply`, `adminer_update_apply`, `adminer_set_theme` | 🔑 system | `stack(action: set_version …)`; `adminer_set_theme` is cosmetic → manage |
| `set_php_version_installed`, `prefetch_core_binaries`, `retry_download`, `apply_php_settings` | 🔑 manage (stack-level) | `php_install(minor)`, `php_settings_set(minor, k, v)` (validated in core as the CLI's is), `downloads(action)` |
| `mailpit_messages`, `mailpit_message`, `mailpit_message_raw` — the USER's inbox | 🔑 read **and** the existing mail toggle | `mail_inbox(action: list\|get\|raw)` — the credential-harvest pivot the old plan §3.5 named; the scratch-only `mail_list`/`mail_get` stay as they are |
| `mailpit_mark_all_read` | 🔑 manage | `mail_inbox(action: mark_read)` |
| `mailpit_clear`, `mailpit_delete` | 🔑 destroy | `mail_inbox(action: clear\|delete)` |
| `log_targets`, `tail_log` (every source: nginx/php-fpm/edge/db/access), `wp_debug_log_status/tail` | 🔑 read | widen the EXISTING `tail_log` with `source` (today: wp-debug only, any site, T0). Access logs carry `?rexenv_login=` tokens — the one scrubber (#201) already covers them; keep the T0 arm exactly as it is |
| `log_clear`, `wp_debug_log_clear` | 🔑 destroy | `log_clear(site_id, source)` |
| `log_download`, `wp_debug_log_download` | 🚫 | returns a path for a Save dialog; an agent reads via `tail_log` |

### 4.4 settings (6) · system (28) · tunnels (3) · valet_import (6) · rewrite (3) · db_import (7) · blueprints (3) · repo (26) · mcp (12) · terminal (4)

| Command | Disposition | Grouped as / note |
|---|---|---|
| `get_setting`, `default_tld`, `sites_folder`, `tld_policy` | ➕ T0 via `settings_access` | `settings(action: get, key)` — **the CLI's allow-list, deny-by-default, ONE list in core** (#397/#443); `mcp_*` keys stay Denied there already |
| `set_setting` | 🔑 system | `settings(action: set)` through `GATED_SETTERS`; `sites_dir` stays Denied (the 24 Jul class) |
| `set_default_tld` | 🔑 system | `tld(action: set)` — resolver write + macOS prompt |
| `app_info`, `global_status`, `dns_status`, `cli_status`, `autostart_status`, `init_error`, `startup_notices`, `unresolvable_tlds`, `firefox_trust_status`, `list_browsers`, `list_editors` | ➕ T0 | `stack_status()` / `app_info()` — the `rex doctor` composite; the `Agent*` view drops socket/Caddyfile paths as `site_status` already does |
| `open_in_browser`, `open_in_editor`, `reveal_path` | 🔑 manage | `open(site_id, target: browser\|editor\|finder)` — site-scoped, never an arbitrary path/URL |
| `open_external` | 🚫 | arbitrary URL opener |
| `set_autostart` | 🔑 system | `settings(action: autostart)` |
| `repair_resolver`, `remove_resolver`, `setup_edge_conflict` | 🔑 system | `tld(action: repair\|remove)`, `stack(action: resolve_edge_conflict)` — root ops, macOS prompt = second consent |
| `system_setup`, `uninstall_system`, `cli_install`, `regenerate_certs`, `trust_local_ca`, `trust_ca_in_firefox` | 🚫 | CA / install / uninstall — the amplifiers (§2.2) |
| `tunnels_status` | ➕ T0 | into `site_status` ("shared: yes, url") — the agent may SEE a share (old §3.7) |
| `stop_tunnel` | 🔑 manage | `share(action: stop)` — stopping a public exposure is the safe direction |
| `start_tunnel` | 🔑 run, **never auto-allowed, + auto-stop** | `share(action: start, minutes ≤ 60)` — D6's reopening condition was "real demand + T1 + scratch-only + auto-stop"; the brief is the demand, the grant is the T1, and the timer is the auto-stop. **Scratch-only or any site is D11** |
| `scan_valet_import`, `resolver_drift` | ➕ T0 | `valet_import(action: scan)` — executes nothing |
| `valet_import_run`, `valet_import_cancel` | 🔑 run | imports serve user code |
| `resolver_take_over`, `resolver_hand_back` | 🔑 system | root ops |
| `rewrite_preview` | 🔑 read | `connection_rewrite(action: preview)` |
| `rewrite_apply`, `rewrite_revert` | 🔑 destroy | rewrites config files in the user's checkout (backup-first is the app's contract) |
| `db_import_state/records/leftovers/record` | 🔑 read | `db_import(action: status)` |
| `db_import_start`, `db_import_cancel` | 🔑 run | Valet/Herd dump→restore into a rexenv database |
| `db_import_delete_leftover` | 🔑 destroy | |
| `list_blueprints` | ➕ T0 | `blueprints(action: list)` |
| `save_blueprint` | 🔑 manage | old §3.3 said "blueprint CRUD never" alongside settings; a blueprint is a create-time preset, not a path — manage |
| `delete_blueprint` | 🔑 destroy | |
| `repo_assets/asset_status/branches/pull_refs/stashes/scripts/tools/site_info/site_jobs/job_state/watches/unmanaged/probe/check` | 🔑 read | `repo(action: …)` — `repo_check` is the zero-exec dep check; `probe` is `ls-remote` |
| `repo_add/adopt/link/git_op/run_step/run_offered_steps/script_job/dist_archive/watch_start/watch_stop/cancel` | 🔑 run | `repo(action: …)` + `job_*` — every one executes repo code or writes into the docroot; `repo_link` path blast-radius validated |
| `mcp_status`, `agent_activity` | ➕ T0 | `agent_activity(limit)` — "what did I do" is a fair question and is already the user's audit view |
| `mcp_set_enabled`, `mcp_set_mail_enabled`, `agent_db_*`, `agent_activity_clear` | 🚫 | self-granting / audit-erasing |
| `terminal_open/write/resize/close` | 🚫 | a PTY is a shell |

**Tally of the proposed surface**: 12 existing + ~30 new grouped tools ≈ **42**.
Flat would be ~150. The grouped shape is D12.

## 5. Milestones — each ships something a user can point at

Same house pattern as before: one task per commit, `verify.sh` green, ledger rows
in the same commit as their invariants, numbering continues after the ledger's
current last row (466 at the time of writing — **read the file, do not trust this
number**).

**P0 — leftovers that are wrong or unrun (§1).** L1 ✅ this commit. L2 = the owner's
packaged run, recorded in `TODO.md` under the existing gates row. L4 as time allows.
*Ships: a plan whose docs agree with its tree.*

**P1 — the foundation (nothing user-visible yet, everything load-bearing).**
1. `agent_site_grants` migration + store helpers, on the `agent_db_grants` shape
   (stored expiry on the DB's clock, revoke = timestamp, cascade on site delete, the
   same plant-proofs).
2. `core::agent_grants::{Scope, authorize, GrantRequests}` — pure, before anything
   opens; per-scope auto-allow with `destroy`/`system` refusing it structurally (no
   variant, not a check).
3. The `Granted<Scope>` witness — one constructor, reads BOTH rows, three compile-error
   bypasses captured like #208.
4. **Third registry** `mcp_server/user_sites.rs` + `UserSiteCtx` whose only door to a
   site is the witness. Dispatch decides capability by registry; the disjointness
   guard grows to three; the sweep and `tools/list` walk a LIST of registries, not
   two named ones (the "sweep walks BOTH" lesson, made shape-proof this time).
5. The sub-toggle (§3.3) + the Settings card: the prompt component generalised from
   `AgentDbGrants.tsx` (scope + site + client + the three answers), the grants list
   with scope badges and Revoke, per-scope auto-allow rows.
6. **The copy, written FIRST** (§6): enable-moment paragraph rewrite, the sub-toggle's
   three paragraphs, the five scope dialogs — each on the must-say list and guarded.
7. Ledger: re-scope the "no registered tool reaches `run_privileged`" row to the
   first two registries; amend #197's scope sentence; new rows for 1–4.
*Ships: a user can grant, see and revoke a scope; no tool uses one yet.*
**P5 SHIPPED 3 Sep 2026** (#489 blueprints / the feed / share status, #490 `repo`, #491
`valet_import` / `connection_rewrite` / `db_import`). Deviation from §4.4, recorded: `db_import
start` is `destroy`, not `run` — it drops and rebuilds the site's database behind
`confirm_overwrite`, which is the shape of a loss. Every module in §4's matrix now has its
tools: **45 tools** across the three registries. SMOKE §P5 is still to write (a real import
and a real rewrite under grants).
**P4 SHIPPED 3 Sep 2026** (#485 `stack_status`/`stack`, #486 `php`/`settings`/`tld`/`open` +
`settings_get`/`php_settings`, #487 `share`, #488 cap/TTL as settings). What moved: the
old "no tool reaches `run_privileged`" row never existed to re-scope — #485 is the narrower
true claim; `share`'s never-auto-allowed rule is checked on the grant ROW, not the claim
path, because an auto-written grant serves a later ordinary claim. SMOKE §P4 (36–39) is the
human leg. Owed from this phase: a `shares`/tunnel status read (`tunnels_status` needs the
`Tunnels` state, which `ReadCtx` does not hold) — a P5 item.
**P3 SHIPPED 3 Sep 2026** (#481 `wp_info`/`wp_plugin`/`wp_theme`, #482 `wp_user`/`wp_option`/
`wp_maintain`, #483 `wp_data`/`wp_network`/`site_wp_run`/`wp_org_search`, #484 `site_logs`/
`mail_inbox`). Two things moved: `site_wp_run` is a separate name from the scratch `wp_run`
because registries are disjoint by name; and every scope now has a stack-level meaning,
because the inbox is a stack-level `read`. Owed: an L1 against a real WordPress (`stack`
tier) — the vetted commands are the app's and proven as the app's; the sandbox L1
covers the gates over the socket and SMOKE §P3 (32–35) is the human leg.
**P2 SHIPPED 3 Sep 2026** (#475 `UserByAgent` + `site_create`, #476 `site_delete`, #477
`site_configure`, #478 `site_restart`/`site_retry`, #479 the widened view + `site_info` +
`site_inspect_folder`, #480 the L1). `job_status`/`job_cancel` moved to P6 with the
progress work they belong to. SMOKE §P2 (27–31) is the owner's run.
**P1 SHIPPED 3 Sep 2026** (#468 v43, #469/#470 the scope model, #471 the witness, #472
the third registry + `every_tool`, #473 the switch and commands, #474 the copy). What
moved from the list above: task 7's `run_privileged` re-scope is deferred to the first
tool that needs it (P4's `stack(start|stop)`) — re-scoping a row for a tool that does
not exist would be a claim about nothing; #197 IS amended. Tasks 5 and 6 landed as one
commit because two guards refuse a registered command or wrapper nothing calls.

**P2 — the site lifecycle (the brief's headline).** `site_create` (WP / PHP /
Laravel, blueprint, multisite, starter DB), `site_delete`, `site_configure` (all
actions), `site_restart`, `site_retry`, `site_info`, `site_inspect_folder`,
`job_status`/`job_cancel`, `list_sites` view widened. L1 `mcp_user_site_check`
(sandbox tier, fixture-owned; the USER's fixture site must be refused on every scope
it lacks and accepted on the one it holds). SMOKE §P2 with the tier-boundary HOLD
re-stated for grants ("ask it to delete a site you did not grant `destroy` on").
*Ships: "make me a Laravel site on PHP 8.3 behind FrankenPHP, then switch it to nginx."*

**P3 — WordPress on real sites + logs + inbox.** The eight `wp_*` grouped tools,
`wp_run`'s third-registry arm (D1 reversal, `run`), `tail_log` widened to every
source, `mail_inbox`. The wp-cli mail fix (#407) already makes `wp user create
--send-email` real. *Ships: the plugin-dev loop on the developer's ACTUAL site.*

**P4 — the stack.** `stack_status`, `stack(...)` incl. the privileged start/stop
(D10), `php_versions`/`php_install`/`php_settings`, `settings`, `tld`, `open`,
`share` (D11), TTL/cap as settings (L7). *Ships: "start rexenv's stack and put my
site on PHP 8.5."*

**P5 — the long tail.** `repo(...)`, `valet_import`, `connection_rewrite`,
`db_import`, `blueprints`, `wp_org_search`, `agent_activity`. *Ships: parity.*

**P6 — protocol work the surface now needs.** Tool annotations (L6); progress
notifications (`notifications/progress` when the client sent a `progressToken`,
else block as today) riding the CLI's streaming records (#447) — L9; the in-band
"rexenv stopped" error in `rex mcp` (L5); `job_*` as the one pattern for every job
registry; a real-client check that a 42-tool `tools/list` is accepted by Claude
Code AND Cursor (D12's evidence). *Ships: long calls that say what they are doing.*

**P7 — Laravel dev loop (L8).** `php_artisan(site, argv)` (scratch T0 / real `run`),
Composer path-repo link with S1's clone-vs-link reasoning RE-RUN for Composer (it
symlinks by default — the old plan said not to assume). The scratch Laravel
skeleton is subsumed by `site_create` + a scratch `type` param — decide whether a
disposable Laravel site is worth the provision shape (D13).

## 6. The copy — drafted first, because it constrained M2 and will constrain this

The guard on the enable-moment paragraph (#209/#211) fires the day the third
registry lands. The replacement is drafted here so a sentence that cannot be
honestly written exposes a wrong scope while changing it is cheap. Lands in the
Settings card near-verbatim, and the must-say list gains each bolded claim.

> **What an agent can do here depends on three switches, and every one is off
> until you turn it on.** With only the main switch on, an agent can **look** at
> your sites — status and debug logs — and **create disposable sites of its own**
> to run code in; it cannot change or delete anything you made. Turn on
> **"Let agents manage my own sites"** and it still cannot — until you grant it a
> specific permission, for a specific site, in the **Site access** section below.
> Each grant names what it allows (read, manage, destroy, run, or system-wide),
> expires on its own, and can be revoked here at any time; the ones that can lose
> your work never turn themselves on. **Anything that would ask macOS for an
> administrator password still asks you** — a grant is never a substitute for that
> dialog. Every call is recorded in the activity feed, whether it was allowed or
> refused.

Sub-toggle copy (three paragraphs, the mail toggle's shape): what it opens (the
Site access grants become possible), what it does NOT do on its own (nothing, until a
grant), and the honest residual (§3.1 of the old plan, unchanged: code an agent runs
in your site runs as you).

## 7. Decisions — D8–D13 SETTLED 3 Sep 2026 (owner: "go with the recommendations")

Each recommendation below is now the ruling. Recorded verbatim rather than rewritten,
so the reasoning that was in front of the owner is the reasoning on file.

| # | Question | Recommendation → RULING |
|---|---|---|
| D8 | Real-site raw `wp_run` at all? (D1 said the refusal is permanent) | **Yes, behind `run`.** The vetted eight cover the paved road; a plugin's own commands need the raw runner, and the grant is a knowing act |
| D9 | `destroy`: scoped 7-day grant like the rest, or per-call native dialog? | **Session grant** ("Allow for this session") as the default answer, no 7-day option for `destroy` |
| D10 | Stack start/stop as a tool, given it reaches `run_privileged`? | **Yes, under `system`**, with the macOS dialog stated as the second consent and the ledger row re-scoped. Alternative: keep §3.3 row 1 and ship parity minus start/stop |
| D11 | `share(start)`: scratch-only or any granted site? | **Any granted site, ≤ 60 min, never auto-allowed** — webhook testing is on real sites |
| D12 | Grouped tools with `action` (~42) vs flat (~150)? | **Grouped.** Prove with a real Cursor + Claude Code `tools/list` in P6 before P3 lands, since the risk is client-side |
| D13 | A scratch Laravel site (disposable, agent-owned)? | Defer — `site_create` + `destroy` grant gives an agent a Laravel site it can delete; build the skeleton only if the reaper/TTL semantics turn out to matter for Laravel |

## 8. Verification + ledger rows (the core set; every row says what it does NOT certify)

| Claim | Proof |
|---|---|
| A parity tool cannot reach a user's site without a `Granted<Scope>` witness, whose one constructor reads the grant AND the site row | structural (compile) + L0 constructor-scope test, #208's shape |
| `authorize` is pure over recorded facts and runs before anything opens; a refusal never touches the engine, the docroot or wp-cli | L0 + a source guard that the tool handlers call no `core::`/`commands::` fn before `authorize` |
| `destroy` and `system` cannot be auto-allowed — no variant exists | structural + L0 |
| Three registries are disjoint; the sweep and `tools/list` walk a list, and a fourth registry added without joining the list fails a guard | L0 plant-proven |
| The enable-moment and sub-toggle copy are held to the registry (the #209 guard, re-scoped) | L0 `include_str!` guard |
| No parity tool returns an env VALUE, a default credential, or a login URL into the feed | the existing planted-secret sweep, with new plants (an env value, `wp_default_creds` output) |
| `site_create` under a non-`system` grant never reaches `configure_resolver` — `ResolverPrompt::Never` holds for the WHOLE provision (the #210 shape, second registry) | L0 end-to-end with the resolver absent |
| A parity tool on a scratch site is refused, and no parity tool calls `promote_if_scratch` on a scratch row | L0 + the #214 source guard extended |
| L1 `mcp_user_site_check` (sandbox): every scope refused without a grant, accepted with one, revoked mid-session and refused again | example, fixture-owned per `examples/common/mod.rs` |
| Human: SMOKE §P2 — the grant boundary in front of a model that wants to help | the §M2a step-8 HOLD, restated for grants |

**What none of this certifies, restated so it cannot be read otherwise:** a grant
bounds *which site and which verb*; code the agent runs inside a granted site runs
as the user (#197). Parity widens the paved road; it does not build a sandbox, and
the copy says so.
