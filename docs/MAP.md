# MAP — where everything lives

One row per subsystem: the files that implement it, its entry points, and what proves
it. Grep less, read this first. Companion docs: `ARCHITECTURE.md` (how the pieces fit),
`PORTS.md` (ports + pins), `CLAIM-LEDGER.md` (what proves each invariant), `TODO.md`
(open work). File paths are relative to `src-tauri/src/` unless prefixed.

## The layer rule (one sentence each)

- `src/` (React/TS) calls typed wrappers in `src/lib/ipc/index.ts` — never raw `invoke`.
- `commands/` = thin Tauri IPC translators; parse args, call `core/`, map errors.
- `core/` = platform-agnostic domain logic; talks to `platform/` only through traits.
- `platform/` = ALL OS-specific code behind the 11 traits in `platform/traits.rs`
  (macOS real; `windows/`/`linux/` are `todo!()` stubs).
- `state/` = SQLite migrations + the store layer; only `state/` writes SQL.

## Backend subsystems

| Subsystem | Core | Commands / IPC | Platform traits touched | Proof anchors |
|---|---|---|---|---|
| Edge proxy (Caddy :443, root daemon, unix-socket admin) | `core/proxy.rs` | `commands/services.rs` | EdgeSupervisor, PrivilegeManager, ProcessSupervisor | ledger #62–69; `examples/wire_probe_check`, `edge_adopt_reload_check` |
| Shared web server (nginx :18088, vhost gen, dotfile guard) | `core/services.rs` | `commands/services.rs` | ProcessSupervisor | #102–107; `dotfile_guard_check`, `nginx_php_serve` |
| PHP versions + FPM pools (+ Xdebug debug pools) | `core/php.rs` (also: `unshipped_minor` — the DERIVED "a version we don't ship" fixture; `security_end`/`eol_since` — the EOL tell's dates), `core/phpconf.rs` (config readers) | `commands/php.rs` (returns `PhpVersionView`, not the stored row) | BinaryProvider, ProcessSupervisor | #108–110, #318, #321, #322; `php_pools_serve`, `xdebug_pool_check` |
| Upstream PHP version check (read-only, no updater) | `core/php_upstream.rs` | via `commands/php.rs` (`PhpVersionView.upstream`) | — (one plain reqwest GET) | #343; php.net `active.php?json`, launch-time + best-effort; produces a version STRING and nothing else |
| Per-site override servers (FrankenPHP 8200s / Apache 8300s) | `core/frankenphp.rs`, `core/apache.rs` | `commands/sites.rs` (server switch) | BinaryProvider, ProcessSupervisor | `frankenphp_serve`, `apache_site_check` |
| DB engines (MySQL/MariaDB/Postgres/Redis, version switch) | `core/db.rs` (DbEngine), `core/database.rs` (MySQL ops), `core/mariadb.rs`, `core/postgres.rs`, `core/redis.rs` | `commands/database.rs` | BinaryProvider, ProcessSupervisor | `db_engine_serve`, `db_version_switch_check`, `mariadb_*` |
| Service lifecycle, adoption, watchdog | `core/service_manager.rs`, `core/proc.rs`, `core/stack_guard.rs`, `core/monitor.rs`, `core/ports.rs` | `commands/services.rs`, `commands/system.rs` | ProcessSupervisor | #70–82; `adopt_check`, `stack_guard_check`, `health_watchdog_check` |
| DNS (hickory agent :15353, resolvers, TLD policy) | `core/dns.rs`, `core/tld.rs` | `commands/system.rs`, `commands/settings.rs` | DnsManager, DnsAgentManager, PrivilegeManager | #44–53; `dns_serve`, `dns_ssl_autostart_check` |
| TLS (local CA, per-site leaves ≤398d) | `core/ssl.rs`, `core/firefox.rs` | `commands/system.rs`, `commands/sites.rs` (cert card) | CertTrustManager | #152–154; `ca_gen`, `site_cert_gen` |
| Binaries (download-on-demand, pins, bundles) | `core/binaries.rs` (incl. `php_url`/`php_self_hosted_tag` — which source publishes a PHP build; `XdebugBottle` — the per-minor Xdebug row), `core/downloads.rs` (hub) | `commands/downloads.rs` | BinaryProvider, Paths | #83–90, #319, #320, #323; `download_progress_check`, `relink_tree_check` |
| Sites (provision, linked sites, move/rename/delete, streamed provisioning) | `core/sites.rs`, `core/site_env.rs`, `core/site_metrics.rs` | `commands/sites.rs`, `commands/site_provision.rs` | Paths, ShellRunner | #91–101; `site_provision_check`, `linked_site_check` |
| Blank-PHP starter (generated page + `db.php` + one seeded table) | `core/starter.rs`, `src-tauri/templates/starter/{index.php,db.php}` | `commands/site_provision.rs` (the Php `db`/`configure` phases), `core/sites.rs` (prepare writes the page) | BinaryProvider (engine), ProcessSupervisor | #426–429; `core::starter` unit tests |
| Laravel create (composer create-project, .env wiring, migrations) | `core/laravel.rs` | `commands/site_provision.rs` (Laravel phases), `commands/wordpress.rs` (`composer_tools`) | ShellRunner, BinaryProvider | `core::laravel` unit tests (.env shapes, install markers) |
| Site FROM a git repo (validate → clone into the docroot → composer/.env/key/migrate) | `core/sites.rs` (`validate_git_source`, `git_source_of`, `clone_into_docroot`), `core/laravel.rs` (`ensure_env_file`), `core/repo.rs` (clone, `composer_install`) | `commands/site_provision.rs` (`clone`/`deps`/`finalize` phases), `commands/repo.rs` (`repo_probe`, reused by the dialog) | ShellRunner, ProcessSupervisor | #263–299; `git_site_clone_check` (L1 sandbox), `git_site_provision_check` (L1 network, the real driver); `docs/PLAN-git-site-clone.md` |
| The site's own checkout — Repository tab: status, branch, fetch/pull/push, stash/restore/reset, scripts (UI: `sites/SiteRepoTab.tsx` → the shared `wordpress/RepoPanel.tsx`) | `core/repo.rs` (path-based git ops; working-tree ops `git_stash_push`/`git_stash_pop`/`git_reset_hard`/`git_status_report` + `validate_stash_ref`) | `commands/repo.rs` (`repo_site_info`; `repo_git_op`'s op whitelist; `repo_stashes`; `job_target` maps `kind: "site"` to the project root) | ShellRunner, ProcessSupervisor | #284–288, #310–312; `git_site_clone_check` §8, `repo_git_ops_check` legs 9–16 |
| WordPress ops (wp-cli, manager, streamed installs from wp.org OR a local zip, wp.org search) | `core/wordpress.rs` (`ensure_slugs` / `ensure_zip_paths` — one gate per source), `core/wporg.rs`, `core/blueprints.rs` | `commands/wordpress.rs` (63 cmds), `commands/wp_install.rs` (`source: wporg\|zip`), `commands/blueprints.rs` | ShellRunner | #141–147, #259–260; `wp_install_serve`, `wp_install_stream_check` (job 4 = the zip leg), `wporg_icons_check` (L1 network — the plugin-list icon column, incl. the derived premium ones), `wp_premium_update_check` (L1 network — the capability grant that makes a PAID plugin's update visible; `update_context_arg`/`checked_list` in `core/wordpress.rs`) |
| The wp-cli command SET (packages-dir pin + the tell) | `core/wp_packages.rs` (the pin, the detection, both copy guards), `core/wordpress.rs` (`wp_argv_prefix`/`wp_command` — the ONE argv builder and the ONE spawn), `core/copy_scan.rs` (test-only: the shared must-say scanner) | `commands/wordpress.rs` (`wp_cli_packages`) | Paths | #228 (pin), #229–230 (the carried command), #301 (the tell); `wp_packages_check` (L1, sandbox — plants its own canary); L2 `uireview` `wppackages-*` |
| What a wp-cli spawn hands back (neither end of stdout is anyone else's) | `core/wordpress.rs` — tail: `EOO_MARKER`, `eoo_require_arg` (the `--require` file beside the phar), `split_at_eoo`, `cut_post_run_tail` at the three captured spawns, `json_from_wp`; head: `wp_argv_prefix`'s `-d display_errors=stderr` | every WordPress screen's read path | — | #316 (tail), #317 (head); `wp_noise_check` (L1, sandbox — a control leg per half, each planting its own disease) |
| MCP server — agents drive rexenv (M1 read-only, M2a scratch sites, M2b PHP switch + mail, M3 `db_query` — behind a grant until D16, 4 Sep 2026; now SELECT-only at the dial's Read; parity SHIPPED 3 Sep 2026 P1–P7, `PLAN-mcp-parity.md`, with D15 the dial and D16 the card). Second `0600` socket beside the CLI's, NEVER TCP | `core/scratch.rs` (TTL, cap, reaper), `core/wp_mailtag.rs` (the `From` stamp), `core/agent_db.rs` (M3 stage 2a — the agent DB principals' names + provisioning SQL), `core/agent_query.rs` (stage 2b — the native `mysql_async` read path; source-guarded to never call `client_base_args`), `core/agent_access.rs` (D15 — the ONE global Agent access dial: Read / Changes / Full with a duration, `needed_for(scope)`, the launch sweep of a session-long level), `core/agent_grants.rs` (the five-scope vocabulary and the `Granted<S>` witness whose ONE door is `claim_by_level` — the dial; D17 retired the grant row, `authorize` and the ask list, and a source guard bans them); ~~`components/mcp/AgentDbGrants.tsx`~~ (deleted by D16) (stage 4 — the consent prompt and the grant list, copy-guarded), `components/mcp/AgentAccessDial.tsx` (D15/D16 — the ONE dial, level sentences served from Rust), ~~`components/mcp/AgentSiteGrants.tsx`~~ (deleted by D17 — publishing is the dial's Full, so no per-call consent surface remains) | `mcp_server.rs` (the module ROOT — toggles, the copy guard; there is no `mcp_server/mod.rs`), `mcp_server/scratch.rs` (executing tools), `tools.rs` (read-only — five tools; `site_info`/`site_inspect_folder` from parity P2.3), `user_sites.rs` (parity — the third registry, empty until P2; `UserCtx` whose only door is the scope witness; `every_tool()` in the root is the ONE enumeration), `view.rs` (`scrub_log_line` — the ONE scrubber), `feed.rs`; `commands/mcp.rs`; `rex mcp` pipe | Paths, ProcessSupervisor | #197 (⚠ NOT a sandbox — the posture IS the claim), #204–#227, #301, #398–#402 (M3: the principals, the native read path, `db_query`'s gate — the consent surface retired by D16, #500–#502); `mcp_socket_check`, `mcp_scratch_check` (sandbox), `mcp_user_site_check` (sandbox — parity: switch, gate, a real rename and a real delete on a sandbox docroot), `mcp_mail_check` (service — brings its own Mailpit), `mcp_secret_sweep`; plan: `PLAN-mcp-server.md` |
| "Log in as" magic link | `core/wp_login.rs` | `commands/wordpress.rs` | — | #33–36; `wp_login_check` |
| Loopback DNS for PHP (WP-Cron/self-calls under c-ares; `resolver_for` records which minors actually have it — 7.4 does not) | `core/wp_dns.rs` | installed by `commands/site_provision.rs` (settle-ok), re-installed by `commands/sites.rs` (rename), swept for all sites by `lib.rs` (launch) | — | #251–253; `wp_dns_check` |
| Tunnels (cloudflared, health probe, mu-plugin rewrite, parent-death guard) | `core/tunnels.rs`, `core/wp_tunnel.rs`, `platform/macos/parent_death_guard.rs` | `commands/tunnels.rs` | ProcessSupervisor (`guard_child_against_our_death`) | #1–32, #430, #432; `tunnel_check`, `tunnel_sweep`, `tunnel_muplugin_check`, `tunnel_parent_death_check` |
| Mail (Mailpit + the catch-all; unread filter, Mark all read, instant read state) | `core/mail.rs` (`search_query`/`UNREAD_QUERY`, `mark_all_read`; `Catch`/`laravel_env`/`catch_all_enabled` — the catch-all's ONE definition), `core/wp_mail_catch.rs` (the WordPress mu-plugin that beats an SMTP plugin), `core/laravel.rs` (`mail_env` — the CLI half) | `commands/mail.rs` (`mailpit_messages`'s `unread_only`, `mailpit_mark_all_read`) | BinaryProvider, ProcessSupervisor | #149–150, #313–315, #504–505; `mailpit_check`, `mail_route_check`, `mail_api_check`, `mail_adopt_settings_check`; wk-check `mail.js` |
| Mail catch-all — three carriers of one switch (`mail.catch_all`) | `core/services.rs` (`generate_fpm_config`'s `env[]`, QUOTED — a bare `null` is `ERROR: empty value` and the pool never starts), `core/php.rs` (`set_mail_catch` — ONE setter, both halves), `core/terminal.rs` (`PtyConfig::env`) | installed by `commands/site_provision.rs` (provision + the `.env` wiring and its `.env.rexenv-backup`), re-applied by `commands/sites.rs` (rename), swept for all sites by `lib.rs` (launch), carried to agents by `mcp_server/user_sites.rs` (`site_artisan`) | — | #504–505 |
| Adminer (internal vhost, deep links, console palette) | `core/adminer.rs` (incl. `set_theme`/`write_theme` + the wrapper's `css()` — the console's scheme) | `commands/database.rs` (incl. `adminer_set_theme`) | — | #37–43, #375; `adminer_*_check` (the palette legs live in `adminer_check`) |
| Valet/Herd migration (scan → import → db copy → rewrite) | `core/valet.rs`; Stage 2: `core/{dbsource,dbcompat,dbdump,dbrestore,dbmirror,dbimport}`; Stage 3: `core/{confedit,confverify,confrewrite}` | `commands/valet_import.rs`, `commands/db_import.rs`, `commands/rewrite.rs` | PrivilegeManager (resolver takeover) | #112–133, #181–182, #186; `valet_scan_check`, `db_dump_check`, `db_restore_check`, `config_rewrite_check` |
| Signed in-app updates (PHP patches, Adminer) — the trust boundary | `core/updates.rs` (`RELEASE_PUBKEY`, `verify`, serial/replay gate, host allowlist, `Family { Php, Adminer }`) | `commands/php.rs`, `commands/settings.rs` | BinaryProvider | plans: `PLAN-binary-updates.md`, `PLAN-adminer-updates.md`; `php_update_check` (network), `adminer_update_check` |
| `wp dist-archive` — a distributable zip from a repo | `core/dist_archive.rs` (bundled package tree, own TMPDIR, refuses without `.distignore`) | `commands/repo.rs` | ShellRunner | #229–#236; `dist_archive_check`; plan: `PLAN-dist-archive.md` |
| `.env` read + write (Laravel create, git clone, connection rewrite) | `core/dotenv.rs` (came out of `core/laravel.rs` for its second caller, and immediately caught a duplicate-key bug) | via `core/laravel.rs`, `core/sites.rs` | — | `core::dotenv` unit tests |
| Add-from-Git + assets (clone, jobs, watchers, link guard) | `core/repo.rs`, `core/devtools.rs` | `commands/repo.rs` (26 cmds; `job_target` resolves the `site` kind to the project root) | ShellRunner | #134–140, #183–185, #284–288; `repo_*_check` |
| Logs viewer | `core/logs.rs` | `commands/logs.rs` | Paths | #102; `log_tail_check` |
| macOS floor a binary DECLARES (`minos`) | `core/macho.rs` | none — read on the failure path by `service_manager::macos_floor_note` | — | #383; diagnosis only, never gates a spawn |
| Terminal (PTY) | `core/terminal.rs` | `commands/terminal.rs` | ShellRunner | `terminal_check` |
| Setup / teardown (system changes) | `core/setup.rs` | `commands/system.rs` | PrivilegeManager, CertTrustManager, DnsManager | `system_setup`, `system_teardown` |
| rex CLI (remote control, never a second brain) | `cli/src/main.rs` (own crate) + `cli_server.rs` (app side), `core/cli.rs` (PATH install) | dispatches to the SAME commands::* fns | Paths | #54–58; `cli_socket_check`; surface: `CLI-ROADMAP.md` |
| App state (SQLite v1–v43, store) | `state/db.rs` (migrations), `state/store.rs`, `state/models.rs`, `state/app.rs` (AppState, locks) | — | — | #167–174 |
| Menu-bar app (tray, no dock icon) | `core/tray.rs` (the menu as data: `TrayModel` → `MenuSpec`, `TrayAction` ids) | `lib.rs` (`install_tray`, `tray_model`, `render_menu`, `refresh_tray`, `on_tray_click`, `show_main_window`, the `Accessory` policy, the `CloseRequested` hide), `platform/macos/activation.rs` (`activate_app`), `scripts/make-menubar-icon.py` → `icons/menubar.png`, `App.tsx` `TrayRouteWatch` ← `tray://route`, `HIDDEN_LAUNCH_FLAG` + `first_window_decision` (login launch), `cli_server::hand_off_to_running_instance` + the `app.open` arm (single instance) | `rex open` | #436, #437, #438, #439, #441; plan: `PLAN-menubar-tray.md`; ARCHITECTURE "the APP outlives the window" |
| App entry / wiring | `lib.rs` (builder, launch adopt, watchdog, exit hooks, `StartupNotices`), `main.rs` (`--dns-agent` and `--tunnel-guard` modes), `error.rs` | — | — | #59–61, #431 |

## Frontend

| Screen / area | Route | Files |
|---|---|---|
| Sites list | `/sites` | `src/routes/Sites.tsx`, `src/components/sites/*` |
| Site detail (tabs: overview, database, logs, terminal, settings, WP) | `/sites/:id/:tab` | `src/routes/SiteDetail.tsx`, `src/components/{sites,wordpress,database,terminal}/*` |
| Services | `/services` | `src/routes/Services.tsx` |
| Databases | `/databases` | `src/routes/Databases.tsx`, `src/components/database/AdminerFrame.tsx` |
| Mail | `/mail` | `src/routes/Mail.tsx` |
| Tunnels | `/tunnels` | `src/routes/Tunnels.tsx` |
| Valet/Herd import | `/import` | `src/routes/Import.tsx` |
| Settings | `/settings` | `src/routes/Settings.tsx` |
| Onboarding | `/onboarding` | `src/routes/Onboarding.tsx` |
| Dev-only harnesses (tree-shaken from prod) | `/dev/git-panel`, `/dev/ui-review` | `src/routes/DevGitPanel.tsx`, `src/routes/DevUiReview.tsx` |
| IPC bridge (the ONLY invoke path; 245 exports) | — | `src/lib/ipc/index.ts` |
| Add a plugin/theme — the four sources behind `SourceTabs` | — | `components/wordpress/WordPressManager.tsx` (wp.org search + the shared `WpInstallCard`), `ZipAddPanel.tsx` (Upload zip), `GitAddPanel.tsx` (From Git), `LinkFolderPanel.tsx` (Link folder); probes `wk-checks/{zipinstall,wptoast,check,linkpanel}.js` |
| Shared UI hooks (editor pick + open, downloads) | — | `src/lib/useEditor.ts`, `src/lib/useDownloads.ts` |
| "Which app opens this" (browser/editor pick, icons, chevron, private window) | `commands/system.rs` (`list_browsers`, `open_in_browser` incl. its `private` arg, `open_external`'s preference route), `platform/macos/mod.rs` (bundle table + per-browser private flag, icon extraction, LaunchServices default) | `src/lib/useBrowser.ts`, `src/components/ui/{app-icon,app-picker,split-button,open-in,menu}.tsx` (`MenuItem`'s `action` = the row's second target) + `src/components/common/IncognitoIcon.tsx` (the private glyph); consumers `routes/{SiteDetail,Sites,Settings}.tsx` · example `browser_detect_check` · wk-check `openin.js` · ARCHITECTURE §8.2 |
| Freshness: "the user came back" (native window focus → query refetch) | — | `src/lib/window-focus.ts` (wired in `src/main.tsx`); consumers: `components/wordpress/{WordPressManager,RepoPanel}.tsx`; probe `wk-checks/focusrefresh.js` |
| PHP version presentation (EOL tell) | `core/php.rs` decides; the client only formats | `src/lib/php.ts` (`eolWhen`/`eolTag`/`eolNote`); consumers `routes/Settings.tsx` (row badge), `components/sites/NewSiteDialog.tsx` (create note), `routes/SiteDetail.tsx` (Environment note + the version select) · ledger #322 · DESIGN.md |
| Self-hosted runtime builds (the artifacts nobody else publishes) | — | `rexenv/runtimes` — build workflow + immutable release assets, pinned by SHA-256 in `core/binaries.rs`; see `docs/PLAN-php-74-support.md` §6 |
| macOS app menu "About rexenv" → in-app About | `lib.rs` (`install_about_menu_item`, emits `menu://about`) | `src/lib/ipc/index.ts` (`onAboutMenu`), `src/App.tsx` (`AboutMenuWatch` → `/settings?section=about`), `routes/Settings.tsx` (`?section=` deep link, `AboutSetting`, `BuildFactsCard`) · ARCHITECTURE §8 "macOS app menu" |
| Shell / theme / state | — | `src/components/shell/*`, `src/lib/theme.ts`, Zustand UI state, TanStack Query server state |

## Where do I…?

- **Add a service** → engine: mirror `core/redis.rs`/`core/postgres.rs`, pin in
  `bundle_manifest`/`manifest()` (`core/binaries.rs`), arm in `core/db.rs`, wire
  `core/downloads.rs`, update `PORTS.md`. Override server: mirror `core/frankenphp.rs`
  (loopback backend, NEVER the edge), one new `OverrideKind` arm.
- **Add an IPC command** → fn in the right `commands/*.rs` + register in `lib.rs` +
  typed wrapper in `src/lib/ipc/index.ts` + (optionally) a `cli_server.rs` dispatch arm.
- **Add a migration** → `state/db.rs` (v-next; nullable, no DEFAULT when a default
  would guess — see v17 reasoning), store fns in `state/store.rs` only.
- **Change a port / bump a binary** → `docs/PORTS.md` in the SAME commit as the constant.
- **Write an invariant comment** → its `CLAIM-LEDGER.md` row + verdict in the SAME commit.
- **Write a copy guard (a must-say list)** → put it in the module that owns the FACTS
  the copy states (#197), scan with `core::copy_scan` — never `split("#[cfg(test)]")`,
  which cuts at the first occurrence and silently stops covering a file whose test
  module sits in the middle — and carry BOTH canaries: a code landmark survives, and a
  comment-only phrase does not. Three guards have now shipped or nearly shipped
  reading their own explanation.
- **Spawn wp-cli** → `wordpress::wp_argv_prefix` + the pin, never a hand-rolled argv;
  the build fails otherwise (#228). `core/terminal.rs`'s wrapper is the one exemption.
- **Write an example** → read `src-tauri/examples/common/mod.rs` FIRST (the invariant),
  claim a fixture port there, declare a tier in `scripts/live-checks.sh`.
