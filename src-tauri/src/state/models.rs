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
    /// Which SQL engine backs a site's database — chosen at create, immutable
    /// after (the DB lives in that engine's datadir).
    ///
    /// `Mysql`/`Mariadb` speak the same wire protocol, take the same client
    /// flags and the same SQL; only the port and the bundled binaries differ,
    /// which is why MariaDB cost nothing to add. **`Postgres` is the first one
    /// that does not**: different client, different dump tool, different DDL
    /// (see `docs/archive/PLAN-postgres-sites.md`), so every site-DB operation
    /// dispatches on the engine rather than assuming one — through
    /// [`crate::core::db::DbEngine`]'s methods, never a bare
    /// `core::database::*` call.
    ///
    /// **WordPress is never `Postgres`** — `wpdb` speaks mysqli/PDO-MySQL only,
    /// so the pair is a broken site rather than a limited one; Laravel and
    /// Blank PHP have no such constraint.
    SiteDbEngine {
        Mysql => "mysql",
        Mariadb => "mariadb",
        Postgres => "postgres",
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

/// A plugin or theme an agent CLONED into a scratch site (v29, S1).
///
/// The clone is why this is recorded rather than derived: the scratch site runs
/// a SNAPSHOT of the source, so "where did this come from" and "when was it last
/// taken" are facts only the add/sync act knows. `kind` is read from the
/// source's own header before the clone — never a parameter an agent asserts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchPackage {
    pub site_id: String,
    pub slug: String,
    /// `plugin` or `theme`, DERIVED from the source's header.
    pub kind: String,
    /// Recorded at add time, never re-derived — a sync can only re-read where
    /// the clone came from.
    pub source_path: String,
    pub synced_at: String,
    /// Stat-only summary of the source tree at sync time. A difference means it
    /// CHANGED; sameness is a strong hint, not a proof.
    pub fingerprint: String,
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
    /// Folder INSIDE [`Site::path`] that the web server roots at (v32). `""`
    /// (the default, and every pre-v32 row) means the path itself. Laravel sets
    /// `public`, keeping `.env` — the site's database credentials — one level
    /// above anything the web server will ever serve.
    ///
    /// Never read this field directly to build a docroot; call
    /// [`Site::served_root`]. `path` stays the thing teardown removes.
    #[serde(default)]
    pub docroot_subdir: String,
    /// The repository this site's code was CLONED from (v33), normalized as
    /// `core::repo::parse_source` produced it. `None` = the code did not come
    /// from a repo — exact for every pre-v33 row, since nothing could clone
    /// into a docroot before that migration.
    #[serde(default)]
    pub git_url: Option<String>,
    /// The branch or tag the user PICKED at create (v33), or `None` for the
    /// remote's default. A record of the choice, not a mirror of the working
    /// tree: what is checked out NOW is git's answer to give
    /// (`core::repo::read_git_status`), and a second copy here would go stale
    /// the first time anyone switches branch.
    #[serde(default)]
    pub git_ref: Option<String>,
    /// Did the user ask for `artisan migrate` when this site was created (v33)?
    ///
    /// Recorded because RETRY re-enters every phase, possibly after an app
    /// restart — without the record, unchecking migrations would hold for
    /// exactly one run and then reverse itself. `None` = ON, which is exact:
    /// every Laravel site made before this column migrated unconditionally.
    /// Read through [`Site::runs_migrations`], never directly.
    #[serde(default)]
    pub git_migrate: Option<bool>,
    /// Did the user ask for the repository's front-end assets to be built
    /// (v35)? Recorded for the same reason as [`Site::git_migrate`] — Retry
    /// rebuilds the phase list from the row. `None` = NO, which is exact:
    /// nothing before this column ran a package manager during provisioning.
    /// Read through [`Site::builds_assets`].
    #[serde(default)]
    pub git_build_assets: Option<bool>,
    /// Did the user ask this Blank-PHP site for a starter database (v41)?
    ///
    /// Intent, not provenance — [`Site::db_created`] answers "may we drop it",
    /// this answers "was one asked for". They are written at different times by
    /// different actors (this at the insert, by the dialog's answer; that by
    /// the job, after `CREATE DATABASE`), so one column could not carry both
    /// without lying during the window between them — which is exactly the
    /// window a failed job leaves a user sitting in, holding Retry.
    ///
    /// `None` = the question does not apply (WordPress and Laravel always need
    /// one; a linked or cloned docroot is never seeded) or the row predates the
    /// column. Read through [`Site::has_starter_db`], never directly.
    #[serde(default)]
    pub starter_db: Option<bool>,
    /// Is this site SERVED (v44)? `false` = the user stopped this one site.
    ///
    /// A rexenv site is not a process — shared nginx, and a php-fpm pool shared
    /// with every site on its PHP minor — so stopping one is a change to the
    /// SERVING SURFACE, not to any process: no nginx server block, a Caddy route
    /// that keeps its certificate and answers 503, and only a site's OWN override
    /// backend (FrankenPHP/Apache) actually stopped. See
    /// `docs/archive/PLAN-per-site-lifecycle.md`.
    ///
    /// Recorded rather than held in memory because services OUTLIVE the app: a
    /// site the user stopped must still be stopped after a relaunch, and the
    /// config rebuild is reached from start, reload, startup adoption, site
    /// edits and the scratch reaper — one recorded fact is the only thing all of
    /// them can read. USER-OWNED: only [`crate::state::store::set_site_enabled`]
    /// writes it, so provisioning and config sync can never turn a stopped site
    /// back on behind the user.
    ///
    /// Every pre-v44 row reads `true`, which is exact: before the column there
    /// was no way to stop one site.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Site {
    /// The directory the web server roots at: [`Site::path`] joined with
    /// [`Site::docroot_subdir`] when the site has one.
    ///
    /// THE single answer to "what do we serve" — vhost, override backend and
    /// tunnel docroot all call this, so a site whose entry point is a subfolder
    /// can never be served from its project root by one code path while another
    /// gets it right. (`path` remains the project root: what teardown removes,
    /// what composer/artisan run in.)
    pub fn served_root(&self) -> std::path::PathBuf {
        let root = std::path::PathBuf::from(&self.path);
        if self.docroot_subdir.is_empty() {
            root
        } else {
            root.join(&self.docroot_subdir)
        }
    }

    /// Does provisioning run `artisan migrate` for this site (v34)?
    ///
    /// The ONE place the NULL default is decided, so a create and a retry can
    /// never disagree about what "no record" meant.
    pub fn runs_migrations(&self) -> bool {
        self.git_migrate.unwrap_or(true)
    }

    /// Does provisioning build this site's front-end assets (v35)?
    ///
    /// NULL means NO, the opposite of [`Site::runs_migrations`]'s default —
    /// and both are facts about what older rows actually did, not a house
    /// style. Named methods rather than `unwrap_or` at each call site so
    /// nobody has to remember which way each one falls.
    pub fn builds_assets(&self) -> bool {
        self.git_build_assets.unwrap_or(false)
    }

    /// Does provisioning create this site's starter database + seeded table
    /// (v41)? NULL means NO — exact: before the column, a Blank-PHP site got a
    /// `phpinfo()` page and no database at all.
    pub fn has_starter_db(&self) -> bool {
        self.starter_db.unwrap_or(false)
    }

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

/// A site row in PRODUCTION shape for tests in other modules — UUID id, an
/// absolute app-data docroot, a derived `wp_` database name. Shared so a fixture
/// cannot drift into the friendly-looking shape that hides real bugs (the
/// `s-ea`/relative-path lesson): every caller gets the shape the app writes.
#[cfg(test)]
pub(crate) fn test_site(id: &str, domain: &str, origin: SiteOrigin) -> Site {
    Site {
        id: id.into(),
        name: domain.split('.').next().unwrap_or(domain).into(),
        domain: domain.into(),
        site_type: SiteType::Wordpress,
        status: ServiceStatus::Stopped,
        php_version: "8.3".into(),
        web_server: WebServer::Nginx,
        ssl: true,
        path: format!(
            "/Users/x/Library/Application Support/dev.rexenv.rexenv/Sites/{domain}"
        ),
        created_at: "2026-08-01 09:00:00".into(),
        multisite: MultisiteMode::None,
        db_name: format!("wp_{}", domain.replace(['.', '-'], "_")),
        db_engine: SiteDbEngine::Mysql,
        xdebug: false,
        override_port: None,
        provisioned: true,
        docroot_managed: Some(true),
        db_created: None,
        content_dir: None,
        mu_dir_created: None,
        origin,
        agent_client: None,
        expires_at: None,
        docroot_subdir: String::new(),
        git_url: None,
        git_ref: None,
        git_migrate: None,
        git_build_assets: None,
        starter_db: None,
        enabled: true,
    }
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
    /// The user stopped THIS site (v44) — as opposed to the stack being down.
    ///
    /// Carried beside `serving` rather than folded into it because the two
    /// stopped-nesses are different answers to "why is my site not up", and one
    /// word for both sends a user to start a stack that is already running. When
    /// this is true, `serving` is false whatever the stack is doing.
    #[serde(default)]
    pub disabled: bool,
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
    /// The patch the USER chose for this minor, or `None` to follow the app's pin.
    ///
    /// The only value on this row the app cannot derive, which is the only reason
    /// it is stored — v36 deleted a `patch` column precisely because that one
    /// mirrored a compile-time constant (#339/#340/#345). The pin stays the
    /// FLOOR: a selection can only move a minor forward.
    pub selected_patch: Option<String>,
    /// Deterministic loopback FastCGI port of this version's pool.
    pub fpm_port: u16,
    /// Whether this version is enabled (the app starts a pool for it).
    pub installed: bool,
    /// Whether this is the default version for new sites.
    pub is_default: bool,
}

/// A PHP version AS THE UI SEES IT — the stored row plus the facts that are
/// derived from the pinned build set rather than recorded.
///
/// A SEPARATE type on purpose. Hanging `xdebug_supported` off [`PhpVersion`]
/// would make it a field that is true when `core::php::list_versions` built the
/// value and `false` when `store::list_php_versions` did — a recorded-vs-derived
/// disagreement inside one struct, which is a defect family this project already
/// tracks. Here the persistence type simply has no such field, so the wrong
/// answer is unrepresentable rather than merely unlikely.
///
/// It exists because the frontend was hand-copying core's rules: `SiteDetail`
/// disabled the Xdebug toggle on a literal `minor === "8.0"`, so the UI's idea
/// of which minors support Xdebug was a second, silently-diverging copy of
/// `binaries::xdebug_supported` — the "guard covers claimed surface" shape, four
/// of which have already bitten here. The client renders what core computed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpVersionView {
    pub minor: String,
    /// Why this minor cannot be installed on THIS host, when the reason is the
    /// host's macOS (`docs/PLAN-macos-13-floor.md` §6.3): the row is still
    /// listed — disabled, with this sentence — never silently omitted. `None`
    /// for every minor the host's tier offers. The sentence is built in Rust
    /// (`binaries::needs_macos_sentence`), so the UI renders it and adds no OS
    /// word of its own.
    pub unavailable_reason: Option<String>,
    /// The patch this build PINS for the minor — derived from `PHP_VERSIONS`,
    /// never stored. It used to be a column, which meant a mirror that could
    /// disagree with the thing it mirrored (ledger #339/#340).
    pub patch: String,
    /// The patch the live pool is ACTUALLY executing, when that differs from
    /// `patch` — read from the running master's executable (ledger #342).
    ///
    /// `None` covers three different states on purpose, because none of them is
    /// a disagreement: no pool running, a pool running the pinned patch, or a
    /// pool we could not identify. The UI says something extra only when there
    /// is something extra to say. **This field is why deleting the column is a
    /// simplification rather than a cover-up**: without it a failed patch bump
    /// renders as the pin while the pool serves the old bytes — the identical
    /// silent lie ledger #339 was shipped to end, just moved somewhere harder
    /// to see.
    pub serving: Option<String>,
    /// A newer patch php.net says EXISTS for this minor, when one does.
    ///
    /// An upstream fact, never an offer: rexenv installs from static-php.dev,
    /// which lags php.net by weeks, so this can name a version rexenv could not
    /// ship. That is why there is no Update button and why the copy says
    /// "exists" rather than "available" (`core::php_upstream`).
    pub upstream: Option<String>,
    /// A patch a VERIFIED manifest offers for this minor, newer than the pin —
    /// one rexenv can actually install. Distinct from `upstream`, which is only
    /// php.net saying a release EXISTS: that has no button because rexenv may
    /// have no build of it. `None` when no key is pinned, nothing newer is
    /// signed for, or the check has never run.
    pub updatable: Option<String>,
    /// When the upstream list was last fetched successfully (`db_now` format),
    /// or `None` if it never has been. Drives "checked N ago" — a check that
    /// finds nothing must still visibly have run.
    pub upstream_checked_at: Option<String>,
    pub fpm_port: u16,
    pub installed: bool,
    pub is_default: bool,
    /// Whether the per-site Xdebug toggle can be offered for this minor —
    /// `binaries::xdebug_supported`, not a client-side guess.
    pub xdebug_supported: bool,
    /// WHY the toggle is unavailable, as the sentence to show — or `None` when
    /// it is available. `xdebug_supported` says whether; this says why, and the
    /// two answers are not interchangeable: 7.4 and 8.0 physically cannot load
    /// an extension, while a minor whose bottle is merely unpinned is a gap in
    /// rexenv, not a fact about the user's PHP.
    ///
    /// Carried on the row for the same reason `xdebug_supported` is: the UI had
    /// a hardcoded sentence saying "its build can't load extensions" for every
    /// absence — a second copy of a core rule, free to disagree with it, and
    /// already wrong for the first unpinned minor to ship.
    pub xdebug_unavailable_reason: Option<String>,
    /// The Xdebug release this minor's debug pool actually loads, or `None`
    /// where the toggle isn't offered. NOT app-wide: a minor past Xdebug's
    /// support window is frozen at its last release (ledger #320).
    pub xdebug_version: Option<&'static str>,
    /// The date upstream security support ENDED (`YYYY-MM-DD`), or `None` while
    /// this minor is still supported. `Some` = rexenv is offering a runtime that
    /// receives no further security fixes, and the UI must say so before the
    /// user picks it — rexenv shipped 8.0 and 8.1 silently for years.
    pub eol_since: Option<&'static str>,
    /// Whether a site on this minor may choose PostgreSQL —
    /// `php::pdo_pgsql_supported`, carried for exactly the reason
    /// `xdebug_supported` is: the alternative is the New-site dialog deciding it
    /// from a literal list of minors, which is a second copy of a core rule that
    /// is free to disagree with it. It WILL move — the day `rexenv/runtimes`
    /// publishes an 8.0 with the driver, or a new minor arrives — and the copy
    /// that moves last is the one a user meets.
    ///
    /// `false` means a PostgreSQL site on this PHP is refused at create
    /// (`sites::ensure_engine_supports`), so the dialog must not offer what the
    /// backend will reject: an option that produces an error message is worse
    /// than an option that is not there.
    pub postgres_supported: bool,
    /// What applying `updatable` would COST, as the sentence to show — or `None`
    /// when it costs nothing.
    ///
    /// An update offer is normally strictly better, and this row exists because
    /// one kind is not: the patch on offer comes from static-php.dev, and those
    /// builds have no working `pdo_pgsql`, so moving a minor rexenv builds
    /// (8.1-8.5) onto upstream's newer patch REMOVES the PostgreSQL driver from
    /// every site on it. That is invisible in the offer itself — a version
    /// number is a version number — and the failure it produces is a site
    /// hanging on its first query, which is what shipped once already (#550).
    ///
    /// Says what it costs; does not refuse. It is the user's machine and there
    /// are good reasons to take a security patch — but "8.3.33" alone is not a
    /// sentence anybody can weigh, and a cost discovered afterwards is not a
    /// choice they made.
    pub update_cost: Option<String>,
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
    /// Non-empty = LINK this existing folder: served in place, never created,
    /// written into, or deleted with the site.
    pub path: String,
    /// SQL engine for the site's database. Defaults to MySQL so older
    /// callers/blueprints keep working unchanged.
    #[serde(default = "db_engine_mysql")]
    pub db_engine: SiteDbEngine,
    /// Non-empty = CLONE this repository into a docroot rexenv creates (v33).
    ///
    /// The third way to fill a docroot, and mutually exclusive with
    /// [`NewSite::path`]: linking adopts a folder rexenv must never write to,
    /// while this one fills a folder rexenv just made. Cloning *into* someone
    /// else's folder is a different and far more dangerous feature — refused
    /// in [`crate::core::sites::validate_git_source`], not silently ranked.
    #[serde(default)]
    pub git_url: String,
    /// Branch or tag to check out, or `None` for the remote's default.
    #[serde(default)]
    pub git_ref: Option<String>,
    /// Run `artisan migrate` once the app is wired? Only meaningful alongside
    /// [`NewSite::git_url`]. Defaults to TRUE — the database is created by this
    /// same job and is empty, so there is nothing a migration can lose.
    #[serde(default = "default_true")]
    pub git_migrate: bool,
    /// Install and build the repo's front-end assets (`<manager> install` then
    /// `<manager> run build`)? Only meaningful alongside [`NewSite::git_url`].
    /// Defaults FALSE so a caller that never heard of this field cannot make
    /// rexenv run a package manager's install scripts.
    #[serde(default)]
    pub git_build_assets: bool,
    /// Create a starter database + seeded table for a Blank-PHP site, and point
    /// its generated page at them (v41). Only meaningful for a Php site whose
    /// docroot rexenv creates and does not clone into — a linked folder is the
    /// user's, and a clone brings its own code.
    ///
    /// Defaults FALSE so a caller that never heard of this field (the CLI, an
    /// agent, an older blueprint) cannot make rexenv boot a database engine.
    #[serde(default)]
    pub starter_db: bool,
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
            docroot_subdir: String::new(),
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
            starter_db: None,
            enabled: true,
        }
    }

    /// The whole point of `docroot_subdir`: a Laravel project's `.env` — its
    /// database credentials — sits at `path`, one level ABOVE everything the
    /// web server may ever reach. If these two answers were ever the same
    /// value, that file would be a public URL.
    #[test]
    fn served_root_is_the_subdir_and_never_the_project_root_that_holds_dot_env() {
        let mut s = site(SiteOrigin::User, None, Some(true));
        s.site_type = SiteType::Laravel;
        s.path = "/Users/x/rexenv/Sites/shop.rex".into();
        s.docroot_subdir = "public".into();
        assert_eq!(s.served_root(), std::path::PathBuf::from("/Users/x/rexenv/Sites/shop.rex/public"));
        assert!(!s.served_root().starts_with(
            std::path::PathBuf::from("/Users/x/rexenv/Sites/shop.rex").join(".env")
        ));

        // Empty (every pre-v32 row, every WordPress site, every LINKED project
        // whose stored path already points at the folder to serve) means the
        // path itself — appending anything there would serve a missing dir.
        s.docroot_subdir = String::new();
        assert_eq!(s.served_root(), std::path::PathBuf::from("/Users/x/rexenv/Sites/shop.rex"));
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
