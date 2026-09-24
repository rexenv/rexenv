//! CA trust on Linux, in two stores (docs/PLAN-linux-port.md D-L3), as pure text:
//!
//! 1. the NSS user database `~/.pki/nssdb` — what Chrome, Chromium, Brave, Edge and Vivaldi read
//!    on Linux; written with `certutil` (libnss3-tools) by the USER, no prompt;
//! 2. the system store — `/usr/local/share/ca-certificates/<name>.crt` +
//!    `update-ca-certificates`, what `curl`, PHP, WP-CLI and Composer read; a ROOT step.
//!
//! Firefox keeps its own store; `core::firefox` handles it with a policy file on every OS.
//! `is_trusted` answers for the NSS half: the browsers are what the user sees first.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::path::{Path, PathBuf};

/// The nickname rexenv's CA carries in NSS. ONE fixed name: `untrust_stale` finds an older CA
/// under it and replaces it, so a regenerated CA never leaves two rexenv roots trusted.
pub(crate) const NSS_NICKNAME: &str = "rexenv local CA";
/// The file name in the system store. `update-ca-certificates` requires `.crt`.
pub(crate) const SYSTEM_CERT_NAME: &str = "rexenv-local-ca.crt";
pub(crate) const SYSTEM_CERT_DIR: &str = "/usr/local/share/ca-certificates";

pub(crate) fn nss_db_dir(home: &Path) -> PathBuf {
    home.join(".pki/nssdb")
}

/// Every NSS database a browser on this machine reads — `~/.pki/nssdb` (Chrome's deb, Brave,
/// Edge, Vivaldi, an unconfined Chromium) and, when Ubuntu's SNAP Chromium has ever run, ITS
/// database: a snap's `$HOME` is `~/snap/chromium/current` and its XDG data dir
/// `…/current/.local/share`, where the snap keeps `pki/nssdb`. Measured 24 Sep 2026 on the VM:
/// with the CA in `~/.pki/nssdb` only, the snap showed `NET::ERR_CERT_AUTHORITY_INVALID` for
/// `https://acme.rex`, and `find` put its `cert9.db` at
/// `~/snap/chromium/3533/.local/share/pki/nssdb` (`current` → `3533`). snapd carries the
/// revision's user data across refreshes, so the copy survives a Chromium update.
///
/// **And every Firefox profile** (`cert9.db` lives IN the profile directory). On the other
/// OSes `core::firefox`'s `user.js` pref (`security.enterprise_roots.enabled`) makes Firefox
/// import the OS store, so no NSS write is needed; on Ubuntu the snap Firefox cannot see
/// `/usr/local/share/ca-certificates` from inside its confinement, so the pref imports nothing.
/// Measured 25 Sep 2026 on the VM: with the pref forced in `user.js`, a headless
/// `firefox --screenshot https://guard.rex` hung on the TLS error for 150 s; `certutil -A` into
/// the profile's database, and the same command rendered the site in seconds. Profiles come
/// from `profiles.ini` under both roots (`firefox_roots`), never from a directory glob.
pub(crate) fn nss_db_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![nss_db_dir(home)];
    let snap_home = home.join("snap/chromium/current");
    if snap_home.is_dir() {
        dirs.push(snap_home.join(".local/share/pki/nssdb"));
    }
    for root in firefox_roots(home) {
        dirs.extend(crate::core::firefox::profiles(&root));
    }
    dirs
}

/// `certutil`'s database argument — the SQLite form, which is what Chrome creates and reads.
pub(crate) fn nss_db_arg(home: &Path) -> String {
    nss_db_arg_for(&nss_db_dir(home))
}

pub(crate) fn nss_db_arg_for(db: &Path) -> String {
    format!("sql:{}", db.display())
}

/// The fix when `certutil` is missing — named in the error, never guessed at by the user.
pub(crate) const CERTUTIL_MISSING: &str = "certutil is not installed, so rexenv cannot add its certificate authority \
to your browsers' trust store. Install it, then retry:\n$ sudo apt install libnss3-tools";

/// `certutil -A`: add `cert` as a trusted CA for SSL (`C,,`).
pub(crate) fn nss_add_args(home: &Path, cert: &Path) -> Vec<String> {
    nss_add_args_for(&nss_db_dir(home), cert)
}

pub(crate) fn nss_add_args_for(db: &Path, cert: &Path) -> Vec<String> {
    vec![
        "-d".into(),
        nss_db_arg_for(db),
        "-A".into(),
        "-t".into(),
        "C,,".into(),
        "-n".into(),
        NSS_NICKNAME.into(),
        "-i".into(),
        cert.display().to_string(),
    ]
}

/// `certutil -D`: remove the nickname.
pub(crate) fn nss_delete_args(home: &Path) -> Vec<String> {
    nss_delete_args_for(&nss_db_dir(home))
}
pub(crate) fn nss_delete_args_for(db: &Path) -> Vec<String> {
    vec!["-d".into(), nss_db_arg_for(db), "-D".into(), "-n".into(), NSS_NICKNAME.into()]
}

/// `certutil -L -a`: the certificate under the nickname as PEM, for comparison with ours.
pub(crate) fn nss_show_args(home: &Path) -> Vec<String> {
    nss_show_args_for(&nss_db_dir(home))
}
pub(crate) fn nss_show_args_for(db: &Path) -> Vec<String> {
    vec!["-d".into(), nss_db_arg_for(db), "-L".into(), "-n".into(), NSS_NICKNAME.into(), "-a".into()]
}

