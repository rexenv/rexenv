//! Live check (sandbox tier): the mail catch-all reaches a FRANKENPHP backend —
//! ledger #514. Run:
//!
//!   cargo run --example frankenphp_mail_catch_check
//!
//! A FrankenPHP site has no php-fpm pool, so neither half of the catch-all
//! (`php_admin_value[sendmail_path]`, `env[MAIL_*]`) ever reached it: a Laravel
//! site on FrankenPHP with a real `MAIL_HOST` in `.env` delivered for real while
//! the Settings card said every site's mail was caught (found by the 5 Sep 2026
//! audit). The fix renders the pool's shim as `php_ini sendmail_path` in the
//! generated FrankenPHP config and carries `MAIL_*` on the backend's process
//! environment. L0 holds the config text; this is the leg only the real binary
//! can answer, and it asks the two questions that matter over the wire:
//!
//!   1. `mail()` inside the embedded PHP runs OUR shim — a fake Mailpit at a
//!      path WITH A SPACE (the shape app-data has), which records its argv and
//!      the message it was handed. The space is the whole point: measured on
//!      the pinned 1.12.4 build, a shim without the inner double quotes reaches
//!      PHP with its single quotes stripped and `sh` splits the path.
//!   2. `getenv('MAIL_HOST')` inside the embedded PHP is Mailpit's — the
//!      process-environment half, which is what Laravel's immutable Dotenv
//!      repository honours over the app's own `.env`.
//!
//! A NEGATIVE CONTROL runs first on the same port with the catch OFF: the shim
//! absent and `getenv` empty. Without it, "the shim was there" cannot be told
//! from "this PHP had it from somewhere else", and a fake sendmail that was
//! never invoked would read the same as one the assertion forgot to check.
//!
//! Everything it writes lives in `common::sandbox`; the binary is the shared
//! cache (no download when warm); the port is a fixture port outside the
//! override range, gated free before anything is spawned.

use rexenv_lib::core::services::RewriteMode;
use rexenv_lib::core::{binaries, frankenphp, mail, ports};
use std::path::Path;

mod common;

/// Outside the `8200..8300` override range, so a running stack's own backends
/// can never be the thing this talks to.
const PORT: u16 = 8379;
const DOMAIN: &str = "fpmail.test";
const SUBJECT: &str = "rexenv-fp-mail-514";

