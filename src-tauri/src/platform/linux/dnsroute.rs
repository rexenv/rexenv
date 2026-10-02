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
//! **…but "what works" was measured through a host that answered `.rex` itself.** On 28 Sep
//! 2026 the link turned out to carry NO DNS scope (`Current Scopes: none`): resolved ignores a
//! link whose only address is link-local, and every `.rex` answer had come from the VM's
//! upstream — the Mac, running rexenv. The link now carries `LINK_ADDR` (a TEST-NET `/32`). A
//! script or unit older than this build's is a NOTICE while the route is live (re-apply once, one
//! prompt — `classify`, #769), and reads as not installed only where liveness cannot be asked. Ask
//! resolved WHICH link answered (`resolvectl query -i rexenv0`), never just what the answer was.
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
/// The one address the link carries, so systemd-resolved gives it a DNS scope at all.
///
/// **Without it the route routed nothing — measured 28 Sep 2026 on the 22.04 VM (systemd 249).**
/// resolved counts a link as relevant for unicast DNS only when it holds an address that is not
/// link-local; a dummy link has only its automatic `fe80::`, so `resolvectl status rexenv0` read
/// `Current Scopes: none` with the server and `~rex` configured, and `resolvectl query -i rexenv0`
/// said "No appropriate name servers". Every `.rex` answer the port had recorded came from a HOST
/// running rexenv — the VM's upstream is the Mac's resolver (whose `/etc/resolver/rex` answers
/// loopback), and WSL's is Windows' (NRPT) — so a lone Ubuntu machine would never have resolved
/// `.rex` at all. A ULA `/128` gave the link its scope in the same measurement; an IPv4 `/32`
/// was chosen instead (owner, 28 Sep 2026) because a global IPv6 address on an IPv4-only host
/// flips `getaddrinfo`'s `AI_ADDRCONFIG` and starts handing out AAAA records. `192.0.2.0/24` is
/// TEST-NET-1 (RFC 5737): documentation only, never on a real network, so a `/32` of it on a
/// dummy link collides with nothing and routes nothing.
pub(crate) const LINK_ADDR: &str = "192.0.2.53/32";
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

/// Who owns `tld`'s marker — the shared macOS-shaped classification, nothing else.
pub(crate) fn marker_owner(tld: &str, port: u16) -> ResolverOwner {
    resolver_files::owner_of(&marker_path(tld), &signature(port))
}

/// The route script and unit as installed, `None` where a file is missing — the shape `classify`
/// compares against this build's.
pub(crate) fn installed_files() -> (Option<String>, Option<String>) {
    (std::fs::read_to_string(SCRIPT_PATH).ok(), std::fs::read_to_string(UNIT_PATH).ok())
}

/// Where liveness CANNOT be asked (no `resolvectl`), a marker of ours counts as INSTALLED only
/// while the route script beside it is the one this build writes: 0.8.8's gave the link no
/// address, so it routed nothing, and a missing one routes nothing either — `Absent` is what makes
/// the app offer its setup step, which rewrites the script. Where liveness CAN be asked,
/// `classify` lets the live answer decide and makes an older shape a notice instead (#769).
pub(crate) fn owner_given(marker: ResolverOwner, installed_script: Option<&str>, port: u16) -> ResolverOwner {
    match marker {
        ResolverOwner::Ours if installed_script != Some(script_contents(port).as_str()) => ResolverOwner::Absent,
        other => other,
    }
}
/// The argv that asks systemd-resolved what it holds for the link — read-only, no root.
pub(crate) const LINK_STATUS_ARGS: [&str; 2] = ["status", LINK];

