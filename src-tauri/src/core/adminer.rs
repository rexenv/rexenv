//! core::adminer — serve the bundled Adminer DB browser as an internal vhost (§5.2).
//!
//! Adminer is a single `adminer.php` (§5.1). We serve it through the shared stack
//! on a fixed internal host (`adminer.rexenv.test`) rooted at an isolated docroot,
//! behind the edge (TLS, local CA). It is deliberately NOT a `Site` in the DB —
//! so it can never be selected as a public tunnel origin (§9): tunnels are scoped
//! to a single site's Host, and this internal vhost isn't one.

use crate::core::binaries;
use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::PathBuf;

/// Internal host Adminer is served on (resolved by the embedded `*.test` DNS).
/// NEVER a public tunnel origin (§9) — it isn't a site.
pub const ADMINER_HOST: &str = "adminer.rexenv.test";

/// Query flag a rexenv deep-link sets to request a one-click scoped session (§11.4).
pub const AUTOLOGIN_FLAG: &str = "rexenv_auto";

/// The deep-link wrapper served as `index.php`. It customizes Adminer via the
/// `adminer_object()` hook (defined in the GLOBAL namespace — Adminer checks the
/// global function name) so a rexenv "Open database" link lands the user straight
/// in the site's DB instead of on a manual login screen:
///   - `login()` accepts a passwordless login, but ONLY for the loopback engines
///     rexenv manages (MySQL root / Postgres have no local password) — never a
///     remote host;
///   - `loginForm()` auto-submits Adminer's OWN rendered form (which already
///     carries the CSRF token + CSP nonce) when `?rexenv_auto` is set, guarded by
///     sessionStorage so a failed login can't loop;
///   - `headers()`/`csp()` replace Adminer's blanket `X-Frame-Options: deny` with
///     `frame-ancestors` scoped to the rexenv webview origins, so the in-app
///     Database Browser `<iframe>` renders while any OTHER site framing this
///     passwordless vhost (clickjacking from a page in the user's browser —
///     `adminer.rexenv.test` resolves locally for them too) stays blocked;
///   - a `header_register_callback` rewrites every `Set-Cookie` to
///     `SameSite=None; Secure; Partitioned`: inside the app the iframe is a
///     cross-site embed (webview origin ≠ `adminer.rexenv.test`), so Adminer's
///     default `SameSite=lax` session/key cookies are withheld from the login
///     POST and every login bounces back to the form. `Partitioned` (CHIPS)
///     keeps the embedded jar isolated per top-level site, and Adminer's own
///     CSRF token still guards every state-changing request.
///
/// SECURITY: Adminer here is an INTERNAL vhost (`adminer.rexenv.test` → 127.0.0.1)
/// behind the local edge and is NEVER a public tunnel origin (§9), so passwordless
/// loopback access stays confined to the local machine.
const WRAPPER_INDEX_PHP: &str = r#"<?php
// rexenv Adminer deep-link wrapper (§11.4) — generated; do not edit by hand.

// The Database Browser embeds this vhost in a cross-site <iframe> (webview
// origin != adminer.rexenv.test), where SameSite=lax cookies are withheld and
// the login POST loses its session. Rewrite every cookie to
// "SameSite=None; Secure; Partitioned" at flush time (covers the PHP session
// cookie and Adminer's own key/permanent cookies alike). CSRF stays covered by
// Adminer's per-session token; framing stays limited by frame-ancestors below.
header_register_callback(function () {
    $rewritten = array();
    foreach (headers_list() as $h) {
        if (stripos($h, 'Set-Cookie:') === 0) {
            $v = preg_replace('/;\s*SameSite=\w+/i', '', trim(substr($h, 11)));
            if (stripos($v, 'secure') === false) {
                $v .= '; Secure';
            }
            $rewritten[] = $v . '; SameSite=None; Partitioned';
        }
    }
    if ($rewritten) {
        header_remove('Set-Cookie');
        foreach ($rewritten as $v) {
            header('Set-Cookie: ' . $v, false);
        }
    }
});

