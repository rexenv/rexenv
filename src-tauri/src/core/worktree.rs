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

use crate::core::repo::{self, CancelToken};
use crate::error::{Error, Result};
use crate::platform::traits::ProcessSupervisor;
use crate::state::models::MultisiteMode;
use crate::state::store;
use rusqlite::Connection;
use serde::Serialize;
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

// ── The branch → domain rule (§2.2) ───────────────────────────────────────────

/// The oldest git that can do everything this feature asks of it: `worktree
/// remove` arrived in 2.17 (`add` in 2.5, `list --porcelain` in 2.7). Every
/// git rexenv meets in practice is far newer (Xcode CLT 2.39, Ubuntu 22.04's
/// 2.34, Git for Windows 2.4x); the floor exists so an old one fails with a
/// sentence instead of `git: 'remove' is not a git command` half-way through.
pub const MIN_GIT: (u32, u32) = (2, 17);

/// `(major, minor)` out of `git --version` output (`git version 2.39.5 (Apple
/// Git-154)`, `git version 2.45.1.windows.1`). `None` when it is not there.
pub fn parse_git_version(line: &str) -> Option<(u32, u32)> {
    let rest = line.trim().strip_prefix("git version ")?;
    let mut parts = rest.split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Refuse a git too old for worktrees, naming this OS's way to a newer one
/// (`platform::words`). An unreadable version is let through: the probe is a
/// courtesy, and refusing a git we merely failed to read would block users the
/// floor was never about.
pub fn require_worktree_git(version_line: Option<&str>) -> Result<()> {
    match version_line.and_then(parse_git_version) {
        Some(v) if v < MIN_GIT => Err(Error::Other(format!(
            "Your git ({}.{}) is too old for worktrees — rexenv needs {}.{} or newer. {}",
            v.0,
            v.1,
            MIN_GIT.0,
            MIN_GIT.1,
            crate::platform::words::current().git_install
        ))),
        _ => Ok(()),
    }
}

/// A DNS label made from a branch name: lowercase, every run of characters
/// outside `[a-z0-9]` one `-`, no `-` at either end, at most `max` bytes.
/// `feature/Checkout_v2` → `feature-checkout-v2`. Empty when the branch has no
/// letter or digit at all — the caller refuses that.
pub fn branch_slug(branch: &str, max: usize) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in branch.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c);
        } else {
            dash = true;
        }
    }
    out.truncate(max);
    out.trim_end_matches('-').to_string()
}

/// Why a child did not get the `feature-x.shop.rex` shape. Each one is a
/// sentence in the New-worktree dialog, so it is a type, not a bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DomainFallback {
    /// The parent is a subdomain multisite: nginx already serves `*.shop.rex`
    /// from the parent's block, and an exact-name child would win there and
    /// silently shadow the network sub-site of that name, now or later.
    SubdomainNetwork,
    /// The parent's own domain is already below a site name (`a.shop.rex`);
    /// a worktree would add one more level each time.
    NestedParent,
    /// `feature-x.shop.rex` is already a site, an alias, or inside some
    /// subdomain network's wildcard.
    Taken,
}

impl DomainFallback {
    pub fn sentence(self) -> &'static str {
        match self {
            DomainFallback::SubdomainNetwork => {
                "The parent is a subdomain multisite, so a name under it belongs to the network."
            }
            DomainFallback::NestedParent => {
                "The parent's domain is already nested, so the worktree stays at the same level."
            }
            DomainFallback::Taken => "That name under the parent is already taken.",
        }
    }
}

/// The domain a new worktree child gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildDomain {
    pub domain: String,
    /// `None` = the preferred `<slug>.<parent>` shape.
    pub fallback: Option<DomainFallback>,
}

