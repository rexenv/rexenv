//! core::wp_tunnel — full-site URL rewriting while a site is shared (§9).
//!
//! A quick tunnel proxies `https://<random>.trycloudflare.com` to the shared nginx
//! with the site's LOCAL `Host` (`--http-host-header`), so WordPress keeps
//! generating `https://<domain>.test` URLs — navigable on the dev machine only.
//! While a tunnel runs, rexenv drops an auto-managed **mu-plugin** with the public
//! origin baked in (the quick-tunnel URL is random per start and invisible to PHP,
//! so it can't be read from the request). For requests that arrived THROUGH the
//! tunnel — marked by the Cloudflare header set, the same set the rexenv login
//! mu-plugin denies on — it:
//!   - overrides `HTTP_HOST`/`HTTPS` so `redirect_canonical` and cookies agree
//!     with the address in the visitor's browser (no redirect loop);
//!   - filters `option_siteurl`/`option_home` (permalinks, wp-admin, REST,
//!     `Location:` headers) and `content_url`/`plugins_url`/`upload_dir` (their
//!     constants were baked from the LOCAL siteurl before mu-plugins load);
//!   - rewrites leftover local URLs in the final output — plain, JSON-escaped
//!     (`https:\/\/…` in REST/inline settings), and %-encoded — via `ob_start`.
//!
//! Local requests carry no Cloudflare headers and are untouched, so the site
//! stays fully usable at its `.test` domain while shared.
//!
//! File lifetime (ruling 28 Jul 2026): **bounded by the tunnel's lifetime plus
//! at most one app relaunch.** Removed on tunnel stop and at app quit — tunnels
//! die with the app (`commands::tunnels::kill_all_on_exit`). After a CRASH the
//! tunnel can survive until the next launch, and while it does this file is
//! live and doing its job for a genuinely live tunnel; the launch sweep
//! (`tunnels::sweep_startup`) then kills the tunnel and removes the file
//! together. Do NOT weaken this into "leftover copies are harmless": a copy
//! next to a live tunnel is ACTIVE by design, and only the sweep — after the
//! kill — may treat one as dead.
//!
//! Scope: the tunnel is bound to ONE Host, so a subdomain-multisite network shares
//! its main site only (sub-sites have their own hosts); subdirectory multisite
//! works whole-network. WP-CLI runs are untouched (`WP_CLI` guard).

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Placeholder the public origin is baked into.
const ORIGIN_PLACEHOLDER: &str = "__REXENV_TUNNEL_ORIGIN__";

/// The auto-managed mu-plugin. `preg_replace_callback` is used over
/// `preg_replace` so replacement text is emitted verbatim (no `\`/`$n`
/// escape processing on origins containing `\/`).
///
/// `pub(crate)` so `wp_login`'s ordering guard can see BOTH halves of the
/// include-time/`init` pairing (CLAIM-LEDGER #308) — a guard that could only see
/// one half would pass while the other side was refactored out from under it.
pub(crate) const MU_PLUGIN_TEMPLATE: &str = r#"<?php
/* Plugin Name: rexenv tunnel URLs
 * Description: Auto-managed by rexenv while this site is shared over a public tunnel. Safe to delete.
 */
