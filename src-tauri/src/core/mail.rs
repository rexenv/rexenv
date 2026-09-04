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

/// The same shim for the PHP **CLI**'s `-d sendmail_path=…`, where the quoting
/// above does not survive.
///
/// **Two forms for one command, and the difference is measured, not stylistic.**
/// php-fpm reads `php_admin_value[sendmail_path]` out of a config file and keeps
/// the single quotes, so the pool's shell sees a quoted path. The CLI's `-d`
/// parser STRIPS them: with [`sendmail_path`]'s value, `ini_get` returns the
/// path with its quotes gone, `/bin/sh -c` then splits it on the space in
/// `Application Support`, and the result is
/// `sh: /Users/…/Library/Application: Permission denied` with `mail()` returning
/// false. Measured against the real binaries on 25 Aug 2026 — the first attempt
/// at routing wp-cli's mail shipped `sendmail_path`'s quoting and did not work.
///
/// So this escapes for `sh` instead of quoting: every byte outside a
/// conservative safe set gets a backslash, which survives `-d`'s unquoting
/// because there are no quotes to remove.
pub fn sendmail_path_cli(mailpit_bin: &Path) -> String {
    format!(
        "{} sendmail -t -S 127.0.0.1:{MAILPIT_SMTP_PORT}",
        sh_escape(&mailpit_bin.display().to_string())
    )
}

/// Backslash-escape everything `/bin/sh` could treat as special.
///
/// An allow-list, not a deny-list: the characters a path may contain unescaped
/// are enumerated, and everything else — spaces, quotes, `$`, backticks,
/// `;`, `&`, `|`, newlines — is escaped. A deny-list would need to be complete
/// to be correct, and the one character it forgot would be the one that
/// mattered.
fn sh_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            let safe = c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+' | ':' | '=' | ',' | '@' | '%');
            let mut out = Vec::new();
            if !safe && c.is_ascii() {
                out.push('\\');
            }
            out.push(c);
            out
        })
        .collect()
}

/// Setting key for the mail catch-all (`true`/`false`; ABSENT means on).
///
/// Absent-is-on is deliberate and is the only reading that keeps the promise
/// true for sites that predate the feature: a default read off a missing row
/// would otherwise make "every site's mail is caught" mean "every site created
/// after the user found the switch".
pub const CATCH_ALL_KEY: &str = "mail.catch_all";

/// Whether rexenv forces every site's outgoing mail into Mailpit.
///
/// ON is the default because a local site mailing the real world is the failure
/// this whole subsystem exists to prevent — a developer testing a password
/// reset should not need to know that their app was configured, months ago, to
/// talk to a real SMTP provider. OFF exists because the opposite is also a real
/// task: deliberately proving a live SES/Postmark integration from a local box.
/// A read error is treated as ON for the same reason the absent row is.
pub fn catch_all_enabled(conn: &rusqlite::Connection) -> bool {
    !matches!(
        crate::state::store::get_setting(conn, CATCH_ALL_KEY),
        Ok(Some(ref v)) if v == "false"
    )
}

