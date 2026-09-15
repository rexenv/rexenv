//! core::setup — one-time system setup (Phase 1 task 3.4).
//!
//! Two privileged-ish OS changes are needed for backbone (`.rex`) HTTPS to work:
//!  1. install the `/etc/resolver/rex` file (root) — via `PrivilegeManager`
//!     (one admin prompt);
//!  2. trust the local CA — on macOS this writes the USER login keychain and
//!     rexenv itself raises the keychain dialog (no root; called in-process so
//!     the dialog is titled rexenv, not `security` — `platform/macos/keychain_trust.rs`).
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
    dns::ensure_resolver(
        platform,
        tld::BACKBONE_TLD,
        dns::DEFAULT_DNS_PORT,
        dns::ResolverPrompt::Allow, // first-run setup: the user IS the one asking
    )?;
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
    db: &std::sync::Mutex<rusqlite::Connection>,
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
    //
    // Planned under the database lock, which is RELEASED before the prompt: the
    // admin dialog (and the keychain one below) stays open as long as the user
    // takes, and every command that reads the database waited on it (#569).
    let plan = dns::plan_resolver_teardown(&*dns::db_lock(db)?, platform, dns::DEFAULT_DNS_PORT)?;
    if !plan.remove.is_empty() {
        root_cmds.push(platform.dns().uninstall_command(&plan.remove));
    }
    if !plan.restore.is_empty() {
        root_cmds.push(platform.dns().restore_command(&plan.restore));
    }
    if !root_cmds.is_empty() {
        platform.privileges().run_privileged(
            &root_cmds.join(" ; "),
            &crate::platform::traits::PromptReason::new(
                "remove its system changes (DNS resolvers and the HTTPS server)",
            ),
        )?;
    }
    // Records + their backups die together, and only after the root step
    // actually succeeded.
    dns::finish_resolver_teardown(&*dns::db_lock(db)?, platform, &plan)?;
    let report = TeardownReport {
        removed: plan.remove.clone(),
        restored: plan.restore.iter().map(|(t, _)| t.clone()).collect(),
        left_alone: plan.reclaimed.clone(),
        backup_missing: plan.backup_missing.clone(),
    };
    ssl::untrust_ca(platform, &ca)?;
    platform.dns_agent().uninstall()?;
    // The `rex` PATH install — the symlink when ours (content-checked), or on
    // Windows the copy in rexenv's own folder and its user-Path entry (#634);
    // unprivileged best-effort: teardown must not add a prompt for harmless litter.
    super::cli::remove_symlink_best_effort(platform);
    Ok(report)
}

// Its own `#[cfg(test)]` module, not the macOS-gated one below: this test runs on
// fakes, and `copy_scan::production_source` strips only `#[cfg(test)]` — inside
// `cfg(all(test, …))` the prompt guard read it as production code.
#[cfg(test)]
mod lock_tests {
    use super::*;

