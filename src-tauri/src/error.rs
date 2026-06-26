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
