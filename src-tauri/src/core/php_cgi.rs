//! core::php_cgi — the php-cgi GROUP pool model (`PoolModel::CgiGroup`,
//! docs/PLAN-windows-port.md §3 D1, ledger #601).
//!
//! Where a platform has no php-fpm, one PHP minor is served by `php-cgi -b
//! 127.0.0.1:<port>` run as a PARENT that spawns `PHP_FCGI_CHILDREN` children on
//! the one listening socket and respawns each after `PHP_FCGI_MAX_REQUESTS` —
//! php-fpm's master in miniature. Measured on the Dell (14 Sep 2026, plan D1):
//! the socket table names the parent alone as the listener; every child's command
//! line is the parent's, so the config path on it identifies the whole group;
//! killing the parent takes every child with it; the excess over the worker count
//! queues rather than fails.
//!
//! Everything here is a pure renderer or a thin spawn around one, so the rules run
//! in `verify.sh` on every host. The lifecycle — ensure, adopt, reap, stop — is
//! `php::PhpFpmPools`, one implementation for both models (owner ruling 14 Sep 2026).

use crate::error::{Error, Result};
use crate::platform::traits::{CgiGroup, Platform, PoolModel};
use std::path::{Path, PathBuf};
use std::process::Child;

/// Children per group: parity with the php-fpm pool's `pm.max_children = 10`
/// (D1(b), ruled 13 Sep 2026 — lowered only by a measurement, never by assumption).
pub const WORKERS: u32 = 10;

/// Requests before a child is replaced: parity with `pm.max_requests = 500`.
pub const MAX_REQUESTS: u32 = 500;

/// The ini a group runs with. rexenv's OWN file — `php-cgi -n` reads no other — so
/// everything not written here is PHP's built-in default, as the php-fpm pool's
/// static build runs with no php.ini at all (owner ruling 14 Sep 2026).
///
/// - `extension_dir` and one line per extension the platform's model names;
/// - the mail catch-all as PHP's SMTP keys: `mail()` connects to Mailpit directly,
///   because the group has no shell to run the `sendmail` shim through;
/// - the user's whitelisted, pre-validated settings as plain `key = value` lines
///   (a site can still `ini_set()` them at runtime, as with `php_value`);
/// - a log file, the counterpart of the pool's `error_log`.
///
/// No `mysqli.default_socket`: a `localhost` MySQL host is a unix socket only where
/// PHP has unix sockets.
pub fn render_ini(
    group: &CgiGroup,
    php_dir: &Path,
    log_file: &Path,
    catch: Option<&super::mail::Catch>,
    settings: &[(String, String)],
) -> String {
    let mut ini = String::from(
        "; rexenv php-cgi group — generated on every start; edits here are overwritten\n",
    );
    ini.push_str(&extension_lines(group, php_dir));
    ini.push_str("log_errors = On\n");
    ini.push_str(&format!("error_log = \"{}\"\n", log_file.display()));
    if catch.is_some() {
        ini.push_str(&format!(
            "SMTP = 127.0.0.1\nsmtp_port = {}\n",
            super::mail::MAILPIT_SMTP_PORT
        ));
    }
    for (key, value) in settings {
        ini.push_str(&format!("{key} = {value}\n"));
    }
    ini
}

/// `extension_dir` and one line per extension — shared by the group's ini and the CLI's, so
/// the pool and every PHP CLI spawn can never load different sets.
fn extension_lines(group: &CgiGroup, php_dir: &Path) -> String {
    let mut lines = format!("extension_dir = \"{}\"\n", php_dir.join("ext").display());
    for ext in group.extensions {
        lines.push_str(&format!("extension = {ext}\n"));
    }
    for ext in group.zend_extensions {
        lines.push_str(&format!("zend_extension = {ext}\n"));
    }
    lines
}

/// The `php.ini` beside `php.exe` in a resolved PHP tree: the extensions and nothing else
/// (owner ruling 14 Sep 2026, ledger #603).
///
/// php.exe with no ini loads NO extension — no curl, openssl or mysqli — while the macOS
/// build compiles them in, so WP-CLI, Composer, artisan and Adminer would run crippled. PHP
/// reads `php.ini` from its executable's own folder by default, so writing it THERE gives
/// every CLI spawn the extensions, including a spawn nobody has found or written yet. The
/// group is unaffected: it runs `-n -c <its own ini>`. No settings and no mail keys: the CLI
/// runs with PHP's defaults, as the static build does; mail catch for the CLI stays with the
/// call sites that pass it.
pub fn render_cli_ini(group: &CgiGroup, php_dir: &Path) -> String {
    let mut ini = String::from(
        "; rexenv — the PHP CLI's extensions for this tree; rewritten when it differs\n",
    );
    ini.push_str(&extension_lines(group, php_dir));
    ini
}

