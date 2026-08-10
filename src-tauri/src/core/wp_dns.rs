//! core::wp_dns — let a WordPress site reach ITSELF over its rexenv hostname.
//!
//! **The bug this exists for** (filed 10 Aug 2026, reproduced on the dev Mac):
//! the bundled static-php builds link libcurl against **c-ares**
//! (`curl_version()['ares'] == "1.34.6"`), and c-ares resolves from
//! `/etc/resolv.conf` alone. rexenv publishes its TLDs through macOS split-DNS
//! (`/etc/resolver/<tld>` → `127.0.0.1` port 15353), which `/etc/resolv.conf`
//! has no syntax to express and c-ares never reads. So inside php-fpm:
//!
//! ```text
//! gethostbyname("tr.rex")   => "127.0.0.1"        (getaddrinfo → mDNSResponder → us)
//! curl_exec("https://tr.rex/") => errno 6: Could not resolve host
//! ```
//!
//! Every `wp_remote_*()` to a rexenv hostname therefore failed. WP-Cron spawns
//! itself with exactly such a request and **never checks the result** — so the
//! scheduler stopped with nothing logged, no admin notice, and Site Health
//! quiet. Same for loopback tests, REST self-calls, and calls to sibling sites.
//! WP-CLI hid it: cron events there run in-process, no HTTP involved.
//!
//! **The fix** is this auto-managed mu-plugin: on `http_api_curl` it hands cURL
//! the address the SYSTEM resolver already knows (`CURLOPT_RESOLVE`), but only
//! for hosts whose TLD is served by a LOOPBACK nameserver — a corporate VPN's
//! `/etc/resolver/internal` pointing at `10.x` is left alone, and public DNS is
//! never touched. Nothing about the site (domain, TLD list) is baked in, so the
//! file never goes stale: it re-reads `/etc/resolver/` per host and asks the
//! system resolver for the answer. A build that already uses curl's threaded
//! resolver (FrankenPHP, Homebrew PHP) makes it a no-op on its first line.

use crate::error::Result;
use crate::state::models::{Site, SiteType};
use crate::state::store;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// The auto-managed mu-plugin. No placeholders: it is byte-identical for every
/// site, which is why nothing here needs re-writing on a domain or TLD change.
const MU_PLUGIN: &str = r#"<?php
/* Plugin Name: rexenv loopback DNS
 * Description: Auto-managed by rexenv so this site can reach itself (WP-Cron,
 *   Site Health, REST self-calls) over its local hostname. Safe to delete.
 */

// rexenv's bundled PHP links libcurl against c-ares, which reads
// /etc/resolv.conf ONLY — it never sees macOS split-DNS (/etc/resolver/<tld>),
// where rexenv publishes its local TLDs. gethostbyname() resolves the site
// fine; every curl request to the same host died with "cURL error 6". WP-Cron
// is fire-and-forget, so that failure was completely silent.
//
// Below: for a host whose TLD is served by a LOOPBACK nameserver, hand cURL
// the address the system resolver gives. Public domains, and TLDs whose
// resolver points somewhere real (a VPN's split-DNS), fall through untouched.
add_action('http_api_curl', function ($handle, $args = array(), $url = '') {
    static $cache = array();

    if (!is_string($url) || $url === '') {
        return;
    }
    // Threaded-resolver builds (FrankenPHP, most distro PHP) already go through
    // getaddrinfo and honour /etc/resolver — nothing to fix.
    $curl = curl_version();
    if (empty($curl['ares'])) {
        return;
    }

    $host = strtolower((string) parse_url($url, PHP_URL_HOST));
    if ($host === '' || filter_var($host, FILTER_VALIDATE_IP)) {
        return;
    }

    if (!array_key_exists($host, $cache)) {
        $cache[$host] = rexenv_dns_loopback_ip($host);
    }
    if ($cache[$host] === null) {
        return;
    }

    $scheme = strtolower((string) parse_url($url, PHP_URL_SCHEME));
    $port   = (int) parse_url($url, PHP_URL_PORT);
    if ($port <= 0) {
        $port = $scheme === 'https' ? 443 : 80;
    }
    curl_setopt($handle, CURLOPT_RESOLVE, array($host . ':' . $port . ':' . $cache[$host]));
}, 10, 3);

