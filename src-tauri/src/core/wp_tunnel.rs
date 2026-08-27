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
    //
    // Everything below keys off the NETWORK domain, never the request's Host. A
    // sub-site request arrives with its own Host — sunrise.php remapped it — and
    // the question each rewrite answers is "where does this host live under the
    // public origin", which only the network domain can answer.
    $net = defined('DOMAIN_CURRENT_SITE') ? strtolower(DOMAIN_CURRENT_SITE) : $local_host;
    $subdomain_network = defined('SUBDOMAIN_INSTALL') && SUBDOMAIN_INSTALL;
    if ($net === '') {
        return;
    }
    // Host is anchored by the scheme prefix and bounded on the right so a
    // lookalike domain ("mysite.tester.com") is never rewritten.
    $net_re   = preg_quote($net, '~');
    $boundary = '(?![A-Za-z0-9.-])';

    // ONE rewrite, used for the option filters, the *_url filters and the
    // output buffer, because three copies of this mapping is how they drift.
    // Two shapes can be served through one tunnel:
    //   <network>            -> <origin>
    //   <label>.<network>    -> <origin>/<label>   (subdomain network only)
    // The second is the whole trick: a tunnel pins ONE Host, so a subdomain
    // network's sub-sites are served as SUBDIRECTORIES while shared —
    // sunrise.php does the request half, this does the URL half. Anything else
    // is left alone: rewriting a host we cannot serve produces a link that
    // lands on the wrong site, which is worse than a link that visibly fails
    // (27 Aug 2026 — "MSD" and "Sub Domain One" both read <origin>/ on My Sites).
    // Paths survive because only the scheme+host prefix is replaced, which is
    // also what keeps a subdirectory network's /sub1 and any ?ver= query intact.
    $styles = [
        ['~https?://', $origin, '/'],
        ['~https?:\\\\/\\\\/', str_replace('/', '\\/', $origin), '\\/'],
        ['~https?%3A%2F%2F', str_replace([':', '/'], ['%3A', '%2F'], $origin), '%2F'],
    ];
    $rewrite = static function ($text) use ($styles, $net_re, $boundary, $subdomain_network) {
        foreach ($styles as [$scheme, $rep, $sep]) {
            if ($subdomain_network) {
                $text = preg_replace_callback(
                    $scheme . '([A-Za-z0-9-]+)\.' . $net_re . $boundary . '~i',
                    static fn ($m) => $rep . $sep . strtolower($m[1]),
                    $text
                );
            }
            $text = preg_replace_callback(
                $scheme . $net_re . $boundary . '~i',
                static fn () => $rep,
                $text
            );
        }
        return $text;
    };

    $filter_url = static function ($url) use ($rewrite, $origin) {
        if (!is_string($url) || $url === '') {
            return $origin;
        }
        return $rewrite($url);
    };
    add_filter('option_siteurl', $filter_url, 1000);
    add_filter('option_home', $filter_url, 1000);

    // WP_CONTENT_URL / WP_PLUGIN_URL were baked from the LOCAL siteurl before
    // mu-plugins load — swap the origin wherever they resurface.
    $swap = static function ($url) use ($rewrite) {
        return is_string($url) ? $rewrite($url) : $url;
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
    ob_start(static function ($out) use ($rewrite) {
        return $rewrite($out);
    });
});
"#;

use crate::core::sites::CONTENT_DIR_LAYOUTS;

