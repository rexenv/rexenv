//! **Does wp-cli's `mail()` reach Mailpit?** (service tier)
//!
//! `sendmail_path` was set on the php-fpm pool only, so mail from a page
//! request was caught and mail from a wp-cli command was handed to the system
//! sendmail and silently dropped — `wp_mail()` returning `true` both times.
//! This runs the REAL argv builder against a REAL site and checks Mailpit's own
//! API for the message, because the only honest answer to "was it delivered"
//! comes from the sink.
//!
//! Needs a scratch site to send from and Mailpit running; it asserts both
//! rather than skipping, so a green run cannot mean "nothing was checked".
//! `cargo run --example wp_mail_sink_check`

use rexenv_lib::core::{binaries, mail, wordpress};
use rexenv_lib::platform;
use std::process::Command;

fn mailpit_total() -> u64 {
    let out = Command::new("/usr/bin/curl")
        .args(["-s", &format!("http://127.0.0.1:{}/api/v1/messages?limit=1", mail::MAILPIT_HTTP_PORT)])
        .output()
        .expect("curl mailpit");
    let body = String::from_utf8_lossy(&out.stdout);
    let at = body.find("\"total\":").expect("mailpit did not answer with a total — is it running?");
    body[at + 8..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("total was not a number")
}

fn main() {
    let plat = platform::current();

    // The FLAG must be in the argv the app actually builds. Asserted first: if
    // it is missing, the delivery check below would fail for a reason this
    // example would otherwise report as "Mailpit is down".
    let phar = binaries::cached_path(&*plat, "wp-cli", binaries::WP_CLI_VERSION)
        .expect("wp-cli is not downloaded — run the app once first");
    let argv = wordpress::wp_argv_prefix(&phar);
    let flag = argv
        .iter()
        .find(|a| a.starts_with("sendmail_path="))
        .expect("the wp-cli argv carries no sendmail_path — mail from `wp` will vanish silently");
    println!("argv carries {flag}");
    assert_eq!(argv.last().map(String::as_str), Some(phar.display().to_string().as_str()));

    let before = mailpit_total();
    println!("mailpit total before: {before}");

    // Send through the SAME builder, from a real site's docroot.
    let site = std::env::args().nth(1).unwrap_or_else(|| {
        panic!("pass a docroot: cargo run --example wp_mail_sink_check -- <docroot>")
    });
    let php = std::env::args()
        .nth(2)
        .map(std::path::PathBuf::from)
        .expect("pass the php binary as the 2nd arg");
    // Pin the wp-cli command set (#228), like every other wp-cli spawn: without
    // it this runs whatever happens to be in the machine's `~/.wp-cli/packages`,
    // and a guard in `core::wordpress` fails the build for leaving it out.
    let (pk, pv) = rexenv_lib::core::wp_packages::pin_packages_env(&phar);
    let out = Command::new(&php)
        .env(pk, pv)
        .args(&argv)
        .args([
            &format!("--path={site}"),
            "eval",
            "var_export(wp_mail('sink@example.invalid','wp-cli sink probe','probe body'));",
        ])
        .output()
        .expect("run wp eval");
    println!(
        "wp eval → {} · stdout {:?}",
        out.status,
        String::from_utf8_lossy(&out.stdout).trim()
    );

    // Give the SMTP hand-off a moment, then ask the SINK.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let after = mailpit_total();
    println!("mailpit total after:  {after}");
    assert!(
        after > before,
        "wp-cli reported success and Mailpit received nothing — the message was handed to the \
         system sendmail and dropped, which is the exact bug this example exists for"
    );
    println!("wp_mail_sink_check: PASS");
}
