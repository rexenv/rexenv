//! The two systemd units rexenv installs on Linux, as pure text (docs/PLAN-linux-port.md §2):
//!
//! - the DNS agent, a USER unit (`~/.config/systemd/user/rexenv-dns.service`, no privilege) —
//!   the macOS LaunchAgent's `KeepAlive` is `Restart=always`;
//! - the root edge, a SYSTEM unit (`/etc/systemd/system/rexenv-edge.service`) installed by ONE
//!   privileged script — the macOS LaunchDaemon, with `launchctl` spelled `systemctl`.
//!
//! Every root command uses absolute paths: `pkexec` hands the script a minimal environment.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use super::resolved::sh_quote;
use std::path::{Path, PathBuf};

pub(crate) const DNS_UNIT: &str = "rexenv-dns.service";
pub(crate) const EDGE_UNIT: &str = "rexenv-edge.service";
/// Root-owned support tree for the edge: the binary the daemon executes must never be the
/// user-writable download cache (a root exec of a user-writable file is a standing LPE — the
/// macOS rule, kept). `/usr/local/lib` is the FHS home for locally installed program files.
pub(crate) const EDGE_ROOT_DIR: &str = "/usr/local/lib/rexenv";

/// systemd quoting for `ExecStart=`: double quotes, with `\`, `"`, `$` and `` ` `` escaped, so
/// an install path with spaces (an AppImage in `~/Apps/rexenv 0.9.0.AppImage`) is one argument.
pub(crate) fn unit_quote(path: &Path) -> String {
    let s = path.display().to_string();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The DNS agent's user unit: `exe --dns-agent`, restarted on any exit, output appended to
/// `log` (`append:` needs systemd ≥ 240; Ubuntu 22.04 ships 249), started with the user's
/// session (`default.target`) so it is up from login and outlives the app.
pub(crate) fn dns_unit_contents(exe: &Path, log: &Path) -> String {
    format!(
        "# Managed by rexenv — the loopback DNS resolver for *.rex. Do not edit.\n\
         [Unit]\n\
         Description=rexenv local DNS resolver\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe} --dns-agent\n\
         Restart=always\n\
         RestartSec=2\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe = unit_quote(exe),
        log = log.display(),
    )
}

/// The program a unit runs, read back out of its `ExecStart=` — the first quoted argument.
pub(crate) fn unit_program(unit: &str) -> Option<PathBuf> {
    let line = unit.lines().find_map(|l| l.trim().strip_prefix("ExecStart="))?;
    let rest = line.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return Some(PathBuf::from(out)),
            c => out.push(c),
        }
    }
    None
}

pub(crate) fn edge_unit_path() -> PathBuf {
    PathBuf::from("/etc/systemd/system").join(EDGE_UNIT)
}
pub(crate) fn edge_binary_path() -> PathBuf {
    PathBuf::from(EDGE_ROOT_DIR).join("bin/caddy")
}
pub(crate) fn edge_wrapper_path() -> PathBuf {
    PathBuf::from(EDGE_ROOT_DIR).join("edge-launch.sh")
}

/// The edge's system unit: `Restart=always` is the `KeepAlive`; `KillMode=mixed` sends the
/// stop signal to the wrapper's `exec`ed caddy (the main process) and SIGKILL to the rest of
/// the cgroup (the chown loop) so nothing survives a stop.
pub(crate) fn edge_unit_contents(wrapper: &Path, start_log: &Path) -> String {
    format!(
        "# Managed by rexenv — the root Caddy edge, kept alive by systemd. Do not edit.\n\
         [Unit]\n\
         Description=rexenv edge (Caddy on :80/:443)\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart=/bin/sh {wrapper}\n\
         Restart=always\n\
         RestartSec=1\n\
         KillMode=mixed\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        wrapper = unit_quote(wrapper),
        log = start_log.display(),
    )
}

