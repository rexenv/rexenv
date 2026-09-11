//! core::app_info — which build is running.
//!
//! Lived in `commands::system` until 11 Sep 2026, when the MCP `stack_status`
//! read needed it: the read bridge (`mcp_server/readctx.rs`) may reach `core::`
//! reads and never `commands::` — the M1 boundary test says so, and it failed the
//! first version that reached for the command. "Is the app I'm talking to the
//! code that was just fixed?" is a question an agent asks too.

use serde::Serialize;

/// Mirrors the frontend `AppInfo` type in `src/types/index.ts`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub tauri_version: String,
    /// Human-readable OS + CPU, e.g. `macOS · Apple silicon` — derived from the
    /// build target, not hardcoded, so it stays correct on Windows/Linux/Intel.
    pub platform: String,
    /// Short git commit this binary was built from, `-dirty` when the tree had
    /// uncommitted changes, `unknown` outside a checkout. Answers "is the app
    /// I'm running the code I just fixed?" — which once cost a whole
    /// misdiagnosis to work out by hand.
    pub commit: String,
    /// UTC build timestamp.
    pub built_at: String,
}

/// A friendly "OS · CPU" label from the compile-time target (`std::env::consts`).
fn platform_label() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    let arch = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "Apple silicon",
        ("macos", "x86_64") => "Intel",
        (_, "aarch64") => "ARM64",
        (_, "x86_64") => "x64",
        (_, other) => other,
    };
    format!("{os} · {arch}")
}

/// The build this process is.
pub fn current() -> AppInfo {
    AppInfo {
        name: "rexenv".into(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tauri_version: tauri::VERSION.to_string(),
        platform: platform_label(),
        commit: env!("REXENV_GIT_COMMIT").to_string(),
        built_at: env!("REXENV_BUILT_AT").to_string(),
    }
}
