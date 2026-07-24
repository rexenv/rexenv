//! core::blueprints — apply a reusable site preset to a freshly-installed site
//! (Phase 3 §11.3). Storage + the `BlueprintSpec` shape live in `state`; this is
//! the WordPress-side automation a blueprint performs after the one-click install.
//!
//! Scope here is the WP-CLI work that needs only `(php, wp, docroot)`: installing
//! the blueprint's plugins/themes (optionally activating them) and toggling
//! WP_DEBUG. The multisite step is driven by the caller (`commands::sites`) because
//! it also persists `sites.multisite` and reloads the edge.

use crate::core::wordpress;
use crate::error::{Error, Result};
use crate::state::models::BlueprintSpec;
use std::path::Path;

/// What a blueprint application did (for logging / the command's response).
#[derive(Debug, Default, Clone)]
pub struct Applied {
    pub plugins_installed: usize,
    pub themes_installed: usize,
    pub wp_debug_set: bool,
}

/// How an apply ended: fully, or stopped between items by the job's
/// CancelToken (the partial `Applied` says how far it got — items already
/// installed are REAL installs, same partial-honesty rule as the install
/// card).
pub enum ApplyOutcome {
    Done(Applied),
    Cancelled(Applied),
}

/// Apply the WordPress parts of a blueprint to an installed site: install (and
/// optionally activate) each plugin + theme STREAMED through the provision
/// job's runner (per-item pgid kill on cancel, verbatim lines to `on_line`),
/// then set WP_DEBUG if requested. Best effort per item is NOT used — a
/// failing install surfaces as an error so the user sees the blueprint didn't
/// fully apply. Multisite is handled by the caller.
pub fn apply_wordpress(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    spec: &BlueprintSpec,
    stream: &wordpress::WpStream,
    on_line: &mut dyn FnMut(&str),
) -> Result<ApplyOutcome> {
    let mut applied = Applied::default();

    let items = spec
        .plugins
        .iter()
        .map(|p| ("plugin", p))
        .chain(spec.themes.iter().map(|t| ("theme", t)));
    for (kind, item) in items {
        let slug = item.slug.trim();
        if slug.is_empty() {
            continue;
        }
        // Slug guard BEFORE any wp-cli spawn (the wp.org-slugs-only rule).
        wordpress::ensure_slugs(kind, std::slice::from_ref(&slug.to_string()))?;
        // One install per item: each blueprint entry has its own activate flag.
        let mut args = vec![kind, "install", slug];
        if item.activate {
            args.push("--activate");
        }
        let sr = wordpress::wp_step_streamed(stream, php_bin, wp_phar, docroot, &args, on_line)?;
        if sr.cancelled {
            return Ok(ApplyOutcome::Cancelled(applied));
        }
        if !sr.ok {
            return Err(Error::Other(format!(
                "blueprint {kind} '{slug}' install failed: {}",
                sr.tail.last().cloned().unwrap_or_else(|| "no output".into())
            )));
        }
        match kind {
            "plugin" => applied.plugins_installed += 1,
            _ => applied.themes_installed += 1,
        }
    }
    if spec.wp_debug {
        wordpress::wp_debug_set(php_bin, wp_phar, docroot, true)?;
        applied.wp_debug_set = true;
    }

    Ok(ApplyOutcome::Done(applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::{BlueprintItem, MultisiteMode, SiteType, WebServer};

    fn spec() -> BlueprintSpec {
        BlueprintSpec {
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            multisite: MultisiteMode::None,
            plugins: vec![BlueprintItem { slug: "woocommerce".into(), activate: true }],
            themes: vec![],
            wp_debug: true,
            language: String::new(),
        }
    }

    #[test]
    fn spec_round_trips_through_json() {
        // The DB stores the spec as JSON; ensure the camelCase contract holds.
        let json = serde_json::to_string(&spec()).unwrap();
        assert!(json.contains("\"siteType\":\"wordpress\""));
        assert!(json.contains("\"wpDebug\":true"));
        assert!(json.contains("\"slug\":\"woocommerce\""));
        let back: BlueprintSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.plugins.len(), 1);
        assert!(back.plugins[0].activate);
        assert!(matches!(back.multisite, MultisiteMode::None));
    }
}
