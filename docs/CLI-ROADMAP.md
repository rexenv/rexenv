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
| `rex open` | `app.open` — brings the app window to the front |
| `rex help`, exit codes 0/1/2 | — |

**When the app isn't running**, every command exits 2 with the reason AND the command
to fix it (`open -a rexenv`, macOS) — an OFFER, never an autostart. `rex` is a remote
control for a RUNNING app, so spawning one itself would start a second process behind
the user's back — adopting services, opening the database as a second writer, taking
over both sockets — as a side effect of `rex status` inside a shell script. The line
says "the installed app" because `open -a` resolves through LaunchServices, which does
not know about a dev build run out of `target/`.

**`open -a rexenv` is ambiguous whenever two copies of the bundle exist**, and this bit
on 1 Sep 2026: with a freshly built `target/release/bundle/macos/rexenv.app` on disk and
the same build installed in `/Applications`, `open -a rexenv` launched the one under
`target/` — LaunchServices resolves by bundle id, and the copy it knows most recently can
win. Nothing warns; the app simply comes up, and whichever copy won is the one that
owns the stack. The hint is still right for users (one copy) and still the honest answer;
the fix on a DEVELOPER's machine is to delete the built bundle after installing it, and
to launch a specific copy with `open /Applications/rexenv.app` when it matters. Guarded by
`the_cli_names_the_start_command_and_never_runs_it` (text-level, and it says so).