/// Make sure a resolved `name` tree carries the CLI's `php.ini`, when this platform serves
/// PHP as a php-cgi group and `name` is that model's PHP. Idempotent; a file that differs
/// (a tree published before this existed, a moved cache) is rewritten.
pub fn ensure_cli_ini(platform: &dyn Platform, name: &str, dir: &Path) -> Result<()> {
    let model = platform.supervisor().php_pool_model();
    let PoolModel::CgiGroup(group) = model else {
        return Ok(());
    };
    if name != model.catalog_name() {
        return Ok(());
    }
    let path = dir.join("php.ini");
    let want = render_cli_ini(&group, dir);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(want.as_str()) {
        std::fs::write(&path, want)?;
    }
    Ok(())
}

/// The group's environment: the worker count and recycle, plus the Laravel half of
/// the mail catch-all. The children inherit the parent's environment, so what is set
/// here is what every request sees through `getenv()` — the role `env[…]` plays in a
/// php-fpm pool.
pub fn group_env(catch: Option<&super::mail::Catch>) -> Vec<(String, String)> {
    let mut env = vec![
        ("PHP_FCGI_CHILDREN".to_string(), WORKERS.to_string()),
        ("PHP_FCGI_MAX_REQUESTS".to_string(), MAX_REQUESTS.to_string()),
    ];
    if let Some(c) = catch {
        env.extend(c.env.iter().map(|(k, v)| (k.to_string(), v.clone())));
    }
    env
}

/// `-n -c <ini> -b 127.0.0.1:<port>`. The ini path on the command line is the
/// ownership marker (it lives under app data), and every child carries it too.
pub fn group_args(ini: &Path, port: u16) -> Vec<String> {
    vec![
        "-n".to_string(),
        "-c".to_string(),
        ini.display().to_string(),
        "-b".to_string(),
        format!("127.0.0.1:{port}"),
    ]
}

/// The name `php -m` lists an extension under.
fn module_name(ext: &str) -> &str {
    match ext {
        "opcache" => "Zend OPcache",
        other => other,
    }
}

/// Judge a `php-cgi -n -c <ini> -m` run: the preflight a group must pass before it
/// exists (D1(a)'s respawn-loop hazard — one process, so it can never spin).
///
/// **Not the exit code alone.** Measured on the Dell: an extension that fails to load
/// still exits 0, with only a `PHP Startup: Unable to load dynamic library …` warning
/// in the output. So any startup warning refuses, quoted, and so does any extension
/// the model asked for that the module list does not show.
pub fn preflight_verdict(
    group: &CgiGroup,
    exited_ok: bool,
    output: &str,
) -> std::result::Result<(), String> {
    let warnings: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("PHP Startup") || l.contains("Unable to load") || l.starts_with("PHP Warning"))
        .collect();
    if !warnings.is_empty() {
        return Err(warnings.join("; "));
    }
    if !exited_ok {
        return Err(format!("php-cgi -m did not exit cleanly: {}", output.trim()));
    }
    let listed: Vec<String> = output.lines().map(|l| l.trim().to_ascii_lowercase()).collect();
    let missing: Vec<&str> = group
        .extensions
        .iter()
        .chain(group.zend_extensions)
        .map(|e| module_name(e))
        .filter(|m| !listed.contains(&m.to_ascii_lowercase()))
        .collect();
    if !missing.is_empty() {
        return Err(format!("these extensions did not load: {}", missing.join(", ")));
    }
    Ok(())
}

/// `php-cgi` inside a resolved PHP directory.
pub fn php_cgi_bin(php_dir: &Path) -> PathBuf {
    php_dir.join("php-cgi")
}

/// The group's ini file name for `name` — also a word every member's command line carries,
/// which is how an adopted parent is told from a recycled pid (`PhpFpmPools::trip_spinning`).
pub fn ini_file_name(name: &str) -> String {
    format!("php-cgi-{name}.ini")
}

