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
    let spec = match spec {
        AddSpec::Existing(b) => {
            repo::validate_ref(b)?;
            spec.clone()
        }
        AddSpec::New { branch, base } => {
            repo::validate_ref(branch)?;
            repo::validate_ref(base)?;
            // A Retry rebuilds the request from the row, and `-b` refuses a branch
            // that already exists — which it does after any earlier attempt got as
            // far as creating it (a cancel mid-checkout, a remove whose site delete
            // then failed). The branch the user asked to make IS that branch, so
            // check it out (review, 10 Oct 2026).
            if branch_exists(supervisor, git, env, repo_dir, branch) {
                on_line(&format!("branch {branch} already exists — checking it out"));
                AddSpec::Existing(branch.clone())
            } else {
                spec.clone()
            }
        }
    };
    repo::run_git_op(supervisor, git, env, repo_dir, "worktree add", &add_args(path, &spec), cancel, on_line)
}

/// Does `refs/heads/<branch>` exist in the repository at `repo_dir`?
pub fn branch_exists(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    repo_dir: &Path,
    branch: &str,
) -> bool {
    repo::run_git_lines(supervisor, git, env, repo_dir, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
        .map(|l| !l.is_empty())
        .unwrap_or(false)
}

/// Is `path` a REGISTERED worktree of the repository at `repo_dir`, with a
/// complete checkout? A Retry skips the `worktree` phase only on this answer, not
/// on "a `.git` file exists": a `git worktree add` cancelled mid-checkout leaves
/// the file with the checkout half done, and that read as "already checked out"
/// (review, 10 Oct 2026). A clean `git status` is the "complete" half.
pub fn checked_out(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    repo_dir: &Path,
    path: &Path,
) -> Result<bool> {
    let want = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let registered = list(supervisor, git, env, repo_dir)?
        .iter()
        .any(|e| std::fs::canonicalize(&e.path).unwrap_or_else(|_| e.path.clone()) == want);
    Ok(registered && uncommitted(supervisor, git, env, path)?.iter().all(|l| is_rexenv_owned(l)))
}

// ── Shape A: a WordPress copy with one plugin/theme dir as a worktree (W4/W5) ──

/// Which kind of `wp-content` checkout a Shape A worktree replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Plugin,
    Theme,
}

impl AssetKind {
    pub fn as_db(self) -> &'static str {
        match self {
            AssetKind::Plugin => "plugin",
            AssetKind::Theme => "theme",
        }
    }
    pub fn parse_db(s: &str) -> Option<Self> {
        match s {
            "plugin" => Some(AssetKind::Plugin),
            "theme" => Some(AssetKind::Theme),
            _ => None,
        }
    }
    fn folder(self) -> &'static str {
        match self {
            AssetKind::Plugin => "plugins",
            AssetKind::Theme => "themes",
        }
    }
}

/// `<content dir>/plugins/<dir>` (or `themes/`), relative to a WordPress
/// docroot. Refuses a `dir` that is not ONE plain folder name — it comes over
/// IPC, and it is joined onto two docroots, one of which is about to be filled.
pub fn asset_rel(content_dir_rel: &str, kind: AssetKind, dir: &str) -> Result<PathBuf> {
    let ok = !dir.is_empty()
        && dir != "."
        && dir != ".."
        && !dir.starts_with('.')
        && !dir.contains(['/', '\\', '\0', ':']);
    if !ok {
        return Err(Error::Other(format!("\"{dir}\" is not a plugin or theme folder name")));
    }
    Ok(Path::new(content_dir_rel).join(kind.folder()).join(dir))
}

/// What [`copy_site_tree`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CopyStats {
    pub files: u64,
    pub bytes: u64,
    /// Links found and NOT copied (relative to the source). A link is neither
    /// followed — a symlinked plugin points at somebody's checkout, and copying
    /// THROUGH it would duplicate that checkout as an unrelated folder — nor
    /// recreated (Windows needs Developer Mode or admin for that). The job log
    /// names each one.
    pub skipped_links: Vec<PathBuf>,
}

