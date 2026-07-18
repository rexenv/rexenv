# rex CLI roadmap

The pick-list for growing the `rex` CLI. Work through it one item at a time;
tick items here with ✓ evidence (same convention as TODO.md).

**The CLI principle (from v1, non-negotiable):** `rex` is remote-control ONLY —
every command is one request over the app's private socket, dispatched in
`src-tauri/src/cli_server.rs` to the SAME `commands::*` fn the UI calls. Never
a parallel code path, never new backend unless flagged here. `cli/` never
links the app lib.

**Legend** — per-item cost tag:
- 🟢 **IPC exists** — cheap win: one dispatch arm + one CLI subcommand + output formatting.
- 🟡 **composite** — new dispatch arm composing EXISTING core/commands fns; no new core logic.
- 🔴 **new backend** — needs new core/IPC work first; design before building.
- ⚪ **CLI-only** — no socket round-trip needed (or a trivial one); lives in `cli/` alone.

Destructive commands take a confirm prompt + `--yes` (the `site delete`
convention). Long-running commands hold the connection (the `site create`
convention) — see "Infrastructure" for progress streaming.

## Shipped (v1 — verified against `cli/src/main.rs` + `cli_server.rs` dispatch)

| Command | Backing IPC |
|---|---|
| `rex status` (`--json` global) | `services_status` + `dns_status` |
| `rex start` / `rex stop` / `rex restart` | `start_services` / `stop_services` (restart = both) |
| `rex site list` | `list_sites` + `sites_serving` |
| `rex site create <domain> [--name --type --php --server --db]` | `create_site` |
| `rex site delete <domain> [--yes]` | `delete_site` (domain→id lookup client-side) |
| `rex site info <domain>` | `list_sites`+`sites_serving`+`sites_resources`+`site_cert_info`+`wp_info` |
| `rex site open <domain>` | CLI-only (`open https://…`, domain validated via `site.list`) |
| `rex site login <domain> [--print]` | `wp_admin_login_url` |
| `rex help`, exit codes 0/1/2 | — |

