//! The Linux DNS route (docs/PLAN-linux-port.md D-L2, ledger #717): a DUMMY LINK with
//! link-scoped routing domains, never a global resolved drop-in.
//!
//! **Why not the drop-in — measured on Ubuntu 22.04, 24 Sep 2026 (P1).** A global
//! `/etc/systemd/resolved.conf.d/*.conf` with `DNS=127.0.0.1:15353` + `Domains=~rex` sent
//! `example.com` to rexenv's resolver too: systemd-resolved uses the GLOBAL servers for every
//! name a link does not claim, and `~rex` on the global scope does not narrow that. Since the
//! resolver answers every name with loopback (ledger #44), the whole internet resolved to
//! `127.0.0.1` for that user until the file was removed. The plan's §6 named exactly this
//! hazard, and it was real.
//!
//! **What works, measured the same day (P1b):** a dummy interface `rexenv0` whose LINK carries
//! the server and the routing domains (`resolvectl dns/domain/default-route`). Link-scoped
//! routing domains are the split-DNS every VPN client uses: only names under `~rex` go to
//! that link's server; `default-route no` keeps everything else off it. `example.com` kept its
//! public answer, `curl` got a 200, NSS agreed, NetworkManager listed the link as
//! `unmanaged`, and the settings survived a resolved restart. Ubuntu Server (no NM) has the
//! same `ip` and `resolvectl`.
//!
//! **The shape.** One marker file per TLD under `/etc/rexenv/dns.d/<tld>` — its bytes are the
//! macOS resolver file's (`nameserver 127.0.0.1\nport <port>\n`), so ownership and the TLD scan
//! are `platform::resolver_files`, shared with macOS, unchanged. One root script
//! (`/usr/local/lib/rexenv/dns-route.sh`) creates the link and applies every marker's TLD; one
//! system unit (`rexenv-dns-route.service`, oneshot, `PartOf=systemd-resolved.service` so a
//! resolved restart re-applies it) runs the script at boot. Installing a TLD = write its
//! marker, (re)write script and unit, restart the unit. Removing the last TLD removes the
//! link, the unit and the script.
//!
//! Pure text; the privileged commands are absolute-pathed (`pkexec` strips `PATH`) and every
//! file content travels through `printf` with `%` doubled and newlines escaped, so nothing
//! here may contain a single quote. `mod.rs` hands these to `PrivilegeManager`.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::platform::resolver_files;
use crate::platform::traits::ResolverOwner;
use std::path::{Path, PathBuf};

/// The per-TLD markers the route script reads. Root-owned; only rexenv's privileged step writes it.
pub(crate) const MARKER_DIR: &str = "/etc/rexenv/dns.d";
/// The dummy interface that carries the route.
pub(crate) const LINK: &str = "rexenv0";
pub(crate) const SCRIPT_PATH: &str = "/usr/local/lib/rexenv/dns-route.sh";
pub(crate) const UNIT: &str = "rexenv-dns-route.service";
pub(crate) const UNIT_PATH: &str = "/etc/systemd/system/rexenv-dns-route.service";

/// `base/rel` as LINUX text. These modules write shell, unit files and `certutil` argv for a
/// Linux machine, and they are unit-tested on every host — `Path::join` on Windows puts a `\`
/// between the parts, which is how `verify.sh`'s first run on windows-latest saw
/// `/etc/rexenv/dns.d\rex` and `"/usr/local/lib/rexenv\edge-launch.sh"` in the generated
/// text (27 Sep 2026, seven tests red that could never fail on a Mac or a Linux box). The
/// slash is part of the OUTPUT, not of this host's filesystem, so it is written as a slash.
pub(crate) fn unix_join(base: &Path, rel: &str) -> PathBuf {
    let b = base.display().to_string();
    PathBuf::from(format!("{}/{}", b.trim_end_matches('/'), rel))
}

pub(crate) fn marker_path(tld: &str) -> PathBuf {
    unix_join(Path::new(MARKER_DIR), tld)
}

/// The marker's bytes — the ownership signature, identical for every TLD (the macOS shape).
pub(crate) fn signature(port: u16) -> String {
    format!("nameserver 127.0.0.1\nport {port}\n")
}

pub(crate) fn owner_of(tld: &str, port: u16) -> ResolverOwner {
    resolver_files::owner_of(&marker_path(tld), &signature(port))
}
pub(crate) fn our_tlds(port: u16) -> Vec<String> {
    resolver_files::tlds_matching_signature(Path::new(MARKER_DIR), &signature(port))
}
pub(crate) fn foreign_tlds(port: u16) -> Vec<String> {
    resolver_files::tlds_not_matching_signature(Path::new(MARKER_DIR), &signature(port))
}

