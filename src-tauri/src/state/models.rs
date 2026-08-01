//! Persisted models — mirror the frontend types in `src/types/index.ts`.
//!
//! The serde representation (camelCase, lowercase enum values) is what crosses
//! the IPC boundary, so it must match the TS types. The same lowercase strings
//! are reused as the on-disk TEXT values, keeping DB and wire formats aligned.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Generate a tiny helper enum: serde lowercase + `as_db()` / `parse_db()`
/// using the identical string for wire and storage.
macro_rules! str_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum $name {
            $($variant),+
        }
        impl $name {
            /// The canonical string used for both JSON and the SQLite TEXT column.
            pub fn as_db(&self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }
            /// Parse from a stored/wire string.
            pub fn parse_db(s: &str) -> Result<Self> {
                match s {
                    $($s => Ok(Self::$variant),)+
                    other => Err(Error::Other(format!(
                        "invalid {} value: {}", stringify!($name), other
                    ))),
                }
            }
        }
    };
}

str_enum!(ServiceStatus {
    Running => "running",
    Stopped => "stopped",
    Starting => "starting",
    Error => "error",
});

str_enum!(WebServer {
    Nginx => "nginx",
    Apache => "apache",
    Frankenphp => "frankenphp",
    Openlitespeed => "openlitespeed",
});

str_enum!(SiteType {
    Wordpress => "wordpress",
    Laravel => "laravel",
    Php => "php",
});

str_enum!(MultisiteMode {
    None => "none",
    Subdomain => "subdomain",
    Subdirectory => "subdirectory",
});

str_enum!(
    /// Which SQL engine backs a site's WordPress database — chosen at create,
    /// immutable after (the DB lives in that engine's datadir). Both speak the
    /// MySQL protocol; only the port and bundled client binaries differ.
    SiteDbEngine {
        Mysql => "mysql",
        Mariadb => "mariadb",
    }
);

/// Who a site belongs to (v27) — the MCP tier boundary itself: an agent may
/// only mutate or delete a site recorded as `Agent`.
///
/// Deliberately NOT a `str_enum!`: those return an error for an unrecognised
/// stored value, which here would take out the whole sites list over one bad
/// cell. The safe read is the conservative one — **anything that is not exactly
/// `"agent"` is the user's site** — because the failure it protects against is
/// asymmetric: reading a scratch site as the user's leaks some disk until they
/// delete it, while reading a user's site as scratch feeds it to the reaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SiteOrigin {
    User,
    Agent,
}

impl SiteOrigin {
    /// The canonical string used for both JSON and the SQLite TEXT column.
    pub fn as_db(&self) -> &'static str {
        match self {
            SiteOrigin::User => "user",
            SiteOrigin::Agent => "agent",
        }
    }

    /// Read a stored value. Never fails: anything but `"agent"` reads as the
    /// user's (see the type doc for why that asymmetry is deliberate).
    pub fn parse_db(s: &str) -> Self {
        match s {
            "agent" => SiteOrigin::Agent,
            _ => SiteOrigin::User,
        }
    }
}