/// A marker of ours counts as INSTALLED only while the link is LIVE too: `resolvectl status
/// rexenv0` names a DNS scope, our server and `~<tld>`. `status` is that command's stdout, or
/// `Err(())` when it failed — no such link, resolved not answering — and both mean `.tld` is not
/// routed here. The caller passes `None` only when it cannot ask at all (no `resolvectl`), which
/// keeps the marker's verdict: guessing "not live" would put a working machine through setup.
///
/// **Why (29 Sep 2026, the 22.04 VM, 0.8.10):** after Remove system changes and a re-setup, the
/// marker and the current script were both in place and the unit had logged "Bus client set DNS
/// server list to: 127.0.0.1:15353" — yet a minute later the link had no server, no domain,
/// `Current Scopes: none` (its `-DefaultRoute` kept), and `.rex` resolved only because the VM's
/// upstream (the Mac) answers it. Six attempts to reproduce it failed, the same sequence included,
/// with `busctl monitor` showing no other client. So the fix is not a guess at the cause: a route
/// that is not live reads as not installed, which is what sends the app to its setup step again
/// (one polkit, the unit re-applies) — the same door the stale-script rule above uses.
pub(crate) fn live_given(marker: ResolverOwner, status: Option<Result<&str, ()>>, tld: &str, port: u16) -> ResolverOwner {
    match (marker, status) {
        (ResolverOwner::Ours, Some(Ok(s))) if !link_routes(s, tld, port) => ResolverOwner::Absent,
        (ResolverOwner::Ours, Some(Err(()))) => ResolverOwner::Absent,
        (other, _) => other,
    }
}

/// Does `resolvectl status <link>`'s output route `tld` to our port: a DNS scope, the server,
/// `~tld`? Values may wrap onto indented lines with no `Key:` of their own, and a server carries
/// its own colon (`127.0.0.1:15353`), so a key is only ever `Name: ` with a space.
fn link_routes(status: &str, tld: &str, port: u16) -> bool {
    let mut fields: Vec<(String, Vec<String>)> = Vec::new();
    for line in status.lines() {
        let t = line.trim();
        match t.split_once(": ") {
            Some((k, v)) if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphabetic() || c == ' ') => {
                fields.push((k.to_string(), v.split_whitespace().map(str::to_string).collect()));
            }
            _ => {
                if let Some((_, vals)) = fields.last_mut() {
                    vals.extend(t.split_whitespace().map(str::to_string));
                }
            }
        }
    }
    let has = |key: &str, want: &str| fields.iter().any(|(k, v)| k == key && v.iter().any(|x| x == want));
    has("Current Scopes", "DNS") && has("DNS Servers", &format!("127.0.0.1:{port}")) && has("DNS Domain", &format!("~{tld}"))
}

/// What this build makes of a route: who owns the marker, whether resolved routes the TLD through
/// the link, and whether the script + unit on disk are THIS build's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouteVerdict {
    pub owner: ResolverOwner,
    /// Ours and live, but the shape on disk is not this build's — written by an older rexenv (or a
    /// file is gone while the link still routes). A notice and a one-prompt re-apply, never MISSING.
    pub older_shape: bool,
}

/// The three facts together (ledger #769). `status` is `resolvectl status rexenv0`'s stdout,
/// `Err(())` when the call failed, `None` when it cannot be asked at all.
///
/// **Why the shape is a notice and not `Absent` (30 Sep 2026, 22.04 VM, 0.8.10 → 0.8.11):** the
/// relaunched 0.8.11 found 0.8.10's unit on disk (`ExecStop=/sbin/ip link del rexenv0`, the shape
/// #762 rewrote) while resolved still routed `~rex` through `rexenv0` and a fresh name resolved —
/// and read it as not installed: `rex status` said MISSING and the app opened onboarding's Welcome
/// over an install with three sites. A route that routes is installed; an older shape is advice.
pub(crate) fn classify(
    marker: ResolverOwner,
    status: Option<Result<&str, ()>>,
    installed_script: Option<&str>,
    installed_unit: Option<&str>,
    tld: &str,
    port: u16,
) -> RouteVerdict {
    if marker != ResolverOwner::Ours {
        return RouteVerdict { owner: marker, older_shape: false };
    }
    let current = installed_script == Some(script_contents(port).as_str()) && installed_unit == Some(unit_contents().as_str());
    match status {
        None => RouteVerdict { owner: owner_given(marker, installed_script, port), older_shape: false },
        Some(s) => match live_given(marker, Some(s), tld, port) {
            ResolverOwner::Ours => RouteVerdict { owner: ResolverOwner::Ours, older_shape: !current },
            other => RouteVerdict { owner: other, older_shape: false },
        },
    }
}