/// The sunrise drop-in: the REQUEST half of serving a subdomain network's
/// sub-sites as subdirectories while shared. `wp-settings.php` requires
/// `ms-settings.php` (line 161) after `require_wp_db()` (136), and
/// `ms-settings.php` includes this file (52) BEFORE it reads `HTTP_HOST` /
/// `REQUEST_URI` to resolve the blog (62–77). That ordering is the whole reason
/// this file exists: it is the last place a rewrite still counts, and `$wpdb` is
/// already live so the blog list can be asked rather than guessed.
///
/// Written and removed with the tunnel, like the URL mu-plugin.
const SUNRISE_TEMPLATE: &str = r#"<?php
/* rexenv — auto-managed while this site is shared over a public tunnel. Safe to delete. */
// A quick tunnel gets ONE hostname and pins ONE Host, so a SUBDOMAIN network's
// sub-sites (s1.<network>) are unreachable through it — measured 27 Aug 2026, an
// outside browser gets ERR_NAME_NOT_RESOLVED, and there is no wildcard to ask for.
// While shared, they are served as SUBDIRECTORIES instead: /s1/... is turned back
// into a request for s1.<network>. The URL half lives in rexenv-tunnel.php; the two
// are one feature and must move together.
call_user_func(static function () {
    // Only through the tunnel. Locally the network stays a real subdomain network,
    // which is the point of developing on one.
    if (empty($_SERVER['HTTP_CF_RAY']) && empty($_SERVER['HTTP_CF_CONNECTING_IP'])) {
        return;
    }
    if (!defined('SUBDOMAIN_INSTALL') || !SUBDOMAIN_INSTALL || !defined('DOMAIN_CURRENT_SITE')) {
        return;
    }
    $net = strtolower((string) DOMAIN_CURRENT_SITE);
    $uri = (string) ($_SERVER['REQUEST_URI'] ?? '/');
    $path = (string) (parse_url($uri, PHP_URL_PATH) ?? '/');
    if (!preg_match('~^/([A-Za-z0-9][A-Za-z0-9-]*)(/|$)~', $path, $m)) {
        return;
    }
    $label = strtolower($m[1]);
    // WP's own reserved names, plus the two directories every request for an
    // asset starts with. A network cannot have a site called these, so a lookup
    // would miss anyway — this just skips a query on the hot path for assets.
    if (in_array($label, ['wp-admin', 'wp-content', 'wp-includes', 'wp-json'], true)) {
        return;
    }
    global $wpdb;
    if (!isset($wpdb) || empty($wpdb->blogs)) {
        return;
    }
    $domain = $label . '.' . $net;
    $blog_id = $wpdb->get_var(
        $wpdb->prepare("SELECT blog_id FROM {$wpdb->blogs} WHERE domain = %s AND path = %s LIMIT 1", $domain, '/')
    );
    if (!$blog_id) {
        // Not a sub-site: leave the request alone so the MAIN site can serve
        // /whatever itself. A page on the main site only loses to a sub-site
        // that genuinely owns that label.
        return;
    }
    // ONLY the Host. REQUEST_URI keeps its /s1 prefix, deliberately:
    //   - ms_load_current_site_and_network() resolves by DOMAIN here (the network
    //     domain is defined in wp-config), and get_site_by_path() falls back to
    //     '/' after trying '/s1/', so the prefix costs it nothing;
    //   - WP::parse_request() strips home_url()'s path itself, and home_url() is
    //     already <origin>/s1 thanks to the URL rewriter, so routing lands right;
    //   - anything WP builds from REQUEST_URI keeps working. Stripping it here
    //     cost exactly that (27 Aug 2026): an unauthenticated hit on
    //     /s1/wp-admin/ bounced to the sub-site's login with
    //     redirect_to=<origin>/wp-admin/ — the MAIN site's dashboard.
    $_SERVER['HTTP_HOST']   = $domain;
    $_SERVER['SERVER_NAME'] = $domain;
});
"#;

/// Marker pair around the wp-config block below. The BEGIN line is also how a
/// second write recognises its own work — the block is written once and never
/// rewritten.
const COOKIE_SCOPE_BEGIN: &str = "// BEGIN rexenv: tunnel cookie scope";
const COOKIE_SCOPE_END: &str = "// END rexenv: tunnel cookie scope";

