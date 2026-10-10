//! The rexsync1 client — rexenv's side of every request (`docs/rexsync-protocol.md` §3–§6).
//!
//! Every call is signed (`sign.rs`), goes out as `?rest_route=` — the one URL form
//! WordPress answers with pretty permalinks on OR off — and has its refusals mapped
//! to the §6 sentences a person can act on. Reads are cursor loops; what reaches
//! the disk is verified first: each file's `sha256`, each SQL chunk's `sha256`, the
//! frame's terminator, and every path against the set that was asked for.

use super::sign::{self, PairingKey};
use crate::error::{Error, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One table in the manifest (§4.1).
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteTable {
    pub name: String,
    pub rows: u64,
    pub bytes: u64,
    pub checksum: String,
}

/// What `/manifest` answers.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub protocol: String,
    pub plugin: String,
    pub site_url: String,
    pub wp: String,
    pub php: String,
    pub mysql: String,
    pub prefix: String,
    pub multisite: bool,
    pub tables: Vec<RemoteTable>,
    pub free_bytes: Option<u64>,
}

/// One file in `/files/list`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RemoteFile {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Deserialize)]
struct FileList {
    files: Vec<RemoteFile>,
    cursor: Option<String>,
}

#[derive(Deserialize)]
struct SqlChunk {
    sql: String,
    sha256: String,
    rows: u64,
    cursor: Option<String>,
}

/// One record of a `/files/read` frame (§4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameRecord {
    pub path: String,
    pub mtime: i64,
    pub bytes: Vec<u8>,
    /// The plugin's refusal for this path, when it gave one.
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct FrameHeader {
    path: String,
    size: u64,
    #[serde(default)]
    mtime: i64,
    sha256: String,
    #[serde(default)]
    error: Option<String>,
}

/// What `/push/begin` answers.
#[derive(Debug, Clone, Deserialize)]
pub struct PushBegun {
    pub push_id: String,
    #[serde(default)]
    pub conflicts: Vec<String>,
}

/// The §4.2 conflict refusal, as the client reports it: the items live changed.
#[derive(Debug, Clone)]
pub struct Conflicts(pub Vec<String>);

/// A `wp-content`-relative path the protocol allows: `/`-separated, relative, no
/// `.`/`..` segments, no `\`, no `:`, nothing empty. Pure.
///
/// `:` is refused everywhere (found by review, 10 Oct 2026): on Windows a segment
/// like `C:` is a drive prefix, and `PathBuf::push` REPLACES the whole path with
/// it — so a hostile site that listed and served `C:/x/evil.php` would have been
/// written outside the pull folder. `name:stream` (an NTFS alternate stream)
/// goes the same way. No WordPress path needs one.
pub fn safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.contains('\0')
        && !rel.contains(':')
        && rel.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Parse a `/files/read` frame, refusing it WHOLE when it is not exactly what was
/// asked for: a missing terminator (a cut-off transfer looks just like a short
/// one), anything after it, a path not in `asked` or not [`safe_rel`], a size that
/// runs past the end, or bytes whose sha256 is not the header's. Pure.
/// The largest single file a pull takes (64 MiB): a frame header claiming more
/// is refused BEFORE anything is allocated for it (#837), and the pull leaves such
/// a file on live and names it. 1 GiB until 10 Oct 2026 — but the plugin builds a
/// frame in PHP memory, and a shared host's PHP (`memory_limit` 128–256 MB, an
/// account-wide resource cap) dies on a file that size, which the client then sees
/// only as a reset connection (#844). Big media is uploads-on-demand's (S3).
pub const MAX_FILE_BYTES: u64 = 64 << 20;

/// One `/files/read` asks for at most this many files and this many bytes (#844):
/// the plugin holds the whole frame in memory, so a request is sized for a shared
/// host, not for the network. A file bigger than the byte budget goes alone.
pub const READ_BATCH_FILES: usize = 200;
pub const READ_BATCH_BYTES: u64 = 8 << 20;

