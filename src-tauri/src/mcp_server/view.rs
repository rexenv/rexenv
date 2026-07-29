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

/// The raw signals behind a serving diagnosis, gathered by
/// `ReadCtx::probe_serving`. Kept separate from the verdict so the I/O and the
/// classification are independently testable — the tunnel prober's lesson.
pub struct ServingSignals {
    /// WIRE: our edge answered (marker header) for this exact host.
    pub edge_answers_ours: bool,
    /// WIRE: something is listening on :443 (to tell "stopped" from "blocked").
    pub tcp_443_open: bool,
    /// MANAGER belief: edge up AND this site's upstream up (the Sites-page bool).
    pub serving_manager: bool,
    /// WIRE: the site's HTTP status for its home URL, if the edge is ours.
    pub http_status: Option<u16>,
}

/// A serving verdict — kept DISTINCT rather than collapsed into "not serving",
/// because an agent acts on whichever we imply and these are four different
/// problems with different owners.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ServingVerdict {
    /// Answers a normal response through the edge.
    Serving,
    /// The site's OWN app returned a 5xx — infrastructure is up, the site's code
    /// is not.
    SiteError,
    /// The edge is up but this site's PHP/web backend isn't answering (502/503/504).
    BackendDown,
    /// The site's setup didn't finish, or it has no working route yet.
    SetupIncomplete,
    /// A non-rexenv server holds :443.
    EdgeBlocked,
    /// Nothing is serving on :443 (the stack looks stopped).
    EdgeDown,
    /// The edge is up but the site's URL didn't respond — reason undetermined.
    Unknown,
}

/// Who can resolve a diagnosis — decided ONCE, reused by every diagnostic tool.
/// An agent cannot start services or resolve a port conflict (that is M2 at best,
/// possibly never), so the honest output names the reason AND that acting on it
/// is the user's, not the agent's — otherwise a model tries to find a way and
/// either hallucinates one or thrashes.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Resolution {
    /// Nothing to resolve — it is serving.
    None,
    /// A human action in the rexenv app (start the stack, resolve a port
    /// conflict, retry setup). An AGENT CANNOT do this.
    UserActionInRexenv,
    /// The infrastructure is fine; the site's own code/config is the issue —
    /// look at the code and the logs (the `tail_log` tool).
    CheckSiteCodeAndLogs,
}

impl ServingSignals {
    /// Classify the signals into a verdict, a human detail that STATES the
    /// probe's scope (what it can't tell), and who resolves it. Pure (takes the
    /// one `Site` fact it needs, `provisioned`), so it is unit-testable without
    /// any I/O or a `Site` fixture.
    pub fn classify(&self, provisioned: bool) -> (ServingVerdict, String, Resolution) {
        use Resolution::*;
        use ServingVerdict::*;
        // 1) Our edge isn't answering for this host.
        if !self.edge_answers_ours {
            return if self.tcp_443_open {
                (EdgeBlocked,
                 "Another server is answering on port 443 — not rexenv's edge — so rexenv can't \
                  serve this (or any) site until that's resolved. Resolving the conflict, or \
                  stopping the other server, is your action in rexenv's Services screen; an agent \
                  can't do it. The probe can't identify the other server, only that its response \
                  isn't rexenv's.".into(),
                 UserActionInRexenv)
            } else {
                (EdgeDown,
                 "Nothing is serving on port 443 — the rexenv stack looks stopped. Start it in the \
                  rexenv app (Start all); an agent can't start services. If you just started it, \
                  give it a moment — the probe only knows the edge didn't answer right now.".into(),
                 UserActionInRexenv)
            };
        }
        // 2) Edge is up and ours. Setup that never finished is the reason before
        //    any HTTP reading.
        if !provisioned {
            return (SetupIncomplete,
                "This site's setup didn't finish (it's marked incomplete) — retry or delete it in \
                 the rexenv app; an agent can't. Until then it may not serve, or serve only \
                 partially.".into(),
                UserActionInRexenv);
        }
        // 3) Read the site's own response.
        match self.http_status {
            Some(code) if (200..400).contains(&code) => {
                (Serving, format!("Serving normally (HTTP {code})."), None)
            }
            Some(code @ 502..=504) => (BackendDown,
                format!("rexenv's edge is up, but this site's PHP/web backend isn't answering \
                         (HTTP {code}). Its pool may be down or its setup incomplete — start the \
                         stack or retry the site in the rexenv app; an agent can't. The probe sees \
                         the gateway error, not the backend's own reason."),
                UserActionInRexenv),
            Some(code) if (500..600).contains(&code) => (SiteError,
                format!("The site is up but its OWN code returned an error (HTTP {code}) — the \
                         site's application, not rexenv's infrastructure. Look at the site's code \
                         and its logs (the tail_log tool). The probe can only see that the app \
                         returned {code}, not why."),
                CheckSiteCodeAndLogs),
            Some(404) if self.serving_manager => (Serving,
                "The site answered HTTP 404 for its home URL, and its backend is up — most likely \
                 the application's own not-found (an empty or freshly-installed site), not a \
                 rexenv problem. The probe can't tell an app 404 from a missing route with \
                 certainty.".into(),
                CheckSiteCodeAndLogs),
            Some(404) => (SetupIncomplete,
                "rexenv's edge is up but this site has no working route to a backend (HTTP 404, \
                 and the manager doesn't see this site's backend up) — it may have been created \
                 while the stack was stopped, or its setup didn't finish. Retry or finish setup in \
                 the rexenv app; an agent can't. The probe can't distinguish an unrouted host from \
                 an app 404 with certainty.".into(),
                UserActionInRexenv),
            Some(code) => (Serving,
                format!("The site's backend answered (HTTP {code}) — it is up and responding. The \
                         probe reports the code but can't judge whether {code} is expected for \
                         this site's app."),
                None),
            Option::None => (Unknown,
                "rexenv's edge is up, but this site's URL didn't respond at all — it may still be \
                 starting, or its backend just came down. Check Services and the site's logs in \
                 the rexenv app. The probe only knows the request didn't complete, not why.".into(),
                UserActionInRexenv),
        }
    }
}

