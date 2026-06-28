//! commands::mail — thin Tauri IPC for the Mailpit mail-catcher (§2.1). Call core/ only.

use crate::core::mail;
use crate::error::Result;
use crate::state::app::AppState;
use serde::Serialize;
use tauri::State;

/// Mailpit health + endpoints for the Mail screen.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailpitStatus {
    pub running: bool,
    pub smtp_port: u16,
    pub http_port: u16,
    pub ui_url: String,
}

/// Current Mailpit status (running probe + endpoints).
#[tauri::command]
pub async fn mailpit_status(_state: State<'_, AppState>) -> Result<MailpitStatus> {
    Ok(MailpitStatus {
        running: mail::running(),
        smtp_port: mail::MAILPIT_SMTP_PORT,
        http_port: mail::MAILPIT_HTTP_PORT,
        ui_url: mail::api_base(),
    })
}
