//! commands::wordpress — thin Tauri IPC for WordPress detection/management
//! (Phase 3 §1). Resolves the bundled PHP + wp-cli phar and delegates to
//! `core::wordpress`. No business logic here.

use crate::core::wordpress::WpInfo;
use crate::core::{self, binaries, php};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use std::path::{Path, PathBuf};
use tauri::State;

/// Resolve (downloading on first use) the bundled PHP CLI for a site's PHP minor
/// version + the wp-cli `.phar`.
async fn wp_tools(state: &State<'_, AppState>, php_minor: &str) -> Result<(PathBuf, PathBuf)> {
    let patch = php::patch_for_minor(php_minor)
        .ok_or_else(|| Error::Other(format!("no pinned PHP build for {php_minor}")))?;
    let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
    // wp-cli is a .phar (not a Mach-O) → resolve_file (no chmod/codesign).
    let wp_phar =
        binaries::resolve_file(state.platform.as_ref(), "wp-cli", binaries::WP_CLI_VERSION).await?;
    Ok((php_bin, wp_phar))
}

/// Detect whether a site runs WordPress, plus its core version + multisite flag.
/// A non-WordPress docroot (e.g. a Blank-PHP site) returns `isWordpress: false`.
#[tauri::command]
pub async fn wp_info(state: State<'_, AppState>, id: String) -> Result<WpInfo> {
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, &id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?
    };
    let (php_bin, wp_phar) = wp_tools(&state, &site.php_version).await?;
    core::wordpress::wp_info(&php_bin, &wp_phar, Path::new(&site.path))
}
