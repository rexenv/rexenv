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
use crate::platform::traits::{CgiGroup, Platform};
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
    ini.push_str(&format!("extension_dir = \"{}\"\n", php_dir.join("ext").display()));
    for ext in group.extensions {
        ini.push_str(&format!("extension = {ext}\n"));
    }
    for ext in group.zend_extensions {
        ini.push_str(&format!("zend_extension = {ext}\n"));
    }
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
    let ini = config_dir.join(format!("php-cgi-{name}.ini"));
    let log = log_dir.join(format!("php-cgi-{name}.log"));
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
    let log = platform.paths().log_dir()?.join("php-cgi-stdout.log");
    platform.supervisor().spawn_logged_env(
        &php_cgi_bin(php_dir),
        &group_args(&ini, port),
        &log,
        &group_env(catch),
    )
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