/// A local site as persisted in SQLite and sent to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    pub id: String,
    pub name: String,
    pub domain: String,
    #[serde(rename = "type")]
    pub site_type: SiteType,
    pub status: ServiceStatus,
    pub php_version: String,
    pub web_server: WebServer,
    pub ssl: bool,
    pub path: String,
    pub created_at: String,
    /// WordPress multisite mode (`none` for a single site). Phase 3 §10.1.
    pub multisite: MultisiteMode,
    /// MySQL database name, derived from the domain ONCE at creation
    /// (`wordpress::db_name_for`) and stored — never re-derived, so a later
    /// domain change leaves the database untouched.
    pub db_name: String,
    /// SQL engine hosting that database (v8; default `mysql`).
    pub db_engine: SiteDbEngine,
    /// Per-site Xdebug toggle (v11, §8.2): when true the site's `.php` routes
    /// to its PHP minor's DEBUG pool (same binary + the pinned `xdebug.so`)
    /// instead of the shared pool. Only offerable on minors with a pinned
    /// Xdebug bottle (`binaries::xdebug_supported`).
    pub xdebug: bool,
    /// Recorded loopback backend port for a FrankenPHP/Apache override site
    /// (v13, B20 §4): allocated collision-free ONCE and stored, so a later domain
    /// change never re-derives it (which would orphan the running backend). `None`
    /// for nginx (no per-site port) and for pre-backfill rows — consumers fall
    /// back to the derived `site_port(domain)` only in that transitional window.
    #[serde(skip)]
    pub override_port: Option<u16>,
    /// Provisioning completeness (v16). `false` = the streamed create job
    /// died/was cancelled mid-provision — the list shows an honest "setup
    /// incomplete" badge with Retry/Delete. Existing rows migrated as `true`
    /// (never alarm sites that were fine yesterday); the job sets 0 after
    /// insert and 1 only when it settles ok. `#[serde(default)]` so payloads
    /// without the field (blueprint specs, older callers) read provisioned.
    #[serde(default = "default_true")]
    pub provisioned: bool,
    /// Does rexenv own this site's docroot — may [`crate::core::sites::teardown`]
    /// remove it? (v17.)
    ///
    /// Recorded where the fact is KNOWN and never re-derived from the path at
    /// delete time: `true` when provision created the directory, `false` for a
    /// folder the user linked (we never made it) and for a docroot MOVED
    /// outside the sites folder (whose move dialog promises it is kept). The
    /// flag only ever goes `true` → `false` — monotonic toward safety.
    ///
    /// `None` = a pre-v17 row the startup backfill hasn't reached yet; only in
    /// that window do consumers fall back to the legacy lexical sites-dir test.
    #[serde(default)]
    pub docroot_managed: Option<bool>,
    /// Did rexenv CREATE this site's database — may deleting the site drop it?
    /// (v19, database import.)
    ///
    /// `Some(true)` = a database import created it. `Some(false)` = the name
    /// already existed on our engine and we restored into it with the user's
    /// typed confirmation — **never dropped, by any path**. `None` = legacy:
    /// created by rexenv's own provisioning, which is the only way a database
    /// could exist before v19, so today's teardown behaviour is unchanged.
    ///
    /// Written BEFORE `CREATE DATABASE`, never after: a crash between the write
    /// and the create leaves a claim on a database that doesn't exist, which
    /// `DROP DATABASE IF EXISTS` shrugs off. The reverse order is the one that
    /// loses data.
    #[serde(default)]
    pub db_created: Option<bool>,
    /// WP content dir RELATIVE to the docroot (v24): `wp-content` for stock
    /// WordPress, `app` for Bedrock, `content` for Radicle. Recorded once at
    /// creation / by the startup backfill from the same fs markers detection
    /// uses — writers read the record, never re-derive (our own historical
    /// junk could poison a use-time probe). `None` = pre-backfill row; reads
    /// as the WP default via [`Site::content_dir_rel`].
    #[serde(default)]
    pub content_dir: Option<String>,
    /// Did rexenv CREATE this site's `mu-plugins/` dir (v25)? `Some(true)` =
    /// a writer created it and site teardown may remove it once empty again.
    /// `None` = not ours / unknown — emptiness alone never makes it deletable.
    #[serde(default)]
    pub mu_dir_created: Option<bool>,
    /// Who this site belongs to (v27). Recorded at insert, never derived from
    /// the domain or the path; only user action moves `Agent` → `User` (Keep,
    /// or any user-initiated mutation), and nothing moves it the other way.
    #[serde(default = "default_origin_user")]
    pub origin: SiteOrigin,
    /// The MCP client's SELF-REPORTED name (v27), for the scratch card's badge.
    /// **Display-only and agent-controlled** — length-capped at write, and
    /// nothing branches on it (see [`Site::reap_due`]).
    #[serde(default)]
    pub agent_client: Option<String>,
    /// When a disposable site expires (v27, RFC-3339-ish `datetime('now')`
    /// text). **`None` means NEVER** — the shape a user site and a KEPT scratch
    /// site share, so [`Site::reap_due`] cannot treat them differently. Keep
    /// clears it; nothing recomputes it from `created_at`.
    #[serde(default)]
    pub expires_at: Option<String>,
}

impl Site {
    /// The recorded content dir relative to the docroot, defaulting to WP's
    /// stock `wp-content` when no record exists yet.
    pub fn content_dir_rel(&self) -> &str {
        self.content_dir.as_deref().unwrap_or("wp-content")
    }

    /// Is this a site an agent owns — the ONE question the tier boundary asks.
    pub fn is_scratch(&self) -> bool {
        self.origin == SiteOrigin::Agent
    }