/// Group `(path, size)` into `/files/read` requests under both budgets, in order.
pub fn read_batches(files: &[(String, u64)]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let (mut cur, mut bytes) = (Vec::new(), 0u64);
    for (path, size) in files {
        if !cur.is_empty() && (cur.len() >= READ_BATCH_FILES || bytes + size > READ_BATCH_BYTES) {
            out.push(std::mem::take(&mut cur));
            bytes = 0;
        }
        cur.push(path.clone());
        bytes += size;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn parse_frame(frame: &[u8], asked: &HashSet<String>) -> Result<Vec<FrameRecord>> {
    let bad = |why: &str| Error::Other(format!("the site's file transfer was not intact: {why}"));
    let mut at = 0usize;
    let mut out = Vec::new();
    loop {
        let len_bytes = frame.get(at..at + 4).ok_or_else(|| bad("it ended without its terminator"))?;
        let len = u32::from_be_bytes(len_bytes.try_into().expect("4 bytes")) as usize;
        at += 4;
        if len == 0 {
            if at != frame.len() {
                return Err(bad("bytes after the terminator"));
            }
            return Ok(out);
        }
        let header = frame.get(at..at + len).ok_or_else(|| bad("a header runs past the end"))?;
        at += len;
        let h: FrameHeader = serde_json::from_slice(header).map_err(|_| bad("a header is not JSON"))?;
        if !safe_rel(&h.path) || !asked.contains(&h.path) {
            return Err(bad(&format!("a file nobody asked for ({})", h.path)));
        }
        if h.size > MAX_FILE_BYTES {
            return Err(bad(&format!("{} is {} MB — larger than a pull takes", h.path, h.size >> 20)));
        }
        let size = usize::try_from(h.size).map_err(|_| bad("a size too large"))?;
        let bytes = frame.get(at..at + size).ok_or_else(|| bad("a file runs past the end"))?.to_vec();
        at += size;
        if sign::sha256_hex(&bytes) != h.sha256 {
            return Err(bad(&format!("{} does not match its checksum", h.path)));
        }
        out.push(FrameRecord { path: h.path, mtime: h.mtime, bytes, error: h.error });
    }
}

/// The conflict list out of a 409 body, when it is one.
pub fn conflicts_of(body: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    if v.get("code")?.as_str()? != "conflict" {
        return None;
    }
    Some(v.get("data")?.get("conflicts")?.as_array()?.iter().filter_map(|c| c.as_str().map(str::to_string)).collect())
}

/// The §6 sentence for a refusal, from the status and the body.
fn refusal(status: u16, body: &str) -> Error {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("code").and_then(|c| c.as_str()).map(str::to_string));
    let why = match (status, code.as_deref()) {
        (_, Some("unknown_key")) => "the site has a different rexenv pairing now — paste its current key into rexenv".into(),
        (_, Some("clock_skew")) => "this computer's clock is too far off the site's — check the date and time".into(),
        (_, Some("bad_signature")) => "the pairing does not match — paste the site's key into rexenv again".into(),
        (_, Some("replayed")) => "the site saw this request twice — try again".into(),
        (_, Some("unknown_table")) => "the site has no such table".into(),
        (_, Some("conflict")) => "the live site changed since rexenv last saw it".into(),
        (_, Some("no_push")) => "the site no longer has that push — start it again".into(),
        (_, Some("no_backup")) => "the site keeps no such backup".into(),
        (_, Some("incomplete")) => "the push is missing data — start it again".into(),
        (403 | 406, None) => "a firewall in front of the site (Wordfence, Cloudflare, ModSecurity) refused the request".into(),
        (404, None) => "the site does not answer as rexenv Sync — is the plugin installed and active?".into(),
        (413, _) => "the site's host refused a request that large".into(),
        (s, c) => {
            // The plugin's own sentence, when it gave one — a 500 from `push/db` names
            // the statement its database refused, which is the whole diagnosis.
            let msg = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_string));
            format!("the site answered {s}{}{}", c.map(|c| format!(" ({c})")).unwrap_or_default(), msg.map(|m| format!(": {m}")).unwrap_or_default())
        }
    };
    Error::Other(format!("rexenv Sync: {why}."))
}