/// The §2.2 rule, pure. `taken` answers "does any site already answer on this
/// exact name" (site domains, aliases, and subdomain-network wildcards — see
/// [`domain_taken`]); `parent_is_subdomain_network` is the parent's multisite
/// mode. Preferred: `feature-x.shop.rex`. Fallback: the parent's FIRST label
/// joined to the slug (`shop-feature-x.rex`, `a-feature-x.shop.rex`), then
/// `-2`, `-3`… if that is taken too.
///
/// No length reason exists, on purpose: the nested shape is only used for a
/// two-label parent, and slug (≤ 63) + `.` + label (≤ 63) + `.` + TLD (≤ 63) is
/// at most 191 bytes — DNS's 253 cannot be reached. (W2 first had a `TooLong`
/// fallback; its test could not construct a case, which is how it went.)
pub fn derive_domain(
    parent_domain: &str,
    parent_is_subdomain_network: bool,
    branch: &str,
    taken: &dyn Fn(&str) -> bool,
) -> Result<ChildDomain> {
    let (first, rest) = parent_domain
        .split_once('.')
        .ok_or_else(|| Error::Other(format!("'{parent_domain}' has no TLD")))?;
    let slug = branch_slug(branch, 63);
    if slug.is_empty() {
        return Err(Error::Other(format!(
            "The branch name \"{branch}\" has no letters or digits to make a domain from — \
             type one in the domain field."
        )));
    }
    let nested = format!("{slug}.{parent_domain}");
    let fallback = if parent_is_subdomain_network {
        Some(DomainFallback::SubdomainNetwork)
    } else if rest.contains('.') {
        Some(DomainFallback::NestedParent)
    } else if taken(&nested) {
        Some(DomainFallback::Taken)
    } else {
        None
    };
    let Some(reason) = fallback else {
        return Ok(ChildDomain { domain: nested, fallback: None });
    };
    // `<first>-<slug>[-N]` must stay one label of ≤ 63 bytes: the slug gives way.
    for n in 1..=99u32 {
        let suffix = if n == 1 { String::new() } else { format!("-{n}") };
        let room = 63usize.saturating_sub(first.len() + 1 + suffix.len());
        let s = branch_slug(&slug, room);
        if s.is_empty() {
            break;
        }
        let candidate = format!("{first}-{s}{suffix}.{rest}");
        if candidate.len() <= 253 && !taken(&candidate) {
            return Ok(ChildDomain { domain: candidate, fallback: Some(reason) });
        }
    }
    Err(Error::Other(format!(
        "No free domain for branch \"{branch}\" under {parent_domain} — type one in the domain field."
    )))
}

