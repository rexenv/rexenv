//! The DNS agent's logon task on Windows, as Task Scheduler XML (W6 S2, ledger #616). Pure text —
//! `WindowsDnsAgent` registers it with `schtasks` — so this file is compiled into the macOS test build
//! and the definition is checked in `verify.sh`.
//!
//! Every setting here was measured on the Dell (15 Sep 2026, `scripts/probes/windows-logon-task.ps1`):
//! the desktop user registers it WITHOUT elevation, and Task Scheduler keeps `InteractiveToken`,
//! `Hidden`, `ExecutionTimeLimit PT0S` (the 72-hour default would stop the resolver on day three), both
//! battery stops off (a laptop on battery would otherwise never resolve `.rex`) and `IgnoreNew`.
//! `RestartOnFailure` is deliberately absent: it did NOT restart a killed action — it covers a task that
//! fails to start. The keep-alive is instead a time trigger repeating every minute (the owner's ruling),
//! a no-op under `IgnoreNew` while the agent runs.

use std::path::Path;

/// The task's full name: the `\rexenv\` folder Task Scheduler creates on registration and removes with
/// its last task (measured).
pub(crate) const DNS_AGENT_TASK: &str = r"\rexenv\dns-agent";

/// The repeating trigger's start: a FIXED past moment, so the definition's bytes are the same on every
/// launch — `install` skips re-registering an unchanged definition, as the macOS plist does.
pub(crate) const REPEAT_FROM: &str = "2026-01-01T00:00:00";