/// The wp-config block. `__REXENV_MU_REL__` is the mu-plugin's path relative to
/// the wp-config file.
const COOKIE_SCOPE_TEMPLATE: &str = r#"// BEGIN rexenv: tunnel cookie scope — auto-added by rexenv, safe to keep.
// On a SUBDOMAIN network WP core pins COOKIE_DOMAIN to '.<network domain>' in
// ms_cookie_constants(), and that runs inside wp-settings.php — BEFORE mu-plugins
// load. So rexenv-tunnel.php structurally cannot reach it, and THIS FILE is the
// only seat there is. What it cost when nothing sat here (measured 27 Aug 2026,
// real quick tunnel): the shared network served pages fine logged-out, and could
// not be logged into AT ALL — every auth cookie came back `domain=.<network>`
// while the visitor was on <random>.trycloudflare.com, the browser dropped all of
// them, and wp-login answered "Cookies are blocked or not supported by your
// browser." An empty COOKIE_DOMAIN makes those cookies host-only: scoped to
// whatever host the visitor actually typed. A subdirectory network never needed
// this — there COOKIE_DOMAIN is simply never defined.
//
// TWO conditions, deliberately, and the second is the load-bearing one: the
// Cloudflare header set says the request came through a tunnel, and the
// mu-plugin's presence says the share is LIVE and is rexenv's (that file exists
// only while a tunnel runs). On the header alone, this block copied to a real
// Cloudflare-fronted production network would silently break cross-subdomain SSO
// there — the failure would look like "users get logged out on subsites", miles
// from anything mentioning rexenv.
if ( ( ! empty( $_SERVER['HTTP_CF_RAY'] ) || ! empty( $_SERVER['HTTP_CF_CONNECTING_IP'] ) )
	&& file_exists( __DIR__ . '/__REXENV_MU_REL__' ) ) {
	define( 'COOKIE_DOMAIN', '' );
}
// A tunnel gets ONE hostname and pins ONE Host, so this network's sub-sites are
// unreachable through it as subdomains — an outside browser gets
// ERR_NAME_NOT_RESOLVED, and there is no wildcard to ask for. While shared they
// are served as SUBDIRECTORIES instead, and the drop-in that does it must be
// declared HERE: ms-settings.php only looks for it when SUNRISE is defined, and
// by then wp-config has already run. Guarded on the file existing so this line
// is inert — and silent — whenever no share is live.
if ( file_exists( __DIR__ . '/__REXENV_SUNRISE_REL__' ) ) {
	define( 'SUNRISE', 'on' );
}
// END rexenv: tunnel cookie scope
"#;

/// Anchors we may insert the block before, best first. Every one of them sits
/// ahead of the `wp-settings.php` require, which is the only property that
/// matters: after that line the constant is too late to define.
const COOKIE_SCOPE_ANCHORS: [&str; 2] =
    ["/* That's all, stop editing!", "require_once ABSPATH . 'wp-settings.php'"];

/// Is this wp-config a SUBDOMAIN multisite? Read from the file WP-CLI wrote,
/// never inferred from the site record — `wp core multisite-convert` owns these
/// constants, and a record that drifted from them would send the block to a
/// network that does not need it (or worse, skip one that does).
fn is_subdomain_network(src: &str) -> bool {
    let squeezed: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed.contains("define('MULTISITE',true)")
        && squeezed.contains("define('SUBDOMAIN_INSTALL',true)")
}

