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
| PHP versions + FPM pools (+ Xdebug debug pools) | `core/php.rs`, `core/phpconf.rs` (config readers) | `commands/php.rs` | BinaryProvider, ProcessSupervisor | #108–110; `php_pools_serve`, `xdebug_pool_check` |
| Per-site override servers (FrankenPHP 8200s / Apache 8300s) | `core/frankenphp.rs`, `core/apache.rs` | `commands/sites.rs` (server switch) | BinaryProvider, ProcessSupervisor | `frankenphp_serve`, `apache_site_check` |
| DB engines (MySQL/MariaDB/Postgres/Redis, version switch) | `core/db.rs` (DbEngine), `core/database.rs` (MySQL ops), `core/mariadb.rs`, `core/postgres.rs`, `core/redis.rs` | `commands/database.rs` | BinaryProvider, ProcessSupervisor | `db_engine_serve`, `db_version_switch_check`, `mariadb_*` |
| Service lifecycle, adoption, watchdog | `core/service_manager.rs`, `core/proc.rs`, `core/stack_guard.rs`, `core/monitor.rs`, `core/ports.rs` | `commands/services.rs`, `commands/system.rs` | ProcessSupervisor | #70–82; `adopt_check`, `stack_guard_check`, `health_watchdog_check` |
| DNS (hickory agent :15353, resolvers, TLD policy) | `core/dns.rs`, `core/tld.rs` | `commands/system.rs`, `commands/settings.rs` | DnsManager, DnsAgentManager, PrivilegeManager | #44–53; `dns_serve`, `dns_ssl_autostart_check` |
| TLS (local CA, per-site leaves ≤398d) | `core/ssl.rs`, `core/firefox.rs` | `commands/system.rs`, `commands/sites.rs` (cert card) | CertTrustManager | #152–154; `ca_gen`, `site_cert_gen` |
| Binaries (download-on-demand, pins, bundles) | `core/binaries.rs`, `core/downloads.rs` (hub) | `commands/downloads.rs` | BinaryProvider, Paths | #83–90; `download_progress_check` |
| Sites (provision, linked sites, move/rename/delete, streamed provisioning) | `core/sites.rs`, `core/site_env.rs`, `core/site_metrics.rs` | `commands/sites.rs`, `commands/site_provision.rs` | Paths, ShellRunner | #91–101; `site_provision_check`, `linked_site_check` |
| Laravel create (composer create-project, .env wiring, migrations) | `core/laravel.rs` | `commands/site_provision.rs` (Laravel phases), `commands/wordpress.rs` (`composer_tools`) | ShellRunner, BinaryProvider | `core::laravel` unit tests (.env shapes, install markers) |
| WordPress ops (wp-cli, manager, streamed installs, wp.org search) | `core/wordpress.rs`, `core/wporg.rs`, `core/blueprints.rs` | `commands/wordpress.rs` (60 cmds), `commands/wp_install.rs`, `commands/blueprints.rs` | ShellRunner | #141–147; `wp_install_serve`, `wp_install_stream_check` |
| "Log in as" magic link | `core/wp_login.rs` | `commands/wordpress.rs` | — | #33–36; `wp_login_check` |
| Tunnels (cloudflared, health probe, mu-plugin rewrite) | `core/tunnels.rs`, `core/wp_tunnel.rs` | `commands/tunnels.rs` | ProcessSupervisor | #1–32; `tunnel_check`, `tunnel_sweep`, `tunnel_muplugin_check` |
| Mail (Mailpit + sendmail shim) | `core/mail.rs` | `commands/mail.rs` | BinaryProvider, ProcessSupervisor | #149–150; `mailpit_check`, `mail_route_check` |
| Adminer (internal vhost, deep links) | `core/adminer.rs` | `commands/database.rs` | — | #37–43; `adminer_*_check` |
| Valet/Herd migration (scan → import → db copy → rewrite) | `core/valet.rs`; Stage 2: `core/{dbsource,dbcompat,dbdump,dbrestore,dbmirror,dbimport}`; Stage 3: `core/{confedit,confverify,confrewrite}` | `commands/valet_import.rs`, `commands/db_import.rs`, `commands/rewrite.rs` | PrivilegeManager (resolver takeover) | #112–133, #181–182, #186; `valet_scan_check`, `db_dump_check`, `db_restore_check`, `config_rewrite_check` |
| Add-from-Git + assets (clone, jobs, watchers, link guard) | `core/repo.rs`, `core/devtools.rs` | `commands/repo.rs` (23 cmds) | ShellRunner | #134–140, #183–185; `repo_*_check` |
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
| Shared UI hooks (editor pick + open, downloads) | — | `src/lib/useEditor.ts`, `src/lib/useDownloads.ts` |
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
- **Write an example** → read `src-tauri/examples/common/mod.rs` FIRST (the invariant),
  claim a fixture port there, declare a tier in `scripts/live-checks.sh`.
