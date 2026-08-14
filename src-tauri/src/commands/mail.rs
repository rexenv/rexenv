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

/// Start Mailpit alone (Services-row toggle). Independent of the serving core:
/// pools route mail to its FIXED SMTP port via the sendmail shim, so it can
/// come and go without touching them.
///
/// Prefetch Mailpit's binary BEFORE taking the services lock (§5
/// prefetch-before-lock): a cold-cache toggle (Mailpit switched on before any
/// Start-all, onboarding prefetch skipped/failed/offline) would otherwise
/// download INSIDE the lock, freezing every status read (which needs `try_lock`
/// on the same lock). On a warm cache `prefetch` is a cache hit and sets no
/// state, so the locked spawn below is unchanged. Spawn under the lock, await
/// readiness with it released (M4).
#[tauri::command]
pub async fn start_mail(state: State<'_, AppState>) -> Result<()> {
    let plan = crate::core::downloads::plan_for_mailpit(state.platform.as_ref());
    crate::core::downloads::prefetch(state.platform.as_ref(), "Start Mailpit", &plan).await?;
    let check = {
        let mut mgr = state.services.lock().await;
        mgr.spawn_mailpit(state.platform.as_ref()).await?
    };
    crate::core::service_manager::await_ready(check.into_iter().collect()).await
}

/// Stop Mailpit alone. Mail sent while it's down is dropped by the shim —
/// that's the same failure mode as any stopped mail catcher.
#[tauri::command]
pub async fn stop_mail(state: State<'_, AppState>) -> Result<()> {
    let mut mgr = state.services.lock().await;
    mgr.stop_mailpit(state.platform.as_ref())
}

/// Inbox listing, optionally filtered by a Mailpit search query and/or the
/// unread filter (§2.3).
///
/// `unread_only` is a flag rather than something the UI splices into `query`
/// itself: the two compose in ONE place (`mail::search_query`), so the filter
/// can never replace the search — which would widen the list at the moment the
/// user was narrowing it, and look like it worked.
#[tauri::command]
pub async fn mailpit_messages(
    _state: State<'_, AppState>,
    query: Option<String>,
    unread_only: Option<bool>,
) -> Result<mail::MailList> {
    let q = mail::search_query(query.as_deref(), unread_only.unwrap_or(false));
    mail::list(q.as_deref()).await
}

/// Mark every captured message read ("Mark all read"). Not per-id on purpose —
/// see `mail::mark_all_read` for why the all-messages case is its own function.
#[tauri::command]
pub async fn mailpit_mark_all_read(_state: State<'_, AppState>) -> Result<()> {
    mail::mark_all_read().await
}

/// One message (body + headers) for the preview pane; marks it read.
#[tauri::command]
pub async fn mailpit_message(_state: State<'_, AppState>, id: String) -> Result<mail::MailDetail> {
    mail::detail(&id).await
}

/// Raw RFC-822 source of a message.
#[tauri::command]
pub async fn mailpit_message_raw(_state: State<'_, AppState>, id: String) -> Result<String> {
    mail::raw(&id).await
}

/// Delete all captured messages ("Clear all").
#[tauri::command]
pub async fn mailpit_clear(_state: State<'_, AppState>) -> Result<()> {
    mail::delete_all().await
}

/// Delete specific messages by ID (row delete / bulk selection delete).
#[tauri::command]
pub async fn mailpit_delete(_state: State<'_, AppState>, ids: Vec<String>) -> Result<()> {
    mail::delete(&ids).await
}