// This body runs at INCLUDE time rather than on a hook, and that is load-bearing
// rather than style. WordPress includes every mu-plugin (wp-settings.php:498)
// before it fires muplugins_loaded (540) or init (771), so the HTTP_HOST rewrite
// below is already in place when rexenv-login.php's init callback reads the Host.
// The pairing does NOT rest on filename sort. Two tidying refactors break it:
// moving this body onto a hook, or moving the login check to include time.
// Observed 14 Aug 2026 — with the CF-header gate removed, the login gate denied
// on Host, and only while this file was present. See CLAIM-LEDGER #308.
call_user_func(static function () {
    $origin = '__REXENV_TUNNEL_ORIGIN__';
    if ($origin === '' || defined('WP_CLI')) {
        return;
    }
    // Only requests that came THROUGH the tunnel: cloudflared rewrites Host back
    // to the local domain, so the marker is the Cloudflare header set (the same
    // set the rexenv login mu-plugin denies on — keep the two complementary).
    if (empty($_SERVER['HTTP_CF_RAY']) && empty($_SERVER['HTTP_CF_CONNECTING_IP'])) {
        return;
    }
    $public_host = strtolower((string) parse_url($origin, PHP_URL_HOST));
    if ($public_host === '') {
        return;
    }
    $local_host = strtolower(explode(':', (string) ($_SERVER['HTTP_HOST'] ?? ''))[0]);

    // Make WP see the public address: redirect_canonical would otherwise bounce
    // every request back to the locally-generated canonical URL forever, and auth
    // cookies must be scoped to the host in the visitor's browser.
    $_SERVER['HTTP_HOST']      = $public_host;
    $_SERVER['SERVER_NAME']    = $public_host;
    $_SERVER['HTTPS']          = 'on';
    $_SERVER['SERVER_PORT']    = '443';
    $_SERVER['REQUEST_SCHEME'] = 'https';

    // Programmatic URLs: home/siteurl are the root nearly everything derives from
    // (permalinks, admin_url, rest_url, wp_safe_redirect's allowed hosts).
    // The blog's PATH is part of the value in a subdirectory multisite network
    // (a sub-site's siteurl is https://<local>/sub1): swap the ORIGIN, keep the
    // path. Returning the bare origin cost exactly that path — observed
    // 26 Aug 2026 on a subdirectory network over a quick tunnel: the sub-site's
    // front page 404'd, and logging in at /sub1/wp-login.php landed on the MAIN
    // site's dashboard because admin_url() had lost the /sub1.
    $to_origin = static function ($url) use ($origin) {
        if (!is_string($url) || $url === '') {
            return $origin;
        }
        return $origin . rtrim((string) parse_url($url, PHP_URL_PATH), '/');
    };
    add_filter('option_siteurl', $to_origin, 1000);
    add_filter('option_home', $to_origin, 1000);

    if ($local_host === '' || $local_host === $public_host) {
        return;
    }
    // Host is anchored by the scheme prefix and bounded on the right so a
    // lookalike domain ("mysite.tester.com") is never rewritten.
    $host_re  = preg_quote($local_host, '~');
    $boundary = '(?![A-Za-z0-9.-])';

    // WP_CONTENT_URL / WP_PLUGIN_URL were baked from the LOCAL siteurl before
    // mu-plugins load — swap the origin wherever they resurface.
    $swap = static function ($url) use ($origin, $host_re, $boundary) {
        return is_string($url)
            ? preg_replace_callback(
                '~https?://' . $host_re . $boundary . '~i',
                static fn () => $origin,
                $url
            )
            : $url;
    };
    add_filter('content_url', $swap, 1000);
    add_filter('plugins_url', $swap, 1000);
    add_filter('upload_dir', static function ($dirs) use ($swap) {
        foreach (['url', 'baseurl'] as $k) {
            if (isset($dirs[$k])) {
                $dirs[$k] = $swap($dirs[$k]);
            }
        }
        return $dirs;
    }, 1000);

    // Everything else — URLs stored in post content (media, links), srcset,
    // JSON-escaped URLs in REST responses and inline script settings, %-encoded
    // redirect params — is rewritten in the final output. Location headers are
    // not part of the buffer; the option filters above cover redirects.
    ob_start(static function ($out) use ($origin, $host_re, $boundary) {
        $pairs = [
            ['~https?://' . $host_re . $boundary . '~i', $origin],
            ['~https?:\\\\/\\\\/' . $host_re . $boundary . '~i', str_replace('/', '\/', $origin)],
            ['~https?%3A%2F%2F' . $host_re . $boundary . '~i', str_replace([':', '/'], ['%3A', '%2F'], $origin)],
        ];
        foreach ($pairs as [$re, $rep]) {
            $out = preg_replace_callback($re, static fn () => $rep, $out);
        }
        return $out;
    });
});
"#;

use crate::core::sites::CONTENT_DIR_LAYOUTS;

/// Path of the auto-managed mu-plugin within a docroot. `content_rel` is the
/// site's RECORDED content dir (`Site::content_dir_rel`, v24) — never derived
/// here at write time.
fn mu_plugin_path(docroot: &Path, content_rel: &str) -> PathBuf {
    docroot.join(content_rel).join("mu-plugins").join("rexenv-tunnel.php")
}

/// The public origin is baked into single-quoted PHP source — reject anything
/// beyond `https://` + a plain host so nothing can escape the string or smuggle
/// PHP (defense-in-depth; `tunnels::extract_url` already constrains the shape).
fn validate_origin(origin: &str) -> Result<()> {
    let host = origin
        .strip_prefix("https://")
        .ok_or_else(|| Error::Other(format!("tunnel origin must be https: {origin}")))?;
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return Err(Error::Other(format!(
            "tunnel origin host may contain only a–z, 0–9, '.', '-': {origin}"
        )));
    }
    Ok(())
}

