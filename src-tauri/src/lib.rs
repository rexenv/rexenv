//! rexenv backend entry. Registers all Tauri commands and wires the module tree.
//!
//! Architecture: `commands/` (thin IPC) → `core/` (platform-agnostic) →
//! `platform/` (OS traits, selected via cfg). See CLAUDE.md.

pub mod cli_server;
pub mod commands;
pub mod core;
pub mod crash;
pub mod error;
pub mod mcp_server;
pub mod platform;
pub mod state;
pub mod utils;

#[cfg(test)]
mod test_support;

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ONE rexenv per app-data directory. Checked before Tauri boots, for the
    // same reason `--dns-agent` and the tunnel guard are: a process that must
    // not exist should not first open a window, adopt services and bind
    // sockets. See `cli_server::hand_off_to_running_instance` for why the
    // socket is the lock and a pid file is not.
    // The socket is CLAIMED here too, not merely probed: a probe-then-bind-
    // later left a seconds-wide window in which a second launch booted fully
    // and then took the first one's socket (review, 3 Sep 2026).
    #[cfg(unix)]
    let cli_socket = match cli_server::claim_at_startup() {
        cli_server::StartupClaim::Ours(listener) => listener,
        cli_server::StartupClaim::AnotherInstanceRuns => return,
    };
    // Windows' lock is a named pipe, taken here for the same reason (W7 S1, ledger #620).
    #[cfg(windows)]
    let app_pipe = match cli_server::claim_pipe_at_startup() {
        cli_server::PipeStartup::Ours(held) => held,
        cli_server::PipeStartup::AnotherInstanceRuns => return,
    };

    // The interactive app may stop ADOPTED services (Stop-all after a relaunch);
    // any other process linking this lib (live-check examples) may stop only
    // what it spawned. See core::stack_guard.
    core::stack_guard::mark_app_process();
    tauri::Builder::default()
        // Native open/save dialogs (Settings → Sites folder picker).
        .plugin(tauri_plugin_dialog::init())
        // Closing the window HIDES it — it never quits. rexenv is a menu-bar
        // app, and the reason is the control plane: `rex` and the MCP server
        // are remote controls for a running app, their sockets are opened by
        // this process and die with it, so a quit used to take the CLI and
        // every agent session with it while the services it manages carried on
        // running. Closing the window now stops NOTHING — no service, no
        // tunnel, no job — so there is nothing to confirm here either; the
        // pause-and-confirm moved to the one place it is true, a real quit
        // (RunEvent::ExitRequested below). The window comes back through the
        // tray's "Open rexenv". See `docs/archive/PLAN-menubar-tray.md`.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                // The tile goes with the window: nothing is on screen any more,
                // so nothing in the dock should suggest there is. AFTER the
                // hide, never before — Regular → Accessory hides windows, and
                // doing it first would race the hide it is meant to follow.
                // …unless the tray never installed: then the tile is the
                // only way back, and it stays.
                #[cfg(target_os = "macos")]
                if window.app_handle().tray_by_id(TRAY_ID).is_some() {
                    dock_follows_window(window.app_handle(), false);
                }
            }
        })
        // Database Browser: `rexdb://localhost/…` proxies the embedded Adminer
        // through Rust with a native cookie jar — WebKit withholds third-party
        // cookies in cross-site iframes (ITP), which silently killed every
        // Adminer login. See `core::adminer::forward`.
        .register_asynchronous_uri_scheme_protocol(
            core::adminer::PROXY_SCHEME,
            |_ctx, request, responder| {
                let (parts, body) = request.into_parts();
                let method = parts.method.as_str().to_string();
                let path_and_query = parts
                    .uri
                    .path_and_query()
                    .map(|pq| pq.as_str().to_string())
                    .unwrap_or_else(|| "/".to_string());
                let content_type = parts
                    .headers
                    .get(tauri::http::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                tauri::async_runtime::spawn(async move {
                    let result = core::adminer::forward(
                        &method,
                        &path_and_query,
                        content_type.as_deref(),
                        body,
                    )
                    .await;
                    let response = match result {
                        Ok(r) => {
                            let mut builder = tauri::http::Response::builder().status(r.status);
                            for (name, value) in r.headers {
                                builder = builder.header(name, value);
                            }
                            builder.body(r.body).unwrap_or_else(|e| {
                                tauri::http::Response::builder()
                                    .status(500)
                                    .body(format!("rexenv db proxy: {e}").into_bytes())
                                    .expect("static 500")
                            })
                        }
                        Err(e) => tauri::http::Response::builder()
                            .status(502)
                            .header("content-type", "text/plain; charset=utf-8")
                            .body(format!("rexenv db proxy: {e}").into_bytes())
                            .expect("static 502"),
                    };
                    responder.respond(response);
                });
            },
        )
        .setup(move |app| {
            let platform = platform::current();
            // The host's pin tier, derived now and never stored (ledger #707);
            // every later resolve reads it through `binaries::tier()`.
            core::binaries::install_tier(platform.binary_tier());

            let hidden_launch = is_hidden_launch();

            // THE DOCK FOLLOWS THE WINDOW. Start as an accessory app — no dock
            // tile, no app-switcher entry — because with no window up the
            // menu-bar item is rexenv's whole presence, and a tile for
            // something that runs all day is a tile nobody clicks. While a
            // window IS up the app is Regular and takes its tile back: a
            // visible window with nothing in the dock cannot be Cmd-Tabbed to,
            // reads as a window belonging to nobody, and leaves the developer
            // hunting the menu bar for a window already on screen.
            // `dock_follows_window` is the ONE place that switch happens; see
            // `docs/archive/PLAN-menubar-tray.md` §2.
            //
            // **The policy is set BEFORE any window is shown**, and that
            // ordering is measured rather than reasoned (31 Aug 2026, three
            // launches of the real app): with this call left where it used to
            // sit — after the window is shown — the launch produced a menu-bar
            // icon and NO window, every time. Switching Regular → Accessory
            // hides the app's windows, so a window shown before the switch is a
            // window the switch takes away. That is also why hiding sets the
            // policy AFTER the hide, and showing sets it BEFORE the show.
            //
            // Set ONCE, to the state this launch is actually in — not flipped
            // twice in the same millisecond. It used to be an unconditional
            // Accessory here, with `show_main_window` switching to Regular a few
            // lines later, and that produced an app with its menu in the menu
            // bar, `lsappinfo` reporting `Foreground` — and NO DOCK TILE. macOS
            // does not reliably add the tile for an Accessory → Regular switch
            // made while the app is still launching. A hidden launch has no
            // window and stays Accessory; a normal launch is Regular from the
            // start and never asks the Dock for anything mid-flight.
            // Decided ONCE, from the facts a login launch will act on below
            // (`first_window_decision`): a hidden launch with setup incomplete
            // shows the onboarding window, and doing that as Accessory → Regular
            // inside `setup` is exactly the mid-launch flip macOS does not
            // reliably give a dock tile for (7723eb9) — on the one screen a new
            // user cannot get past.
            let login_needs_window = hidden_launch && login_launch_needs_window(platform.as_ref());
            #[cfg(target_os = "macos")]
            if hidden_launch && !login_needs_window {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            // THE WINDOW IS NOW HIDDEN BY DEFAULT (`tauri.conf.json`
            // `visible: false`), so somebody has to decide to show it.
            //
            // A launch the USER asked for shows it immediately — before the
            // database opens, before services are adopted — because that work
            // takes seconds and a launch that paints nothing for three seconds
            // reads as a launch that failed. A LOGIN launch (the LaunchAgent
            // passes `--hidden`) shows nothing here; the decision moves to
            // `first_window_decision` below, once there is enough state to ask
            // whether first-run setup is done, and a login is nobody's foreground
            // task so the extra seconds cost nothing.

            // THE APP'S OWN LOG, in every build — this used to be
            // `if cfg!(debug_assertions)`, which meant the installed app wrote
            // `log::info!`/`log::warn!` NOWHERE. Every diagnostic this codebase
            // emits was dev-only: the adoption count, the DNS fallback, and both
            // "skipped the sweep" warnings that are the GC's safety valves. A
            // user asked where to read one line and the honest answer was
            // "you can't" — which also means every bug report from a release
            // build arrives without the one artefact that would explain it.
            //
            // It goes in `log_dir()`, NOT macOS's `~/Library/Logs`, because that
            // is where nginx, php-fpm, Caddy, the DB and every per-site job log
            // already are, and because `core::logs` tails that directory — so
            // the app's own log becomes a source in the app's own Logs tab.
            // One directory beats the platform convention when the convention
            // would split a diagnosis across two places.
            //
            // Rotation is not optional here: this file now grows on a real
            // machine for months. `caddy-start.log` in the same directory
            // reached 1.2 MB unattended, and it is root-owned so a user cannot
            // even delete it.
            {
                use tauri_plugin_log::{Target, TargetKind};
                let sinks = log_sinks(platform.paths().log_dir().ok(), cfg!(debug_assertions));
                let targets = sinks.into_iter().map(|s| match s {
                    LogSink::File(path) => Target::new(TargetKind::Folder {
                        path,
                        file_name: Some(APP_LOG_STEM.into()),
                    }),
                    LogSink::Stdout => Target::new(TargetKind::Stdout),
                });
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .targets(targets)
                        .max_file_size(2 * 1024 * 1024)
                        .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                        // Local, because every other timestamp in that directory
                        // is local and a reader correlating them should not have
                        // to do timezone arithmetic to line up two files.
                        .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                        .build(),
                )?;
                // The MCP feed's file form lives beside the app log, so the
                // Logs tab tails it like every other source (its own tab, "AI
                // agents (MCP)"). Set here and nowhere else: unset, the feed is
                // table-only, which is what every lib test and `rex` get.
                if let Ok(dir) = platform.paths().log_dir() {
                    mcp_server::feed::set_log_path(dir.join(core::logs::MCP_LOG_FILE));
                }
            }

            // Show the window for a launch the USER asked for. Everything
            // below (database, adoption) takes seconds, and a launch that
            // paints nothing for three seconds reads as one that failed — so
            // this is as early as it can honestly go, but NOT earlier:
            //
            // The first version called this at the very top of `setup`, beside
            // `platform::current()`, and produced a tray icon with no window.
            // Two things are true up there and both are silent: the window may
            // not exist yet (`show_main_window` warns, but the log plugin is
            // installed a few lines BELOW, so the warning goes nowhere), and the
            // activation policy has not been set. Here, both are settled — the
            // logger exists to record a miss, and the policy is already
            // Accessory.
            if !hidden_launch {
                show_main_window(app.handle());
            }

            // The macOS app menu's "About rexenv" opens the app's OWN About
            // screen, not the native panel. The native panel can show a name,
            // a version and a copyright line and nothing else — no commit, no
            // build date, no licences, no links — while Settings → About
            // already answers "which build is this?" (commit + built-at), the
            // question that once cost a whole misdiagnosis. Two About surfaces
            // where one is strictly poorer is a doc that lies by omission.
            //
            // Built by EDITING the default menu, not replacing it: everything
            // else in the app menu (Services, Hide, Quit) and the Edit menu's
            // Cmd-C/V/Z keep working. macOS-only because that submenu is
            // macOS's; other platforms get the default menu untouched.
            #[cfg(target_os = "macos")]
            install_about_menu_item(app.handle())?;

            // The menu-bar status item. rexenv is a menu-bar app because the
            // CLI and the MCP server are remote controls for a RUNNING app:
            // both sockets are opened by THIS process a few lines below and
            // both die with it, so a quit takes the whole control plane with
            // it while the services it manages carry on. See
            // `docs/archive/PLAN-menubar-tray.md`. Failure to build it is logged, not
            // fatal — an app with no status item is still a working app, and
            // refusing to launch over a missing icon would be worse.
            if let Err(e) = install_tray(app.handle()) {
                log::warn!("tray: could not install the menu-bar item: {e}");
                // With no status item the dock tile is the only way back to
                // a hidden window and the app menu the only Cmd+Q — so the
                // tile stays. Accessory-with-no-tray is an app only `rex
                // open` can reach.
                #[cfg(target_os = "macos")]
                dock_follows_window(app.handle(), true);
            }


            // JS dialog panels (alert/confirm/prompt): wry implements none on
            // macOS, so confirm() silently returned false in-app — Adminer's
            // confirm-gated delete/drop buttons no-oped. Installed on the raw
            // WKWebView. macOS-only gap (and `PlatformWebview::inner` is
            // macOS/iOS-only): Windows/Linux webviews render their own dialogs.
            #[cfg(target_os = "macos")]
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.with_webview(|pw| unsafe {
                    platform::install_js_dialog_panels(pw.inner());
                });
            }

            // WebView2's browser shortcuts are a browser's, not an app's: Ctrl+R and F5 RELOADED the whole
            // app mid-task (measured on the Dell, 16 Sep 2026), and Ctrl+P, Ctrl+F and F12 are the same
            // family. wry leaves them at WebView2's default and tauri 2.11 does not surface the switch, so
            // the controller is asked directly (W9 S2, ruling Q2, ledger #637). Best-effort: a webview that
            // cannot answer is a webview whose keys stay as they were, not a launch that fails.
            #[cfg(target_os = "windows")]
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.with_webview(|pw| {
                    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
                    use windows::core::Interface;
                    // SAFETY: the controller tauri hands us belongs to this window's live webview, and every
                    // call here is a COM getter or setter on it, made on the thread tauri runs this closure on.
                    let done = unsafe {
                        pw.controller()
                            .CoreWebView2()
                            .and_then(|webview| webview.Settings())
                            .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
                            .and_then(|settings| settings.SetAreBrowserAcceleratorKeysEnabled(false))
                    };
                    match done {
                        Ok(()) => log::info!("webview: browser accelerator keys off (Ctrl+R, F5, Ctrl+P, Ctrl+F, F12)"),
                        Err(e) => log::warn!("webview: could not turn the browser accelerator keys off: {e}"),
                    }
                });
            }

            // DNS: the resolution plane must SURVIVE the app — the data plane
            // (nginx/fpm/DB/edge) already outlives a quit, but sites are
            // unreachable without DNS, and the old always-in-process resolver
            // died with the app (observed live: sites coasted ~1h40m on client
            // caches after a quit, then went dark until relaunch). Preferred
            // state: the per-user LaunchAgent (`rexenv --dns-agent`, KeepAlive,
            // no privilege). The agent plist is refreshed on every launch so it
            // tracks THIS binary (dev <-> installed hand off); a live old holder
            // of the port is adopted now and handed off later (the agent retries
            // its bind every 10s). In-process is the automatic FALLBACK so DNS
            // never regresses; failure of both is logged, not fatal.
            let dns_port = core::dns::DEFAULT_DNS_PORT;
            let agent_log = platform
                .paths()
                .log_dir()
                .map(|d| d.join("dns-agent.log"))
                .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/rexenv-dns-agent.log"));
            let agent_up = {
                let already = core::dns::answers_as_ours(dns_port);
                // Install/refresh regardless of `already`: the answering process
                // may be an OLD app instance's in-process resolver with no agent
                // installed at all.
                let installed = std::env::current_exe()
                    .map_err(error::Error::from)
                    .and_then(|exe| platform.dns_agent().install(&exe, &agent_log));
                if let Err(e) = &installed {
                    log::warn!("dns: could not install the resolver agent: {e}");
                }
                already
                    || (installed.is_ok() && {
                        // Give a fresh agent a moment to bind + answer.
                        let mut up = false;
                        for _ in 0..10 {
                            if core::dns::answers_as_ours(dns_port) {
                                up = true;
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                        up
                    })
            };
            // **Is the agent that is answering THIS build?** It outlives the app,
            // so after any bundle replacement — a self-update, or a user dragging
            // a new copy over the old one — launchd's process is still executing
            // the OLD inode. Nothing above notices: the plist is byte-identical
            // (the path did not move) so `install` skips the reload, and the
            // watchdog only kicks when the probe FAILS, which it does not,
            // because a stale agent answers perfectly well.
            //
            // Asking it who it is turns that into a measurement. `None` — an
            // agent from before this question existed — counts as stale, which is
            // the honest reading: it is by definition an older build.
            // `docs/archive/PLAN-self-update.md` T5, ledger #533.
            if agent_up {
                let answered = core::dns::agent_build_identity(dns_port);
                if core::dns::agent_is_stale(answered.as_deref()) {
                    log::info!(
                        "dns: the resolver agent is running {} but this build is {} — \
                         kickstarting it",
                        answered.as_deref().unwrap_or("an older build"),
                        core::dns::build_identity()
                    );
                    // In place (`launchctl kickstart -k`), never unload/load: a
                    // reload re-registers with Background Task Management and
                    // macOS posts an "App Background Activity" notification each
                    // time. Costs a sub-second gap in `.rex` resolution.
                    if let Err(e) = platform.dns_agent().kickstart() {
                        log::warn!("dns: could not kickstart the stale resolver agent: {e}");
                    }
                }
            }

            let dns_state = if agent_up {
                log::info!("dns: resolver agent serving on udp {dns_port} (survives app quits)");
                state::app::DnsState::new(None, state::app::DnsMode::Agent)
            } else {
                match tauri::async_runtime::block_on(core::dns::DnsService::start_default(
                    platform.as_ref(),
                )) {
                    Ok(dns) => {
                        log::warn!(
                            "dns: resolver agent unavailable — running IN-PROCESS \
                             (sites will stop resolving shortly after the app quits)"
                        );
                        state::app::DnsState::new(Some(dns), state::app::DnsMode::InProcess)
                    }
                    Err(e) => {
                        log::error!("dns: failed to start any resolver: {e}");
                        state::app::DnsState::new(None, state::app::DnsMode::Down)
                    }
                }
            };
            // ALWAYS managed so status + the health watchdog can read/repair it
            // without a missing-state panic.
            let in_process = matches!(dns_state.mode(), state::app::DnsMode::InProcess);
            app.manage::<state::app::DnsState>(dns_state);
            // Losing the startup race to the agent must not be permanent — see
            // `spawn_dns_handoff`. Only when we ARE the in-process holder: in
            // agent mode there is nothing to hand over, and in `Down` mode there
            // is nothing to hand.
            if in_process {
                spawn_dns_handoff(app.handle().clone(), dns_port);
            }

            // Registry of live PTY terminal sessions (§4.1).
            app.manage(commands::terminal::Terminals::default());
            app.manage(commands::repo::RepoJobs::default());
            app.manage(commands::repo::RepoWatches::default());
            app.manage(commands::wp_install::WpInstallJobs::default());
            app.manage(commands::site_provision::ProvisionJobs::default());
            app.manage(commands::valet_import::ImportJobs::default());
            app.manage(commands::db_import::DbImportJobs::default());
            // Registry of live per-site public tunnels (§9.1) + its health
            // prober (step 2 status honesty: dead children settled and public
            // URLs probed every 30s, UI open or not).
            app.manage(commands::tunnels::Tunnels::default());
            commands::tunnels::spawn_health_prober(app.handle().clone());

            // Open the app SQLite database (creating it + running migrations) and
            // hold it in app state for the IPC commands.
            // Notices the launch sweeps raise. They run BELOW, before any window
            // exists, so an emitted event would go to nobody — the queue is
            // drained by the frontend on mount (`startup_notices`).
            let notices = commands::system::StartupNotices::default();
            // Fatal init: open the DB + load/create the CA. On failure `AppState` can't
            // be built — so we do NOT leave it unmanaged (every AppState command would
            // then panic with a cryptic "state not managed"). Instead we record a
            // human-readable reason in the ALWAYS-managed `InitError`; the frontend
            // reads it first and shows an error screen rather than driving the app
            // (task 1.2 / H3).
            let init_error: Option<String> = match (
                state::db::open_for_platform(platform.paths()),
                core::ssl::load_or_create(platform.paths(), platform.permissions()),
            ) {
                (Ok(conn), Ok(ca)) => {
                    // Seed/refresh the PHP version registry (Phase 2 §1.2);
                    // preserves the user's installed and default choices on
                    // re-run. It no longer reports patch bumps: which minors
                    // need a restart is a LIVE question, asked below of the
                    // adopted masters themselves (migration v36, ledger #342).
                    if let Err(e) = core::php::seed_registry(&conn) {
                        log::error!("php: failed to seed version registry: {e}");
                    }
                    // The verified update catalog, re-checked from the cache, into
                    // the resolve path BEFORE anything resolves — so a patch the
                    // user selected is resolvable offline, exactly like a pin.
                    core::updates::install_cached(&conn);
                    // Did a self-update just happen? The answer is the version
                    // THIS binary reads from itself, never the one the marker
                    // hoped for — and this is the only place the previous
                    // bundle is deleted, because reaching here means this build
                    // launched and opened its database, which is the closest
                    // thing to "healthy" a process can honestly say about
                    // itself. `docs/archive/PLAN-self-update.md` T4.
                    if let Some((level, message)) =
                        core::app_update::finish_at_launch(&conn, &*platform)
                    {
                        log::info!("app update: {message}");
                        notices.push(level, message);
                    }
                    // B20 §4 Phase B: record each existing override site's port
                    // BEFORE any site is read or adopted. One-time + idempotent;
                    // non-colliding sites keep their exact current port (the
                    // consumer fallback covers the window if this ever fails).
                    if let Err(e) = core::sites::backfill_override_ports(&conn) {
                        log::error!("sites: override-port backfill failed (using derived fallback): {e}");
                    }
                    // v17 Phase B: freeze each existing site's docroot ownership
                    // — the legacy lexical sites-dir test, evaluated ONCE — so
                    // deleting a site stops depending on the mutable sites-dir
                    // setting. Zero behavior change; moved-out docroots stay
                    // preserved. One-time + idempotent; the legacy test remains
                    // the fallback for any row this fails to record.
                    // v24: record each WP site's content dir once (Bedrock/
                    // Radicle linked layouts) — mu-plugin writers read the
                    // record; a NULL row falls back to plain `wp-content`.
                    if let Err(e) = core::sites::backfill_content_dir(&conn) {
                        log::error!("sites: content-dir backfill failed (writers fall back to wp-content): {e}");
                    }
                    if let Err(e) =
                        core::sites::backfill_docroot_managed(&conn, platform.as_ref())
                    {
                        log::error!("sites: docroot-ownership backfill failed (using the legacy path test): {e}");
                    }
                    // Services OUTLIVE the app: closing rexenv doesn't stop the
                    // stack, so adopt any rexenv-owned survivors into this session's
                    // manager — status shows them running, Stop all works, Start all
                    // skips them. (Replaces the old stop-orphans-at-boot behavior.)
                    let sites = core::sites::list(&conn).unwrap_or_default();
                    // Bundled PHP resolves through c-ares, which never reads
                    // macOS split-DNS — so a site could not reach itself and
                    // WP-Cron died silently (10 Aug 2026). The mu-plugin that
                    // fixes it is installed per site at provision; this pass is
                    // what covers the sites that predate it, or lost the file.
                    core::wp_dns::ensure_all(&conn, &sites);
                    // The mail catcher: an SMTP plugin replaces PHPMailer's
                    // transport, after which the pool's `sendmail_path` is never
                    // consulted and a site imported from production delivers for
                    // real. This pass covers sites that predate the file, and it
                    // is also how turning the catch-all OFF reaches them.
                    core::wp_mail_catch::apply_all(&conn, &sites);
                    // The retired "start services when rexenv opens" setting
                    // (folded into the login toggle, 29 Sep 2026): swept so
                    // `rex setting` never lists a key nothing reads.
                    if let Err(e) = state::store::delete_setting(&conn, RETIRED_AUTO_START_SETTING) {
                        log::warn!("settings: could not sweep {RETIRED_AUTO_START_SETTING}: {e}");
                    }
                    // Drift: a resolver file we BORROWED that Valet/Herd has
                    // since taken back. Checked here because it is otherwise
                    // silent — our resolver still answers on its own port, so
                    // every health probe stays green while sites on that TLD
                    // stop resolving. One small file read per borrowed TLD.
                    for tld in core::dns::drifted_takeovers(
                        &conn,
                        platform.as_ref(),
                        core::dns::DEFAULT_DNS_PORT,
                    ) {
                        log::warn!(
                            "dns: the .{tld} resolver is no longer ours — Valet or Herd took it \
                             back. rexenv .{tld} sites won't resolve until you take it over \
                             again or move them to .rex"
                        );
                    }
                    // A resolver backup with no record can only come from a
                    // crash between writing the file and inserting its row —
                    // the row owns the file everywhere else. Sweep so litter
                    // can't accumulate in app-data unnoticed.
                    let swept = core::dns::sweep_orphan_backups(&conn, platform.as_ref());
                    if swept > 0 {
                        log::info!("dns: swept {swept} orphaned resolver backup(s)");
                    }
                    // Tunnel rows surviving to launch mean a crashed session
                    // (a clean exit clears the table): kill each recorded pid
                    // only on positive argv identification, and remove the
                    // mu-plugin + row in every branch. Tunnels die with the
                    // app — this is the crash half of that ruling.
                    let orphaned = core::tunnels::sweep_startup(&conn, platform.as_ref());
                    if orphaned > 0 {
                        log::warn!(
                            "tunnels: killed {orphaned} tunnel(s) still sharing after a crash"
                        );
                        notices.push(
                            "warn",
                            format!(
                                "Stopped {orphaned} public share(s) left running by a crashed \
                                 session — their links are dead now. Share again from the \
                                 Tunnels page if you still need them."
                            ),
                        );
                    }
                    // Backstop for the class "DB and process table disagree"
                    // (rowless but provably ours — pre-v23 fossils, app-data
                    // reset, lost records): measured on this machine, such
                    // processes serve publicly for WEEKS with nothing else
                    // able to see them.
                    let rowless = core::tunnels::sweep_rowless(&conn, platform.as_ref());
                    if rowless > 0 {
                        log::warn!(
                            "tunnels: stopped {rowless} PUBLIC share(s) this app had no record of"
                        );
                        notices.push(
                            "warn",
                            format!(
                                "Stopped {rowless} public share(s) this app had no record of — \
                                 they were serving a site to the internet. Nothing else on this \
                                 Mac could see them; check the Tunnels page if that is a surprise."
                            ),
                        );
                    }
                    // First run: create the sites root (~/rexenv/Sites, or the
                    // user's configured folder). Non-fatal — provision gives
                    // its own clear error if the folder still can't be made.
                    if let Err(e) = core::sites::ensure_sites_dir(&conn, platform.as_ref()) {
                        log::error!("sites: could not create the sites folder: {e}");
                    }
                    let state = state::app::AppState::new(conn, platform, ca);
                    {
                        let mut mgr = tauri::async_runtime::block_on(state.services.lock());
                        // The catch-all as the user last left it: an adopted
                        // session rewrites pool configs on the next restart, and
                        // a hardcoded `true` here would re-enable catching for
                        // someone who had deliberately turned it off.
                        let catch_mail = {
                            let conn = state.db.lock().expect("database lock poisoned");
                            // The manager's mirrors of per-site state, BEFORE adoption —
                            // see `load_mirrors` (ledger #785).
                            commands::services::load_mirrors(&conn, &mut mgr);
                            core::mail::catch_all_enabled(&conn)
                        };
                        let adopted =
                            mgr.adopt_startup(state.platform.as_ref(), &sites, catch_mail);
                        if adopted > 0 {
                            log::info!("adopted {adopted} running service(s) from a prior session");
                        }
                    }
                    app.manage(state);
                    // REFRESH the login plist if autostart is on. Same
                    // reason the DNS agent's plist is rewritten on every
                    // launch: a plist written by an older build names an older
                    // binary and, here, LACKS `--hidden` — so a user who
                    // enabled autostart before this change would keep getting a
                    // window at every login and nothing would ever fix it.
                    // Rewriting also re-points the entry after the app moves
                    // (dev build ↔ /Applications).
                    {
                        let autostart = app.state::<state::app::AppState>();
                        let autostart = autostart.platform.autostart();
                        if autostart.is_enabled().unwrap_or(false) {
                            // `refresh`, not `enable`: a launch of a dev build
                            // must not re-point the user's login item at a
                            // target/debug binary the next `cargo clean` deletes.
                            if let Err(e) = autostart.refresh() {
                                log::warn!("autostart: could not refresh the login item: {e}");
                            }
                        }
                    }
                    // The `rex` copy on the user's Path (Windows), brought up to this app's `rex` — never
                    // installed unasked, never moved to a dev build (`core::cli::refresh_at_launch`, #634).
                    {
                        let cli_state = app.state::<state::app::AppState>();
                        if let Err(e) = crate::core::cli::refresh_at_launch(cli_state.platform.as_ref()) {
                            log::warn!("cli: could not refresh the installed rex: {e}");
                        }
                    }

                    // A LOGIN launch stays hidden — unless first-run setup is
                    // unfinished, in which case the window is the only thing
                    // that can fix it (A7).
                    if hidden_launch {
                        first_window_decision(app.handle(), login_needs_window);
                    }
                    // The tray went up before any of this existed, holding a
                    // menu that claims nothing (`core::tray::bootstrap`). Now
                    // there is something to say — fill it in immediately rather
                    // than leaving the first tick to do it five seconds later.
                    refresh_tray(app.handle());

                    // PHP patch bump (pins ride app releases — Option A, no
                    // in-app updater): adopted pools still serve the OLD patch
                    // binary, so restart each bumped minor's live pool on the
                    // new pin, then GC the outdated `php-<oldpatch>/` caches.
                    // Prefetch happens FIRST and outside the services lock
                    // (download hub progress; the locked stop→ensure gap is a
                    // cache hit, not a download). Stack stopped ⇒ nothing to
                    // restart; the next start resolves the new pin anyway.
                    let bump = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        let Some(state) = bump.try_state::<state::app::AppState>() else {
                            return;
                        };
                        let platform = state.platform.as_ref();
                        // Hand the pool manager the registry's effective patches
                        // BEFORE any restart below. Without this the launch path
                        // has an empty snapshot, `effective` falls back to the
                        // pin, and a bump restart would drop a user's chosen
                        // patch back to the app's — silently, on the one path
                        // nobody is watching.
                        // Read UNDER the lock, push AFTER it drops — an `if let`
                        // holding the guard across the await is not Send, which is
                        // the compiler enforcing the house rule for us.
                        let want = state
                            .db
                            .lock()
                            .ok()
                            .and_then(|conn| core::php::effective_patches(&conn).ok())
                            .unwrap_or_default();
                        if !want.is_empty() {
                            state.services.lock().await.set_php_patches(want.clone());
                        }

                        // Which minors are serving bytes that are not this
                        // build's pin — asked of the RUNNING masters, so an
                        // adopted survivor from a previous app version answers
                        // for itself. No stored patch, nothing to consume, and
                        // a failure simply leaves the pool where it is: the next
                        // launch asks the same live question and gets the same
                        // answer, which is what makes the retry real.
                        //
                        // Compared against the EFFECTIVE patch — the `patches`
                        // snapshot read above — never against the pin. Against the
                        // pin, a minor the user had UPDATED read as "serving bytes
                        // that are not this build's pin" on EVERY launch, so its
                        // pool was stopped and respawned forever: 502s on that
                        // minor's sites, a health-log restart, and no cause the
                        // user could see. The restart put it back on the same patch
                        // that triggered the check.
                        let live: Vec<String> = {
                            let mgr = state.services.lock().await;
                            mgr.running_php_patches(platform)
                                .unwrap_or_default()
                                .into_iter()
                                .filter(|running| {
                                    want.get(&core::php::minor_of(running))
                                        .is_some_and(|w| w != running)
                                })
                                .map(|running| core::php::minor_of(&running))
                                .filter(|m| mgr.has_php_pool(m))
                                .collect()
                        };
                        // Per minor, in order: fetch, restart, wait, THEN confirm.
                        // `continue` rather than `return` — one minor's failure is
                        // not another minor's, and the batch form let the first
                        // failure abandon every later minor while the GC below
                        // still ran (ledger #338).
                        for minor in &live {
                            // The EFFECTIVE patch, not the pin: planning the pin
                            // prefetches bytes nobody will run and leaves the real
                            // download to happen inside the services lock, where
                            // every status poll's `try_lock` then fails.
                            let plan = match want.get(minor) {
                                Some(p) => core::downloads::plan_for_php_patch(platform, minor, p),
                                None => core::downloads::plan_for_php_with(platform, minor, &want),
                            };
                            if let Err(e) = core::downloads::prefetch(
                                platform,
                                &format!("Update PHP {minor}"),
                                &plan,
                            )
                            .await
                            {
                                // Old pool keeps serving, row keeps the old patch,
                                // so the next launch reports this minor again.
                                log::warn!("php: patch-update prefetch failed for {minor}: {e}");
                                continue;
                            }
                            let checks = {
                                let mut mgr = state.services.lock().await;
                                match mgr
                                    .restart_pools_for(platform, std::slice::from_ref(minor))
                                    .await
                                {
                                    Ok(c) => c,
                                    Err(e) => {
                                        log::warn!(
                                            "php: patch-update pool restart failed for {minor}: {e}"
                                        );
                                        continue;
                                    }
                                }
                            };
                            if let Err(e) = core::service_manager::await_ready(checks).await {
                                log::warn!(
                                    "php: {minor}'s patch-updated pool did not become ready: {e}"
                                );
                                continue;
                            }
                        }
                        // Repair caches that are PRESENT but incomplete — the
                        // bytes are there and something beside them is not
                        // (a re-pinned digest, or the licence texts every cache
                        // predating ledger #336 is missing). Done HERE, at app
                        // launch, rather than in `auto_start_inner`: login-start
                        // stays strictly offline (ledger #175 — never download,
                        // never prompt), and by the next login the cache is
                        // whole. Only ever repairs what already EXISTS, so a
                        // minor the user never installed is never fetched.
                        let repairs: Vec<String> = {
                            let installed = state
                                .db
                                .lock()
                                .ok()
                                .and_then(|conn| core::php::installed_effective(&conn).ok())
                                .unwrap_or_default();
                            installed
                                .into_iter()
                                .filter(|(_, patch)| {
                                    ["php", "php-fpm"].iter().any(|n| {
                                        core::binaries::needs_repair(platform, n, patch)
                                    })
                                })
                                .map(|(minor, _)| minor)
                                .collect()
                        };
                        for minor in &repairs {
                            log::info!("php: repairing an incomplete {minor} cache");
                            // Repair the patch that was FOUND broken. Planning the
                            // pin here would re-fetch a tree that is already fine
                            // and leave the incomplete one incomplete, so the
                            // detect would fire again at every launch.
                            let plan = match want.get(minor) {
                                Some(p) => core::downloads::plan_for_php_patch(platform, minor, p),
                                None => core::downloads::plan_for_php_with(platform, minor, &want),
                            };
                            if let Err(e) = core::downloads::prefetch(
                                platform,
                                &format!("Repair PHP {minor}"),
                                &plan,
                            )
                            .await
                            {
                                log::warn!("php: could not repair the {minor} cache: {e}");
                            }
                        }

                        // Refresh the SIGNED update manifest, so the catalog is
                        // populated before the user ever opens Settings.
                        //
                        // `install_cached` above loads what was already accepted —
                        // and on a fresh install that is nothing, so without this
                        // the catalog stays empty forever and the Update button can
                        // never appear. It could not: `php_update_check` existed as
                        // a command and an IPC wrapper and NOTHING called it. The
                        // door was built and the handle was never hung, which is
                        // why `every_ipc_wrapper_is_actually_called` now exists.
                        //
                        // Best-effort and last: no key pinned, no network, or a bad
                        // signature all leave the app resolving exactly its pins.
                        match core::updates::fetch().await {
                            Ok((doc, sig)) => {
                                let accepted = state.db.lock().ok().map(|conn| {
                                    core::updates::accept(&conn, &doc, sig.trim())
                                });
                                match accepted {
                                    Some(Ok(cat)) => {
                                        log::info!(
                                            "php: update manifest accepted ({} entries)",
                                            cat.versions().len()
                                        );
                                        core::binaries::install_catalog(cat);
                                    }
                                    Some(Err(e)) => {
                                        log::info!("php: update manifest not accepted: {e}")
                                    }
                                    None => log::warn!("php: update manifest not stored — db lock"),
                                }
                            }
                            Err(e) => log::info!("php: update manifest check skipped: {e}"),
                        }

                        // The APP's own signed release descriptor, on the same
                        // best-effort contract and for the same reason the PHP
                        // manifest is fetched here: a check nobody calls is a
                        // door with no handle, and `php_update_check` shipped
                        // exactly that way once. `auto_check_enabled` is read
                        // BEFORE any I/O, so the setting is a switch on the
                        // request rather than on what is done with the answer.
                        // `docs/archive/PLAN-self-update.md` T2.
                        let auto = state
                            .db
                            .lock()
                            .ok()
                            .map(|c| core::app_update::auto_check_enabled(&c))
                            .unwrap_or(true);
                        if auto {
                            check_for_app_update(&state).await;
                        } else {
                            log::info!("app update: automatic checking is off");
                        }

                        // Ask php.net what PHP has actually released, so the
                        // Settings rows can say "8.3.33 exists · this build pins
                        // 8.3.31". Best-effort and last: it gates nothing, it
                        // downloads nothing executable, and a failure leaves the
                        // previous answer (and the "checked N ago" line honest).
                        // See core::php_upstream for the line this must not
                        // cross — it may only ever produce a version STRING.
                        // Fetch UNLOCKED, then take the db lock only to write —
                        // the house rule about never holding a lock across a
                        // wait, and `fetch` takes no Connection so it cannot be
                        // done the other way round.
                        match core::php_upstream::fetch().await {
                            Ok(latest) => match state.db.lock() {
                                Ok(conn) => {
                                    if let Err(e) = core::php_upstream::store_check(&conn, latest) {
                                        log::warn!("php: could not cache the upstream list: {e}");
                                    }
                                }
                                Err(_) => log::warn!("php: upstream check not cached — db lock"),
                            },
                            Err(e) => log::info!("php: upstream version check skipped: {e}"),
                        }

                        // Sweep superseded caches. The keep-set is what the
                        // registry says each minor should RUN (`want`) unioned
                        // with what the live MASTERS are executing — never the
                        // pin table. This block is reached after a restart that
                        // failed partway, with later minors still serving from
                        // their old masters, and the process is the only thing
                        // that cannot be wrong about which bytes it is running.
                        //
                        // The union is not belt-and-braces in either direction.
                        // Live-only was wrong in the most ordinary way there is:
                        // with the stack stopped — after a reboot, or Stop all
                        // then quit — `running_php_patches` returns `Some(vec![])`
                        // rather than `None`, so a live-only set would delete the
                        // tree the user just installed. And `want`-only misses a
                        // master still on the previous patch mid-restart.
                        //
                        // TWO not-knowing guards, both costing a skipped sweep
                        // rather than a live tree — the sweep deletes what is NOT
                        // in the set, so an incomplete set is the dangerous one:
                        //   - a pool that could not be identified (`None` here);
                        //   - an empty `want`, which means the registry read
                        //     failed (`unwrap_or_default` above) and NOT that
                        //     nothing should be kept — `php_caches_to_keep`
                        //     refuses that case (see its docs; the pins used to
                        //     mask it).
                        let running = {
                            let mgr = state.services.lock().await;
                            mgr.running_php_patches(platform)
                        };
                        match running {
                            Some(live) => {
                                let effective: Vec<String> = want.values().cloned().collect();
                                for dir in core::binaries::gc_outdated_php_caches(
                                    platform, &effective, &live,
                                ) {
                                    log::info!("php: removed outdated binary cache {dir}");
                                }
                            }
                            None => log::warn!(
                                "php: skipped the outdated-cache sweep — a running pool could \
                                 not be identified, and the sweep deletes what it cannot see"
                            ),
                        }
                    });

                    // Login-start: the ONE "Start rexenv at login" toggle installs
                    // a `--hidden` launch, and that launch — only that launch —
                    // runs Start all, so the whole stack returns after a reboot
                    // without a click while an open the user made starts nothing.
                    // Runs AFTER adoption (already-running services are skipped)
                    // and is login-safe by construction: never downloads, never
                    // prompts (see `auto_start_services`). The gate also runs a
                    // login that a `--hidden` launch HANDED OFF to this process
                    // before it got here (see `LoginStartGate`).
                    if LOGIN_START.ready(hidden_launch) {
                        run_login_start(app.handle());
                    }
                    None
                }
                // Its own screen: "not writable / disk full" is the wrong advice
                // for data a newer rexenv migrated (ledger #593).
                (Err(e @ error::Error::NewerSchema { .. }), _) => {
                    log::error!("db: refused a database a newer rexenv migrated: {e}");
                    Some(e.to_string())
                }
                (Err(e), _) => {
                    log::error!("db: failed to open app database: {e}");
                    Some(format!(
                        "rexenv couldn't open its local database.\n\n{e}\n\nThis usually means \
                         its data folder isn't writable or the disk is full. Fix that, then reopen rexenv."
                    ))
                }
                (_, Err(e)) => {
                    log::error!("ssl: failed to load/create CA: {e}");
                    Some(format!(
                        "rexenv couldn't set up its local certificate authority.\n\n{e}\n\nThis \
                         usually means its data folder isn't writable. Fix that, then reopen rexenv."
                    ))
                }
            };
            // An init failure is a screen, not a log line: if the database or
            // the CA could not be opened, the app shows an error page — and a
            // hidden window would leave a menu-bar icon whose every action
            // fails for reasons nobody can read. Shown even at login.
            if hidden_launch && init_error.is_some() {
                // DEFERRED past `setup`: the policy above is Accessory for a
                // hidden launch, and flipping to Regular from inside `setup`
                // is the mid-launch switch macOS does not reliably give a dock
                // tile for (7723eb9). Shown from a task instead, the same
                // runtime transition `rex open` makes — measured to work.
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    show_main_window(&handle);
                });
            }
            app.manage(commands::system::InitError(init_error));
            // Named by TYPE, not just by binding: the guard in `commands::system`
            // reads this file to prove every `State<'_, T>` a command takes is
            // managed, and a bare `app.manage(notices)` tells it nothing about
            // which state that is.
            app.manage::<commands::system::StartupNotices>(notices);

            // `rex` CLI socket (see `cli_server`): requests execute in THIS
            // process through the same command fns the UI calls. Spawned even
            // when init failed — each request then gets the honest
            // still-starting/failed error instead of a dead socket.
            #[cfg(unix)]
            cli_server::spawn(app.handle().clone(), cli_socket);
            #[cfg(windows)]
            cli_server::spawn_pipe(app.handle().clone(), app_pipe);

            // MCP server socket (see `mcp_server`): the AI-agent endpoint,
            // driven through the `rex mcp` pipe. Its own `0600` socket beside
            // the CLI one; M1 tools are read-only (list_sites), reached only
            // through ReadCtx. OPT-IN — the socket binds only if the user
            // enabled it in Settings → AI agents (default off).
            // "Allow for this session" means THIS process: end every session
            // grant (and a session-long dial) the previous launch left behind
            // BEFORE the socket binds and any client can connect — the review
            // found the bind first, a window the size of the sweep.
            {
                use tauri::Manager;
                if let Some(state) = app.try_state::<state::app::AppState>() {
                    commands::mcp::end_session_grants_at_launch(state.inner());
                }
            }
            mcp_server::spawn_if_enabled(app.handle().clone());

            // The scratch reaper: collect agent-created sites whose clock ran
            // out — now (catching everything that expired while rexenv was
            // closed) and hourly after. Skips anything publicly shared (#29:
            // rexenv never stops a share on the user's behalf), refuses to
            // delete more than the scratch cap in one pass, and every removal is
            // both a feed row and a user-visible summary.
            commands::scratch::spawn(app.handle().clone());

            // Re-check for an app update every 6 hours. A machine left running
            // for a week would otherwise only ever hear about a release at its
            // next launch, which for a menu-bar app that outlives its window can
            // be a long time. Reads the setting on every tick.
            spawn_app_update_poller(app.handle().clone());

            // Health watchdog: every 10s probe every service the manager OWNS and
            // respawn dead ones (bounded attempts) — the UI used to show "running"
            // forever off the initial start state while e.g. a crashed edge left
            // every site unreachable until a manual Stop all / Start all. Uses
            // try_lock so it never contends with a user-driven start/stop, and
            // await_ready runs AFTER the lock is dropped (M4). Also restarts the
            // in-process DNS resolver if its task died. Events are appended to
            // <log_dir>/health.log and emitted as `service-health`.
            // Download-progress bridge: forward download-hub snapshots to the
            // frontend as `download-progress` events. Same race-free pattern as
            // `service-health`: full snapshots, not deltas, so event order can't
            // matter. The watch channel + 100ms pause coalesces chunk-level
            // updates to ≤10 events/s, always ending on the final state (the
            // last change re-arms `changed()`, so the terminal snapshot is
            // always emitted).
            let dl = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::Emitter;
                let mut rx = core::downloads::hub().subscribe();
                while rx.changed().await.is_ok() {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    rx.borrow_and_update(); // mark the burst seen, then emit its final state
                    let _ = dl.emit("download-progress", core::downloads::hub().snapshot());
                }
            });

            let watchdog = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::Emitter;
                let mut dns_failures: u32 = 0;
                // Consecutive polls the wire probe found silent (`core::dns::PROBE_MISS_POLLS`):
                // a kick on the FIRST silent poll restarted a healthy agent on a loaded VM
                // (27–29 Sep 2026) and toasted "DNS stopped unexpectedly".
                let mut dns_probe_misses: u32 = 0;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    let Some(state) = watchdog.try_state::<state::app::AppState>() else {
                        continue; // init failed — nothing to supervise
                    };
                    // Snapshot the site set (brief DB lock, never held across .await).
                    let sites = state
                        .db
                        .lock()
                        .ok()
                        .and_then(|conn| core::sites::list(&conn).ok())
                        .unwrap_or_default();
                    let (mut events, checks) = {
                        let Ok(mut mgr) = state.services.try_lock() else {
                            continue; // a start/stop is in flight — skip this tick
                        };
                        mgr.reconcile_health(state.platform.as_ref(), &state.ca, &sites)
                            .await
                    };
                    // Readiness of respawned services — with the lock dropped.
                    if let Err(e) = core::service_manager::await_ready(checks).await {
                        log::warn!("health: a respawned service did not become ready: {e}");
                    }

                    // DNS resolution (owned here, not by the manager). Mode-aware:
                    // Agent → probe over the wire; a dead agent gets a bounded
                    // launchctl kickstart, and after 3 failed kicks we fall back
                    // to an IN-PROCESS resolver so sites keep resolving NOW (the
                    // degraded mode is surfaced in Settings). InProcess → restart
                    // the task in place (the old behavior). Down → treated as
                    // Agent: `Down` is latched by ONE path, the handoff's rebind
                    // failing while the agent holds the port but does not answer
                    // YET — the agent's cold bind is a process booting — and the
                    // first version left that arm empty, so the app said DOWN for
                    // the rest of the session while `.rex` resolved fine (macOS
                    // 13→15 upgrade run, 23 Sep 2026: agent answering 20s after
                    // the latch, `rex status` DOWN two hours later). The probe
                    // below is what makes Down a snapshot, not a verdict.
                    let dns = watchdog.state::<state::app::DnsState>();
                    match dns.mode() {
                        state::app::DnsMode::Agent | state::app::DnsMode::Down => {
                            // Three datagrams, then two polls, before a kick (#756): a loaded VM
                            // answers late or drops one; a dead agent answers none of them twice.
                            let answered = core::dns::answers_as_ours_patiently(
                                core::dns::DEFAULT_DNS_PORT,
                                core::dns::PROBE_TRIES,
                            );
                            dns_probe_misses = core::dns::probe_misses_after(dns_probe_misses, answered);
                            if answered {
                                dns_failures = 0;
                                if matches!(dns.mode(), state::app::DnsMode::Down) {
                                    dns.set(None, state::app::DnsMode::Agent);
                                    events.push(core::service_manager::HealthEvent {
                                        service: "DNS".into(),
                                        action: "adopted",
                                        detail: "resolver agent is answering after all — \
                                                 adopted (the handoff had given it up as down)"
                                            .into(),
                                    });
                                }
                            } else if !core::dns::kick_after(dns_probe_misses) {
                                // One silent poll: say nothing, ask again in 10 s. A toast here
                                // named a restart the resolver never needed.
                            } else if dns_failures < 3 {
                                dns_failures += 1;
                                match state.platform.dns_agent().kickstart() {
                                    Ok(()) => events.push(core::service_manager::HealthEvent {
                                        service: "DNS".into(),
                                        action: "restarted",
                                        detail: "resolver agent was not answering; kicked it \
                                                 (next poll verifies)"
                                            .into(),
                                    }),
                                    Err(e) => events.push(core::service_manager::HealthEvent {
                                        service: "DNS".into(),
                                        action: "restart-failed",
                                        detail: format!("resolver agent kickstart failed: {e}"),
                                    }),
                                }
                            } else if dns_failures == 3 {
                                dns_failures += 1; // one-shot fallback, no loop
                                match core::dns::DnsService::start_default(state.platform.as_ref())
                                    .await
                                {
                                    Ok(new_dns) => {
                                        dns.set(Some(new_dns), state::app::DnsMode::InProcess);
                                        // The SECOND door into in-process mode
                                        // gets the same handoff as the first:
                                        // launchd relaunches the agent (KeepAlive)
                                        // and it retries its bind every 10s
                                        // forever — against a port we now hold.
                                        // Without this, DNS died with the app for
                                        // the rest of the session by a different
                                        // route than the startup race.
                                        spawn_dns_handoff(
                                            watchdog.clone(),
                                            core::dns::DEFAULT_DNS_PORT,
                                        );
                                        events.push(core::service_manager::HealthEvent {
                                            service: "DNS".into(),
                                            action: "restarted",
                                            detail: "resolver agent would not come back — \
                                                     serving in-process instead (sites resolve, \
                                                     but not after the app quits; handing back \
                                                     to the agent when it answers)"
                                                .into(),
                                        });
                                    }
                                    Err(e) => events.push(core::service_manager::HealthEvent {
                                        service: "DNS".into(),
                                        action: "gave-up",
                                        detail: format!(
                                            "resolver agent dead and in-process fallback \
                                             failed: {e}"
                                        ),
                                    }),
                                }
                            }
                        }
                        state::app::DnsMode::InProcess => {
                            let died = dns
                                .service
                                .lock()
                                .map(|g| g.as_ref().is_some_and(|d| !d.is_running()))
                                .unwrap_or(false);
                            if died && dns_failures < 3 {
                                match core::dns::DnsService::start_default(state.platform.as_ref())
                                    .await
                                {
                                    Ok(new_dns) => {
                                        dns.set(Some(new_dns), state::app::DnsMode::InProcess);
                                        dns_failures = 0;
                                        events.push(core::service_manager::HealthEvent {
                                            service: "DNS".into(),
                                            action: "restarted",
                                            detail: "embedded resolver task had died; restarted"
                                                .into(),
                                        });
                                    }
                                    Err(e) => {
                                        dns_failures += 1;
                                        events.push(core::service_manager::HealthEvent {
                                            service: "DNS".into(),
                                            action: "restart-failed",
                                            detail: e.to_string(),
                                        });
                                    }
                                }
                            } else if !died {
                                dns_failures = 0;
                            }
                        }
                        // The user asked for this silence (Settings → Remove
                        // system changes). Probing it would kick the agent
                        // that was just uninstalled and, after three misses,
                        // serve DNS in-process — the clean-15 smoke watched
                        // exactly that (#715). Deliberately empty.
                        state::app::DnsMode::Removed => {}
                    }

                    if !events.is_empty() {
                        core::service_manager::log_health_events(state.platform.as_ref(), &events);
                        for e in &events {
                            log::warn!("health: [{}] {}: {}", e.action, e.service, e.detail);
                        }
                        let _ = watchdog.emit("service-health", &events);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_info,
            commands::system::platform_words,
            commands::system::repair_resolver,
            commands::system::unresolvable_tlds,
            commands::system::remove_resolver,
            commands::system::init_error,
            commands::system::startup_notices,
            commands::system::global_status,
            commands::system::open_external,
            commands::system::reveal_path,
            commands::system::list_editors,
            commands::system::open_in_editor,
            commands::system::list_browsers,
            commands::system::open_in_browser,
            commands::system::dns_status,
            commands::system::cli_status,
            commands::system::cli_install,
            commands::system::system_setup,
            commands::system::trust_local_ca,
            commands::system::firefox_trust_status,
            commands::system::trust_ca_in_firefox,
            commands::system::allow_tlds_in_firefox,
            commands::system::regenerate_certs,
            commands::system::autostart_status,
            commands::system::set_autostart,
            commands::system::uninstall_system,
            commands::blueprints::list_blueprints,
            commands::blueprints::save_blueprint,
            commands::blueprints::delete_blueprint,
            commands::sites::list_sites,
            commands::sites::offered_web_servers,
            commands::sites::offered_db_engines,
            commands::sites::sites_serving,
            commands::sites::sites_resources,
            commands::sites::create_site,
            commands::sites::rename_site,
            commands::sites::change_site_domain,
            commands::sites::move_site_docroot,
            commands::sites::relink_site_docroot,
            commands::sites::inspect_linked_folder,
            commands::valet_import::scan_valet_import,
            commands::valet_import::resolver_tld_status,
            commands::valet_import::resolver_take_over,
            commands::valet_import::resolver_hand_back,
            commands::valet_import::resolver_drift,
            commands::valet_import::valet_import_run,
            commands::valet_import::valet_import_cancel,
            commands::db_import::db_import_start,
            commands::db_import::db_import_state,
            commands::rewrite::rewrite_preview,
            commands::rewrite::rewrite_apply,
            commands::rewrite::rewrite_revert,
            commands::db_import::db_import_cancel,
            commands::db_import::db_import_record,
            commands::db_import::db_import_records,
            commands::db_import::db_import_leftovers,
            commands::db_import::db_import_delete_leftover,
            commands::sites::list_site_env,
            commands::sites::set_site_env,
            commands::sites::site_cert_info,
            commands::sites::regenerate_site_cert,
            commands::sites::set_site_php_version,
            commands::sites::set_site_web_server,
            commands::sites::restart_site,
            commands::sites::set_site_enabled,
            commands::sites::set_all_sites_enabled,
            commands::sites::add_site_domain,
            commands::sites::remove_site_domain,
            commands::sites::site_domains,
            commands::sites::all_site_domains,
            commands::sites::set_site_xdebug,
            commands::sites::delete_site,
            commands::sites::keep_site,
            commands::sites::scratch_packages,
            commands::php::list_php_versions,
            commands::php::frankenphp_embedded_php,
            commands::php::set_php_version_installed,
            commands::php::set_default_php_version,
            commands::php::php_update_check,
            commands::app_update::app_update_state,
            commands::app_update::app_update_check,
            commands::app_update::app_update_apply,
            commands::app_update::app_update_restart,
            commands::app_update::app_update_readiness,
            commands::app_update::app_update_skip,
            commands::app_update::app_update_set_auto_check,
            commands::php::php_update_apply,
            commands::database::adminer_status,
            commands::database::adminer_set_theme,
            commands::database::adminer_update_check,
            commands::database::adminer_update_apply,
            commands::php::get_php_settings,
            commands::php::apply_php_settings,
            commands::system::setup_edge_conflict,
            commands::wordpress::wp_info,
            commands::wordpress::wp_cli_packages,
            commands::wordpress::wp_plugins,
            commands::wordpress::wp_org_search_plugins,
            commands::wordpress::wp_org_search_themes,
            commands::wordpress::wp_org_plugin_icons,
            commands::wordpress::wp_plugin_activate,
            commands::wordpress::wp_plugin_deactivate,
            commands::wordpress::wp_plugin_update,
            commands::wordpress::wp_plugin_delete,
            commands::wordpress::wp_themes,
            commands::wordpress::wp_theme_activate,
            commands::wordpress::wp_theme_update,
            commands::wordpress::wp_theme_delete,
            commands::wordpress::wp_users,
            commands::wordpress::wp_user_create,
            commands::wordpress::wp_user_set_password,
            commands::wordpress::wp_user_set_role,
            commands::wordpress::wp_user_delete,
            commands::wordpress::wp_primary_admin,
            commands::wordpress::wp_user_login_url,
            commands::wordpress::wp_admin_login_url,
            commands::wordpress::wp_debug_get,
            commands::wordpress::wp_debug_set,
            commands::wordpress::wp_debug_flag_get,
            commands::wordpress::wp_debug_flag_set,
            commands::wordpress::wp_maintenance_get,
            commands::wordpress::wp_maintenance_set,
            commands::wordpress::wp_search_replace,
            commands::wordpress::wp_permalink_get,
            commands::wordpress::wp_permalink_set,
            commands::wordpress::wp_checksum_cleanup,
            commands::wordpress::wp_options,
            commands::wordpress::wp_option_update,
            commands::wordpress::wp_core_versions,
            commands::wordpress::wp_core_switch_version,
            commands::wordpress::wp_languages,
            commands::wordpress::wp_switch_language,
            commands::wordpress::wp_cache_flush,
            commands::wordpress::wp_transient_delete_all,
            commands::wordpress::wp_cron_events,
            commands::wordpress::wp_cron_run_due,
            commands::wordpress::wp_cron_run_hook,
            commands::wordpress::wp_rewrite_flush,
            commands::wordpress::wp_core_update,
            commands::wordpress::wp_core_reinstall,
            commands::wordpress::wp_core_cut_names,
            commands::wordpress::wp_core_repair_cut_names,
            commands::wordpress::wp_core_verify_checksums,
            commands::wordpress::wp_db_export,
            commands::wordpress::wp_db_import,
            commands::wordpress::wp_content_export,
            commands::wordpress::wp_site_reset,
            commands::wordpress::wp_default_creds,
            commands::wordpress::wp_multisite_convert,
            commands::wordpress::wp_network_sites,
            commands::wordpress::wp_network_site_create,
            commands::wordpress::wp_network_site_delete,
            commands::wordpress::wp_plugin_activate_network,
            commands::wordpress::wp_plugin_deactivate_network,
            commands::wordpress::wp_themes_network_enabled,
            commands::wordpress::wp_theme_enable_network,
            commands::wordpress::wp_theme_disable_network,
            commands::wordpress::wp_super_admins,
            commands::wordpress::wp_super_admin_add,
            commands::services::start_services,
            commands::services::stop_services,
            commands::services::restart_web_service,
            commands::services::services_status,
            commands::services::runtime_problem,
            commands::downloads::downloads_state,
            commands::downloads::retry_download,
            commands::downloads::core_binaries_plan,
            commands::downloads::prefetch_core_binaries,
            commands::logs::log_targets,
            commands::logs::tail_log,
            commands::logs::log_clear,
            commands::logs::log_download,
            commands::logs::wp_debug_log_status,
            commands::logs::wp_debug_log_tail,
            commands::logs::wp_debug_log_clear,
            commands::logs::wp_debug_log_download,
            commands::terminal::terminal_open,
            commands::terminal::terminal_open_external,
            commands::terminal::list_terminals,
            commands::terminal::terminal_write,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_close,
            commands::mail::mailpit_status,
            commands::mail::start_mail,
            commands::mail::stop_mail,
            commands::mail::mailpit_messages,
            commands::mail::mailpit_message,
            commands::mail::mailpit_message_raw,
            commands::mail::mailpit_clear,
            commands::mail::mailpit_mark_all_read,
            commands::mail::mailpit_delete,
            commands::mail::mail_catch_all,
            commands::mail::set_mail_catch_all,
            commands::database::databases_status,
            commands::database::start_database,
            commands::database::stop_database,
            commands::database::db_engine_versions,
            commands::database::db_engine_refusals,
            commands::system::legacy_notice,
            commands::database::set_db_engine_version,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::sites_folder,
            commands::settings::default_tld,
            commands::settings::set_default_tld,
            commands::settings::tld_policy,
            commands::mcp::mcp_status,
            commands::mcp::mcp_set_enabled,
            commands::mcp::agent_activity,
            commands::mcp::agent_activity_clear,
            commands::mcp::agent_access_get,
            commands::mcp::agent_access_set,
            commands::tunnels::start_tunnel,
            commands::tunnels::stop_tunnel,
            commands::tunnels::tunnels_status,
            commands::repo::repo_probe,
            commands::repo::repo_add,
            commands::repo::repo_run_step,
            commands::repo::repo_cancel,
            commands::repo::repo_job_state,
            commands::repo::repo_site_jobs,
            commands::repo::repo_assets,
            commands::repo::repo_site_info,
            commands::repo::repo_asset_status,
            commands::repo::repo_unmanaged,
            commands::repo::repo_adopt,
            commands::repo::repo_git_op,
            commands::repo::repo_branches,
            commands::repo::repo_stashes,
            commands::repo::repo_pull_refs,
            commands::repo::repo_check,
            commands::repo::repo_run_offered_steps,
            commands::wp_install::wp_install_job,
            commands::wp_install::wp_install_cancel,
            commands::wp_install::wp_install_active,
            commands::site_provision::site_provision_job,
            commands::site_provision::site_provision_retry,
            commands::site_provision::site_provision_cancel,
            commands::site_provision::site_provision_active,
            commands::repo::repo_scripts,
            commands::repo::repo_script_job,
            commands::repo::repo_dist_archive,
            commands::repo::repo_watch_start,
            commands::repo::repo_watch_stop,
            commands::repo::repo_watches,
            commands::repo::repo_watch_log,
            commands::repo::repo_link,
            commands::repo::repo_tools,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            match event {
                // Quitting stops live public shares (tunnels die with the
                // app), so a quit with shares up pauses ONCE for a native
                // confirm naming the count — inform, don't obstruct: no
                // shares means no dialog, ever. THE gate for every quit:
                // `AppHandle::exit` raises this event, so the tray's Quit
                // arrives here too. Closing the window does not — it hides,
                // and hiding stops nothing worth confirming.
                tauri::RunEvent::ExitRequested { api, .. } => {
                    if !commands::tunnels::confirm_quit_or_prompt(app) {
                        api.prevent_exit();
                        // A "Keep sharing" with a swapped build waiting IS the user choosing to
                        // keep rexenv running — the one moment the About card may say so
                        // (ledger #771). An ordinary quit declined arms nothing and says nothing.
                        if relaunch_armed() {
                            core::app_update::note_restart_declined();
                        }
                    }
                }
                // Repo install/build jobs AND tunnels die WITH the app
                // (deliberate opposite of services-outlive-the-app: jobs are
                // interactive actions — an orphaned npm would keep writing
                // into wp-content — and a tunnel outliving the app serves the
                // PUBLIC unattended; lifecycle ruling 28 Jul 2026). Crash
                // paths bypass this hook entirely — the launch sweep
                // (core::tunnels::sweep_startup) is the other half.
                tauri::RunEvent::Exit => {
                    // Linux: tear the webviews down BEFORE the process exits. WebKitGTK's
                    // web process lives on after its UI process is gone and died SIGSEGV
                    // (apport: "WebKitWebProcess closed unexpectedly") the moment the OLD
                    // app left through this gate on 1 of 2 in-app updates (22.04 VM, 29
                    // Sep 2026); an ordinary `exit(0)` with the window still up gives it
                    // no orderly shutdown. Destroying the windows first does — the
                    // mitigation the row asked to decide on (#757); the next sighting's
                    // `.crash` says whether it was enough. macOS and Windows tear their
                    // webviews down with the process and never showed it.
                    #[cfg(target_os = "linux")]
                    for (_, window) in app.webview_windows() {
                        let _ = window.destroy();
                    }
                    commands::repo::cancel_all_on_exit(app);
                    commands::tunnels::kill_all_on_exit(app);
                    // A self-update quits and reopens, and THIS is where the
                    // reopen is arranged — after the gate above has already let
                    // the quit through. Spawning it any earlier would leave a
                    // helper waiting on a pid that a cancelled quit keeps alive.
                    // The helper waits for this pid to be gone before it opens
                    // the bundle, which is what keeps the single-instance socket
                    // out of the race `AppHandle::restart` would create.
                    let pending = RELAUNCH_AFTER_EXIT.lock().ok().and_then(|mut s| s.take());
                    if let Some(bundle) = pending {
                        let platform = platform::current();
                        match platform.app_bundle().spawn_relauncher(&bundle) {
                            Ok(()) => log::info!(
                                "app update: reopening {} once this process exits",
                                bundle.display()
                            ),
                            // Nothing is lost: the update is installed, and the
                            // next ordinary launch runs it.
                            Err(e) => log::warn!(
                                "app update: could not spawn the relauncher ({e}) — rexenv \
                                 is updated and will run the new build the next time it opens"
                            ),
                        }
                    }
                }
                _ => {}
            }
        });
}