/// Does any site already answer on `domain`? Its own domain, one of its
/// aliases, or — for a subdomain network — anything under either, which is
/// exactly the `server_name` set nginx gives it (`core::services::server_block`
/// adds `*.<name>` for the domain AND every alias of a subdomain network).
pub fn domain_taken(conn: &Connection, domain: &str) -> Result<bool> {
    let aliases = store::all_site_aliases(conn)?;
    for site in store::list_sites(conn)? {
        let own = std::iter::once(site.domain.as_str());
        let extra = aliases.get(&site.id).into_iter().flatten().map(String::as_str);
        for name in own.chain(extra) {
            if name == domain {
                return Ok(true);
            }
            if site.multisite == MultisiteMode::Subdomain
                && domain.len() > name.len()
                && domain.ends_with(name)
                && domain.as_bytes()[domain.len() - name.len() - 1] == b'.'
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

// ── `git worktree` itself ─────────────────────────────────────────────────────

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeEntry {
    pub path: PathBuf,
    /// Short branch name (`refs/heads/` stripped); `None` when detached or bare.
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    /// git's own verdict that the folder is gone (`prunable`, git ≥ 2.31).
    pub prunable: bool,
}

/// Parse `git worktree list --porcelain`. Records start at `worktree <path>`;
/// blank separators may already be gone (`run_git_lines` drops them), so the
/// record boundary is that line, never the blank. The first record is the main
/// worktree. Unknown attribute lines are ignored (git adds them over time).
/// Paths are taken as git prints them — `/`-separated on every OS, including
/// Windows (`C:/Users/…`), which `PathBuf` accepts as is.
pub fn parse_porcelain<S: AsRef<str>>(lines: &[S]) -> Vec<WorktreeEntry> {
    let mut out: Vec<WorktreeEntry> = Vec::new();
    for raw in lines {
        let line = raw.as_ref().trim_end_matches(['\r', '\n']);
        if let Some(p) = line.strip_prefix("worktree ") {
            out.push(WorktreeEntry {
                path: PathBuf::from(p),
                branch: None,
                head: None,
                detached: false,
                bare: false,
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(cur) = out.last_mut() else { continue };
        let (key, val) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "HEAD" => cur.head = Some(val.to_string()),
            "branch" => {
                cur.branch = Some(val.strip_prefix("refs/heads/").unwrap_or(val).to_string())
            }
            "detached" => cur.detached = true,
            "bare" => cur.bare = true,
            "locked" => cur.locked = true,
            "prunable" => cur.prunable = true,
            _ => {}
        }
    }
    out
}

/// Every worktree of the repository `repo_dir` belongs to (any of its worktrees
/// answers for all of them), main worktree first.
pub fn list(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    repo_dir: &Path,
) -> Result<Vec<WorktreeEntry>> {
    let lines = repo::run_git_lines(supervisor, git, env, repo_dir, &["worktree", "list", "--porcelain"])?;
    Ok(parse_porcelain(&lines))
}

/// What `add` checks out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddSpec {
    /// An existing local or remote-tracking branch (git DWIMs `origin/x` into a
    /// local tracking branch when the name is unique).
    Existing(String),
    /// A NEW branch made from `base`.
    New { branch: String, base: String },
}

/// Argv for `git worktree add`, pure. Both names have been through
/// `repo::validate_ref` (no leading `-`, no whitespace), and `path` is
/// absolute — so neither can be read as an option.
pub fn add_args(path: &Path, spec: &AddSpec) -> Vec<String> {
    let p = path.to_string_lossy().into_owned();
    match spec {
        AddSpec::Existing(b) => vec!["worktree".into(), "add".into(), p, b.clone()],
        AddSpec::New { branch, base } => vec![
            "worktree".into(),
            "add".into(),
            "-b".into(),
            branch.clone(),
            p,
            base.clone(),
        ],
    }
}

/// `git worktree add` into `path`, streamed into the job log. Refuses — before
/// git runs — a relative path, a path that already exists (git would accept an
/// EMPTY existing dir; rexenv only ever adds into a folder it is about to make,
/// the same posture as `clone_repo`), and a branch name `repo::validate_ref`
/// rejects.
#[allow(clippy::too_many_arguments)] // flat mirror of the step's inputs (clone_repo precedent)
pub fn add(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    repo_dir: &Path,
    path: &Path,
    spec: &AddSpec,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::Other(format!("worktree path {} is not absolute", path.display())));
    }
    if path.exists() {
        return Err(Error::Other(format!(
            "{} already exists — rexenv only adds a worktree into a folder it creates.",
            path.display()
        )));
    }
    match spec {
        AddSpec::Existing(b) => {
            repo::validate_ref(b)?;
        }
        AddSpec::New { branch, base } => {
            repo::validate_ref(branch)?;
            repo::validate_ref(base)?;
        }
    }
    repo::run_git_op(supervisor, git, env, repo_dir, "worktree add", &add_args(path, spec), cancel, on_line)
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

    #[test]
    fn git_version_floor() {
        assert_eq!(parse_git_version("git version 2.39.5 (Apple Git-154)"), Some((2, 39)));
        assert_eq!(parse_git_version("git version 2.45.1.windows.1\r\n"), Some((2, 45)));
        assert_eq!(parse_git_version("git version 2.34.1"), Some((2, 34)));
        assert_eq!(parse_git_version("hub version 2.14"), None);
        assert!(require_worktree_git(Some("git version 2.17.0")).is_ok());
        assert!(require_worktree_git(Some("git version 3.0.0")).is_ok());
        assert!(require_worktree_git(Some("garbage")).is_ok(), "an unreadable version is let through");
        assert!(require_worktree_git(None).is_ok(), "so is a probe that answered nothing");
        let err = require_worktree_git(Some("git version 2.16.6")).unwrap_err().to_string();
        assert!(err.contains("2.16") && err.contains("2.17"), "{err}");
        // The install hint is THIS OS's, never a literal from one OS.
        assert!(err.contains(crate::platform::words::current().git_install), "{err}");
    }

    #[test]
    fn branch_slugs() {
        assert_eq!(branch_slug("feature/Checkout_v2", 63), "feature-checkout-v2");
        assert_eq!(branch_slug("--fix//double--dash--", 63), "fix-double-dash");
        assert_eq!(branch_slug("main", 63), "main");
        assert_eq!(branch_slug("///", 63), "");
        assert_eq!(branch_slug("ফিচার", 63), "", "non-ASCII has nothing to keep");
        assert_eq!(branch_slug("abc-def", 4), "abc", "a cut never ends on '-'");
        assert_eq!(branch_slug(&"x".repeat(100), 63).len(), 63);
    }

    fn none_taken(_: &str) -> bool {
        false
    }

    /// **The §2.2 rule: `feature-x.shop.rex` first, `shop-feature-x.rex` when
    /// the nested name would be a problem** (ledger #816). Each fallback reason
    /// is asserted on its own, so a deleted branch of the rule fails by name.
    #[test]
    fn child_domains_prefer_nested_and_fall_back_for_each_reason() {
        let d = derive_domain("shop.rex", false, "feature/x", &none_taken).unwrap();
        assert_eq!(d, ChildDomain { domain: "feature-x.shop.rex".into(), fallback: None });

        let d = derive_domain("shop.rex", true, "feature/x", &none_taken).unwrap();
        assert_eq!(d.domain, "shop-feature-x.rex");
        assert_eq!(d.fallback, Some(DomainFallback::SubdomainNetwork));

        let d = derive_domain("a.shop.rex", false, "fix", &none_taken).unwrap();
        assert_eq!(d.domain, "a-fix.shop.rex", "same depth as the parent");
        assert_eq!(d.fallback, Some(DomainFallback::NestedParent));

        let taken = |n: &str| n == "fix.shop.rex";
        let d = derive_domain("shop.rex", false, "fix", &taken).unwrap();
        assert_eq!(d.domain, "shop-fix.rex");
        assert_eq!(d.fallback, Some(DomainFallback::Taken));

        // The fallback is taken too → -2, -3.
        let taken = |n: &str| n == "fix.shop.rex" || n == "shop-fix.rex" || n == "shop-fix-2.rex";
        let d = derive_domain("shop.rex", false, "fix", &taken).unwrap();
        assert_eq!(d.domain, "shop-fix-3.rex");

        // The fallback label never exceeds 63 bytes, suffix included.
        let d = derive_domain(&format!("{}.rex", "s".repeat(40)), true, &"b".repeat(63), &none_taken)
            .unwrap();
        let label = d.domain.split('.').next().unwrap();
        assert!(label.len() <= 63 && !label.ends_with('-'), "{label}");
        assert!(crate::core::sites::validate_domain(&d.domain).is_ok(), "{}", d.domain);

        assert!(derive_domain("shop.rex", false, "///", &none_taken).is_err());
    }

    #[test]
    fn the_longest_nested_name_still_fits_dns() {
        let parent = format!("{}.{}", "p".repeat(63), "t".repeat(63));
        let d = derive_domain(&parent, false, &"z".repeat(80), &none_taken).unwrap();
        assert_eq!(d.fallback, None);
        assert_eq!(d.domain.len(), 63 + 1 + 63 + 1 + 63);
    }

    #[test]
    fn porcelain_parses_every_record_shape() {
        let raw = [
            "worktree /Users/me/Sites/shop.rex",
            "HEAD 1111111111111111111111111111111111111111",
            "branch refs/heads/main",
            "",
            "worktree /Users/me/Sites/feature-x.shop.rex",
            "HEAD 2222222222222222222222222222222222222222",
            "branch refs/heads/feature/x",
            "locked",
            "worktree /Users/me/Sites/gone.rex",
            "HEAD 3333333333333333333333333333333333333333",
            "detached",
            "prunable gitdir file points to non-existent location",
            "worktree C:/Users/dell/rexenv/Sites/win.rex\r",
            "HEAD 4444444444444444444444444444444444444444\r",
            "branch refs/heads/win\r",
            "some-future-attribute value",
        ];
        let e = parse_porcelain(&raw);
        assert_eq!(e.len(), 4);
        assert_eq!(e[0].branch.as_deref(), Some("main"));
        assert_eq!(e[0].path, PathBuf::from("/Users/me/Sites/shop.rex"));
        assert_eq!(e[1].branch.as_deref(), Some("feature/x"));
        assert!(e[1].locked && !e[1].detached);
        assert!(e[2].detached && e[2].prunable && e[2].branch.is_none());
        assert_eq!(e[3].path, PathBuf::from("C:/Users/dell/rexenv/Sites/win.rex"), "CR stripped");
        assert_eq!(e[3].branch.as_deref(), Some("win"));
        assert!(parse_porcelain::<&str>(&[]).is_empty());
        assert!(parse_porcelain(&["HEAD orphan-before-any-record"]).is_empty());
    }

    #[test]
    fn add_args_put_options_before_the_path() {
        let p = Path::new("/s/x.rex/wp-content/plugins/p");
        assert_eq!(
            add_args(p, &AddSpec::Existing("feature/x".into())),
            ["worktree", "add", "/s/x.rex/wp-content/plugins/p", "feature/x"]
        );
        assert_eq!(
            add_args(p, &AddSpec::New { branch: "fix".into(), base: "main".into() }),
            ["worktree", "add", "-b", "fix", "/s/x.rex/wp-content/plugins/p", "main"]
        );
    }

    #[test]
    fn taken_covers_domains_aliases_and_subdomain_network_wildcards() {
        use crate::state::db;
        let conn = db::open_in_memory().unwrap();
        let mk = |id: &str, domain: &str, ms: &str| {
            conn.execute(
                "INSERT INTO sites (id, name, domain, type, php_version, path, multisite) \
                 VALUES (?1, ?1, ?2, 'wordpress', '8.3', '/nowhere', ?3)",
                rusqlite::params![id, domain, ms],
            )
            .unwrap();
            store::get_site(&conn, id).unwrap().unwrap()
        };
        let plain = mk("shop", "shop.rex", "none");
        let net = mk("net", "net.rex", "subdomain");
        assert_eq!(net.multisite, MultisiteMode::Subdomain);
        store::add_site_alias(&conn, &net.id, "net-alias.rex").unwrap();
        store::add_site_alias(&conn, &plain.id, "shop-alias.rex").unwrap();

        assert!(domain_taken(&conn, "shop.rex").unwrap());
        assert!(domain_taken(&conn, "shop-alias.rex").unwrap());
        assert!(!domain_taken(&conn, "x.shop.rex").unwrap(), "a plain site owns no wildcard");
        assert!(domain_taken(&conn, "x.net.rex").unwrap(), "a network owns its wildcard");
        assert!(domain_taken(&conn, "x.net-alias.rex").unwrap(), "…and its aliases' wildcards");
        assert!(!domain_taken(&conn, "xnet.rex").unwrap(), "a suffix without the dot is not under it");
        assert!(!domain_taken(&conn, "free.rex").unwrap());
    }
}
