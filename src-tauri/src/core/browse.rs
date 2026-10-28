//! core::browse — the web UI's folder and file picker, server-side (docs/PLAN-headless-web.md T8).
//!
//! A browser cannot open the server's native dialog (the window's `tauri-plugin-dialog` picker
//! would open on a display nobody sees), so the web UI lists the server's directories through this
//! and renders its own picker. **Only under the roots** — the user's home and the Sites folder —
//! and a path is resolved (symlinks followed) BEFORE it is compared, so a link inside home that
//! points at `/etc` lists nothing (ledger #931). Hidden entries are left out, as a desktop
//! dialog leaves them out by default.

use crate::error::{Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub dir: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    /// The directory listed, resolved.
    pub path: String,
    /// Its parent, when the parent is still under a root.
    pub parent: Option<String>,
    /// Where the picker can jump to: the roots themselves.
    pub roots: Vec<String>,
    pub entries: Vec<Entry>,
}

/// The sentence for a path outside every root.
pub const OUTSIDE: &str = "rexenv's web picker lists only your home folder and your Sites folder";

/// Whether `path` (already resolved) is one of `roots` or inside one.
pub fn under_a_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

/// List `path` (or the first root), directories first, then files whose extension is in
/// `extensions` (none = directories only — a folder picker), each group by name.
pub fn list(path: Option<&str>, roots: &[PathBuf], extensions: &[String]) -> Result<DirListing> {
    let roots: Vec<PathBuf> = roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).collect();
    let first = roots.first().ok_or_else(|| Error::Other("no folder to browse".into()))?;
    let asked = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => first.clone(),
    };
    let dir = std::fs::canonicalize(&asked).map_err(|e| Error::Other(format!("{}: {e}", asked.display())))?;
    if !under_a_root(&dir, &roots) {
        return Err(Error::Other(OUTSIDE.into()));
    }
    if !dir.is_dir() {
        return Err(Error::Other(format!("{} is not a folder", dir.display())));
    }
    let wanted = |name: &str| {
        let lower = name.to_ascii_lowercase();
        extensions.iter().any(|e| lower.ends_with(&format!(".{}", e.trim_start_matches('.').to_ascii_lowercase())))
    };
    let mut entries = Vec::new();
    for e in std::fs::read_dir(&dir)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let p = e.path();
        // `is_dir` follows a link; a link out of the roots is listed but refuses when opened.
        let is_dir = p.is_dir();
        if is_dir || (!extensions.is_empty() && p.is_file() && wanted(&name)) {
            entries.push(Entry { name, path: p.display().to_string(), dir: is_dir });
        }
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let parent = dir.parent().filter(|p| under_a_root(p, &roots)).map(|p| p.display().to_string());
    Ok(DirListing {
        path: dir.display().to_string(),
        parent,
        roots: roots.iter().map(|r| r.display().to_string()).collect(),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway tree, removed when dropped.
    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tree() -> (Tmp, PathBuf, PathBuf) {
        let t = Tmp(std::env::temp_dir().join(format!("rexenv-browse-{}", uuid::Uuid::new_v4().simple())));
        let home = t.0.join("home");
        let outside = t.0.join("etc");
        std::fs::create_dir_all(home.join("Sites/blog")).unwrap();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(home.join("dump.SQL"), "x").unwrap();
        std::fs::write(home.join("notes.txt"), "x").unwrap();
        std::fs::write(outside.join("passwd"), "x").unwrap();
        (t, home, outside)
    }

    /// Ledger #931.
    #[test]
    fn the_picker_lists_only_under_its_roots_and_follows_a_link_before_it_judges() {
        let (_t, home, outside) = tree();
        let roots = vec![home.clone()];
        let l = list(None, &roots, &[]).unwrap();
        let names: Vec<&str> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Sites"], "folders only, no dot-entries");
        assert_eq!(l.parent, None, "the root's parent is outside");
        let files = list(None, &roots, &["sql".into()]).unwrap();
        assert!(files.entries.iter().any(|e| e.name == "dump.SQL" && !e.dir));
        assert!(!files.entries.iter().any(|e| e.name == "notes.txt"));
        assert!(list(Some(&outside.display().to_string()), &roots, &[]).unwrap_err().to_string().contains(OUTSIDE));
        let dotdot = format!("{}/Sites/../..", home.display());
        assert!(list(Some(&dotdot), &roots, &[]).is_err(), "`..` resolves before the check");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, home.join("Sites/escape")).unwrap();
            let via_link = format!("{}/Sites/escape", home.display());
            assert!(list(Some(&via_link), &roots, &[]).unwrap_err().to_string().contains(OUTSIDE));
        }
        let sub = list(Some(&format!("{}/Sites", home.display())), &roots, &[]).unwrap();
        assert_eq!(sub.parent.as_deref(), Some(std::fs::canonicalize(&home).unwrap().to_str().unwrap()));
    }
}
