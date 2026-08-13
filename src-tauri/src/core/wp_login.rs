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
//!     local: `localhost`/`.localhost`/`.test`, or the site's OWN domain (injected
//!     per-site so custom TLDs like `.rex` work) including its subdomains
//!     (multisite). So a token captured while a site is shared over a public
//!     Cloudflare tunnel (§9) can't be replayed through it.

use crate::core::wordpress::wp_run;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Default token lifetime (2 minutes).
pub const LOGIN_TTL_SECS: u64 = 120;

/// The auto-managed mu-plugin that consumes the one-time login token.
/// Template: `{{SITE_DOMAIN}}` is replaced with the site's own (already
/// validated, `[a-z0-9.-]`-only) domain by [`mu_plugin_source`], so the host
/// allow-list covers custom TLDs like `.rex` without opening up to arbitrary
/// hosts.
const MU_PLUGIN: &str = r#"<?php
/* Plugin Name: rexenv one-time login
 * Description: Auto-managed by rexenv for the "Log in as" feature. Safe to delete.
 */
// A denied magic link falls back to the normal login page with a clear notice
// (rexenv_denied=1 below) instead of a dead-end error page.
add_filter('login_message', function ($message) {
    if (!empty($_GET['rexenv_denied'])) {
        $message = '<div id="login_error">rexenv: the one-click login link was invalid, expired,'
            . ' already used, or not local. Log in manually below.</div>' . $message;
    }
    return $message;
});