/// Copy a WordPress docroot into `dest` for a worktree child: every directory
/// and regular file, except the relative paths in `skip` (the asset dir the
/// worktree will occupy; `uploads` when asked). `dest` must NOT exist — the
/// caller copies into a staging dir it then renames into place (the
/// git-site-clone pattern), so a half copy is never the docroot.
///
/// `std::fs::copy` is copy-on-write on APFS (`core::scratch::clone_tree`'s
/// measurement), so the copy costs time per file, not disk per byte, there.
/// `cancelled` is polled per entry.
pub fn copy_site_tree(
    source: &Path,
    dest: &Path,
    skip: &[PathBuf],
    cancelled: &dyn Fn() -> bool,
) -> Result<CopyStats> {
    if dest.exists() {
        return Err(Error::Other(format!("{} already exists — not copying over it", dest.display())));
    }
    let mut stats = CopyStats::default();
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        std::fs::create_dir_all(dest.join(&rel))?;
        for entry in std::fs::read_dir(source.join(&rel))? {
            if cancelled() {
                return Err(Error::Other("copy cancelled".into()));
            }
            let entry = entry?;
            let child = rel.join(entry.file_name());
            if skip.iter().any(|s| s == &child) {
                continue;
            }
            let ft = entry.file_type()?; // does not follow links
            // A `.git` FILE whose repository is outside the source tree (another
            // worktree, a submodule of an outside repo): a copy would be a second
            // checkout pointing at the SAME `.git/worktrees/<x>` — git commands in
            // the copy would move the original's HEAD, and the copy could never be
            // `git worktree remove`d, so its site could never be deleted (review,
            // 10 Oct 2026). Left out, and listed with the links.
            if ft.is_file() && entry.file_name() == ".git" {
                let inside = gitdir_of(&entry.path()).is_some_and(|g| resolved(&g).starts_with(resolved(source)));
                if !inside {
                    stats.skipped_links.push(child);
                    continue;
                }
            }
            if ft.is_symlink() {
                stats.skipped_links.push(child);
            } else if ft.is_dir() {
                stack.push(child);
            } else if ft.is_file() {
                stats.bytes += std::fs::copy(entry.path(), dest.join(&child))?;
                stats.files += 1;
            }
        }
    }
    Ok(stats)
}

/// Where Shape B worktree folders live: `Worktrees/` BESIDE the sites folder
/// (`~/rexenv/Sites` → `~/rexenv/Worktrees/<domain>`), not inside it. A Shape B
/// child is a LINKED site — its folder is git's, removed only by `git worktree
/// remove` (ledger #814) — and the linked-folder rule refuses anything inside
/// the managed sites folder, on purpose: there, linking would only opt a folder
/// out of cleanup. For a worktree that opt-out is the point, but the rule is
/// right for every other link, so the worktrees get their own place instead of
/// an exception to it.
pub fn site_worktrees_dir(sites_dir: &Path) -> Result<PathBuf> {
    let parent = sites_dir
        .parent()
        .ok_or_else(|| Error::Other(format!("{} has no parent folder", sites_dir.display())))?;
    Ok(parent.join("Worktrees"))
}

/// The ignored config files a Shape B worktree copies from its parent's
/// project root when it lacks them (§2.5) — a FIXED list, never "every ignored
/// file": `vendor/` is rebuilt or copied by rule, uploads are content, and an
/// ignored file nobody listed is something the user keeps out of git on purpose.
/// `wp-config.php` is added by the caller at the SERVED root.
pub const CONFIG_FILES: [&str; 3] = [".env", ".env.local", "auth.json"];

/// Byte-identical files — both must exist. A cheap length check first.
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) if x.is_file() && y.is_file() && x.len() == y.len() => {
            matches!((std::fs::read(a), std::fs::read(b)), (Ok(p), Ok(q)) if p == q)
        }
        _ => false,
    }
}