/// `rexenv.log`'s stem — the file `core::logs` offers in the Logs tab. One
/// constant, because a viewer pointed at a name nothing writes is a tab that is
/// permanently empty and says nothing about why.
pub const APP_LOG_STEM: &str = "rexenv";

/// Where the app's own `log::` output goes.
#[derive(Debug, PartialEq, Eq)]
enum LogSink {
    /// A directory; the plugin writes `<APP_LOG_STEM>.log` inside it.
    File(std::path::PathBuf),
    Stdout,
}

/// The sinks this build installs — **a value, not a condition inside `setup()`**,
/// because the bug it replaced could not be reached without a running Tauri app.
///
/// The file is unconditional. It used to be the whole plugin that was
/// `if cfg!(debug_assertions)`, so an INSTALLED rexenv wrote `log::info!` and
/// `log::warn!` nowhere at all: the adoption count, the DNS fallback, and both
/// "skipped the sweep" warnings that are the cache GC's safety valves were
/// dev-only. Every bug report from a release build arrived without the one
/// artefact that would have explained it.
///
/// `debug` adds stdout on top; it never takes the file away. A terminal is a
/// convenience for whoever is watching one — the file is what exists afterwards.
fn log_sinks(log_dir: Option<std::path::PathBuf>, debug: bool) -> Vec<LogSink> {
    let mut sinks: Vec<LogSink> = log_dir.map(LogSink::File).into_iter().collect();
    if debug {
        sinks.push(LogSink::Stdout);
    }
    sinks
}