/// Render the mu-plugin with the public origin baked in.
fn render(origin: &str) -> String {
    MU_PLUGIN_TEMPLATE.replace(ORIGIN_PLACEHOLDER, origin)
}

/// Write (or refresh) the mu-plugin with this tunnel's public origin. Idempotent:
/// the quick-tunnel URL changes on every start, so the file is compared and only
/// rewritten when its content differs.
pub fn enable(docroot: &Path, content_rel: &str, origin: &str) -> Result<bool> {
    validate_origin(origin)?;
    let rendered = render(origin.trim_end_matches('/'));
    let path = mu_plugin_path(docroot, content_rel);
    let mut created_dir = false;
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&rendered) {
        if let Some(parent) = path.parent() {
            // Returned so the caller can RECORD dir ownership (v25) — site
            // teardown removes a dir we created, never one inferred from
            // emptiness.
            created_dir = !parent.exists();
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, rendered)?;
    }
    Ok(created_dir)
}

/// Remove the mu-plugin (tunnel stopped). Missing file is fine. Sweeps EVERY
/// known layout rather than taking a content-dir argument: removal callers
/// (stop, dead-child settle, exit hook, launch sweep) may hold only a
/// recorded docroot, and the file names are exactly ours — removing them from
/// a layout dir that never had them changes nothing.
pub fn disable(docroot: &Path) -> Result<()> {
    for layout in CONTENT_DIR_LAYOUTS {
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

    const ORIGIN: &str = "https://blue-cat-runs-fast.trycloudflare.com";

    #[test]
    fn render_bakes_origin_and_the_guard_tripwires() {
        // What THIS level can prove: substitution happened and the guard
        // clauses didn't vanish from the source. The behavioral proof — the
        // guards firing under real PHP in all three modes — is
        // `examples/tunnel_muplugin_check.rs`; run it after editing PLUGIN_SRC.
        let php = render(ORIGIN);
        assert!(php.contains(&format!("$origin = '{ORIGIN}';")));
        assert!(!php.contains(ORIGIN_PLACEHOLDER));
        // Tripwires only (presence, not behavior): the tunnel-only
        // discriminator and the lookalike-domain boundary.
        for needle in ["HTTP_CF_RAY", "HTTP_CF_CONNECTING_IP", "(?![A-Za-z0-9.-])"] {
            assert!(php.contains(needle), "mu-plugin missing guard tripwire: {needle}");
        }
    }

    #[test]
    fn validate_origin_rejects_php_string_escapes() {
        assert!(validate_origin(ORIGIN).is_ok());
        for bad in [
            "http://x.trycloudflare.com",       // not https
            "https://",                          // empty host
            "https://x.test/'.phpinfo().'",      // quote escape
            "https://x\\.test",                  // backslash
            "https://X.test",                    // uppercase (never emitted)
            "https://x.test/path",               // '/' beyond the host
        ] {
            assert!(validate_origin(bad).is_err(), "accepted: {bad}");
        }
    }

    #[test]
    fn enable_writes_updates_and_disable_removes() {
        let dir = std::env::temp_dir().join("rexenv-wptunnel-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        enable(&dir, "wp-content", ORIGIN).unwrap();
        let p = mu_plugin_path(&dir, "wp-content");
        assert!(p.is_file());
        assert!(std::fs::read_to_string(&p).unwrap().contains(ORIGIN));

        // Next start gets a NEW random URL → file must be refreshed in place.
        enable(&dir, "wp-content", "https://other-name.trycloudflare.com").unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("other-name") && !s.contains("blue-cat"));

        disable(&dir).unwrap();
        assert!(!p.exists());
        disable(&dir).unwrap(); // idempotent on a missing file
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bedrock_layout_writes_where_wp_loads_and_disable_sweeps_every_layout() {
        let dir = std::env::temp_dir().join("rexenv-wptunnel-bedrock-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // The recorded Bedrock rel writes under app/, never wp-content/.
        enable(&dir, "app", ORIGIN).unwrap();
        assert!(mu_plugin_path(&dir, "app").is_file());
        assert!(!mu_plugin_path(&dir, "wp-content").exists());

        // disable takes no layout — it must clean EVERY known one, including a
        // pre-v24 stray our own bug wrote into a dead wp-content/.
        enable(&dir, "wp-content", ORIGIN).unwrap(); // the historical stray
        disable(&dir).unwrap();
        for layout in CONTENT_DIR_LAYOUTS {
            assert!(!mu_plugin_path(&dir, layout).exists(), "left behind in {layout}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