/**
 * Whether an /etc/resolver/<tld> file describes a zone served from THIS machine.
 * A company VPN's split-DNS ("nameserver 10.8.0.1") must answer false: rerouting
 * it to whatever the local resolver says would break a working setup in the name
 * of fixing a local one. Split out so it can be proven directly (wp_dns_check).
 */
function rexenv_dns_resolver_is_loopback($conf) {
    return (bool) preg_match(
        '/^[ \t]*nameserver[ \t]+(?:127\.\d{1,3}\.\d{1,3}\.\d{1,3}|::1)[ \t]*$/mi',
        $conf
    );
}

/**
 * The IP for $host when — and only when — its TLD is a split-DNS zone served by
 * a loopback nameserver. Returns null for everything else.
 */
function rexenv_dns_loopback_ip($host) {
    $dot = strrpos($host, '.');
    if ($dot === false) {
        return null;
    }
    $tld = substr($host, $dot + 1);
    // Path-safe TLD only: this value is concatenated into a filesystem path.
    if (!preg_match('/\A[a-z0-9-]{1,63}\z/', $tld)) {
        return null;
    }
    $file = '/etc/resolver/' . $tld;
    if (!is_file($file)) {
        return null;
    }
    $conf = @file_get_contents($file);
    if (!is_string($conf) || !rexenv_dns_resolver_is_loopback($conf)) {
        return null;
    }
    // The system resolver (getaddrinfo) DOES read /etc/resolver — ask it, and
    // pass its answer on rather than assuming 127.0.0.1.
    $ip = gethostbyname($host);
    if ($ip === $host || !filter_var($ip, FILTER_VALIDATE_IP)) {
        return null;
    }
    return $ip;
}
"#;

/// Path to the mu-plugin within a docroot. `content_rel` is the site's RECORDED
/// content dir (`Site::content_dir_rel`, v24 — `app` for Bedrock, `content` for
/// Radicle): a hardcoded `wp-content/` both litters the user's repo and writes
/// where WordPress never loads, which for THIS file would mean the cron fix
/// silently not applying — the exact failure mode it exists to end.
fn mu_plugin_path(docroot: &Path, content_rel: &str) -> PathBuf {
    docroot.join(content_rel).join("mu-plugins").join("rexenv-dns.php")
}

/// Whether the mu-plugin is installed for `content_rel` (live stat, no record).
pub fn is_installed(docroot: &Path, content_rel: &str) -> bool {
    mu_plugin_path(docroot, content_rel).is_file()
}

/// Write the mu-plugin if missing or changed (idempotent). Returns whether the
/// `mu-plugins/` DIR was created by this call — the caller records that fact
/// (v25 `sites.mu_dir_created`) so teardown can remove a dir WE created without
/// ever inferring ownership from emptiness.
pub fn ensure(docroot: &Path, content_rel: &str) -> Result<bool> {
    let path = mu_plugin_path(docroot, content_rel);
    let mut created_dir = false;
    if std::fs::read_to_string(&path).ok().as_deref() != Some(MU_PLUGIN) {
        if let Some(parent) = path.parent() {
            created_dir = !parent.exists();
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, MU_PLUGIN)?;
    }
    Ok(created_dir)
}

/// Install (or refresh) the mu-plugin for one WordPress site and record a
/// `mu-plugins/` dir we created (v25). Best-effort by design — a site whose
/// docroot is a temporarily missing linked folder, or whose content dir does
/// not exist yet, is skipped rather than conjured: writing a `wp-content/`
/// into an unmounted volume's mount point would be worse than a site whose
/// cron waits for the next launch. Returns whether the file is now in place.
pub fn ensure_for_site(conn: &Connection, site: &Site) -> bool {
    if site.site_type != SiteType::Wordpress {
        return false;
    }
    let docroot = Path::new(&site.path);
    let content = docroot.join(site.content_dir_rel());
    if !content.is_dir() {
        return false;
    }
    match ensure(docroot, site.content_dir_rel()) {
        Ok(created_dir) => {
            if created_dir {
                let _ = store::set_site_mu_dir_created(conn, &site.id);
            }
            true
        }
        Err(e) => {
            log::warn!("wp_dns: could not install the loopback-DNS mu-plugin for {}: {e}", site.domain);
            false
        }
    }
}