/// `s` with the five XML special characters escaped.
pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// The task definition: run `exe --dns-agent --log "<log>"` as `user_sid`, at that user's logon and
/// every minute after `REPEAT_FROM`, one instance at a time, for as long as it runs.
pub(crate) fn dns_agent_task_xml(user_sid: &str, exe: &Path, log: &Path) -> String {
    let sid = xml_escape(user_sid);
    let command = xml_escape(&exe.display().to_string());
    let arguments = xml_escape(&format!("--dns-agent --log \"{}\"", log.display()));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>rexenv's DNS resolver: answers *.rex (and your other rexenv TLDs) with this computer. Removed by rexenv's "Remove system changes".</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{sid}</UserId>
    </LogonTrigger>
    <TimeTrigger>
      <Repetition>
        <Interval>PT1M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
      <StartBoundary>{REPEAT_FROM}</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{sid}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// The SID a REGISTERED task runs as, read off `schtasks /Query /TN … /XML`: the first `<UserId>` under
/// `<Principals>`. `None` for anything that is not a task definition with a principal.
pub(crate) fn task_principal_sid(xml: &str) -> Option<String> {
    let principals = xml.split("<Principals>").nth(1)?;
    let principals = principals.split("</Principals>").next()?;
    let sid = principals.split("<UserId>").nth(1)?.split("</UserId>").next()?.trim();
    (!sid.is_empty()).then(|| sid.to_string())
}

/// The refusal when the task belongs to another account. rexenv is one account per machine
/// (`docs/INSTALL.md`): the task's NAME is machine-wide, so the second account's `schtasks /Create
/// … /F` silently took it from the first — it then ran at the second account's logon only, until the
/// first account's next launch took it back. Found reading `install` on 29 Sep 2026; never run.
pub(crate) fn foreign_task_refusal(holder_sid: &str) -> String {
    format!(
        "the DNS agent task {DNS_AGENT_TASK} is registered for another account on this computer \
         ({holder_sid}) — rexenv is one account per machine, so this launch will not take it over; \
         `.rex` is served by this app while it runs. Use rexenv from that account, or remove its system \
         changes there first."
    )
}

/// The definition as the file `schtasks /Create /XML` reads: UTF-16 little-endian with its byte-order
/// mark, matching the `encoding="UTF-16"` the document declares (the measured shape).
pub(crate) fn utf16_file_bytes(xml: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ledger #787 — **the silent installer stops the agent's task and waits the binary free
    /// before the swap, refuses to skip a locked file, and restarts the task after; the
    /// uninstaller ends and deletes the task before it removes rexenv.exe** — the NSIS hooks
    /// Tauri includes (`tauri.conf.json` → `nsis/hooks.nsh`). `setup.exe /S` over a running
    /// rexenv returned 0 with the old binary in place (30 Sep 2026), and `uninstall.exe` left
    /// an agent answering :53 from a file it could not delete (21 Sep 2026). The task name is
    /// THIS constant's, held together here so a rename fails the build's tests.
    #[test]
    fn the_installer_hooks_stop_the_agent_for_a_silent_swap_and_take_its_task_on_uninstall() {
        let hooks = include_str!("../../../nsis/hooks.nsh");
        let conf = include_str!("../../../tauri.conf.json");
        assert!(conf.contains("\"installerHooks\": \"nsis/hooks.nsh\""), "tauri.conf.json must hand the hooks to the bundler");
        assert!(hooks.is_ascii() && !hooks.starts_with('\u{feff}'), "ASCII and no BOM — the English.nsh rule (makensis rejects a second BOM)");
        assert!(hooks.lines().any(|l| l.trim() == "AllowSkipFiles off"), "a locked file must ABORT a silent install, never be skipped: {hooks}");
        assert!(hooks.contains(&format!("!define REXENV_DNS_TASK \"{DNS_AGENT_TASK}\"")), "the hooks must name the agent task exactly as the app registers it ({DNS_AGENT_TASK})");
        let section = |name: &str| {
            let at = hooks.find(&format!("!macro {name}")).unwrap_or_else(|| panic!("the hooks must define {name}"));
            let rest = &hooks[at..];
            &rest[..rest.find("!macroend").expect("macroend")]
        };
        let pre = section("NSIS_HOOK_PREINSTALL");
        assert!(pre.contains("${If} ${Silent}") && pre.contains("!insertmacro REXENV_STOP_AGENT_AND_WAIT"), "the silent install stops the agent and waits — only the silent one: {pre}");
        let stop = section("REXENV_STOP_AGENT_AND_WAIT");
        assert!(stop.contains("schtasks /End /TN \"${REXENV_DNS_TASK}\"") && stop.contains("FindProcessCurrentUser") && stop.contains("KillProcessCurrentUser") && stop.contains("Sleep 500") && stop.contains("${LoopUntil} $1 >= 20"), "end the task, then kill and wait, bounded: {stop}");
        let post = section("NSIS_HOOK_POSTINSTALL");
        assert!(post.contains("${If} ${Silent}") && post.contains("schtasks /Run /TN \"${REXENV_DNS_TASK}\""), "the silent install brings the resolver back on the new binary: {post}");
        let preun = section("NSIS_HOOK_PREUNINSTALL");
        assert!(preun.contains("schtasks /End /TN \"${REXENV_DNS_TASK}\"") && preun.contains("schtasks /Delete /TN \"${REXENV_DNS_TASK}\" /F"), "the uninstaller ends AND deletes the task: {preun}");
        assert!(!preun.contains("${If} ${Silent}"), "the task goes with the app in every uninstall, silent or not: {preun}");
        let postun = section("NSIS_HOOK_POSTUNINSTALL");
        assert!(postun.contains("${IfNot} ${Silent}") && postun.contains("MessageBox") && postun.contains(".rex DNS rule") && postun.contains("certificate"), "the interactive uninstall still says what it did not remove: {postun}");
    }

    fn element<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
        xml.split(&format!("<{tag}>"))
            .skip(1)
            .filter_map(|rest| rest.split(&format!("</{tag}>")).next())
            .collect()
    }

    /// The measured shape, setting by setting: a logon trigger and a per-minute repetition for THIS
    /// user, the interactive token without elevation, no time limit, no battery stops, one instance,
    /// hidden — and no `RestartOnFailure`, which did not restart a killed action.
    #[test]
    fn the_agent_task_runs_the_resolver_at_logon_and_every_minute_for_as_long_as_it_lives() {
        let sid = "S-1-5-21-3487155226-1665577948-3202841263-1001";
        let xml = dns_agent_task_xml(
            sid,
            Path::new(r"C:\Program Files\rexenv\rexenv.exe"),
            Path::new(r"C:\Users\DELL\AppData\Local\rexenv\rexenv\data\logs\dns-agent.log"),
        );
        assert!(xml.contains("<LogonTrigger>"), "{xml}");
        assert_eq!(element(&xml, "UserId"), vec![sid, sid], "trigger and principal are this user");
        assert_eq!(element(&xml, "Interval"), vec!["PT1M"]);
        assert_eq!(element(&xml, "StartBoundary"), vec![REPEAT_FROM]);
        assert_eq!(element(&xml, "LogonType"), vec!["InteractiveToken"]);
        assert_eq!(element(&xml, "RunLevel"), vec!["LeastPrivilege"]);
        assert_eq!(element(&xml, "ExecutionTimeLimit"), vec!["PT0S"]);
        assert_eq!(element(&xml, "DisallowStartIfOnBatteries"), vec!["false"]);
        assert_eq!(element(&xml, "StopIfGoingOnBatteries"), vec!["false"]);
        assert_eq!(element(&xml, "MultipleInstancesPolicy"), vec!["IgnoreNew"]);
        assert_eq!(element(&xml, "Hidden"), vec!["true"]);
        assert!(!xml.contains("RestartOnFailure"), "measured not to restart a killed action");
        assert_eq!(element(&xml, "Command"), vec![r"C:\Program Files\rexenv\rexenv.exe"]);
        assert_eq!(
            element(&xml, "Arguments"),
            vec![r"--dns-agent --log &quot;C:\Users\DELL\AppData\Local\rexenv\rexenv\data\logs\dns-agent.log&quot;"]
        );
        // Deterministic: the same inputs give the same bytes, so an unchanged definition is skipped.
        assert_eq!(xml, dns_agent_task_xml(sid, Path::new(r"C:\Program Files\rexenv\rexenv.exe"), Path::new(r"C:\Users\DELL\AppData\Local\rexenv\rexenv\data\logs\dns-agent.log")));
    }

    /// A user folder may hold `&` or an apostrophe; the document must stay XML.
    /// **A registered task's principal is read back, and another account's is refused** — the
    /// definition rexenv writes parses to the SID it was given, a foreign one names the holder in the
    /// refusal, and a non-task answers nothing. TEXT: `install` asks BEFORE `/Create`, so the `/F`
    /// never lands on another account's task.
    #[test]
    fn another_accounts_task_is_never_taken_over() {
        let ours = "S-1-5-21-3487155226-1665577948-3202841263-1001";
        let theirs = "S-1-5-21-3487155226-1665577948-3202841263-1002";
        let xml = dns_agent_task_xml(theirs, Path::new(r"C:\x\rexenv.exe"), Path::new(r"C:\x\a.log"));
        assert_eq!(task_principal_sid(&xml).as_deref(), Some(theirs));
        assert_eq!(task_principal_sid("<?xml version=\"1.0\"?><Task></Task>"), None);
        assert_eq!(task_principal_sid("ERROR: The system cannot find the file specified."), None);
        let why = foreign_task_refusal(theirs);
        assert!(why.contains(theirs) && why.contains(DNS_AGENT_TASK) && why.contains("one account per machine"), "{why}");
        assert_ne!(task_principal_sid(&xml).as_deref(), Some(ours));
        let src = include_str!("mod.rs");
        let install = src.split("    fn install(&self, exe: &Path, log: &Path) -> Result<()> {\n        let sid = acl::current_user_sid()?;").nth(1).expect("WindowsDnsAgent::install");
        let install = &install[..install.find("\n    }\n").expect("its end")];
        let pos = |needle: &str| install.find(needle).unwrap_or_else(|| panic!("`{needle}` is gone from install"));
        assert!(pos("task_principal_sid(") < pos("foreign_task_refusal(") && pos("foreign_task_refusal(") < pos("\"/Create\""), "the principal is checked, and refused, before /Create");
    }

    #[test]
    fn paths_with_xml_characters_are_escaped() {
        let xml = dns_agent_task_xml("S-1-5-21-1", Path::new(r"C:\Users\A & B\rexenv.exe"), Path::new(r"C:\Users\O'Neil <x>\dns.log"));
        assert_eq!(element(&xml, "Command"), vec![r"C:\Users\A &amp; B\rexenv.exe"]);
        assert_eq!(element(&xml, "Arguments"), vec![r"--dns-agent --log &quot;C:\Users\O&apos;Neil &lt;x&gt;\dns.log&quot;"]);
        assert!(!xml.contains(" & "), "a raw ampersand");
    }

    #[test]
    fn the_file_is_utf16_little_endian_with_its_byte_order_mark() {
        let bytes = utf16_file_bytes("<a>é</a>");
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "<a>é</a>");
    }
}
