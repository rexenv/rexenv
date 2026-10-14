//! The sync BASE (plan §2.6): what BOTH sides looked like at the last sync —
//! every live table's change stamp and every live file's `size:mtime` (the
//! plugin's view), and every LOCAL table's stamp (`stamps.rs`) — recorded after
//! each pull or push. The next pull brings only what moved on live against it;
//! the next push sends only what moved here against it, and the plugin refuses
//! what moved on live since. Plain JSON beside the pairing file; nothing in it is
//! secret. The HISTORY (the last 20 syncs) sits beside it, for the Live tab.

use crate::error::Result;
use crate::platform::traits::Platform;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncBase {
    /// LIVE's stamps, by table — the plugin's `checksum` from the manifest.
    pub tables: BTreeMap<String, String>,
    /// LIVE's files, `size:mtime`.
    pub files: BTreeMap<String, String>,
    /// Unix seconds of the pull/push that recorded it. A local file with a later
    /// mtime has changed here since.
    pub at: i64,
    /// The LOCAL copy's stamps, by table (`stamps::local_stamps`) — absent in a base
    /// written before 14 Oct 2026, which then reads as "every table may have changed
    /// here" (the old behaviour: push every table).
    #[serde(default)]
    pub local_tables: BTreeMap<String, String>,
    /// "pull" | "push" | "rollback" — what recorded it.
    #[serde(default)]
    pub kind: String,
    /// The local half as it was BEFORE the last push (`local_tables` and `at` of the
    /// base that push replaced) — what a rollback puts back, so the local changes
    /// that push sent count as "ahead" again once live no longer has them (#862).
    #[serde(default)]
    pub before_push: Option<Box<LocalHalf>>,
}

/// The local copy's half of a base: its table stamps and the sync moment.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalHalf {
    pub local_tables: BTreeMap<String, String>,
    pub at: i64,
}

/// Unix seconds now.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// One finished pull, push or rollback — what the Live tab's history lists.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub at: i64,
    /// "pull" | "push" | "rollback".
    pub kind: String,
    /// "ok" | "failed" | "conflicts".
    pub status: String,
    pub tables: usize,
    pub files: usize,
    pub rows: u64,
    #[serde(default)]
    pub backup_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// How many syncs the history keeps.
pub const HISTORY_KEEP: usize = 20;

fn history_file(platform: &dyn Platform, site_id: &str) -> Result<PathBuf> {
    // Beside the base, `<site id>.history.json` (`with_extension` on `x.base.json`
    // would have made `x.base.history.json` — the first VM run did).
    let base = file(platform, site_id)?;
    Ok(base.with_file_name(format!("{site_id}.history.json")))
}

/// The site's history, newest first.
pub fn history(platform: &dyn Platform, site_id: &str) -> Result<Vec<HistoryEntry>> {
    match std::fs::read(history_file(platform, site_id)?) {
        Ok(b) => Ok(serde_json::from_slice(&b).unwrap_or_default()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

/// Add one entry at the front, keeping [`HISTORY_KEEP`]. Pure core: `keep_recent`.
pub fn history_push(platform: &dyn Platform, site_id: &str, entry: HistoryEntry) -> Result<()> {
    let p = history_file(platform, site_id)?;
    let mut list = history(platform, site_id)?;
    list.insert(0, entry);
    keep_recent(&mut list);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serde_json::to_vec(&list).expect("json"))?;
    Ok(())
}

/// Newest first, at most [`HISTORY_KEEP`].
pub fn keep_recent(list: &mut Vec<HistoryEntry>) {
    list.sort_by(|a, b| b.at.cmp(&a.at));
    list.truncate(HISTORY_KEEP);
}

fn file(platform: &dyn Platform, site_id: &str) -> Result<PathBuf> {
    // The same rule as the pairing file's (#847): a site id is a UUID the app made,
    // and `live_sync_unpair` hands this an id straight from the webview.
    if site_id.is_empty() || !site_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err(crate::error::Error::Other("not a site id".into()));
    }
    Ok(super::secrets::dir(platform)?.join(format!("{site_id}.base.json")))
}


pub fn get(platform: &dyn Platform, site_id: &str) -> Result<Option<SyncBase>> {
    match std::fs::read(file(platform, site_id)?) {
        Ok(b) => Ok(serde_json::from_slice(&b).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn put(platform: &dyn Platform, site_id: &str, base: &SyncBase) -> Result<()> {
    let p = file(platform, site_id)?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serde_json::to_vec(base).expect("json"))?;
    Ok(())
}

/// Forget the base AND the history — both describe the pairing that is going.
pub fn delete(platform: &dyn Platform, site_id: &str) -> Result<()> {
    let _ = std::fs::remove_file(history_file(platform, site_id)?);
    match std::fs::remove_file(file(platform, site_id)?) {
        Ok(()) | Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    /// **A base path is built only from a site id** (ledger #847). Plant: drop the
    /// check and `../x` becomes a path one folder up.
    #[test]
    fn a_base_path_is_only_ever_a_site_id() {
        let plat = crate::platform::current();
        for bad in ["", "../x", "a/b", "x;y", "..\\x"] {
            assert!(super::file(plat.as_ref(), bad).is_err(), "{bad:?}");
        }
        assert!(super::file(plat.as_ref(), "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").is_ok());
    }

    /// An old base (no `local_tables`, no `kind`) still reads; the history keeps the
    /// newest 20, newest first.
    #[test]
    fn an_old_base_reads_and_the_history_is_bounded() {
        let b: super::SyncBase = serde_json::from_str(r#"{"tables":{"wp_posts":"ai:1"},"files":{},"at":5}"#).unwrap();
        assert!(b.local_tables.is_empty() && b.kind.is_empty() && b.at == 5);
        let mut list: Vec<super::HistoryEntry> = (0..25).map(|i| super::HistoryEntry { at: i, kind: "pull".into(), status: "ok".into(), ..Default::default() }).collect();
        super::keep_recent(&mut list);
        assert_eq!(list.len(), super::HISTORY_KEEP);
        assert_eq!((list[0].at, list[19].at), (24, 5));
    }
}