/// The menu-bar template icon, embedded rather than read from disk — a bundled
/// app has no `icons/` directory beside the binary. Derived from the app icon
/// by `scripts/make-menubar-icon.py`, never hand-drawn: a second mark drifts
/// from the first the day the brand changes.
#[cfg(target_os = "macos")]
const MENUBAR_ICON: &[u8] = include_bytes!("../icons/menubar.png");

/// Windows' tray icon: the colour app icon. The notification area draws an icon as it is, so macOS's
/// template glyph would be a black mark on a dark taskbar (W7 S5, owner's ruling Q2, ledger #624).
#[cfg(windows)]
const WINDOWS_TRAY_ICON: &[u8] = include_bytes!("../icons/32x32.png");

/// Linux's tray icon: the colour app icon too, for the Windows reason — an appindicator draws an icon
/// as it is, and Ubuntu's top bar is dark, so the template glyph would vanish into it
/// (docs/PLAN-linux-port.md L4; unmeasured on a desktop yet).
#[cfg(target_os = "linux")]
const LINUX_TRAY_ICON: &[u8] = include_bytes!("../icons/32x32.png");

/// Install the menu-bar status item.
///
/// Clicking it opens the menu (Herd's shape, and the one this feature was asked
/// for) — the window is reached through the menu's "Open rexenv", not by
/// clicking the icon. `icon_as_template` is what lets macOS tint the glyph for
/// a light or a dark menu bar; without it the icon is drawn as-is and is
/// invisible against one of the two.
fn install_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::image::Image;
    use tauri::tray::TrayIconBuilder;

    // The status item goes up EARLY — it is the app's whole presence, and a menu
    // bar showing nothing while the app opens databases and adopts services
    // looks like a launch that failed. State does not exist yet at this point
    // (`app.manage` is much further down `setup`, and asking for it here
    // aborted the process: "state() called before manage()"), so the first menu
    // is the bootstrap one: Open and Quit, and no claim about anything else.
    // `refresh_tray` right after `manage` swaps in the real menu.
    let spec = tray_model(app).map_or_else(core::tray::bootstrap, |m| core::tray::build(&m));
    let menu = render_menu(app, &spec)?;
    *last_menu().lock().unwrap_or_else(|e| e.into_inner()) = Some((spec, menu.clone()));

    // macOS: the template glyph, tinted for a light or a dark menu bar, and the menu on any click.
    #[cfg(target_os = "macos")]
    let builder = TrayIconBuilder::with_id(TRAY_ID).icon(Image::from_bytes(MENUBAR_ICON)?).icon_as_template(true);
    // Linux: the colour icon, and the menu on any click — an appindicator has no click event of its
    // own, so the menu IS the interaction (docs/PLAN-linux-port.md L4).
    #[cfg(target_os = "linux")]
    let builder = TrayIconBuilder::with_id(TRAY_ID).icon(Image::from_bytes(LINUX_TRAY_ICON)?);
    // Windows (W7 S5, owner's ruling Q2, ledger #624): the colour icon, and the Windows convention for the
    // clicks — the window on a LEFT click, the menu on a RIGHT one. Acting on the release, as a button does.
    #[cfg(windows)]
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(WINDOWS_TRAY_ICON)?)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    builder
        .tooltip("rexenv")
        .menu(&menu)
        .on_menu_event(|app, event| on_tray_click(app, event.id().as_ref()))
        .build(app)?;

    // Keep it current. ~5s, coalesced. macOS closes an open menu when its items
    // are REPLACED, so the tick almost never replaces them: `refresh_tray`
    // skips a spec that reads the same and EDITS the live items when only the
    // numbers moved — an unconditional rebuild slammed the menu shut under the
    // cursor of anyone who held it open for five seconds, which is how this was
    // reported. The tick is also the ONLY trigger: hooking every
    // path that can change a service state means every one of them must
    // remember, which is the "whole-surface claim that checks one place"
    // failure this project keeps a ledger about. Five seconds of staleness in
    // a menu nobody is looking at costs nothing; a missed hook costs a menu
    // that is wrong for as long as it is open.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            refresh_tray(&handle);
        }
    });
    Ok(())
}

