//! Uploads on demand (plan §11 Q3, S3 — built 15 Oct 2026): a media file the local copy
//! does not have is fetched from the LIVE site the first time a page asks for it,
//! saved under `wp-content/uploads/`, and served — so "pull, recent uploads only" (and
//! a pull that skipped a file the site refused) still renders every page, and the
//! history of a big media library stays on live until a page needs one of its files.
//!
//! An auto-managed mu-plugin, like the mail catcher and the loopback-DNS one: a
//! request for a missing upload reaches WordPress (every web server rexenv generates
//! falls through to `index.php` for a path that is not a file), and at
//! `muplugins_loaded` — before any plugin, theme or 404 template — the file is
//! fetched over HTTPS from the live uploads URL baked into the plugin, written beside
//! its siblings and echoed with the live site's content type. A fetch that fails
//! lets WordPress answer its 404 as before. Only paths under the uploads folder,
//! with no `..`, and only file extensions media has. A site behind HTTP basic auth
//! gets 401s from live: the fetch then fails quietly and the page 404s as it did.
//!
//! The mu-plugin is never pushed (`pull::skipped_locally` leaves `mu-plugins/rexenv-*`
//! out of every walk) and goes with Disconnect, or with the switch in the Live tab.

use crate::error::Result;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "rexenv-live-uploads.php";

/// The plugin, with `%LIVE%` replaced by the live uploads URL (no trailing slash).
const MU_PLUGIN: &str = r#"<?php
/* Plugin Name: rexenv live uploads
 * Description: Auto-managed by rexenv. A media file this copy does not have is fetched from the live site the first time it is asked for, saved here, and served. Turn it off in rexenv → the site's Live tab ("Fetch missing uploads from live"). Safe to delete.
 */
if (!defined('ABSPATH')) {
    exit;
}

add_action('muplugins_loaded', static function () {
    $live = '%LIVE%';
    $uri = isset($_SERVER['REQUEST_URI']) ? (string) $_SERVER['REQUEST_URI'] : '';
    $path = (string) parse_url($uri, PHP_URL_PATH);
    $prefix = (string) parse_url(content_url('/uploads/'), PHP_URL_PATH);
    if ('' === $prefix || '' === $path || 0 !== strpos($path, $prefix)) {
        return;
    }
    $rel = rawurldecode(substr($path, strlen($prefix)));
    if ('' === $rel || '/' === $rel[0] || false !== strpos($rel, "\0") || false !== strpos($rel, '\\') || false !== strpos($rel, '..')) {
        return;
    }
    if (!preg_match('~\.(jpe?g|png|gif|webp|avif|svg|ico|bmp|tiff?|heic|mp4|m4v|mov|webm|mp3|m4a|ogg|wav|pdf|zip|txt|csv|json|xml|woff2?|ttf|otf|eot)$~i', $rel)) {
        return;
    }
    $local = WP_CONTENT_DIR . '/uploads/' . $rel;
    if (is_file($local)) {
        return;
    }
    $r = wp_remote_get($live . '/' . str_replace('%2F', '/', rawurlencode($rel)), array('timeout' => 25, 'redirection' => 2, 'limit_response_size' => 67108864));
    if (is_wp_error($r) || 200 !== (int) wp_remote_retrieve_response_code($r)) {
        return; // WordPress answers its 404 as before
    }
    $body = wp_remote_retrieve_body($r);
    if ('' === $body || !wp_mkdir_p(dirname($local))) {
        return;
    }
    $tmp = $local . '.rexenv-part';
    if (false === file_put_contents($tmp, $body) || !rename($tmp, $local)) {
        @unlink($tmp);
        return;
    }
    $type = wp_remote_retrieve_header($r, 'content-type');
    if (!is_string($type) || '' === $type) {
        $ft = wp_check_filetype($local);
        $type = !empty($ft['type']) ? $ft['type'] : 'application/octet-stream';
    }
    header('Content-Type: ' . $type);
    header('Content-Length: ' . strlen($body));
    header('X-Rexenv-Live-Uploads: fetched');
    echo $body; // phpcs:ignore WordPress.Security.EscapeOutput -- the file's own bytes
    exit;
});
"#;

/// The live uploads URL for a site URL: `https://host[/path]/wp-content/uploads`.
pub fn live_uploads_url(site_url: &str) -> String {
    format!("{}/wp-content/uploads", site_url.trim_end_matches('/'))
}

/// The plugin's text for `site_url`. Pure.
pub fn source(site_url: &str) -> String {
    MU_PLUGIN.replace("%LIVE%", &live_uploads_url(site_url).replace('\'', ""))
}

fn path(docroot: &Path, content_rel: &str) -> PathBuf {
    docroot.join(content_rel).join("mu-plugins").join(FILE_NAME)
}

/// Is the plugin in place for `content_rel` (a live stat)?
pub fn is_installed(docroot: &Path, content_rel: &str) -> bool {
    path(docroot, content_rel).is_file()
}

/// Write the plugin for `site_url` (idempotent; rewritten when the URL moved).
pub fn ensure(docroot: &Path, content_rel: &str, site_url: &str) -> Result<()> {
    let p = path(docroot, content_rel);
    let want = source(site_url);
    if std::fs::read_to_string(&p).ok().as_deref() != Some(want.as_str()) {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, want)?;
    }
    Ok(())
}

/// Remove the plugin from every known layout. Missing is fine.
pub fn remove(docroot: &Path) -> Result<()> {
    for layout in crate::core::sites::CONTENT_DIR_LAYOUTS {
        match std::fs::remove_file(path(docroot, layout)) {
            Ok(()) | Err(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The plugin fetches ONLY under the live uploads URL, only media extensions, and
    /// refuses a path that leaves the uploads folder** (ledger #880). Plant: drop the `..`
    /// test → red.
    #[test]
    fn the_plugin_is_bounded_to_uploads() {
        let s = source("https://shop.example.com/");
        assert!(s.contains("$live = 'https://shop.example.com/wp-content/uploads';"), "{s}");
        assert!(s.contains("false !== strpos($rel, '..')") && s.contains("'/' === $rel[0]"), "the traversal guard");
        assert!(s.contains("preg_match('~\\.(jpe?g|png|") && s.contains("muplugins_loaded"));
        assert!(s.contains("limit_response_size"), "a size ceiling on the fetch");
        assert!(!s.contains("%LIVE%"));
        // The pull's walk leaves it out of every push.
        assert!(crate::core::live_sync::pull::skipped_locally(&format!("mu-plugins/{FILE_NAME}")));
        let dir = std::env::temp_dir().join(format!("rexenv-uod-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        ensure(&dir, "wp-content", "https://a.example").unwrap();
        assert!(is_installed(&dir, "wp-content"));
        ensure(&dir, "wp-content", "https://b.example").unwrap();
        assert!(std::fs::read_to_string(path(&dir, "wp-content")).unwrap().contains("https://b.example/wp-content/uploads"));
        remove(&dir).unwrap();
        assert!(!is_installed(&dir, "wp-content"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