/// Where a group's PARENT writes its stdout and stderr: one file per minor, so the reason a
/// group is stopped is read from that group's own output and never another minor's.
pub fn output_log(log_dir: &Path, minor: &str) -> PathBuf {
    // The name lives on `PoolModel` so the Logs tab reads the same one (ledger #650). This
    // module only ever runs as a CgiGroup, so it asks that arm directly rather than taking a
    // platform it does not otherwise need.
    log_dir.join(
        crate::platform::traits::PoolModel::CgiGroup(crate::platform::traits::CgiGroup {
            extensions: &[],
            zend_extensions: &[],
        })
        .output_log_name(minor),
    )
}

/// Write the group's ini for `name` (`8.3`, or a candidate's name) and return its path.
pub fn write_ini(
    platform: &dyn Platform,
    group: &CgiGroup,
    php_dir: &Path,
    name: &str,
    catch: Option<&super::mail::Catch>,
    settings: &[(String, String)],
) -> Result<PathBuf> {
    let config_dir = platform.paths().config_dir()?;
    let log_dir = platform.paths().log_dir()?;
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&log_dir)?;
    let ini = config_dir.join(ini_file_name(name));
    let log = log_dir.join(PoolModel::CgiGroup(*group).log_name(name));
    std::fs::write(&ini, render_ini(group, php_dir, &log, catch, settings))?;
    Ok(ini)
}

/// Run the preflight against `ini`; `Err` quotes what PHP said.
pub fn preflight(platform: &dyn Platform, group: &CgiGroup, php_dir: &Path, ini: &Path) -> Result<()> {
    let log_dir = platform.paths().log_dir()?;
    std::fs::create_dir_all(&log_dir)?;
    let probe = log_dir.join("php-cgi-preflight.log");
    let _ = std::fs::remove_file(&probe);
    let args = vec![
        "-n".to_string(),
        "-c".to_string(),
        ini.display().to_string(),
        "-m".to_string(),
    ];
    let mut child = platform.supervisor().spawn_logged(&php_cgi_bin(php_dir), &args, &probe)?;
    let status = child.wait()?;
    let output = std::fs::read_to_string(&probe).unwrap_or_default();
    preflight_verdict(group, status.success(), &output)
        .map_err(|why| Error::Other(format!("PHP refused its configuration ({}): {why}", ini.display())))
}

/// Start the group for `minor` on `port`: write the ini, pass the preflight, spawn the
/// parent as a service. The returned child is the PARENT — the pid to supervise.
pub fn start_group(
    platform: &dyn Platform,
    group: &CgiGroup,
    php_dir: &Path,
    minor: &str,
    port: u16,
    catch: Option<&super::mail::Catch>,
    settings: &[(String, String)],
) -> Result<Child> {
    let ini = write_ini(platform, group, php_dir, minor, catch, settings)?;
    preflight(platform, group, php_dir, &ini)?;
    let log = output_log(&platform.paths().log_dir()?, minor);
    platform.supervisor().spawn_logged_env(
        &php_cgi_bin(php_dir),
        &group_args(&ini, port),
        &log,
        &group_env(catch),
    )
}

/// The churn breaker's threshold (plan §3 D1(a), owner ruling 14 Sep 2026, ledger #605): a
/// group PARENT that used at least this share of one CPU core over a watchdog window is
/// spinning. The parent runs no PHP — its CPU is its respawn loop. Measured on the Dell
/// (`windows_cgi_churn_probe`): 96.8% while it could not spawn a worker, 0.9% under one
/// client's 1722 requests a second (45 workers recycled in 15 s), 0.6% while a script killed
/// its own worker on every request.
pub const SPIN_CPU_SHARE: f64 = 0.25;

/// The shortest window the share is judged over: the parent's own startup costs CPU, and a
/// watchdog tick landing a second after a spawn must not read that as a spin.
pub const SPIN_MIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether `cpu_ms` of parent CPU over `window` is a spin.
pub fn spinning(cpu_ms: u64, window: std::time::Duration) -> bool {
    window >= SPIN_MIN_WINDOW && cpu_ms as f64 >= window.as_millis() as f64 * SPIN_CPU_SHARE
}

/// A process's user + kernel CPU so far, in ms; `None` when it cannot be read.
pub fn cpu_ms(pid: u32) -> Option<u64> {
    let mut sys = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
    sys.process(p).map(sysinfo::Process::accumulated_cpu_time)
}

