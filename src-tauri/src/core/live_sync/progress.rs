//! Resume (plan §2.2 "a dropped request is retried from the cursor, never from zero",
//! S3 — built 15 Oct 2026): what an interrupted pull or push had already moved, kept
//! beside the scratch files so the NEXT one continues instead of starting over.
//!
//! A pull: the tables already imported into the staging database, each with the live
//! stamp it was exported under (`staged`); the tables already swapped into the local
//! database by a pull that then failed on its files (`swapped`); and the files already
//! verified into the incoming folder (which is kept, under a name per site). The next
//! pull reuses a staged table whose live stamp has not moved, treats a swapped one as
//! kept, and skips an incoming file whose size and mtime are the listing's.
//!
//! A push: the open push's id on live, what it was asked to send, and the local stamps
//! of those tables and files at the time. The next push asks the plugin to resume it
//! (`/push/begin` with `resume`, plugin 0.1.4): tables the plugin holds whole and files
//! it holds to the last byte — provided they have not changed here since — are not
//! sent again.
//!
//! Both records are plain JSON under the live-sync scratch folder, named by the local
//! database (one per site), and removed when the job finishes.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// An interrupted pull's progress.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PullProgress {
    /// Tables in the staging database, by the live stamp they were exported under.
    #[serde(default)]
    pub staged: BTreeMap<String, String>,
    /// Tables swapped into the local database, by the live stamp they carried.
    #[serde(default)]
    pub swapped: BTreeMap<String, String>,
}

/// An interrupted push: the open push on live and what it was asked to carry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushProgress {
    pub push_id: String,
    pub tables: Vec<String>,
    pub files: Vec<String>,
    /// The local stamps of `tables` when they were sent — a table whose stamp moved since
    /// is sent again.
    #[serde(default)]
    pub local_tables: BTreeMap<String, String>,
    /// The local mtimes of `files` when they were sent.
    #[serde(default)]
    pub local_files: BTreeMap<String, i64>,
}

pub fn pull_file(scratch: &Path, db_name: &str) -> PathBuf {
    scratch.join(format!("pull-{db_name}.progress.json"))
}

pub fn push_file(scratch: &Path, db_name: &str) -> PathBuf {
    scratch.join(format!("push-{db_name}.open.json"))
}

/// The incoming folder a pull writes verified files into — one per site, kept across
/// an interrupted pull so its files are not fetched twice.
pub fn incoming_dir(content: &Path, db_name: &str) -> PathBuf {
    content.join(format!(".rexsync-incoming-{db_name}"))
}

pub fn read<T: serde::de::DeserializeOwned>(p: &Path) -> Option<T> {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn write<T: Serialize>(p: &Path, v: &T) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serde_json::to_vec(v).expect("json"))?;
    Ok(())
}

pub fn forget(p: &Path) {
    let _ = std::fs::remove_file(p);
}

/// Which of the tables a push will send can be skipped: those the plugin holds whole
/// (`done_on_live`) whose local stamp is still what the record saw. Pure.
pub fn push_tables_to_skip(rec: &PushProgress, done_on_live: &[String], local_now: &BTreeMap<String, String>) -> Vec<String> {
    done_on_live.iter().filter(|t| rec.local_tables.contains_key(*t) && rec.local_tables.get(*t) == local_now.get(*t)).cloned().collect()
}

/// Which files can be skipped: those live holds to the last byte (`received`, by path →
/// bytes) whose local size and mtime are still what the record saw. Pure.
pub fn push_files_to_skip(rec: &PushProgress, received: &BTreeMap<String, u64>, local_now: &BTreeMap<String, (u64, i64)>) -> Vec<String> {
    local_now
        .iter()
        .filter(|(p, (size, mtime))| received.get(*p) == Some(size) && rec.local_files.get(*p) == Some(mtime))
        .map(|(p, _)| p.clone())
        .collect()
}

/// May `rec` be resumed for a push of exactly `tables` + `files`? Only when the open
/// push was asked for everything this one wants — a push that widened its list starts
/// over (the plugin's conflict verdict covered the old list). Pure.
pub fn push_resumable(rec: &PushProgress, tables: &[String], files: &[String]) -> bool {
    !rec.push_id.is_empty() && tables.iter().all(|t| rec.tables.contains(t)) && files.iter().all(|f| rec.files.contains(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A resumed push skips only what live holds WHOLE and what has not changed here
    /// since** (ledger #881): a table the plugin finished whose local stamp moved is sent
    /// again; a file received short, or edited since, is sent again; a push asking for a
    /// table the open push never named starts over. Plant: drop the stamp comparison →
    /// the edited table is skipped.
    #[test]
    fn a_resume_skips_only_the_unchanged_whole_parts() {
        let rec = PushProgress {
            push_id: "p1".into(),
            tables: vec!["wp_posts".into(), "wp_options".into()],
            files: vec!["a.txt".into(), "b.txt".into(), "c.txt".into()],
            local_tables: [("wp_posts", "ck:1"), ("wp_options", "opt:2:2")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            local_files: [("a.txt", 100), ("b.txt", 100), ("c.txt", 100)].into_iter().map(|(a, b)| (a.to_string(), b)).collect(),
        };
        let done = vec!["wp_posts".to_string(), "wp_options".to_string()];
        let now: BTreeMap<String, String> = [("wp_posts", "ck:1"), ("wp_options", "opt:3:3")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        assert_eq!(push_tables_to_skip(&rec, &done, &now), vec!["wp_posts"]);
        let received: BTreeMap<String, u64> = [("a.txt", 5u64), ("b.txt", 3), ("c.txt", 5)].into_iter().map(|(a, b)| (a.to_string(), b)).collect();
        let local: BTreeMap<String, (u64, i64)> = [("a.txt", (5u64, 100i64)), ("b.txt", (5, 100)), ("c.txt", (5, 200))].into_iter().map(|(a, b)| (a.to_string(), b)).collect();
        assert_eq!(push_files_to_skip(&rec, &received, &local), vec!["a.txt"], "b short on live, c edited here");
        assert!(push_resumable(&rec, &["wp_posts".into()], &["a.txt".into()]));
        assert!(!push_resumable(&rec, &["wp_terms".into()], &[]), "a table the open push never named");
        assert!(!push_resumable(&PushProgress::default(), &[], &[]));
    }
}
