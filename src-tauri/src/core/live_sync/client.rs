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
        let size = usize::try_from(h.size).map_err(|_| bad("a size too large"))?;
        let bytes = frame.get(at..at + size).ok_or_else(|| bad("a file runs past the end"))?.to_vec();
        at += size;
        if sign::sha256_hex(&bytes) != h.sha256 {
            return Err(bad(&format!("{} does not match its checksum", h.path)));
        }
        out.push(FrameRecord { path: h.path, mtime: h.mtime, bytes, error: h.error });
    }
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
        (403 | 406, None) => "a firewall in front of the site (Wordfence, Cloudflare, ModSecurity) refused the request".into(),
        (404, None) => "the site does not answer as rexenv Sync — is the plugin installed and active?".into(),
        (413, _) => "the site's host refused a request that large".into(),
        (s, c) => format!("the site answered {s}{}", c.map(|c| format!(" ({c})")).unwrap_or_default()),
    };
    Error::Other(format!("rexenv Sync: {why}."))
}

/// A connection to one paired site.
pub struct Client {
    key: PairingKey,
    base: String,
    http: reqwest::Client,
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
        Ok(Self { key, base, http })
    }

    /// Send to another origin while the key — and the manifest's `site_url` it is
    /// checked against — stay the site's. For a site reached through another
    /// address (a staging hostname, a fixture server in a live check).
    pub fn with_base_url(mut self, base: &str) -> Self {
        self.base = base.trim_end_matches('/').to_string();
        self
    }

    async fn send(&self, method: reqwest::Method, route: &str, query: &[(&str, &str)], body: Vec<u8>) -> Result<Vec<u8>> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        let nonce = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, uuid::Uuid::new_v4().as_bytes());
        let canonical = sign::canonical(method.as_str(), route, query, &ts, &nonce, &body);
        let sig = sign::signature(&self.key.secret, &canonical);
        let mut params: Vec<(&str, &str)> = vec![("rest_route", route)];
        params.extend_from_slice(query);
        let resp = self
            .http
            .request(method, format!("{}/", self.base))
            .query(&params)
            .header("X-Rexsync-Key", &self.key.key_id)
            .header("X-Rexsync-Ts", &ts)
            .header("X-Rexsync-Nonce", &nonce)
            .header("X-Rexsync-Sig", &sig)
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| Error::Other(format!("rexenv Sync: the site did not answer ({e}).")))?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(|e| Error::Other(format!("rexenv Sync: {e}")))?.to_vec();
        if !(200..300).contains(&status) {
            return Err(refusal(status, &String::from_utf8_lossy(&bytes)));
        }
        Ok(bytes)
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
    pub async fn list_files(&self, exclude: &[&str]) -> Result<Vec<RemoteFile>> {
        let globs = exclude.join(",");
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut q: Vec<(&str, &str)> = Vec::new();
            if !globs.is_empty() {
                q.push(("exclude", &globs));
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
    pub async fn read_files(&self, paths: &[String], dest: &Path, exclude: &[&str]) -> Result<Vec<FrameRecord>> {
        let globs = exclude.join(",");
        let mut refused = Vec::new();
        for chunk in paths.chunks(200) {
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
        assert!(refusal(404, "").to_string().contains("plugin installed"));
    }
}