/// A connection to one paired site.
pub struct Client {
    key: PairingKey,
    base: String,
    http: reqwest::Client,
    /// HTTP basic auth in front of the site (§11 Q5) — sent as `Authorization`
    /// on every request; nothing to do with the pairing.
    basic_auth: Option<(String, String)>,
    /// The last 409's conflict list, for `push_begin` to hand back as data.
    last_conflicts: std::sync::Mutex<Option<Vec<String>>>,
    /// The waits before each RETRY of a read (#843); its length is the retry count.
    backoff: Vec<Duration>,
}

/// How one try ended when it did not succeed.
enum Attempt {
    /// Dropped, reset, cut off, or 429/502/503/504 — the site may answer next time.
    Transient(String),
    /// The site answered and refused: retrying changes nothing.
    Final(Error),
}

/// The default waits before retrying a read: 2 s, 5 s, 15 s (#843).
const BACKOFF: [u64; 3] = [2, 5, 15];

/// May this request be sent again after the site dropped it? Only a READ (#843):
/// the GETs, and `/files/read` (a POST only for its body). Never a push route — a
/// `/push/db` chunk sent twice would insert its rows twice into the shadow table.
pub fn retryable(method: &reqwest::Method, route: &str) -> bool {
    *method == reqwest::Method::GET || route == "/rexenv-sync/v1/files/read"
}