add_action('init', function () {
    if (empty($_GET['rexenv_login'])) {
        return;
    }
    $token   = (string) $_GET['rexenv_login'];
    $user_id = isset($_GET['rexenv_user']) ? (int) $_GET['rexenv_user'] : 0;

    $deny = function () {
        wp_safe_redirect(add_query_arg('rexenv_denied', '1', wp_login_url()));
        exit;
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
    // rexenv injects this site's own domain so custom TLDs (e.g. .rex) pass;
    // subdomains of it are allowed for multisite.
    $site   = '{{SITE_DOMAIN}}';
    $local_host = $host === 'localhost' || $host === '127.0.0.1'
        || $host === $site || substr($host, -strlen('.' . $site)) === '.' . $site
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

/// Path to the auto-managed mu-plugin within a docroot. `content_rel` is the
/// site's RECORDED content dir (`Site::content_dir_rel`, v24 — `app` for
/// Bedrock, `content` for Radicle): writing to a hardcoded `wp-content/`
/// there both litters the user's repo and silently breaks this feature (the
/// file never loads).
fn mu_plugin_path(docroot: &Path, content_rel: &str) -> PathBuf {
    docroot.join(content_rel).join("mu-plugins").join("rexenv-login.php")
}

/// The mu-plugin source for a site: the template with the site's own domain
/// injected into the host allow-list. `domain` is the stored (already
/// `validate_domain`-vetted, `[a-z0-9.-]`-only) value — it can't escape the
/// single-quoted PHP string.
fn mu_plugin_source(domain: &str) -> String {
    MU_PLUGIN.replace("{{SITE_DOMAIN}}", domain)
}

/// The character class a vetted hostname can contain — nothing that can close
/// or escape a single-quoted PHP string (`'`, `\`) or smuggle interpolation.
/// `core::sites::validate_domain` enforces more; this narrower re-check lives
/// AT the injection point (see [`ensure_muplugin`]).
fn domain_is_php_string_safe(domain: &str) -> bool {
    !domain.is_empty()
        && domain
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

/// Write the mu-plugin if missing or changed (idempotent). Content is per-site
/// (the domain is baked into the host allow-list), so a domain change is
/// picked up by the next `issue` call rewriting the file. Returns whether the
/// `mu-plugins/` DIR was created by this call — the caller records that fact
/// (v25 `sites.mu_dir_created`) so site teardown can remove a dir WE created
/// without ever inferring ownership from emptiness.
pub fn ensure_muplugin(docroot: &Path, content_rel: &str, domain: &str) -> Result<bool> {
    // Injection-point guard (parity with wp_tunnel::validate_origin): the
    // domain is validate_domain-vetted upstream (M7), but THIS is where it
    // enters a single-quoted PHP string — re-check the character class here
    // so no future call path can reach the replace with an escape.
    if !domain_is_php_string_safe(domain) {
        return Err(crate::error::Error::Other(format!(
            "domain {domain:?} is not a vetted hostname"
        )));
    }
    let path = mu_plugin_path(docroot, content_rel);
    let source = mu_plugin_source(domain);
    let current = std::fs::read_to_string(&path).ok();
    let mut created_dir = false;
    if current.as_deref() != Some(source.as_str()) {
        if let Some(parent) = path.parent() {
            created_dir = !parent.exists();
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, source)?;
    }
    Ok(created_dir)
}

/// Remove the login mu-plugin (site delete / domain rename — its owner since
/// the 28 Jul 2026 step-6 ruling: the file lives while rexenv MANAGES the
/// site, is rewritten by every `issue`, and goes away when the site leaves
/// rexenv or changes domain. Same every-layout sweep as the tunnel file — a
/// pre-v24 stray in a Bedrock repo's dead `wp-content/` still gets cleaned).
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

/// Issue a one-time login for `user_id`: ensure the mu-plugin, generate a token,
/// store its hash + expiry in the `rexenv_login` option (via WP-CLI), and return
/// the raw token plus whether the mu-plugins dir was created by this call (the
/// caller records it — see [`ensure_muplugin`]). The caller builds the magic
/// URL (`https://<domain>/?rexenv_login=<token>&rexenv_user=<id>`).
pub fn issue(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    content_rel: &str,
    domain: &str,
    user_id: u64,
    ttl_secs: u64,
) -> Result<(String, bool)> {
    let created_dir = ensure_muplugin(docroot, content_rel, domain)?;

    // ~244-bit token (two v4 UUIDs); store only its hash. NOT 256: a v4 UUID
    // carries 122 random bits, not 128 — 4 are the version nibble and 2 the
    // variant. The margin is enormous either way; the number is corrected
    // because a security comment that rounds in the FLATTERING direction is the
    // kind a later reader trusts instead of re-deriving.
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
    Ok((token, created_dir))
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
    fn mu_plugin_path_follows_the_recorded_content_dir() {
        let p = mu_plugin_path(Path::new("/srv/site"), "wp-content");
        assert!(p.ends_with("wp-content/mu-plugins/rexenv-login.php"));
        // Bedrock: content lives at docroot/app — a hardcoded wp-content
        // would litter the repo AND never load (the silent-broken class).
        let p = mu_plugin_path(Path::new("/srv/bedrock/web"), "app");
        assert!(p.ends_with("web/app/mu-plugins/rexenv-login.php"));
    }

    #[test]
    fn injection_point_refuses_what_could_escape_the_php_string() {
        // Parity with wp_tunnel::validate_origin_rejects_php_string_escapes:
        // the guarantee used to be INHERITED from validate_domain with nothing
        // at the injection point — a refactor away from a PHP injection into
        // every site's mu-plugins. ensure_muplugin now refuses before any
        // replace or write.
        let dir = std::env::temp_dir().join("rexenv-wplogin-inject-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for bad in [
            "a'b.test",                 // closes the quote
            "a\\.test",                 // backslash games
            "x.test'.phpinfo().'",      // the classic
            "{$x}.test",                // interpolation shapes
            "a b.test",                 // whitespace
            "ACME.test",                // uppercase is never emitted by the vet
            "",
        ] {
            assert!(
                ensure_muplugin(&dir, "wp-content", bad).is_err(),
                "accepted into a single-quoted PHP string: {bad:?}"
            );
        }
        assert!(ensure_muplugin(&dir, "wp-content", "acme.rex").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mu_plugin_source_keeps_the_guard_tripwires() {
        // What THIS level can prove: the guard clauses are present in the
        // source. Enforcement — magic link works, replay denied, tunnel-header
        // request denied — is proven by `examples/wp_login_check.rs` against a
        // real WP over TLS; run it after editing the plugin source. (The old
        // needle list also carried ".test" and "'exp'", which match anywhere
        // in the file and guarded nothing.)
        let src = mu_plugin_source("acme.rex");
        for needle in [
            "HTTP_CF_CONNECTING_IP",
            "HTTP_X_FORWARDED_FOR",
            "delete_option('rexenv_login')",
            "hash_equals",
            "wp_set_auth_cookie",
        ] {
            assert!(src.contains(needle), "mu-plugin missing guard tripwire: {needle}");
        }
        // The site's own domain is baked into the host allow-list (custom TLDs),
        // and no unexpanded placeholder survives.
        assert!(src.contains("$site   = 'acme.rex';"), "site domain injected");
        assert!(!src.contains("{{SITE_DOMAIN}}"));
    }

    #[test]
    fn created_flag_reports_dir_creation_and_remove_sweeps_every_layout() {
        let dir = std::env::temp_dir().join("rexenv-wplogin-remove-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // First write creates mu-plugins/ → the flag the caller records (v25).
        assert!(ensure_muplugin(&dir, "wp-content", "a.test").unwrap());
        // Idempotent re-ensure creates nothing.
        assert!(!ensure_muplugin(&dir, "wp-content", "a.test").unwrap());

        // A stray in another layout (the pre-v24 bug's droppings) is swept too.
        ensure_muplugin(&dir, "app", "a.test").unwrap();
        remove(&dir).unwrap();
        for layout in crate::core::sites::CONTENT_DIR_LAYOUTS {
            assert!(!mu_plugin_path(&dir, layout).exists(), "left behind in {layout}");
        }
        remove(&dir).unwrap(); // idempotent on missing files
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_muplugin_writes_then_is_idempotent_and_tracks_domain_changes() {
        let dir = std::env::temp_dir().join("rexenv-wplogin-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        ensure_muplugin(&dir, "wp-content", "acme.test").unwrap();
        let p = mu_plugin_path(&dir, "wp-content");
        assert!(p.is_file());
        ensure_muplugin(&dir, "wp-content", "acme.test").unwrap(); // no rewrite when unchanged
        assert_eq!(std::fs::read_to_string(&p).unwrap(), mu_plugin_source("acme.test"));
        // A domain change (Change domain → .rex) rewrites the allow-list.
        ensure_muplugin(&dir, "wp-content", "acme.rex").unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("'acme.rex'"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
