//! core::redis — Redis service (shipped from the "Deferred services" plan;
//! evidence in docs/archive/SHIPPED-2026-07.md).
//!
//! One shared Redis server on a loopback port, from the FIRST bottle bundle
//! (`core::binaries::resolve_bundle("redis", …)` — redis + relinked openssl@3
//! dylibs). No init step (Redis creates its dump files on demand); config is
//! passed entirely as command-line arguments, so there is no conf file to
//! template or quote. The data dir lives under app-data like the other engines.

use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::Child;

/// `redis-server` inside the bundle tree.
pub fn redis_server_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/redis-server")
}

/// `redis-cli` inside the bundle tree.
pub fn redis_cli_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/redis-cli")
}

/// Redis working/data directory under app-data (RDB snapshots land here).
pub fn data_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("redis").join("data"))
}

/// Start the shared Redis server (foreground, loopback-only) via
/// `ProcessSupervisor`. The `--dir` arg carries the app-data path, which is
/// ALSO what makes the process adoptable after an app restart (`adopt_startup`
/// keys ownership on our port + the app-data marker in the cmdline).
pub fn start(platform: &dyn Platform, basedir: &Path, datadir: &Path, port: u16) -> Result<Child> {
    std::fs::create_dir_all(datadir)?;
    let args = vec![
        "--port".to_string(),
        port.to_string(),
        "--bind".to_string(),
        "127.0.0.1".to_string(),
        "--dir".to_string(),
        datadir.display().to_string(),
        // Foreground child under our supervisor, logs on stdout.
        "--daemonize".to_string(),
        "no".to_string(),
    ];
    let log = platform.paths().log_dir()?.join("redis-stdout.log");
    // A locale Redis can SET, never the host's. `redis-server` calls
    // `setlocale(LC_COLLATE, "")` at startup and `exit(1)`s when that fails —
    // "Failed to configure LOCALE for invalid locale name." is its LAST line,
    // and the supervisor sees only "did not start within 15s". On the macOS
    // 13.6 VM (T7, 23 Sep 2026) the login session carried `LANG=C.UTF-8`, a
    // locale macOS 13 does not have (`locale -a`), so every spawn died at once;
    // macOS 15 has it and the same bundle was fine there. `C` exists on every
    // OS and is all Redis needs (collation for SORT ALPHA). Ledger #714.
    let env = [("LC_ALL".to_string(), "C".to_string())];
    platform
        .supervisor()
        .spawn_logged_env(&redis_server_bin(basedir), &args, &log, &env)
}

/// Stop a running Redis by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ledger #714 — the spawn carries its own locale. A source tripwire: the
    /// supervisor is a platform trait, and the env is the one argument a stub
    /// would have to be written to capture; the text is what a refactor to
    /// `spawn_logged` (the env-less form) would silently drop.
    #[test]
    fn redis_is_spawned_with_a_locale_it_can_set() {
        const SRC: &str = include_str!("redis.rs");
        let body = &SRC[SRC.find("pub fn start(").expect("start")..SRC.find("pub fn stop(").expect("stop")];
        assert!(body.contains(r#"("LC_ALL".to_string(), "C".to_string())"#), "the LC_ALL=C pair is gone");
        assert!(body.contains("spawn_logged_env("), "the env-carrying spawn is gone");
        assert!(!body.contains(".spawn_logged(&"), "the env-less spawn is back");
    }

    #[test]
    fn bin_paths_under_basedir() {
        let base = Path::new("/opt/redis");
        assert_eq!(redis_server_bin(base), Path::new("/opt/redis/bin/redis-server"));
        assert_eq!(redis_cli_bin(base), Path::new("/opt/redis/bin/redis-cli"));
    }
}