/// Why a spinning group was stopped, for the health event the user reads: the last
/// `unable to spawn` line in the group's own output (`tail`), else the CPU and where to look.
pub fn spin_reason(tail: &str, log: &Path) -> String {
    match tail.lines().rev().map(str::trim).find(|l| l.contains("unable to spawn")) {
        Some(line) => format!(
            "stopped — its php-cgi parent could not start its workers and was retrying in a loop \
             (\"{line}\"). Fix the cause, then start it again"
        ),
        None => format!(
            "stopped — its php-cgi parent was using most of a CPU core in a loop. See {}, then \
             start it again",
            log.display()
        ),
    }
}

/// The last `max` bytes of `path`, lossily decoded. A spinning parent writes about a megabyte
/// a second, so the reason is read from the end, never the whole file.
pub fn read_tail(path: &Path, max: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    if file.seek(SeekFrom::Start(len.saturating_sub(max))).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = file.take(max).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP: CgiGroup = CgiGroup {
        extensions: &["curl", "mysqli", "mbstring"],
        zend_extensions: &["opcache"],
    };

    fn catch() -> super::super::mail::Catch {
        super::super::mail::Catch {
            sendmail_path: "'/unused' sendmail -t".into(),
            env: super::super::mail::laravel_env(),
        }
    }

    /// The breaker's threshold against the Dell's own numbers (ledger #605).
    #[test]
    fn the_breaker_trips_on_a_spin_and_never_on_recycling_or_a_short_window() {
        use std::time::Duration;
        // 96.8% of a core over a 10 s tick: the parent that could not spawn a worker.
        assert!(spinning(9_680, Duration::from_secs(10)));
        // 0.9% over 15 s of 1722 requests a second: 45 workers recycled, legitimately.
        assert!(!spinning(135, Duration::from_secs(15)));
        // 0.6% over 11 s while a script killed its own worker on every request.
        assert!(!spinning(66, Duration::from_secs(11)));
        // The edge: a quarter of the window trips, a millisecond less does not.
        assert!(spinning(2_500, Duration::from_secs(10)));
        assert!(!spinning(2_499, Duration::from_secs(10)));
        // A tick a second after a spawn: the parent's own startup is not a spin.
        assert!(!spinning(900, Duration::from_secs(1)));
    }

    #[test]
    fn the_reason_quotes_the_groups_last_spawn_failure_or_names_its_log() {
        let tail = "unable to spawn: [0x00000002]: The system cannot find the file specified\r\n\
                    unable to spawn: [0x00000005]: Access is denied.\r\n";
        let log = Path::new("/logs/php-cgi-8.3-output.log");
        let reason = spin_reason(tail, log);
        assert!(reason.contains("(\"unable to spawn: [0x00000005]: Access is denied.\")"), "{reason}");
        let fallback = spin_reason("PHP Notice: nothing about spawning\n", log);
        assert!(fallback.contains("php-cgi-8.3-output.log"), "{fallback}");
    }

    /// Two minors never share an output file — the reason is read from the tripped group's own.
    #[test]
    fn each_minor_writes_its_own_output_log() {
        let dir = Path::new("/logs");
        assert_ne!(output_log(dir, "8.3"), output_log(dir, "8.4"));
        assert_eq!(ini_file_name("8.3"), "php-cgi-8.3.ini");
    }

    #[test]
    fn read_tail_reads_only_the_end_and_nothing_from_a_missing_file() {
        let path = std::env::temp_dir().join(format!("rexenv-read-tail-{}.log", std::process::id()));
        std::fs::write(&path, "0123456789abcdef").unwrap();
        assert_eq!(read_tail(&path, 6), "abcdef");
        assert_eq!(read_tail(&path, 1_000), "0123456789abcdef");
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_tail(&path, 6), "");
    }

    #[test]
    fn the_ini_carries_extensions_settings_smtp_and_nothing_unix() {
        let settings = vec![("memory_limit".to_string(), "512M".to_string())];
        let ini = render_ini(&GROUP, Path::new("/php-8.3.32"), Path::new("/logs/php-cgi-8.3.log"), Some(&catch()), &settings);
        assert!(ini.contains("extension_dir = \""), "{ini}");
        for line in ["extension = curl", "extension = mysqli", "extension = mbstring", "zend_extension = opcache"] {
            assert!(ini.lines().any(|l| l == line), "missing {line:?}:\n{ini}");
        }
        assert!(ini.lines().any(|l| l == "SMTP = 127.0.0.1"), "{ini}");
        assert!(ini.lines().any(|l| l == format!("smtp_port = {}", super::super::mail::MAILPIT_SMTP_PORT)), "{ini}");
        assert!(ini.lines().any(|l| l == "memory_limit = 512M"), "{ini}");
        assert!(!ini.contains("sendmail_path"), "the shim needs a shell the group does not have:\n{ini}");
        assert!(!ini.contains("default_socket"), "{ini}");
    }

    /// The CLI's ini carries exactly the group's extensions and no pool concerns (no log file,
    /// no SMTP keys, no settings); the group's ini renders the same extension lines.
    #[test]
    fn the_cli_ini_loads_the_groups_extensions_and_nothing_else() {
        let dir = Path::new("/bin/php-8.3.32");
        let cli = render_cli_ini(&GROUP, dir);
        let group = render_ini(&GROUP, dir, Path::new("/l.log"), Some(&catch()), &[("memory_limit".into(), "1G".into())]);
        for line in ["extension = curl", "extension = mysqli", "extension = mbstring", "zend_extension = opcache"] {
            assert!(cli.lines().any(|l| l == line), "missing {line:?}:\n{cli}");
        }
        assert!(cli.contains(&format!("extension_dir = \"{}\"", dir.join("ext").display())), "{cli}");
        assert!(!cli.contains("SMTP") && !cli.contains("error_log") && !cli.contains("memory_limit"), "{cli}");
        assert!(group.contains(&extension_lines(&GROUP, dir)), "the group's ini must carry the same extension lines");
    }

    #[test]
    fn with_the_catch_off_there_are_no_smtp_keys_and_no_mail_env() {
        let ini = render_ini(&GROUP, Path::new("/php"), Path::new("/l.log"), None, &[]);
        assert!(!ini.contains("SMTP") && !ini.contains("smtp_port"), "{ini}");
        let env = group_env(None);
        assert!(env.iter().all(|(k, _)| !k.starts_with("MAIL_")), "{env:?}");
    }

    #[test]
    fn the_env_sizes_the_group_like_the_fpm_pool_and_carries_the_laravel_half() {
        let env = group_env(Some(&catch()));
        let get = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("PHP_FCGI_CHILDREN"), Some("10"));
        assert_eq!(get("PHP_FCGI_MAX_REQUESTS"), Some("500"));
        assert_eq!(get("MAIL_HOST"), Some("127.0.0.1"));
        assert_eq!(get("MAIL_PORT"), Some(super::super::mail::MAILPIT_SMTP_PORT.to_string().as_str()));
    }

    #[test]
    fn the_args_ignore_every_other_ini_and_bind_loopback() {
        assert_eq!(
            group_args(Path::new("/cfg/php-cgi-8.3.ini"), 9783),
            ["-n", "-c", "/cfg/php-cgi-8.3.ini", "-b", "127.0.0.1:9783"]
        );
    }

    /// The Dell's own output shapes: a clean module list passes; the measured
    /// missing-library warning refuses although the run exited 0; an extension the
    /// model asked for that is absent from the list refuses by name.
    #[test]
    fn the_preflight_reads_the_output_not_just_the_exit_code() {
        let clean = "[PHP Modules]\ncurl\nmbstring\nmysqli\nZend OPcache\n\n[Zend Modules]\nZend OPcache\n";
        assert_eq!(preflight_verdict(&GROUP, true, clean), Ok(()));

        let warned = "<b>Warning</b>:  PHP Startup: Unable to load dynamic library 'mysqli' (tried: C:\\php\\ext\\mysqli (The specified module could not be found)) in <b>Unknown</b> on line <b>0</b><br />\n[PHP Modules]\ncurl\n";
        let err = preflight_verdict(&GROUP, true, warned).unwrap_err();
        assert!(err.contains("Unable to load dynamic library 'mysqli'"), "{err}");

        let short = "[PHP Modules]\ncurl\nmbstring\n";
        let err = preflight_verdict(&GROUP, true, short).unwrap_err();
        assert!(err.contains("mysqli") && err.contains("Zend OPcache"), "{err}");

        assert!(preflight_verdict(&GROUP, false, clean).is_err());
    }
}