/// The sentence the status surfaces show for an older shape — the rule's words beside the rule.
pub(crate) fn older_shape_notice(tld: &str) -> String {
    format!(
        "Your .{tld} route was set up by an older rexenv. It works — re-apply it once (one administrator \
         prompt) so this version's route is in place."
    )
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
         /sbin/ip addr replace {LINK_ADDR} dev {LINK}\n\
         /bin/udevadm settle --timeout=5 2>/dev/null || true\n\
         i=0; until /usr/bin/resolvectl status {LINK} >/dev/null 2>&1 || [ $i -ge 50 ]; do i=$((i+1)); sleep 0.1; done\n\
         D=\"\"\n\
         for f in {MARKER_DIR}/*; do\n\
         \x20 [ -f \"$f\" ] || continue\n\
         \x20 n=$(/usr/bin/basename \"$f\")\n\
         \x20 case \"$n\" in *[!a-z]*) continue;; esac\n\
         \x20 D=\"$D ~$n\"\n\
         done\n\
         t=0\n\
         while [ $t -lt 3 ]; do\n\
         \x20 t=$((t+1))\n\
         \x20 /usr/bin/resolvectl dns {LINK} 127.0.0.1:{port}\n\
         \x20 /usr/bin/resolvectl domain {LINK} $D\n\
         \x20 /usr/bin/resolvectl default-route {LINK} no\n\
         \x20 sleep 0.3\n\
         \x20 if /usr/bin/resolvectl dns {LINK} 2>/dev/null | /bin/grep -q 127.0.0.1:{port}; then break; fi\n\
         \x20 echo \"rexenv: {LINK} lost its DNS server right after it was set (try $t) - setting it again\" >&2\n\
         done\n\
         /usr/bin/resolvectl flush-caches\n"
    )
}