/// The route script. Idempotent: the link is created only when absent, and every marker is
/// re-applied on each run. A marker whose name is not a plain lowercase label is skipped —
/// nothing else may reach `resolvectl domain`'s argv from a file name.
pub(crate) fn script_contents(port: u16) -> String {
    format!(
        "#!/bin/sh\n\
         # Managed by rexenv - routes the TLDs under {MARKER_DIR} to its local resolver through the\n\
         # dummy link {LINK} (link-scoped routing domains, no default route). Do not edit.\n\
         set -e\n\
         /sbin/ip link show {LINK} >/dev/null 2>&1 || /sbin/ip link add {LINK} type dummy\n\
         /sbin/ip link set {LINK} up\n\
         D=\"\"\n\
         for f in {MARKER_DIR}/*; do\n\
         \x20 [ -f \"$f\" ] || continue\n\
         \x20 n=$(/usr/bin/basename \"$f\")\n\
         \x20 case \"$n\" in *[!a-z]*) continue;; esac\n\
         \x20 D=\"$D ~$n\"\n\
         done\n\
         /usr/bin/resolvectl dns {LINK} 127.0.0.1:{port}\n\
         /usr/bin/resolvectl domain {LINK} $D\n\
         /usr/bin/resolvectl default-route {LINK} no\n\
         /usr/bin/resolvectl flush-caches\n"
    )
}