    /// May the reaper delete this site right now, at `now` (the `datetime('now')`
    /// text format)? **The single place the reap predicate is expressed**, so
    /// "does NULL mean never here too?" has exactly one answer instead of one
    /// per call site.
    ///
    /// Every clause is a recorded fact, and each says no on its own:
    /// - `origin == Agent` — the user's sites are not the reaper's business,
    ///   including one the user hand-named `foo.scratch.rex`;
    /// - `expires_at` is `Some` AND in the past — **`None` is never**, which is
    ///   what a Kept scratch site looks like after Keep clears it (a stale
    ///   expiry left on a Kept row would be a reap waiting to happen, so Keep
    ///   clears the value rather than flipping a second flag);
    /// - `docroot_managed == Some(true)` — deletion touches a docroot, so it
    ///   happens only where we RECORDED making one. `None` (a pre-v17 row) is
    ///   not good enough for an unattended delete, even though the interactive
    ///   delete path tolerates it.
    ///
    /// `agent_client` is deliberately absent: it is the one agent-controlled
    /// value on the row, and nothing that decides a deletion may read it.
    pub fn reap_due(&self, now: &str) -> bool {
        self.is_scratch()
            && self.docroot_managed == Some(true)
            && self.expires_at.as_deref().is_some_and(|e| e < now)
    }
}

fn default_origin_user() -> SiteOrigin {
    SiteOrigin::User
}

fn default_true() -> bool {
    true
}

/// Live per-site serving status (H1 follow-up). `serving` is true only when the edge
/// is up AND the site's own upstream is up (its FrankenPHP backend, or nginx + the
/// php-fpm pool its version routes to) — so a partial stack no longer shows every
/// site as running. Keyed by `domain` (the frontend overlays it on the site rows).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteServing {
    pub domain: String,
    pub serving: bool,
}

/// A PHP version in the installed-versions registry (Phase 2 §1.2). Keyed by the
/// minor series (`8.3`); `patch` is the pinned build (`8.3.31`); `fpm_port` is the
/// deterministic loopback port of that version's php-fpm pool. `installed` is true
/// once the user has added the version (its binaries are fetched on first start).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpVersion {
    /// Minor series, e.g. `8.3` — the primary key and what a `Site.php_version` references.
    pub minor: String,
    /// Pinned patch build, e.g. `8.3.31`.
    pub patch: String,
    /// Deterministic loopback FastCGI port of this version's pool.
    pub fpm_port: u16,
    /// Whether this version is enabled (the app starts a pool for it).
    pub installed: bool,
    /// Whether this is the default version for new sites.
    pub is_default: bool,
}

/// A git-sourced wp-content dir's provenance (add-from-Git): which repo/ref a
/// plugin or theme folder was cloned from. Drives the list "git" badge and is
/// the seam for future update-pull/watch features.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitAsset {
    /// "plugin" | "theme".
    pub kind: String,
    /// Folder name under wp-content/{plugins,themes} — matches the wp-cli
    /// list row's slug/stylesheet name.
    pub dir_name: String,
    /// Normalized clone URL (adopted repos: their origin remote; may be
    /// empty when a local-only checkout has no remote).
    pub url: String,
    /// Requested branch/tag at add time (None = the remote default).
    pub git_ref: Option<String>,
    /// "cloned" | "adopted" | "linked" — linked assets DELETE BY UNLINK.
    pub source: String,
}

/// One plugin/theme entry in a blueprint: a wp.org slug + whether to activate it
/// after install (Phase 3 §11.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintItem {
    pub slug: String,
    #[serde(default)]
    pub activate: bool,
}

/// The reusable recipe a blueprint applies to a new site (§11.3). Stored as a JSON
/// blob in `blueprints.spec`. The site config fields pre-fill the New Site dialog;
/// the WordPress fields drive post-install automation (plugins/themes/multisite).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintSpec {
    #[serde(rename = "siteType")]
    pub site_type: SiteType,
    pub php_version: String,
    pub web_server: WebServer,
    #[serde(default = "multisite_none")]
    pub multisite: MultisiteMode,
    #[serde(default)]
    pub plugins: Vec<BlueprintItem>,
    #[serde(default)]
    pub themes: Vec<BlueprintItem>,
    #[serde(default)]
    pub wp_debug: bool,
    #[serde(default)]
    pub language: String,
}

fn multisite_none() -> MultisiteMode {
    MultisiteMode::None
}

/// A named, reusable site preset (§11.3): id + display name + its [`BlueprintSpec`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    pub id: String,
    pub name: String,
    pub spec: BlueprintSpec,
}

/// Input for creating a site. `id`, `status`, `ssl`, and `created_at` are
/// assigned by `core::sites::create`, not supplied by the caller.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSite {
    pub name: String,
    pub domain: String,
    #[serde(rename = "type")]
    pub site_type: SiteType,
    pub php_version: String,
    pub web_server: WebServer,
    pub path: String,
    /// SQL engine for the site's database. Defaults to MySQL so older
    /// callers/blueprints keep working unchanged.
    #[serde(default = "db_engine_mysql")]
    pub db_engine: SiteDbEngine,
}