/// The launcher — the macOS wrapper with GNU `stat` (`-c %u` owner, `-c %h` link count): keep
/// the 0600 admin socket owned by the invoking user across every config reload (caddy recreates
/// it as root each time), refuse to chown a hardlinked inode, then `exec` caddy so systemd
/// tracks the real edge pid.
pub(crate) fn edge_wrapper_contents(caddy_bin: &Path, caddyfile: &Path, admin_sock: &Path, appdata: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         # Managed by rexenv — root Caddy edge under systemd Restart=always. Do not edit.\n\
         SOCK={sock}\n\
         OWNER=$(/usr/bin/stat -c %u {appdata})\n\
         ( while :; do \
         [ -S \"$SOCK\" ] && [ \"$(/usr/bin/stat -c %u \"$SOCK\" 2>/dev/null)\" != \"$OWNER\" ] \
         && [ \"$(/usr/bin/stat -c %h \"$SOCK\" 2>/dev/null)\" = 1 ] \
         && /bin/chown -h \"$OWNER\" \"$SOCK\" 2>/dev/null; sleep 1; done ) &\n\
         exec {caddy} run --config {cfg} --adapter caddyfile\n",
        sock = sh_quote(admin_sock),
        appdata = sh_quote(appdata),
        caddy = sh_quote(caddy_bin),
        cfg = sh_quote(caddyfile),
    )
}

/// ONE privileged shell: copy caddy into the root tree, drop in the staged wrapper and unit,
/// reload systemd, enable (a Stop-all disables), (re)start.
pub(crate) fn edge_install_command(src_caddy: &Path, staged_wrapper: &Path, staged_unit: &Path) -> String {
    format!(
        "/bin/mkdir -p {bindir} && \
         /bin/cp {src} {bin} && /bin/chown root:root {bin} && /bin/chmod 755 {bin} && \
         /bin/cp {sw} {wrapper} && /bin/chown root:root {wrapper} && /bin/chmod 755 {wrapper} && \
         /bin/cp {su} {unit} && /bin/chown root:root {unit} && /bin/chmod 644 {unit} && \
         /usr/bin/systemctl daemon-reload && /usr/bin/systemctl enable {name} && /usr/bin/systemctl restart {name}",
        bindir = sh_quote(&PathBuf::from(EDGE_ROOT_DIR).join("bin")),
        src = sh_quote(src_caddy),
        bin = sh_quote(&edge_binary_path()),
        sw = sh_quote(staged_wrapper),
        wrapper = sh_quote(&edge_wrapper_path()),
        su = sh_quote(staged_unit),
        unit = sh_quote(&edge_unit_path()),
        name = EDGE_UNIT,
    )
}

/// An explicit stop must also DISABLE, or `Restart=always` plus the next boot bring it back.
pub(crate) fn edge_stop_command() -> String {
    format!("/usr/bin/systemctl disable --now {EDGE_UNIT} 2>/dev/null ; :")
}

pub(crate) fn edge_start_command() -> String {
    format!("/usr/bin/systemctl enable {EDGE_UNIT} 2>/dev/null ; /usr/bin/systemctl restart {EDGE_UNIT}")
}

pub(crate) fn edge_uninstall_command() -> String {
    format!(
        "/usr/bin/systemctl disable --now {EDGE_UNIT} 2>/dev/null ; /bin/rm -f {unit} {wrapper} {bin} ; /usr/bin/systemctl daemon-reload",
        unit = sh_quote(&edge_unit_path()),
        wrapper = sh_quote(&edge_wrapper_path()),
        bin = sh_quote(&edge_binary_path()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_start_quotes_the_program_and_reads_it_back() {
        let exe = Path::new("/home/u/Apps/rexenv 0.9.0.AppImage");
        let unit = dns_unit_contents(exe, Path::new("/home/u/.local/share/rexenv/logs/dns-agent.log"));
        assert!(unit.contains("ExecStart=\"/home/u/Apps/rexenv 0.9.0.AppImage\" --dns-agent\n"), "{unit}");
        assert!(unit.contains("Restart=always\n") && unit.contains("WantedBy=default.target\n"));
        assert!(unit.contains("StandardOutput=append:/home/u/.local/share/rexenv/logs/dns-agent.log\n"));
        assert_eq!(unit_program(&unit), Some(exe.to_path_buf()));
        assert_eq!(unit_quote(Path::new("/a/$b\"c")), "\"/a/\\$b\\\"c\"");
        assert_eq!(unit_program("[Service]\nExecStart=\"/a/\\$b\\\"c\" --x\n"), Some(PathBuf::from("/a/$b\"c")));
        assert_eq!(unit_program("[Service]\n"), None);
    }

    #[test]
    fn the_edge_unit_kills_the_whole_cgroup_and_the_wrapper_uses_gnu_stat() {
        let unit = edge_unit_contents(&edge_wrapper_path(), Path::new("/var/log/rexenv-edge.log"));
        assert!(unit.contains("ExecStart=/bin/sh \"/usr/local/lib/rexenv/edge-launch.sh\"\n"), "{unit}");
        assert!(unit.contains("KillMode=mixed\n") && unit.contains("Restart=always\n"));
        let w = edge_wrapper_contents(
            &edge_binary_path(),
            Path::new("/home/u/.local/share/rexenv/config/Caddyfile"),
            Path::new("/home/u/.local/share/rexenv/run/caddy-admin.sock"),
            Path::new("/home/u/.local/share/rexenv"),
        );
        assert!(w.contains("/usr/bin/stat -c %u '/home/u/.local/share/rexenv'"), "{w}");
        assert!(w.contains("/usr/bin/stat -c %h"), "the hardlink guard (B16) must survive the port");
        assert!(!w.contains("stat -f"), "BSD stat flags do not exist on Linux");
        assert!(w.ends_with("exec '/usr/local/lib/rexenv/bin/caddy' run --config '/home/u/.local/share/rexenv/config/Caddyfile' --adapter caddyfile\n"));
    }

    #[test]
    fn the_root_commands_are_absolute_and_a_stop_disables() {
        let i = edge_install_command(Path::new("/home/u/.local/share/rexenv/bin/caddy"), Path::new("/tmp/w"), Path::new("/tmp/u"));
        assert!(i.starts_with("/bin/mkdir -p '/usr/local/lib/rexenv/bin' && /bin/cp '/home/u/.local/share/rexenv/bin/caddy' '/usr/local/lib/rexenv/bin/caddy' && /bin/chown root:root"));
        assert!(i.ends_with("/usr/bin/systemctl daemon-reload && /usr/bin/systemctl enable rexenv-edge.service && /usr/bin/systemctl restart rexenv-edge.service"), "{i}");
        assert!(i.contains("/etc/systemd/system/rexenv-edge.service"));
        assert_eq!(edge_stop_command(), "/usr/bin/systemctl disable --now rexenv-edge.service 2>/dev/null ; :");
        assert!(edge_start_command().ends_with("/usr/bin/systemctl restart rexenv-edge.service"));
        let u = edge_uninstall_command();
        assert!(u.contains("/bin/rm -f '/etc/systemd/system/rexenv-edge.service' '/usr/local/lib/rexenv/edge-launch.sh' '/usr/local/lib/rexenv/bin/caddy'"), "{u}");
        for cmd in [&i, &u] {
            assert!(!cmd.contains(" systemctl ") || cmd.contains("/usr/bin/systemctl"), "{cmd}");
        }
    }
}