/// The unit: a oneshot that stays "active" so `PartOf` can restart it with resolved, and whose
/// stop removes the link (which is what removes the route).
pub(crate) fn unit_contents() -> String {
    format!(
        "# Managed by rexenv - the .rex DNS route. Do not edit.\n\
         [Unit]\n\
         Description=rexenv DNS route (local TLDs to its resolver via {LINK})\n\
         After=network.target systemd-resolved.service\n\
         Wants=systemd-resolved.service\n\
         PartOf=systemd-resolved.service\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         RemainAfterExit=yes\n\
         ExecStart=/bin/sh {SCRIPT_PATH}\n\
         ExecStop=/sbin/ip link del {LINK}\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    )
}

/// The guard every root command starts with: resolved must be the thing answering, or a link
/// route changes nothing and the user is told so instead of finding out from a browser.
const REQUIRE_RESOLVED: &str = "/usr/bin/systemctl -q is-active systemd-resolved || { \
     echo 'rexenv: systemd-resolved is not running on this machine, so a DNS route would change nothing. \
     rexenv supports Ubuntu with systemd-resolved (the default); see docs/INSTALL.md.' >&2; exit 1; }";

/// `text` as a single-quoted `printf` argument: `%` doubled, newlines as `\n`. Refuses a
/// single quote by construction — none of the texts here carries one, and the test holds it.
fn printf_arg(text: &str) -> String {
    assert!(!text.contains('\''), "a route text may not contain a single quote");
    text.replace('%', "%%").replace('\n', "\\n")
}

/// Single-quote a path for `/bin/sh` (app-data paths hold spaces; an embedded `'` becomes
/// `'\''` so nothing breaks out into the ROOT command — the macOS rule, B12).
pub(crate) fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

const APPLY: &str = "/usr/bin/systemctl daemon-reload && /usr/bin/systemctl enable rexenv-dns-route.service && /usr/bin/systemctl restart rexenv-dns-route.service";

pub(crate) fn install_command(tld: &str, port: u16) -> String {
    format!(
        "{REQUIRE_RESOLVED} && /bin/mkdir -p {MARKER_DIR} /usr/local/lib/rexenv && \
         /usr/bin/printf '{marker}' > {path} && /bin/chmod 644 {path} && \
         /usr/bin/printf '{script}' > {SCRIPT_PATH} && /bin/chmod 755 {SCRIPT_PATH} && \
         /usr/bin/printf '{unit}' > {UNIT_PATH} && /bin/chmod 644 {UNIT_PATH} && {APPLY}",
        marker = printf_arg(&signature(port)),
        path = marker_path(tld).display(),
        script = printf_arg(&script_contents(port)),
        unit = printf_arg(&unit_contents()),
    )
}

/// Remove the markers; with none left, take the whole route down (unit, script, link).
pub(crate) fn uninstall_command(tlds: &[String]) -> String {
    let files = tlds.iter().map(|t| marker_path(t).display().to_string()).collect::<Vec<_>>().join(" ");
    let rm = if files.is_empty() { String::new() } else { format!("/bin/rm -f {files} ; ") };
    format!(
        "{rm}if [ -z \"$(/bin/ls -A {MARKER_DIR} 2>/dev/null)\" ]; then \
         /usr/bin/systemctl disable --now {UNIT} 2>/dev/null ; /sbin/ip link del {LINK} 2>/dev/null ; \
         /bin/rm -f {UNIT_PATH} {SCRIPT_PATH} ; /usr/bin/systemctl daemon-reload ; \
         else /usr/bin/systemctl restart {UNIT} ; fi ; /usr/bin/resolvectl flush-caches"
    )
}

pub(crate) fn restore_command(restores: &[(String, PathBuf)]) -> String {
    let cmds = restores
        .iter()
        .map(|(tld, backup)| {
            let dest = marker_path(tld);
            format!("/bin/cp {} {} && /bin/chmod 644 {}", sh_quote(backup), dest.display(), dest.display())
        })
        .collect::<Vec<_>>()
        .join(" && ");
    if cmds.is_empty() {
        return "/usr/bin/resolvectl flush-caches".into();
    }
    format!("{cmds} && /usr/bin/systemctl restart {UNIT} && /usr/bin/resolvectl flush-caches")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the shell's `printf '<arg>'` writes — the inverse of `printf_arg`.
    fn printf_writes(arg: &str) -> String {
        arg.replace("\\n", "\n").replace("%%", "%")
    }

    #[test]
    fn the_route_is_link_scoped_with_no_default_route_and_reads_only_plain_labels() {
        let s = script_contents(15353);
        assert!(s.contains("/sbin/ip link add rexenv0 type dummy"));
        assert!(s.contains("/usr/bin/resolvectl dns rexenv0 127.0.0.1:15353\n"));
        assert!(s.contains("/usr/bin/resolvectl default-route rexenv0 no\n"), "a default route is P1's failure: {s}");
        assert!(s.contains("case \"$n\" in *[!a-z]*) continue;; esac"), "a marker name that is not a label never reaches argv");
        assert!(!s.contains("resolved.conf.d"), "the global drop-in is the mechanism P1 refuted");
        let u = unit_contents();
        assert!(u.contains("PartOf=systemd-resolved.service\n"), "a resolved restart must re-apply the link");
        assert!(u.contains("ExecStop=/sbin/ip link del rexenv0\n"));
        assert!(u.contains("RemainAfterExit=yes\n"));
    }

    #[test]
    fn the_marker_is_the_macos_signature_so_the_shared_classification_applies() {
        assert_eq!(signature(15353), "nameserver 127.0.0.1\nport 15353\n");
        assert_eq!(marker_path("test"), PathBuf::from("/etc/rexenv/dns.d/test"));
    }

    /// The whole install is one `pkexec` shell; every file inside it must come out of `printf`
    /// byte-identical to the text this module defines.
    #[test]
    fn the_install_command_writes_every_file_byte_for_byte_through_printf() {
        let cmd = install_command("rex", 15353);
        assert!(cmd.starts_with("/usr/bin/systemctl -q is-active systemd-resolved ||"), "{cmd}");
        assert!(!cmd.contains("$HOME"), "pkexec strips the environment");
        let args: Vec<&str> = cmd.split("/usr/bin/printf '").skip(1).map(|s| s.split("' > ").next().unwrap()).collect();
        assert_eq!(args.len(), 3, "marker, script, unit");
        assert_eq!(printf_writes(args[0]), signature(15353));
        assert_eq!(printf_writes(args[1]), script_contents(15353));
        assert_eq!(printf_writes(args[2]), unit_contents());
        assert!(cmd.contains("> /etc/rexenv/dns.d/rex && /bin/chmod 644 /etc/rexenv/dns.d/rex"));
        assert!(cmd.ends_with("/usr/bin/systemctl restart rexenv-dns-route.service"), "{cmd}");
    }

    #[test]
    fn removing_the_last_tld_takes_the_link_and_the_unit_down_and_a_partial_removal_restarts() {
        let u = uninstall_command(&["rex".into(), "test".into()]);
        assert!(u.starts_with("/bin/rm -f /etc/rexenv/dns.d/rex /etc/rexenv/dns.d/test ; if [ -z"), "{u}");
        assert!(u.contains("/sbin/ip link del rexenv0") && u.contains("/bin/rm -f /etc/systemd/system/rexenv-dns-route.service /usr/local/lib/rexenv/dns-route.sh"));
        assert!(u.contains("else /usr/bin/systemctl restart rexenv-dns-route.service ; fi"));
        assert!(uninstall_command(&[]).starts_with("if [ -z"), "nothing to remove still settles the unit");
        let r = restore_command(&[("test".into(), PathBuf::from("/home/u/.local/share/rexenv/backups/it's"))]);
        assert!(r.contains("/bin/cp '/home/u/.local/share/rexenv/backups/it'\\''s' /etc/rexenv/dns.d/test"), "{r}");
        assert!(r.ends_with("/usr/bin/systemctl restart rexenv-dns-route.service && /usr/bin/resolvectl flush-caches"));
    }
}