fn db_engine_mysql() -> SiteDbEngine {
    SiteDbEngine::Mysql
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A site in PRODUCTION shape (UUID id, absolute app-data docroot) — a
    /// friendlier fixture would let a wrong field read as right.
    fn site(origin: SiteOrigin, expires_at: Option<&str>, docroot_managed: Option<bool>) -> Site {
        Site {
            id: "7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30".into(),
            name: "Shop".into(),
            domain: "shop.scratch.rex".into(),
            site_type: SiteType::Wordpress,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            ssl: true,
            path: "/Users/x/Library/Application Support/dev.rexenv.rexenv/Sites/shop.scratch.rex"
                .into(),
            created_at: "2026-08-01 09:00:00".into(),
            multisite: MultisiteMode::None,
            db_name: "wp_shop_scratch".into(),
            db_engine: SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed,
            db_created: None,
            content_dir: None,
            mu_dir_created: None,
            origin,
            agent_client: None,
            expires_at: expires_at.map(str::to_string),
        }
    }

    const NOW: &str = "2026-08-01 12:00:00";
    const PAST: &str = "2026-08-01 11:00:00";
    const FUTURE: &str = "2026-08-01 13:00:00";

    #[test]
    fn a_stored_origin_that_isnt_exactly_agent_reads_as_the_users_site() {
        assert_eq!(SiteOrigin::parse_db("agent"), SiteOrigin::Agent);
        assert_eq!(SiteOrigin::parse_db("user"), SiteOrigin::User);
        // Anything else — junk, a future value, an empty cell — reads as the
        // user's. Erroring here would fail the whole sites list over one cell;
        // reading it as scratch would feed a real site to the reaper. Only one
        // of those two failure modes is survivable.
        for odd in ["", "AGENT", " agent", "robot", "üser"] {
            assert_eq!(SiteOrigin::parse_db(odd), SiteOrigin::User, "{odd:?}");
        }
    }

    #[test]
    fn null_expiry_means_never_for_a_user_site_and_a_kept_scratch_site_alike() {
        // The two rows that carry no expiry must be indistinguishable to the
        // reaper — this is the whole reason Keep CLEARS `expires_at` instead of
        // setting a second flag beside a stale one.
        let user = site(SiteOrigin::User, None, Some(true));
        let kept = site(SiteOrigin::Agent, None, Some(true)); // Keep cleared it
        assert!(!user.reap_due(NOW));
        assert!(!kept.reap_due(NOW), "a Kept scratch site never expires");
        assert!(!user.reap_due("2099-01-01 00:00:00"));
        assert!(!kept.reap_due("2099-01-01 00:00:00"));
    }

    #[test]
    fn reap_is_due_only_for_an_expired_agent_site_whose_docroot_we_recorded_making() {
        // The one true case.
        assert!(site(SiteOrigin::Agent, Some(PAST), Some(true)).reap_due(NOW));
        // Every clause says no on its own.
        assert!(!site(SiteOrigin::Agent, Some(FUTURE), Some(true)).reap_due(NOW), "not yet expired");
        assert!(
            !site(SiteOrigin::User, Some(PAST), Some(true)).reap_due(NOW),
            "a USER site with a stale expiry is still not the reaper's business — including one \
             the user hand-named *.scratch.rex"
        );
        assert!(
            !site(SiteOrigin::Agent, Some(PAST), Some(false)).reap_due(NOW),
            "a docroot we do not own is never deleted unattended"
        );
        assert!(
            !site(SiteOrigin::Agent, Some(PAST), None).reap_due(NOW),
            "NULL docroot_managed (pre-v17) is not good enough for an UNATTENDED delete, even \
             though the interactive delete path tolerates it"
        );
        // Boundary: expiry exactly now has not passed.
        assert!(!site(SiteOrigin::Agent, Some(NOW), Some(true)).reap_due(NOW));
    }

    #[test]
    fn nothing_about_a_deletion_reads_the_agent_asserted_client_name() {
        // `agent_client` is the ONE agent-controlled value on the row. It is
        // display-only, and the place where branching on it would actually hurt
        // is the predicate that deletes things — so pin it there: two sites
        // differing ONLY in that field must reap identically, whatever it says.
        let mut plain = site(SiteOrigin::Agent, Some(PAST), Some(true));
        let mut hostile = plain.clone();
        plain.agent_client = Some("Claude Code".into());
        hostile.agent_client = Some("' OR origin='user".into());
        assert_eq!(plain.reap_due(NOW), hostile.reap_due(NOW));
        assert!(hostile.reap_due(NOW));
        // And a user site stays untouchable no matter what it claims to be.
        let mut user = site(SiteOrigin::User, Some(PAST), Some(true));
        user.agent_client = Some("Claude Code".into());
        assert!(!user.reap_due(NOW));
    }
}
