//! Agent-facing views — what a tool is allowed to tell an agent, as a type that
//! can only carry those fields (the Stage-2/3 "types that can't carry a secret"
//! discipline, applied to the read boundary).
//!
//! The conversion DROPS rather than redacts: every kept field is named in
//! `from_site`, so a new `Site` field never auto-appears in agent output —
//! surfacing it is a deliberate, reviewable edit here. Notably absent: the
//! docroot **path** and the **db_name**. A path is a filesystem pointer into the
//! user's project that an agent doesn't need to answer "which sites do I have";
//! the default is that a path appears only where a tool genuinely cannot work
//! without it, and listing sites is not that.

use crate::state::models::{Site, SiteType, WebServer};
use serde::Serialize;

/// One site, as an agent sees it. `SiteType`/`WebServer` serialize to their
/// lowercase wire strings (`"wordpress"`, `"nginx"`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSiteView {
    /// Stable id — the handle for referencing this site in later tool calls.
    pub id: String,
    /// The domain — the human-meaningful reference (e.g. `myblog.rex`).
    pub domain: String,
    /// Display name.
    pub name: String,
    #[serde(rename = "type")]
    pub site_type: SiteType,
    /// PHP minor the site runs (e.g. `8.3`).
    pub php_version: String,
    /// Web server backing it — serving context an agent uses when a site won't load.
    pub web_server: WebServer,
    /// Whether the site is ACTUALLY serving right now (edge up AND its upstream
    /// up), not merely whether the stack is up.
    pub serving: bool,
}

impl AgentSiteView {
    /// Build from a `Site` and its live serving state. Every field is named
    /// explicitly; any `Site` field not named here is dropped by construction.
    pub fn from_site(s: &Site, serving: bool) -> Self {
        AgentSiteView {
            id: s.id.clone(),
            domain: s.domain.clone(),
            name: s.name.clone(),
            site_type: s.site_type, // Copy
            php_version: s.php_version.clone(),
            web_server: s.web_server, // Copy
            serving,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_view_carries_only_the_agent_fields_never_a_path_or_db_name() {
        let v = AgentSiteView {
            id: "abc".into(),
            domain: "myblog.rex".into(),
            name: "My Blog".into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            serving: true,
        };
        let json = serde_json::to_value(&v).expect("serialise");
        let keys: BTreeSet<&str> =
            json.as_object().expect("object").keys().map(String::as_str).collect();
        let expected: BTreeSet<&str> =
            ["id", "domain", "name", "type", "phpVersion", "webServer", "serving"]
                .into_iter()
                .collect();
        // Adding a field to AgentSiteView is a deliberate act — this fails loudly
        // if one appears, so a docroot path or db name can never slip in silently.
        assert_eq!(keys, expected, "AgentSiteView key set drifted");
        assert!(json.get("path").is_none() && json.get("docroot").is_none(), "path leaked");
        assert!(json.get("dbName").is_none(), "db name leaked");
        // The wire strings are the readable lowercase forms.
        assert_eq!(json["type"], "wordpress");
        assert_eq!(json["webServer"], "nginx");
    }
}