/// Should a LOGIN launch open its window anyway?
///
/// `--hidden` is a request, not a command. rexenv starting quietly in the menu
/// bar is the point of the flag — but a machine where first-run setup is
/// unfinished cannot resolve `.rex` or trust the local CA, so every site is
/// broken, and a silent tray icon on that machine is an app that looks dead
/// while hiding the one screen that fixes it. Onboarding is judged on exactly
/// the two facts `FirstRunGate` routes on — the OS resolver file and per-user
/// CA trust — so the tray and the frontend cannot disagree about whether this
/// machine is set up.
fn first_window_decision(app: &tauri::AppHandle, needs_window: bool) {
    use tauri::Manager;
    if app.try_state::<state::app::AppState>().is_none() {
        // No state means init failed; that path shows the window itself.
        return;
    }
    if !needs_window {
        log::info!("launched at login — staying in the {}", crate::platform::words::current().tray_home);
        return;
    }
    log::info!("launched at login with setup incomplete — showing the window");
    show_main_window(app);
}

/// The two facts a login launch needs before it may stay silent in the menu
/// bar: `.rex` resolves here, and the CA is trusted. Read from the platform,
/// not from `AppState`, because the activation policy has to be decided
/// BEFORE state exists — and a login launch on a machine that cannot resolve
/// `.rex` must show the window rather than look broken and hide the fix (A7).
fn login_launch_needs_window(platform: &dyn platform::traits::Platform) -> bool {
    let resolver_installed = platform.dns().route_owner(core::tld::BACKBONE_TLD, core::dns::DEFAULT_DNS_PORT)
        != core::dns::ResolverOwner::Absent;
    let ca_trusted = core::ssl::ca_dir(platform.paths())
        .map(|dir| dir.join(core::ssl::CA_CERT_FILE))
        .is_ok_and(|cert| cert.exists() && platform.cert_trust().is_trusted(&cert));
    if !(resolver_installed && ca_trusted) {
        log::info!(
            "login launch with setup incomplete (resolver: {resolver_installed}, CA trusted: \
             {ca_trusted}) — the window will be shown"
        );
    }
    !(resolver_installed && ca_trusted)
}

