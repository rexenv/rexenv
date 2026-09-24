//! The Linux DNS route: one systemd-resolved drop-in per TLD (docs/PLAN-linux-port.md D-L2).
//!
//! `/etc/systemd/resolved.conf.d/rexenv-<tld>.conf` says `DNS=127.0.0.1:<port>` and
//! `Domains=~<tld>`. The `~` makes it a ROUTING domain: only names under `<tld>` go to that
//! server, everything else keeps the link's own servers — the property the macOS resolver file
//! and the Windows NRPT rule give by construction, and the one the plan's P1 probe must
//! measure on a real Ubuntu before a user meets it, because rexenv's resolver answers EVERY
//! name with loopback (ledger #44). Ownership is the file's exact content, the macOS shape.
//!
//! Pure: paths, contents, classification over a directory of fixture files, and the root
//! shell commands. `mod.rs` hands these to `PrivilegeManager`. Tested on the macOS host.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::platform::traits::ResolverOwner;
use std::path::{Path, PathBuf};

/// Where resolved reads drop-ins. `resolved.conf.d` under `/etc/systemd`, the documented place
/// for local overrides (never `/usr/lib/systemd`, which a package upgrade replaces).
pub(crate) const DROPIN_DIR: &str = "/etc/systemd/resolved.conf.d";

/// The file for one TLD.
pub(crate) fn dropin_path(tld: &str) -> PathBuf {
    Path::new(DROPIN_DIR).join(format!("rexenv-{tld}.conf"))
}

/// The TLD a drop-in file NAME routes, if the name is one of ours — `rexenv-<label>.conf` with a
/// valid label. Anything else is not ours by construction, and can never reach the root `rm`
/// in `uninstall_command` (the macOS B10 rule, restated for a suffix).
pub(crate) fn tld_of_name(name: &str) -> Option<&str> {
    let label = name.strip_prefix("rexenv-")?.strip_suffix(".conf")?;
    crate::core::tld::is_valid_label(label).then_some(label)
}

/// The drop-in's exact bytes — the ownership signature for `tld`.
pub(crate) fn contents(tld: &str, port: u16) -> String {
    format!(
        "# Managed by rexenv — routes *.{tld} to its local resolver. Do not edit.\n\
         [Resolve]\n\
         DNS=127.0.0.1:{port}\n\
         Domains=~{tld}\n"
    )
}

/// Who owns `tld`'s route under `dir`: our file with our bytes is ours; our file with other
/// bytes, or ANY other drop-in that names `~<tld>` as a routing domain, is foreign; nothing
/// is absent. A file that exists but cannot be read is foreign, never absent.
pub(crate) fn owner_of(dir: &Path, tld: &str, port: u16) -> ResolverOwner {
    let ours = dir.join(format!("rexenv-{tld}.conf"));
    match std::fs::read_to_string(&ours) {
        Ok(c) if c == contents(tld, port) => return ResolverOwner::Ours,
        Ok(c) => return ResolverOwner::Foreign { content: Some(c) },
        Err(_) if ours.exists() => return ResolverOwner::Foreign { content: None },
        Err(_) => {}
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path == ours || !path.is_file() {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(c) if routing_domains(&c).iter().any(|d| d == tld) => {
                return ResolverOwner::Foreign { content: Some(c) }
            }
            _ => {}
        }
    }
    ResolverOwner::Absent
}

/// Every TLD under `dir` whose drop-in is ours (name AND bytes), sorted.
pub(crate) fn our_tlds(dir: &Path, port: u16) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let tld = tld_of_name(&name)?.to_string();
            (std::fs::read_to_string(e.path()).ok()? == contents(&tld, port)).then_some(tld)
        })
        .collect();
    out.sort();
    out
}

/// Every single-label routing domain some OTHER file under `dir` claims, plus ours with the
/// wrong bytes — the TLDs another tool answers on this machine. Sorted, deduplicated.
pub(crate) fn foreign_tlds(dir: &Path, port: u16) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if !e.path().is_file() {
            continue;
        }
        let Ok(name) = e.file_name().into_string() else { continue };
        let Ok(c) = std::fs::read_to_string(e.path()) else { continue };
        if let Some(tld) = tld_of_name(&name) {
            if c == contents(tld, port) {
                continue;
            }
        }
        out.extend(routing_domains(&c));
    }
    out.sort();
    out.dedup();
    out
}

/// The `~label` entries of every `Domains=` line that are a single valid TLD label.
fn routing_domains(conf: &str) -> Vec<String> {
    conf.lines()
        .filter_map(|l| l.trim().strip_prefix("Domains="))
        .flat_map(|v| v.split_whitespace())
        .filter_map(|d| d.strip_prefix('~'))
        .filter(|d| crate::core::tld::is_valid_label(d))
        .map(str::to_string)
        .collect()
}

/// The guard every root command starts with: resolved must be the thing answering, or the
/// drop-in changes nothing and the user is told so instead of finding out from a browser.
/// Absolute paths throughout — `pkexec` hands the script a minimal `PATH`.
const REQUIRE_RESOLVED: &str = "/usr/bin/systemctl -q is-active systemd-resolved || { \
     echo 'rexenv: systemd-resolved is not running on this machine, so a resolver drop-in would change nothing. \
     rexenv supports Ubuntu with systemd-resolved (the default); see docs/INSTALL.md.' >&2; exit 1; }";

