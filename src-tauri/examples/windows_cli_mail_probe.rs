//! WP-CLI's mail on Windows when the Mailpit path has a SPACE — measured before anything changes
//! (docs/TODO.md, found by `windows_wp_site_check`, ledger #606).
//!
//! ```text
//! scripts/probes/windows-example.sh dell@<host> windows_cli_mail_probe
//! ```
//!
//! A PROBE, not a proof: it prints what each shape does. WP-CLI routes `mail()` into Mailpit with
//! `-d sendmail_path=<mail::sendmail_path_cli(mailpit)>` (`wordpress::wp_argv_prefix`), a value
//! escaped for `/bin/sh`. On the Dell it delivered from `C:\Users\DELL\…`, a path with no space.
//! This runs the same `php.exe -d …` a WP-CLI spawn runs, with a `mail()` call, for:
//!
//! 1. today's shim, the Mailpit copy in a folder WITHOUT a space (the control — it must deliver);
//! 2. today's shim, the copy in a folder WITH a space;
//! 3. the space path double-quoted instead of sh-escaped;
//! 4. no sendmail at all — PHP's own `SMTP` / `smtp_port` keys, the way the php-cgi group's ini
//!    already routes a page's mail.
//!
//! For each: `mail()`'s return, what PHP read back as `sendmail_path`, anything on stderr, and
//! whether a message with that case's subject reached Mailpit's API.
//!
//! Fixture-owned: a sandboxed platform under `%TEMP%\rexenv-cli-mail-probe` holding Mailpit's
//! data and the two copies of `mailpit.exe` (removed at the end); Mailpit's fixed ports on a
//! machine with no rexenv stack (`require_stack_stopped`); the server held by `OwnedService`. The
//! binary cache is the documented exception, resolved and never modified. `demo` tier:
//! Windows-only; on macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_cli_mail_probe: skipped — a Windows probe (docs/TODO.md, #606)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check, OwnedService};
    use rexenv_lib::core::{binaries, mail};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process::ExitCode;
    use std::time::{Duration, Instant};

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_cli_mail_probe");
        let root = std::env::temp_dir().join("rexenv-cli-mail-probe");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());

        let (mailpit, php) = match (
            binaries::resolve(&*plat, "mailpit", binaries::pins().mailpit).await,
            binaries::resolve_program(&*plat, "php", binaries::pins().php).await,
        ) {
            (Ok(m), Ok(p)) => (m, p),
            (m, p) => {
                check.is("Mailpit and PHP resolve", false, &format!("{:?} {:?}", m.err(), p.err()));
                return check.verdict();
            }
        };
        let plain = root.join("nospace").join("mailpit.exe");
        let spaced = root.join("with space").join("mailpit.exe");
        for copy in [&plain, &spaced] {
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            let copied = std::fs::copy(&mailpit, copy);
            check.is(&format!("mailpit.exe copied to {}", copy.display()), copied.is_ok(), &format!("{copied:?}"));
        }

        let started = mail::start(&*plat, &mailpit);
        check.is("Mailpit starts", started.is_ok(), &format!("{:?}", started.as_ref().err()));
        let Ok(child) = started else {
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        };
        let mut server = OwnedService::new(child, "mailpit");
        common::await_listening(mail::MAILPIT_HTTP_PORT, "mailpit", None);
        common::await_listening(mail::MAILPIT_SMTP_PORT, "mailpit smtp", None);

        let smtp = mail::MAILPIT_SMTP_PORT;
        let quoted = format!("\"{}\" sendmail -t -S 127.0.0.1:{smtp}", spaced.display());
        let cases: Vec<(&str, Vec<String>)> = vec![
            ("case1 shim no space", vec!["-d".into(), format!("sendmail_path={}", mail::sendmail_path_cli(&plain))]),
            ("case2 shim with space", vec!["-d".into(), format!("sendmail_path={}", mail::sendmail_path_cli(&spaced))]),
            ("case3 quoted with space", vec!["-d".into(), format!("sendmail_path={quoted}")]),
            ("case4 smtp keys", vec!["-d".into(), "SMTP=127.0.0.1".into(), "-d".into(), format!("smtp_port={smtp}")]),
            // The first fix added an EMPTIED sendmail_path to the keys, and real WP-CLI mail then
            // vanished with `true` (windows_wp_site_check, 14 Sep 2026). Is the empty value the cause?
            (
                "case5 smtp keys with an emptied sendmail_path",
                vec!["-d".into(), "sendmail_path=".into(), "-d".into(), "SMTP=127.0.0.1".into(), "-d".into(), format!("smtp_port={smtp}")],
            ),
        ];
        let mut delivered = Vec::new();
        for (subject, flags) in &cases {
            let script = format!(
                "$r = mail('owner@probe.rex', '{subject}', 'sent by windows_cli_mail_probe', 'From: probe@probe.rex'); \
                 echo 'mail=' . var_export($r, true) . ' sendmail_path=' . var_export(ini_get('sendmail_path'), true) \
                 . ' SMTP=' . ini_get('SMTP') . ':' . ini_get('smtp_port');"
            );
            let mut args = flags.clone();
            args.push("-r".into());
            args.push(script);
            let out = std::process::Command::new(&php).args(&args).output();
            let said = match &out {
                Ok(o) => format!(
                    "exit {:?} · {} · stderr: {}",
                    o.status.code(),
                    String::from_utf8_lossy(&o.stdout).trim(),
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
                Err(e) => format!("spawn failed: {e}"),
            };
            let arrived = wait_for_mail(subject);
            println!("## {subject}\n  argv: {flags:?}\n  {said}\n  reached Mailpit: {arrived}");
            delivered.push((*subject, arrived));
        }
        check.is(
            "the control (today's shim, no space) delivers — the fixture works",
            delivered.first().is_some_and(|(_, ok)| *ok),
            "the control did not deliver",
        );

        server.stop();
        let _ = std::fs::remove_dir_all(&root);
        check.verdict()
    }

    fn mailpit_messages() -> String {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", mail::MAILPIT_HTTP_PORT)) else { return "no connection".into() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = write!(s, "GET /api/v1/messages HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn wait_for_mail(subject: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if mailpit_messages().contains(subject) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        false
    }
}