/// Give the DNS port back to the agent when we only hold it because we won a
/// race at login.
///
/// **The bug this exists for, measured 1 Sep 2026 on a real logout/login.** The
/// resolver is meant to OUTLIVE the app: it lives in a per-user LaunchAgent, and
/// the in-process resolver is a fallback that dies with the process. That
/// fallback used to be unreachable in practice, because the app was launched by
/// hand long after login, by which time the agent already answered. "Start on
/// login" made both start together — and the startup probe waits 2s, less than a
/// cold agent needs (it is this entire binary booting into `--dns-agent`). So the
/// app bound the port itself, and the agent — which retries every 10s — could
/// never win it back. The result was DNS dying with the app on every machine
/// that uses the feature Phase C shipped.
///
/// **Why a handoff and not a longer wait.** A bigger startup timeout is a guess
/// that taxes every launch and still loses on a slow one; the race is not
/// something to win, it is something to undo afterwards. So: stop our resolver,
/// KICKSTART the agent so its bind happens now rather than on its own 10s
/// cadence, and probe. If it answers, we are done for good. If it does not,
/// rebind in-process and try again later.
///
/// **The cost is a short gap** with nobody serving `.rex` — bounded by the probe
/// window, and paid at most `ATTEMPTS` times. That is the honest trade: a few
/// seconds of no resolution shortly after login, against DNS that otherwise dies
/// with the app for the rest of the session. It gives up loudly rather than
/// looping forever, because a machine where the agent cannot bind at all has a
/// different problem and should say so once.
/// The bundle a completed swap wants reopened, set by `app_update_apply` and
/// read by the exit hook.
///
/// A cell rather than an argument because the two ends are a command and a
/// runtime event with nothing between them. It is read in `RunEvent::Exit` —
/// AFTER the quit gate has already agreed — so a user who answers "Keep
/// sharing" to the confirm leaves no helper waiting on a pid that is not going
/// to die, and the swapped bundle simply takes effect at the next ordinary
/// launch.
static RELAUNCH_AFTER_EXIT: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

/// Record that this process, when it exits, should be reopened at `bundle`.
pub fn relaunch_after_exit(bundle: std::path::PathBuf) {
    if let Ok(mut slot) = RELAUNCH_AFTER_EXIT.lock() {
        *slot = Some(bundle);
    }
}

/// Is a reopen armed — i.e. is the quit being asked for the one that opens a swapped build?
/// Read by the quit gate: a declined quit counts as "keep rexenv running" only then (#771).
pub fn relaunch_armed() -> bool {
    RELAUNCH_AFTER_EXIT.lock().map(|s| s.is_some()).unwrap_or(false)
}

/// Where a SWAPPED build is waiting, between the swap and the user agreeing to
/// the restart.
///
/// The apply no longer quits on its own: it swaps, and then the app TELLS the
/// user it is about to quit and reopen and waits for them to say go (owner
/// ruling 20 Sep 2026 — "the app must not restart itself without asking").
/// Nothing here arms the relaunch; [`relaunch_after_exit`] does, and only once
/// the user has clicked through, so an app that is never clicked simply keeps
/// running and the swapped build takes effect at the next ordinary launch.
static SWAPPED_AWAITING_RESTART: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

/// Record that a swap landed at `bundle` and the restart is owed to the user.
pub fn swapped_awaiting_restart(bundle: std::path::PathBuf) {
    if let Ok(mut slot) = SWAPPED_AWAITING_RESTART.lock() {
        *slot = Some(bundle);
    }
}

/// Take the swapped bundle, if this process has one waiting. `None` means no
/// apply has completed in this process — a restart command must then refuse
/// rather than quit an app that has nothing new to reopen into.
pub fn take_swapped_awaiting_restart() -> Option<std::path::PathBuf> {
    SWAPPED_AWAITING_RESTART.lock().ok()?.take()
}

/// One app-update check: fetch unlocked, accept + record under a brief lock.
///
/// Best-effort by contract, exactly like the PHP manifest poll above it — no key
/// pinned, no network, a stale serial or a bad signature all leave the app
/// resolving exactly what it resolves today, and every one of them is a log line
/// rather than anything a user is shown. The timestamp is written only on the
/// success path, so `checked N ago` can never age a failure into a success.
async fn check_for_app_update(state: &state::app::AppState) {
    let variant = platform::current().app_bundle().descriptor_variant();
    match core::app_update::fetch(variant.as_deref(), core::updates::BACKGROUND_DEADLINE).await {
        Ok((doc, sig)) => {
            let Ok(conn) = state.db.lock() else {
                log::warn!("app update: descriptor not stored — db lock");
                return;
            };
            let document = core::app_update::this_document(variant.as_deref());
            match core::app_update::accept(&conn, &document, &doc, sig.trim()) {
                Ok(m) => {
                    let st = core::app_update::state(&conn, &document);
                    if let Err(e) = core::app_update::store_check(&conn, st.offered.clone()) {
                        log::warn!("app update: could not record the check: {e}");
                    }
                    match (&st.offered, &st.no_offer_reason) {
                        (Some(o), _) => log::info!(
                            "app update: {} can be installed (serial {}, {} bytes)",
                            o.version,
                            m.serial,
                            o.size_bytes
                        ),
                        (None, Some(why)) => {
                            log::info!("app update: nothing to offer — {why}")
                        }
                        (None, None) => log::info!("app update: nothing to offer"),
                    }
                }
                Err(e) => log::info!("app update: descriptor not accepted: {e}"),
            }
        }
        Err(e) => log::info!("app update: check skipped: {e}"),
    }
}

/// Re-check every 6 hours, so a machine left running for a week still hears
/// about a release.
///
/// A `sleep` loop rather than an interval, matching every other periodic task in
/// this file, and it re-reads the setting on every tick — turning automatic
/// checking off must stop the NEXT request, not just hide the answer. The first
/// tick sleeps first: the launch sweep has already checked by then.
fn spawn_app_update_poller(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(6 * 60 * 60)).await;
            let Some(state) = app.try_state::<state::app::AppState>() else { continue };
            let auto = state
                .db
                .lock()
                .ok()
                .map(|c| core::app_update::auto_check_enabled(&c))
                .unwrap_or(false);
            if auto {
                check_for_app_update(&state).await;
            }
        }
    });
}

fn spawn_dns_handoff(app: tauri::AppHandle, port: u16) {
    use tauri::Manager;
    const ATTEMPTS: u32 = 5;
    const BETWEEN: std::time::Duration = std::time::Duration::from_secs(20);
    const PROBE_EVERY: std::time::Duration = std::time::Duration::from_millis(250);
    const PROBE_TRIES: u32 = 24; // ~6s, comfortably past a cold agent's bind

    tauri::async_runtime::spawn(async move {
        for attempt in 1..=ATTEMPTS {
            tokio::time::sleep(BETWEEN).await;

            let Some(dns) = app.try_state::<state::app::DnsState>() else {
                return;
            };
            // Someone else already settled it (the watchdog, a later probe).
            if !matches!(dns.mode(), state::app::DnsMode::InProcess) {
                return;
            }
            let Some(state) = app.try_state::<state::app::AppState>() else {
                return;
            };
            let agent = state.platform.dns_agent();
            if !agent.is_installed() {
                log::warn!("dns: no resolver agent installed — staying in-process");
                return;
            }

            // Release the port. Taking the service OUT of the state (rather than
            // stopping it in place) is what makes the failure path honest: while
            // the handoff is in flight, `DnsState` says the in-process resolver
            // is not running, because it is not.
            // `shutdown` AWAITS the aborted task: a plain drop only requests
            // the abort, and the socket lives until the scheduler gets to the
            // task — so the agent, once kicked, could still lose its first bind
            // and sleep 10s, longer than the probe window below.
            if let Some(service) = dns.take_service() {
                service.shutdown().await;
            }

            // Kickstart rather than wait for the agent's own 10s retry: it turns
            // a gap measured in cadence into one measured in startup.
            if let Err(e) = agent.kickstart() {
                log::warn!("dns: could not kickstart the resolver agent: {e}");
            }

            let mut handed_off = false;
            for _ in 0..PROBE_TRIES {
                tokio::time::sleep(PROBE_EVERY).await;
                if core::dns::answers_as_ours(port) {
                    handed_off = true;
                    break;
                }
            }
            if handed_off {
                dns.set(None, state::app::DnsMode::Agent);
                log::info!(
                    "dns: handed the resolver back to the agent (attempt {attempt}) — \
                     name resolution now survives app quits"
                );
                return;
            }

            // The agent did not take it. Serve again ourselves rather than leave
            // the machine with no resolver at all.
            match core::dns::DnsService::start(port).await {
                Ok(svc) => dns.set(Some(svc), state::app::DnsMode::InProcess),
                Err(e) => {
                    // "Address in use" here almost always means the agent took
                    // the port just after the last probe — the handoff
                    // SUCCEEDED late. Latching `Down` on that (the first
                    // version did) reported a dead resolver the watchdog then
                    // never re-examined, while `.rex` resolved fine.
                    if core::dns::answers_as_ours(port) {
                        dns.set(None, state::app::DnsMode::Agent);
                        log::info!(
                            "dns: the resolver agent took the port after the probe window \
                             (attempt {attempt}) — handed off"
                        );
                    } else {
                        dns.set(None, state::app::DnsMode::Down);
                        log::error!("dns: could not rebind in-process after a handoff attempt: {e}");
                    }
                    return;
                }
            }
        }
        log::warn!(
            "dns: the resolver agent never took the port after {ATTEMPTS} handoff attempts — \
             staying in-process (sites will stop resolving shortly after the app quits)"
        );
    });
}

/// The flag the login LaunchAgent passes (`platform::macos` writes it into the
/// plist, `setup` reads it here). One constant because it is a contract between
/// a file on disk and a process: a typo in either half is an app that opens a
/// window at every login, which is the entire thing this flag exists to stop.
///
/// It is a REQUEST, not a command — see `first_window_decision`.
pub const HIDDEN_LAUNCH_FLAG: &str = "--hidden";

/// Whether THIS process was started as a login launch: its own `--hidden`, or — on Windows — a
/// `--hidden` launch that hopped out of a confining job through Explorer, which carries no
/// arguments (`platform::hopped_hidden`, ledger #742). Decided once: the hop's marker is consumed
/// on first read, and both the pipe claim and `setup` ask.
pub(crate) fn is_hidden_launch() -> bool {
    static HIDDEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *HIDDEN.get_or_init(|| std::env::args().any(|a| a == HIDDEN_LAUNCH_FLAG) || platform::hopped_hidden())
}