/// Give a SUBDOMAIN network's auth cookies a chance to survive the tunnel.
///
/// Idempotent and **permanent**: written once at tunnel start, never removed.
/// Removal is the dangerous direction — a half-applied edit to wp-config takes
/// the whole site down, while a leftover block is inert (its second condition
/// is a file that exists only while a share is live). Non-subdomain sites and
/// files that already carry the block are left byte-identical.
///
/// Returns whether the file was written.
pub fn ensure_subdomain_cookie_scope(docroot: &Path, content_rel: &str) -> Result<bool> {
    let path = docroot.join("wp-config.php");
    let Ok(src) = std::fs::read_to_string(&path) else {
        // No wp-config (Bedrock keeps its config elsewhere, or this is not a
        // WP docroot at all): nothing this function can safely edit.
        return Ok(false);
    };
    if !is_subdomain_network(&src) {
        return Ok(false);
    }
    if !CONTENT_DIR_LAYOUTS.contains(&content_rel) {
        return Err(Error::Other(format!(
            "refusing to write a cookie-scope block for an unknown content dir: {content_rel}"
        )));
    }
    let mu_rel = format!("{content_rel}/mu-plugins/rexenv-tunnel.php");
    let block = COOKIE_SCOPE_TEMPLATE
        .replace("__REXENV_MU_REL__", &mu_rel)
        .replace("__REXENV_SUNRISE_REL__", &format!("{content_rel}/sunrise.php"));
    // The delimiters are the block's only bounds. Nothing removes it today, and
    // this check is why: whoever writes that remover will be deleting a range
    // out of the file that decides whether the site boots, so an unterminated
    // block must never reach the disk in the first place.
    if !block.contains(COOKIE_SCOPE_BEGIN) || !block.contains(COOKIE_SCOPE_END) {
        return Err(Error::Other("cookie-scope block lost a delimiter — refusing to write".into()));
    }

    // An EXISTING block is replaced in place rather than skipped. "Write once"
    // was right about not appending a second copy and wrong about never
    // touching it again: the block grew a second job (the sunrise declaration)
    // on 27 Aug 2026, and a site shared before that date would have kept the
    // older, half-working version forever. The delimiters are what make this
    // safe — the range replaced is exactly ours, and a file whose END marker is
    // missing is left alone rather than guessed at.
    if let Some(begin) = src.find(COOKIE_SCOPE_BEGIN) {
        let Some(end) = src[begin..].find(COOKIE_SCOPE_END).map(|i| begin + i) else {
            return Err(Error::Other(
                "wp-config.php has a rexenv cookie-scope block with no END marker — \
                 refusing to guess where it stops; remove it by hand and share again"
                    .into(),
            ));
        };
        let end = src[end..].find('\n').map_or(src.len(), |i| end + i + 1);
        if src[begin..end] == *block.as_str() {
            return Ok(false);
        }
        let mut out = String::with_capacity(src.len() + block.len());
        out.push_str(&src[..begin]);
        out.push_str(&block);
        out.push_str(&src[end..]);
        std::fs::write(&path, out)?;
        return Ok(true);
    }

    let at = COOKIE_SCOPE_ANCHORS.iter().find_map(|a| src.find(a)).ok_or_else(|| {
        // Loud, not silent: without the block a subdomain share cannot be
        // logged into, so "shared anyway" would be the worse outcome.
        Error::Other(format!(
            "wp-config.php has no anchor to insert the tunnel cookie-scope block before \
             (looked for {:?}); a subdomain network cannot be logged into through a tunnel \
             without it — add the block by hand or share a subdirectory network instead",
            COOKIE_SCOPE_ANCHORS
        ))
    })?;
    let mut out = String::with_capacity(src.len() + block.len() + 1);
    out.push_str(&src[..at]);
    out.push_str(&block);
    out.push('\n');
    out.push_str(&src[at..]);
    std::fs::write(&path, out)?;
    Ok(true)
}

/// Path of the auto-managed mu-plugin within a docroot. `content_rel` is the
/// site's RECORDED content dir (`Site::content_dir_rel`, v24) — never derived
/// here at write time.
fn mu_plugin_path(docroot: &Path, content_rel: &str) -> PathBuf {
    docroot.join(content_rel).join("mu-plugins").join("rexenv-tunnel.php")
}

