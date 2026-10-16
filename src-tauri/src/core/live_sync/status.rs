//! The sync STATUS (plan §2.6, built 14 Oct 2026): is the local copy up to date
//! with live, behind it (live has changes to pull), ahead of it (changes here to
//! push), or both (diverged)? One pure verdict, `judge`, over four facts the
//! command gathers: the base, live's manifest + file list now, the local copy's
//! table stamps now, the local files' mtimes now.
//!
//! Each side is compared with ITS OWN half of the base — a live stamp with the
//! live stamp recorded, a local stamp with the local stamp recorded — never
//! across. A live-owned table (users, orders…) that changed here does not make
//! the copy "ahead": a push leaves those out by default, so they are listed but
//! do not count. A base without local stamps (written before this existed) says
//! "unknown" for every local table rather than "changed".

use super::base::SyncBase;
use super::client::{Manifest, RemoteFile};
use super::push::live_owned;
use serde::Serialize;
use std::collections::BTreeMap;

/// What one side did to one item since the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Same,
    Changed,
    /// Not in the base: new on that side.
    New,
    /// In the base, not on that side now.
    Gone,
    /// The base cannot say (an old base without local stamps).
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableStatus {
    pub name: String,
    pub live: Change,
    pub local: Change,
    pub live_owned: bool,
}

impl Change {
    /// Something to show: changed, new or gone — `Same` and `Unknown` are not news.
    pub fn moved(self) -> bool {
        matches!(self, Change::Changed | Change::New | Change::Gone)
    }
}

