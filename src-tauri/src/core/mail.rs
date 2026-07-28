//! core::mail — Mailpit mail-catching service (Phase 3 §2.1).
//!
//! Mailpit is a single static Go binary that runs an SMTP sink plus a web UI /
//! HTTP API. We bind both to loopback on fixed ports (SMTP 11025, HTTP/API
//! 18025 — offset from Mailpit's stock 1025/8025 so we don't collide with a
//! standalone Mailpit/MailHog or Herd Pro's bundled Mailpit on THEIR stock
//! ports. Their defaults, not a guarantee — ledger #149; `ports::ensure_free`
//! is the actual guard either way)
//! and persist captured mail to a SQLite file under app-data so it survives
//! restarts. Supervised like the other services via `ProcessSupervisor`.
//! Platform-agnostic: talks to `platform/` traits only.

use crate::core::ports;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Child;

/// SMTP bind port — where php-fpm's sendmail shim delivers (§2.2).
/// NOT Mailpit's stock 1025: rexenv-offset, like every other internal port.
pub const MAILPIT_SMTP_PORT: u16 = 11025;
/// HTTP port — the web UI and the REST API (`/api/v1/…`), and our health probe.
/// NOT Mailpit's stock 8025: rexenv-offset, like every other internal port.
pub const MAILPIT_HTTP_PORT: u16 = 18025;

/// Mailpit's persistent message store under app-data.
pub fn data_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("mailpit"))
}

/// Base URL of the Mailpit HTTP API / web UI.
pub fn api_base() -> String {
    format!("http://127.0.0.1:{MAILPIT_HTTP_PORT}")
}

/// Connect bound for the Mailpit API client — instant on a healthy loopback.
const MAIL_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
/// Total-request bound. A WALL-CLOCK cap is correct here (unlike downloads,
/// the B34 lesson): every Mailpit API response is small and bounded — the
/// largest body is one raw email, sub-second on loopback — so no healthy
/// request approaches this, while a wedged Mailpit (port bound, accept or
/// response stalled) surfaces as an error instead of hanging the Mail screen
/// forever (B25).
const MAIL_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Build a Mailpit API client with the given total-request bound (separate from
/// [`client`] so tests can prove the bound bites without waiting out 15s).
fn build_client(total: std::time::Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(MAIL_CONNECT_TIMEOUT)
        .timeout(total)
        .build()
        .expect("mailpit http client")
}

/// The ONE shared Mailpit API client (connection pool reused across calls —
/// previously every call built its own `Client::new()`), with both bounds applied.
fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| build_client(MAIL_REQUEST_TIMEOUT))
}

/// The php-fpm `sendmail_path` shim that routes a site's PHP `mail()` into
/// Mailpit's SMTP sink (§2.2): Mailpit's own `sendmail` subcommand aimed at the
/// local SMTP port. The binary path is single-quoted (app-data paths contain
/// spaces) since PHP runs this via `/bin/sh -c`. `-t` is accepted for sendmail
/// compatibility; `-S` selects the SMTP server.
pub fn sendmail_path(mailpit_bin: &Path) -> String {
    format!(
        "'{}' sendmail -t -S 127.0.0.1:{MAILPIT_SMTP_PORT}",
        mailpit_bin.display()
    )
}

/// Start the Mailpit server (loopback SMTP + HTTP, persistent DB) via
/// `ProcessSupervisor`; stdout/stderr go to a per-service log.
pub fn start(platform: &dyn Platform, mailpit_bin: &Path) -> Result<Child> {
    let dir = data_dir(platform)?;
    std::fs::create_dir_all(&dir)?;
    let db = dir.join("mailpit.db");
    let args = vec![
        "--listen".to_string(),
        format!("127.0.0.1:{MAILPIT_HTTP_PORT}"),
        "--smtp".to_string(),
        format!("127.0.0.1:{MAILPIT_SMTP_PORT}"),
        "--database".to_string(),
        db.display().to_string(),
        "--quiet".to_string(),
    ];
    let log = platform.paths().log_dir()?.join("mailpit-stdout.log");
    platform.supervisor().spawn_logged(mailpit_bin, &args, &log)
}

/// Stop a running Mailpit by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// True if Mailpit's HTTP port is accepting connections (the service is up).
pub fn running() -> bool {
    ports::is_listening(MAILPIT_HTTP_PORT)
}

