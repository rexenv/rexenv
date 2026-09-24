//! The Linux DNS route, END TO END, exactly as the app installs it (docs/PLAN-linux-port.md
//! P1, ledger #717): the platform's own `install_command`/`uninstall_command` run as root, a
//! real resolver on `127.0.0.1:15353`, and `resolvectl` asked what resolves where.
//!
//! **Why this example exists.** The first Linux DNS design — a global systemd-resolved
//! drop-in with `Domains=~rex` — passed every L0 test and made `example.com` resolve to
//! `127.0.0.1` on the first Ubuntu VM run (24 Sep 2026): the mechanism was wrong in a way
//! no text test can see. This is the run that would have caught it, kept so the NEXT change
//! to the route is measured before a user meets it.
//!
//! Linux only, `system` tier: it needs `sudo` (pkexec has no terminal) and changes the
//! machine's DNS for its duration. It refuses when a rexenv route already exists (the owner's
//! own install would be torn down), and removes everything it created on every exit.
//!
//!   cargo run --example linux_dns_route_check      # on an Ubuntu host with sudo
//!
//! Elsewhere it prints a skip line.

#[cfg(target_os = "linux")]
mod common;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("linux_dns_route_check: skipped — a Linux check (docs/PLAN-linux-port.md P1)");
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    use common::Check;
    use rexenv_lib::core::dns::DEFAULT_DNS_PORT;
    use rexenv_lib::platform;
    use rexenv_lib::platform::traits::{PromptReason, ResolverOwner};
    use std::process::{Command, ExitCode};

    let plat = platform::current();
    let dns = plat.dns();
    let mut checks = Check::new("linux_dns_route_check");

    // Refuse to touch a real install: the uninstall at the end would remove the owner's route.
    let existing = dns.our_route_tlds(DEFAULT_DNS_PORT);
    if !existing.is_empty() || std::path::Path::new("/sys/class/net/rexenv0").exists() {
        eprintln!("REFUSED: a rexenv DNS route already exists on this machine ({existing:?}, or the rexenv0 link) — this check installs and removes its own");
        return ExitCode::FAILURE;
    }

    // The real resolver, in-process (the agent's semantics: every name is loopback).
    let (addr, mut server) = match rexenv_lib::core::dns::serve_udp(DEFAULT_DNS_PORT).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("REFUSED: cannot bind the resolver on 127.0.0.1:{DEFAULT_DNS_PORT}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let _serving = tokio::spawn(async move {
        let _ = server.block_until_done().await;
    });
    println!("resolver: {addr}");

    // Root through sudo (a terminal has no polkit agent); the SAME shell strings the app hands pkexec.
    let root = |script: &str| -> Result<String, String> {
        let out = Command::new("sudo").args(["-n", "/bin/sh", "-c", script]).output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    };
    let query = |name: &str| -> String {
        let out = Command::new("resolvectl").args(["query", name]).output().expect("resolvectl");
        let s = String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").to_string();
        if s.is_empty() { String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("").to_string() } else { s }
    };
    let loopback = |answer: &str| answer.contains(": 127.0.0.1");

    let before = query("example.com");
    checks.is("example.com resolves publicly before", !loopback(&before) && before.contains("example.com:"), &before);

    let _reason = PromptReason::new("route .rex for the check");
    let inst = root(&dns.install_command("rex", DEFAULT_DNS_PORT));
    checks.is("install_command(\"rex\") runs as root", inst.is_ok(), &inst.clone().err().unwrap_or_default());
    std::thread::sleep(std::time::Duration::from_secs(2));
    checks.is("route_owner(rex) = Ours", dns.route_owner("rex", DEFAULT_DNS_PORT) == ResolverOwner::Ours, "");
    checks.is("our_route_tlds = [rex]", dns.our_route_tlds(DEFAULT_DNS_PORT) == vec!["rex".to_string()], "");
    let link = Command::new("resolvectl").args(["status", "rexenv0"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let link_ok = link.contains("127.0.0.1:15353") && link.contains("~rex") && link.contains("-DefaultRoute");
    checks.is("rexenv0 carries 127.0.0.1:15353 and ~rex with no default route", link_ok, &link);
    if !link_ok {
        // The diagnosis a failure needs, printed here because the unit is gone by the end.
        for (what, cmd) in [
            ("unit status", "/usr/bin/systemctl status rexenv-dns-route.service --no-pager -l 2>&1 | head -30"),
            ("unit journal", "/usr/bin/journalctl -u rexenv-dns-route.service --no-pager -o short 2>&1 | tail -30"),
            ("script as written", "/bin/cat /usr/local/lib/rexenv/dns-route.sh"),
            ("unit as written", "/bin/cat /etc/systemd/system/rexenv-dns-route.service"),
            ("script run by hand", "/bin/sh -x /usr/local/lib/rexenv/dns-route.sh 2>&1"),
        ] {
            println!("---- {what}\n{}", root(cmd).unwrap_or_else(|e| format!("(failed: {e})")));
        }
    }
    let a = query("anything.rex");
    checks.is("anything.rex → 127.0.0.1", loopback(&a), &a);
    let sub = query("sub.site.rex");
    checks.is("sub.site.rex → 127.0.0.1 (multisite)", loopback(&sub), &sub);
    let ex = query("example.com");
    checks.is("example.com STILL public (P1 — the drop-in failed here)", !loopback(&ex) && ex.contains("example.com:"), &ex);
    let ub = query("ubuntu.com");
    checks.is("ubuntu.com still public", !loopback(&ub), &ub);

    let inst2 = root(&dns.install_command("test", DEFAULT_DNS_PORT));
    checks.is("a second TLD installs", inst2.is_ok(), &inst2.clone().err().unwrap_or_default());
    std::thread::sleep(std::time::Duration::from_secs(2));
    checks.is("our_route_tlds = [rex, test]", dns.our_route_tlds(DEFAULT_DNS_PORT) == vec!["rex".to_string(), "test".to_string()], "");
    let t = query("a.test");
    checks.is("a.test → 127.0.0.1", loopback(&t), &t);
    let b = query("b.rex");
    checks.is("b.rex still → 127.0.0.1", loopback(&b), &b);
    let ex2 = query("example.com");
    checks.is("example.com still public with two TLDs", !loopback(&ex2), &ex2);

    // A resolved restart must not lose the route (PartOf).
    let _ = root("/usr/bin/systemctl restart systemd-resolved");
    std::thread::sleep(std::time::Duration::from_secs(3));
    let after_restart = query("c.rex");
    checks.is("c.rex → 127.0.0.1 after a resolved restart", loopback(&after_restart), &after_restart);

    // Partial removal keeps the link; removing the last TLD takes it down.
    let un1 = root(&dns.uninstall_command(&["test".to_string()]));
    checks.is("uninstall(test) runs", un1.is_ok(), &un1.clone().err().unwrap_or_default());
    std::thread::sleep(std::time::Duration::from_secs(2));
    checks.is("rexenv0 still present after a partial removal", std::path::Path::new("/sys/class/net/rexenv0").exists(), "");
    checks.is("our_route_tlds = [rex] after removing test", dns.our_route_tlds(DEFAULT_DNS_PORT) == vec!["rex".to_string()], "");
    let un2 = root(&dns.uninstall_command(&["rex".to_string()]));
    checks.is("uninstall(rex) runs", un2.is_ok(), &un2.clone().err().unwrap_or_default());
    std::thread::sleep(std::time::Duration::from_secs(2));
    checks.is("rexenv0 gone after the last TLD", !std::path::Path::new("/sys/class/net/rexenv0").exists(), "");
    checks.is("unit file gone", !std::path::Path::new("/etc/systemd/system/rexenv-dns-route.service").exists(), "");
    checks.is("route_owner(rex) = Absent", dns.route_owner("rex", DEFAULT_DNS_PORT) == ResolverOwner::Absent, "");
    let ex3 = query("example.com");
    checks.is("example.com public after uninstall", !loopback(&ex3), &ex3);

    checks.verdict()
}
