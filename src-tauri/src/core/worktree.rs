//! Git worktrees as sites (`docs/PLAN-git-worktrees.md`).
//!
//! A worktree child is a site whose code — the whole docroot, or one plugin/theme
//! dir inside a copied WordPress — is a `git worktree` of another site's checkout.
//! A worktree is the one kind of folder whose `.git` is a FILE (`gitdir: <path>`)
//! pointing at a repository that lives somewhere ELSE, and that is the fact this
//! module's first piece is about: a folder like that may hold uncommitted work
//! whose repository will not notice it vanish, so rexenv must never delete it
//! with `remove_dir_all` (ledger #814). Only `git worktree remove` — which refuses
//! a dirty worktree on its own — may.

use std::path::{Component, Path, PathBuf};

/// The repository directory a `.git` FILE points at, resolved against the
/// file's own folder (git writes a relative path when `worktree.useRelativePaths`
/// is set, an absolute one otherwise). `None` when the file does not hold a
/// `gitdir:` line.
pub fn gitdir_of(dot_git_file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dot_git_file).ok()?;
    let line = text.lines().find_map(|l| l.trim().strip_prefix("gitdir:"))?;
    let raw = line.trim();
    if raw.is_empty() {
        return None;
    }
    let p = PathBuf::from(raw);
    let base = dot_git_file.parent()?;
    Some(normalize(&if p.is_absolute() { p } else { base.join(p) }))
}

/// Lexical `.`/`..` folding — the gitdir may not exist any more (a pruned or
/// moved repository), so `canonicalize` cannot be the only answer.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `p` resolved as far as the filesystem allows: its longest EXISTING ancestor
/// canonicalized (so a symlinked sites folder, or macOS's `/var` →
/// `/private/var`, compares equal to the canonical root), the missing tail
/// appended lexically. Canonicalizing only whole paths was wrong for exactly
/// the case that matters: a gitdir that does not exist (yet, or any more) came
/// back un-canonical and compared unequal to its own tree.
fn resolved(p: &Path) -> PathBuf {
    let p = normalize(p);
    let mut tail = Vec::new();
    let mut cur = p.as_path();
    loop {
        if let Ok(c) = std::fs::canonicalize(cur) {
            return tail.iter().rev().fold(c, |acc: PathBuf, part| acc.join(part));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                cur = parent;
            }
            _ => return p,
        }
    }
}

/// The first checkout under `root` whose repository lives OUTSIDE `root` — a
/// git worktree (or a submodule of some other repo) whose `.git` is a file
/// pointing elsewhere. `None` = nothing under `root` belongs to a repository
/// that would outlive deleting `root`.
///
/// What this deliberately does NOT flag: a `.git` file whose gitdir is inside
/// `root` (a submodule of a repo that is itself under `root` — deleting `root`
/// takes the repo and the checkout together), and a `.git` DIRECTORY (an
/// ordinary clone, which is its own repository).
///
/// Fail-safe both ways it can be unsure: a `.git` file that cannot be read or
/// holds no `gitdir:` line counts as foreign, and a directory that cannot be
/// listed is reported as the hit — "could not look" never means "nothing there".
/// Symlinks are not followed (a link out of the tree is not the tree, and
/// `remove_dir_all` removes the link, not its target). `.git` directories are
/// not descended into.
pub fn foreign_checkout_under(root: &Path) -> Option<PathBuf> {
    let root_resolved = resolved(root);
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && dir == root => return None,
            Err(_) => return Some(dir),
        };
        for entry in entries {
            let Ok(entry) = entry else { return Some(dir) };
            let Ok(ft) = entry.file_type() else { return Some(entry.path()) };
            let name = entry.file_name();
            if name == ".git" {
                if ft.is_file() {
                    match gitdir_of(&entry.path()) {
                        Some(gd) if resolved(&gd).starts_with(&root_resolved) => {}
                        _ => return Some(dir),
                    }
                }
                continue;
            }
            if ft.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexenv-wt-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn gitdir_is_read_absolute_and_relative() {
        let d = tmp("gitdir");
        let f = d.join(".git");
        std::fs::write(&f, "gitdir: /repo/.git/worktrees/feature-x\n").unwrap();
        assert_eq!(gitdir_of(&f).unwrap(), PathBuf::from("/repo/.git/worktrees/feature-x"));
        std::fs::write(&f, "gitdir: ../main/.git/worktrees/x\n").unwrap();
        assert_eq!(gitdir_of(&f).unwrap(), normalize(&d.join("../main/.git/worktrees/x")));
        std::fs::write(&f, "not a pointer\n").unwrap();
        assert!(gitdir_of(&f).is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// **A managed docroot holding a worktree whose repository lives elsewhere is
    /// found, wherever it sits in the tree** (ledger #814). This is the check the
    /// site delete preflight and teardown's `remove_dir_all` both stand on. Plant:
    /// make `foreign_checkout_under` skip `.git` FILES (treat them like dirs) and
    /// the first assert fails.
    #[test]
    fn a_worktree_whose_repository_is_outside_the_tree_is_found() {
        let site = tmp("site");
        let repo = tmp("repo");
        let plugin = site.join("wp-content/plugins/my-plugin");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join(".git"),
            format!("gitdir: {}\n", repo.join(".git/worktrees/my-plugin").display()),
        )
        .unwrap();
        assert_eq!(foreign_checkout_under(&site), Some(plugin.clone()));

        // A relative gitdir that climbs out of the tree is just as foreign.
        std::fs::write(plugin.join(".git"), "gitdir: ../../../../elsewhere/.git/worktrees/a\n")
            .unwrap();
        assert_eq!(foreign_checkout_under(&site), Some(plugin.clone()));

        // Unreadable intent counts as foreign: no gitdir line.
        std::fs::write(plugin.join(".git"), "garbage").unwrap();
        assert_eq!(foreign_checkout_under(&site), Some(plugin));

        std::fs::remove_dir_all(&site).unwrap();
        std::fs::remove_dir_all(&repo).unwrap();
    }

    #[test]
    fn ordinary_clones_and_inner_submodules_are_not_foreign() {
        let site = tmp("plain");
        // An ordinary clone: `.git` is a directory — its own repository.
        let clone = site.join("wp-content/plugins/cloned");
        std::fs::create_dir_all(clone.join(".git/objects")).unwrap();
        // A submodule whose repository lives inside the same tree.
        let sub = clone.join("vendor/lib");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), "gitdir: ../../.git/modules/lib\n").unwrap();
        // A `.git` file INSIDE a `.git` dir is never looked at.
        std::fs::write(clone.join(".git/objects/.git"), "gitdir: /far/away\n").unwrap();
        assert_eq!(foreign_checkout_under(&site), None);
        assert_eq!(foreign_checkout_under(&site.join("does-not-exist")), None);
        std::fs::remove_dir_all(&site).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_tree_is_not_followed() {
        let site = tmp("link");
        let outside = tmp("outside");
        std::fs::write(outside.join(".git"), "gitdir: /far/away\n").unwrap();
        std::os::unix::fs::symlink(&outside, site.join("linked")).unwrap();
        assert_eq!(foreign_checkout_under(&site), None);
        std::fs::remove_dir_all(&site).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }
}
