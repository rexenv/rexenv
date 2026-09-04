//! core::wp_mail_catch — catch a WordPress site's mail even when the site has
//! been configured to send it somewhere real.
//!
//! # The gap this closes
//!
//! rexenv's promise is that a local site's mail is CAUGHT, never delivered. The
//! php-fpm pool pins `sendmail_path`, which routes PHP's own `mail()` into
//! Mailpit — and that is the whole mechanism, so it holds exactly as long as
//! nothing has replaced PHPMailer's transport.
//!
//! An SMTP plugin replaces it. WP Mail SMTP, FluentSMTP, Post SMTP and every
//! plugin of that shape hook `phpmailer_init` and call `$phpmailer->isSMTP()`,
//! after which PHPMailer opens its own socket to the configured host and
//! `sendmail_path` is never consulted. Measured 4 Sep 2026 on a real site
//! through rexenv's own CLI shim:
//!
//! ```text
//! wp_mail(...)                                  => true,  in Mailpit
//! wp_mail(...) with the site calling isSMTP()   => false, NOT in Mailpit
//! ```
//!
//! Against a reachable provider the second line is worse than `false`: the mail
//! is genuinely delivered, from a developer's laptop, to whoever the row in the
//! staging database happens to name. A site imported from production carries
//! exactly that configuration.
//!
//! # Why this is allowed to win, when [`crate::core::wp_mailtag`] is not
//!
//! The stamp deliberately runs at `PHP_INT_MAX` and still LOSES to a site's own
//! later filter, because its failure direction is "the agent misses its own
//! mail" and that is the safe way to fail. This file is the opposite ruling and
//! the reasoning is the same one read from the other end: here the failure
//! direction of losing is "a customer receives mail from a laptop", so the
//! catch must be the last word. It hooks `phpmailer_init` at `PHP_INT_MAX` —
//! WordPress runs equal-priority callbacks in the order they were added, and
//! ours is added from an mu-plugin, i.e. before any ordinary plugin exists — and
//! calls `isMail()` to put the transport back on PHP's `mail()`, which the pool
//! has already aimed at Mailpit. Verified live the same day: with a plugin-shaped
//! `isSMTP()` callback registered first, `wp_mail()` returned true and the
//! message arrived in Mailpit.
//!
//! # What it does NOT catch, stated because the promise reads absolute
//!
//! A mailer that never touches PHPMailer is invisible here: an HTTP-API
//! transport (Mailgun's API, SES via the AWS SDK, Postmark's REST endpoint)
//! posts with `wp_remote_post` and no `phpmailer_init` fires. Catching those
//! would mean filtering the site's outbound HTTP by hostname, which is a
//! different mechanism with a different blast radius, and it is not built. The
//! common case — SMTP credentials in an SMTP plugin — is what this covers.

use crate::error::Result;
use crate::state::models::{Site, SiteType};
use crate::state::store;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// The auto-managed mu-plugin.
///
/// It changes the TRANSPORT and nothing else — not `From`, not the recipient,
/// not the subject or body. The developer is trying to test their mail; the
/// point is to make it arrive somewhere they can read it, not to make it
/// different. (Same rule as the scratch stamp, which touches `From` alone.)
///
/// `isMail()` rather than a hand-set `Mailer = 'mail'`: it is PHPMailer's own
/// API and it also clears the SMTP-specific state the plugin set, so nothing is
/// left half-configured for a later hook to read.
const MU_PLUGIN: &str = r#"<?php
/* Plugin Name: rexenv mail catcher
 * Description: Auto-managed by rexenv. Forces this local site's outgoing mail through rexenv's mail catcher (Mailpit), even when an SMTP plugin is configured, so a development site can never deliver to a real inbox. Turn it off in rexenv → Settings → Services → "Catch all outgoing mail". Safe to delete.
 */
if (!defined('ABSPATH')) {
    exit;
}

/**
 * Put PHPMailer back on PHP's mail() — which php-fpm's sendmail_path aims at
 * Mailpit — after any SMTP plugin has had its say.
 *
 * PHP_INT_MAX so this runs LAST. WordPress runs equal-priority callbacks in
 * registration order, and an mu-plugin is loaded before every ordinary plugin,
 * so a plugin registering at PHP_INT_MAX too would still run after us; that is
 * a deliberate site override and rexenv does not fight it.
 */
add_action('phpmailer_init', static function ($phpmailer) {
    // isMail() also clears the SMTP-specific state the plugin set, so nothing
    // is left half-configured for a later hook to read back.
    $phpmailer->isMail();
    $phpmailer->Host = '';
    $phpmailer->SMTPAuth = false;
    $phpmailer->SMTPSecure = '';
}, PHP_INT_MAX);
"#;

/// Path to the mu-plugin within a docroot. `content_rel` is the site's RECORDED
/// content dir — a hardcoded `wp-content/` writes where WordPress never loads on
/// a Bedrock site, which for THIS file means the catch silently not applying,
/// i.e. mail leaving the machine with nothing anywhere saying why.
fn mu_plugin_path(docroot: &Path, content_rel: &str) -> PathBuf {
    docroot.join(content_rel).join("mu-plugins").join("rexenv-mail.php")
}