/// Install (or refresh) the mu-plugin for every WordPress site — the startup
/// pass. It is what makes the promise "every site rexenv hosts" true rather
/// than "every site created since this shipped": sites that predate it, sites
/// imported by another path, and sites whose file a user deleted are all
/// picked up at the next launch.
pub fn ensure_all(conn: &Connection, sites: &[Site]) {
    let installed = sites.iter().filter(|s| ensure_for_site(conn, s)).count();
    if installed > 0 {
        log::info!("wp_dns: loopback-DNS mu-plugin in place for {installed} WordPress site(s)");
    }
}

/// Remove the mu-plugin across EVERY known layout (same sweep as the login and
/// tunnel files — a stray in a Bedrock repo's dead `wp-content/` still goes).
/// Missing files are fine.
pub fn remove(docroot: &Path) -> Result<()> {
    for layout in crate::core::sites::CONTENT_DIR_LAYOUTS {
        match std::fs::remove_file(mu_plugin_path(docroot, layout)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rexenv-wpdns-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn mu_plugin_path_follows_the_recorded_content_dir() {
        let p = mu_plugin_path(Path::new("/srv/site"), "wp-content");
        assert!(p.ends_with("wp-content/mu-plugins/rexenv-dns.php"));
        // Bedrock: content lives at docroot/app. Writing to wp-content there
        // means the file never loads — and this file not loading is invisible
        // (cron just stays silent), the failure this module exists to end.
        let p = mu_plugin_path(Path::new("/srv/bedrock/web"), "app");
        assert!(p.ends_with("web/app/mu-plugins/rexenv-dns.php"));
    }

    #[test]
    fn ensure_writes_then_is_idempotent_and_remove_sweeps_every_layout() {
        let dir = fixture("lifecycle");
        assert!(ensure(&dir, "wp-content").unwrap(), "first write creates mu-plugins/");
        assert!(is_installed(&dir, "wp-content"));
        assert!(!ensure(&dir, "wp-content").unwrap(), "re-ensure creates nothing");

        // A hand-edited file is restored (the content is ours, not the user's).
        std::fs::write(mu_plugin_path(&dir, "wp-content"), "<?php // tampered").unwrap();
        ensure(&dir, "wp-content").unwrap();
        assert_eq!(std::fs::read_to_string(mu_plugin_path(&dir, "wp-content")).unwrap(), MU_PLUGIN);

        ensure(&dir, "app").unwrap();
        remove(&dir).unwrap();
        for layout in crate::core::sites::CONTENT_DIR_LAYOUTS {
            assert!(!mu_plugin_path(&dir, layout).exists(), "left behind in {layout}");
        }
        remove(&dir).unwrap(); // idempotent on missing files
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_plugin_keeps_its_guards_and_bakes_in_no_per_site_state() {
        // What THIS level proves: the guard clauses are present. That the fix
        // WORKS — a real wp_remote_get to the site's own https URL returning
        // 200 through php-fpm — is proven by `examples/wp_dns_check.rs`.
        for needle in [
            "http_api_curl",         // the hook every wp_remote_*() curl call passes
            "CURLOPT_RESOLVE",       // the fix itself
            "empty($curl['ares'])",  // no-op on threaded-resolver builds
            "/etc/resolver/",        // the split-DNS zone test
            "nameserver",            // ...restricted to LOOPBACK nameservers
            "gethostbyname",         // the answer comes from the system resolver
            "[a-z0-9-]{1,63}",       // the TLD is concatenated into a path
        ] {
            assert!(MU_PLUGIN.contains(needle), "mu-plugin missing guard tripwire: {needle}");
        }
        // No per-site interpolation: no domain, no TLD list, no placeholder —
        // that is what makes a domain change or a new TLD a non-event here
        // (the login file, which DOES bake the domain, must be rewritten).
        assert!(!MU_PLUGIN.contains("{{"), "a placeholder would need a rewrite trigger");
    }
}
