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
    /// Normalized clone URL.
    pub url: String,
    /// Requested branch/tag at add time (None = the remote default).
    pub git_ref: Option<String>,
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
