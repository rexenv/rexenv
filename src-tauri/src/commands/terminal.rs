//! commands::terminal — Tauri IPC for the built-in terminal (§4.1).
//!
//! Owns the live PTY sessions (a Tauri-managed registry) and bridges core's
//! callback output to per-session Tauri events (`terminal://output/<id>`). Thin:
//! resolves the site docroot + bundled PHP/WP-CLI, then delegates to
//! `core::terminal`.

use crate::core::terminal::{PtyConfig, TerminalSession};
use crate::core::{binaries, php, sites, terminal};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

/// Tauri-managed registry of open PTY sessions, keyed by session id.
#[derive(Default)]
pub struct Terminals(Mutex<HashMap<String, TerminalSession>>);

/// Per-session output event name (the frontend listens on this).
pub fn output_event(id: &str) -> String {
    format!("terminal://output/{id}")
}

/// Which plugin/theme folder a session should start in, when it was opened from
/// a WordPress asset row instead of the site's Terminal tab. The PATH never
/// crosses IPC — only the kind + slug, resolved against the site's RECORDED
/// content dir on this side, so the frontend cannot name a directory.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    /// "plugin" | "theme" — validated by `repo::asset_dest`.
    pub kind: String,
    /// The asset's folder name (`wp plugin list`'s `name`).
    pub name: String,
}

/// Open a shell in a site's docroot with bundled PHP + a `wp` wrapper on PATH.
/// `asset` starts it in that plugin/theme's folder instead (the terminal button
/// on a WordPress row). Returns the new session id; output streams via
/// [`output_event`].
#[tauri::command]
pub async fn terminal_open(
    app: AppHandle,
    state: State<'_, AppState>,
    terms: State<'_, Terminals>,
    site_id: String,
    rows: u16,
    cols: u16,
    asset: Option<AssetRef>,
) -> Result<String> {
    // Resolve the site + its PHP version (lock the DB briefly, never across await).
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        sites::get(&conn, &site_id)?.ok_or_else(|| Error::Other(format!("no site {site_id}")))?
    };
    let minor = php::minor_of(&site.php_version);
    // The patch the site's POOL runs, not the pin. `php -v` in a site's own
    // terminal disagreeing with the site it belongs to is the whole bug.
    let (patch, mail_env) = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        // The catch-all's terminal half. `php artisan` typed here is the most
        // likely way a developer makes a Laravel site send mail, and it would
        // otherwise be the one surface that still delivered it for real.
        (php::patch_to_run(&conn, &minor)?, crate::core::laravel::mail_env(&conn))
    };

    let platform = state.platform.as_ref();
    let php_bin = binaries::resolve(platform, "php", &patch).await?;
    let wp_phar = binaries::resolve_file(platform, "wp-cli", binaries::WP_CLI_VERSION).await?;
    let wp_dir = terminal::ensure_wp_wrapper(platform, &php_bin, &wp_phar)?;
    let php_dir = php_bin
        .parent()
        .ok_or_else(|| Error::Other("php binary has no parent dir".into()))?
        .to_path_buf();

    let cwd = match &asset {
        Some(a) => terminal::asset_cwd(
            Path::new(&site.path),
            site.content_dir_rel(),
            &a.kind,
            &a.name,
        )?,
        None => PathBuf::from(&site.path),
    };

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let id = uuid::Uuid::new_v4().to_string();
    let app_handle = app.clone();
    let event = output_event(&id);

    let session = TerminalSession::open(
        PtyConfig {
            cwd,
            shell,
            path_prepend: vec![php_dir, wp_dir],
            env: mail_env,
            rows,
            cols,
        },
        move |chunk| {
            // Best-effort: forward each output chunk as a Tauri event.
            let _ = app_handle.emit(&event, chunk);
        },
    )?;

    terms
        .0
        .lock()
        .map_err(|_| Error::Other("terminal registry poisoned".into()))?
        .insert(id.clone(), session);
    Ok(id)
}

/// Write input (keystrokes / pasted text) to a session's shell.
#[tauri::command]
pub fn terminal_write(terms: State<'_, Terminals>, id: String, data: String) -> Result<()> {
    let map = terms
        .0
        .lock()
        .map_err(|_| Error::Other("terminal registry poisoned".into()))?;
    let session = map.get(&id).ok_or_else(|| Error::Other(format!("no terminal {id}")))?;
    session.write(data.as_bytes())
}

/// Resize a session's PTY (on viewport change).
#[tauri::command]
pub fn terminal_resize(terms: State<'_, Terminals>, id: String, rows: u16, cols: u16) -> Result<()> {
    let map = terms
        .0
        .lock()
        .map_err(|_| Error::Other("terminal registry poisoned".into()))?;
    let session = map.get(&id).ok_or_else(|| Error::Other(format!("no terminal {id}")))?;
    session.resize(rows, cols)
}

/// Close a session (kill the shell + drop it from the registry).
#[tauri::command]
pub fn terminal_close(terms: State<'_, Terminals>, id: String) -> Result<()> {
    let session = terms
        .0
        .lock()
        .map_err(|_| Error::Other("terminal registry poisoned".into()))?
        .remove(&id);
    if let Some(s) = session {
        let _ = s.kill();
    }
    Ok(())
}
