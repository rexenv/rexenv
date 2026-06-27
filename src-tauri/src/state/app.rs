//! App-wide state held by Tauri (`app.manage`) and accessed from commands.

use crate::core::monitor::Monitor;
use crate::platform::traits::Platform;
use rusqlite::Connection;
use std::sync::Mutex;

/// Shared application state. The SQLite connection is behind a `Mutex` (rusqlite
/// `Connection` is `Send` but not `Sync`); commands lock it for the call. The
/// platform impl is kept for commands that touch the filesystem (e.g. delete).
/// The `Monitor` is kept across polls so CPU% reflects the polling interval.
pub struct AppState {
    pub db: Mutex<Connection>,
    pub platform: Box<dyn Platform>,
    pub monitor: Mutex<Monitor>,
}

impl AppState {
    pub fn new(conn: Connection, platform: Box<dyn Platform>) -> Self {
        Self {
            db: Mutex::new(conn),
            platform,
            monitor: Mutex::new(Monitor::new()),
        }
    }
}