/// An answer worth retrying: the site is overloaded or a gateway gave up, not a refusal.
fn transient_status(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

impl Client {
    /// Requests go to the key's own site.
    pub fn new(key: PairingKey) -> Result<Self> {
        // No redirects: a site that answered with a redirect would send the signed
        // headers on to wherever it pointed — an `http://` URL included, which is
        // the plain-connection pull `parse_key` exists to refuse (review, 10 Oct 2026).
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| Error::Other(format!("rexenv Sync: {e}")))?;
        let base = key.site_url.clone();
        Ok(Self { key, base, http, basic_auth: None, last_conflicts: std::sync::Mutex::new(None), backoff: BACKOFF.iter().map(|s| Duration::from_secs(*s)).collect() })
    }

    /// Send `Authorization: Basic` on every request — a site behind HTTP auth.
    pub fn with_basic_auth(mut self, user: &str, password: &str) -> Self {
        self.basic_auth = Some((user.to_string(), password.to_string()));
        self
    }

    /// Send to another origin while the key — and the manifest's `site_url` it is
    /// checked against — stay the site's. For a site reached through another
    /// address (a staging hostname, a fixture server in a live check).
    pub fn with_base_url(mut self, base: &str) -> Self {
        self.base = base.trim_end_matches('/').to_string();
        self
    }

    /// Other waits before the read retries (tests use milliseconds; empty = never retry).
    pub fn with_backoff(mut self, backoff: Vec<Duration>) -> Self {
        self.backoff = backoff;
        self
    }

    /// One request, signed afresh (a new ts and nonce) — and for a READ, again after
    /// each backoff wait when the site dropped it or answered 429/502/503/504 (#843).
    /// The first real host reset every connection for some minutes, then recovered
    /// (10 Oct 2026); a pull that had read 3 of 12 tables died with it.
    async fn send(&self, method: reqwest::Method, route: &str, query: &[(&str, &str)], body: Vec<u8>) -> Result<Vec<u8>> {
        let waits: &[Duration] = if retryable(&method, route) { &self.backoff } else { &[] };
        let mut attempt = 0usize;
        loop {
            match self.send_once(method.clone(), route, query, body.clone()).await {
                Ok(bytes) => return Ok(bytes),
                Err(Attempt::Final(e)) => return Err(e),
                Err(Attempt::Transient(why)) => {
                    let Some(wait) = waits.get(attempt) else {
                        let tries = attempt + 1;
                        return Err(Error::Other(if tries > 1 { format!("{} (tried {tries} times)", Self::dropped(&why)) } else { Self::dropped(&why) }));
                    };
                    tokio::time::sleep(*wait).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn send_once(&self, method: reqwest::Method, route: &str, query: &[(&str, &str)], body: Vec<u8>) -> std::result::Result<Vec<u8>, Attempt> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        let nonce = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, uuid::Uuid::new_v4().as_bytes());
        let canonical = sign::canonical(method.as_str(), route, query, &ts, &nonce, &body);
        let sig = sign::signature(&self.key.secret, &canonical);
        let mut params: Vec<(&str, &str)> = vec![("rest_route", route)];
        params.extend_from_slice(query);
        let mut req = self.http.request(method, format!("{}/", self.base)).query(&params);
        if let Some((u, p)) = &self.basic_auth {
            req = req.basic_auth(u, Some(p));
        }
        let resp = req
            .header("X-Rexsync-Key", &self.key.key_id)
            .header("X-Rexsync-Ts", &ts)
            .header("X-Rexsync-Nonce", &nonce)
            .header("X-Rexsync-Sig", &sig)
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| Attempt::Transient(e.to_string()))?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(|e| Attempt::Transient(e.to_string()))?.to_vec();
        if transient_status(status) {
            return Err(Attempt::Transient(format!("HTTP {status}")));
        }
        if !(200..300).contains(&status) {
            let text = String::from_utf8_lossy(&bytes);
            *self.last_conflicts.lock().expect("conflicts lock") = conflicts_of(&text);
            return Err(Attempt::Final(refusal(status, &text)));
        }
        Ok(bytes)
    }

    /// The sentence for a request the site never finished answering — refused, reset or
    /// cut off mid-body. The first real-host run (live-sync.rex.bd, 10 Oct 2026) showed it
    /// as "error decoding response body": for some minutes the host reset EVERY connection
    /// from this computer, the homepage included, then answered normally again — the cause
    /// was never established (an overloaded or restarting server, or a short firewall
    /// throttle; a pull is only ~25 requests). Said as reqwest put it, it read as a
    /// protocol bug; said plainly, the next step is obvious.
    pub fn dropped(detail: &str) -> String {
        format!(
            "rexenv Sync: the site stopped answering ({detail}). The server may be overloaded or \
             restarting, or a security firewall on the host (Imunify360, CSF, ModSecurity, Wordfence) \
             may be limiting this computer. Open the site in a browser: if it loads, wait a few \
             minutes and try again."
        )
    }

    fn json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
        serde_json::from_slice(bytes).map_err(|e| Error::Other(format!("rexenv Sync: the site's answer was not what the protocol says ({e}).")))
    }

    /// `/manifest`, refusing a site that is not the one the key was made for, or
    /// that speaks another protocol version.
    pub async fn manifest(&self) -> Result<Manifest> {
        let m: Manifest = Self::json(&self.send(reqwest::Method::GET, "/rexenv-sync/v1/manifest", &[], Vec::new()).await?)?;
        if m.protocol != sign::PROTOCOL {
            return Err(Error::Other(format!(
                "rexenv Sync: the site's plugin speaks {}, this rexenv speaks {} — update the older one.",
                m.protocol,
                sign::PROTOCOL
            )));
        }
        if m.site_url.trim_end_matches('/') != self.key.site_url {
            return Err(Error::Other(format!(
                "rexenv Sync: this key is for {}, but the site answering is {}.",
                self.key.site_url, m.site_url
            )));
        }
        Ok(m)
    }

    /// Every file under `wp-content`, through the cursor.
    pub async fn list_files(&self, exclude: &[&str], uploads_since: Option<i64>) -> Result<Vec<RemoteFile>> {
        let globs = exclude.join(",");
        let since = uploads_since.map(|s| s.to_string());
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut q: Vec<(&str, &str)> = Vec::new();
            if !globs.is_empty() {
                q.push(("exclude", &globs));
            }
            if let Some(s) = &since {
                q.push(("uploads_since", s));
            }
            if let Some(c) = &cursor {
                q.push(("cursor", c));
            }
            let page: FileList = Self::json(&self.send(reqwest::Method::GET, "/rexenv-sync/v1/files/list", &q, Vec::new()).await?)?;
            out.extend(page.files);
            match page.cursor {
                Some(c) if Some(&c) != cursor.as_ref() => cursor = Some(c),
                Some(_) => return Err(Error::Other("rexenv Sync: the file list did not move forward.".into())),
                None => return Ok(out),
            }
        }
    }

    /// Fetch `paths` (≤ 200 per request) and write each under `dest`, which must
    /// not be the destination's final place — the caller moves a whole verified set
    /// into place. Returns the records the plugin refused.
    /// `files` = `(path, size)` from `/files/list` (size 0 when unknown); requests are
    /// sized by [`read_batches`].
    pub async fn read_files(&self, files: &[(String, u64)], dest: &Path, exclude: &[&str]) -> Result<Vec<FrameRecord>> {
        let globs = exclude.join(",");
        let mut refused = Vec::new();
        for chunk in read_batches(files) {
            let chunk = chunk.as_slice();
            let asked: HashSet<String> = chunk.iter().cloned().collect();
            let body = serde_json::to_vec(&serde_json::json!({ "paths": chunk })).expect("json");
            let q: Vec<(&str, &str)> = if globs.is_empty() { vec![] } else { vec![("exclude", &globs)] };
            let frame = self.send(reqwest::Method::POST, "/rexenv-sync/v1/files/read", &q, body).await?;
            for rec in parse_frame(&frame, &asked)? {
                if rec.error.is_some() {
                    refused.push(rec);
                    continue;
                }
                let to = join_rel(dest, &rec.path)?;
                if let Some(p) = to.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::write(&to, &rec.bytes)?;
                // LIVE's mtime, not the moment of the pull (#841): a push sends files
                // changed locally since the last sync (`mtime > base.at`), and a pulled
                // file stamped "now" read as changed — the first push after a real pull
                // would have uploaded all of wp-content. A rename keeps the time.
                if rec.mtime > 0 {
                    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(rec.mtime as u64);
                    std::fs::File::options().write(true).open(&to)?.set_modified(t)?;
                }
            }
        }
        Ok(refused)
    }

    /// One table, through the cursor, into `out` — written as `<out>.partial` and
    /// renamed only when the last chunk is in, every chunk's sha256 checked first.
    /// Returns the row count.
    pub async fn export_table(&self, table: &str, out: &Path) -> Result<u64> {
        let partial = PathBuf::from(format!("{}.partial", out.display()));
        let mut file = std::fs::File::create(&partial)?;
        let mut rows = 0u64;
        let mut cursor: Option<String> = None;
        let result: Result<()> = async {
            loop {
                let mut q: Vec<(&str, &str)> = vec![("table", table)];
                if let Some(c) = &cursor {
                    q.push(("cursor", c));
                }
                let chunk: SqlChunk = Self::json(&self.send(reqwest::Method::GET, "/rexenv-sync/v1/db/export", &q, Vec::new()).await?)?;
                if sign::sha256_hex(chunk.sql.as_bytes()) != chunk.sha256 {
                    return Err(Error::Other(format!("rexenv Sync: a chunk of {table} did not match its checksum.")));
                }
                file.write_all(chunk.sql.as_bytes())?;
                rows += chunk.rows;
                match chunk.cursor {
                    Some(c) if Some(&c) != cursor.as_ref() => cursor = Some(c),
                    Some(_) => return Err(Error::Other(format!("rexenv Sync: the export of {table} did not move forward."))),
                    None => return Ok(()),
                }
            }
        }
        .await;
        drop(file);
        match result {
            Ok(()) => {
                std::fs::rename(&partial, out)?;
                Ok(rows)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                Err(e)
            }
        }
    }
}