// ── HTTP API client (§2.3) ───────────────────────────────────────────────────
//
// The Mail screen reads Mailpit through these so the UI never calls the API
// directly (typed-IPC rule). We talk Mailpit's REST API (`/api/v1/…`) over
// loopback and reshape its PascalCase JSON into camelCase DTOs for the frontend.

/// An email address (display name may be empty).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAddress {
    pub name: String,
    pub address: String,
}

/// One message as shown in the inbox list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailSummary {
    pub id: String,
    pub from: MailAddress,
    pub to: Vec<MailAddress>,
    pub subject: String,
    pub created: String,
    pub read: bool,
    pub snippet: String,
}

/// The inbox listing (counts + the page of messages).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailList {
    pub total: i64,
    pub unread: i64,
    pub messages: Vec<MailSummary>,
}

/// One header (values joined when a header repeats).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailHeader {
    pub name: String,
    pub value: String,
}

/// A full message for the preview pane.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailDetail {
    pub id: String,
    pub from: MailAddress,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub subject: String,
    pub date: String,
    pub text: String,
    pub html: String,
    pub headers: Vec<MailHeader>,
}

// Wire shapes (Mailpit's PascalCase JSON) — kept private; mapped to the DTOs above.
#[derive(Deserialize, Default)]
struct WireAddr {
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Address", default)]
    address: String,
}
#[derive(Deserialize)]
struct WireSummary {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "From")]
    from: Option<WireAddr>,
    #[serde(rename = "To", default)]
    to: Option<Vec<WireAddr>>,
    #[serde(rename = "Subject", default)]
    subject: String,
    #[serde(rename = "Created", default)]
    created: String,
    #[serde(rename = "Read", default)]
    read: bool,
    #[serde(rename = "Snippet", default)]
    snippet: String,
}
#[derive(Deserialize)]
struct WireList {
    #[serde(rename = "total", default)]
    total: i64,
    #[serde(rename = "unread", default)]
    unread: i64,
    #[serde(rename = "messages", default)]
    messages: Vec<WireSummary>,
}
#[derive(Deserialize)]
struct WireDetail {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "From")]
    from: Option<WireAddr>,
    #[serde(rename = "To", default)]
    to: Option<Vec<WireAddr>>,
    #[serde(rename = "Cc", default)]
    cc: Option<Vec<WireAddr>>,
    #[serde(rename = "Subject", default)]
    subject: String,
    #[serde(rename = "Date", default)]
    date: String,
    #[serde(rename = "Text", default)]
    text: String,
    #[serde(rename = "HTML", default)]
    html: String,
}

impl From<WireAddr> for MailAddress {
    fn from(a: WireAddr) -> Self {
        MailAddress { name: a.name, address: a.address }
    }
}
fn addrs(v: Option<Vec<WireAddr>>) -> Vec<MailAddress> {
    v.unwrap_or_default().into_iter().map(Into::into).collect()
}