/// The rexenv mu-plugins that name ONE site and must not ride along in a copy:
/// the tunnel rewrite, the login link, the scratch-mail stamp. The loopback-DNS
/// and mail-catch files are domain-agnostic and stay (the copy is served by
/// the same stack). Each remover is a no-op when its file is absent.
pub fn strip_site_specific_mu_plugins(docroot: &Path) -> Result<()> {
    crate::core::wp_tunnel::disable(docroot)?;
    crate::core::wp_login::remove(docroot)?;
    crate::core::wp_mailtag::disable(docroot)?;
    Ok(())
}

/// The `wp-config.php` constants a copy must re-point, in the order they are
/// set: the database, and the URL / network constants when the file defines
/// them. `DB_NAME` always; the rest only if present — `wp config set` would
/// otherwise ADD a `WP_HOME` the parent never had, pinning the copy's URL in a
/// second place.
pub fn wp_config_moves(wp_config: &str, db_name: &str, domain: &str) -> Vec<(&'static str, String)> {
    let mut out = vec![("DB_NAME", db_name.to_string())];
    for (name, value) in [
        ("WP_HOME", format!("https://{domain}")),
        ("WP_SITEURL", format!("https://{domain}")),
        ("DOMAIN_CURRENT_SITE", domain.to_string()),
    ] {
        if defines_constant(wp_config, name) {
            out.push((name, value));
        }
    }
    out
}

/// Does PHP source `define()` the constant `name` (either quote style, any
/// spacing)? Commented-out lines (`//`, `#`) do not count.
pub fn defines_constant(php: &str, name: &str) -> bool {
    php.lines().any(|l| {
        let t = l.trim_start();
        if t.starts_with("//") || t.starts_with('#') || t.starts_with('*') {
            return false;
        }
        let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        compact.contains(&format!("define('{name}'")) || compact.contains(&format!("define(\"{name}\""))
    })
}

// ── Removing a worktree (W6, §2.6) ─────────────────────────────────────────────

/// What `git status --porcelain` lists in a worktree — tracked changes and
/// untracked files, NOT ignored ones (`vendor/`, a build) — the work a removal
/// would destroy. Empty = clean.
pub fn uncommitted(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    worktree: &Path,
) -> Result<Vec<String>> {
    repo::run_git_lines(supervisor, git, env, worktree, &["status", "--porcelain", "--untracked-files=all"])
}

/// The refusal a dirty worktree gets — names the folder, the first files and
/// the two ways out. Pure.
pub fn dirty_refusal(worktree: &Path, files: &[String]) -> String {
    const SHOWN: usize = 8;
    let list: Vec<&str> = files.iter().take(SHOWN).map(String::as_str).collect();
    let more = files.len().saturating_sub(SHOWN);
    let tail = if more > 0 { format!("\n  … and {more} more") } else { String::new() };
    format!(
        "{} has {} uncommitted change{} — removing it would lose {}:\n  {}{tail}\n\
         Commit or stash them (the branch itself is kept either way), or remove it anyway.",
        worktree.display(),
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        if files.len() == 1 { "it" } else { "them" },
        list.join("\n  "),
    )
}

/// The path in one `git status --porcelain` line (`XY path`, or `XY old -> new`
/// for a rename — the new side), unquoted. Pure.
fn porcelain_path(line: &str) -> &str {
    let rest = line.get(3..).unwrap_or("").trim();
    let rest = rest.rsplit(" -> ").next().unwrap_or(rest);
    rest.trim_matches('"')
}

/// Is this porcelain line one of rexenv's OWN files — a `rexenv-*.php` it
/// writes into a site's `mu-plugins/` (login, mail catch, scratch mail, DNS,
/// tunnel)? rexenv regenerates those; they are never the user's work. Pure.
///
/// Found on Windows, 10 Oct 2026: a site worktree whose checkout carried them
/// had them re-written by the job and read as uncommitted work, so even a CLEAN
/// worktree site refused Delete. In the common case, where the repository does
/// not track them, they show as `??` on every OS — the same refusal everywhere.
pub fn is_rexenv_owned(porcelain_line: &str) -> bool {
    let p = Path::new(porcelain_path(porcelain_line));
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let in_mu = p.parent().and_then(|d| d.file_name()).and_then(|n| n.to_str()) == Some("mu-plugins");
    // The EXACT names rexenv writes (`sites::cleanup_muplugin_artifacts`' list), not
    // a `rexenv-*.php` prefix: a user's own `rexenv-notes.php` there is their work
    // (review, 10 Oct 2026).
    in_mu && REXENV_MU_PLUGINS.contains(&name)
}