impl Client {
    /// `/push/begin` (§4.2). `Err` carries the §6 sentence; a conflict is
    /// `Ok(PushBegun)` ONLY when every conflict was in `override` — otherwise the
    /// `Conflicts` list comes back so the caller can ask.
    pub async fn push_begin(&self, tables: &[String], files: &[String], base: &super::base::SyncBase, override_items: &[String]) -> Result<std::result::Result<PushBegun, Conflicts>> {
        let body = serde_json::to_vec(&serde_json::json!({ "tables": tables, "files": files, "base": { "tables": base.tables, "files": base.files }, "override": override_items })).expect("json");
        match self.send(reqwest::Method::POST, "/rexenv-sync/v1/push/begin", &[], body).await {
            Ok(bytes) => Ok(Ok(Self::json(&bytes)?)),
            Err(e) => {
                // The refusal's body is folded into the error text by `send`; re-read it.
                if let Some(list) = self.last_conflicts.lock().expect("conflicts lock").take() {
                    return Ok(Err(Conflicts(list)));
                }
                Err(e)
            }
        }
    }

    /// `/push/file`: the whole file, in ≤ 4 MB pieces appended by offset.
    pub async fn push_file(&self, push_id: &str, rel: &str, bytes: &[u8]) -> Result<()> {
        const PIECE: usize = 4 * 1024 * 1024;
        let mut offset = 0usize;
        loop {
            let end = (offset + PIECE).min(bytes.len());
            let off = offset.to_string();
            self.send(reqwest::Method::POST, "/rexenv-sync/v1/push/file", &[("push_id", push_id), ("path", rel), ("offset", &off)], bytes[offset..end].to_vec()).await?;
            offset = end;
            if offset >= bytes.len() {
                return Ok(());
            }
        }
    }

