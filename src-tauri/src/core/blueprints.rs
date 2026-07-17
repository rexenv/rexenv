//! core::blueprints — apply a reusable site preset to a freshly-installed site
//! (Phase 3 §11.3). Storage + the `BlueprintSpec` shape live in `state`; this is
//! the WordPress-side automation a blueprint performs after the one-click install.
//!
//! Scope here is the WP-CLI work that needs only `(php, wp, docroot)`: installing
//! the blueprint's plugins/themes (optionally activating them) and toggling
//! WP_DEBUG. The multisite step is driven by the caller (`commands::sites`) because
//! it also persists `sites.multisite` and reloads the edge.

use crate::core::wordpress;
use crate::error::Result;
use crate::state::models::BlueprintSpec;
use std::path::Path;

/// What a blueprint application did (for logging / the command's response).
#[derive(Debug, Default, Clone)]
pub struct Applied {
    pub plugins_installed: usize,
    pub themes_installed: usize,
    pub wp_debug_set: bool,
}

/// Apply the WordPress parts of a blueprint to an installed site: install (and
/// optionally activate) each plugin + theme, then set WP_DEBUG if requested. Best
/// effort per item is NOT used — a failing install surfaces as an error so the user
/// sees the blueprint didn't fully apply. Multisite is handled by the caller.
pub fn apply_wordpress(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    spec: &BlueprintSpec,
) -> Result<Applied> {
    let mut applied = Applied::default();

    for p in &spec.plugins {
        if p.slug.trim().is_empty() {
            continue;
        }
        // One install per item: each blueprint entry has its own activate flag.
        wordpress::plugin_install(php_bin, wp_phar, docroot, &[p.slug.trim().to_string()], p.activate)?;
        applied.plugins_installed += 1;
    }
    for t in &spec.themes {
        if t.slug.trim().is_empty() {
            continue;
        }
        wordpress::theme_install(php_bin, wp_phar, docroot, &[t.slug.trim().to_string()], t.activate)?;
        applied.themes_installed += 1;
    }
    if spec.wp_debug {
        wordpress::wp_debug_set(php_bin, wp_phar, docroot, true)?;
        applied.wp_debug_set = true;
    }

    Ok(applied)
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