impl TableStatus {
    pub fn moved(&self) -> bool {
        self.live.moved() || self.local.moved()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatus {
    /// Changed or new on live since the base — a pull fetches these.
    pub live_changed: Vec<String>,
    /// In the base, no longer on live — a pull removes the local copy if untouched here.
    pub live_gone: Vec<String>,
    /// mtime after the base here — a push sends these.
    pub local_changed: Vec<String>,
    /// In both lists above: one side will overwrite the other.
    pub both: Vec<String>,
    pub total_on_live: usize,
    pub total_local: usize,
}

/// The one-word answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// No base: pull first.
    NeverSynced,
    UpToDate,
    /// Live has changes; nothing here to push.
    Behind,
    /// Changes here; live has none.
    Ahead,
    /// Both sides changed.
    Diverged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub verdict: Verdict,
    pub live_host: String,
    pub base_at: Option<i64>,
    pub base_kind: Option<String>,
    pub tables: Vec<TableStatus>,
    /// Tables live changed (count a pull would export).
    pub tables_behind: usize,
    /// Tables changed here that a push sends by default (not live-owned).
    pub tables_ahead: usize,
    /// Live-owned tables changed here — listed, not counted.
    pub tables_ahead_live_owned: usize,
    pub files: FileStatus,
    pub checked_at: i64,
}

/// Limit the file name lists to what a panel can show; the counts stay exact.
pub const FILE_LIST_CAP: usize = 200;

/// The verdict. `local_files` = `(wp-content-relative path, mtime)` of every local file
/// a push would consider; `local_stamps` = the local copy's stamps now.
pub fn judge(
    live_host: &str,
    base: Option<&SyncBase>,
    manifest: &Manifest,
    live_files: &[RemoteFile],
    local_stamps: &BTreeMap<String, String>,
    local_files: &[(String, i64)],
    now: i64,
) -> SyncStatus {
    let mut tables = Vec::new();
    let mut files = FileStatus { total_on_live: live_files.len(), total_local: local_files.len(), ..Default::default() };
    let Some(b) = base else {
        for t in &manifest.tables {
            tables.push(TableStatus { name: t.name.clone(), live: Change::New, local: Change::Unknown, live_owned: live_owned(&manifest.prefix, &t.name) });
        }
        files.live_changed = live_files.iter().map(|f| f.path.clone()).take(FILE_LIST_CAP).collect();
        return SyncStatus {
            verdict: Verdict::NeverSynced,
            live_host: live_host.into(),
            base_at: None,
            base_kind: None,
            tables_behind: manifest.tables.len(),
            tables_ahead: 0,
            tables_ahead_live_owned: 0,
            tables,
            files,
            checked_at: now,
        };
    };
    let local_known = !b.local_tables.is_empty();
    let mut names: Vec<&String> = manifest.tables.iter().map(|t| &t.name).chain(b.tables.keys()).chain(local_stamps.keys()).collect();
    names.sort();
    names.dedup();
    let (mut behind, mut ahead, mut ahead_owned) = (0, 0, 0);
    for name in names {
        let live_now = manifest.tables.iter().find(|t| &t.name == name).map(|t| &t.checksum);
        let live = match (b.tables.get(name), live_now) {
            (Some(was), Some(now)) if was == now => Change::Same,
            (Some(_), Some(_)) => Change::Changed,
            (None, Some(_)) => Change::New,
            (Some(_), None) => Change::Gone,
            (None, None) => Change::Same, // only local has it: nothing for live to say
        };
        let local = if !local_known {
            Change::Unknown
        } else {
            match (b.local_tables.get(name), local_stamps.get(name)) {
                (Some(was), Some(now)) if was == now => Change::Same,
                (Some(_), Some(_)) => Change::Changed,
                (None, Some(_)) => Change::New,
                (Some(_), None) => Change::Gone,
                (None, None) => Change::Same,
            }
        };
        let owned = live_owned(&manifest.prefix, name);
        if live.moved() {
            behind += 1;
        }
        if local.moved() {
            if owned {
                ahead_owned += 1;
            } else {
                ahead += 1;
            }
        }
        tables.push(TableStatus { name: name.clone(), live, local, live_owned: owned });
    }
    // Files: live against the base's live stamps; local by mtime against `at`.
    let live_now: BTreeMap<&str, String> = live_files.iter().map(|f| (f.path.as_str(), format!("{}:{}", f.size, f.mtime))).collect();
    let mut live_changed: Vec<String> = live_now.iter().filter(|(p, s)| b.files.get(**p) != Some(*s)).map(|(p, _)| p.to_string()).collect();
    let mut live_gone: Vec<String> = b.files.keys().filter(|p| !live_now.contains_key(p.as_str())).cloned().collect();
    let mut local_changed: Vec<String> = local_files.iter().filter(|(p, m)| b.file_changed_here(p, *m)).map(|(p, _)| p.clone()).collect();
    live_changed.sort();
    live_gone.sort();
    local_changed.sort();
    let both: Vec<String> = local_changed.iter().filter(|p| live_changed.binary_search(p).is_ok() || live_gone.binary_search(p).is_ok()).cloned().collect();
    let live_side = !live_changed.is_empty() || !live_gone.is_empty() || behind > 0;
    let local_side = !local_changed.is_empty() || ahead > 0;
    let verdict = match (live_side, local_side) {
        (false, false) => Verdict::UpToDate,
        (true, false) => Verdict::Behind,
        (false, true) => Verdict::Ahead,
        (true, true) => Verdict::Diverged,
    };
    live_changed.truncate(FILE_LIST_CAP);
    live_gone.truncate(FILE_LIST_CAP);
    local_changed.truncate(FILE_LIST_CAP);
    files.live_changed = live_changed;
    files.live_gone = live_gone;
    files.local_changed = local_changed;
    files.both = both;
    SyncStatus {
        verdict,
        live_host: live_host.into(),
        base_at: Some(b.at),
        base_kind: Some(b.kind.clone()).filter(|k| !k.is_empty()),
        tables,
        tables_behind: behind,
        tables_ahead: ahead,
        tables_ahead_live_owned: ahead_owned,
        files,
        checked_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::live_sync::client::RemoteTable;

    fn manifest(tables: &[(&str, &str)]) -> Manifest {
        Manifest {
            protocol: "rexsync1".into(),
            plugin: "0.1.3".into(),
            site_url: "https://example.com".into(),
            wp: "6.8".into(),
            php: "8.3".into(),
            mysql: "8.4".into(),
            prefix: "wp_".into(),
            multisite: false,
            subdomain: false,
            tables: tables.iter().map(|(n, c)| RemoteTable { name: n.to_string(), rows: 1, bytes: 1, checksum: c.to_string() }).collect(),
            free_bytes: None,
        }
    }
    fn files(list: &[(&str, u64, i64)]) -> Vec<RemoteFile> {
        list.iter().map(|(p, s, m)| RemoteFile { path: p.to_string(), size: *s, mtime: *m }).collect()
    }
    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
    fn base() -> SyncBase {
        SyncBase {
            tables: map(&[("wp_posts", "ai:5"), ("wp_options", "opt:9:1"), ("wp_users", "ai:2")]),
            files: map(&[("themes/x/style.css", "10:100"), ("uploads/a.jpg", "5:50")]),
            at: 1000,
            local_tables: map(&[("wp_posts", "ck:1"), ("wp_options", "opt:9:7"), ("wp_users", "ck:3")]),
            local_files: BTreeMap::new(),
            kind: "pull".into(),
            before_push: None,
            anonymised: Vec::new(),
        }
    }

    /// **The verdict reads each side against its OWN half of the base, and a
    /// live-owned table changed here never makes the copy "ahead"** (ledger #860).
    /// Plants: compare local stamps with live stamps → the up-to-date leg goes
    /// diverged; count live-owned tables → the users leg goes ahead.
    #[test]
    fn the_four_verdicts_and_the_live_owned_exception() {
        let b = base();
        let live_same = manifest(&[("wp_posts", "ai:5"), ("wp_options", "opt:9:1"), ("wp_users", "ai:2")]);
        let lf = files(&[("themes/x/style.css", 10, 100), ("uploads/a.jpg", 5, 50)]);
        let local_same = map(&[("wp_posts", "ck:1"), ("wp_options", "opt:9:7"), ("wp_users", "ck:3")]);
        let local_files = vec![("themes/x/style.css".to_string(), 900), ("uploads/a.jpg".to_string(), 900)];
        let s = judge("example.com", Some(&b), &live_same, &lf, &local_same, &local_files, 2000);
        assert_eq!(s.verdict, Verdict::UpToDate, "{s:?}");
        assert!(s.tables.iter().all(|t| t.live == Change::Same && t.local == Change::Same));

        // live moved a table + a file + dropped a file → behind
        let live_moved = manifest(&[("wp_posts", "ai:6"), ("wp_options", "opt:9:1"), ("wp_users", "ai:2")]);
        let lf2 = files(&[("themes/x/style.css", 11, 101), ("uploads/new.jpg", 1, 1)]);
        let s = judge("example.com", Some(&b), &live_moved, &lf2, &local_same, &local_files, 2000);
        assert_eq!(s.verdict, Verdict::Behind);
        assert_eq!((s.tables_behind, s.tables_ahead), (1, 0));
        assert_eq!(s.files.live_changed, vec!["themes/x/style.css", "uploads/new.jpg"]);
        assert_eq!(s.files.live_gone, vec!["uploads/a.jpg"]);

        // only local moved: a post table and a file after `at` → ahead
        let local_moved = map(&[("wp_posts", "ck:2"), ("wp_options", "opt:9:7"), ("wp_users", "ck:3")]);
        let local_files2 = vec![("themes/x/style.css".to_string(), 1500), ("uploads/a.jpg".to_string(), 900)];
        let s = judge("example.com", Some(&b), &live_same, &lf, &local_moved, &local_files2, 2000);
        assert_eq!(s.verdict, Verdict::Ahead);
        assert_eq!(s.files.local_changed, vec!["themes/x/style.css"]);
        assert_eq!(s.tables.iter().find(|t| t.name == "wp_posts").unwrap().local, Change::Changed);

        // only wp_users moved locally (live-owned): listed, not counted → up to date
        let users_only = map(&[("wp_posts", "ck:1"), ("wp_options", "opt:9:7"), ("wp_users", "ck:9")]);
        let s = judge("example.com", Some(&b), &live_same, &lf, &users_only, &local_files, 2000);
        assert_eq!(s.verdict, Verdict::UpToDate);
        assert_eq!((s.tables_ahead, s.tables_ahead_live_owned), (0, 1));
        assert!(s.tables.iter().find(|t| t.name == "wp_users").unwrap().live_owned);

        // both → diverged, and the file both sides touched is named
        let s = judge("example.com", Some(&b), &live_moved, &lf2, &local_moved, &local_files2, 2000);
        assert_eq!(s.verdict, Verdict::Diverged);
        assert_eq!(s.files.both, vec!["themes/x/style.css"]);

        // no base → never synced, every table new on live
        let s = judge("example.com", None, &live_same, &lf, &local_same, &local_files, 2000);
        assert_eq!(s.verdict, Verdict::NeverSynced);
        assert!(s.tables.iter().all(|t| t.live == Change::New && t.local == Change::Unknown));

        // per-file local mtimes (a base since 14 Oct 2026): a file whose mtime differs from
        // the RECORDED one is changed here even when it is older than `at` — the shape an
        // incremental pull leaves (#863).
        let mut dated = b.clone();
        dated.local_files = [("themes/x/style.css", 500), ("uploads/a.jpg", 900)].into_iter().map(|(p, m)| (p.to_string(), m)).collect();
        let s = judge("example.com", Some(&dated), &live_same, &lf, &local_same, &local_files, 2000);
        assert_eq!(s.verdict, Verdict::Ahead, "{:?}", s.files);
        assert_eq!(s.files.local_changed, vec!["themes/x/style.css"]);

        // an old base without local stamps: local unknown, never "ahead" from tables alone
        let mut old = b.clone();
        old.local_tables.clear();
        let s = judge("example.com", Some(&old), &live_same, &lf, &local_moved, &local_files, 2000);
        assert_eq!(s.verdict, Verdict::UpToDate);
        assert!(s.tables.iter().all(|t| t.local == Change::Unknown));
    }
}