    /// `/push/db`: one chunk of a table's dump.
    pub async fn push_sql(&self, push_id: &str, table: &str, sql: &str) -> Result<()> {
        let body = serde_json::to_vec(&serde_json::json!({ "sql": sql, "sha256": sign::sha256_hex(sql.as_bytes()) })).expect("json");
        self.send(reqwest::Method::POST, "/rexenv-sync/v1/push/db", &[("push_id", push_id), ("table", table)], body).await.map(|_| ())
    }

    /// `/push/swap` → the backup id.
    pub async fn push_swap(&self, push_id: &str) -> Result<String> {
        let v: serde_json::Value = Self::json(&self.send(reqwest::Method::POST, "/rexenv-sync/v1/push/swap", &[("push_id", push_id)], Vec::new()).await?)?;
        v.get("backup_id").and_then(|b| b.as_str()).map(str::to_string).ok_or_else(|| Error::Other("rexenv Sync: the swap answered with no backup id.".into()))
    }

    pub async fn push_rollback(&self, backup_id: &str) -> Result<()> {
        self.send(reqwest::Method::POST, "/rexenv-sync/v1/push/rollback", &[("backup_id", backup_id)], Vec::new()).await.map(|_| ())
    }

    pub async fn push_abort(&self, push_id: &str) -> Result<()> {
        self.send(reqwest::Method::POST, "/rexenv-sync/v1/push/abort", &[("push_id", push_id)], Vec::new()).await.map(|_| ())
    }
}