/// Every mu-plugin file rexenv writes into a site: login, mail catch, scratch
/// mail, loopback DNS, tunnel.
pub const REXENV_MU_PLUGINS: [&str; 5] =
    ["rexenv-login.php", "rexenv-mail.php", "rexenv-scratch-mail.php", "rexenv-dns.php", "rexenv-tunnel.php"];

/// `git worktree remove` — git deletes the folder. Run from the REPOSITORY the
/// worktree belongs to (`repo_dir`), never from inside the worktree itself.
/// Without `force`, a worktree with uncommitted work is refused with
/// [`dirty_refusal`]'s list, so the user sees the files, not git's one-line
/// refusal. rexenv's own regenerable files ([`is_rexenv_owned`]) are not the
/// user's work: they never make it refuse, and when they are the ONLY changes,
/// git is told `--force` so its own check does not refuse on their account.
/// `force` is the user's "remove anyway" — and still only ever removes the
/// worktree, never the branch.
#[allow(clippy::too_many_arguments)] // flat mirror of the step's inputs (clone_repo precedent)
pub fn remove(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    repo_dir: &Path,
    worktree: &Path,
    force: bool,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let mut force_git = force;
    if !force {
        let dirty = uncommitted(supervisor, git, env, worktree)?;
        let theirs: Vec<String> = dirty.iter().filter(|l| !is_rexenv_owned(l)).cloned().collect();
        if !theirs.is_empty() {
            return Err(Error::Other(dirty_refusal(worktree, &theirs)));
        }
        force_git = !dirty.is_empty();
    }
    let path = worktree.to_string_lossy().into_owned();
    let mut args: Vec<String> = vec!["worktree".into(), "remove".into()];
    if force_git {
        args.push("--force".into());
    }
    args.push(path);
    repo::run_git_op(supervisor, git, env, repo_dir, "worktree remove", &args, cancel, on_line)
}

/// Why rexenv will not serve the existing worktree at `path` as a site, in words
/// that fit a WORKTREE — or `None` when it can (ledger #848). The rule is the
/// linked-folder rule (`sites::validate_linked_docroot`); only its advice changes:
/// "create a site normally" means nothing for a folder git made, so a worktree
/// inside the sites folder is told how to move out instead. Asked by the "made
/// elsewhere" list BEFORE it offers Serve — the macOS VM run (10 Oct 2026) offered
/// Serve on a worktree that Serve then refused.
pub fn serve_refusal(conn: &Connection, platform: &dyn crate::platform::traits::Platform, path: &Path) -> Option<String> {
    let err = crate::core::sites::validate_linked_docroot(conn, platform, &path.to_string_lossy()).err()?.to_string();
    if err.contains("inside your rexenv sites folder") {
        return Some(format!(
            "{} is inside your rexenv sites folder, where a site cannot be linked — move it out first \
             (`git worktree move {} <a folder outside it>`), or make the worktree with New worktree…, \
             which puts it in ~/rexenv/Worktrees.",
            path.display(),
            path.display()
        ));
    }
    Some(err)
}

