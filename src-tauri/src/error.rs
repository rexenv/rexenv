//! Shared error type for the rexenv backend.

use serde::Serialize;

/// All fallible backend operations return this. It is serializable so it can
/// cross the Tauri IPC boundary to the frontend.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("not implemented on this platform: {0}")]
    Unsupported(&'static str),

    #[error("{0}")]
    Other(String),

    /// The app database was migrated by a NEWER rexenv than this build — refused
    /// before anything writes to it (`state::db::refuse_newer_schema`, ledger #593).
    /// Its own variant so the launch screen can say THIS, not the generic
    /// "data folder isn't writable" advice every other open failure gets.
    #[error(
        "This copy of rexenv is older than its data: the database was last opened by a \
         newer rexenv (schema v{found}), and this copy only understands up to v{known}.\n\n\
         Nothing was changed. Open the newer rexenv instead — install it again if it was \
         replaced — and this data keeps working."
    )]
    NewerSchema { found: i64, known: i64 },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Serialize errors as their display string for the frontend.
impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