/// The environment that points a **Laravel** app at Mailpit — and OUTRANKS the
/// app's own `.env`.
///
/// # Why the environment and not the file
///
/// `sendmail_path` cannot reach Laravel at all. Laravel's sendmail transport
/// does not consult php.ini; `config/mail.php` ships
/// `'path' => env('MAIL_SENDMAIL_PATH', '/usr/sbin/sendmail -bs -i')`, so the
/// pool's shim — the mechanism that catches WordPress's `mail()` — is invisible
/// to it. Measured 4 Sep 2026 on a real site: `MAIL_MAILER=sendmail` +
/// `Mail::raw(...)` exited 0, reported success, and Mailpit received nothing.
/// Silent success is the shape this tree treats as a defect.
///
/// The lever that DOES work is Laravel's own env precedence: `LoadEnvironment-
/// Variables` builds an **immutable** Dotenv repository, so a variable already
/// present in the process environment is never overwritten by `.env`. Setting
/// these in the php-fpm pool and in the artisan runner therefore beats a real
/// `MAIL_HOST=smtp.gmail.com` in the developer's own file, which is exactly the
/// case the catch-all is for. Verified live the same day: `.env` said
/// `MAIL_MAILER=log`, the environment said smtp/11025, and the message arrived
/// in Mailpit.
///
/// # Why these nine keys
///
/// Each one is a way an app can escape, not a synonym for the last:
/// - `MAIL_MAILER` / `MAIL_DRIVER` — the same choice under Laravel >= 7 and
///   <= 6. An old app reads only the second, and would keep its own mailer.
/// - `MAIL_URL` — Laravel 11 lets one DSN override host, port and credentials
///   together. Left alone it silently wins over everything below it.
/// - `MAIL_HOST` / `MAIL_PORT` — the sink.
/// - `MAIL_USERNAME` / `MAIL_PASSWORD` — `"null"`, which Laravel's `Env` maps
///   to a real null, so a stray credential cannot make Mailpit refuse the
///   session by attempting AUTH against a server that offers none.
/// - `MAIL_SCHEME` / `MAIL_ENCRYPTION` — the modern and legacy spellings of
///   "no TLS". Mailpit's SMTP listener is plaintext; a TLS attempt fails
///   closed, and a mail that fails is a mail the developer never sees.
///
/// Returned rather than written by each caller for the [`sendmail_path`]
/// reason: one definition, two renderings (a pool config, a process
/// environment), so the pool and the CLI cannot drift into disagreeing about
/// where a site's mail goes.
pub fn laravel_env() -> Vec<(&'static str, String)> {
    vec![
        ("MAIL_MAILER", "smtp".to_string()),
        ("MAIL_DRIVER", "smtp".to_string()),
        ("MAIL_URL", "null".to_string()),
        ("MAIL_HOST", "127.0.0.1".to_string()),
        ("MAIL_PORT", MAILPIT_SMTP_PORT.to_string()),
        ("MAIL_USERNAME", "null".to_string()),
        ("MAIL_PASSWORD", "null".to_string()),
        ("MAIL_SCHEME", "smtp".to_string()),
        ("MAIL_ENCRYPTION", "null".to_string()),
    ]
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

/// The Mailpit search term that selects unread messages — the inbox filter's
/// whole implementation, and it lives HERE rather than in the frontend because
/// this is where the query string is built either way. Server-side by design:
/// filtering the returned page in the UI would only hide the unread messages
/// that happen to be on it, which is exactly the "I can't find the new ones"
/// problem the filter exists for.
pub const UNREAD_QUERY: &str = "is:unread";

/// Combine the user's search text with the unread filter, or return `None` when
/// neither is asked for (so the caller keeps using the plain listing endpoint).
/// PURE — unit-tested, because the two inputs compose in the one place that can
/// get it wrong: a filter that replaced the search would silently widen the
/// result the moment both are on.
pub fn search_query(search: Option<&str>, unread_only: bool) -> Option<String> {
    let text = search.map(str::trim).filter(|q| !q.is_empty());
    match (text, unread_only) {
        (Some(q), true) => Some(format!("{q} {UNREAD_QUERY}")),
        (Some(q), false) => Some(q.to_string()),
        (None, true) => Some(UNREAD_QUERY.to_string()),
        (None, false) => None,
    }
}

/// Mark EVERY captured message read (`PUT /api/v1/messages`, no IDs and no
/// search — Mailpit's documented "then all mailbox messages are updated").
///
/// The empty body is the whole point here and a trap everywhere else: the same
/// shape on `DELETE` means "delete everything", which is why [`delete`] refuses
/// an empty ID list rather than falling through to a wipe. Marking all read is
/// spelled as its OWN function for that reason — there is no id-taking variant
/// that can be called with an empty list and quietly do this.
pub async fn mark_all_read() -> Result<()> {
    let url = format!("{}/api/v1/messages", api_base());
    client()
        .put(&url)
        .json(&serde_json::json!({ "Read": true }))
        .send()
        .await
        .map_err(|e| Error::Other(format!("mailpit PUT {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("mailpit PUT {url}: {e}")))?;
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
    fn the_unread_filter_narrows_a_search_instead_of_replacing_it() {
        // Both on: the terms COMPOSE. A filter that replaced the search would
        // widen the list at the exact moment the user was narrowing it — and
        // it would look like it worked, because unread mail did appear.
        assert_eq!(search_query(Some("invoice"), true).as_deref(), Some("invoice is:unread"));
        assert_eq!(search_query(Some("invoice"), false).as_deref(), Some("invoice"));
        assert_eq!(search_query(None, true).as_deref(), Some("is:unread"));
        // Neither: None, so the caller keeps the plain listing endpoint rather
        // than searching for an empty string.
        assert_eq!(search_query(None, false), None);
        assert_eq!(search_query(Some("   "), false), None);
        assert_eq!(search_query(Some("  "), true).as_deref(), Some("is:unread"));
        // Whitespace around real text is trimmed, not carried into the query.
        assert_eq!(search_query(Some(" hi "), true).as_deref(), Some("hi is:unread"));
    }

    /// **The CLI shim survives `-d`'s unquoting; the fpm one does not.**
    ///
    /// `sendmail_path` single-quotes the binary, which php-fpm's config parser
    /// keeps. The CLI's `-d` STRIPS quotes, so that form reaches `/bin/sh` as an
    /// unquoted path and splits on the space in `Application Support` —
    /// `sh: /Users/…/Library/Application: Permission denied`, `mail()` false,
    /// message gone. Measured on the real binaries 25 Aug 2026; the first
    /// attempt at routing wp-cli's mail shipped the quoted form and did not work.
    #[test]
    fn the_cli_shim_escapes_instead_of_quoting_because_minus_d_strips_quotes() {
        let bin = std::path::Path::new("/Users/x/Library/Application Support/rexenv/bin/mailpit");
        let cli = sendmail_path_cli(bin);
        assert!(!cli.contains('\''), "a quote here is removed by `-d` and the path then splits: {cli}");
        assert!(
            cli.contains("Application\\ Support"),
            "the space must be backslash-escaped, or sh splits the path: {cli}"
        );
        assert!(cli.ends_with(&format!("sendmail -t -S 127.0.0.1:{MAILPIT_SMTP_PORT}")), "{cli}");

        // The fpm form keeps its quotes — the two are deliberately different,
        // and asserting both here is what stops someone "unifying" them.
        let fpm = sendmail_path(bin);
        assert!(fpm.starts_with('\''), "the fpm form is quoted for the config parser: {fpm}");

        // Shell metacharacters in a path are escaped, not just spaces: a
        // deny-list would have to be complete to be right.
        let nasty = std::path::Path::new("/tmp/a b;c$d`e'f\"g/mailpit");
        let esc = sendmail_path_cli(nasty);
        for ch in [' ', ';', '$', '`', '\'', '"'] {
            let at = esc.find(ch).unwrap_or_else(|| panic!("{ch:?} vanished from {esc}"));
            assert_eq!(
                esc.as_bytes()[at - 1], b'\\',
                "{ch:?} reached the shell unescaped in {esc}"
            );
        }
    }

    /// **Absent means ON, and only the exact string `false` turns it off.**
    ///
    /// The default is read on the "sites that predate the switch" case, which
    /// is the one an absent row actually describes. A `bool::from_str`-shaped
    /// reading — anything-but-`true` is off — would silently un-catch every
    /// site on the machine the day this shipped, and it would look like a
    /// working default because a fresh install writes the row.
    #[test]
    fn the_catch_all_is_on_until_something_says_the_word_false() {
        let c = crate::state::db::open_in_memory().unwrap();
        assert!(catch_all_enabled(&c), "an absent row must mean caught, not delivered");

        crate::state::store::set_setting(&c, CATCH_ALL_KEY, "false").unwrap();
        assert!(!catch_all_enabled(&c));

        crate::state::store::set_setting(&c, CATCH_ALL_KEY, "true").unwrap();
        assert!(catch_all_enabled(&c));

        // Garbage is not "off". Anything we cannot read as a deliberate opt-out
        // leaves the mail caught, because that is the recoverable direction:
        // the developer sees a message they expected to leave, not a customer
        // receiving one from a laptop.
        for junk in ["", "0", "no", "FALSE", "off"] {
            crate::state::store::set_setting(&c, CATCH_ALL_KEY, junk).unwrap();
            assert!(catch_all_enabled(&c), "{junk:?} is not the opt-out");
        }

        crate::state::store::delete_setting(&c, CATCH_ALL_KEY).unwrap();
        assert!(catch_all_enabled(&c));
    }

    /// **Every escape hatch a Laravel app has is closed, not just the obvious one.**
    ///
    /// The nine keys are nine different ways an app can end up mailing
    /// somewhere else; a set that covered only `MAIL_MAILER`/`MAIL_HOST` would
    /// look right and lose to a `MAIL_URL` DSN or to a Laravel 6 app reading
    /// `MAIL_DRIVER`. Asserting the whole set here is what stops the list being
    /// trimmed to the ones someone happened to test.
    #[test]
    fn the_laravel_env_closes_every_route_out_of_mailpit() {
        let env = laravel_env();
        let get = |k: &str| {
            env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str())
        };
        // The sink itself.
        assert_eq!(get("MAIL_HOST"), Some("127.0.0.1"));
        assert_eq!(get("MAIL_PORT"), Some(MAILPIT_SMTP_PORT.to_string().as_str()));
        // Both spellings of "which mailer" — Laravel >= 7 and <= 6.
        assert_eq!(get("MAIL_MAILER"), Some("smtp"));
        assert_eq!(get("MAIL_DRIVER"), Some("smtp"));
        // A DSN would override host AND port AND credentials in one key.
        assert_eq!(get("MAIL_URL"), Some("null"));
        // AUTH against a server offering none fails the whole session.
        assert_eq!(get("MAIL_USERNAME"), Some("null"));
        assert_eq!(get("MAIL_PASSWORD"), Some("null"));
        // Mailpit's listener is plaintext; TLS fails closed, and a mail that
        // fails is a mail the developer never sees.
        assert_eq!(get("MAIL_SCHEME"), Some("smtp"));
        assert_eq!(get("MAIL_ENCRYPTION"), Some("null"));
        // No key appears twice: the pool config would emit two `env[]` lines
        // and php-fpm takes the last, so a duplicate is a silent coin-flip.
        let mut names: Vec<_> = env.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate key in laravel_env()");
    }

    /// The port is READ from the constant, never spelled again. A literal here
    /// would keep passing on the day the sink moved.
    #[test]
    fn the_laravel_env_reads_the_smtp_port_rather_than_restating_it() {
        let env = laravel_env();
        let port = env.iter().find(|(n, _)| *n == "MAIL_PORT").unwrap().1.clone();
        assert_eq!(port.parse::<u16>().unwrap(), MAILPIT_SMTP_PORT);
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