#[cfg(test)]
mod tests {
    /// **A worktree inside the sites folder is refused with a way OUT, and one
    /// outside it is servable** (ledger #848). Plant: drop the rewrite and the
    /// sentence says "create a site normally".
    #[test]
    fn a_worktree_in_the_sites_folder_is_told_how_to_move_out() {
        let conn = crate::state::db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let root = std::env::temp_dir().join(format!("rexenv-serve-refusal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (sites, outside) = (root.join("Sites"), root.join("elsewhere-try"));
        std::fs::create_dir_all(sites.join("elsewhere")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        crate::state::store::set_setting(&conn, crate::core::sites::SITES_DIR_KEY, &sites.to_string_lossy()).unwrap();
        let inside = serve_refusal(&conn, &*platform, &sites.join("elsewhere")).expect("refused");
        assert!(inside.contains("git worktree move") && inside.contains("New worktree") && !inside.contains("create a site normally"), "{inside}");
        assert_eq!(serve_refusal(&conn, &*platform, &outside), None);
        let _ = std::fs::remove_dir_all(&root);
    }

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
        // An ABSOLUTE path in this OS's own shape — `/repo/…` is not absolute on
        // Windows (no drive), which is how the first Windows run of this test
        // failed (10 Oct 2026). Written with `/`, the way git writes it there too
        // (`gitdir: C:/Users/…`); `Path` equality compares components, so the
        // separator spelling does not matter.
        let abs = d.join("repo").join(".git").join("worktrees").join("feature-x");
        std::fs::write(&f, format!("gitdir: {}\n", abs.display().to_string().replace('\\', "/"))).unwrap();
        assert_eq!(gitdir_of(&f).unwrap(), abs);
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

    #[test]
    fn asset_paths_take_one_plain_folder_name() {
        assert_eq!(
            asset_rel("wp-content", AssetKind::Plugin, "my-plugin").unwrap(),
            Path::new("wp-content").join("plugins").join("my-plugin")
        );
        assert_eq!(
            asset_rel("wp-content", AssetKind::Theme, "twentyx").unwrap(),
            Path::new("wp-content").join("themes").join("twentyx")
        );
        for bad in ["", ".", "..", "../x", "a/b", "a\\b", ".hidden", "c:x"] {
            assert!(asset_rel("wp-content", AssetKind::Plugin, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_site_copy_skips_what_it_is_told_and_never_follows_a_link() {
        let src = tmp("copysrc");
        let dst_parent = tmp("copydst");
        let dst = dst_parent.join("child");
        let plugin = src.join("wp-content/plugins/mine");
        let other = src.join("wp-content/plugins/other");
        std::fs::create_dir_all(plugin.join("src")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir_all(src.join("wp-content/uploads/2026")).unwrap();
        std::fs::write(src.join("wp-config.php"), "<?php").unwrap();
        std::fs::write(plugin.join("src/a.php"), "x").unwrap();
        std::fs::write(other.join("o.php"), "12345").unwrap();
        std::fs::write(src.join("wp-content/uploads/2026/p.jpg"), "jpg").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&other, src.join("wp-content/plugins/linked")).unwrap();
        // A plugin that is itself a worktree of an OUTSIDE repository: its `.git`
        // file must not be copied (the copy would share the original's worktree).
        let wt = src.join("wp-content/plugins/wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: /far/away/.git/worktrees/wt\n").unwrap();
        std::fs::write(wt.join("wt.php"), "1").unwrap();

        let skip = [PathBuf::from("wp-content/plugins/mine"), PathBuf::from("wp-content/uploads")];
        let stats = copy_site_tree(&src, &dst, &skip, &|| false).unwrap();
        assert!(dst.join("wp-config.php").exists());
        assert!(dst.join("wp-content/plugins/other/o.php").exists());
        assert!(!dst.join("wp-content/plugins/mine").exists(), "the asset dir is the worktree's");
        assert!(!dst.join("wp-content/uploads").exists(), "uploads skipped when asked");
        assert_eq!(stats.files, 3);
        assert_eq!(stats.bytes, 5 + 5 + 1);
        assert!(dst.join("wp-content/plugins/wt/wt.php").exists() && !dst.join("wp-content/plugins/wt/.git").exists(), "an outside worktree's pointer is not copied");
        assert!(stats.skipped_links.contains(&PathBuf::from("wp-content/plugins/wt/.git")));
        #[cfg(unix)]
        {
            assert!(!dst.join("wp-content/plugins/linked").exists(), "a link is not followed");
            assert!(stats.skipped_links.contains(&PathBuf::from("wp-content/plugins/linked")));
        }
        // Never over an existing destination; a cancel stops it.
        assert!(copy_site_tree(&src, &dst, &[], &|| false).is_err());
        let dst2 = dst_parent.join("child2");
        assert!(copy_site_tree(&src, &dst2, &[], &|| true).is_err());
        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&dst_parent).unwrap();
    }

    #[test]
    fn wp_config_moves_only_what_the_file_defines() {
        let cfg = "<?php\ndefine( 'DB_NAME', 'wp_shop' );\ndefine(\"WP_HOME\", 'https://shop.rex');\n\
                   // define('WP_SITEURL', 'x');\n";
        let m = wp_config_moves(cfg, "wp_fix_shop", "fix.shop.rex");
        assert_eq!(
            m,
            vec![
                ("DB_NAME", "wp_fix_shop".to_string()),
                ("WP_HOME", "https://fix.shop.rex".to_string()),
            ],
            "a commented define is not a define"
        );
        assert!(defines_constant("define ( 'X' , 1 );", "X"));
        assert!(!defines_constant("# define('X', 1);", "X"));
        assert!(!defines_constant("define('XY', 1);", "X"), "a longer name is a different constant");
    }

    #[test]
    fn a_dirty_worktree_is_refused_with_its_files_named() {
        let files: Vec<String> = (1..=10).map(|i| format!("?? f{i}.php")).collect();
        let m = dirty_refusal(Path::new("/s/x/wp-content/plugins/p"), &files);
        assert!(m.contains("/s/x/wp-content/plugins/p") && m.contains("10 uncommitted changes"), "{m}");
        assert!(m.contains("?? f8.php") && !m.contains("?? f9.php") && m.contains("and 2 more"), "{m}");
        assert!(m.contains("branch itself is kept"), "{m}");
        let one = dirty_refusal(Path::new("/w"), &[" M a.php".to_string()]);
        assert!(one.contains("1 uncommitted change ") && one.contains("lose it"), "{one}");
    }

    #[test]
    fn same_file_is_byte_equality_of_two_existing_files() {
        let d = tmp("same");
        std::fs::write(d.join("a"), "lock-1").unwrap();
        std::fs::write(d.join("b"), "lock-1").unwrap();
        std::fs::write(d.join("c"), "lock-2").unwrap();
        assert!(same_file(&d.join("a"), &d.join("b")));
        assert!(!same_file(&d.join("a"), &d.join("c")), "same length, other bytes");
        assert!(!same_file(&d.join("a"), &d.join("missing")));
        assert!(!same_file(&d.join("missing"), &d.join("missing2")), "two absent files are not 'the same lock'");
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// **rexenv's own regenerable mu-plugins are never the user's uncommitted
    /// work** (ledger #820's refinement, found on Windows). Plant: make
    /// `is_rexenv_owned` return false and the Shape B Delete in
    /// `worktree_site_check` refuses a clean site again.
    #[test]
    fn rexenv_owned_files_are_told_apart_from_the_users_work() {
        for owned in [
            "?? wp-content/mu-plugins/rexenv-dns.php",
            " M wp-content/mu-plugins/rexenv-mail.php",
            "?? web/app/mu-plugins/rexenv-login.php",
            "?? \"site dir/wp-content/mu-plugins/rexenv-tunnel.php\"",
            "?? wp-content/mu-plugins/rexenv-scratch-mail.php",
        ] {
            assert!(is_rexenv_owned(owned), "{owned}");
        }
        for theirs in [
            "?? wip.txt",
            " M wp-content/mu-plugins/my-loader.php",
            "?? wp-content/plugins/rexenv-dns.php",
            " M wp-content/mu-plugins/rexenv-notes.txt",
            "?? wp-content/mu-plugins/rexenv-my-loader.php",
            "R  wp-content/mu-plugins/rexenv-dns.php -> wp-content/mu-plugins/mine.php",
        ] {
            assert!(!is_rexenv_owned(theirs), "{theirs}");
        }
    }
}