/// **Login-start runs once per process, for either of the two ways a login reaches it.**
///
/// 1. This process IS the login launch (`--hidden`) — `setup` says so when it is ready.
/// 2. A login launch found this process already holding the lock and HANDED OFF to it
///    (`cli_server::handoff_request(true)`). Measured 29 Sep 2026 on the 15.8 VM with the
///    drafted 0.8.10: a reboot with rexenv open → macOS's "Reopen windows when logging back
///    in" (the restart dialog's default) relaunched rexenv WITHOUT `--hidden`, the LaunchAgent's
///    `--hidden` launch found it running, sent a bare `app.open` and exited — and nothing
///    started. The one toggle's whole promise failed on the most ordinary reboot there is.
///
/// A handoff can arrive before `setup` has the state login-start needs (the socket is claimed
/// before Tauri boots), so a request made early is held until `ready`. Both sides store their
/// fact, then read the other's (SeqCst): whichever lands second sees both, so a login is never
/// lost between them, and `ran` makes a second run impossible.
pub(crate) struct LoginStartGate {
    requested: AtomicBool,
    ready: AtomicBool,
    ran: AtomicBool,
}

impl LoginStartGate {
    const fn new() -> Self {
        Self { requested: AtomicBool::new(false), ready: AtomicBool::new(false), ran: AtomicBool::new(false) }
    }

    /// A handed-off login asks for Start all. True = run it now.
    pub(crate) fn request(&self) -> bool {
        self.requested.store(true, Ordering::SeqCst);
        self.ready.load(Ordering::SeqCst) && self.claim()
    }

    /// `setup` reached the point login-start may run; `login` = this launch was `--hidden`.
    /// True = run it now.
    pub(crate) fn ready(&self, login: bool) -> bool {
        if login {
            self.requested.store(true, Ordering::SeqCst);
        }
        self.ready.store(true, Ordering::SeqCst);
        self.requested.load(Ordering::SeqCst) && self.claim()
    }

    fn claim(&self) -> bool {
        !self.ran.swap(true, Ordering::SeqCst)
    }
}

static LOGIN_START: LoginStartGate = LoginStartGate::new();

/// A `--hidden` launch handed its login to this process (`cli_server`'s `app.open` arm).
pub(crate) fn request_login_start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if LOGIN_START.request() {
        run_login_start(app);
    }
}

fn run_login_start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let auto = app.clone();
    tauri::async_runtime::spawn(async move {
        commands::services::auto_start_services(auto).await;
    });
}

/// The setting that used to say whether ANY launch ran Start all — retired 29 Sep 2026
/// when it was folded into the login toggle (a `--hidden` launch starts the stack, a
/// user's launch does not). Named only so the launch sweep can delete a leftover row.
const RETIRED_AUTO_START_SETTING: &str = "start_services_on_launch";

/// The tray icon's id — also how `refresh_tray` finds it again.
const TRAY_ID: &str = "main";

/// The last spec rendered AND the menu it produced — so a tick can (a) skip a
/// menu that would read identically and (b) EDIT one that differs only in its
/// numbers, instead of replacing it.
///
/// The menu handle is kept for the second half: macOS closes an open menu the
/// moment its items are replaced, and Tauri's `TrayIcon` has no getter for the
/// menu it was given, so the only way to reach those items again is to have
/// kept them. `Mutex` rather than app state: the tray is installed before
/// anything can ask for it, and this is the only reader.
type LastMenu = Option<(core::tray::MenuSpec, tauri::menu::Menu<tauri::Wry>)>;
fn last_menu() -> &'static std::sync::Mutex<LastMenu> {
    static LAST: std::sync::OnceLock<std::sync::Mutex<LastMenu>> = std::sync::OnceLock::new();
    LAST.get_or_init(|| std::sync::Mutex::new(None))
}

/// Read the app's state into the menu's model.
///
/// Everything here is READ from where the UI reads it — the same
/// `service_infos` snapshot the Services screen and the footer use, the same
/// `summarize`, the same sites table, the same settings row. The tray measures
/// nothing of its own (`docs/archive/PLAN-menubar-tray.md` §3 rule 1).
fn tray_model(app: &tauri::AppHandle) -> Option<core::tray::TrayModel> {
    use tauri::Manager;
    // `try_state`, never `state`: the tray is installed before `setup` manages
    // the app state, and `state()` does not return an error there — it ABORTS
    // the process (non-unwinding panic). `None` means "not yet", and the caller
    // shows a menu that claims nothing.
    let state = app.try_state::<state::app::AppState>()?;

    // try_lock snapshot + freshness — never blocks on the services lock (rule
    // 2). A busy lock yields the previous rows, and `stale` makes the menu say
    // so instead of presenting them as current.
    let (infos, fresh) = state.service_infos_fresh();
    let (running, total, summary) =
        commands::system::summarize(
            &infos.iter().map(|i| (i.running, i.optional)).collect::<Vec<(bool, bool)>>(),
        );

    // Sites and the MCP flag share ONE brief DB lock, released here — nothing
    // below spawns or waits while it is held.
    let (sites, mcp_on) = {
        match state.db.lock() {
            Ok(conn) => {
                let sites = core::sites::list(&conn)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| core::tray::TraySite { domain: s.domain })
                    .collect();
                let mcp_on = matches!(
                    state::store::get_setting(&conn, mcp_server::MCP_ENABLED_KEY),
                    Ok(Some(v)) if v == "true"
                );
                (sites, mcp_on)
            }
            // A poisoned DB lock is not a reason to lose the menu bar: the
            // menu still opens the window and still quits, which are the two
            // items that must never depend on anything.
            Err(_) => (Vec::new(), false),
        }
    };

    Some(core::tray::TrayModel {
        summary,
        running,
        total,
        sites,
        mcp_on,
        // The snapshot, never a read of the descriptor: drawing a menu must not
        // verify a signature, and the tray may not block (#437/#438).
        update: core::app_update::current_offer().map(|o| o.version),
        stale: !fresh,
    })
}

/// Turn a `MenuSpec` into a real menu. The ONLY place Tauri menu types meet the
/// tray's rules — walking a tree, nothing decided here.
fn render_menu(
    app: &tauri::AppHandle,
    spec: &core::tray::MenuSpec,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

    fn items(
        app: &tauri::AppHandle,
        entries: &[core::tray::MenuEntry],
    ) -> tauri::Result<Vec<Box<dyn IsMenuItem<tauri::Wry>>>> {
        let mut out: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::new();
        for e in entries {
            match e {
                // A label is a disabled item: information, not something
                // broken. It carries no id, so it can never be clicked into a
                // stale action.
                core::tray::MenuEntry::Label(text) => {
                    out.push(Box::new(MenuItem::new(app, text, false, None::<&str>)?))
                }
                core::tray::MenuEntry::Separator => {
                    out.push(Box::new(PredefinedMenuItem::separator(app)?))
                }
                core::tray::MenuEntry::Item { title, action, enabled, checked } => match checked {
                    Some(on) => out.push(Box::new(CheckMenuItem::with_id(
                        app,
                        action.id(),
                        title,
                        *enabled,
                        *on,
                        None::<&str>,
                    )?)),
                    None => out.push(Box::new(MenuItem::with_id(
                        app,
                        action.id(),
                        title,
                        *enabled,
                        None::<&str>,
                    )?)),
                },
                core::tray::MenuEntry::Submenu { title, entries } => {
                    let kids = items(app, entries)?;
                    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
                        kids.iter().map(|b| b.as_ref()).collect();
                    out.push(Box::new(Submenu::with_items(app, title, true, &refs)?));
                }
            }
        }
        Ok(out)
    }

    let built = items(app, &spec.entries)?;
    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = built.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(app, &refs)
}

/// Bring the tray menu up to date — by EDITING it where that is possible, and
/// only rebuilding when it is not.
///
/// Two rules, and the second one is a bug a user reported: macOS closes an open
/// menu the moment its items are replaced. The tick runs every 5s and the
/// status line moves constantly (the running count, and the `updating…` suffix
/// a busy services lock adds), so "rebuild whenever the spec differs" slammed
/// the menu shut under the cursor of anyone reading it — the menu appeared to
/// hide itself a few seconds after being opened.
///
/// So:
/// 1. identical spec → do nothing;
/// 2. same SHAPE (`MenuSpec::same_shape` — same ids, same tree) → set the
///    titles, enabled flags and checkmarks on the LIVE items, which macOS
///    renders without disturbing an open menu;
/// 3. anything else (a site added, an update offer appearing) → a real rebuild.
///    Rare, and there is no way to add a row to an open menu anyway.
fn refresh_tray(app: &tauri::AppHandle) {
    // No state yet: leave whatever is up. Replacing a menu with the bootstrap
    // one would be a menu going BACKWARDS in front of the user.
    let Some(model) = tray_model(app) else {
        return;
    };
    let spec = core::tray::build(&model);

    // Decide under the lock, act after it: applying the edits can wait on the
    // main thread (which is inside a menu-tracking loop for exactly as long as
    // the menu is open), and holding a lock across that wait is the thing this
    // project's locking rule exists to forbid.
    let previous = {
        let mut last = last_menu().lock().unwrap_or_else(|e| e.into_inner());
        match last.as_ref() {
            Some((old, _)) if old == &spec => return,
            Some((old, menu)) if old.same_shape(&spec) => {
                let menu = menu.clone();
                *last = Some((spec.clone(), menu.clone()));
                Some(menu)
            }
            _ => None,
        }
    };
    if let Some(menu) = previous {
        match edit_menu_in_place(&menu, &spec) {
            Ok(()) => return,
            // The shape said these lined up and they did not. Fall through to a
            // rebuild rather than leaving a menu half-edited — a row showing
            // one service's title over another's id is worse than a flicker.
            Err(e) => log::warn!("tray: could not edit the menu in place ({e}) — rebuilding"),
        }
    }

    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match render_menu(app, &spec) {
        // A failed rebuild leaves the PREVIOUS menu in place — the item keeps
        // working with older numbers, which is strictly better than a status
        // item with no menu at all.
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu.clone())) {
                log::warn!("tray: could not swap the menu: {e}");
                return;
            }
            *last_menu().lock().unwrap_or_else(|e| e.into_inner()) = Some((spec, menu));
        }
        Err(e) => log::warn!("tray: could not rebuild the menu: {e}"),
    }
}

/// Write a spec's editable fields onto an ALREADY RENDERED menu of the same
/// shape. Called only after `same_shape`, so the walks line up one-for-one;
/// every mismatch is still checked and returns an error, because a wrong
/// pairing here would put one row's title on another row's id.
fn edit_menu_in_place(
    menu: &tauri::menu::Menu<tauri::Wry>,
    spec: &core::tray::MenuSpec,
) -> Result<(), String> {
    use tauri::menu::MenuItemKind;

    fn rows(
        kinds: Vec<MenuItemKind<tauri::Wry>>,
        out: &mut Vec<MenuItemKind<tauri::Wry>>,
    ) -> tauri::Result<()> {
        for k in kinds {
            match k {
                // Separators carry nothing that can change; a submenu's own
                // row is shape, and `MenuSpec::editable` skips both. Its
                // children are walked in place, which is the same pre-order.
                MenuItemKind::Predefined(_) => {}
                MenuItemKind::Submenu(sub) => rows(sub.items()?, out)?,
                other => out.push(other),
            }
        }
        Ok(())
    }

    let mut live = Vec::new();
    rows(menu.items().map_err(|e| e.to_string())?, &mut live).map_err(|e| e.to_string())?;
    let want = spec.editable();
    if live.len() != want.len() {
        return Err(format!("{} live rows vs {} in the spec", live.len(), want.len()));
    }
    for (row, entry) in live.iter().zip(want) {
        match (row, entry) {
            (MenuItemKind::MenuItem(it), core::tray::Editable::Label(text)) => {
                it.set_text(text).map_err(|e| e.to_string())?;
            }
            (
                MenuItemKind::MenuItem(it),
                core::tray::Editable::Item { title, enabled, checked: None },
            ) => {
                it.set_text(title).map_err(|e| e.to_string())?;
                it.set_enabled(enabled).map_err(|e| e.to_string())?;
            }
            (
                MenuItemKind::Check(it),
                core::tray::Editable::Item { title, enabled, checked: Some(on) },
            ) => {
                it.set_text(title).map_err(|e| e.to_string())?;
                it.set_enabled(enabled).map_err(|e| e.to_string())?;
                it.set_checked(on).map_err(|e| e.to_string())?;
            }
            _ => return Err("a live row and its spec entry are of different kinds".into()),
        }
    }
    Ok(())
}

/// Dispatch a click. Every arm goes through the SAME entry point the UI uses —
/// `start_services`/`stop_services`, `open_external`, the settings row — so the
/// tray can never become a second way of doing something with different rules.
fn on_tray_click(app: &tauri::AppHandle, id: &str) {
    use tauri::{Emitter, Manager};
    let Some(action) = core::tray::TrayAction::parse(id) else {
        // Unknown id does NOTHING (never a default action): the menu is rebuilt
        // on a timer, so a click can land on an item that no longer exists.
        log::warn!("tray: unknown menu id {id}");
        return;
    };
    match action {
        core::tray::TrayAction::Open => show_main_window(app),
        // The ONLY way out of the app now that closing the window hides it.
        // Deliberately just `exit`: the share confirm lives on
        // `RunEvent::ExitRequested`, which `exit` raises, so quitting passes
        // through ONE gate no matter who asked — the tray, Cmd+Q, or a `rex`
        // command. Calling the gate here as well would be a second copy of a
        // rule that must not be able to differ.
        core::tray::TrayAction::Quit => app.exit(0),
        core::tray::TrayAction::StartAll | core::tray::TrayAction::StopAll => {
            let start = matches!(action, core::tray::TrayAction::StartAll);
            let handle = app.clone();
            // Off the menu thread: start_all can download binaries and stop_all
            // can sit on a privileged prompt. A menu click that blocks is a
            // beachball on the menu bar itself.
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<state::app::AppState>();
                let result = if start {
                    commands::services::start_services(state).await
                } else {
                    commands::services::stop_services(state, handle.state()).await
                };
                if let Err(e) = result {
                    // No window may exist to show a toast in, so the log is the
                    // surface — and the menu's own status line is the other
                    // half of the answer, since it will show what actually came
                    // up within the next tick.
                    log::warn!("tray: {} all failed: {e}", if start { "start" } else { "stop" });
                }
                refresh_tray(&handle);
            });
        }
        core::tray::TrayAction::OpenSite(domain) => {
            let state = app.state::<state::app::AppState>();
            // `open_external`, not a raw shell open: the browser preference is
            // applied in ONE place for all dozen call sites, and a tray that
            // opened LaunchServices directly would be the thirteenth that
            // forgot.
            if let Err(e) = commands::system::open_external(state, format!("https://{domain}")) {
                log::warn!("tray: could not open {domain}: {e}");
            }
        }
        core::tray::TrayAction::Route(route) => {
            // Show the window FIRST, then ask the frontend to navigate: the
            // event needs a webview to arrive in.
            show_main_window(app);
            if let Err(e) = app.emit(TRAY_ROUTE_EVENT, route.path()) {
                log::warn!("tray: could not route to {}: {e}", route.path());
            }
        }
        // The tray OPENS the update; it never installs one. The consent
        // sentence and the button live in one place, and a menu item that
        // skipped them would be a second path to the one action in this app
        // that replaces the app itself.
        core::tray::TrayAction::UpdateTo(_) => {
            show_main_window(app);
            if let Err(e) = app.emit(TRAY_ROUTE_EVENT, "/settings?section=about") {
                log::warn!("tray: could not route to the update card: {e}");
            }
        }
        // The app menu's About, reachable without the app menu: with the
        // window closed rexenv is Accessory and has no menu bar of its own, so
        // the tray is the ONLY way to the version and the licences. Same event
        // as the app-menu item — one About screen, two doors.
        core::tray::TrayAction::About => {
            show_main_window(app);
            if let Err(e) = app.emit(ABOUT_MENU_EVENT, ()) {
                log::warn!("tray: could not open About: {e}");
            }
        }
        core::tray::TrayAction::ToggleMcp => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<state::app::AppState>();
                // The command, not a raw settings write: turning MCP on BINDS a
                // socket and turning it off UNBINDS one. A tray that only
                // flipped the row would leave the checkmark and the listener
                // disagreeing — the exact shape `MCP_ENABLED_KEY`'s own doc
                // warns about ("the toggle can never read on while nothing
                // listens").
                // Read the CURRENT value rather than the rendered checkmark:
                // the menu may have been built seconds ago, and toggling from
                // a stale checkmark writes the state the user already has.
                let Some(on) = tray_model(&handle).map(|m| m.mcp_on) else {
                    return;
                };
                if let Err(e) = commands::mcp::mcp_set_enabled(handle.clone(), state, !on) {
                    log::warn!("tray: could not toggle the MCP server: {e}");
                }
                refresh_tray(&handle);
            });
        }
    }
}

/// Event the frontend listens for to navigate from a tray menu item. The
/// payload is the route path.
pub const TRAY_ROUTE_EVENT: &str = "tray://route";

