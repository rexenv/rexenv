//! rexenv backend entry. Registers all Tauri commands and wires the module tree.
//!
//! Architecture: `commands/` (thin IPC) → `core/` (platform-agnostic) →
//! `platform/` (OS traits, selected via cfg). See CLAUDE.md.

#[cfg(unix)]
pub mod cli_server;
pub mod commands;
pub mod core;
pub mod error;
pub mod platform;
pub mod state;
pub mod utils;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The interactive app may stop ADOPTED services (Stop-all after a relaunch);
    // any other process linking this lib (live-check examples) may stop only
    // what it spawned. See core::stack_guard.
    core::stack_guard::mark_app_process();
    tauri::Builder::default()
        // Native open/save dialogs (Settings → Sites folder picker).
        .plugin(tauri_plugin_dialog::init())
        // Closing the window quits the app, which stops live public shares —
        // same pause-and-confirm as Cmd+Q (RunEvent::ExitRequested below).
        // Held HERE too because a closed-then-cancelled window can't come
        // back; preventing the close keeps it alive under the dialog.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                use tauri::Manager;
                if !commands::tunnels::confirm_quit_or_prompt(window.app_handle()) {
                    api.prevent_close();
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
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
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

            let platform = platform::current();

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
            app.manage(dns_state);

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
                    // preserves the user's installed choices on re-run. `bumped`
                    // = minors whose pinned patch moved with THIS app release
                    // (Option A updates) — their live pools are restarted below.
                    let bumped = match core::php::seed_registry(&conn) {
                        Ok(b) => b,
                        Err(e) => {
                            log::error!("php: failed to seed version registry: {e}");
                            Vec::new()
                        }
                    };
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
                    // Opt-in "start services when rexenv opens" (Settings) — read
                    // while the connection is still ours; acted on below, after
                    // AppState is managed and survivors are adopted.
                    let auto_start = state::store::get_setting(
                        &conn,
                        commands::services::AUTO_START_SETTING,
                    )
                    .ok()
                    .flatten()
                    .as_deref()
                        == Some("true");
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
                        let adopted = mgr.adopt_startup(state.platform.as_ref(), &sites);
                        if adopted > 0 {
                            log::info!("adopted {adopted} running service(s) from a prior session");
                        }
                    }
                    app.manage(state);

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
                        let live: Vec<String> = {
                            let mgr = state.services.lock().await;
                            bumped.into_iter().filter(|m| mgr.has_php_pool(m)).collect()
                        };
                        if !live.is_empty() {
                            for minor in &live {
                                let plan = core::downloads::plan_for_php(platform, minor);
                                if let Err(e) = core::downloads::prefetch(
                                    platform,
                                    &format!("Update PHP {minor}"),
                                    &plan,
                                )
                                .await
                                {
                                    log::warn!("php: patch-update prefetch failed: {e}");
                                    return; // old pool keeps serving; retry next launch
                                }
                            }
                            let checks = {
                                let mut mgr = state.services.lock().await;
                                match mgr.restart_pools_for(platform, &live).await {
                                    Ok(c) => c,
                                    Err(e) => {
                                        log::warn!("php: patch-update pool restart failed: {e}");
                                        Vec::new()
                                    }
                                }
                            };
                            if let Err(e) = core::service_manager::await_ready(checks).await {
                                log::warn!("php: a patch-updated pool did not become ready: {e}");
                            }
                        }
                        for dir in core::binaries::gc_outdated_php_caches(platform) {
                            log::info!("php: removed outdated binary cache {dir}");
                        }
                    });

                    // Opt-in login-start: with "Open rexenv at login" + this
                    // setting, the whole stack returns after a reboot without a
                    // click. Runs AFTER adoption (already-running services are
                    // skipped, so a mid-day relaunch is a no-op) and is login-safe
                    // by construction: never downloads, never prompts (see
                    // `auto_start_services`).
                    if auto_start {
                        let auto = app.handle().clone();
                        tauri::async_runtime::spawn(async move {
                            commands::services::auto_start_services(auto).await;
                        });
                    }
                    None
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
            app.manage(commands::system::InitError(init_error));

            // `rex` CLI socket (see `cli_server`): requests execute in THIS
            // process through the same command fns the UI calls. Spawned even
            // when init failed — each request then gets the honest
            // still-starting/failed error instead of a dead socket.
            #[cfg(unix)]
            cli_server::spawn(app.handle().clone());

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
                    // the task in place (the old behavior). A resolver that never
                    // started (Down) stays a Settings problem, not a restart loop.
                    let dns = watchdog.state::<state::app::DnsState>();
                    match dns.mode() {
                        state::app::DnsMode::Agent => {
                            if core::dns::answers_as_ours(core::dns::DEFAULT_DNS_PORT) {
                                dns_failures = 0;
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
                                        events.push(core::service_manager::HealthEvent {
                                            service: "DNS".into(),
                                            action: "restarted",
                                            detail: "resolver agent would not come back — \
                                                     serving in-process instead (sites resolve, \
                                                     but not after the app quits)"
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
                        state::app::DnsMode::Down => {}
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
            commands::system::init_error,
            commands::system::global_status,
            commands::system::open_external,
            commands::system::reveal_path,
            commands::system::list_editors,
            commands::system::open_in_editor,
            commands::system::dns_status,
            commands::system::cli_status,
            commands::system::cli_install,
            commands::system::system_setup,
            commands::system::trust_local_ca,
            commands::system::firefox_trust_status,
            commands::system::trust_ca_in_firefox,
            commands::system::regenerate_certs,
            commands::system::autostart_status,
            commands::system::set_autostart,
            commands::system::uninstall_system,
            commands::blueprints::list_blueprints,
            commands::blueprints::save_blueprint,
            commands::blueprints::delete_blueprint,
            commands::sites::list_sites,
            commands::sites::sites_serving,
            commands::sites::sites_resources,
            commands::sites::create_site,
            commands::sites::rename_site,
            commands::sites::change_site_domain,
            commands::sites::move_site_docroot,
            commands::sites::inspect_linked_folder,
            commands::valet_import::scan_valet_import,
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
            commands::sites::set_site_xdebug,
            commands::sites::delete_site,
            commands::php::list_php_versions,
            commands::php::set_php_version_installed,
            commands::php::set_default_php_version,
            commands::php::get_php_settings,
            commands::php::apply_php_settings,
            commands::wordpress::wp_info,
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
            commands::wordpress::wp_theme_enable_network,
            commands::wordpress::wp_theme_disable_network,
            commands::wordpress::wp_super_admins,
            commands::wordpress::wp_super_admin_add,
            commands::services::start_services,
            commands::services::stop_services,
            commands::services::services_status,
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
            commands::mail::mailpit_delete,
            commands::database::databases_status,
            commands::database::start_database,
            commands::database::stop_database,
            commands::database::db_engine_versions,
            commands::database::set_db_engine_version,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::sites_folder,
            commands::settings::default_tld,
            commands::settings::set_default_tld,
            commands::settings::tld_policy,
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
            commands::repo::repo_asset_status,
            commands::repo::repo_unmanaged,
            commands::repo::repo_adopt,
            commands::repo::repo_git_op,
            commands::repo::repo_branches,
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
                // shares means no dialog, ever. Covers Cmd+Q; window close
                // routes through on_window_event above.
                tauri::RunEvent::ExitRequested { api, .. } => {
                    if !commands::tunnels::confirm_quit_or_prompt(app) {
                        api.prevent_exit();
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
                    commands::repo::cancel_all_on_exit(app);
                    commands::tunnels::kill_all_on_exit(app);
                }
                _ => {}
            }
        });
}