/// The sunrise drop-in lives at the content dir's ROOT — `ms-settings.php`
/// includes exactly `WP_CONTENT_DIR . '/sunrise.php'` and nowhere else.
fn sunrise_path(docroot: &Path, content_rel: &str) -> PathBuf {
    docroot.join(content_rel).join("sunrise.php")
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
    // The sunrise drop-in ships with the mu-plugin because they are two halves
    // of ONE behaviour (request rewrite + URL rewrite). Splitting their
    // lifetimes would give a share that resolves sub-sites but links to hosts
    // nobody can reach, or the reverse — both worse than neither.
    let sunrise = sunrise_path(docroot, content_rel);
    if std::fs::read_to_string(&sunrise).ok().as_deref() != Some(SUNRISE_TEMPLATE) {
        if let Some(parent) = sunrise.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&sunrise, SUNRISE_TEMPLATE)?;
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
        for path in [mu_plugin_path(docroot, layout), sunrise_path(docroot, layout)] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
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

    const SUBDOMAIN_CONFIG: &str = "<?php\ndefine( 'MULTISITE', true );\n\
define( 'SUBDOMAIN_INSTALL', true );\n\
/* That's all, stop editing! */\nrequire_once ABSPATH . 'wp-settings.php';\n";

    /// Throwaway dir, named per test + pid so parallel tests never collide.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-cookiescope-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_config(dir: &std::path::Path, src: &str) -> PathBuf {
        let p = dir.join("wp-config.php");
        std::fs::write(&p, src).unwrap();
        p
    }

    #[test]
    fn cookie_scope_lands_before_wp_settings_and_only_once() {
        let dir = scratch("once");
        let cfg = write_config(&dir, SUBDOMAIN_CONFIG);

        assert!(ensure_subdomain_cookie_scope(&dir, "wp-content").unwrap());
        let after = std::fs::read_to_string(&cfg).unwrap();
        // The ONE property that matters: defined before wp-settings runs.
        let block = after.find(COOKIE_SCOPE_BEGIN).expect("block written");
        let end = after.find(COOKIE_SCOPE_END).expect("block closed");
        // The REQUIRE, not the first mention — the block's own comment names
        // wp-settings.php, and matching that would pass no matter where the
        // block landed.
        let settings =
            after.find("require_once ABSPATH . 'wp-settings.php'").expect("require kept");
        assert!(block < end && end < settings, "block must close before wp-settings:\n{after}");
        assert!(after.contains("wp-content/mu-plugins/rexenv-tunnel.php"));

        // Second call is a no-op — the file stays byte-identical.
        assert!(!ensure_subdomain_cookie_scope(&dir, "wp-content").unwrap());
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), after);
        // Both jobs are declared: the cookie scope and the sunrise drop-in.
        assert!(after.contains("'COOKIE_DOMAIN', ''"));
        assert!(after.contains("wp-content/sunrise.php"));
    }

    #[test]
    fn an_older_block_is_upgraded_in_place_not_duplicated() {
        let dir = scratch("upgrade");
        // A site shared before the block grew its second job. Written-once
        // would have left this one half-working forever.
        let stale = SUBDOMAIN_CONFIG.replace(
            "/* That's all, stop editing! */",
            "// BEGIN rexenv: tunnel cookie scope\n             define( 'COOKIE_DOMAIN', '' );\n             // END rexenv: tunnel cookie scope\n             /* That's all, stop editing! */",
        );
        let cfg = write_config(&dir, &stale);
        assert!(ensure_subdomain_cookie_scope(&dir, "wp-content").unwrap(), "upgraded");
        let after = std::fs::read_to_string(&cfg).unwrap();
        assert_eq!(after.matches(COOKIE_SCOPE_BEGIN).count(), 1, "one block, not two:\n{after}");
        assert!(after.contains("wp-content/sunrise.php"), "gained the sunrise line:\n{after}");
        assert!(
            after.find(COOKIE_SCOPE_END).unwrap()
                < after.find("require_once ABSPATH . 'wp-settings.php'").unwrap(),
            "still ahead of wp-settings:\n{after}"
        );

        // A block whose END marker was hand-deleted is never guessed at.
        write_config(&dir, &stale.replace("// END rexenv: tunnel cookie scope\n", ""));
        assert!(ensure_subdomain_cookie_scope(&dir, "wp-content").is_err());
    }

    #[test]
    fn cookie_scope_skips_every_config_that_does_not_need_it() {
        let dir = scratch("skip");
        // Subdirectory network: COOKIE_DOMAIN is never defined there, so the
        // block would be noise pretending to be a fix.
        let subdir =
            SUBDOMAIN_CONFIG.replace("'SUBDOMAIN_INSTALL', true", "'SUBDOMAIN_INSTALL', false");
        let cfg = write_config(&dir, &subdir);
        assert!(!ensure_subdomain_cookie_scope(&dir, "wp-content").unwrap());
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), subdir);

        // Single site.
        write_config(&dir, "<?php\n/* That's all, stop editing! */\n");
        assert!(!ensure_subdomain_cookie_scope(&dir, "wp-content").unwrap());

        // No wp-config at all (Bedrock keeps its config elsewhere).
        let empty = scratch("noconfig");
        assert!(!ensure_subdomain_cookie_scope(&empty, "wp-content").unwrap());
    }

    #[test]
    fn cookie_scope_refuses_loudly_rather_than_sharing_a_network_nobody_can_log_into() {
        let dir = scratch("loud");
        // A wp-config with no anchor ahead of wp-settings: writing the block
        // anywhere else would be too late, so the start must FAIL instead of
        // handing out a public URL that refuses every login.
        write_config(
            &dir,
            "<?php\ndefine( 'MULTISITE', true );\ndefine( 'SUBDOMAIN_INSTALL', true );\n",
        );
        let err = ensure_subdomain_cookie_scope(&dir, "wp-content").unwrap_err();
        assert!(format!("{err}").contains("cannot be logged into"), "{err}");

        // An unrecorded content dir is never interpolated into PHP.
        write_config(&dir, SUBDOMAIN_CONFIG);
        assert!(ensure_subdomain_cookie_scope(&dir, "../etc").is_err());
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