/// `dest` joined with a [`safe_rel`] path, segment by segment (so a `/` in the
/// protocol's spelling becomes this OS's separator) — and CHECKED to still be
/// under `dest` afterwards, whatever the segments were. Two walls, not one: the
/// `safe_rel` rule is about spellings, this is about where the bytes would land.
fn join_rel(dest: &Path, rel: &str) -> Result<PathBuf> {
    let mut p = dest.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
    }
    // `starts_with` compares COMPONENTS: a push that replaced the path (a drive, a
    // root) no longer starts with `dest`.
    if !safe_rel(rel) || !p.starts_with(dest) {
        return Err(Error::Other(format!("rexenv Sync: refused to write {rel} outside the pull folder.")));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(path: &str, bytes: &[u8]) -> Vec<u8> {
        let h = serde_json::json!({ "path": path, "size": bytes.len(), "mtime": 1, "sha256": sign::sha256_hex(bytes) });
        let h = serde_json::to_vec(&h).unwrap();
        let mut out = (h.len() as u32).to_be_bytes().to_vec();
        out.extend(h);
        out.extend_from_slice(bytes);
        out
    }

    fn asked(p: &[&str]) -> HashSet<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    /// **A frame is taken whole or not at all** (ledger #826). Plant: accept a
    /// frame with no terminator and the "cut off" assertion fails.
    #[test]
    fn a_frame_is_accepted_only_when_it_is_exactly_what_was_asked() {
        let mut good = rec("plugins/a/a.php", b"<?php // a");
        good.extend(rec("uploads/x.jpg", b"\xff\xd8"));
        good.extend(0u32.to_be_bytes());
        let r = parse_frame(&good, &asked(&["plugins/a/a.php", "uploads/x.jpg"])).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].bytes, b"\xff\xd8");

        let cut = &good[..good.len() - 4];
        assert!(parse_frame(cut, &asked(&["plugins/a/a.php", "uploads/x.jpg"])).unwrap_err().to_string().contains("terminator"), "cut off");
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(parse_frame(&trailing, &asked(&["plugins/a/a.php", "uploads/x.jpg"])).is_err(), "bytes after the end");
        assert!(parse_frame(&good, &asked(&["plugins/a/a.php"])).is_err(), "a file nobody asked for");
        let mut evil = rec("../wp-config.php", b"x");
        evil.extend(0u32.to_be_bytes());
        assert!(parse_frame(&evil, &asked(&["../wp-config.php"])).is_err(), "traversal, even if asked");
        let mut lie = rec("a.txt", b"one");
        let at = lie.len() - 3;
        lie[at] = b'X';
        lie.extend(0u32.to_be_bytes());
        assert!(parse_frame(&lie, &asked(&["a.txt"])).unwrap_err().to_string().contains("checksum"));
        // A header claiming more than the ceiling is refused before its bytes are
        // looked at — let alone allocated (#837). Plant: drop the ceiling and this
        // reads "runs past the end" instead.
        let h = serde_json::to_vec(&serde_json::json!({ "path": "big.bin", "size": MAX_FILE_BYTES + 1, "mtime": 1, "sha256": "00" })).unwrap();
        let mut huge = (h.len() as u32).to_be_bytes().to_vec();
        huge.extend(h);
        huge.extend_from_slice(b"xx");
        assert!(parse_frame(&huge, &asked(&["big.bin"])).unwrap_err().to_string().contains("larger than a pull takes"));
    }

    /// **A `/files/read` request never asks for more than 200 files or 8 MB, a file
    /// over the budget goes alone, and order is kept** (ledger #844). Plant: drop the
    /// byte test in `read_batches` and the 5 MB files share one 25 MB request.
    #[test]
    fn file_reads_are_sized_for_a_shared_host() {
        let mb = 1u64 << 20;
        let small: Vec<(String, u64)> = (0..450).map(|i| (format!("s{i}"), 10)).collect();
        let b = read_batches(&small);
        assert_eq!(b.iter().map(Vec::len).collect::<Vec<_>>(), vec![200, 200, 50]);
        let big: Vec<(String, u64)> = (0..5).map(|i| (format!("b{i}"), 5 * mb)).collect();
        let b = read_batches(&big);
        assert_eq!(b.len(), 5, "8 MB budget: one 5 MB file per request");
        let mixed = vec![("a".to_string(), 1), ("huge".to_string(), 40 * mb), ("c".to_string(), 1)];
        assert_eq!(read_batches(&mixed), vec![vec!["a".to_string()], vec!["huge".to_string()], vec!["c".to_string()]]);
        assert!(MAX_FILE_BYTES <= 64 * mb, "a file a shared host's PHP can hold in memory");
    }

    /// A one-thread fake site on 127.0.0.1: each accepted connection is answered by
    /// `reply(n)` (n = 0, 1, …) — `None` closes it unanswered, like a reset host.
    fn fake_site(reply: impl Fn(usize) -> Option<String> + Send + 'static) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for stream in l.incoming().flatten() {
                let n = h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut stream = stream;
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                if let Some(r) = reply(n) {
                    let _ = stream.write_all(r.as_bytes());
                }
            }
        });
        (port, hits)
    }

    fn test_client(port: u16) -> Client {
        let key = PairingKey { site_url: "https://example.com".into(), key_id: "k_0123abcd".into(), secret: vec![7u8; 32] };
        Client::new(key).unwrap().with_base_url(&format!("http://127.0.0.1:{port}")).with_backoff(vec![Duration::from_millis(5); 2])
    }

    /// **A READ the site drops is sent again after each backoff wait, and then named
    /// plainly; a PUSH route is never sent twice; 429/502/503/504 count as a drop and
    /// a later 200 is used** (ledger #843). Plant: make `retryable` return true for
    /// every route and the push leg sees 3 connections, not 1.
    #[tokio::test]
    async fn reads_retry_after_a_drop_and_pushes_never_do() {
        use std::sync::atomic::Ordering::SeqCst;
        assert!(retryable(&reqwest::Method::GET, "/rexenv-sync/v1/manifest"));
        assert!(retryable(&reqwest::Method::POST, "/rexenv-sync/v1/files/read"));
        for r in ["/rexenv-sync/v1/push/begin", "/rexenv-sync/v1/push/db", "/rexenv-sync/v1/push/file", "/rexenv-sync/v1/push/swap", "/rexenv-sync/v1/push/rollback"] {
            assert!(!retryable(&reqwest::Method::POST, r), "{r} must never be retried");
        }

        // Every connection dropped: the manifest is tried 1 + 2 times, then named.
        let (port, hits) = fake_site(|_| None);
        let c = test_client(port);
        let e = c.manifest().await.unwrap_err().to_string();
        assert_eq!(hits.load(SeqCst), 3, "{e}");
        assert!(e.contains("stopped answering") && e.contains("tried 3 times"), "{e}");
        // A push route against the same dropping site: exactly ONE try.
        let before = hits.load(SeqCst);
        let e = c.push_abort("p1").await.unwrap_err().to_string();
        assert_eq!(hits.load(SeqCst) - before, 1, "a push route was retried: {e}");
        assert!(!e.contains("tried"), "{e}");

        // 503 first, then the manifest: the retry is used and nothing is reported.
        let manifest = serde_json::json!({
            "protocol": sign::PROTOCOL, "plugin": "0.1.0", "site_url": "https://example.com", "wp": "6.8", "php": "8.3.1",
            "mysql": "8.0", "prefix": "wp_", "multisite": false, "tables": [], "free_bytes": null
        })
        .to_string();
        let (port, hits) = fake_site(move |n| {
            Some(if n == 0 {
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else {
                format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{manifest}", manifest.len())
            })
        });
        let m = test_client(port).manifest().await.unwrap();
        assert_eq!((m.wp.as_str(), hits.load(SeqCst)), ("6.8", 2));
    }

    /// **Nothing a site sends can be written outside the pull folder** (ledger
    /// #826, the review's finding). Plant: drop the `:` rule and `C:/x` passes
    /// `safe_rel`, and on Windows `join_rel` would land at the drive root.
    #[test]
    fn join_rel_never_leaves_the_destination() {
        let dest = std::env::temp_dir().join("rexenv-join-rel");
        assert_eq!(join_rel(&dest, "plugins/a/b.php").unwrap(), dest.join("plugins").join("a").join("b.php"));
        for bad in ["C:/x/evil.php", "../x", "/etc/passwd", "a:b"] {
            assert!(join_rel(&dest, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn paths_and_refusals() {
        for ok in ["a", "plugins/x/y.php", "uploads/2026/10/a b.jpg"] {
            assert!(safe_rel(ok), "{ok}");
        }
        for bad in ["", "/etc/passwd", "a/../b", "./a", "a//b", "a\\b", "..", "a/", "C:/x/evil.php", "plugins/a.php:stream"] {
            assert!(!safe_rel(bad), "{bad:?}");
        }
        let e = refusal(401, r#"{"code":"unknown_key","message":"x"}"#).to_string();
        assert!(e.contains("paste its current key"), "{e}");
        assert!(refusal(403, "<html>blocked</html>").to_string().contains("firewall"));
        // A connection the host RESET (no status at all) names the firewall too.
        let d = Client::dropped("error decoding response body");
        assert!(d.contains("stopped answering") && d.contains("firewall") && d.contains("error decoding response body"), "{d}");
        assert!(refusal(404, "").to_string().contains("plugin installed"));
    }
}
