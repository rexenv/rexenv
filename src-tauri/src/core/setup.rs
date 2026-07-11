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
/// whichever TLDs were added over time (one admin prompt) — and untrust the CA.
pub fn run_system_teardown(platform: &dyn Platform) -> Result<()> {
    let ca = ssl::load_or_create(platform.paths(), platform.permissions())?;
    dns::remove_all_resolvers(platform, dns::DEFAULT_DNS_PORT)?;
    ssl::untrust_ca(platform, &ca)?;
    Ok(())
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