/// `certutil -N --empty-password`: create the database when Chrome never has.
pub(crate) fn nss_create_args(home: &Path) -> Vec<String> {
    nss_create_args_for(&nss_db_dir(home))
}
pub(crate) fn nss_create_args_for(db: &Path) -> Vec<String> {
    vec!["-d".into(), nss_db_arg_for(db), "-N".into(), "--empty-password".into()]
}

/// Two PEMs describe the same certificate when their base64 bodies match — whitespace and
/// header text differ between writers and mean nothing.
pub(crate) fn same_pem(a: &str, b: &str) -> bool {
    fn body(s: &str) -> String {
        s.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("-----"))
            .collect::<String>()
    }
    let (a, b) = (body(a), body(b));
    !a.is_empty() && a == b
}

/// The root shell that installs the CA system-wide. Absolute paths: `pkexec`'s environment.
pub(crate) fn system_trust_command(cert: &Path) -> String {
    format!(
        "/bin/mkdir -p {dir} && /bin/cp {src} {dst} && /bin/chmod 644 {dst} && /usr/sbin/update-ca-certificates",
        dir = SYSTEM_CERT_DIR,
        src = super::dnsroute::sh_quote(cert),
        dst = format!("{SYSTEM_CERT_DIR}/{SYSTEM_CERT_NAME}"),
    )
}

/// The root shell that removes it. `--fresh` rebuilds the bundle without the removed file.
pub(crate) fn system_untrust_command() -> String {
    format!("/bin/rm -f {SYSTEM_CERT_DIR}/{SYSTEM_CERT_NAME} && /usr/sbin/update-ca-certificates --fresh")
}

/// Where Firefox keeps profiles on Linux: the deb/tarball location, and Ubuntu's snap.
pub(crate) fn firefox_roots(home: &Path) -> [PathBuf; 2] {
    [home.join(".mozilla/firefox"), home.join("snap/firefox/common/.mozilla/firefox")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certutil_writes_the_sqlite_user_db_under_one_nickname() {
        let home = Path::new("/home/u");
        assert_eq!(nss_db_arg(home), "sql:/home/u/.pki/nssdb");
        let add = nss_add_args(home, Path::new("/home/u/.local/share/rexenv/ca/ca.pem"));
        assert_eq!(add[..7], ["-d", "sql:/home/u/.pki/nssdb", "-A", "-t", "C,,", "-n", "rexenv local CA"]);
        assert_eq!(nss_delete_args(home)[2..], ["-D", "-n", "rexenv local CA"]);
        assert!(nss_show_args(home).ends_with(&["-a".to_string()]));
    }

    /// The snap's database joins the list only once the snap has a home — a machine without
    /// snap Chromium gets one database, never an empty directory created for a browser it lacks.
    #[test]
    fn the_snap_chromium_db_is_listed_only_when_the_snap_has_run() {
        let home = std::env::temp_dir().join(format!("rexenv-nss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(nss_db_dirs(&home), vec![home.join(".pki/nssdb")]);
        std::fs::create_dir_all(home.join("snap/chromium/current")).unwrap();
        assert_eq!(nss_db_dirs(&home), vec![home.join(".pki/nssdb"), home.join("snap/chromium/current/.local/share/pki/nssdb")]);
        assert_eq!(nss_db_arg_for(&home.join("snap/chromium/current/.local/share/pki/nssdb")), format!("sql:{}/snap/chromium/current/.local/share/pki/nssdb", home.display()));
        // A Firefox profile named by profiles.ini is a database too (the snap root here — the
        // one the VM measured; the deb root is the same code). A directory profiles.ini does
        // not name is not.
        let ff = home.join("snap/firefox/common/.mozilla/firefox");
        std::fs::create_dir_all(ff.join("s4g1o8kw.default")).unwrap();
        std::fs::create_dir_all(ff.join("stray.dir")).unwrap();
        std::fs::write(ff.join("profiles.ini"), "[Profile0]\nName=default\nIsRelative=1\nPath=s4g1o8kw.default\nDefault=1\n").unwrap();
        assert_eq!(
            nss_db_dirs(&home),
            vec![home.join(".pki/nssdb"), home.join("snap/chromium/current/.local/share/pki/nssdb"), ff.join("s4g1o8kw.default")]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn pem_equality_ignores_wrapping_and_headers() {
        let a = "-----BEGIN CERTIFICATE-----\nAAAA\nBBBB\n-----END CERTIFICATE-----\n";
        let b = "-----BEGIN CERTIFICATE-----\r\nAAAABBBB\r\n-----END CERTIFICATE-----";
        assert!(same_pem(a, b));
        assert!(!same_pem(a, "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n"));
        assert!(!same_pem("", ""), "two empties are not the same certificate");
    }

    #[test]
    fn the_system_store_commands_are_absolute_and_name_the_crt() {
        let c = system_trust_command(Path::new("/home/u/.local/share/rexenv/ca/ca.pem"));
        assert_eq!(
            c,
            "/bin/mkdir -p /usr/local/share/ca-certificates && /bin/cp '/home/u/.local/share/rexenv/ca/ca.pem' /usr/local/share/ca-certificates/rexenv-local-ca.crt && /bin/chmod 644 /usr/local/share/ca-certificates/rexenv-local-ca.crt && /usr/sbin/update-ca-certificates"
        );
        assert!(system_untrust_command().ends_with("update-ca-certificates --fresh"));
    }

    #[test]
    fn firefox_is_looked_for_where_the_snap_puts_it_too() {
        let [deb, snap] = firefox_roots(Path::new("/home/u"));
        assert_eq!(deb, PathBuf::from("/home/u/.mozilla/firefox"));
        assert_eq!(snap, PathBuf::from("/home/u/snap/firefox/common/.mozilla/firefox"));
    }
}