    /// #569 — **teardown never holds the database lock across its prompts**: the
    /// root one (restore/remove the resolver files) or the keychain one (untrust
    /// the CA). Both stay open as long as the user takes; with the lock held,
    /// every command that reads the database waited on the user.
    #[test]
    fn teardown_prompts_without_holding_the_database_lock() {
        use crate::platform::traits::{
            AutostartManager, BinaryProvider, CertTrustManager, DnsAgentManager, DnsManager,
            EdgeSupervisor, Paths, PermissionManager, PrivilegeManager, ProcessSupervisor,
            ShellRunner,
        };
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        let root = std::env::temp_dir().join(format!("rexenv-teardown569-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("config")).unwrap();

        struct TmpPaths(PathBuf);
        impl Paths for TmpPaths {
            fn app_data_dir(&self) -> Result<PathBuf> {
                Ok(self.0.clone())
            }
            fn config_dir(&self) -> Result<PathBuf> {
                Ok(self.0.join("config"))
            }
            fn log_dir(&self) -> Result<PathBuf> {
                Ok(self.0.join("logs"))
            }
            fn bin_dir(&self) -> Result<PathBuf> {
                Ok(self.0.join("bin"))
            }
            fn hosts_file(&self) -> PathBuf {
                self.0.join("hosts")
            }
        }
        struct TmpDns(PathBuf);
        impl DnsManager for TmpDns {
            fn route_label(&self, tld: &str) -> String {
                self.0.join(format!("resolver-{tld}")).display().to_string()
            }
            fn route_contents(&self, port: u16) -> String {
                format!("nameserver 127.0.0.1\nport {port}\n")
            }
            fn route_owner(&self, tld: &str, port: u16) -> crate::platform::traits::ResolverOwner {
                crate::platform::resolver_files::owner_of(&self.0.join(format!("resolver-{tld}")), &self.route_contents(port))
            }
            fn our_route_tlds(&self, port: u16) -> Vec<String> {
                crate::platform::resolver_files::tlds_matching_signature(&self.0, &self.route_contents(port))
            }
            fn foreign_route_tlds(&self, port: u16) -> Vec<String> {
                crate::platform::resolver_files::tlds_not_matching_signature(&self.0, &self.route_contents(port))
            }
            fn install_command(&self, _tld: &str, _port: u16) -> String {
                "install".into()
            }
            fn uninstall_command(&self, _tlds: &[String]) -> String {
                "uninstall".into()
            }
            fn restore_command(&self, _restores: &[(String, PathBuf)]) -> String {
                "restore".into()
            }
        }
        /// Every dialog — root or keychain — notes whether the database was free.
        #[derive(Clone)]
        struct Probe {
            db: Arc<Mutex<rusqlite::Connection>>,
            asked: Arc<AtomicUsize>,
            held: Arc<AtomicUsize>,
        }
        impl Probe {
            fn ask(&self) {
                self.asked.fetch_add(1, Ordering::SeqCst);
                if self.db.try_lock().is_err() {
                    self.held.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        impl PrivilegeManager for Probe {
            fn run_privileged(
                &self,
                _script: &str,
                _reason: &crate::platform::traits::PromptReason,
            ) -> Result<String> {
                self.ask();
                Ok(String::new())
            }
        }
        struct Trust(Probe);
        impl CertTrustManager for Trust {
            fn trust_ca(&self, _ca: &Path) -> Result<()> {
                Ok(())
            }
            fn untrust_ca(&self, _ca: &Path) -> Result<()> {
                self.0.ask();
                Ok(())
            }
        }
        struct NoEdge;
        impl EdgeSupervisor for NoEdge {
            fn is_installed(&self) -> bool {
                false
            }
            fn is_enabled(&self) -> bool {
                true
            }
            fn plist_path(&self) -> PathBuf {
                PathBuf::new()
            }
            fn wrapper_path(&self) -> PathBuf {
                PathBuf::new()
            }
            fn daemon_binary_path(&self) -> PathBuf {
                PathBuf::new()
            }
            fn plist_contents(&self, _w: &Path, _l: &Path) -> String {
                String::new()
            }
            fn wrapper_contents(&self, _c: &Path, _f: &Path, _s: &Path, _a: &Path) -> String {
                String::new()
            }
            fn install_command(&self, _c: &Path, _w: &Path, _p: &Path) -> String {
                String::new()
            }
            fn start_command(&self) -> String {
                String::new()
            }
            fn stop_command(&self) -> String {
                String::new()
            }
            fn uninstall_command(&self) -> String {
                String::new()
            }
        }
        struct NoAgent;
        impl DnsAgentManager for NoAgent {
            fn is_installed(&self) -> bool {
                false
            }
            fn definition_path(&self) -> Result<PathBuf> {
                Ok(PathBuf::new())
            }
            fn definition_contents(&self, _e: &Path, _l: &Path) -> String {
                String::new()
            }
            fn install(&self, _e: &Path, _l: &Path) -> Result<()> {
                Ok(())
            }
            fn kickstart(&self) -> Result<()> {
                Ok(())
            }
            fn uninstall(&self) -> Result<()> {
                Ok(())
            }
        }
        struct RealPerms;
        impl PermissionManager for RealPerms {
            fn set_executable(&self, _p: &Path) -> Result<()> {
                Ok(())
            }
            fn set_private(&self, _p: &Path) -> Result<()> {
                Ok(())
            }
            fn write_private(&self, path: &Path, contents: &[u8]) -> Result<()> {
                std::fs::write(path, contents)?;
                Ok(())
            }
        }
        struct P(TmpPaths, TmpDns, Probe, Trust, NoEdge, NoAgent, RealPerms);
        impl Platform for P {
            fn paths(&self) -> &dyn Paths {
                &self.0
            }
            fn dns(&self) -> &dyn DnsManager {
                &self.1
            }
            fn privileges(&self) -> &dyn PrivilegeManager {
                &self.2
            }
            fn cert_trust(&self) -> &dyn CertTrustManager {
                &self.3
            }
            fn edge(&self) -> &dyn EdgeSupervisor {
                &self.4
            }
            fn dns_agent(&self) -> &dyn DnsAgentManager {
                &self.5
            }
            fn permissions(&self) -> &dyn PermissionManager {
                &self.6
            }
            fn supervisor(&self) -> &dyn ProcessSupervisor {
                unimplemented!()
            }
            fn autostart(&self) -> &dyn AutostartManager {
                unimplemented!()
            }
            fn shell(&self) -> &dyn ShellRunner {
                unimplemented!()
            }
            fn binaries(&self) -> &dyn BinaryProvider {
                unimplemented!()
            }
            fn app_bundle(&self) -> &dyn crate::platform::traits::AppBundle {
                unimplemented!()
            }
        }

        let db = Arc::new(Mutex::new(crate::state::db::open_in_memory().unwrap()));
        // A borrowed TLD: ours on disk now, their file kept in our backup — so the
        // root step has real work (a restore) and the record must go afterwards.
        const THEIRS: &str = "nameserver 127.0.0.1\nport 53\n";
        let backup = root.join("backup-test");
        std::fs::write(&backup, THEIRS).unwrap();
        std::fs::write(root.join("resolver-test"), "nameserver 127.0.0.1\nport 15353\n").unwrap();
        crate::state::store::insert_resolver_takeover(
            &db.lock().unwrap(),
            "test",
            THEIRS,
            &backup.display().to_string(),
        )
        .unwrap();

        let probe = Probe { db: db.clone(), asked: Arc::new(AtomicUsize::new(0)), held: Arc::new(AtomicUsize::new(0)) };
        let platform = P(
            TmpPaths(root.clone()),
            TmpDns(root.clone()),
            probe.clone(),
            Trust(probe.clone()),
            NoEdge,
            NoAgent,
            RealPerms,
        );

        let report = run_system_teardown(&db, &platform).expect("teardown");
        assert_eq!(report.restored, vec!["test".to_string()], "their file is put back: {report:?}");
        assert!(
            crate::state::store::get_resolver_takeover(&db.lock().unwrap(), "test").unwrap().is_none(),
            "the record goes once the root step succeeded"
        );
        assert_eq!(probe.asked.load(Ordering::SeqCst), 2, "the root prompt and the keychain prompt");
        assert_eq!(
            probe.held.load(Ordering::SeqCst),
            0,
            "the database was locked while a teardown dialog was open — every command that reads \
             it waits on the user"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

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