async fn get_text(url: &str) -> Result<String> {
    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Other(format!("mailpit GET {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("mailpit GET {url}: {e}")))?;
    resp.text()
        .await
        .map_err(|e| Error::Other(format!("mailpit read {url}: {e}")))
}

/// List captured messages, optionally filtered by a Mailpit search `query`
/// (empty ⇒ the full inbox, newest first).
pub async fn list(query: Option<&str>) -> Result<MailList> {
    let url = match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => {
            let enc: String = url_encode(q);
            format!("{}/api/v1/search?query={enc}", api_base())
        }
        None => format!("{}/api/v1/messages", api_base()),
    };
    let body = get_text(&url).await?;
    let wire: WireList =
        serde_json::from_str(&body).map_err(|e| Error::Other(format!("mailpit list parse: {e}")))?;
    Ok(MailList {
        total: wire.total,
        unread: wire.unread,
        messages: wire
            .messages
            .into_iter()
            .map(|m| MailSummary {
                id: m.id,
                from: m.from.unwrap_or_default().into(),
                to: addrs(m.to),
                subject: m.subject,
                created: m.created,
                read: m.read,
                snippet: m.snippet,
            })
            .collect(),
    })
}

/// Fetch one message (body + headers) for the preview pane. Fetching it marks it
/// read in Mailpit (its API side effect).
pub async fn detail(id: &str) -> Result<MailDetail> {
    let base = api_base();
    let body = get_text(&format!("{base}/api/v1/message/{id}")).await?;
    let wire: WireDetail = serde_json::from_str(&body)
        .map_err(|e| Error::Other(format!("mailpit message parse: {e}")))?;

    // Headers come from a separate endpoint as { name: [values…] }.
    let hdr_body = get_text(&format!("{base}/api/v1/message/{id}/headers")).await?;
    let raw_headers: BTreeMap<String, Vec<String>> =
        serde_json::from_str(&hdr_body).unwrap_or_default();
    let headers = raw_headers
        .into_iter()
        .map(|(name, vals)| MailHeader { name, value: vals.join(", ") })
        .collect();

    Ok(MailDetail {
        id: wire.id,
        from: wire.from.unwrap_or_default().into(),
        to: addrs(wire.to),
        cc: addrs(wire.cc),
        subject: wire.subject,
        date: wire.date,
        text: wire.text,
        html: wire.html,
        headers,
    })
}

/// The raw RFC-822 source of a message (the preview's "Raw" tab).
pub async fn raw(id: &str) -> Result<String> {
    get_text(&format!("{}/api/v1/message/{id}/raw", api_base())).await
}

/// Delete every captured message (`DELETE /api/v1/messages`).
pub async fn delete_all() -> Result<()> {
    let url = format!("{}/api/v1/messages", api_base());
    client()
        .delete(&url)
        .send()
        .await
        .map_err(|e| Error::Other(format!("mailpit DELETE {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("mailpit DELETE {url}: {e}")))?;
    Ok(())
}

/// Delete specific captured messages (`DELETE /api/v1/messages` with an IDs
/// body). NOTE: Mailpit treats an EMPTY ID list as "delete everything", so an
/// accidental empty selection must not fall through to a wipe — reject it.
pub async fn delete(ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        return Err(Error::Other("mailpit delete: no message IDs given".into()));
    }
    let url = format!("{}/api/v1/messages", api_base());
    client()
        .delete(&url)
        .json(&serde_json::json!({ "IDs": ids }))
        .send()
        .await
        .map_err(|e| Error::Other(format!("mailpit DELETE {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("mailpit DELETE {url}: {e}")))?;
    Ok(())
}

/// Minimal percent-encoding for a search query (keeps unreserved chars; encodes
/// the rest as %XX). Avoids a url crate dependency for this one small use.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_base_targets_loopback_http_port() {
        assert_eq!(api_base(), "http://127.0.0.1:18025");
    }

    #[test]
    fn ports_are_distinct() {
        assert_ne!(MAILPIT_SMTP_PORT, MAILPIT_HTTP_PORT);
    }

    #[test]
    fn url_encode_keeps_unreserved_escapes_rest() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a@b.test"), "a%40b.test");
        assert_eq!(url_encode("Az0-_.~"), "Az0-_.~");
    }

    #[test]
    fn sendmail_path_quotes_binary_and_targets_smtp() {
        let shim = sendmail_path(Path::new("/App Support/bin/mailpit"));
        // Binary path single-quoted (it contains a space).
        assert!(shim.starts_with("'/App Support/bin/mailpit' sendmail"));
        assert!(shim.contains("-t"));
        assert!(shim.contains("-S 127.0.0.1:11025"));
    }

    // (No constant-echo test for the two timeout values: asserting a const
    // equals its definition proves nothing — the wedged-server test below is
    // the behavioral proof that the bound exists, and the rationale for the
    // magnitudes lives on the constants themselves.)

    #[tokio::test]
    async fn mail_client_times_out_against_a_wedged_server() {
        // A Mailpit that accepts the connection then never answers (the wedged
        // shape) must ERROR within the bound — previously reqwest::get with no
        // timeout hung the Mail screen forever (B25). Built with a short bound
        // via the same constructor the shared client uses, so the test proves
        // the construction enforces the timeout without waiting out 15s.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                drop(stream);
            }
        });
        let start = std::time::Instant::now();
        let r = build_client(std::time::Duration::from_millis(300))
            .get(format!("http://{addr}/api/v1/messages"))
            .send()
            .await;
        assert!(r.is_err(), "a wedged server must error, not hang");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(3),
            "bounded by the timeout, not the server: {:?}",
            start.elapsed()
        );
    }
}
