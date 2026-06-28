//! core::wp_login — one-click "Log in as" via a hardened single-use token (§7.1).
//!
//! Clicking "Log in as" generates a random token, stores only its SHA-256 hash
//! (+ target user + short expiry) in a WP option, and drops a tiny **mu-plugin**
//! that consumes the token on the next request. Security properties enforced by
//! the mu-plugin:
//!   - **single-use** — the option is deleted on the first attempt (success or not);
//!   - **short-TTL** — rejected once `exp` passes (seconds–minutes);
//!   - **loopback/local-only** — rejected if the request carries Cloudflare tunnel
//!     headers (`CF-Connecting-IP`/`CF-Ray`/…), if the originating client (leftmost
//!     `X-Forwarded-For`, else `REMOTE_ADDR`) isn't loopback, or if the `Host` isn't
//!     a local `.test`/`localhost`. So a token captured while a site is shared over
//!     a public Cloudflare tunnel (§9) can't be replayed through it.

use crate::core::wordpress::wp_run;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Default token lifetime (2 minutes).
pub const LOGIN_TTL_SECS: u64 = 120;

/// The auto-managed mu-plugin that consumes the one-time login token.
const MU_PLUGIN: &str = r#"<?php
/* Plugin Name: rexenv one-time login
 * Description: Auto-managed by rexenv for the "Log in as" feature. Safe to delete.
 */
add_action('init', function () {
    if (empty($_GET['rexenv_login'])) {
        return;
    }
    $token   = (string) $_GET['rexenv_login'];
    $user_id = isset($_GET['rexenv_user']) ? (int) $_GET['rexenv_user'] : 0;

    $deny = function () {
        wp_die('rexenv: login link is invalid, expired, already used, or not local.', 'Login', ['response' => 403]);
    };

    // Local-only: never honor a magic login proxied through a public tunnel.
    foreach (['HTTP_CF_CONNECTING_IP', 'HTTP_CF_RAY', 'HTTP_CF_IPCOUNTRY', 'HTTP_CF_VISITOR', 'HTTP_TRUE_CLIENT_IP'] as $h) {
        if (!empty($_SERVER[$h])) { $deny(); }
    }
    $xff    = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? '';
    $client = trim(explode(',', $xff)[0]);
    if ($client === '') { $client = $_SERVER['REMOTE_ADDR'] ?? ''; }
    $loopback = in_array($client, ['127.0.0.1', '::1'], true) || strpos($client, '127.') === 0;
    if (!$loopback) { $deny(); }

    $host = strtolower(explode(':', $_SERVER['HTTP_HOST'] ?? '')[0]);
    $local_host = $host === 'localhost' || $host === '127.0.0.1'
        || substr($host, -5) === '.test' || substr($host, -10) === '.localhost';
    if (!$local_host) { $deny(); }

    // Consume the token regardless of outcome → strictly single-use.
    $stored = get_option('rexenv_login');
    delete_option('rexenv_login');
    if (!$stored) { $deny(); }
    $data = json_decode($stored, true);
    if (!is_array($data)) { $deny(); }
    if ((int) ($data['exp'] ?? 0) < time()) { $deny(); }
    if ((int) ($data['user'] ?? 0) !== $user_id) { $deny(); }
    if (!hash_equals((string) ($data['hash'] ?? ''), hash('sha256', $token))) { $deny(); }

    if (!get_user_by('id', $user_id)) { $deny(); }
    wp_set_auth_cookie($user_id, false);
    wp_set_current_user($user_id);
    wp_safe_redirect(admin_url());
    exit;
});
"#;

/// Path to the auto-managed mu-plugin within a docroot.
fn mu_plugin_path(docroot: &Path) -> PathBuf {
    docroot.join("wp-content").join("mu-plugins").join("rexenv-login.php")
}

/// Write the mu-plugin if missing or changed (idempotent).
pub fn ensure_muplugin(docroot: &Path) -> Result<()> {
    let path = mu_plugin_path(docroot);
    let current = std::fs::read_to_string(&path).ok();
    if current.as_deref() != Some(MU_PLUGIN) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, MU_PLUGIN)?;
    }
    Ok(())
}

/// Issue a one-time login for `user_id`: ensure the mu-plugin, generate a token,
/// store its hash + expiry in the `rexenv_login` option (via WP-CLI), and return
/// the raw token. The caller builds the magic URL
/// (`https://<domain>/?rexenv_login=<token>&rexenv_user=<id>`).
pub fn issue(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    user_id: u64,
    ttl_secs: u64,
) -> Result<String> {
    ensure_muplugin(docroot)?;

    // 256-bit token (two v4 UUIDs of randomness); store only its hash.
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let hash = sha256_hex(token.as_bytes());
    let exp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| Error::Other(format!("clock: {e}")))?
        .as_secs()
        + ttl_secs;

    let payload = serde_json::json!({ "hash": hash, "user": user_id, "exp": exp }).to_string();
    // Store (or overwrite any prior pending token) as a non-autoloaded option.
    wp_run(php_bin, wp_phar, docroot, &["option", "update", "rexenv_login", &payload, "--autoload=no"])?;
    Ok(token)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mu_plugin_path_is_under_mu_plugins() {
        let p = mu_plugin_path(Path::new("/srv/site"));
        assert!(p.ends_with("wp-content/mu-plugins/rexenv-login.php"));
    }

    #[test]
    fn mu_plugin_enforces_local_and_single_use() {
        // The shipped mu-plugin must check tunnel headers, loopback, host, expiry,
        // single-use deletion, and a timing-safe hash compare.
        for needle in [
            "HTTP_CF_CONNECTING_IP",
            "HTTP_X_FORWARDED_FOR",
            "delete_option('rexenv_login')",
            "hash_equals",
            "'exp'",
            "wp_set_auth_cookie",
            ".test",
        ] {
            assert!(MU_PLUGIN.contains(needle), "mu-plugin missing guard: {needle}");
        }
    }

    #[test]
    fn ensure_muplugin_writes_then_is_idempotent() {
        let dir = std::env::temp_dir().join("rexenv-wplogin-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        ensure_muplugin(&dir).unwrap();
        let p = mu_plugin_path(&dir);
        assert!(p.is_file());
        let mtime1 = std::fs::metadata(&p).unwrap().modified().unwrap();
        ensure_muplugin(&dir).unwrap(); // no rewrite when unchanged
        assert_eq!(std::fs::read_to_string(&p).unwrap(), MU_PLUGIN);
        let _ = mtime1;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
