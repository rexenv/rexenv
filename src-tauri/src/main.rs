// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // FIRST, before any mode: a panic anywhere below — the DNS agent, the tunnel
    // guard, the relauncher or the app — is written to crash.log, and on a
    // Windows release build shown, instead of vanishing into a console the
    // process does not have (docs/PLAN-windows-port.md §3a Q1, ledger #596).
    rexenv_lib::crash::install();

    // Windows' elevated step (`rexenv.exe --elevated-step`, W6 S3): started by UAC for one privileged change,
    // it runs only rexenv's own ops and exits. Before every other mode — it opens no window. Linux's
    // polkit step (`rexenv --privileged-step`) takes the same door.
    let argv: Vec<String> = std::env::args().collect();
    // `--print-version`: what THIS binary is, for the self-update's staged copy to answer for
    // itself (a Linux AppImage has no Info.plist or VERSIONINFO to read). Before everything —
    // it must open nothing and touch nothing.
    if argv.iter().any(|a| a == rexenv_lib::core::app_update::PRINT_VERSION_FLAG) {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    if let Some(code) = rexenv_lib::platform::run_elevated_step(&argv) {
        std::process::exit(code);
    }

    // Headless resolver mode, run by the per-user LaunchAgent so local-TLD DNS
    // survives app quits (and is up from login). Checked BEFORE Tauri boots:
    // the agent must never open a window, touch SQLite, or start services.
    if std::env::args().any(|a| a == "--dns-agent") {
        // `--log <path>`: where the agent's output goes when its supervisor captures none — a
        // Windows logon task (W6 S2, ledger #616). launchd's plist redirects it on macOS.
        if let Some(log) = argv.iter().position(|a| a == "--log").and_then(|i| argv.get(i + 1)) {
            rexenv_lib::platform::send_output_to(std::path::Path::new(log));
        }
        std::process::exit(rexenv_lib::core::dns::run_agent());
    }
    // Tunnel guard: a detached watcher that ends ONE public share when the app
    // that started it dies — including a SIGKILL, which runs none of our
    // shutdown code. Checked here, before Tauri boots, for the same reason the
    // DNS agent is: it must never open a window or touch app state.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        if let Some(args) = rexenv_lib::core::tunnels::parse_guard_args(&argv) {
            std::process::exit(rexenv_lib::platform::run_tunnel_guard(args));
        }
    }
    // The relauncher: waits for the process that swapped the bundle to be gone,
    // then reopens rexenv. Spawned by the OLD build, so this flag is a
    // cross-version contract — and checked here, before Tauri, because it must
    // open no window and touch no app state. macOS and Windows; Linux with its
    // port.
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    if let Some(args) = rexenv_lib::core::app_update::parse_relaunch_args(&argv) {
        std::process::exit(rexenv_lib::platform::run_relauncher(args));
    }
    // A launcher's job that forbids breakaway would make EVERY service start fail
    // with "access denied" — the installer's "Run rexenv" did exactly that on the
    // first installed copy (19 Sep 2026). Hop out through Explorer, once, before
    // the pipe or a window exists; off Windows this is always `None`.
    if let Some(code) = rexenv_lib::platform::relaunch_outside_confining_job() {
        std::process::exit(code);
    }
    rexenv_lib::run();
}