/// The unit: a oneshot that stays "active" so `PartOf` can restart it with resolved, and whose
/// stop removes the link (which is what removes the route).
/// The unit's stop REVERTS the link's DNS and leaves the dummy link in place; only the
/// uninstall command deletes it. Until 30 Sep 2026 `ExecStop` was `ip link del`, so every
/// re-apply (`systemctl restart`) deleted and re-created `rexenv0` — a new ifindex each time,
/// udev's remove of the old one landing after the add of the new — and resolved's late
/// teardown of the old link took the new link's just-set config with it: measured on the
/// 22.04 VM, 3 of 50 re-applies came back `Current Scopes: none` within four seconds of
/// "Bus client set DNS server list", NetworkManager `unmanaged` and only rexenv's own calls
/// on the bus; with the link kept across restarts, 0 of 20 (ifindex constant). That is the
/// 28–29 Sep "`rexenv0` lost its DNS server and domain once" (#741) — the re-setup that day
/// re-created the link the same way. The script's settle + verify-retry is belt and braces
/// for the first apply, which still creates the link.
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
         ExecStop=/usr/bin/resolvectl revert {LINK}\n\
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

    /// Ledger #741 — a marker of ours counts as installed only while resolved actually routes the
    /// TLD through the link. `live` and `lost` are the 22.04 VM's own `resolvectl status rexenv0`,
    /// verbatim (29 Sep 2026): `lost` is both the failure and what `resolvectl revert rexenv0`
    /// produces — the server and domain gone, `.rex` then answered on `enp0s1` by the upstream.
    #[test]
    fn a_marker_counts_only_while_the_link_routes_the_tld() {
        let live = "Link 9 (rexenv0)\n    Current Scopes: DNS\n         Protocols: -DefaultRoute +LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\nCurrent DNS Server: 127.0.0.1:15353\n       DNS Servers: 127.0.0.1:15353\n        DNS Domain: ~rex\n";
        let lost = "Link 9 (rexenv0)\nCurrent Scopes: none\n     Protocols: -DefaultRoute +LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\n";
        let ours = || ResolverOwner::Ours;
        assert_eq!(live_given(ours(), Some(Ok(live)), "rex", 15353), ResolverOwner::Ours);
        assert_eq!(live_given(ours(), Some(Ok(lost)), "rex", 15353), ResolverOwner::Absent, "the VM's failure reads as not installed");
        assert_eq!(live_given(ours(), Some(Ok(live)), "test", 15353), ResolverOwner::Absent, "another TLD is not routed by ~rex");
        assert_eq!(live_given(ours(), Some(Ok(live)), "rex", 5353), ResolverOwner::Absent, "another port is not ours");
        let no_scope = live.replace("Current Scopes: DNS", "Current Scopes: none");
        assert_eq!(live_given(ours(), Some(Ok(&no_scope)), "rex", 15353), ResolverOwner::Absent, "0.8.8's address-less link");
        let wrapped = "Link 4 (rexenv0)\nCurrent Scopes: DNS\n       DNS Servers: 10.0.0.1\n                    127.0.0.1:15353\n        DNS Domain: ~test\n                    ~rex\n";
        assert_eq!(live_given(ours(), Some(Ok(wrapped)), "rex", 15353), ResolverOwner::Ours, "values wrapped onto their own lines");
        assert_eq!(live_given(ours(), Some(Err(())), "rex", 15353), ResolverOwner::Absent, "no link, or resolved not answering");
        assert_eq!(live_given(ours(), Some(Ok("")), "rex", 15353), ResolverOwner::Absent, "no link: resolvectl says so on stderr and exits 0");
        assert_eq!(live_given(ours(), None, "rex", 15353), ResolverOwner::Ours, "cannot ask: the marker's verdict stands");
        assert_eq!(live_given(ResolverOwner::Absent, Some(Ok(live)), "rex", 15353), ResolverOwner::Absent);
        let foreign = ResolverOwner::Foreign { content: None };
        assert_eq!(live_given(foreign.clone(), Some(Err(())), "rex", 15353), foreign, "a foreign marker stays foreign");
    }

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
        assert!(u.contains("ExecStop=/usr/bin/resolvectl revert rexenv0\n"), "a re-apply never deletes the link (#762)");
        assert!(!u.contains("ip link del"), "{u}");
        assert!(u.contains("RemainAfterExit=yes\n"));
    }

    /// resolved gives a link no DNS scope without a non-link-local address (28 Sep 2026, the VM):
    /// the address must be set, before the server, and must never be link-local.
    #[test]
    fn the_link_carries_a_non_link_local_address_so_resolved_gives_it_a_dns_scope() {
        let s = script_contents(15353);
        let addr = s.find("/sbin/ip addr replace 192.0.2.53/32 dev rexenv0\n").expect(&s);
        assert!(addr < s.find("/usr/bin/resolvectl dns").unwrap(), "the address comes before the server: {s}");
        assert!(!LINK_ADDR.starts_with("169.254.") && !LINK_ADDR.starts_with("fe80"), "link-local gets no DNS scope");
        assert!(LINK_ADDR.starts_with("192.0.2.") && LINK_ADDR.ends_with("/32"), "TEST-NET-1, one host: {LINK_ADDR}");
    }

    /// A marker of ours with a stale or missing script is NOT installed — the update's repair path.
    /// **A re-apply keeps the link and verifies what it set.** The script waits for udev
    /// and for resolved to know the link, sets the DNS, and re-reads it — up to three tries —
    /// before it trusts the apply; the unit's stop reverts the DNS rather than deleting the
    /// link (the churn that lost 3 of 50 re-applies on the 22.04 VM, 30 Sep 2026). The
    /// uninstall still deletes the link — that is the one place it should go.
    #[test]
    fn a_re_apply_keeps_the_link_and_verifies_the_dns_it_set() {
        let script = script_contents(15353);
        assert!(script.contains("/bin/udevadm settle --timeout=5 2>/dev/null || true\n"), "{script}");
        assert!(script.contains("until /usr/bin/resolvectl status rexenv0 >/dev/null 2>&1 || [ $i -ge 50 ]"), "waits for resolved to know the link");
        assert!(script.contains("while [ $t -lt 3 ]; do") && script.contains("| /bin/grep -q 127.0.0.1:15353; then break; fi"), "verifies and retries: {script}");
        assert!(!script.contains("ip link del"), "the script never deletes the link");
        assert!(unit_contents().contains("ExecStop=/usr/bin/resolvectl revert rexenv0\n"));
        assert!(uninstall_command(&["rex".into()]).contains("/sbin/ip link del rexenv0"), "the uninstall is where the link goes");
        // The verify loop must not trip `set -e`: a failed grep sits in an `if`, never bare.
        assert!(!script.contains("\n /bin/grep") && !script.contains("\n  /bin/grep"), "{script}");
    }

    #[test]
    fn an_old_or_missing_route_script_reads_as_absent_so_setup_is_offered_again() {
        let current = script_contents(15353);
        let old = current.replace("/sbin/ip addr replace 192.0.2.53/32 dev rexenv0\n", "");
        assert_ne!(old, current, "the 0.8.8 script is the current one minus the address line");
        assert_eq!(owner_given(ResolverOwner::Ours, Some(&current), 15353), ResolverOwner::Ours);
        assert_eq!(owner_given(ResolverOwner::Ours, Some(&old), 15353), ResolverOwner::Absent);
        assert_eq!(owner_given(ResolverOwner::Ours, None, 15353), ResolverOwner::Absent);
        assert_eq!(owner_given(ResolverOwner::Absent, Some(&current), 15353), ResolverOwner::Absent);
        let foreign = ResolverOwner::Foreign { content: None };
        assert_eq!(owner_given(foreign.clone(), None, 15353), foreign, "a foreign route is never reclassified");
    }

    /// Ledger #769 — **a route an older rexenv wrote that still routes is OURS, with a notice — never
    /// MISSING.** `unit_0810` is the shape 0.8.10 installed (`ExecStop=/sbin/ip link del rexenv0`, which
    /// #762 rewrote); on 30 Sep 2026 the relaunched 0.8.11 read it as not installed while resolved still
    /// routed `~rex` through `rexenv0`, and opened onboarding's Welcome over an install with three sites.
    #[test]
    fn an_older_route_shape_that_still_routes_is_ours_with_a_notice_never_missing() {
        let live = "Link 9 (rexenv0)\n    Current Scopes: DNS\n         Protocols: -DefaultRoute +LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\nCurrent DNS Server: 127.0.0.1:15353\n       DNS Servers: 127.0.0.1:15353\n        DNS Domain: ~rex\n";
        let lost = "Link 9 (rexenv0)\nCurrent Scopes: none\n     Protocols: -DefaultRoute +LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\n";
        let current_script = script_contents(15353);
        let current_unit = unit_contents();
        let unit_0810 = current_unit.replace("ExecStop=/usr/bin/resolvectl revert rexenv0\n", "ExecStop=/sbin/ip link del rexenv0\n");
        assert_ne!(unit_0810, current_unit, "0.8.10's unit is the current one with the link-deleting stop");
        let old_script = current_script.replace("/sbin/ip addr replace 192.0.2.53/32 dev rexenv0\n", "");
        let ours = ResolverOwner::Ours;
        let v = |status, script: &str, unit: &str| classify(ours.clone(), status, Some(script), Some(unit), "rex", 15353);
        assert_eq!(v(Some(Ok(live)), &current_script, &current_unit), RouteVerdict { owner: ours.clone(), older_shape: false });
        assert_eq!(
            v(Some(Ok(live)), &current_script, &unit_0810),
            RouteVerdict { owner: ours.clone(), older_shape: true },
            "0.8.10's unit, still routing: ours, with the notice — the 30 Sep failure read Absent here"
        );
        assert!(v(Some(Ok(live)), &old_script, &current_unit).older_shape, "an older script that still routes: the same notice");
        assert_eq!(
            v(Some(Ok(lost)), &current_script, &unit_0810),
            RouteVerdict { owner: ResolverOwner::Absent, older_shape: false },
            "not routing: MISSING whatever the shape — setup re-applies (#741)"
        );
        assert_eq!(v(Some(Err(())), &current_script, &current_unit).owner, ResolverOwner::Absent, "resolvectl failing: not routed here");
        // No resolvectl at all: liveness cannot be asked, so the script's shape is the only evidence (the 28 Sep rule).
        assert_eq!(v(None, &old_script, &current_unit), RouteVerdict { owner: ResolverOwner::Absent, older_shape: false });
        assert_eq!(v(None, &current_script, &unit_0810), RouteVerdict { owner: ours.clone(), older_shape: false });
        assert_eq!(
            classify(ours.clone(), Some(Ok(live)), None, None, "rex", 15353),
            RouteVerdict { owner: ours.clone(), older_shape: true },
            "script or unit gone while the link still routes: re-apply, not MISSING"
        );
        let foreign = ResolverOwner::Foreign { content: None };
        assert_eq!(classify(foreign.clone(), Some(Ok(live)), None, None, "rex", 15353), RouteVerdict { owner: foreign, older_shape: false });
        assert_eq!(classify(ResolverOwner::Absent, Some(Ok(live)), Some(&current_script), Some(&current_unit), "rex", 15353).owner, ResolverOwner::Absent);
        let n = older_shape_notice("rex");
        assert!(n.contains(".rex") && n.contains("older rexenv") && n.contains("re-apply") && n.contains("It works"), "{n}");
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
