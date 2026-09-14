//! W6 S4, ledger #618: a TLD's route on Windows is an NRPT rule, read and written through
//! `DnsManager` — `route_owner`, `our_route_tlds`, `foreign_route_tlds` from the registry, and the
//! install / uninstall / restore PowerShell run elevated — including R3's takeover of a rule another tool
//! owns that names more than one namespace.
//!
//! ```text
//! scripts/probes/windows-example.sh <host> windows_nrpt_route_check     # the SSH session is elevated
//! ```
//!
//! `DnsManager`'s commands are rexenv OPS (`nrpt-install …`, ledger #619); they are run here through
//! `platform::run_elevated_ops_in_this_process` — the elevated step's own body, with the SSH session's
//! elevated token standing in for UAC (the dialog and UAC are `windows_uac_step_check`'s). What this checks
//! is the rules and what Windows' resolver does with them. Test TLDs only — `.rexnrptcheck` and
//! `.rexnrptother` — never `.rex`.
//!
//! 1. Nothing routes either test TLD first; the agent (this example re-run as `agent`) answers on :53.
//! 2. `install_command` → the route is ours and in `our_route_tlds`; `a.rexnrptcheck` resolves to
//!    127.0.0.1 through Windows' own resolver (no `-Server`). `uninstall_command` → absent again, and the
//!    name no longer resolves.
//! 3. Another tool's rule naming BOTH test TLDs → the route is foreign, its content the whole rule, and
//!    both TLDs are in `foreign_route_tlds`.
//! 4. The takeover: the content saved where rexenv keeps it (`<app data>\\resolver-backups\\rexnrptcheck`, the
//!    only place the elevated step reads a restore from), `install_command` run → the route is ours, and
//!    the other tool's rule — the same key — still routes `.rexnrptother`.
//! 5. The hand-back, joined as the core joins privileged steps (`uninstall ; restore`) → rexenv's rule is
//!    gone and the other tool's rule, the same key, names both TLDs again.
//!
//! Fixture-owned: a guard removes every rule naming a test TLD, the test TLD's backup file and stops the
//! agent, on every path; the check refuses to start if that backup file already exists. `demo` tier.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_nrpt_route_check: skipped — a Windows check (ledger #618, W6)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("agent") {
        std::process::exit(rexenv_lib::core::dns::run_agent());
    }
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use base64::Engine;
    use rexenv_lib::core::dns::{self, ResolverOwner};
    use rexenv_lib::platform::traits::Platform;
    use std::path::PathBuf;
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::time::Duration;

    const TLD: &str = "rexnrptcheck";
    const OTHER: &str = "rexnrptother";

    /// Run `script`; `(succeeded, stdout, stdout + stderr)`. Progress records are silenced: PowerShell writes
    /// them to stderr as CLIXML ("Preparing modules for first use"), and the first run of this check read
    /// that as part of an answer that was, in fact, 127.0.0.1.
    fn powershell_out(script: &str) -> (bool, String, String) {
        let script = format!("$ProgressPreference = 'SilentlyContinue'\n{script}");
        let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
        match Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded]).output() {
            Ok(o) => {
                let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let all = format!("{out} {}", String::from_utf8_lossy(&o.stderr)).trim().to_string();
                (o.status.success(), out, all)
            }
            Err(e) => (false, String::new(), e.to_string()),
        }
    }

    fn powershell(script: &str) -> (bool, String) {
        let (ok, _, all) = powershell_out(script);
        (ok, all)
    }

    /// Run `DnsManager` ops as the elevated step does, in this (elevated) process.
    fn run_ops(ops: &str) -> (bool, String) {
        let (code, out) = rexenv_lib::platform::run_elevated_ops_in_this_process(ops);
        (code == 0, format!("exit {code}: {out}"))
    }

    fn resolves(name: &str) -> String {
        powershell_out(&format!(
            "try {{ (Resolve-DnsName {name} -Type A -QuickTimeout -ErrorAction Stop | Where-Object Type -eq 'A').IPAddress }} catch {{ 'ERR ' + $_.Exception.Message }}"
        ))
        .1
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
    }

    /// The other tool's rule(s) naming `tld`, as the route's foreign content records them.
    fn foreign_rules(plat: &dyn Platform, tld: &str) -> Vec<serde_json::Value> {
        match plat.dns().route_owner(tld, 53) {
            ResolverOwner::Foreign { content: Some(c) } => serde_json::from_str(&c).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    struct Guard(Option<Child>, PathBuf);

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = powershell(&format!(
                "foreach ($r in @(Get-DnsClientNrptRule | Where-Object {{ $_.Namespace -contains '.{TLD}' -or $_.Namespace -contains '.{OTHER}' }})) {{ Remove-DnsClientNrptRule -Name $r.Name -Force }}"
            ));
            let _ = std::fs::remove_file(&self.1);
            if let Some(mut c) = self.0.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_nrpt_route_check");
        let plat = rexenv_lib::platform::current();
        let backup: PathBuf = plat.paths().app_data_dir().expect("app data").join("resolver-backups").join(TLD);
        if backup.exists() {
            check.is("no backup for the test TLD exists before the check", false, &backup.display().to_string());
            return check.verdict();
        }

        // ── 1. Nothing routes the test TLDs; the agent answers. ──
        let clean = plat.dns().route_owner(TLD, 53) == ResolverOwner::Absent && plat.dns().route_owner(OTHER, 53) == ResolverOwner::Absent;
        check.is("no rule routes the test TLDs before the check", clean, "a rule already names one");
        if !clean || dns::answers_as_ours(53) {
            if dns::answers_as_ours(53) {
                check.is("no rexenv resolver already answers :53", false, "stop it first");
            }
            return check.verdict();
        }
        let exe = std::env::current_exe().expect("exe");
        let mut guard = Guard(Command::new(&exe).arg("agent").stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok(), backup.clone());
        let mut up = false;
        for _ in 0..40 {
            if dns::answers_as_ours(53) {
                up = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        check.is("the agent answers on 127.0.0.1:53", up, "no answer");

        // ── 2. Install, resolve, uninstall. ──
        let (ok, out) = run_ops(&plat.dns().install_command(TLD, 53));
        check.is("install_command runs elevated", ok, &out);
        check.is("the route is then ours", plat.dns().route_owner(TLD, 53) == ResolverOwner::Ours, &format!("{:?}", plat.dns().route_owner(TLD, 53)));
        check.is("our_route_tlds names it", plat.dns().our_route_tlds(53).contains(&TLD.to_string()), &format!("{:?}", plat.dns().our_route_tlds(53)));
        let answer = resolves(&format!("a.{TLD}"));
        check.is("Windows' resolver (no -Server) sends a subdomain of it to rexenv: 127.0.0.1", answer == "127.0.0.1", &answer);
        let (ok, out) = run_ops(&plat.dns().uninstall_command(&[TLD.to_string()]));
        check.is("uninstall_command runs elevated", ok, &out);
        check.is("the route is absent again", plat.dns().route_owner(TLD, 53) == ResolverOwner::Absent, &format!("{:?}", plat.dns().route_owner(TLD, 53)));
        let gone = resolves(&format!("a.{TLD}"));
        check.is("and the name no longer resolves", gone.starts_with("ERR"), &gone);

        // ── 3. Another tool's rule naming both test TLDs. ──
        let (ok, out) = powershell(&format!(
            "Add-DnsClientNrptRule -Namespace '.{TLD}','.{OTHER}' -NameServers '10.9.9.9' -Comment 'another tool' | Out-Null"
        ));
        check.is("another tool's two-namespace rule is planted", ok, &out);
        let theirs = foreign_rules(&*plat, TLD);
        let key = theirs.first().and_then(|r| r["key"].as_str()).unwrap_or_default().to_string();
        println!("  · their rule: {theirs:?}");
        check.is("the route is foreign, its content the whole rule (both namespaces)", theirs.len() == 1 && theirs[0]["namespaces"].as_array().map(|a| a.len()) == Some(2), &format!("{theirs:?}"));
        let foreign = plat.dns().foreign_route_tlds(53);
        check.is("foreign_route_tlds names both test TLDs", foreign.contains(&TLD.to_string()) && foreign.contains(&OTHER.to_string()), &format!("{foreign:?}"));

        // ── 4. Takeover. ──
        let content = match plat.dns().route_owner(TLD, 53) {
            ResolverOwner::Foreign { content: Some(c) } => c,
            other => {
                check.is("a foreign route with content to back up", false, &format!("{other:?}"));
                return check.verdict();
            }
        };
        std::fs::create_dir_all(backup.parent().expect("backup dir")).expect("backup dir");
        std::fs::write(&backup, &content).expect("backup");
        let (ok, out) = run_ops(&plat.dns().install_command(TLD, 53));
        check.is("install_command over the foreign rule runs elevated", ok, &out);
        check.is("the route is ours after the takeover", plat.dns().route_owner(TLD, 53) == ResolverOwner::Ours, &format!("{:?}", plat.dns().route_owner(TLD, 53)));
        let left = foreign_rules(&*plat, OTHER);
        println!("  · their rule after the takeover: {left:?}");
        check.is(
            "their rule — the same key — still routes the other TLD, and only it",
            left.len() == 1 && left[0]["key"].as_str() == Some(key.as_str()) && left[0]["namespaces"] == serde_json::json!([format!(".{OTHER}")]),
            &format!("{left:?}"),
        );
        let answer = resolves(&format!("b.{TLD}"));
        check.is("the taken-over TLD resolves to rexenv", answer == "127.0.0.1", &answer);

        // ── 5. Hand back, joined as the core joins privileged steps. ──
        let joined = [plat.dns().uninstall_command(&[TLD.to_string()]), plat.dns().restore_command(&[(TLD.to_string(), backup.clone())])].join(" ; ");
        let (ok, out) = run_ops(&joined);
        check.is("uninstall ; restore runs elevated as one script", ok, &out);
        let back = foreign_rules(&*plat, TLD);
        println!("  · their rule after the hand-back: {back:?}");
        check.is(
            "their rule — the same key — names both TLDs again",
            back.len() == 1 && back[0]["key"].as_str() == Some(key.as_str()) && back[0]["namespaces"].as_array().map(|a| a.len()) == Some(2),
            &format!("{back:?}"),
        );
        check.is("no rexenv rule is left for the TLD", !plat.dns().our_route_tlds(53).contains(&TLD.to_string()), &format!("{:?}", plat.dns().our_route_tlds(53)));

        drop(guard.0.take().map(|mut c| {
            let _ = c.kill();
            c.wait()
        }));
        drop(guard);
        let after = plat.dns().route_owner(TLD, 53) == ResolverOwner::Absent && plat.dns().route_owner(OTHER, 53) == ResolverOwner::Absent;
        check.is("after cleanup no rule names either test TLD", after, "");
        check.verdict()
    }
}
