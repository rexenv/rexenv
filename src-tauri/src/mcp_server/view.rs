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
}

/// A serving verdict — kept DISTINCT rather than collapsed into "not serving",
/// because an agent acts on whichever we imply and these are different problems
/// with different owners. All are read from the SERVING PATH's own state (edge
/// liveness + the manager's backend state), NEVER by requesting the site — M1
/// runs nothing, so the site's own render errors are `tail_log`'s territory, not
/// a verdict here.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ServingVerdict {
    /// rexenv's edge and this site's backend are both up — the stack is serving it.
    Serving,
    /// The edge is up but this site's PHP/web backend isn't running.
    BackendDown,
    /// The site's setup didn't finish, or it has no working route yet.
    SetupIncomplete,
    /// A non-rexenv server holds :443.
    EdgeBlocked,
    /// Nothing is serving on :443 (the stack looks stopped).
    EdgeDown,
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
    // A third owner ("check the site's own code/logs") was here for the
    // HTTP-500 verdict; Option A drops the site request, so site_status no
    // longer produces it — the site's own errors are tail_log's territory. Re-add
    // deliberately if a future diagnostic needs it, rather than keep it unused.
}

impl ServingSignals {
    /// Classify the signals into a verdict, a human detail that STATES the
    /// probe's scope (what it can't tell), and who resolves it. Pure (takes the
    /// one `Site` fact it needs, `provisioned`), so it is unit-testable without
    /// any I/O or a `Site` fixture.
    pub fn classify(&self, provisioned: bool) -> (ServingVerdict, String, Resolution) {
        use Resolution::*;
        use ServingVerdict::*;
        // 1) Our edge isn't answering for this host (a 204 marker probe the edge
        //    answers itself — it does NOT request the site).
        if !self.edge_answers_ours {
            return if self.tcp_443_open {
                (EdgeBlocked,
                 "Another server is answering on port 443 — not rexenv's edge — so rexenv can't \
                  serve this (or any) site until that's resolved. Resolving the conflict, or \
                  stopping the other server, is your action in rexenv's Services screen; an agent \
                  can't do it. The probe can't identify the other server, only that it isn't \
                  rexenv's edge.".into(),
                 UserActionInRexenv)
            } else {
                (EdgeDown,
                 "Nothing is serving on port 443 — the rexenv stack looks stopped. Start it in the \
                  rexenv app (Start all); an agent can't start services. If you just started it, \
                  give it a moment — the probe only knows the edge didn't answer right now.".into(),
                 UserActionInRexenv)
            };
        }
        // 2) Edge is up and ours. Setup that never finished is the reason first.
        if !provisioned {
            return (SetupIncomplete,
                "This site's setup didn't finish (it's marked incomplete) — retry or delete it in \
                 the rexenv app; an agent can't. Until then it may not serve, or serve only \
                 partially.".into(),
                UserActionInRexenv);
        }
        // 3) Edge up + provisioned: is the site's own backend up, per the stack's
        //    state? (Read from service_infos — NOT by requesting the site.)
        if self.serving_manager {
            (Serving,
             "The stack is serving this site — rexenv's edge and this site's backend are both up. \
              This checks the serving PATH from the stack's own state, WITHOUT requesting the site \
              (M1 runs nothing). Whether the site's own code renders correctly — a PHP fatal, a \
              plugin error — is NOT checked here; use tail_log for the site's own errors.".into(),
             None)
        } else {
            (BackendDown,
             "rexenv's edge is up, but this site's PHP/web backend isn't running (the stack has no \
              live pool/server for it). Start the stack, or if setup was incomplete retry the \
              site, in the rexenv app; an agent can't. Read from the stack's state, not by \
              requesting the site.".into(),
             UserActionInRexenv)
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

/// A tail of a site's log, as an agent sees it — scrubbed of KNOWN
/// rexenv-issued tokens and cookie headers, capped, tail-only.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLogTail {
    pub id: String,
    pub domain: String,
    /// Which log this is (M1: `wp-debug` only).
    pub source: &'static str,
    /// The most recent lines, oldest first, each scrubbed.
    pub lines: Vec<String>,
    /// The honesty contract, in the output itself — NOT "sanitised".
    pub note: &'static str,
}

/// The scope + scrubber caveat, stated where the agent reads it every time.
const LOG_NOTE: &str = "The site's own WordPress debug log (tail only, capped). Paths rexenv knows \
    — the site's docroot, rexenv's own directories, the home directory — are replaced with labels \
    like <docroot>, and rexenv-issued login tokens and cookie headers are removed. This does NOT \
    make the content safe: a debug log can contain anything the site's code wrote to it (paths \
    rexenv doesn't know, request data, config dumps, third-party API responses). Treat it as raw \
    output.";

/// The note when there is no WordPress debug log to read — a non-WordPress site.
/// A normal, non-concerning answer, not an error.
const LOG_NOTE_NONE: &str = "This site isn't WordPress, so it has no WordPress debug log. Only the \
    WordPress debug log is exposed in this version.";

impl AgentLogTail {
    /// Build from the raw tail, scrubbing each line against known token/cookie
    /// shapes AND the paths rexenv knows (so an absolute path in a stack trace
    /// doesn't hand the agent the docroot + OS username that `AgentSiteView`
    /// drops). `source` is a fixed string from the closed set, never a filename.
    pub fn from_lines(
        id: &str,
        domain: &str,
        known: &KnownPaths,
        source: &'static str,
        raw: Vec<String>,
    ) -> Self {
        AgentLogTail {
            id: id.to_string(),
            domain: domain.to_string(),
            source,
            lines: raw.iter().map(|l| scrub_log_line(l, known)).collect(),
            note: LOG_NOTE,
        }
    }

    /// A non-WordPress site: no log source, returned as a normal EMPTY result
    /// (never an error — so it doesn't read as "something's off" in the feed).
    pub fn none_for_non_wordpress(id: &str, domain: &str) -> Self {
        AgentLogTail {
            id: id.to_string(),
            domain: domain.to_string(),
            source: "wp-debug",
            lines: Vec::new(),
            note: LOG_NOTE_NONE,
        }
    }
}

/// The absolute paths rexenv KNOWS, each with the label that replaces it.
///
/// **Derived, not enumerated at the call site.** Every entry but the docroot
/// comes from the [`Paths`] trait — the one place that already knows where
/// rexenv keeps things — so a directory added there is scrubbed everywhere
/// without a second list needing to hear about it. The docroot is the site's
/// own, and `<home>` is last because it is the least specific: what these paths
/// really carry is the **OS username**, and home is the prefix that carries it
/// even in paths rexenv never chose.
///
/// Entries are sorted longest-first and applied in that order, so the most
/// specific label wins: a file under `config_dir` reads `<rexenv-config>/…`, not
/// `<rexenv-data>/config/…`.
///
/// **What this cannot reach, stated where it is built.** The set is closed by
/// construction — it is *the paths rexenv knows*. Output from a raw runner or a
/// debug log is arbitrary: a plugin can print a path under `/opt`, another
/// user's home, a path assembled at runtime, or a secret that is not a path at
/// all. None of those are reachable by any prefix list, and the claim is
/// therefore "rexenv's own paths are removed", never "no path escapes".
pub struct KnownPaths {
    entries: Vec<(String, &'static str)>,
}

impl KnownPaths {
    /// The full set for one site: its docroot, rexenv's own directories, home.
    pub fn for_site(paths: &dyn crate::platform::traits::Paths, docroot: &str) -> Self {
        let home = directories::BaseDirs::new().map(|b| b.home_dir().display().to_string());
        Self::with_home(paths, docroot, home.as_deref())
    }

    /// Home injected, so the set is testable without depending on whose machine
    /// the test runs on.
    fn with_home(
        paths: &dyn crate::platform::traits::Paths,
        docroot: &str,
        home: Option<&str>,
    ) -> Self {
        let mut entries = vec![(docroot.to_string(), "<docroot>")];
        for (dir, label) in [
            (paths.config_dir(), "<rexenv-config>"),
            (paths.log_dir(), "<rexenv-logs>"),
            (paths.bin_dir(), "<rexenv-bin>"),
            (paths.app_data_dir(), "<rexenv-data>"),
        ] {
            if let Ok(d) = dir {
                entries.push((d.display().to_string(), label));
            }
        }
        if let Some(h) = home {
            entries.push((h.to_string(), "<home>"));
        }
        Self::sorted(entries)
    }

    fn sorted(mut entries: Vec<(String, &'static str)>) -> Self {
        // A blank or root prefix would replace everything (or nothing useful) —
        // an unconfigured path must not turn the scrubber into a shredder.
        entries.retain(|(p, _)| !p.is_empty() && p != "/");
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        entries.dedup_by(|a, b| a.0 == b.0);
        KnownPaths { entries }
    }
}

/// Redact KNOWN rexenv-issued tokens, cookie headers and rexenv's own absolute
/// paths from one line of agent-facing text.
///
/// **One function, every door.** It scrubs `tail_log`'s log lines and `wp_run`'s
/// stdout/stderr alike, because they carry the SAME values — a WP fatal's stack
/// trace and a `Success: Created …` line both print the docroot, and the OS
/// username inside it, that `AgentSiteView` deliberately drops (#201). A second
/// scrubber would agree the day it was written and drift after, which
/// `one_scrubber_serves_every_door_a_docroot_can_leave_by` fails on.
///
/// **It does NOT make arbitrary content safe.** It removes THESE shapes: a
/// rexenv login token (`rexenv_login=…`), `Cookie:`/`Set-Cookie:` values, and
/// the prefixes in [`KnownPaths`]. A debug log holds whatever the site's code
/// logged and a raw `wp` command prints whatever it prints; no pattern list
/// catches an unknown-shaped secret, and no prefix list catches a path rexenv
/// never chose. The tools' scope limits (closed log source, tail-only, line and
/// byte caps) and their notes are the rest of the defence; this is one honest
/// layer, never a "the output is now safe" claim.
pub fn scrub_log_line(line: &str, known: &KnownPaths) -> String {
    let mut out = redact_token_after(line, "rexenv_login=");
    out = redact_cookie_header(&out);
    // Longest prefix first (see `KnownPaths`), applied to the accumulating
    // string so an already-labelled path can't be re-matched by a shorter one.
    for (prefix, label) in &known.entries {
        out = out.replace(prefix.as_str(), label);
    }
    out
}

/// Replace the token following `marker` (URL-token characters) with `<redacted>`,
/// for every occurrence in the line.
fn redact_token_after(line: &str, marker: &str) -> String {
    let mut result = String::new();
    let mut rest = line;
    while let Some(pos) = rest.find(marker) {
        result.push_str(&rest[..pos + marker.len()]);
        let after = &rest[pos + marker.len()..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '%')))
            .unwrap_or(after.len());
        result.push_str("<redacted>");
        rest = &after[end..];
    }
    result.push_str(rest);
    result
}

/// Redact a cookie header's value. `Set-Cookie:` is checked before `Cookie:` so
/// the shorter marker never matches inside the longer one.
fn redact_cookie_header(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    for marker in ["set-cookie:", "cookie:"] {
        if let Some(pos) = lower.find(marker) {
            let end = pos + marker.len();
            return format!("{}{} <redacted>", &line[..pos], &line[pos..end]);
        }
    }
    line.to_string()
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

    fn sig(edge_ours: bool, tcp443: bool, mgr: bool) -> ServingSignals {
        ServingSignals { edge_answers_ours: edge_ours, tcp_443_open: tcp443, serving_manager: mgr }
    }

    #[test]
    fn classify_keeps_the_failures_distinct_never_collapsing_to_not_serving() {
        use ServingVerdict::*;
        // edge down (nothing on :443) vs edge blocked (a foreign server on :443)
        assert_eq!(sig(false, false, false).classify(true).0, EdgeDown);
        assert_eq!(sig(false, true, false).classify(true).0, EdgeBlocked);
        // edge up + provisioned: backend up (per the stack's own state) = serving;
        // backend down = BackendDown — distinct, different owners.
        assert_eq!(sig(true, true, true).classify(true).0, Serving);
        assert_eq!(sig(true, true, false).classify(true).0, BackendDown);
        // setup incomplete beats the backend state, even with the edge up.
        assert_eq!(sig(true, true, true).classify(false).0, SetupIncomplete);
    }

    #[test]
    fn every_non_serving_names_the_user_and_serving_states_it_never_ran_the_site() {
        // A non-serving verdict never leaves the model to guess an action it
        // can't take: all M1 infra faults resolve to the user in rexenv.
        for (s, prov) in [
            (sig(false, false, false), true), // edge down
            (sig(false, true, false), true),  // edge blocked
            (sig(true, true, false), true),   // backend down
            (sig(true, true, false), false),  // setup incomplete
        ] {
            let (verdict, detail, resolution) = s.classify(prov);
            assert_ne!(verdict, ServingVerdict::Serving);
            assert_eq!(resolution, Resolution::UserActionInRexenv, "an agent can't fix infra");
            assert!(detail.contains("rexenv"), "{detail}");
            assert!(detail.contains("can't") || detail.contains("only"), "{detail}");
        }
        // The Serving verdict itself states it did NOT run the site (Option A) and
        // points at tail_log for the site's own render errors.
        let (v, detail, _) = sig(true, true, true).classify(true);
        assert_eq!(v, ServingVerdict::Serving);
        assert!(detail.contains("requesting the site"), "must state it didn't run the site: {detail}");
        assert!(detail.contains("tail_log"), "{detail}");
    }

    /// rexenv's paths, in PRODUCTION shape — an absolute macOS app-data root
    /// under a home directory, with the real nesting. A friendlier fixture
    /// (`/tmp/x`, a flat layout) would hide both the username the scrub exists
    /// for and the longest-prefix-wins ordering.
    struct FakePaths {
        data: std::path::PathBuf,
    }
    impl crate::platform::traits::Paths for FakePaths {
        fn app_data_dir(&self) -> crate::error::Result<std::path::PathBuf> {
            Ok(self.data.clone())
        }
        fn config_dir(&self) -> crate::error::Result<std::path::PathBuf> {
            Ok(self.data.join("config"))
        }
        fn log_dir(&self) -> crate::error::Result<std::path::PathBuf> {
            Ok(self.data.join("logs"))
        }
        fn bin_dir(&self) -> crate::error::Result<std::path::PathBuf> {
            Ok(self.data.join("bin"))
        }
        fn hosts_file(&self) -> std::path::PathBuf {
            std::path::PathBuf::from("/etc/hosts")
        }
    }

    const HOME: &str = "/Users/somebody";

    fn known(docroot: &str) -> KnownPaths {
        let paths = FakePaths {
            data: std::path::PathBuf::from(HOME).join("Library/Application Support/rexenv"),
        };
        KnownPaths::with_home(&paths, docroot, Some(HOME))
    }

    #[test]
    fn the_scrub_covers_every_path_rexenv_knows_and_the_most_specific_label_wins() {
        // The widening (12b): `tail_log` leaked the docroot (#201); `wp_run`
        // re-emits it AND rexenv's own directories — `wp cli info` prints the
        // wp-cli phar and the pinned PHP binary, both under app-data, both
        // carrying the OS username. Every entry below is DERIVED from `Paths`,
        // so a directory added there is scrubbed without a second list hearing
        // about it.
        let dr = format!("{HOME}/Sites/probe.scratch.rex");
        let k = known(&dr);

        let cases = [
            (format!("PHP Fatal error: boom in {dr}/wp-content/plugins/x.php on line 5"),
             "<docroot>/wp-content/plugins/x.php"),
            // Nested under app-data: the SPECIFIC label wins, not `<rexenv-data>/bin/…`.
            (format!("Error: {HOME}/Library/Application Support/rexenv/bin/wp-cli.phar not found"),
             "<rexenv-bin>/wp-cli.phar"),
            (format!("see {HOME}/Library/Application Support/rexenv/logs/php-fpm-8.3.log"),
             "<rexenv-logs>/php-fpm-8.3.log"),
            (format!("nginx: {HOME}/Library/Application Support/rexenv/config/nginx.conf"),
             "<rexenv-config>/nginx.conf"),
            (format!("db at {HOME}/Library/Application Support/rexenv/rexenv.sqlite3"),
             "<rexenv-data>/rexenv.sqlite3"),
            // Under home but none of rexenv's: the username still must not travel.
            (format!("required {HOME}/Projects/acme/vendor/autoload.php"),
             "<home>/Projects/acme/vendor/autoload.php"),
        ];
        for (line, expected) in cases {
            let s = scrub_log_line(&line, &k);
            assert!(s.contains(expected), "expected `{expected}` in `{s}`");
            assert!(!s.contains(HOME), "the OS username survived: {s}");
        }

        // And the honest limit, asserted rather than only written: a path rexenv
        // never chose is NOT reachable by a prefix list, and comes through as-is.
        let unknown = "Error: /opt/vendor/acme/lib.php is missing";
        assert_eq!(scrub_log_line(unknown, &k), unknown, "the claim is rexenv's paths, not all paths");
    }

    #[test]
    fn a_blank_or_root_prefix_never_turns_the_scrubber_into_a_shredder() {
        // An unconfigured docroot (a site row mid-provision) must not match
        // every line, and a `Paths` impl answering `/` must not erase the output.
        let k = KnownPaths::with_home(&FakePaths { data: std::path::PathBuf::from("/") }, "", None);
        let line = "Success: Activated plugin 'acme'.";
        assert_eq!(scrub_log_line(line, &k), line);
    }

    #[test]
    fn the_scrubber_removes_tokens_cookies_and_the_docroot_but_keeps_benign_content() {
        let dr = format!("{HOME}/Sites/myblog.rex");
        let k = known(&dr);
        let token = "TOKENSECRETee55ff66";
        let scrubbed = scrub_log_line(&format!("GET /wp-login.php?rexenv_login={token}&redir=1"), &k);
        assert!(!scrubbed.contains(token), "login token survived: {scrubbed}");
        assert!(scrubbed.contains("rexenv_login=<redacted>"), "{scrubbed}");
        assert!(scrubbed.contains("redir=1"), "benign query lost — the scrubber over-reached");

        let cookie =
            scrub_log_line("Set-Cookie: wordpress_logged_in=SECRETVALUE99; Path=/; HttpOnly", &k);
        assert!(!cookie.contains("SECRETVALUE99"), "cookie value survived: {cookie}");
        assert!(cookie.starts_with("Set-Cookie: <redacted>"), "{cookie}");

        // A realistic WP stack-trace line: the docroot (and the OS username in
        // it) must NOT reach the agent — this is the assembly-review leak.
        let trace = format!("PHP Fatal error: boom in {dr}/wp-content/plugins/x.php on line 5");
        let s = scrub_log_line(&trace, &k);
        assert!(!s.contains(&dr), "docroot survived: {s}");
        assert!(!s.contains(HOME), "OS home/username survived: {s}");
        assert!(s.contains("<docroot>/wp-content/plugins/x.php"), "{s}");

        // Benign content is untouched — the scrubber is not a blanket eraser.
        let benign = "[29-Jul-2026] PHP Warning: undefined variable $x on line 10";
        assert_eq!(scrub_log_line(benign, &k), benign);
    }

    #[test]
    fn the_log_tail_note_never_claims_the_content_is_safe() {
        // The one place a false "logs are sanitised" line would get written.
        let tail =
            AgentLogTail::from_lines("id", "d.rex", &known("/dr"), "wp-debug", vec!["a".into()]);
        let note = tail.note.to_ascii_lowercase();
        assert!(note.contains("not") && note.contains("safe"), "{}", tail.note);
        assert!(!note.contains("sanitis"), "must not claim sanitised: {}", tail.note);
    }
}