/// Dock tile on while a window is up, off while it is not.
///
/// Two different apps live in one process: with a window on screen rexenv is an
/// ordinary Mac app (dock tile, Cmd-Tab, an application menu), and with the
/// window closed it is a background service whose whole presence is the
/// menu-bar item. The activation policy is what says which, and it is switched
/// on exactly two edges — showing the window and closing it.
///
/// The plan wrote this down as the FALLBACK if an accessory app cost the
/// webview its clipboard. It did not (A9 measured that), so pure Accessory
/// shipped first; this arrived for a different reason — a visible window with
/// nothing in the dock cannot be Cmd-Tabbed to and reads as a window belonging
/// to no app.
///
/// A failure is logged, never fatal: the wrong dock state is a cosmetic fault,
/// and refusing to show a window over it would not be.
#[cfg(target_os = "macos")]
fn dock_follows_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>, window_up: bool) {
    let policy = if window_up {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    if let Err(e) = app.set_activation_policy(policy) {
        log::warn!("tray: could not set the activation policy: {e}");
    }
}

/// Bring the main window back: activate the app, then show, un-minimise and
/// focus. All four, because a window can be hidden AND minimised, and an
/// accessory app that merely shows one has not come to the front.
pub(crate) fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        log::warn!("tray: no main window to show");
        return;
    };
    // Take the dock tile back BEFORE the window appears: a window shown while
    // the app is still Accessory would be a window the Regular switch then
    // hides (measured — see the policy call in `setup`), and a visible window
    // with no tile cannot be Cmd-Tabbed to.
    #[cfg(target_os = "macos")]
    dock_follows_window(app, true);
    // Activate FIRST: an accessory app is not made active by showing a window,
    // so without this the window comes up behind whatever the developer was
    // reading — which reads as a menu item that did nothing.
    #[cfg(target_os = "macos")]
    platform::activate_app();
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Event the frontend listens for to open Settings → About.
///
/// Not macOS-gated even though the app-menu item that first raised it is: the
/// tray's own **About rexenv** raises the SAME event on every platform, because
/// two ways in must not become two About screens.
pub const ABOUT_MENU_EVENT: &str = "menu://about";

/// The app menu's "Check for Updates…" — id and event.
///
/// Separate from the About item because it does something as well as going
/// somewhere: the frontend navigates to the card AND runs a check, so a user who
/// came to the menu bar asking gets an answer rather than a screen.
#[cfg(target_os = "macos")]
const CHECK_UPDATES_MENU_ID: &str = "rex-check-updates";
pub const CHECK_UPDATES_MENU_EVENT: &str = "menu://check-updates";

