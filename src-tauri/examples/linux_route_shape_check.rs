//! Live check (Linux only, sandbox tier, READ-ONLY): how THIS build classifies the DNS route
//! that is installed on this machine — ledger #769.
//! Run: `cargo run --example linux_route_shape_check -- [--expect clean|notice|absent]`
//!
//! # What this proves that L0 cannot
//!
//! The L0 test feeds `dnsroute::classify` the VM's `resolvectl status` text verbatim and 0.8.10's
//! unit derived from this build's by its one changed line. This asks the REAL platform object —
//! the real marker under `/etc/rexenv/dns.d`, the real `resolvectl`, the real script and unit on
//! disk — and prints what `route_owner` and `route_notice` answer, which is what `rex status`,
//! the Settings card and onboarding's gate read. With `--expect` it is a check: the SMOKE Linux
//! row writes 0.8.10's `ExecStop` by hand and expects `notice`, re-applies and expects `clean`.
//!
//! # Fixture-owned
//!
//! Nothing is written, spawned for effect or deleted: two files are read, `resolvectl status` is
//! asked (read-only, no root). The re-apply the row then runs is `rex tld --repair rex` — the
//! app's own privileged step, never this example.

#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use std::process::ExitCode;

mod common;

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    use rexenv_lib::core::dns::ResolverOwner;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let expect = args.iter().position(|a| a == "--expect").and_then(|i| args.get(i + 1)).map(String::as_str);
    let mut checks = common::Check::new("linux_route_shape_check");
    let platform = rexenv_lib::platform::current();
    let dns = platform.dns();
    let tld = rexenv_lib::core::tld::BACKBONE_TLD;
    let port = rexenv_lib::core::dns::DEFAULT_DNS_PORT;

    let unit = std::fs::read_to_string("/etc/systemd/system/rexenv-dns-route.service").unwrap_or_default();
    let exec_stop = unit.lines().find(|l| l.starts_with("ExecStop=")).unwrap_or("(no unit on disk)");
    let status = std::process::Command::new("resolvectl")
        .args(["status", "rexenv0"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let live: Vec<&str> = status
        .lines()
        .filter(|l| l.contains("Current Scopes") || l.contains("DNS Servers") || l.contains("DNS Domain"))
        .map(str::trim)
        .collect();
    let owner = dns.route_owner(tld, port);
    let notice = dns.route_notice(tld, port);
    println!("unit:   {exec_stop}");
    println!("link:   {}", if live.is_empty() { "(no rexenv0 status)".to_string() } else { live.join(" · ") });
    println!("owner:  {owner:?}");
    println!("notice: {}", notice.as_deref().unwrap_or("(none)"));

    match expect {
        None => {
            println!("\n(no --expect: printed what this build sees; nothing asserted)");
        }
        Some("clean") => {
            checks.is("the route is ours", owner == ResolverOwner::Ours, &format!("{owner:?}"));
            checks.is("and this build's shape — no notice", notice.is_none(), notice.as_deref().unwrap_or(""));
        }
        Some("notice") => {
            checks.is("the route is OURS although the shape is older — never Absent", owner == ResolverOwner::Ours, &format!("{owner:?} — the 30 Sep 2026 failure read Absent here"));
            checks.is(
                "and the notice names the TLD and the re-apply",
                notice.as_deref().is_some_and(|n| n.contains(&format!(".{tld}")) && n.contains("older rexenv") && n.contains("re-apply")),
                notice.as_deref().unwrap_or("(none)"),
            );
        }
        Some("absent") => {
            checks.is("the route reads Absent", owner == ResolverOwner::Absent, &format!("{owner:?}"));
            checks.is("and carries no notice", notice.is_none(), notice.as_deref().unwrap_or(""));
        }
        Some(other) => {
            eprintln!("linux_route_shape_check: --expect takes clean, notice or absent (got `{other}`)");
            return ExitCode::FAILURE;
        }
    }
    checks.verdict()
}

/// The Linux route is `platform/linux/dnsroute.rs`'s; elsewhere there is nothing to classify.
#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("linux_route_shape_check: skipped — Linux-only (the dummy-link DNS route)");
    ExitCode::SUCCESS
}
