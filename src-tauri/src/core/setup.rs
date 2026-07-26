//! core::setup — one-time system setup (Phase 1 task 3.4).
//!
//! Two privileged-ish OS changes are needed for backbone (`.rex`) HTTPS to work:
//!  1. install the `/etc/resolver/rex` file (root) — via `PrivilegeManager`
//!     (one admin prompt);
//!  2. trust the local CA — on macOS this writes the USER login keychain and
//!     `security` shows its OWN native auth dialog (no root).
//!
//! A true single prompt for both isn't possible with osascript (System-keychain
//! trust needs a UI session a detached-root shell lacks), so we deliberately use
//! the login keychain for trust. A privileged-helper (SMAppService) would enable
//! a single prompt later.

use crate::core::{dns, ssl, tld};

/// What teardown actually did to the OS resolver files, so the UI can say it
/// rather than claim a generic success. Borrowed files are RESTORED, not
/// removed; files another tool has since reclaimed are left alone.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeardownReport {
    /// Ours — deleted.
    pub removed: Vec<String>,
    /// Borrowed — their file put back.
    pub restored: Vec<String>,
    /// They had already reclaimed these; we touched nothing.
    pub left_alone: Vec<String>,
    /// Borrowed, but our backup was gone: ours removed, theirs unrecoverable.
    pub backup_missing: Vec<String>,
}
use crate::error::Result;
use crate::platform::traits::Platform;

/// The privileged part of setup (the `.rex` backbone resolver-file install)
/// as a shell script — run via `PrivilegeManager`. Pure; exposed for
/// inspection/testing.
pub fn resolver_install_script(platform: &dyn Platform, dns_port: u16) -> String {
    platform.dns().install_command(tld::BACKBONE_TLD, dns_port)
}

/// Run system setup: ensure the CA exists, install the `.rex` backbone
/// resolver file (admin prompt), then trust the CA (native trust dialog).
/// Returns the CA. ONLY `.rex` is installed here — it's the seeded default
/// for new sites and hosts the internal Adminer vhost (`adminer.rexenv.rex`).
/// Every other TLD — `.test` included — gets its resolver file on first use
/// only (site create / change-domain / default change via
/// `dns::ensure_resolver`); a fresh install never writes `/etc/resolver/test`.
///
/// The resolver step is skipped when the file already has the expected content
/// — so on a second macOS account (resolver is system-wide, trust is per-user)
/// setup only shows the keychain dialog, not a pointless admin prompt.
pub fn run_system_setup(platform: &dyn Platform) -> Result<ssl::LocalCa> {
    let ca = ssl::load_or_create(platform.paths(), platform.permissions())?;
    // 1) privileged: install the backbone resolver file (one admin prompt).
    dns::ensure_resolver(platform, tld::BACKBONE_TLD, dns::DEFAULT_DNS_PORT)?;
    // 2) user: trust the CA (native dialog; login keychain, no root).
    ssl::trust_ca(platform, &ca)?;
    Ok(ca)
}

/// Reverse system setup: remove ALL rexenv-owned resolver files — every
/// `/etc/resolver/<tld>` whose content matches our loopback+port signature,
/// whichever TLDs were added over time (one admin prompt) — untrust the CA, and
/// unload + remove the DNS LaunchAgent (unprivileged) so no resolver process is
/// left behind pointing at nothing.
pub fn run_system_teardown(
    conn: &rusqlite::Connection,
    platform: &dyn Platform,
) -> Result<TeardownReport> {
    let ca = ssl::load_or_create(platform.paths(), platform.permissions())?;
    // ONE privileged prompt for every root change on the way out, run together so
    // uninstall shows a single auth dialog:
    //  - boot out + REMOVE the root edge LaunchDaemon (its plist, wrapper, and the
    //    root-owned caddy copy). Without this, `stop_all` skips a `Daemon` edge and
    //    the plist survives, so KeepAlive keeps a root Caddy serving :443 after
    //    "Remove system changes" — a lingering root daemon that contradicts the
    //    command's promise (finding B2);
    //  - remove every rexenv `/etc/resolver/<tld>` file (+ DNS flush).
    let mut root_cmds: Vec<String> = Vec::new();
    if platform.edge().is_installed() {
        root_cmds.push(platform.edge().uninstall_command());
    }
    //  - resolver files: ours get removed, but any we BORROWED from Valet/Herd
    //    get THEIR file put back instead. Once we take a file over it carries
    //    our signature, so without this the sweep would delete it and the user
    //    would be left with neither their config nor ours (v18).
    let plan = dns::plan_resolver_teardown(conn, platform, dns::DEFAULT_DNS_PORT)?;
    if !plan.remove.is_empty() {
        root_cmds.push(platform.dns().uninstall_command(&plan.remove));
    }
    if !plan.restore.is_empty() {
        root_cmds.push(platform.dns().restore_command(&plan.restore));
    }
    if !root_cmds.is_empty() {
        platform.privileges().run_privileged(&root_cmds.join(" ; "))?;
    }
    // Records + their backups die together, and only after the root step
    // actually succeeded.
    dns::finish_resolver_teardown(conn, platform, &plan)?;
    let report = TeardownReport {
        removed: plan.remove.clone(),
        restored: plan.restore.iter().map(|(t, _)| t.clone()).collect(),
        left_alone: plan.reclaimed.clone(),
        backup_missing: plan.backup_missing.clone(),
    };
    ssl::untrust_ca(platform, &ca)?;
    platform.dns_agent().uninstall()?;
    // The `rex` PATH symlink — ours only (content-checked), unprivileged
    // best-effort: teardown must not add a prompt for harmless litter.
    super::cli::remove_symlink_best_effort(platform);
    Ok(report)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn resolver_install_script_writes_backbone_resolver_for_port() {
        let platform = crate::platform::current();
        let script = resolver_install_script(&*platform, 15353);
        // First-run installs ONLY the .rex backbone — never /etc/resolver/test.
        assert!(script.contains("/etc/resolver/rex"));
        assert!(!script.contains("/etc/resolver/test"));
        assert!(script.contains(r"printf 'nameserver 127.0.0.1\nport 15353\n'"));
        // DNS cache flush so the resolver file takes effect immediately.
        assert!(script.contains("dscacheutil -flushcache"));
    }
}