/// Swap the macOS app menu's predefined About item for one that opens the
/// app's own About screen.
///
/// Edits the DEFAULT menu in place (remove index 0, insert ours) so the rest of
/// it — Services/Hide/Quit and the whole Edit menu with Cmd-C/V/Z — survives;
/// rebuilding a menu from scratch is how apps lose the clipboard shortcuts they
/// never wrote.
///
/// The `cfg` sits on the FUNCTION. It used to sit between this doc comment and
/// the Quit id's, which gated the const and left the function compiling on every
/// OS — so a Windows build failed on the id it could no longer see
/// (docs/PLAN-windows-port.md §2.1).
#[cfg(target_os = "macos")]
fn install_about_menu_item(app: &tauri::AppHandle) -> tauri::Result<()> {
    // The custom Quit item's id — custom so that Cmd+Q raises `ExitRequested`
    // like every other quit, instead of `terminate:`-ing straight past the gate.
    // Scoped to its only user, so no attribute can separate the two again.
    const QUIT_MENU_ID: &str = "rex-quit";
    use tauri::menu::{Menu, MenuItem, MenuItemKind};
    use tauri::Emitter;

    let menu = Menu::default(app)?;
    let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() else {
        // No app submenu (should not happen on macOS) — leave the default menu
        // alone rather than shipping a half-edited one.
        log::warn!("menu: no app submenu — keeping the default About item");
        return Ok(());
    };
    let about = MenuItem::with_id(app, "rex-about", "About rexenv", true, None::<&str>)?;
    app_menu.remove_at(0)?;
    app_menu.insert(&about, 0)?;
    // "Check for Updates…" where every Mac app puts it: right under About. The
    // card can be reached from the tray and from Settings, but this is the item
    // a person looks for when they came to the menu bar ASKING — and it costs
    // one line, where its absence costs a support question.
    let check = MenuItem::with_id(app, CHECK_UPDATES_MENU_ID, "Check for Updates…", true, None::<&str>)?;
    app_menu.insert(&check, 1)?;
    // Cmd+Q goes through THE quit gate. The predefined Quit item is
    // `terminate:` — the app delegate ends the process and `ExitRequested`
    // is never raised, so the public-share confirm on that event (the "ONE
    // gate every quit passes through", ledger #436) covered the tray's Quit
    // and `rex`, and NOT the keyboard: with a share up, Cmd+Q killed the
    // tunnel with no prompt (review, 3 Sep 2026). A custom item with the
    // same accelerator calls `exit`, which raises the event.
    let items = app_menu.items()?;
    let quit_at = items.iter().position(|item| {
        matches!(item, MenuItemKind::Predefined(p) if p.text().is_ok_and(|t| t.starts_with("Quit")))
    });
    if let Some(pos) = quit_at {
        let quit = MenuItem::with_id(app, QUIT_MENU_ID, "Quit rexenv", true, Some("CmdOrCtrl+Q"))?;
        app_menu.remove_at(pos)?;
        app_menu.insert(&quit, pos)?;
    } else {
        log::warn!("menu: no predefined Quit item to replace — Cmd+Q will bypass the share confirm");
    }
    app.set_menu(menu)?;

    app.on_menu_event(|app, event| {
        if event.id() == QUIT_MENU_ID {
            // Same call as the tray's Quit: `exit` raises `ExitRequested`,
            // where the gate lives. Not a second copy of the gate.
            app.exit(0);
            return;
        }
        if event.id() == CHECK_UPDATES_MENU_ID {
            if let Some(win) = app.get_webview_window("main") {
                // Show first: a check whose answer lands on a hidden window is
                // a menu item that appears to do nothing.
                let _ = win.show();
                let _ = win.set_focus();
                let _ = win.emit(CHECK_UPDATES_MENU_EVENT, ());
            }
            return;
        }
        if event.id() == "rex-about" {
            if let Some(win) = app.get_webview_window("main") {
                // The window may be hidden or behind: an About that opens
                // out of sight reads as a dead menu item.
                let _ = win.show();
                let _ = win.set_focus();
                let _ = win.emit(ABOUT_MENU_EVENT, ());
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// **A release build writes its own log.** The whole plugin was behind
    /// `cfg!(debug_assertions)`, and the cost was not theoretical: asked where to
    /// read one line on an installed app, the honest answer was "nowhere".
    #[test]
    fn the_app_log_file_is_written_in_every_build() {
        let dir = PathBuf::from("/tmp/rexenv-log-fixture");
        for debug in [false, true] {
            assert!(
                log_sinks(Some(dir.clone()), debug).contains(&LogSink::File(dir.clone())),
                "debug={debug}: no file sink — an installed app would log nowhere"
            );
        }
        // Stdout is the DEV extra, never the only sink.
        assert!(!log_sinks(Some(dir.clone()), false).contains(&LogSink::Stdout));
        assert_eq!(log_sinks(Some(dir.clone()), true).len(), 2);
        // An unresolvable log dir must not silently become stdout-only in a
        // release build: there is no terminal, so that is "no logging" again.
        assert!(log_sinks(None, false).is_empty());
    }

    /// **Login-start runs on a login and on nothing else.** The "Start rexenv at login" toggle
    /// installs a `--hidden` launch, so the one switch is the whole of "my sites are back after a
    /// reboot" — and an open the user made never runs Start all behind their back. A login reaches
    /// login-start two ways: this process IS the `--hidden` launch, or a `--hidden` launch handed
    /// off to this one (`cli_server::handoff_request(true)` → the `app.open` arm). Until 29 Sep 2026
    /// a second setting (`start_services_on_launch`) gated this instead, on ANY launch; it is
    /// retired, and the only mention left in production source is the sweep that deletes a
    /// leftover row.
    ///
    /// **TEXT, not behaviour** (the #175 bound): the one `auto_start_services` spawn lives in
    /// `run_login_start`, whose two callers are gated by `LOGIN_START.ready(hidden_launch)` and
    /// `LOGIN_START.request()`, and the only `request_login_start` call sits under
    /// `if handoff_is_login(&args) {`. The gate's own behaviour is the next test; a real login is
    /// a SMOKE leg per OS.
    #[test]
    fn login_start_runs_only_on_a_login() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        assert_eq!(
            src.matches("commands::services::auto_start_services(").count(),
            1,
            "login-start is spawned from more than one place (moved? update this guard AND ledger #739)"
        );
        let call = src.find("commands::services::auto_start_services(").expect("counted above");
        let owner = src[..call].rfind("fn ").expect("the spawn sits in a function");
        assert!(
            src[owner..].starts_with("fn run_login_start"),
            "the one login-start spawn left `run_login_start`: {:?}",
            &src[owner..(owner + 40).min(src.len())]
        );
        assert_eq!(src.matches("run_login_start(").count(), 2, "run_login_start gained a caller");
        assert_eq!(src.matches("LOGIN_START.ready(").count(), 1);
        assert!(
            src.contains("if LOGIN_START.ready(hidden_launch) {"),
            "setup's login fact is not this launch's `--hidden` any more — login-start would run on a \
             launch the user made"
        );
        assert_eq!(src.matches("LOGIN_START.request()").count(), 1);
        assert!(src.contains("if LOGIN_START.request() {"));

        let cli = crate::core::copy_scan::production_source(include_str!("cli_server.rs"));
        assert_eq!(cli.matches("crate::request_login_start(").count(), 1, "a second way into login-start");
        let at = cli.find("crate::request_login_start(").expect("counted above");
        let gate = cli[..at].rfind("if ").expect("some `if` precedes the call");
        assert!(
            cli[gate..].starts_with("if handoff_is_login(&args) {"),
            "the handoff's login-start is not gated on the login flag alone: {:?}",
            &cli[gate..(gate + 40).min(cli.len())]
        );

        // The retired setting gates nothing: its one production mention is the
        // constant the launch sweep deletes it by.
        assert_eq!(
            src.matches("\"start_services_on_launch\"").count(),
            1,
            "start_services_on_launch is read somewhere again — the login toggle is the ONE opt-in"
        );
        assert!(src.contains("delete_setting(&conn, RETIRED_AUTO_START_SETTING)"));
    }

    /// **A login starts the stack exactly once, whichever way it arrives and in whichever order**
    /// (`LoginStartGate`, ledger #739). The 0.8.10 VM case is the second block: a launch the user
    /// did not make with `--hidden` (macOS's Reopen-windows relaunch) holds the lock, and the login
    /// arrives as a handoff.
    #[test]
    fn a_login_starts_the_stack_once_whichever_way_it_arrives() {
        // This launch is the login.
        let g = LoginStartGate::new();
        assert!(g.ready(true));
        assert!(!g.request(), "a handoff after the launch's own login-start must not run it again");
        // A launch without `--hidden`, then a handed-off login.
        let g = LoginStartGate::new();
        assert!(!g.ready(false), "a launch the user made starts nothing");
        assert!(g.request());
        assert!(!g.request());
        // A handoff that lands while setup is still running is HELD, not dropped.
        let g = LoginStartGate::new();
        assert!(!g.request());
        assert!(g.ready(false));
        // Both at once: still once.
        let g = LoginStartGate::new();
        assert!(!g.request());
        assert!(g.ready(true));
        assert!(!g.request());
        // No login at all.
        let g = LoginStartGate::new();
        assert!(!g.ready(false));

        // The race itself: setup's `ready` and a handoff's `request` on two threads, many times.
        // Exactly one of them runs it, every time — never zero (a lost login), never two.
        for _ in 0..500 {
            let g = std::sync::Arc::new(LoginStartGate::new());
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let (g2, b2) = (g.clone(), barrier.clone());
            let handoff = std::thread::spawn(move || {
                b2.wait();
                g2.request()
            });
            barrier.wait();
            let setup = g.ready(false);
            let handoff = handoff.join().expect("the handoff thread");
            assert!(setup ^ handoff, "setup ran: {setup}, handoff ran: {handoff}");
        }
    }

    /// **The DNS handoff's ORDER is the whole of it.** Release the port, then
    /// kickstart the agent, then probe, and rebind if the agent did not take
    /// it. Any other order is a different bug: kickstarting before releasing
    /// gives the agent a port we still hold (it retries and fails); probing
    /// before kickstarting measures the cadence we are trying to skip; and
    /// skipping the rebind leaves a machine with no resolver at all — strictly
    /// worse than the in-process one it started with.
    ///
    /// **TEXT, not behaviour** (the #175 bound): it reads the function and
    /// checks the four steps appear in that order. It cannot see a real agent
    /// take a real port — that needs a login race on a real machine, and it is
    /// a SMOKE leg. What it catches is the reordering a later refactor makes
    /// while "tidying", which is silent: the handoff simply never succeeds and
    /// the app keeps serving DNS it should have given away.
    #[test]
    fn the_dns_handoff_releases_the_port_before_it_asks_the_agent_to_take_it() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src.find("fn spawn_dns_handoff").expect("the handoff exists");
        let body = &src[start..];
        let end = body.find("\nfn ").map(|i| i + 1).unwrap_or(body.len());
        let body = &body[..end];
        // Landmark first: a mis-sliced body makes every position check below
        // pass on an empty string (the copy_scan rule).
        assert!(body.contains("ATTEMPTS"), "sliced the wrong function");

        let step = |needle: &str| {
            body.find(needle).unwrap_or_else(|| panic!("the handoff must {needle}"))
        };
        let release = step("take_service");
        let kickstart = step("kickstart");
        let probe = step("answers_as_ours");
        let rebind = step("DnsService::start");
        assert!(release < kickstart, "release the port BEFORE asking the agent to take it");
        assert!(kickstart < probe, "kickstart BEFORE probing, or the probe measures the wait");
        assert!(probe < rebind, "rebind only AFTER the probe says the agent did not take it");
    }

    /// Ledger #442, leg (4) — **the watchdog re-examines `Down`; there is no
    /// empty `Down` arm.** TEXT, not behaviour (the #175 bound): the watchdog
    /// is a closure over a live app. What it catches is the arm going back to
    /// `DnsMode::Down => {}` — the shape that, on the macOS 13→15 upgrade run
    /// (23 Sep 2026), had `rex status` say DOWN for two hours while the agent
    /// answered every query.
    #[test]
    fn the_dns_watchdog_never_leaves_down_unexamined() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src.find("let dns = watchdog.state::<state::app::DnsState>();").expect("the watchdog reads DnsState");
        let body = &src[start..];
        let end = body.find("if !events.is_empty()").expect("the watchdog flushes its events");
        let body = &body[..end];
        assert!(body.contains("kickstart"), "sliced the wrong block");
        assert!(
            !body.contains("DnsMode::Down => {}"),
            "Down must be probed like Agent, never left as an empty arm"
        );
        assert!(
            body.contains("DnsMode::Agent | state::app::DnsMode::Down =>"),
            "Down shares the Agent arm's probe, kick and fallback"
        );
        let probe = body.find("answers_as_ours").expect("the arm probes the wire");
        let adopt = body[probe..].find("DnsMode::Agent);").expect("an answering agent is adopted out of Down");
        assert!(adopt > 0, "adoption follows the probe");
    }

    /// Ledger #757 — **on Linux every webview is destroyed inside `RunEvent::Exit`
    /// before anything else runs there.** TEXT (the #175 bound): the arm's first
    /// statement is the `cfg(target_os = "linux")` destroy loop, ahead of the job
    /// and tunnel cleanup — the shape whose absence let WebKitWebProcess die on the
    /// app's exit (22.04 VM, 29 Sep 2026).
    #[test]
    fn the_linux_exit_destroys_the_webviews_before_anything_else() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let arm = src.split("tauri::RunEvent::Exit => {").nth(1).expect("the Exit arm");
        let arm = &arm[..arm.find("\n                }\n").expect("the arm's end")];
        let pos = |needle: &str| arm.find(needle).unwrap_or_else(|| panic!("`{needle}` is gone from the Exit arm"));
        let cfg = pos("#[cfg(target_os = \"linux\")]");
        let destroy = pos("window.destroy()");
        let jobs = pos("cancel_all_on_exit(app)");
        assert!(cfg < destroy && destroy < jobs, "the webviews go first, and only on Linux");
    }

    /// Ledger #756 — **the watchdog asks patiently and kicks only after two silent
    /// polls.** TEXT (the #175 bound): the Agent/Down arm probes with
    /// `answers_as_ours_patiently`, feeds the miss count through `probe_misses_after`,
    /// and no kick precedes `kick_after`. Plant-proven by putting the single-try probe
    /// back and by deleting the gate.
    #[test]
    fn the_dns_watchdog_kicks_only_after_two_silent_polls() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src.find("let dns = watchdog.state::<state::app::DnsState>();").expect("the watchdog reads DnsState");
        let body = &src[start..];
        let body = &body[..body.find("if !events.is_empty()").expect("the watchdog flushes its events")];
        let pos = |needle: &str| body.find(needle).unwrap_or_else(|| panic!("`{needle}` is gone from the watchdog"));
        let probe = pos("core::dns::answers_as_ours_patiently(");
        assert!(!body.contains("if core::dns::answers_as_ours(core::dns::DEFAULT_DNS_PORT)"), "the single-try probe is back");
        let fed = pos("dns_probe_misses = core::dns::probe_misses_after(dns_probe_misses, answered);");
        let gate = pos("else if !core::dns::kick_after(dns_probe_misses) {");
        let kick = pos("dns_agent().kickstart()");
        assert!(probe < fed && fed < gate && gate < kick, "probe → count → gate → kick, in that order");
    }

    /// Ledger #715 — **a resolver the user REMOVED is never brought back by the
    /// watchdog.** TEXT, not behaviour (the #175 bound). Two halves, both
    /// required: the teardown command must SAY the removal was asked for
    /// (`DnsMode::Removed`), and the watchdog must have a deliberately empty arm
    /// for it — the `Down` arm is probed (#442 leg 4), so folding `Removed` into
    /// it would kick the uninstalled agent exactly as before.
    #[test]
    fn a_removed_resolver_is_never_resurrected_by_the_watchdog() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src.find("let dns = watchdog.state::<state::app::DnsState>();").expect("the watchdog reads DnsState");
        let body = &src[start..];
        let end = body.find("if !events.is_empty()").expect("the watchdog flushes its events");
        let body = &body[..end];
        assert!(body.contains("kickstart"), "sliced the wrong block");
        assert!(
            body.contains("state::app::DnsMode::Removed => {}"),
            "the watchdog must leave a removed resolver alone — an empty Removed arm"
        );
        assert!(
            !body.contains("| state::app::DnsMode::Removed") && !body.contains("DnsMode::Removed |"),
            "Removed must not share the probing arms"
        );
        let cmd = crate::core::copy_scan::production_source(include_str!("commands/system.rs"));
        let teardown = cmd.find("pub async fn uninstall_system(").expect("the uninstall command exists");
        let teardown = &cmd[teardown..];
        let teardown = &teardown[..teardown.find("\n#[").unwrap_or(teardown.len())];
        let ran = teardown.find("run_system_teardown(").expect("uninstall_system runs the teardown");
        let told = teardown.find("DnsMode::Removed").expect("uninstall_system must set DnsMode::Removed");
        assert!(told > ran, "the mode is set AFTER the teardown succeeded, not before it could fail");
    }

    /// Ledger #624 — **the Windows tray opens the window on a LEFT click and the menu on a RIGHT one, with the
    /// colour icon** (owner's ruling Q2, W7 S5). TEXT, not behaviour (the #175 bound): the clicks themselves are
    /// the owner's run on the Dell. What it catches is a refactor that drops the Windows branch or its
    /// left-click handler — silently giving Windows macOS's menu-on-any-click and a template glyph that is a
    /// black mark on a dark taskbar.
    #[test]
    fn the_windows_tray_opens_the_window_on_a_left_click_with_the_colour_icon() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src.find("fn install_tray").expect("the tray install exists");
        let body = &src[start..];
        let end = body.find("\nfn ").map(|i| i + 1).unwrap_or(body.len());
        let body = &body[..end];
        assert!(body.contains("TrayIconBuilder"), "sliced the wrong function");
        let windows = &body[body.find("#[cfg(windows)]").expect("install_tray has no Windows branch")..];
        assert!(
            windows.contains("WINDOWS_TRAY_ICON") && !windows.contains("icon_as_template"),
            "the Windows tray must use the colour icon, not macOS's template glyph"
        );
        assert!(windows.contains(".show_menu_on_left_click(false)"), "a left click must not open the menu on Windows");
        assert!(
            windows.contains("MouseButton::Left") && windows.contains("MouseButtonState::Up") && windows.contains("show_main_window"),
            "a left click must bring the window back on Windows"
        );
        assert!(
            src.contains(r#"const WINDOWS_TRAY_ICON: &[u8] = include_bytes!("../icons/32x32.png");"#),
            "the Windows tray icon is the colour app icon"
        );
    }

    /// Ledger #637 — **the Windows webview is asked to turn WebView2's browser shortcuts off**: Ctrl+R and F5
    /// reloaded the whole app (measured on the Dell), and wry leaves them at WebView2's default. The call is
    /// `SetAreBrowserAcceleratorKeysEnabled(false)` under `cfg(target_os = "windows")`, best-effort.
    #[test]
    fn the_windows_webview_turns_the_browser_accelerator_keys_off() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src
            .find("SetAreBrowserAcceleratorKeysEnabled")
            .expect("the Windows webview no longer asks for the browser keys to be turned off");
        let call = &src[start..src[start..].find(')').map(|i| start + i + 1).expect("the call's end")];
        assert!(call.contains("(false)"), "the browser accelerator keys are being turned ON: {call}");
        let before = &src[..start];
        let guard = before.rfind("#[cfg(target_os = \"windows\")]").expect("the call is not behind a Windows cfg");
        assert!(
            before[guard..].contains("with_webview"),
            "the call no longer runs inside the window's own webview closure"
        );
    }

    /// **Cmd+Q passes through the quit gate.** The predefined Quit item is
    /// `terminate:`, which never raises `ExitRequested`; so the gate on that
    /// event covered the tray and `rex` and not the keyboard, and a share
    /// died silently under Cmd+Q. The app menu must carry a CUSTOM quit that
    /// calls `exit` (which raises the event), and nothing may put the
    /// predefined one back.
    #[test]
    fn cmd_q_is_a_custom_item_that_raises_exit_requested() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src.find("fn install_about_menu_item").expect("the menu install exists");
        let body = &src[start..];
        let end = body.find("\n#[cfg(test)]").unwrap_or(body.len());
        let body = &body[..end];
        assert!(body.contains("rex-about"), "sliced the wrong function");
        assert!(
            body.contains("QUIT_MENU_ID") && body.contains("CmdOrCtrl+Q"),
            "the app menu has no custom Quit with the Cmd+Q accelerator — the predefined one \
             terminates past the share confirm"
        );
        assert!(
            body.contains("event.id() == QUIT_MENU_ID") && body.contains("app.exit(0)"),
            "the custom Quit must call `exit`, which raises ExitRequested — the ONE gate"
        );
        assert!(
            !src.contains("PredefinedMenuItem::quit"),
            "a predefined Quit item is back — it bypasses the gate"
        );
    }

    /// **The tray is never a SECOND way to do something.** Every menu action
    /// goes through the entry point the UI already uses — `start_services`,
    /// `stop_services`, `open_external`, `mcp_set_enabled` — so a rule that
    /// lives in one of them (the browser preference, the share confirm, the
    /// socket bind that must accompany the MCP flag) cannot be missing from the
    /// menu-bar path.
    ///
    /// **This proves TEXT, not behaviour** — the same honest bound as #175's
    /// order guard. It cannot see that a click reaches the command; it sees
    /// that the dispatcher names the commands and does not name the shortcuts
    /// around them. That is worth having anyway, because the drift it catches
    /// is a tidy one-liner: `shell().open(url)` instead of `open_external` is
    /// shorter, works on the developer's machine, and silently ignores the
    /// preferred browser — the exact failure `open_external`'s own doc calls
    /// "the thirteenth call site that forgot".
    /// **The updater never calls Tauri's restart, and the relaunch goes through
    /// the ONE quit gate.**
    ///
    /// `AppHandle::restart` on the main thread skips `ExitRequested` and `Exit`
    /// entirely — the live-share confirm, the tunnel kill and the repo-job
    /// cancel all live in those events (#436). Off the main thread it can be
    /// cancelled by `prevent_exit` and leave a thread sleeping forever with
    /// `restart_on_exit` latched, so the NEXT quit silently relaunches instead.
    /// And it spawns the child before exiting, which races the single-instance
    /// socket (#441).
    ///
    /// None of that is visible in a type, so it is read out of the source.
    #[test]
    fn the_app_updater_never_calls_tauri_restart_and_exits_through_the_one_gate() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // Needles assembled, so this test cannot convict itself.
        let banned = [
            format!("app_handle.{}", "restart()"),
            format!("app.{}", "restart()"),
            format!("request_{}", "restart()"),
            format!("process::{}", "restart("),
        ];
        let mut scanned = 0usize;
        for dir in ["src", "examples"] {
            let mut stack = vec![root.join(dir)];
            while let Some(d) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&d) else { continue };
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                        let Ok(text) = std::fs::read_to_string(&p) else { continue };
                        let prod = crate::core::copy_scan::production_source(&text);
                        scanned += 1;
                        for needle in &banned {
                            assert!(
                                !prod.contains(needle.as_str()),
                                "{} calls {needle} — the relaunch must go through app.exit(0) \
                                 so the quit gate runs",
                                p.display()
                            );
                        }
                    }
                }
            }
        }
        assert!(scanned > 100, "the scan only read {scanned} files — it stopped working");

        // And the quit must go through the gate rather than an exit of its own.
        // It no longer lives in the apply: since 20 Sep 2026 the apply SWAPS and
        // returns, and the quit is `app_update_restart` — the OK button of the
        // dialog that tells the user the app is about to close and reopen.
        let cmd = crate::core::copy_scan::production_source(include_str!(
            "commands/app_update.rs"
        ));
        assert!(cmd.contains("app_update_apply"), "sliced the wrong file");
        assert!(
            cmd.contains(&format!("app.{}", "exit(0)")),
            "the restart must quit through app.exit(0), which raises ExitRequested"
        );
        // The apply itself must NOT quit: the user is told first and clicks OK.
        // Sliced between the two functions rather than by attribute, because the
        // text between them is whatever the file happens to carry.
        // The apply's BODY, not the span up to the next function: the prose
        // between them explains the quit and would fail a scan that read it.
        let from = cmd.find("pub async fn app_update_apply").expect("the apply");
        let body_end = cmd[from..].find("\n}\n").expect("the apply's closing brace") + from;
        assert!(
            cmd.find("pub fn app_update_restart").is_some_and(|r| r > body_end),
            "app_update_restart must be its own function, after the apply"
        );
        assert!(
            !cmd[from..body_end].contains(&format!("app.{}", "exit(0)")),
            "app_update_apply quits on its own again — the user must be told the app is \
             about to close and reopen, and clicking OK is what quits it"
        );
        assert!(
            !cmd.contains(&format!("std::process::{}", "exit(")),
            "a bare process::exit would skip every exit hook, including the relaunch"
        );
    }

    /// **An update is applied only from a GUI click.**
    ///
    /// Self-update replaces the process that enforces the agent-access dial and
    /// `settings_access`; the relaunch kills the caller's socket mid-call, so an
    /// agent could never observe the result of what it asked for; and this tree
    /// already runs `cloudflared --no-autoupdate` for the same class of reason.
    #[test]
    fn the_app_updater_installs_only_from_the_gui() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let needle = format!("app_update_{}", "apply");
        for path in ["src/cli_server.rs", "src/mcp_server.rs"] {
            let text = std::fs::read_to_string(root.join(path)).expect(path);
            assert!(
                !crate::core::copy_scan::production_source(&text).contains(&needle),
                "{path} reaches the apply — install is a GUI click"
            );
        }
        let mut stack = vec![root.join("src/mcp_server")];
        let mut scanned = 0usize;
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    let Ok(text) = std::fs::read_to_string(&p) else { continue };
                    scanned += 1;
                    assert!(
                        !crate::core::copy_scan::production_source(&text).contains(&needle),
                        "{} reaches the apply — install is a GUI click",
                        p.display()
                    );
                }
            }
        }
        assert!(scanned > 0, "the MCP scan read no files");
        // Anti-vacuity: the needle DOES appear where the click lives, so a typo
        // in it could not make this pass everywhere.
        let handler = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        assert!(handler.contains(&needle), "the apply is not registered at all");
    }

    /// **The tray reads the offer from an in-process snapshot — never the
    /// database, never the network.**
    ///
    /// `tray_model` runs on the menu-bar path every few seconds. A read of the
    /// signed descriptor would mean verifying a signature to draw a menu (and a
    /// menu that can fail is a menu bar with nothing in it); a fetch would put a
    /// network call a centimetre from the user's cursor. Both are the "the tray
    /// measures nothing, and never blocks" rule (#437/#438).
    #[test]
    fn the_tray_reads_the_offer_from_a_snapshot_never_the_network() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src.find("fn tray_model").expect("the model builder exists");
        let body = &src[start..start + 2500.min(src.len() - start)];
        assert!(body.contains("TrayModel"), "sliced the wrong function");

        assert!(
            body.contains(&format!("app_update::current_{}", "offer()")),
            "the tray must read the published snapshot"
        );
        for banned in [
            format!("app_update::{}", "cached("),
            format!("app_update::{}", "fetch("),
            format!("app_update::{}", "state("),
        ] {
            assert!(
                !body.contains(&banned),
                "tray_model reaches for {banned} — drawing a menu must not verify a \
                 signature or touch the network"
            );
        }
    }

    /// **The auto-check setting gates the REQUEST, not the answer.**
    ///
    /// A setting honoured after the fetch would still send it — the user turned
    /// the check off and the app kept talking to GitHub anyway, which is the one
    /// thing the toggle is for. So every call site of the check must be inside a
    /// branch that read `auto_check_enabled` first, and this reads the source to
    /// say so, because nothing about the types would stop the other order.
    ///
    /// The poller is checked separately from the launch sweep on purpose: they
    /// are two call sites, and a guard covering one of them while claiming both
    /// is the shape this project keeps finding as the real defect.
    #[test]
    fn auto_check_is_read_before_the_request_not_applied_to_the_answer() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        // Assemble the needle so this test cannot convict itself by matching the
        // literal in its own body (the copy_scan rule).
        let call = format!("check_for_app_{}", "update(&state)");
        let gate = format!("auto_check_{}", "enabled(");

        let sites: Vec<usize> = src.match_indices(&call).map(|(i, _)| i).collect();
        assert_eq!(
            sites.len(),
            2,
            "expected exactly two call sites (the launch sweep and the poller); \
             a third one needs its own gate and this test needs to know about it"
        );
        for at in sites {
            // The gate must appear in the ~1500 characters before the call —
            // the same function, not somewhere else in the file.
            let window = &src[at.saturating_sub(1500)..at];
            assert!(
                window.contains(&gate),
                "a check is fired without reading the auto-check setting first"
            );
        }
        // And the poller must re-read it every tick rather than capturing it
        // once: turning the setting off must stop the NEXT request.
        let poller = src
            .find("fn spawn_app_update_poller")
            .expect("the poller exists");
        let body = &src[poller..];
        assert!(body.contains("sleep"), "sliced the wrong function");
        let tick = body.find("loop {").expect("the poller loops");
        assert!(
            body[tick..tick + 900].contains(&gate),
            "the poller must read the setting inside its loop, not once at spawn"
        );
    }

    #[test]
    fn the_tray_acts_only_through_the_commands_the_ui_uses() {
        let src = crate::core::copy_scan::production_source(include_str!("lib.rs"));
        let start = src.find("fn on_tray_click").expect("dispatcher exists");
        let body = &src[start..];
        // A landmark first: an empty or mis-sliced body makes every `contains`
        // below pass (the copy_scan rule).
        assert!(body.contains("TrayAction::Quit"), "sliced the wrong function");

        for must in [
            "commands::services::start_services",
            "commands::services::stop_services",
            "commands::system::open_external",
            "commands::mcp::mcp_set_enabled",
        ] {
            assert!(body.contains(must), "the tray must act through {must}");
        }
        for must_not in [
            // The tray OPENS the update card; it never installs. The consent
            // sentence and the button live in ONE place, and a menu item that
            // reached the apply would be a second path past the only sentence
            // that tells a user what pressing it costs.
            "app_update_apply",
            "app_update_restart",
            // The browser preference lives in open_external, once.
            "shell().open",
            // The MCP flag without the bind: the toggle would read "on" while
            // nothing listens, which its own key doc forbids.
            "set_setting",
        ] {
            assert!(!body.contains(must_not), "the tray must not reach for {must_not} directly");
        }
    }
}
