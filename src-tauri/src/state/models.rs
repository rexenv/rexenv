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
    Openlitespeed => "openlitespeed",
});

str_enum!(SiteType {
    Wordpress => "wordpress",
    Laravel => "laravel",
    Php => "php",
});

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
}