/// One answered request, or the check's OWN failure — never an empty body that a
/// `!contains(…)` check reads as a pass. Bounded since 13 Sep 2026 (a backend that
/// accepted and never answered held 0.7.1's release gate for 50 minutes); since 29 Sep
/// 2026 an empty or unanswered request is recorded as a failed check naming curl's exit
/// and the backend's own log, because on 14 Sep the control's empty body passed the
/// "does not carry our shim" check vacuously and the run was red for a reason no line
/// stated (`docs/TODO.md`, ledger #754).
fn get(check: &mut common::Check, label: &str, path: &str, log: &Path) -> String {
    match common::http_get_bounded(PORT, DOMAIN, path, 60) {
        Ok(body) => body,
        Err(why) => {
            check.is(label, false, &format!("{why}\n  {}", common::log_tail(log, 30)));
            String::new()
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let (plat, sandbox) = common::sandbox("fpmail");
    let mut check = common::Check::new("frankenphp_mail_catch_check");

    if let Err(e) = ports::ensure_free(&*plat, PORT, ports::Proto::Tcp, "FrankenPHP") {
        eprintln!("port {PORT} busy: {e}");
        return std::process::ExitCode::FAILURE;
    }

    // The docroot: one page that mails, one that reads the environment back.
    let docroot = sandbox.root().join("docroot");
    std::fs::create_dir_all(&docroot).expect("docroot");
    std::fs::write(
        docroot.join("m.php"),
        format!("<?php var_export(mail('to@example.test', '{SUBJECT}', 'body-514')); echo \"\\n\";"),
    )
    .expect("m.php");
    std::fs::write(
        docroot.join("e.php"),
        "<?php header('Content-Type: text/plain');\n\
         echo 'sendmail_path=', ini_get('sendmail_path'), \"\\n\";\n\
         echo 'MAIL_HOST=', var_export(getenv('MAIL_HOST'), true), \"\\n\";\n\
         echo 'MAIL_URL=', var_export(getenv('MAIL_URL'), true), \"\\n\";\n\
         echo 'ENV_MAIL_PORT=', var_export($_ENV['MAIL_PORT'] ?? null, true), \"\\n\";\n",
    )
    .expect("e.php");

    // The fake Mailpit, at a path WITH A SPACE — app-data's own shape — that
    // records what it was handed. `sendmail -t` reads the message on stdin.
    let sent = sandbox.root().join("sent.txt");
    let fake_dir = sandbox.root().join("App Support");
    std::fs::create_dir_all(&fake_dir).expect("fake dir");
    let fake = fake_dir.join("mailpit");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\nprintf 'ARGV:%s\\n' \"$@\" > \"{}\"\ncat >> \"{}\"\n", sent.display(), sent.display()),
    )
    .expect("fake mailpit");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    let bin = binaries::resolve(&*plat, "frankenphp", binaries::pins().frankenphp)
        .await
        .expect("frankenphp binary (cached)");
    // The backend's own stdout/stderr (under the sandbox) — spilled by every failure below.
    let log = frankenphp::log_path(&*plat, DOMAIN).expect("frankenphp log path");

    // ── Negative control: catch OFF ──────────────────────────────────────────
    {
        let conf = frankenphp::write_config(&*plat, DOMAIN, &docroot, PORT, RewriteMode::Single, &[], None)
            .expect("config (off)");
        let mut child = common::OwnedService::new(
            frankenphp::start(&*plat, &bin, DOMAIN, &conf, &[]).expect("start frankenphp (off)"),
            "frankenphp",
        );
        // ANSWERING, not merely listening: FrankenPHP accepts before its worker is up, and
        // the port poll alone let a request in that was never answered (13 and 14 Sep 2026).
        common::await_http_answer(PORT, DOMAIN, "/e.php", "frankenphp (catch off)", Some(&log));
        let e = get(&mut check, "control backend answered /e.php", "/e.php", &log);
        check.is(
            "control: with the catch OFF the embedded PHP does not carry our shim",
            !e.is_empty() && !e.contains("App Support/mailpit"),
            if e.is_empty() { "no body to judge — the request above failed" } else { &e },
        );
        check.is("control: with the catch OFF getenv('MAIL_HOST') is empty", e.contains("MAIL_HOST=false"), &e);
        child.stop();
        if !ports::wait_free(&*plat, PORT, ports::Proto::Tcp, 40, std::time::Duration::from_millis(100)) {
            check.is("control backend released the port", false, "still listening");
        }
    }

    // ── The real thing: catch ON ─────────────────────────────────────────────
    let shim = mail::sendmail_path(&fake);
    let env: Vec<(String, String)> =
        mail::laravel_env().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    let conf = frankenphp::write_config(&*plat, DOMAIN, &docroot, PORT, RewriteMode::Single, &env, Some(&shim))
        .expect("config (on)");
    let mut child = common::OwnedService::new(
        frankenphp::start(&*plat, &bin, DOMAIN, &conf, &env).expect("start frankenphp (on)"),
        "frankenphp",
    );
    common::await_http_answer(PORT, DOMAIN, "/e.php", "frankenphp (catch on)", Some(&log));

    let e = get(&mut check, "catch-on backend answered /e.php", "/e.php", &log);
    check.is(
        "ini_get('sendmail_path') is the shim VERBATIM — single quotes kept, path intact",
        e.contains(&format!("sendmail_path={shim}\n")),
        &e,
    );
    check.is("getenv('MAIL_HOST') is Mailpit's loopback", e.contains("MAIL_HOST='127.0.0.1'"), &e);
    check.is("getenv('MAIL_URL') is the literal `null` Laravel maps to a real null", e.contains("MAIL_URL='null'"), &e);
    check.is(
        "$_ENV carries the port too (Dotenv's second adapter)",
        e.contains(&format!("ENV_MAIL_PORT='{}'", mail::MAILPIT_SMTP_PORT)),
        &e,
    );

    let _ = std::fs::remove_file(&sent);
    let m = get(&mut check, "catch-on backend answered /m.php", "/m.php", &log);
    check.is("mail() returned true through the embedded PHP", m.trim() == "true", &m);
    let recorded = std::fs::read_to_string(&sent).unwrap_or_default();
    check.is(
        "the fake Mailpit at a path with a space RAN, with sendmail's argv",
        recorded.contains("ARGV:sendmail\nARGV:-t\nARGV:-S\n")
            && recorded.contains(&format!("ARGV:127.0.0.1:{}\n", mail::MAILPIT_SMTP_PORT)),
        if recorded.is_empty() { "nothing recorded — the shim never ran (the path split, or mail() went elsewhere)" } else { &recorded },
    );
    check.is(
        "…and was handed the message",
        recorded.contains(&format!("Subject: {SUBJECT}")) && recorded.contains("body-514"),
        &recorded,
    );

    child.stop();
    check.verdict()
}