## Sites

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `site create --multisite subdomain\|subdirectory` | `wp_multisite_convert` after `create_site` | ✓ | shipped 16 Jul — full live run: create → multisite subdirectory in info → 200 → deleted |
| `site create --blueprint <name>` | `list_blueprints` (name→id) + `create_site(blueprint_id)` | ✓ | shipped 16 Jul — miss errors naming saved blueprints; `rex blueprints` lists them |
| `site info <domain>` | `list_sites` + `sites_serving` + `sites_resources` + `site_cert_info` (+ `wp_info` for WP) | ✓ | shipped 16 Jul — live-verified on tr.rex (WP, real core version) + gpt.rex (php/FrankenPHP resources) |
| `site open <domain>` | — (`open https://<domain>`) | ✓ | shipped 16 Jul — domain validated via `site.list`; missing-domain exit 1 |
| `site login <domain> [--print]` | `wp_admin_login_url` | ✓ | shipped 16 Jul — minted link curl-verified: 302 → /wp-admin/; non-WP site refused |
| `site logs <domain> [--source K] [--lines N] [--follow]` | `log_targets` + `tail_log` (+ `wp_debug_log_tail` via the `wp-debug` pseudo-source) | ✓ | shipped 16 Jul — sources list, tails, --follow caught a live request; wp-debug reads the docroot debug.log |
| `site rename <domain> <name>` | `rename_site` | ✓ | shipped 16 Jul — round-trip live |
| `site domain <domain> <new-domain> [--yes]` | `change_site_domain` | ✓ | shipped 16 Jul — confirm-gated; passthrough, live-verify against the running app (guard blocks override bounce in the harness) |
| `site move <domain> <dest-parent>` | `move_site_docroot` | ✓ | shipped 16 Jul — passthrough (preflights backend-side); verify in-app once |
| `site php <domain> <minor>` | `set_site_php_version` | ✓ | shipped 16 Jul — 8.3→8.4→8.3 live, 200 both ways |
| `site server <domain> nginx\|frankenphp\|apache` | `set_site_web_server` | ✓ | shipped 16 Jul — passthrough; verify against the running app (override stop is guard-blocked in the harness) |
| `site xdebug <domain> on\|off` | `set_site_xdebug` | ✓ | shipped 16 Jul — on→200→off live; FrankenPHP refusal verbatim, exit 1 |
| `site env <domain> [set K=V \| unset K]` | `list_site_env` / `set_site_env` | ✓ | shipped 16 Jul — set→list→unset live (client-side merge; backend replaces the set) |
| `site cert <domain> [--regenerate]` | `site_cert_info` / `regenerate_site_cert` | ✓ | shipped 16 Jul — info live (SANs, days left) |
| `site restart <domain>` (single-site backend bounce) | — | 🔴 | no single-site restart IPC (UI doesn't have it either); needs a manager seam |

## PHP

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `php list` | `list_php_versions` | ✓ | shipped 16 Jul — live (6 pinned minors) |
| `php default <minor>` | `set_default_php_version` | ✓ | shipped 16 Jul — flip + restore live |
| `php install <minor>` / `php uninstall <minor>` | `set_php_version_installed` | ✓ | shipped 16 Jul — thin passthrough, NOT live-run (pool stop is meaningless under the example guard); verify in-app once |
| `php settings <minor> [set K=V]` | `get_php_settings` / `apply_php_settings` | ✓ | shipped 16 Jul — read live (real ini values); set is a passthrough (pool restart guard-blocked in harness) |

## Services

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `service start\|stop <engine>` | `start_database` / `stop_database` | ✓ | shipped 16 Jul — postgres cycled idle→running→idle live |
| `service start\|stop mailpit` | `start_mail` / `stop_mail` | ✓ | shipped 16 Jul |
| `service start\|stop nginx\|caddy\|php-<minor>` | — | 🔴 | web tier has no single-service IPC (deliberate — topology invariants); design first |
| `logs [key] [--lines N] [--follow]` | `tail_log` + a `logs.list` dir-listing arm | ✓ | shipped 16 Jul — no key lists every log file with sizes |

## Database

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `db export <domain>` | `wp_db_export` | ✓ | shipped 16 Jul — always ~/Downloads (the IPC's contract; a custom path would be new backend); MySQL + MariaDB both live-verified |
| `db import <domain> <file.sql> [--yes]` | `wp_db_import` | ✓ | shipped 16 Jul — confirm + backup-first tip; path canonicalized client-side; round-trip live-verified (site 200 after) |
| `db reset <domain>` | `wp_site_reset` | ✓ | shipped 16 Jul — TYPED domain confirmation (--confirm <domain> for scripts); wrong-confirm abort live |
| `db versions [--set <engine> <version>]` | `databases_status` + `db_engine_versions` / `set_db_engine_version` | ✓ | shipped 16 Jul — matrix live; --set passthrough |
| `db browse` | — (`open https://adminer.rexenv.rex`) | ✓ | shipped 16 Jul |

## WordPress

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `wp <domain> plugin list\|install\|activate\|deactivate\|update\|delete` | `wp_plugins` / `wp_plugin_*` | ✓ | shipped 16 Jul — full install→delete cycle live, zero residue; network variants still unmapped |
| `wp <domain> theme list\|install\|activate\|update\|delete` | `wp_themes` / `wp_theme_*` | ✓ | shipped 16 Jul |
| `wp <domain> user list\|create\|set-password\|set-role` | `wp_users` / `wp_user_*` | ✓ | shipped 16 Jul — passwords generated (urandom) + printed once, never argv; login-or-id accepted. NOTE: no `wp_user_delete` IPC exists — a CLI user delete would be new backend |
| `wp <domain> search-replace <from> <to> [--dry-run] [--yes]` | `wp_search_replace` | ✓ | shipped 16 Jul — dry_run exposed; live dry-run verified |
| `wp <domain> cache-flush` / `cron run` | `wp_cache_flush` / `wp_cron_run_due` | ✓ | shipped 16 Jul — 18 due events executed live |
| `wp <domain> core update\|versions\|switch <v>` | `wp_core_update/versions/switch_version` | ✓ | shipped 16 Jul — versions live (wp.org list); update/switch passthroughs (long, not live-run) |
| `wp <domain> maintenance [on\|off]` | `wp_maintenance_get/set` | ✓ | shipped 16 Jul — wire-proven (503 during, 200 after) |
| `rex wp <domain> -- <raw wp-cli args>` (passthrough) | — | 🔴 | no generic-exec IPC (deliberate: every WP op is a vetted command); a raw passthrough is a security/design decision, not a gap-fill |

## Mail

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `mail list` | `mailpit_messages` | ✓ | shipped 16 Jul — 3 real caught messages listed |
| `mail clear [--yes]` | `mailpit_clear` | ✓ | shipped 16 Jul (confirm-gated; not live-run — user mail) |
| `mail open` | `mailpit_status` (uiUrl) + local `open` | ✓ | shipped 16 Jul |

## Tunnels

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `tunnel list` | `tunnels_status` | ✓ | shipped 16 Jul |
| `tunnel start\|stop <domain>` | `start_tunnel` / `stop_tunnel` | ✓ | shipped 16 Jul — start confirm-gated, prints public URL; start not live-run (public exposure) |

## TLD & config

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `tld [--set <tld>]` | `default_tld` / `set_default_tld` | ✓ | shipped 16 Jul — policy errors stay backend-side |
| `config get\|set <key> [value]` | `get_setting` / `set_setting` | 🟡 | raw KV — allow-list the keys the UI exposes, don't open the whole table |

## Misc

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `rex version` | `app_info` + CLI's own version | ✓ | shipped 16 Jul |
| `rex doctor` | composite: `dns_status` + `services_status` + `edge_answers_as_ours` + `default_ports` scan + `cli_status` | ✓ | shipped 16 Jul — exit 0/1 (CI-gateable); synthetic foreign listener flagged with attributed holder + copyable fix |
| `rex completions zsh\|bash` | — | ✓ | shipped 16 Jul — static tree, both syntax-checked |

## Repo group (git/asset feature-set — wave 1 SHIPPED 18 Jul 2026; wave 2 = the 🟡 job-shaped rows)

Every backing IPC below shipped with the add-from-Git/asset phases
(`commands/repo.rs`) — the whole group is dispatch arms + subcommands, no
new core. **The delete guard needs no CLI work at all:** `rex wp <domain>
plugin|theme delete` already dispatches to `wp_plugin_delete`/
`wp_theme_delete` (cli_server.rs), which carry the phase-D unlink-only
symlink interception — the CLI cannot bypass it BY CONSTRUCTION (one code
path). What a `repo delete` alias would add is only the loss-warning
preview in the confirm (the UI fetches `repo_asset_status` first).

Job-shaped ops (add / fetch / pull / checkout / push / run) return a job id
and stream via Tauri events, which the CLI socket doesn't carry — the 🟡
arms below poll `repo_job_state` until terminal, then print the job's flat
log via the existing `logs.tail` (`log_key` is in every snapshot). LIVE
line streaming is the same 🔴 "progress streaming" infra item as always.

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `repo status <domain> <dir> [--theme]` | `repo_asset_status` | ✓ | shipped 18 Jul (wave 1) — live via `examples/cli_repo_check`: clean→dirty flip read back |
| `repo list <domain> [--status]` | `repo_assets` (+ per-row `repo_asset_status`) | ✓ | shipped 18 Jul — per-row status errors never abort the list |
| `repo adopt <domain> <dir> [--theme]` | `repo_adopt` | ✓ | shipped 18 Jul — prints branch/remote summary after |
| `repo link <domain> <path> [--name --theme]` | `repo_link` | ✓ | shipped 18 Jul — path canonicalized client-side; symlink verified on disk in the live check |
| `repo branches <domain> <dir>` | `repo_branches` | ✓ | shipped 18 Jul — current marked `*` |
| `repo add <domain> <url> [--theme --branch --name]` | `repo_probe` + `repo_add` + poll `repo_job_state` + `logs.tail` | 🟡 | hold the connection (site-create convention); print step transitions + log tail on finish |
| `repo fetch\|pull\|checkout\|push <domain> <dir> [ref]` | `repo_git_op` + poll + `logs.tail` | 🟡 | same job-poll shape; checkout requires ref |
| `repo run <domain> <dir> <script>` | `repo_scripts` (validate) + `repo_script_job` + poll + `logs.tail` | 🟡 | one-shot scripts only |
| `repo install <domain> <dir> [composer\|install\|build]` | `repo_run_step` on the latest job / or a fresh op job | 🟡 | needs a live job with offered steps — or fold into `repo add --install` (run offered steps after clone, still explicit via the flag) |
| `repo watch start\|stop\|list <domain> [dir] [script]` | `repo_watch_start` / `repo_watch_stop` (stop resolves the id BY DIR via `repo_watches`) | ✓ | shipped 18 Jul — start prints the runs-inside-the-app/stops-on-quit note; live check proved start→list→stop with zero orphans; `--tail` stays 🔴 streaming |
| `repo delete <domain> <dir> [--theme --yes]` | `repo_asset_status` (loss preview) + `wp_plugin_delete`/`wp_theme_delete` | 🟡 | alias over the ALREADY-GUARDED delete + the UI's status-driven confirm text; `--yes` prints the warning anyway |
| `repo tools [--refresh]` | `repo_tools` | ✓ | shipped 18 Jul — plus a fixed line stating composer is the bundled phar |

## Infrastructure (enables the above, not user commands)

- **Progress streaming** 🔴 — long ops (`site create`, `db import`, `wp core
  update`) currently hold the connection silently; stream progress lines over
  the same socket (multi-line response before the final envelope). Design
  once, benefits everything.
- **Protocol version handshake** 🟡 — v1 already errors on unknown cmds both
  directions; add an explicit version field when the surface grows.
- **`--json` everywhere** — v1 rule, keep it: every new command returns the
  raw IPC payload under `--json`.

## Status (16 Jul 2026) — cheap tier COMPLETE

42 commands shipped (every 🟢/🟡/⚪ except `config get/set`). What remains:

1. `config get|set` (🟡) — parked on a decision: which settings keys to
   allow-list (never the whole KV table).
2. Design-first 🔴 set: single-site restart (manager seam), web-tier
   single-service control (topology invariant), raw wp passthrough (security
   decision), `wp_user_delete` (no IPC exists), progress streaming for long ops.
3. **In-app verifies owed** (passthroughs whose restart/exposure half is
   guard-blocked in the example harness — exercise each once against the
   running app): `php install/uninstall`, `php settings set`, `db versions
   --set`, `site server/domain/move`, `mail clear`, `tunnel start`,
   `wp core update/switch`.