function adminer_object() {
    if (!class_exists('RexenvAdminer')) {
        class RexenvAdminer extends \Adminer\Adminer {
            function login($login, $password) {
                // Passwordless login for rexenv's loopback engines only.
                return strpos(\Adminer\SERVER, '127.0.0.1') === 0
                    || strpos(\Adminer\SERVER, 'localhost') === 0;
            }
            function loginForm() {
                parent::loginForm();
                if (isset($_GET['rexenv_auto'])) {
                    echo "<script" . \Adminer\nonce() . ">"
                       . "if(!sessionStorage.getItem('rexenv_autologin')){"
                       . "sessionStorage.setItem('rexenv_autologin','1');"
                       . "var f=document.querySelector('[name=\"auth[driver]\"]');"
                       . "if(f&&f.form){f.form.submit();}}"
                       . "</script>";
                }
            }
            function headers() {
                // Adminer hardcodes "X-Frame-Options: deny", which blanks the
                // rexenv Database Browser <iframe>. Drop it; the csp() hook below
                // re-adds frame protection scoped to the app's webview origins.
                header_remove('X-Frame-Options');
            }
            function csp(array $csp) {
                // Embeddable ONLY by the rexenv app webview (prod macOS/Linux,
                // prod Windows, Vite dev) — every other ancestor stays blocked.
                $csp[0]['frame-ancestors'] =
                    'tauri://localhost https://tauri.localhost http://localhost:1420';
                return $csp;
            }
        }
    }
    return new \RexenvAdminer();
}
require __DIR__ . '/adminer.php';
"#;

/// Web docroot for Adminer, isolated from the binary cache.
pub fn docroot(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("adminer"))
}

/// Ensure the Adminer docroot exists: the bundled Adminer as `adminer.php` plus the
/// rexenv deep-link wrapper as `index.php` (the served entrypoint). Downloads +
/// caches `adminer.php` (§5.1) on first use. Idempotent: the Adminer copy is
/// refreshed only when missing / a different size, and the wrapper only when its
/// content differs (so a wrapper update redeploys on next start).
pub async fn ensure(platform: &dyn Platform) -> Result<PathBuf> {
    let dir = docroot(platform)?;
    std::fs::create_dir_all(&dir)?;

    // The real Adminer, copied beside the wrapper (the wrapper `require`s it).
    let adminer_php = dir.join("adminer.php");
    let src = binaries::resolve_file(platform, "adminer", binaries::ADMINER_VERSION).await?;
    let stale = std::fs::metadata(&adminer_php).ok().map(|m| m.len())
        != std::fs::metadata(&src).ok().map(|m| m.len());
    if stale {
        std::fs::copy(&src, &adminer_php)?;
    }

    // The served entrypoint: our deep-link wrapper (rewrite only if changed).
    let index = dir.join("index.php");
    if std::fs::read_to_string(&index).ok().as_deref() != Some(WRAPPER_INDEX_PHP) {
        std::fs::write(&index, WRAPPER_INDEX_PHP)?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_an_internal_subdomain() {
        assert!(ADMINER_HOST.ends_with(".rexenv.test"));
        // Not a wildcard / not a user site domain.
        assert_eq!(ADMINER_HOST, "adminer.rexenv.test");
    }

    #[test]
    fn wrapper_defines_the_deeplink_hook_and_loopback_gate() {
        // Global-namespace hook (Adminer checks the global function name).
        assert!(WRAPPER_INDEX_PHP.contains("function adminer_object()"));
        assert!(WRAPPER_INDEX_PHP.contains("extends \\Adminer\\Adminer"));
        // Passwordless login is gated to loopback only (never a remote host).
        assert!(WRAPPER_INDEX_PHP.contains("function login("));
        assert!(WRAPPER_INDEX_PHP.contains("127.0.0.1"));
        // Auto-submit is keyed on the deep-link flag + carries the CSP nonce.
        assert!(WRAPPER_INDEX_PHP.contains(AUTOLOGIN_FLAG));
        assert!(WRAPPER_INDEX_PHP.contains("\\Adminer\\nonce()"));
        // It serves the real Adminer beside it.
        assert!(WRAPPER_INDEX_PHP.contains("require __DIR__ . '/adminer.php'"));
    }

    #[test]
    fn wrapper_allows_framing_only_from_the_app_webview() {
        // Adminer's blanket deny is dropped (it blanks the in-app iframe)…
        assert!(WRAPPER_INDEX_PHP.contains("header_remove('X-Frame-Options')"));
        // …and replaced by frame-ancestors scoped to rexenv webview origins.
        assert!(WRAPPER_INDEX_PHP.contains("function csp("));
        assert!(WRAPPER_INDEX_PHP.contains("'frame-ancestors'"));
        assert!(WRAPPER_INDEX_PHP.contains("tauri://localhost"));
        assert!(WRAPPER_INDEX_PHP.contains("http://localhost:1420"));
        // No wildcard — never embeddable by arbitrary origins.
        assert!(!WRAPPER_INDEX_PHP.contains("frame-ancestors *"));
    }

    #[test]
    fn wrapper_rewrites_cookies_for_the_cross_site_iframe() {
        // SameSite=lax cookies are withheld from the embedded iframe's login
        // POST; the wrapper rewrites them to None+Secure+Partitioned at flush.
        assert!(WRAPPER_INDEX_PHP.contains("header_register_callback"));
        assert!(WRAPPER_INDEX_PHP.contains("SameSite=None; Partitioned"));
        assert!(WRAPPER_INDEX_PHP.contains("Secure"));
    }
}