/// A site's serving diagnosis, as an agent sees it. Carries the distilled
/// verdict and its scope — NEVER the internals the probe/doctor touched (config
/// contents, socket paths, generated vhost text, the CA). Those are dropped at
/// this conversion; the secret-leak sweep proves they don't reach an agent.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSiteStatus {
    pub id: String,
    pub domain: String,
    /// The simple answer.
    pub serving: bool,
    /// The specific, un-collapsed verdict.
    pub verdict: ServingVerdict,
    /// Human explanation — and what the probe can and cannot tell.
    pub detail: String,
    /// Who resolves it — an agent, or the user in rexenv, or the site's code.
    pub resolution: Resolution,
}

impl AgentSiteStatus {
    pub fn from_signals(site: &Site, signals: &ServingSignals) -> Self {
        let (verdict, detail, resolution) = signals.classify(site.provisioned);
        AgentSiteStatus {
            id: site.id.clone(),
            domain: site.domain.clone(),
            serving: verdict == ServingVerdict::Serving,
            verdict,
            detail,
            resolution,
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

    fn sig(edge_ours: bool, tcp443: bool, mgr: bool, http: Option<u16>) -> ServingSignals {
        ServingSignals {
            edge_answers_ours: edge_ours,
            tcp_443_open: tcp443,
            serving_manager: mgr,
            http_status: http,
        }
    }

    #[test]
    fn classify_keeps_the_failures_distinct_never_collapsing_to_not_serving() {
        use ServingVerdict::*;
        // edge down (nothing on :443) vs edge blocked (a foreign server on :443)
        assert_eq!(sig(false, false, false, None).classify(true).0, EdgeDown);
        assert_eq!(sig(false, true, false, None).classify(true).0, EdgeBlocked);
        // a gateway 502 (backend down) is NOT the site's own 500 (its code) —
        // different verdicts, different owners
        assert_eq!(sig(true, true, false, Some(502)).classify(true).0, BackendDown);
        let (v, _, r) = sig(true, true, true, Some(500)).classify(true);
        assert_eq!(v, SiteError);
        assert_eq!(r, Resolution::CheckSiteCodeAndLogs);
        // serving
        assert_eq!(sig(true, true, true, Some(200)).classify(true).0, Serving);
        // setup incomplete beats the HTTP reading, even with the edge up
        assert_eq!(sig(true, true, true, Some(200)).classify(false).0, SetupIncomplete);
        // a 404 with the backend up is the app's own 404 (serving); with the
        // backend NOT up it reads as unrouted/incomplete
        assert_eq!(sig(true, true, true, Some(404)).classify(true).0, Serving);
        assert_eq!(sig(true, true, false, Some(404)).classify(true).0, SetupIncomplete);
        // edge up but no response at all → Unknown, never a confident "serving"
        assert_eq!(sig(true, true, true, None).classify(true).0, Unknown);
    }

    #[test]
    fn every_non_serving_diagnosis_names_who_acts_and_states_the_probe_scope() {
        // The honesty contract: a non-serving verdict never leaves the model to
        // guess an action it can't take, and always says what the probe can't tell.
        for s in [
            sig(false, false, false, None),    // edge down
            sig(false, true, false, None),     // edge blocked
            sig(true, true, false, Some(502)), // backend down
            sig(true, true, true, Some(500)),  // site error
            sig(true, true, false, Some(404)), // unrouted / setup incomplete
        ] {
            let (verdict, detail, resolution) = s.classify(true);
            assert_ne!(verdict, ServingVerdict::Serving);
            assert_ne!(resolution, Resolution::None, "must name who resolves it");
            // Points at rexenv OR the site's own code/logs — never nowhere.
            assert!(
                detail.contains("rexenv") || detail.contains("code") || detail.contains("logs"),
                "{detail}"
            );
            // States a limit of the probe.
            assert!(detail.contains("can't") || detail.contains("only"), "{detail}");
        }
    }
}
