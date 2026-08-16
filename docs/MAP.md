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
| Laravel create (composer create-project, .env wiring, migrations) | `core/laravel.rs` | `commands/site_provision.rs` (Laravel phases), `commands/wordpress.rs` (`composer_tools`) | ShellRunner, BinaryProvider | `core::laravel` unit tests (.env shapes, install markers) |
| Site FROM a git repo (validate → clone into the docroot → composer/.env/key/migrate) | `core/sites.rs` (`validate_git_source`, `git_source_of`, `clone_into_docroot`), `core/laravel.rs` (`ensure_env_file`), `core/repo.rs` (clone, `composer_install`) | `commands/site_provision.rs` (`clone`/`deps`/`finalize` phases), `commands/repo.rs` (`repo_probe`, reused by the dialog) | ShellRunner, ProcessSupervisor | #263–299; `git_site_clone_check` (L1 sandbox), `git_site_provision_check` (L1 network, the real driver); `docs/PLAN-git-site-clone.md` |
| The site's own checkout — Repository tab: status, branch, fetch/pull/push, stash/restore/reset, scripts (UI: `sites/SiteRepoTab.tsx` → the shared `wordpress/RepoPanel.tsx`) | `core/repo.rs` (path-based git ops; working-tree ops `git_stash_push`/`git_stash_pop`/`git_reset_hard`/`git_status_report` + `validate_stash_ref`) | `commands/repo.rs` (`repo_site_info`; `repo_git_op`'s op whitelist; `repo_stashes`; `job_target` maps `kind: "site"` to the project root) | ShellRunner, ProcessSupervisor | #284–288, #310–312; `git_site_clone_check` §8, `repo_git_ops_check` legs 9–16 |
| WordPress ops (wp-cli, manager, streamed installs from wp.org OR a local zip, wp.org search) | `core/wordpress.rs` (`ensure_slugs` / `ensure_zip_paths` — one gate per source), `core/wporg.rs`, `core/blueprints.rs` | `commands/wordpress.rs` (60 cmds), `commands/wp_install.rs` (`source: wporg\|zip`), `commands/blueprints.rs` | ShellRunner | #141–147, #259–260; `wp_install_serve`, `wp_install_stream_check` (job 4 = the zip leg) |
| The wp-cli command SET (packages-dir pin + the tell) | `core/wp_packages.rs` (the pin, the detection, both copy guards), `core/wordpress.rs` (`wp_argv_prefix`/`wp_command` — the ONE argv builder and the ONE spawn), `core/copy_scan.rs` (test-only: the shared must-say scanner) | `commands/wordpress.rs` (`wp_cli_packages`) | Paths | #228 (pin), #229–230 (the carried command), #301 (the tell); `wp_packages_check` (L1, sandbox — plants its own canary); L2 `uireview` `wppackages-*` |
| What a wp-cli spawn hands back (neither end of stdout is anyone else's) | `core/wordpress.rs` — tail: `EOO_MARKER`, `eoo_require_arg` (the `--require` file beside the phar), `split_at_eoo`, `cut_post_run_tail` at the three captured spawns, `json_from_wp`; head: `wp_argv_prefix`'s `-d display_errors=stderr` | every WordPress screen's read path | — | #316 (tail), #317 (head); `wp_noise_check` (L1, sandbox — a control leg per half, each planting its own disease) |
| MCP server — agents drive rexenv (M1 read-only, M2a scratch sites, M2b PHP switch + mail). Second `0600` socket beside the CLI's, NEVER TCP | `core/scratch.rs` (TTL, cap, reaper), `core/wp_mailtag.rs` (the `From` stamp) | `mcp_server/` — `mod.rs` (toggles, the copy guard), `scratch.rs` (executing tools), `tools.rs` (read-only), `view.rs` (`scrub_log_line` — the ONE scrubber), `feed.rs`; `commands/mcp.rs`; `rex mcp` pipe | Paths, ProcessSupervisor | #197 (⚠ NOT a sandbox — the posture IS the claim), #204–#227, #301; `mcp_socket_check`, `mcp_scratch_check` (sandbox), `mcp_mail_check` (service — brings its own Mailpit), `mcp_secret_sweep`; plan: `PLAN-mcp-server.md` |
| "Log in as" magic link | `core/wp_login.rs` | `commands/wordpress.rs` | — | #33–36; `wp_login_check` |
| Loopback DNS for PHP (WP-Cron/self-calls under c-ares) | `core/wp_dns.rs` | installed by `commands/site_provision.rs` (settle-ok), re-installed by `commands/sites.rs` (rename), swept for all sites by `lib.rs` (launch) | — | #251–253; `wp_dns_check` |
| Tunnels (cloudflared, health probe, mu-plugin rewrite) | `core/tunnels.rs`, `core/wp_tunnel.rs` | `commands/tunnels.rs` | ProcessSupervisor | #1–32; `tunnel_check`, `tunnel_sweep`, `tunnel_muplugin_check` |
| Mail (Mailpit + sendmail shim; unread filter, Mark all read, instant read state) | `core/mail.rs` (`search_query`/`UNREAD_QUERY`, `mark_all_read`) | `commands/mail.rs` (`mailpit_messages`'s `unread_only`, `mailpit_mark_all_read`) | BinaryProvider, ProcessSupervisor | #149–150, #313–315; `mailpit_check`, `mail_route_check`, `mail_api_check`; wk-check `mail.js` |
| Adminer (internal vhost, deep links) | `core/adminer.rs` | `commands/database.rs` | — | #37–43; `adminer_*_check` |
| Valet/Herd migration (scan → import → db copy → rewrite) | `core/valet.rs`; Stage 2: `core/{dbsource,dbcompat,dbdump,dbrestore,dbmirror,dbimport}`; Stage 3: `core/{confedit,confverify,confrewrite}` | `commands/valet_import.rs`, `commands/db_import.rs`, `commands/rewrite.rs` | PrivilegeManager (resolver takeover) | #112–133, #181–182, #186; `valet_scan_check`, `db_dump_check`, `db_restore_check`, `config_rewrite_check` |
| Add-from-Git + assets (clone, jobs, watchers, link guard) | `core/repo.rs`, `core/devtools.rs` | `commands/repo.rs` (24 cmds; `job_target` resolves the `site` kind to the project root) | ShellRunner | #134–140, #183–185, #284–288; `repo_*_check` |
| Logs viewer | `core/logs.rs` | `commands/logs.rs` | Paths | #102; `log_tail_check` |
| Terminal (PTY) | `core/terminal.rs` | `commands/terminal.rs` | ShellRunner | `terminal_check` |
| Setup / teardown (system changes) | `core/setup.rs` | `commands/system.rs` | PrivilegeManager, CertTrustManager, DnsManager | `system_setup`, `system_teardown` |
| rex CLI (remote control, never a second brain) | `cli/src/main.rs` (own crate) + `cli_server.rs` (app side), `core/cli.rs` (PATH install) | dispatches to the SAME commands::* fns | Paths | #54–58; `cli_socket_check`; surface: `CLI-ROADMAP.md` |
| App state (SQLite v1–v25, store) | `state/db.rs` (migrations), `state/store.rs`, `state/models.rs`, `state/app.rs` (AppState, locks) | — | — | #167–174 |
| App entry / wiring | `lib.rs` (builder, launch adopt, watchdog, exit hooks), `main.rs` (`--dns-agent` mode), `error.rs` | — | — | #59–61 |

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
| IPC bridge (the ONLY invoke path; 217 exports) | — | `src/lib/ipc/index.ts` |
| Add a plugin/theme — the four sources behind `SourceTabs` | — | `components/wordpress/WordPressManager.tsx` (wp.org search + the shared `WpInstallCard`), `ZipAddPanel.tsx` (Upload zip), `GitAddPanel.tsx` (From Git), `LinkFolderPanel.tsx` (Link folder); probes `wk-checks/{zipinstall,wptoast,check,linkpanel}.js` |
| Shared UI hooks (editor pick + open, downloads) | — | `src/lib/useEditor.ts`, `src/lib/useDownloads.ts` |
| "Which app opens this" (browser/editor pick, icons, chevron, private window) | `commands/system.rs` (`list_browsers`, `open_in_browser` incl. its `private` arg, `open_external`'s preference route), `platform/macos/mod.rs` (bundle table + per-browser private flag, icon extraction, LaunchServices default) | `src/lib/useBrowser.ts`, `src/components/ui/{app-icon,app-picker,split-button,open-in,menu}.tsx` (`MenuItem`'s `action` = the row's second target) + `src/components/common/IncognitoIcon.tsx` (the private glyph); consumers `routes/{SiteDetail,Sites,Settings}.tsx` · example `browser_detect_check` · wk-check `openin.js` · ARCHITECTURE §8.2 |
| Freshness: "the user came back" (native window focus → query refetch) | — | `src/lib/window-focus.ts` (wired in `src/main.tsx`); consumers: `components/wordpress/{WordPressManager,RepoPanel}.tsx`; probe `wk-checks/focusrefresh.js` |
| PHP version presentation (EOL tell) | `core/php.rs` decides; the client only formats | `src/lib/php.ts` (`eolWhen`/`eolTag`/`eolNote`); consumers `routes/Settings.tsx` (row badge), `components/sites/NewSiteDialog.tsx` (create note), `routes/SiteDetail.tsx` (Environment note + the version select) · ledger #322 · DESIGN.md |
| Self-hosted runtime builds (the artifacts nobody else publishes) | — | `rexenv/runtimes` — build workflow + immutable release assets, pinned by SHA-256 in `core/binaries.rs`; see `docs/PLAN-php-74-support.md` §6 |
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