/// Restart resolved so the drop-in is read (systemd 249 on Ubuntu 22.04 has no config reload),
/// then drop its caches.
const APPLY: &str = "/usr/bin/systemctl restart systemd-resolved && /usr/bin/resolvectl flush-caches";

pub(crate) fn install_command(tld: &str, port: u16) -> String {
    let printf_arg = contents(tld, port).replace('\n', "\\n");
    format!(
        "{REQUIRE_RESOLVED} && /bin/mkdir -p {DROPIN_DIR} && /usr/bin/printf '{printf_arg}' > {} && {APPLY}",
        dropin_path(tld).display()
    )
}

pub(crate) fn uninstall_command(tlds: &[String]) -> String {
    let files = tlds.iter().map(|t| dropin_path(t).display().to_string()).collect::<Vec<_>>().join(" ");
    if files.is_empty() {
        return APPLY.to_string();
    }
    format!("/bin/rm -f {files} && {APPLY}")
}

pub(crate) fn restore_command(restores: &[(String, PathBuf)]) -> String {
    let cmds = restores
        .iter()
        .map(|(tld, backup)| {
            let dest = dropin_path(tld);
            format!("/bin/cp {} {} && /bin/chmod 644 {}", sh_quote(backup), dest.display(), dest.display())
        })
        .collect::<Vec<_>>()
        .join(" && ");
    if cmds.is_empty() {
        return APPLY.to_string();
    }
    format!("{cmds} && {APPLY}")
}

/// Single-quote a path for `/bin/sh` — app-data paths may hold spaces; an embedded `'` becomes
/// `'\''` so nothing breaks out of the quotes into the ROOT command (the macOS rule, B12).
pub(crate) fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexenv-resolved-{}-{}", std::process::id(), rand_tag()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn rand_tag() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn the_dropin_is_a_routing_domain_on_our_loopback_port() {
        let c = contents("rex", 15353);
        assert!(c.contains("DNS=127.0.0.1:15353\n"));
        assert!(c.contains("Domains=~rex\n"), "a plain `rex` would be a SEARCH domain, not a route: {c}");
        assert!(c.starts_with("# Managed by rexenv"));
        assert_eq!(dropin_path("test"), PathBuf::from("/etc/systemd/resolved.conf.d/rexenv-test.conf"));
    }

    #[test]
    fn only_our_file_names_are_ours() {
        assert_eq!(tld_of_name("rexenv-rex.conf"), Some("rex"));
        assert_eq!(tld_of_name("rexenv-Rex.conf"), None, "labels are lowercase");
        assert_eq!(tld_of_name("rexenv-a;b.conf"), None, "a shell metachar name never reaches rm");
        assert_eq!(tld_of_name("valet.conf"), None);
        assert_eq!(tld_of_name("rexenv-rex"), None);
    }

    #[test]
    fn ownership_is_exact_bytes_and_foreign_files_claim_their_domains() {
        let dir = fixture();
        assert_eq!(owner_of(&dir, "rex", 15353), ResolverOwner::Absent);
        std::fs::write(dir.join("rexenv-rex.conf"), contents("rex", 15353)).unwrap();
        assert_eq!(owner_of(&dir, "rex", 15353), ResolverOwner::Ours);
        // Same file, other port: foreign, and its bytes are what a takeover would put back.
        std::fs::write(dir.join("rexenv-dev.conf"), contents("dev", 5333)).unwrap();
        assert_eq!(owner_of(&dir, "dev", 15353), ResolverOwner::Foreign { content: Some(contents("dev", 5333)) });
        // Another tool's drop-in routing `test`: foreign for `test`, invisible for `rex`.
        std::fs::write(dir.join("valet.conf"), "[Resolve]\nDNS=127.0.0.1\nDomains=~test ~site\n").unwrap();
        assert!(matches!(owner_of(&dir, "test", 15353), ResolverOwner::Foreign { .. }));
        assert_eq!(our_tlds(&dir, 15353), vec!["rex".to_string()]);
        assert_eq!(foreign_tlds(&dir, 15353), vec!["dev".to_string(), "site".to_string(), "test".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_search_domain_is_not_a_route() {
        assert!(routing_domains("[Resolve]\nDomains=corp.example ~rex\n").iter().eq(["rex"].iter()));
        assert!(routing_domains("Domains=~.\n").is_empty(), "the catch-all route is not a TLD");
    }

    #[test]
    fn the_root_commands_use_absolute_paths_and_refuse_without_resolved() {
        let i = install_command("rex", 15353);
        assert!(i.starts_with("/usr/bin/systemctl -q is-active systemd-resolved ||"), "{i}");
        assert!(i.contains("/usr/bin/printf '# Managed by rexenv"));
        assert!(i.contains("> /etc/systemd/resolved.conf.d/rexenv-rex.conf && /usr/bin/systemctl restart systemd-resolved"));
        assert!(!i.contains("$HOME"), "pkexec strips the environment");
        assert_eq!(uninstall_command(&[]), APPLY);
        assert!(uninstall_command(&["rex".into(), "test".into()]).starts_with(
            "/bin/rm -f /etc/systemd/resolved.conf.d/rexenv-rex.conf /etc/systemd/resolved.conf.d/rexenv-test.conf && "
        ));
        let r = restore_command(&[("test".into(), PathBuf::from("/home/u/.local/share/rexenv/backups/it's.conf"))]);
        assert!(r.contains("/bin/cp '/home/u/.local/share/rexenv/backups/it'\\''s.conf' /etc/systemd/resolved.conf.d/rexenv-test.conf"), "{r}");
    }
}