/// Whether the mu-plugin is installed for `content_rel` (live stat, no record).
pub fn is_installed(docroot: &Path, content_rel: &str) -> bool {
    mu_plugin_path(docroot, content_rel).is_file()
}

/// Write the mu-plugin if missing or changed (idempotent). Returns whether the
/// `mu-plugins/` DIR was created by this call, which the caller records (v25
/// `sites.mu_dir_created`) so teardown can remove a dir WE created without ever
/// inferring ownership from emptiness.
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

/// Remove the mu-plugin across EVERY known layout — a stray in a Bedrock repo's
/// dead `wp-content/` still goes. Missing files are fine.
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

/// Install or REMOVE the mu-plugin for one WordPress site, according to the
/// catch-all switch.
///
/// Both directions in one function on purpose. The switch is a fact with a
/// lifetime, not an event: a file installed while the catch-all was on and left
/// behind when it was turned off would keep hijacking mail the user had asked
/// to be delivered — the one-fact-lifetime shape, and the setting would look
/// broken with nothing to point at. So every pass that can install can also
/// take it away.
///
/// Best-effort by design, like the loopback-DNS file: a site whose docroot is a
/// temporarily missing linked folder is skipped rather than conjured. Returns
/// whether the file is now in place.
pub fn apply_for_site(conn: &Connection, site: &Site) -> bool {
    if site.site_type != SiteType::Wordpress {
        return false;
    }
    let docroot = Path::new(&site.path);
    let content = docroot.join(site.content_dir_rel());
    if !content.is_dir() {
        return false;
    }
    if !crate::core::mail::catch_all_enabled(conn) {
        if let Err(e) = remove(docroot) {
            log::warn!("wp_mail_catch: could not remove the mail mu-plugin for {}: {e}", site.domain);
        }
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
            log::warn!("wp_mail_catch: could not install the mail mu-plugin for {}: {e}", site.domain);
            false
        }
    }
}

/// The startup / setting-changed pass over every WordPress site.
///
/// It is what makes "every site rexenv hosts" true rather than "every site
/// created since this shipped": sites that predate it, sites imported by another
/// path, and sites whose file a user deleted are all picked up here — and it is
/// also how turning the switch OFF actually reaches the files.
pub fn apply_all(conn: &Connection, sites: &[Site]) {
    let installed = sites.iter().filter(|s| apply_for_site(conn, s)).count();
    if installed > 0 {
        log::info!("wp_mail_catch: mail mu-plugin in place for {installed} WordPress site(s)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-wpmailcatch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wp-content")).unwrap();
        dir
    }

    /// **The hook has to run LAST and put the transport back on `mail()`.**
    ///
    /// Both halves matter and each fails differently: a lower priority loses to
    /// the SMTP plugin and catches nothing, and a hook that runs last but only
    /// clears `Host` leaves `Mailer = 'smtp'` and PHPMailer fails the send
    /// instead of delivering it locally — a mail that fails is a mail the
    /// developer never sees, which is the failure this exists to prevent
    /// wearing a different mask.
    #[test]
    fn the_catcher_runs_last_and_returns_the_transport_to_php_mail() {
        assert!(MU_PLUGIN.contains("phpmailer_init"));
        assert!(MU_PLUGIN.contains("PHP_INT_MAX"), "a lower priority loses to the SMTP plugin");
        assert!(MU_PLUGIN.contains("isMail()"), "clearing Host alone leaves Mailer = 'smtp'");
        // It changes the transport and NOTHING about the message: the developer
        // is testing their mail, and the point is that it arrives readable, not
        // that it arrives different. (Same rule as the scratch stamp's From.)
        for untouched in ["wp_mail_from", "->Subject", "->Body", "->addAddress", "wp_mail_to"] {
            assert!(!MU_PLUGIN.contains(untouched), "the catcher must not rewrite {untouched}");
        }
        // Direct access to a mu-plugin is a bare PHP file served by nginx.
        assert!(MU_PLUGIN.contains("ABSPATH"));
    }

    /// The file follows the site's RECORDED content dir. A hardcoded
    /// `wp-content/` on a Bedrock site writes where WordPress never loads, and
    /// for THIS file that means mail leaving the machine with nothing saying why.
    #[test]
    fn the_catcher_is_written_where_the_site_actually_loads_from() {
        let dir = fixture("layout");
        std::fs::create_dir_all(dir.join("app")).unwrap();

        ensure(&dir, "app").unwrap();
        assert!(is_installed(&dir, "app"));
        assert!(!is_installed(&dir, "wp-content"), "written to the layout it was given");

        // Idempotent: a second pass rewrites nothing and still reports installed.
        ensure(&dir, "app").unwrap();
        assert!(is_installed(&dir, "app"));

        // The sweep clears EVERY known layout, including a stray in a Bedrock
        // repo's dead `wp-content/`.
        ensure(&dir, "wp-content").unwrap();
        remove(&dir).unwrap();
        assert!(!is_installed(&dir, "app"));
        assert!(!is_installed(&dir, "wp-content"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