## Sites

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `site create --multisite subdomain\|subdirectory` | `wp_multisite_convert` after `create_site` | ✓ | shipped 16 Jul — full live run: create → multisite subdirectory in info → 200 → deleted |
| `site create --starter-db` (Blank PHP) | `create_site` (`NewSite.starter_db`) | ✓ | **shipped 3 Sep 2026** (ledger #462) — the dialog's Database field for a Blank-PHP site: creates the database, seeds `starter_items`, writes `db.php`. **The work was the REFUSAL, not the flag.** `sites::create` records the field through a `.then_some` that drops it everywhere else in silence, so asking on a WordPress site or a `--path` link would have produced an ordinary site with no error and nothing to say why; `sites::starter_db_refusal` now owns that rule beside the `.then_some` and the arm reads it. Reported from the row the app wrote back, never from the flag we sent |
| `site create --blueprint <name>` | `list_blueprints` (name→id) + `create_site(blueprint_id)` | ✓ | shipped 16 Jul — miss errors naming saved blueprints; `rex blueprints` lists them |
| `site info <domain>` | `list_sites` + `sites_serving` + `sites_resources` + `site_cert_info` (+ `wp_info` for WP) | ✓ | shipped 16 Jul — live-verified on a real WP site (real core version) + a FrankenPHP php site (resources) |
| `site open <domain>` | — (`open https://<domain>`) | ✓ | shipped 16 Jul — domain validated via `site.list`; missing-domain exit 1 |
| `site login <domain> [--print]` | `wp_admin_login_url` | ✓ | shipped 16 Jul — minted link curl-verified: 302 → /wp-admin/; non-WP site refused |
| `site logs <domain> [--source K] [--lines N] [--follow]` | `log_targets` + `tail_log` (+ `wp_debug_log_tail` via the `wp-debug` pseudo-source) | ✓ | shipped 16 Jul — sources list, tails, --follow caught a live request; wp-debug reads the docroot debug.log |
| `site rename <domain> <name>` | `rename_site` | ✓ | shipped 16 Jul — round-trip live |
| `site domain <domain> <new-domain> [--yes]` | `change_site_domain` | ✓ | shipped 16 Jul — confirm-gated; passthrough, live-verify against the running app (guard blocks override bounce in the harness) |
| `site move <domain> <dest-parent>` | `move_site_docroot` | ✓ | shipped 16 Jul — passthrough (preflights backend-side); verify in-app once |
| `site relink <domain> <path>` | `relink_site_docroot` | ✓ | **shipped + live-verified 23 Aug 2026** — the re-point path for a linked/imported folder the USER moved (`site move` refuses those); records the path + reloads, touches no file. The CLI canonicalises the path before sending, so a relative one means what the user's cwd says, not the app's — verified from `/private/tmp` with `./first`. Live proof is what the site SERVED, which changed with the re-point while the old docroot stayed byte-identical |
| `site php <domain> <minor>` | `set_site_php_version` | ✓ | shipped 16 Jul — 8.3→8.4→8.3 live, 200 both ways |
| `site server <domain> nginx\|frankenphp\|apache` | `set_site_web_server` | ✓ | shipped 16 Jul — passthrough; verify against the running app (override stop is guard-blocked in the harness) |
| `site xdebug <domain> on\|off` | `set_site_xdebug` | ✓ | shipped 16 Jul — on→200→off live; FrankenPHP refusal verbatim, exit 1 |
| `site env <domain> [set K=V \| unset K]` | `list_site_env` / `set_site_env` | ✓ | shipped 16 Jul — set→list→unset live (client-side merge; backend replaces the set) |
| `site cert <domain> [--regenerate]` | `site_cert_info` / `regenerate_site_cert` | ✓ | shipped 16 Jul — info live (SANs, days left) |
| `tld --remove <tld>` | `remove_resolver` | ✓ | **shipped 3 Sep 2026** (ledger #457). Takes OUR resolver file for a TLD no site answers on back out, behind the same privileged prompt that installed it; refuses a foreign file, an in-use TLD, and the backbone. A verb because nothing removes one automatically — a site delete must not prompt for a password. Not live-run |
| `site domains <domain> [--add N \| --remove N]` | `site_domains` / `add_site_domain` / `remove_site_domain` | ✓ | **shipped 2 Sep 2026** (ledger #450/#451, the last Valet compatibility tail). One verb for read and write, because it is one question — which names does this site answer on — and every reply is the whole list with the primary first. Adding SERVES it: the configs are rebuilt and the web tier reloaded in the same call, since a stored-but-unserved alias is the UI and the browser disagreeing. Refusals name the site a hostname already belongs to. **Live 3 Sep 2026** — see the in-app-verifies list below, which is where live-run status is recorded. **3 Sep 2026**: every `<domain>` argument is normalised (trailing dot, case) before lookup, as the app normalises on the way in; `rex site list` gained an ALSO column with the extra names, since `find_site` accepts them and its error sends people to the list; `rex site delete <alias>` stays a refusal, but now names the owning site and both ways out (delete by primary, or `--remove` the name); `rex doctor`'s TLD row prints one `tld --repair` per ABSENT resolver and says of a FOREIGN one that another tool answers there — `--repair` refuses a foreign file by design, so prescribing it there was a circle |
| `site restart <domain> [--pool]` | `restart_site` | ✓ | **shipped 2 Sep 2026.** The design ruling this row was waiting for is that "restart this site" has NO single meaning here: the default topology gives a site no process of its own (shared nginx → one php-fpm pool per PHP MINOR), so only an override site (FrankenPHP/Apache on its loopback port) has something to bounce. Three honest outcomes, and the reply says which: `backend` (stopped + respawned on the site's RECORDED port, so the edge route still points at it), `shared` (config rebuilt + nginx/edge reloaded — what actually makes a default site pick up a change), `refused` (an ADOPTED backend a non-app process may not stop — reported and exit 1, never a silent no-op). **The pool bounce is opt-in** (`--pool`) because it stops every site on that minor; the report carries `sitesOnPool` either way, so the number is on screen before the flag is used and after. New manager seam `restart_site_backend` — spawn under the lock, `await_ready` after dropping it. **The default-site outcome ran live 3 Sep 2026**; the other two are still owed. Status in the in-app-verifies list below |
| `site start <domain> [--all]` / `site stop <domain> [--all]` | `set_site_enabled` / `set_all_sites_enabled` | ✓ | **shipped 4 Sep 2026** (v44, `docs/PLAN-per-site-lifecycle.md`). Stopping ONE site is a change to the SERVING SURFACE, not to any process: the site's serving nginx block is replaced by a STOPPED one (503 + the stop page — a name with no block falls through to nginx's default server, #511) and its Caddy route keeps its certificate but answers 503, while the shared web server and the php-fpm pool — shared by every site on that PHP minor — keep running for everyone else. The only process this stops is the site's OWN override backend (FrankenPHP/Apache), which serves that site and nothing else. `start` ensures the site's pool if the stack is up; it never starts the stack, and when the stack is down the reply carries the reason instead of a cheerful "started". The verbs are `start`/`stop` — the app's own words — with the output saying every time that this is not `rex stop`, because the two are one argument apart and the mistake is otherwise silent. `--all` is the same switch over every site (one rebuild, one reload; half-provisioned sites skipped and counted). Not live-run |
| `site retry <domain>` (finish a "setup incomplete" half-provision) | `site_provision_retry` (domain→id client-side) | ✓ | **shipped 24 Aug 2026.** Polls SERVER-side so the CLI stays request/reply, like `site.create`, which already blocks for a whole provision — a second protocol for the same user-visible operation would be two things to keep in step. Bounded at 15 min so a wedged provision cannot hold the socket forever; the caller gets the last snapshot and the log key it names. ⚠ **The "`site.create` failures … point here" half of this row is UNVERIFIED** — nothing in the tree matches a message naming this command, and reproducing a mid-provision failure to read the text has not been done. Left as this file's claim rather than repeated into a code comment. ✅ **in-app verified 24 Aug 2026**: a fully provisioned site refused, a half-site (`provisioned = 0`) retried to completion with the flag flipping back, and the linked docroot's own `index.php` byte-identical afterwards — the never-clobber guarantee on the case where clobbering would destroy a folder rexenv does not own |

## PHP

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `php list` | `list_php_versions` | ✓ | shipped 16 Jul — live. (Listed "6 pinned minors" until 7.4 shipped 15 Aug 2026; the count is `binaries::PHP_VERSIONS`, so don't write it down again.) |
| `php default <minor>` | `set_default_php_version` | ✓ | shipped 16 Jul — flip + restore live |
| `php install <minor>` / `php uninstall <minor>` | `set_php_version_installed` | ✓ | shipped 16 Jul — thin passthrough, NOT live-run (pool stop is meaningless under the example guard); verify in-app once |
| `php settings <minor> [set K=V]` | `get_php_settings` / `apply_php_settings` | ✓ | shipped 16 Jul — read live (real ini values); set is a passthrough (pool restart guard-blocked in harness) |

## Services

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `service start\|stop <engine>` | `start_database` / `stop_database` | ✓ | shipped 16 Jul — postgres cycled idle→running→idle live |
| `service start\|stop mailpit` | `start_mail` / `stop_mail` | ✓ | shipped 16 Jul |
| `service restart nginx\|edge\|php-<minor>` | `restart_web_service` | ✓ | **shipped 2 Sep 2026** (ledger #445). The design this row asked for, and the answer is that the verb is RESTART only: the web tier has no useful stopped state — a stopped nginx is every default site 502-ing with nothing on screen to say why — so `start`/`stop` on a web-tier name is REFUSED BY NAME, pointing at `rex service restart` or `rex start`/`rex stop`. Always respawns on a FRESHLY generated config (resurrecting a service on the config it already had is the state a restart is trying to escape). The edge is RELOADED, not restarted, and says so: it is a root KeepAlive daemon whose stop is a privileged `disable` + `bootout` with every site offline at :443 in between, and the live config is what anyone asking for a restart wanted. A service that is not running is REPORTED, never started — `start_all` owns the ORDER (pools → nginx → edge) and a lone service started out of order is a stack that half works. Adopted nginx + a non-app process = refused, like every other stack-guard path. **Live 3 Sep 2026** — see the in-app-verifies list below |
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
| `wp <domain> user list\|create\|set-password\|set-role\|delete` | `wp_users` / `wp_user_*` | ✓ | shipped 16 Jul — passwords generated (urandom) + printed once, never argv; login-or-id accepted. **`user delete` shipped 2 Sep 2026** (ledger #446), the new backend that NOTE described: `wp user delete` also decides what happens to the account's POSTS, and wp-cli's default is to delete them, so the choice is a required argument (`--reassign <login|id>` or `--delete-posts`) and both the CLI and the IPC refuse a caller who gave neither or both. Primary administrator refused (the account one-click login and rexenv's tools resolve to, and unlike a role change it cannot be undone); self-reassignment refused (it is `--delete-posts` under the opposite name); multisite refused outright, because there `wp user delete` removes them from THIS site while the network account survives — "deleted" would be false. **Live 3 Sep 2026 — and the run found the verb UNCOMPLETABLE** (ledger #464); see the in-app-verifies list below |
| `wp <domain> search-replace <from> <to> [--dry-run] [--yes]` | `wp_search_replace` | ✓ | shipped 16 Jul — dry_run exposed; live dry-run verified |
| `wp <domain> cache-flush` / `cron run` | `wp_cache_flush` / `wp_cron_run_due` | ✓ | shipped 16 Jul — 18 due events executed live |
| `wp <domain> core update\|versions\|switch <v>` | `wp_core_update/versions/switch_version` | ✓ | shipped 16 Jul — versions live (wp.org list); update/switch passthroughs (long, not live-run) |
| `wp <domain> maintenance [on\|off]` | `wp_maintenance_get/set` | ✓ | shipped 16 Jul — wire-proven (503 during, 200 after) |
| `rex wp <domain> -- <raw wp-cli args>` (passthrough) | — | 🔴 | no generic-exec IPC (deliberate: every WP op is a vetted command); a raw passthrough is a security/design decision, not a gap-fill |

## Mail

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `mail list [--unread] [query]` | `mailpit_messages` | ✓ | shipped 16 Jul — 3 real caught messages listed. The socket command also takes `query` and `unread` (bool, absent = whole inbox, so an older `rex` against a newer app is unchanged), and **both reached the CLI on 23 Aug 2026** (ledger #385) — this row said they had not for ten days, while the Infrastructure section below recorded the same day they did |
| `mail mark-read` | `mailpit_mark_all_read` | ✓ | socket command `mail.mark_read` landed 14 Aug 2026 with the Mail screen's "Mark all read"; **`rex mail mark-read` shipped 23 Aug 2026** (ledger #385) — named `mark-read`, not `read`, because it marks EVERY message. The tag on this row still read "IPC exists" ten days later |
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
| `tld [--set <tld>] [--repair <tld>]` | `default_tld` / `set_default_tld` / `repair_resolver` | ✓ | shipped 16 Jul — policy errors stay backend-side. **`--repair` added 3 Sep 2026**: puts back the OS resolver file for a TLD your sites answer on, which is the fix `rex doctor` names when it finds one missing (#457). A SEPARATE flag from `--set` on purpose — that one decides what new sites are called, and conflating them would make "change my default" quietly write a root-owned file. Scoped to TLDs IN USE: `ensure_resolver` writes under `/etc/resolver` behind a privileged prompt, and a verb that took any string would be a way to point arbitrary TLDs at this machine's resolver |
| `config get\|set <key> [value]` | `get_setting` / `set_setting` | ✓ | **shipped 24 Aug 2026** (ledger #397). NOT raw KV: `core::settings_access` rules per key and DENIES by default. The list lives in core because the L0 guard reads it too — a security boundary with two copies is the defect this tree keeps finding. Writes go through `set_setting`, so a validated key still gets its setter |

## Misc

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `rex version` | `app_info` + CLI's own version | ✓ | shipped 16 Jul |
| `rex doctor` | composite: `dns_status` + `services_status` + `edge_answers_as_ours` + `default_ports` scan + `cli_status` + `resolver_drift` | ✓ | shipped 16 Jul — exit 0/1 (CI-gateable); synthetic foreign listener flagged with attributed holder + copyable fix. **13 Aug 2026: the `Resolvers` line.** `resolverDrift` had been in the payload since the beginning and was rendered by nothing, so a TLD reclaimed by Valet or Herd — sites dark while every other line reads ✓ — was invisible here. Now a FINDING (counts toward the exit code), naming the TLDs and pointing at rexenv → Import, which is the only place a takeover can be redone. An ABSENT field (an older app) reads as ⚠ unknown, never ✓ |
| `rex completions zsh\|bash` | — | ✓ | shipped 16 Jul — static tree, both syntax-checked |

## Repo group (git/asset feature-set — waves 1+2 SHIPPED 18 Jul 2026; `watch --tail` closed 3 Sep 2026, so the group is COMPLETE)

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
line streaming was the same "progress streaming" infra item — which SHIPPED 2 Sep 2026
(ledger #447), and `repo watch --tail` followed on 3 Sep 2026 (ledger #467). It did not need
the streaming protocol in the end: a watcher already writes a log file, so the CLI follows
THAT through the existing `logs.tail`, and the only new thing the app had to say was WHICH
file — carried in the watcher snapshot as `logKey`, never rebuilt by the caller.

| Command | Backing IPC | Tag | Notes |
|---|---|---|---|
| `repo status <domain> <dir> [--theme]` | `repo_asset_status` | ✓ | shipped 18 Jul (wave 1) — live via `examples/cli_repo_check`: clean→dirty flip read back |
| `repo list <domain> [--status]` | `repo_assets` (+ per-row `repo_asset_status`) | ✓ | shipped 18 Jul — per-row status errors never abort the list |
| `repo adopt <domain> <dir> [--theme]` | `repo_adopt` | ✓ | shipped 18 Jul — prints branch/remote summary after |
| `repo link <domain> <path> [--name --theme]` | `repo_link` | ✓ | shipped 18 Jul — path canonicalized client-side; symlink verified on disk in the live check |
| `repo branches <domain> <dir>` | `repo_branches` | ✓ | shipped 18 Jul — current marked `*`; 23 Jul: also prints a `tags:` section (newest first) |
| `repo prs <domain> <dir>` | `repo_pull_refs` | ✓ | shipped 23 Jul — PR/MR head refs via ls-remote (refs-only, no host API/token); checkout of a listed ref lands detached |
| `repo check <domain> <dir>` | `repo_check` | ✓ | shipped 23 Jul — zero-exec dep check (presence + stored-fingerprint staleness); offers steps, runs nothing; `--install` chaining lands with the Run-all stage |
| `repo add <domain> <url> [--branch --name --theme --install]` | `repo_add` + poll `repo_job_state` + `logs.tail`; `--install` chains offered steps via `repo_run_step` | ✓ | shipped 18 Jul (wave 2) — live: cloned octocat/Hello-World through dispatch, detect ok, dir on disk |
| `repo fetch\|pull\|checkout\|push <domain> <dir> [ref] [--install]` | `repo_git_op` + poll + `logs.tail` (+ offered-step chain on `--install`) | ✓ | shipped 18 Jul — live vs a LOCAL bare origin: pull file arrived, push seen at origin, checkout landed on feat |
| `repo run <domain> <dir> <script>` | `repo_script_job` + poll + `logs.tail` (script validated backend-side) | ✓ | shipped 18 Jul — live: one-shot output present in the completion log |
| `repo install …` | — | ✓ | resolved as the `--install` flag on `add`/`pull`/`checkout` (explicit consent on the command line; offered steps chain via `repo_run_step`, first failure stops the chain) — no standalone command needed |
| `repo watch start\|stop\|list\|tail <domain> [dir] [script]` | `repo_watch_start` / `repo_watch_stop` (stop resolves the id BY DIR via `repo_watches`) + `logs.tail` on the snapshot's `logKey` | ✓ | shipped 18 Jul — start prints the runs-inside-the-app/stops-on-quit note; live check proved start→list→stop with zero orphans. **`--tail` shipped 3 Sep 2026** (ledger #467): `watch start … --tail` follows the output, and `watch tail <dir>` picks up a watcher already running (the common case — started in the app, watched from the terminal). Ctrl-C stops FOLLOWING, not the watcher, and the message says so because the opposite is what a reader assumes |
| `repo delete <domain> <dir> [--theme --yes]` | `repo_asset_status` (loss/linked preview, printed even with `--yes`) → the EXISTING `wp.plugin.delete`/`wp.theme.delete` arms | ✓ | shipped 18 Jul — pure client composition, zero new delete path; guard proven through dispatch (link gone, target byte-intact) |
| `repo tools [--refresh]` | `repo_tools` | ✓ | shipped 18 Jul — plus a fixed line stating composer is the bundled phar |

## Infrastructure (enables the above, not user commands)

- ~~**Progress streaming** 🔴~~ — **SHIPPED 2 Sep 2026** (ledger #447). Opt-in `stream: true`,
  `{"progress": …}` records keyed so an older reader can skip them, and exactly one `{"ok": …}`
  envelope always last; `site create` and `site retry` print their phases live. Long ops used to
  hold the connection silently, which is indistinguishable from a hang.
- ~~**Protocol version handshake** 🟡~~ — **answered differently, 23 Aug 2026**
  (ledger #386). A protocol integer answers "is the wire contract compatible",
  which is not the question anyone has: `rex` and the app ship in the SAME cask
  at the same version, so a difference is never a negotiation — it is a stale
  build, and naming which side is stale is the whole fix. Worse, a protocol
  number would stay SILENT for the case that actually keeps happening, because
  adding a command is backward-compatible and would never bump it.
  `rex` now compares its own version against the app's and says so: on any
  `unknown command` reply (always a build mismatch — a typo is rejected
  client-side and never reaches the app) and in `rex version`, which already
  printed both numbers and never pointed out that they differed.
- **Every dispatch arm is now REACHABLE, and a guard keeps it that way** (23 Aug
  2026, ledger #385). `mail.mark_read` had been answered since mail shipped with
  no `rex` verb sending it — a door built and left shut, found by a docs
  reconcile reading the table by eye. `rex mail mark-read` sends it, and
  `every_command_this_server_answers_is_reachable_from_the_cli` fails on the next
  one. The same scan found `mail.list`'s `query`/`unread` parameters unreachable
  (the CLI sent `Null`); `rex mail list [--unread] [query]` uses them.
- **`--json` everywhere** — v1 rule, keep it: every new command returns the
  raw IPC payload under `--json`.

## Status — every 🟢/🟡/⚪ item is SHIPPED

The last one was `site create --starter-db`, closed 3 Sep 2026 (#462). This heading
had claimed a clean sweep for as long as that row sat three screens above it —
the same shape as the two `mail` rows below, and the reason both are now gated by
`no_roadmap_row_calls_unbuilt_a_thing_the_cli_already_dispatches`.

92 commands shipped. **That number is now GENERATED** (`scripts/doc-counts.sh`, and as of 2 Sep 2026 it
counts what it claims to: the counter matched `"word.word" =>` only, so every
three-segment name (`wp.user.password`, `wp.plugin.install`, …) and every
alternation arm went uncounted — 56 where 87 commands answer. A generated number
measuring a SUBSET is worse than a typed one, because nobody re-derives it. The
jump from 56 to 87 is that fix, not 31 new commands;
enforced by `verify.sh`) and this heading no longer carries a date: it read
"Status (16 Jul 2026) — 42 commands shipped" while the tree had 53, in the one
file a reader consults to learn what exists. A status line nobody re-counts is a
status line that is wrong, and dating it only tells you how long it has been so.

~~`config get|set`~~ — **shipped 24 Aug 2026** (ledger #397). The parked question
was which keys to allow-list; the answer is deny-by-default with the policy in
CORE (`core/settings_access.rs`), so the CLI and its guard read ONE list, and
writes route through `set_setting` so a key with a validating setter still gets
it — since 2 Sep 2026 (ledger #443) that routing is a REGISTRY lookup
(`GATED_SETTERS`) rather than a per-key branch, so gating a third key is one row
and `rex config set` cannot outrun it. ~~`site retry`~~ — **shipped 24 Aug 2026.** Whether `site.create`'s failure
message actually names it is a separate, unverified claim (see the row above).

What remains:
2. Design-first 🔴 set: ~~single-site restart~~ (shipped 2 Sep 2026, #444),
   ~~web-tier single-service control~~ (shipped 2 Sep 2026, #445),
   ~~`wp_user_delete`~~ (shipped 2 Sep 2026, #446),
   ~~progress streaming for long ops~~ (shipped 2 Sep 2026, #447). Still open:
   raw wp passthrough (security decision) — the one left is the one that is a
   ruling about what the CLI may execute, not a gap.
3. **Completions are part of shipping a verb** (2 Sep 2026, ledger #454).
   `site restart`, `site domains` and `service restart` all shipped working the
   same day and none was offered by `rex <tab>` — the place most terminal users
   would ever learn they exist. `every_dispatched_subcommand_is_offered_by_completions`
   now compares the completion constant against the dispatch in both directions.
4. **In-app verifies owed** (passthroughs whose restart/exposure half is
   guard-blocked in the example harness — exercise each once against the
   running app): `php install/uninstall`, `php settings set`, `db versions
   --set`, `site server/domain/move`, `mail clear`, `tunnel start`,
   `wp core update/switch`, and — until 3 Sep 2026 — the whole 2 Sep set.
   <!-- live-run-record: everything between these markers names a command that HAS
        been driven against a running app. The list ABOVE them names commands still
        owed, and the two read alike to a scanner — which is why the boundary is
        marked rather than guessed. `no_row_calls_a_command_unrun_that_the_verifies_list_records_as_run`
        reads only what is inside. -->
   **RUN LIVE 3 Sep 2026** against the dev app (`cddf821`) on two scratch sites,
   deleted after with no residue, and the user's own 11 sites 200 throughout:
   `site create --starter-db` (#462 — the seed is REAL: `php_livetest_rex`,
   table `starter_items`, the page renders "connected" and one seeded row),
   streamed `site create` (#447 — four phases, 36 → 36 → 78 → 89, one envelope
   last), `site restart` (#444 — the default-site outcome, naming the shared
   php-8.3 pool and the 18 sites `--pool` would affect; **proven by pid**: fpm
   stayed 1591, nginx stayed 61496, so nothing was bounced uninvited),
   `site domains --add/--remove` (the alias served HTTPS 200 and the cert was
   REISSUED — SAN carried `alias-livetest.rex` — and stopped being served on
   removal), `service restart nginx|edge` (#445 — nginx pid 61496 → 19165, a
   true restart; the edge reloaded, not restarted; `start`/`stop` of a web-tier
   name refused by name), and `wp user delete` (#446/#464 — **which the run
   found UNCOMPLETABLE**, see the row).
   **`site retry` — VERIFIED 24 Aug 2026** (refusal, a real half-provision retried to
   completion, and no clobber of a linked docroot).
   **`mail mark-read`, `mail list --unread` and `mail list <query>` — VERIFIED
   live 23 Aug 2026** against the running app: unread went 2 → 0, the unread
   filter cut 3 rows to 2, and a query matched 1 of 3. The live run also found
   the header describing the MAILBOX above a FILTERED list, which reads as a
   listing bug; it says "1 of 3 messages shown" now.
   **`repo watch --tail` and `watch tail` — VERIFIED live 3 Sep 2026** on a scratch
   WordPress site with a fixture plugin (a `package.json` whose `dev` script prints a
   tick a second, linked in — never the user's own repos, whose build scripts are not
   ours to run): ticks arrived one per second as they were produced; the watcher was
   still `running` after the tail was killed, which is the promise the message makes;
   `watch tail <dir>` picked up the same watcher already running; and an unknown dir
   was refused by name pointing at `watch list`. Site deleted after with no residue,
   the fixture FOLDER intact (link-only delete, as `repo link` promises).
   **`site relink` — VERIFIED live 23 Aug 2026** on a throwaway linked site with
   two docroots: what the site SERVED changed with the re-point (the config
   regenerated and the edge reloaded), the old docroot was untouched, a relative
   path resolved against the CLI's cwd, and a missing path was refused before
   anything was sent.
   <!-- /live-run-record -->
   Still owed: **`site restart`**'s other two outcomes (shipped 2 Sep 2026 with L0
   only: the backend leg needs a real FrankenPHP/Apache site stopped and
   respawned, and the refusal leg needs an ADOPTED backend, neither of which a
   unit test can hold). Kept BELOW the marker on purpose — a partly-run command
   is named on both sides, and only the owed half belongs outside.
