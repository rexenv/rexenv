//! App-wide state held by Tauri (`app.manage`) and accessed from commands.

use crate::platform::traits::Platform;
use rusqlite::Connection;
use std::sync::Mutex;

/// Shared application state. The SQLite connection is behind a `Mutex` (rusqlite
/// `Connection` is `Send` but not `Sync`); commands lock it for the call. The
/// platform impl is kept for commands that touch the filesystem (e.g. delete).
pub struct AppState {
    pub db: Mutex<Connection>,
    pub platform: Box<dyn Platform>,
}

impl AppState {
    pub fn new(conn: Connection, platform: Box<dyn Platform>) -> Self {
        Self {
            db: Mutex::new(conn),
            platform,
        }
    }
}
