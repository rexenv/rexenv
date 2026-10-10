//! The pure text rules behind `EdgeSupervisor::supervised_process_age` (ledger #851): the pid an OS
//! supervisor reports for the edge, and how long `ps` says a process has run. Tested on every host;
//! the commands that produce the text run only on their own OS (`platform/macos`, `platform/linux`).

use std::time::Duration;

/// The job's CURRENT pid from `launchctl print system/<label>`: the top-level `pid = N` line, which
/// launchd prints only while the job has a process (spawned — still inside `xpcproxy` — or running).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn launchctl_pid(print: &str) -> Option<u32> {
    print
        .lines()
        .find_map(|l| l.trim().strip_prefix("pid = "))
        .and_then(|v| v.trim().parse().ok())
        .filter(|&p| p > 0)
}

/// systemd's `MainPID` value (`systemctl show -p MainPID --value <unit>`): `0` means no process.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn systemd_main_pid(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|&p| p > 0)
}

/// `ps -o etime=`: `[[dd-]hh:]mm:ss`, padded with spaces.
pub(crate) fn ps_etime(text: &str) -> Option<Duration> {
    let t = text.trim();
    let (days, rest) = match t.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, t),
    };
    let parts: Vec<u64> = rest.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (h, m, s) = match parts.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    if m >= 60 || s >= 60 {
        return None;
    }
    Some(Duration::from_secs(((days * 24 + h) * 60 + m) * 60 + s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 15.8 VM's own output, 10 Oct 2026, four minutes after boot (abridged).
    const LAUNCHCTL: &str = "system/dev.rexenv.rexenv.edge = {\n\tactive count = 1\n\tpath = /Library/LaunchDaemons/dev.rexenv.rexenv.edge.plist\n\ttype = LaunchDaemon\n\tstate = running\n\n\tprogram = /Library/Application Support/dev.rexenv.rexenv/edge-launch.sh\n\truns = 1\n\tpid = 302\n\tlast exit code = (never exited)\n}\n";

    #[test]
    fn launchd_reports_a_pid_only_while_the_job_has_a_process() {
        assert_eq!(launchctl_pid(LAUNCHCTL), Some(302));
        let stopped = LAUNCHCTL.replace("\tpid = 302\n", "").replace("running", "not running");
        assert_eq!(launchctl_pid(&stopped), None);
        assert_eq!(launchctl_pid("Could not find service \"x\" in domain for system"), None);
    }

    #[test]
    fn systemd_main_pid_zero_is_no_process() {
        assert_eq!(systemd_main_pid("4211\n"), Some(4211));
        assert_eq!(systemd_main_pid("0\n"), None);
        assert_eq!(systemd_main_pid(""), None);
    }

    #[test]
    fn every_ps_etime_shape_reads() {
        assert_eq!(ps_etime("   03:44\n"), Some(Duration::from_secs(224)));
        assert_eq!(ps_etime("1:02:03"), Some(Duration::from_secs(3723)));
        assert_eq!(ps_etime("2-01:00:05"), Some(Duration::from_secs(2 * 86400 + 3605)));
        assert_eq!(ps_etime(""), None);
        assert_eq!(ps_etime("12"), None);
        assert_eq!(ps_etime("01:75"), None);
    }
}
