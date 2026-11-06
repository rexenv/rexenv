//! `rexenv.exe --headless-setup` (docs/PLAN-headless-web.md T5 W2): the administrator's one-time step
//! for a Windows PC that runs rexenv with nobody logged on. The rules and words are
//! `headless_rules.rs`; this file makes the changes.
//!
//! **Why an administrator console, and why the same account.** A headless launch is session 0 — an
//! S4U boot task — where no dialog can be answered (§2b, #943), so everything the desktop asks UAC for
//! has to happen here, once: the `.rex` NRPT rule (run in-process, because this process is already
//! elevated — `WindowsPrivileges::run_privileged`), the CA's trust, and the two S4U boot tasks, which
//! only an administrator may register. The tasks run as the token that registered them and the consent
//! goes into that account's own database, so the console must be the account rexenv runs as — an
//! administrator's own elevated console, not another administrator's.

use super::headless_rules as rules;
use super::{acl, logon_task, WindowsDnsAgent, WindowsPaths};
use crate::core::headless_consent::Consent;
use crate::error::{Error, Result};
use crate::platform::traits::{DnsAgentManager, Paths};

/// `None` when argv is not the setup; otherwise the exit code, after printing what was done.
pub(crate) fn run_setup(argv: &[String]) -> Option<i32> {
    argv.iter().position(|a| a == rules::SETUP_FLAG)?;
    Some(match setup(argv) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            0
        }
        Err(e) => {
            eprintln!("rexenv --headless-setup: {e}");
            1
        }
    })
}

fn setup(argv: &[String]) -> Result<Vec<String>> {
    use crate::core::{setup, ssl, tld};
    let args = rules::parse_setup_args(argv).map_err(Error::Other)?;
    // Everything that can refuse, refuses before the first change.
    if !acl::process_is_elevated() {
        return Err(Error::Other(rules::NOT_ELEVATED.into()));
    }
    let exe = std::env::current_exe()?;
    if rules::is_build_output(&exe) {
        return Err(Error::Other(rules::BUILD_OUTPUT.into()));
    }
    let platform = crate::platform::current();
    let ca_trusted = ssl::ca_dir(platform.paths())
        .map(|dir| dir.join(ssl::CA_CERT_FILE))
        .is_ok_and(|cert| cert.exists() && platform.cert_trust().is_trusted(&cert));
    if !ca_trusted && acl::session_id() == Some(0) {
        return Err(Error::Other(rules::CA_NEEDS_DESKTOP.into()));
    }
    let sid = acl::current_user_sid()?;
    // Typed consent wins; with none, ask at the console — and with no console to ask at, change neither.
    let consent = if !args.consent.is_empty() {
        args.consent
    } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        ask()?
    } else {
        Consent::default()
    };

    let mut said = vec![format!(
        "rexenv.exe --headless-setup for this account ({}), registering {}.",
        std::env::var("USERNAME").unwrap_or_default(),
        exe.display()
    )];
    // 1. The desktop's own first-run setup: the `.rex` rule (in-process, elevated) and the CA's trust.
    let ca = setup::run_system_setup(platform.as_ref())?;
    said.push(format!(
        "✓ .{} names resolve to this PC, and rexenv's local CA ({}) is trusted by this account.",
        tld::BACKBONE_TLD,
        ca.cert_path.display()
    ));
    // 2. The app's boot task. Written BEFORE the agent's, because the agent's `install` asks whether it
    //    exists to choose a boot task over a logon one (`WindowsDnsAgent::start`).
    let xml = logon_task::headless_task_xml(&sid, &exe, args.web);
    let file = WindowsPaths.config_dir()?.join("headless-task.xml");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&file, logon_task::utf16_file_bytes(&xml))?;
    schtasks_ok(&["/Create", "/TN", logon_task::HEADLESS_TASK, "/XML", &file.display().to_string(), "/F"])?;
    said.push(format!(
        "✓ {} starts rexenv headless as this account at every boot, with nobody logged on (and restarts it within a minute if it stops).",
        logon_task::HEADLESS_TASK
    ));
    // 3. The DNS agent, re-registered at boot by the same code every launch uses — so a launch's refresh
    //    keeps it a boot task.
    let log = platform.paths().log_dir()?.join("dns-agent.log");
    WindowsDnsAgent.install(&exe, &log)?;
    said.push(format!("✓ {} now starts at boot too, so .rex resolves with nobody logged on.", logon_task::DNS_AGENT_TASK));
    said.push(if args.web {
        "✓ web UI: requested (--web). On Windows it cannot answer yet with nobody logged on — session 0 starts no \
         WebView2 (docs/PLAN-headless-web.md §2b); `rex` and `rex mcp` drive this PC."
            .to_string()
    } else {
        format!("web UI: off (re-run with {} to request it).", rules::ENABLE_WEB_FLAG)
    });
    // 4. D7's two consents, into this account's own database (ledger #920).
    if consent.is_empty() {
        said.push(rules::CONSENT_UNCHANGED.into());
    } else {
        let conn = crate::state::db::open_for_platform(platform.paths())?;
        said.extend(crate::core::headless_consent::apply(&conn, &consent)?.into_iter().map(|l| format!("✓ {l}")));
    }
    said.push(format!(
        "Start it now — quit the rexenv app first (a second rexenv hands off to it and exits):\n> schtasks /Run /TN {}\nor restart the PC.",
        logon_task::HEADLESS_TASK
    ));
    Ok(said)
}

/// `schtasks` for the setup's own task, failing with what it printed.
fn schtasks_ok(args: &[&str]) -> Result<()> {
    let out = std::process::Command::new("schtasks").args(args).output()?;
    if out.status.success() {
        return Ok(());
    }
    let said = format!("{} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Err(Error::Other(format!(
        "could not register {}: {}",
        logon_task::HEADLESS_TASK,
        said.split_whitespace().collect::<Vec<_>>().join(" ")
    )))
}

/// Ask the two consents at the console, with the Settings card's own sentences (D7).
fn ask() -> Result<Consent> {
    use crate::core::headless_consent::{access_question, mcp_question, parse_level, parse_yes};
    fn prompt<T>(question: &str, parse: impl Fn(&str) -> Option<T>) -> Result<T> {
        use std::io::Write;
        loop {
            print!("{question}");
            std::io::stdout().flush()?;
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line)? == 0 {
                return Err(Error::Other("no answer (end of input) — nothing was changed".into()));
            }
            if let Some(v) = parse(&line) {
                return Ok(v);
            }
        }
    }
    let mcp = prompt(&mcp_question(), parse_yes)?;
    let access = if mcp { Some(prompt(&format!("\n{}", access_question()), parse_level)?) } else { None };
    Ok(Consent { mcp: Some(mcp), access })
}
