//! Git-repo sources for "add plugin/theme from Git": parse every common way a
//! developer pastes a repository — https, ssh://, scp-like `git@host:path`,
//! `owner/repo` shorthand, and forge web URLs with a `/tree/<branch>` suffix —
//! into one normalized clone URL + derived target folder name; then the
//! network side: the `ls-remote` probe (branch/tag picker + early auth check)
//! and the streamed, cancellable clone.
//!
//! Execution model: every child runs in its OWN process group via
//! `ProcessSupervisor::spawn_streamed` with the user's login-shell env
//! (`core::devtools`), stdout+stderr pumped line-wise to the caller (UI log
//! pane + flat log file) — never a frozen spinner. Cancel = group signal
//! (git/npm spawn worker trees; a positive-pid kill would orphan them).
//! `GIT_TERMINAL_PROMPT=0` on every git call: a hidden credential prompt
//! must FAIL FAST with a mapped, honest error — never hang. Anything that
//! reaches `git` argv is validated at parse time (M7 class).

use crate::error::{Error, Result};
use crate::platform::traits::ProcessSupervisor;
use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A parsed, normalized repository source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSource {
    /// Normalized URL handed to `git ls-remote`/`git clone` verbatim.
    pub url: String,
    /// Host, lowercased (display + the shorthand disclosure line).
    pub host: String,
    /// Default target folder name: last path segment minus `.git`.
    pub dir_name: String,
    /// Branch/tag CANDIDATE extracted from a pasted forge web URL
    /// (`github.com/o/r/tree/<ref>`, GitLab `/-/tree/<ref>`). Branch names may
    /// contain slashes, so a `/tree/feat/x/docs` tail is ambiguous — the UI
    /// matches this against the probe's real refs and silently drops a miss.
    pub ref_candidate: Option<String>,
}

fn other(msg: impl Into<String>) -> Error {
    Error::Other(msg.into())
}

/// Parse user input into a [`RepoSource`]. Errors are user-facing.
pub fn parse_source(raw: &str) -> Result<RepoSource> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(other(
            "Paste a git repository URL — https://host/owner/repo, \
             git@host:owner/repo.git, or owner/repo (GitHub).",
        ));
    }
    if s.chars().any(char::is_whitespace) {
        return Err(other("That doesn't look like a git URL (it contains spaces)."));
    }
    // npm-style `git+https://…` prefix — devs paste these from package.json.
    let s = s.strip_prefix("git+").unwrap_or(s);
    if s.starts_with("git://") {
        return Err(other(
            "The git:// protocol is unencrypted and no longer served by major \
             forges — paste the https:// or git@ form of this URL.",
        ));
    }
    if is_archive(s) {
        return Err(other(
            "That's an archive, not a repository. Paste the repository URL — \
             wp.org plugins install from the search tab.",
        ));
    }
    if let Some((scheme, rest)) = s.split_once("://") {
        return match scheme {
            "https" | "http" => parse_http_url(scheme, rest),
            "ssh" => parse_ssh_url(rest),
            _ => Err(other(format!(
                "Unsupported scheme \"{scheme}://\" — use https://, ssh://, or \
                 the git@host:path form."
            ))),
        };
    }
    if let Some(src) = parse_scp_like(s)? {
        return Ok(src);
    }
    if let Some(src) = parse_shorthand(s)? {
        return Ok(src);
    }
    Err(other(
        "Unrecognized repository reference. Accepted forms: \
         https://host/owner/repo, git@host:owner/repo.git, \
         ssh://git@host/owner/repo, or owner/repo (GitHub).",
    ))
}

fn is_archive(s: &str) -> bool {
    let path = s.split(['#', '?']).next().unwrap_or(s);
    [".zip", ".tar.gz", ".tgz", ".tar.bz2", ".tar.xz"]
        .iter()
        .any(|ext| path.to_ascii_lowercase().ends_with(ext))
}

/// `https://host/…` (any forge, self-hosted included). GitHub/GitLab web-UI
/// suffixes are understood: `/tree/<ref>` becomes the ref candidate; other
/// routes (`/blob/…`, `/pull/…`, GitLab `/-/…`) truncate to the repository.
fn parse_http_url(scheme: &str, rest: &str) -> Result<RepoSource> {
    // A pasted web URL often carries `#readme` or GitLab's `?ref_type=heads`.
    let rest = rest.split(['#', '?']).next().unwrap_or(rest);
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| other("That URL has no repository path (expected host/owner/repo)."))?;
    let host = host.to_ascii_lowercase();
    if host.is_empty() {
        return Err(other("That URL has no host."));
    }
    let mut segs: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut candidate = None;
    if host == "github.com" {
        if segs.len() < 2 {
            return Err(other("A GitHub URL needs owner/repo."));
        }
        if segs.len() > 2 {
            if segs[2] == "tree" && segs.len() > 3 {
                candidate = Some(segs[3..].join("/"));
            }
            segs.truncate(2); // /blob/…, /pull/…, /releases/… → the repo
        }
    } else if let Some(pos) = segs.iter().position(|p| *p == "-") {
        // GitLab route marker: /group[/subgroup]/repo/-/tree/<ref>[/…]
        if segs.get(pos + 1) == Some(&"tree") && segs.len() > pos + 2 {
            candidate = Some(segs[pos + 2..].join("/"));
        }
        segs.truncate(pos);
    }
    let last = segs
        .last()
        .ok_or_else(|| other("That URL has no repository path (expected host/owner/repo)."))?;
    let dir_name = derive_dir_name(last)?;
    Ok(RepoSource {
        url: format!("{scheme}://{host}/{}", segs.join("/")),
        host,
        dir_name,
        ref_candidate: candidate,
    })
}

/// A leading `-` in an ssh/scp URL's user or host would be handed to `ssh` as an
/// option flag (`-oProxyCommand=…` → arbitrary command execution BEFORE any
/// clone — the CVE-2017-1000117 class). The `--` before the git URL protects
/// git's own parser, not the downstream ssh, so we refuse it at parse time.
fn reject_dash_authority() -> Error {
    other(
        "that SSH URL's user or host starts with '-', which git would pass to ssh as an option. \
         Use a normal git@host:owner/repo or ssh://git@host/owner/repo.",
    )
}

/// `ssh://git@host[:port]/path/repo.git`.
fn parse_ssh_url(rest: &str) -> Result<RepoSource> {
    let rest = rest.split(['#', '?']).next().unwrap_or(rest).trim_end_matches('/');
    let (userhost, path) = rest
        .split_once('/')
        .ok_or_else(|| other("An ssh:// URL needs a path (ssh://git@host/owner/repo)."))?;
    let host = userhost
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(userhost)
        .split(':') // strip an explicit port for display
        .next()
        .unwrap_or(userhost)
        .to_ascii_lowercase();
    // Reject a leading-`-` user/host before it can reach ssh as a flag, and pin
    // the host charset (parse_scp_like's rule — ssh:// had none). See
    // [`reject_dash_authority`].
    let user = userhost.rsplit_once('@').map(|(u, _)| u).unwrap_or("");
    if user.starts_with('-')
        || host.is_empty()
        || host.starts_with('-')
        || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(reject_dash_authority());
    }
    let last = path
        .rsplit('/')
        .next()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| other("An ssh:// URL needs a repository name."))?;
    Ok(RepoSource {
        url: format!("ssh://{userhost}/{path}"),
        host,
        dir_name: derive_dir_name(last)?,
        ref_candidate: None,
    })
}

/// scp-like `user@host:path` (the form forges label "SSH"). Returns Ok(None)
/// when the shape doesn't apply so parsing can fall through.
fn parse_scp_like(s: &str) -> Result<Option<RepoSource>> {
    let Some((userhost, path)) = s.split_once(':') else {
        return Ok(None);
    };
    let Some((user, host)) = userhost.split_once('@') else {
        return Ok(None);
    };
    if user.is_empty()
        || host.is_empty()
        || path.is_empty()
        || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Ok(None);
    }
    // Same ssh-option-injection guard as parse_ssh_url: a leading `-` in the user
    // or host would reach ssh as a flag (CVE-2017-1000117 class). See
    // [`reject_dash_authority`]. This is a scp-shaped input, so refuse loudly
    // rather than falling through to the shorthand parser.
    if user.starts_with('-') || host.starts_with('-') {
        return Err(reject_dash_authority());
    }
    let path = path.trim_end_matches('/');
    let last = path
        .rsplit('/')
        .next()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| other("That git@ URL needs a repository name after the colon."))?;
    Ok(Some(RepoSource {
        url: format!("{user}@{}:{path}", host.to_ascii_lowercase()),
        host: host.to_ascii_lowercase(),
        dir_name: derive_dir_name(last)?,
        ref_candidate: None,
    }))
}

/// `owner/repo` shorthand → GitHub over https. The owner side must be
/// dot-free so `example.com/foo` never false-positives as shorthand.
fn parse_shorthand(s: &str) -> Result<Option<RepoSource>> {
    let Some((owner, repo)) = s.split_once('/') else {
        return Ok(None);
    };
    let owner_ok = !owner.is_empty()
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let repo_ok = !repo.is_empty()
        && !repo.contains('/')
        && repo.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !owner_ok || !repo_ok {
        return Ok(None);
    }
    Ok(Some(RepoSource {
        url: format!("https://github.com/{owner}/{repo}.git"),
        host: "github.com".into(),
        dir_name: derive_dir_name(repo)?,
        ref_candidate: None,
    }))
}

/// Where an added asset lands: `<docroot>/wp-content/{plugins|themes}/<dir>`.
/// `kind` and `dir_name` are validated HERE (M7 class — both cross IPC).
pub fn asset_dest(docroot: &Path, kind: &str, dir_name: &str) -> Result<std::path::PathBuf> {
    let sub = match kind {
        "plugin" => "plugins",
        "theme" => "themes",
        other => return Err(other_kind(other)),
    };
    let name = derive_dir_name(dir_name)?;
    Ok(docroot.join("wp-content").join(sub).join(name))
}

fn other_kind(kind: &str) -> Error {
    Error::Other(format!("unknown asset kind \"{kind}\" (expected plugin or theme)"))
}

/// Public validation for a user-edited target folder name (same rule as the
/// URL-derived default).
pub fn validate_dir_name(name: &str) -> Result<String> {
    derive_dir_name(name)
}

/// Target folder name from the repo's last path segment, `.git` stripped.
/// Becomes a directory under wp-content — validated here, once, before it can
/// touch a path (M7 class). Leading dots are refused outright: WordPress
/// ignores hidden plugin/theme folders, and the vhost templates 404 them.
fn derive_dir_name(seg: &str) -> Result<String> {
    let name = seg.strip_suffix(".git").unwrap_or(seg);
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        return Err(other(format!(
            "Can't use \"{seg}\" as a folder name under wp-content — it must \
             start with a letter or digit and contain only letters, digits, \
             dots, dashes, or underscores."
        )));
    }
    Ok(name.to_string())
}

// ---------------------------------------------------------------------------
// Remote probe (git ls-remote)
// ---------------------------------------------------------------------------

/// What a remote offers — feeds the branch/tag picker. Probing also validates
/// URL + auth EARLY, before any clone starts.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRefs {
    pub default_branch: Option<String>,
    pub branches: Vec<String>,
    pub tags: Vec<String>,
}

/// Hard cap for the probe — it's one small round-trip; anything longer is a
/// stalled network or a hidden prompt (which `GIT_TERMINAL_PROMPT=0` turns
/// into a fast error instead).
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// `git ls-remote --symref <url>`: default branch (HEAD symref), branches,
/// tags. `env` is the login-shell snapshot (SSH_AUTH_SOCK rides along for
/// private repos).
pub fn probe_remote(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    url: &str,
) -> Result<RemoteRefs> {
    let env = with_git_env(env);
    let args = ["ls-remote", "--symref", "--", url];
    // Network probe: no checkout dir — spawn_streamed needs a valid cwd, and one
    // is irrelevant to `ls-remote`, so use the temp dir.
    let out = run_captured_with_cap(supervisor, git, &args, &std::env::temp_dir(), &env, PROBE_TIMEOUT)
        .map_err(|e| Error::Other(format!("probing {url} failed: {e}")))?;
    if !out.ok {
        return Err(map_git_error(&out.stderr_tail, url));
    }
    Ok(parse_ls_remote(&out.stdout))
}

fn parse_ls_remote(stdout: &str) -> RemoteRefs {
    let mut refs = RemoteRefs::default();
    for line in stdout.lines() {
        // `ref: refs/heads/main\tHEAD` — the default branch symref.
        if let Some(rest) = line.strip_prefix("ref: refs/heads/") {
            if let Some((name, target)) = rest.split_once('\t') {
                if target == "HEAD" {
                    refs.default_branch = Some(name.to_string());
                }
            }
            continue;
        }
        let Some((_oid, name)) = line.split_once('\t') else {
            continue;
        };
        if let Some(b) = name.strip_prefix("refs/heads/") {
            refs.branches.push(b.to_string());
        } else if let Some(t) = name.strip_prefix("refs/tags/") {
            if !t.ends_with("^{}") {
                // peeled duplicates
                refs.tags.push(t.to_string());
            }
        }
    }
    refs
}

// ---------------------------------------------------------------------------
// Pull-request refs (refs/pull/N/head — GitHub/Gitea; refs/merge-requests/N/
// head — GitLab). Refs-only, NO host API and NO tokens: the number + sha is
// all a ref carries (titles/authors would need the host API — deliberately
// out of scope). Bitbucket advertises neither pattern → empty list.
// ---------------------------------------------------------------------------

/// One host-advertised PR/MR head ref.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRef {
    pub number: u64,
    pub sha: String,
    /// Full ref name — the checkout target (`refs/pull/12/head`).
    #[serde(rename = "ref")]
    pub full_ref: String,
}

/// Both host patterns in ONE ls-remote call — they OR together and a
/// non-matching pattern is silently empty, so no host detection is needed.
const PULL_REF_PATTERNS: [&str; 2] = ["refs/pull/*/head", "refs/merge-requests/*/head"];

/// Does this checkout target name a PR/MR head ref (⇒ fetch-then-detach flow)?
pub fn is_pull_ref(r: &str) -> bool {
    r.starts_with("refs/pull/") || r.starts_with("refs/merge-requests/")
}

/// Parse `ls-remote` output for PR/MR head refs. PURE. Sorted highest number
/// first (newest, like tags) — ls-remote output is lexicographic (10, 100,
/// 1000…), never trust its order.
pub fn parse_pull_refs(stdout: &str) -> Vec<PullRef> {
    let mut out: Vec<PullRef> = stdout
        .lines()
        .filter_map(|line| {
            let (oid, name) = line.split_once('\t')?;
            if name.ends_with("^{}") {
                return None; // peeled duplicates
            }
            let number = name
                .strip_prefix("refs/pull/")
                .or_else(|| name.strip_prefix("refs/merge-requests/"))?
                .strip_suffix("/head")?
                .parse::<u64>()
                .ok()?;
            Some(PullRef { number, sha: oid.trim().to_string(), full_ref: name.to_string() })
        })
        .collect();
    out.sort_by(|a, b| b.number.cmp(&a.number));
    out
}

/// PR/MR head refs advertised by `origin` (network — one ls-remote round
/// trip, PROBE_TIMEOUT cap; NOT `run_git_lines`, whose 10s local cap is too
/// short for a network call).
pub fn list_pull_refs(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
) -> Result<Vec<PullRef>> {
    let env = with_git_env(env);
    let dir_s = dir.to_string_lossy().into_owned();
    let args = [
        "-C",
        dir_s.as_str(),
        "ls-remote",
        "origin",
        PULL_REF_PATTERNS[0],
        PULL_REF_PATTERNS[1],
    ];
    let out = run_captured_with_cap(supervisor, git, &args, dir, &env, PROBE_TIMEOUT)?;
    if !out.ok {
        return Err(map_git_error(&out.stderr_tail, "the remote"));
    }
    Ok(parse_pull_refs(&out.stdout))
}

/// The user's env + non-negotiable git overrides. `GIT_TERMINAL_PROMPT=0`:
/// a credential prompt from a background process is an invisible hang — fail
/// fast and map the error instead.
fn with_git_env(env: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        env.iter().filter(|(k, _)| k != "GIT_TERMINAL_PROMPT").cloned().collect();
    out.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
    out
}

/// Captured run with a wall-clock cap. Output is drained on reader THREADS —
/// `ls-remote` on a big repo overflows a pipe buffer (gutenberg has thousands
/// of refs), so a `wait`-then-read would deadlock into a fake timeout.
#[derive(Debug)]
struct CapturedRun {
    ok: bool,
    stdout: String,
    stderr_tail: Vec<String>,
}

/// Spawn through `spawn_streamed` (which sets `process_group(0)` — the child is
/// its own group leader, `pgid = child.id()`) so that on timeout `stop_group`
/// can kill the WHOLE group, not just the leader. `ls-remote` forks an ssh /
/// git-remote-https grandchild that inherits the stdout pipe; the old leader-only
/// `child.kill()` left it alive holding the pipe write-end, so the reader threads
/// could never join (they'd block on `read_to_end` until ssh's own network
/// timeout minutes later) — the exact orphan class `spawn_streamed` exists to
/// prevent (B7). `cwd` is the checkout dir for local ops (they also pass `-C`);
/// the network probe has no dir, so it passes a guaranteed-existing temp dir.
fn run_captured_with_cap(
    supervisor: &dyn ProcessSupervisor,
    program: &Path,
    args: &[&str],
    cwd: &Path,
    env: &[(String, String)],
    cap: Duration,
) -> Result<CapturedRun> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut child = supervisor.spawn_streamed(program, &owned, cwd, env)?;
    let pgid = child.id(); // == the group leader (process_group(0)) — positive ID
    let mut stdout = child.stdout.take().expect("spawn_streamed pipes stdout");
    let mut stderr = child.stderr.take().expect("spawn_streamed pipes stderr");
    // Readers drain CONCURRENTLY with the wait loop below — this is what keeps
    // the pipe from filling on the success path (the deadlock the doc warns of
    // is `wait`-before-drain; we never do that).
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stderr.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() >= cap {
            // Kill the whole group FIRST (leader + the ssh grandchild that holds
            // the pipe), THEN join: once every group member is dead the pipe
            // write-ends close, `read_to_end` hits EOF, and the joins return
            // immediately instead of hanging on the orphan.
            let _ = supervisor.stop_group(pgid);
            let _ = child.wait();
            let _ = out_t.join();
            let _ = err_t.join();
            return Err(Error::Other(format!(
                "timed out after {}s (stalled network?)",
                cap.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = String::from_utf8_lossy(&out_t.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_t.join().unwrap_or_default()).into_owned();
    let stderr_tail: Vec<String> =
        stderr.lines().rev().take(20).map(|l| l.to_string()).collect::<Vec<_>>();
    let stderr_tail = stderr_tail.into_iter().rev().collect();
    Ok(CapturedRun { ok: status.success(), stdout, stderr_tail })
}

// ---------------------------------------------------------------------------
// Streaming step runner + cancellation
// ---------------------------------------------------------------------------

/// Cooperative cancel shared between a running step and the UI. Killing goes
/// through the SUPERVISOR (whole process group) — never a bare pid.
#[derive(Clone, Default)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    pgid: Mutex<Option<u32>>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }
    /// The live child's process group, when a step is running (ps evidence,
    /// diagnostics).
    pub fn current_pgid(&self) -> Option<u32> {
        *self.inner.pgid.lock().expect("pgid lock")
    }
    /// Flag + signal the whole group of the current step (if any). A step
    /// that spawns AFTER this sees the flag and stops itself.
    pub fn cancel(&self, supervisor: &dyn ProcessSupervisor) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        if let Some(pgid) = self.current_pgid() {
            let _ = supervisor.stop_group(pgid);
        }
    }
    fn register(&self, pgid: u32) {
        *self.inner.pgid.lock().expect("pgid lock") = Some(pgid);
    }
    fn clear(&self) {
        *self.inner.pgid.lock().expect("pgid lock") = None;
    }
}

/// One streamed step's outcome. `Err` is reserved for plumbing failures
/// (spawn, IO); a non-zero exit or a cancel comes back `Ok` with the flags +
/// tail so the caller applies TOOL-SPECIFIC error mapping.
#[derive(Debug)]
pub struct StepResult {
    pub ok: bool,
    pub cancelled: bool,
    pub exit: Option<i32>,
    /// Last output lines (stdout+stderr interleaved) — error-mapping input.
    pub tail: Vec<String>,
}

const TAIL_LINES: usize = 40;

/// Run one child in its own process group, pumping stdout+stderr to `on_line`
/// as they arrive (log pane + log file). Blocking — callers use
/// `spawn_blocking` (the wp-cli convention).
/// Idle bound for steps whose long TOTAL silence means a dead pipe, not a slow
/// one: git ops stream `--progress` redraws continuously (every `\r` counts as
/// a line via `pump_lines`) and package installers print per package, so five
/// straight minutes of NOTHING is a black-holed connection (B25). User scripts
/// (`run`, `build`, watch) are exempt by POLICY, not by a bigger number — a
/// silent `tsc` or an idle watcher is legitimate there; pass `None`.
pub const STEP_IDLE_LIMIT: Duration = Duration::from_secs(300);

/// How often the receive loop wakes to check the idle clock.
const IDLE_TICK: Duration = Duration::from_secs(1);

pub fn run_step_streamed(
    supervisor: &dyn ProcessSupervisor,
    program: &Path,
    args: &[String],
    cwd: &Path,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
    idle_limit: Option<Duration>,
) -> Result<StepResult> {
    if cancel.is_cancelled() {
        return Ok(StepResult { ok: false, cancelled: true, exit: None, tail: Vec::new() });
    }
    let mut child = supervisor.spawn_streamed(program, args, cwd, env)?;
    let pgid = child.id();
    cancel.register(pgid);
    // Cancel raced the spawn: the flag was set between the check above and
    // register — kill what we just started.
    if cancel.is_cancelled() {
        let _ = supervisor.stop_group(pgid);
    }
    let stdout = child.stdout.take().expect("spawn_streamed pipes stdout");
    let stderr = child.stderr.take().expect("spawn_streamed pipes stderr");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let tx2 = tx.clone();
    let t1 = std::thread::spawn(move || pump_lines(stdout, tx));
    let t2 = std::thread::spawn(move || pump_lines(stderr, tx2));
    let mut tail: VecDeque<String> = VecDeque::with_capacity(TAIL_LINES);
    let mut deliver = |line: String, tail: &mut VecDeque<String>| {
        on_line(&line);
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    };
    // Any line (stdout or stderr — git progress redraws count) resets the idle
    // clock. On `idle_limit` of total silence: kill THIS step's process group
    // (`pgid` — the group spawn_streamed created, the same positive ID the
    // cancel path uses) so grandchildren die too and the pipes close, then fall
    // through to the shared drain/join/wait path below.
    let mut last_output = Instant::now();
    let mut stalled = false;
    loop {
        match rx.recv_timeout(IDLE_TICK) {
            Ok(line) => {
                last_output = Instant::now();
                deliver(line, &mut tail);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(limit) = idle_limit {
                    if last_output.elapsed() >= limit {
                        deliver(
                            format!(
                                "no output for {}s — killed as stalled (network black hole?)",
                                limit.as_secs()
                            ),
                            &mut tail,
                        );
                        let _ = supervisor.stop_group(pgid);
                        stalled = true;
                        break;
                    }
                }
            }
            // Both pumps dropped their senders (child closed its pipes).
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // Post-kill remnants (bounded: the group kill closes every pipe end,
    // grandchildren included, so the pumps exit and this drains fast).
    for line in rx {
        deliver(line, &mut tail);
    }
    let _ = t1.join();
    let _ = t2.join();
    let status = child.wait()?;
    cancel.clear();
    Ok(StepResult {
        ok: status.success() && !cancel.is_cancelled() && !stalled,
        cancelled: cancel.is_cancelled(),
        exit: status.code(),
        tail: tail.into(),
    })
}

/// Byte pump: split on `\n` AND `\r` (git's `--progress` redraws lines with
/// bare carriage returns), strip ANSI color, drop empties, forward.
fn pump_lines(mut reader: impl Read, tx: std::sync::mpsc::Sender<String>) {
    let mut buf = [0u8; 8192];
    let mut line: Vec<u8> = Vec::new();
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &b in &buf[..n] {
            if b == b'\n' || b == b'\r' {
                flush_line(&mut line, &tx);
            } else {
                line.push(b);
            }
        }
    }
    flush_line(&mut line, &tx);
}

fn flush_line(line: &mut Vec<u8>, tx: &std::sync::mpsc::Sender<String>) {
    if line.is_empty() {
        return;
    }
    let s = strip_ansi(&String::from_utf8_lossy(line));
    line.clear();
    let s = s.trim_end();
    if !s.is_empty() {
        let _ = tx.send(s.to_string());
    }
}

/// Remove ANSI escape sequences (CSI `ESC[…<final>` and the stray lone ESC) —
/// the log pane is a plain styled `<pre>`, not a terminal.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // consume parameter/intermediate bytes until a final byte @–~
            for f in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&f) {
                    break;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Clone
// ---------------------------------------------------------------------------

/// Streamed, cancellable `git clone` into `dest` (which must NOT exist — the
/// collision is checked here, before any network traffic). On failure or
/// cancel the partially-written `dest` is removed — but only because this fn
/// created it; an existing dir is refused, never deleted. Full history, but
/// `--no-recurse-submodules`: cloning an UNTRUSTED repo must not process its
/// attacker-controlled `.gitmodules`, which can execute code at clone/checkout
/// time via the `ext::` transport, a dash-leading submodule URL (option
/// injection), OR the path/hook traversal class (incl. CVE-2024-32002, a 2024
/// macOS clone-time RCE) — git blocks these only version-dependently, and we
/// clone on arbitrary machines. The `.gitmodules` FILE is still written (inert
/// text); submodule init is a deferred explicit opt-in step (B4-A-full), never
/// implicit on clone. The explicit flag also overrides a user's
/// `clone.recurseSubmodules=true` gitconfig.
#[allow(clippy::too_many_arguments)] // flat mirror of the step's inputs (apache::generate_config precedent)
pub fn clone_repo(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    url: &str,
    git_ref: Option<&str>,
    dest: &Path,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    if dest.exists() {
        return Err(Error::Other(format!(
            "\"{}\" already exists — pick another folder name, or remove the \
             existing folder first.",
            dest.display()
        )));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| Error::Other(format!("invalid clone target {}", dest.display())))?;
    std::fs::create_dir_all(parent)?;
    let args = clone_args(url, git_ref, dest);
    let env = with_git_env(env);
    on_line(&format!("$ git {}", args.join(" ")));
    // Network op: total silence means a black hole, not a slow link (B25).
    let result =
        run_step_streamed(supervisor, git, &args, parent, &env, cancel, on_line, Some(STEP_IDLE_LIMIT))?;
    if result.ok {
        return Ok(());
    }
    // Failed or cancelled clone: remove the partial checkout so a retry
    // doesn't hit our own collision guard. Guarded by the exists-check above —
    // this fn is the dir's creator.
    if dest.exists() {
        let _ = std::fs::remove_dir_all(dest);
    }
    if result.cancelled {
        return Err(Error::Other("clone cancelled".into()));
    }
    Err(map_git_error(&result.tail, url))
}

/// The `git` argv for [`clone_repo`] — factored out so the security-critical
/// flags are unit-testable without spawning git. Global `-c` config (which MUST
/// precede the subcommand) disables the dangerous transports as defense-in-depth;
/// `--no-recurse-submodules` is the real guard (see [`clone_repo`]). The pinned
/// URL/dest stay positional after `--` (git's own option-parser guard).
fn clone_args(url: &str, git_ref: Option<&str>, dest: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-c".into(),
        "protocol.ext.allow=never".into(),
        "-c".into(),
        "protocol.file.allow=user".into(),
        "clone".into(),
        "--progress".into(),
        "--no-recurse-submodules".into(),
    ];
    if let Some(r) = git_ref {
        args.push("--branch".into());
        args.push(r.to_string());
    }
    args.push("--".into());
    args.push(url.to_string());
    args.push(dest.to_string_lossy().into_owned());
    args
}

/// Translate git's stderr tail into an honest, actionable error (ports.rs
/// house style: the copy-paste fix is the last line, `$ `-prefixed).
pub fn map_git_error(tail: &[String], url: &str) -> Error {
    let joined = tail.join("\n");
    let host = host_of(url);
    if joined.contains("Permission denied (publickey)")
        || joined.contains("Authentication failed")
    {
        return Error::Other(format!(
            "{host} refused authentication. For a private repo, make sure the \
             SSH key you use for it is loaded — test your access in a \
             terminal:\n$ ssh -T git@{host}"
        ));
    }
    if joined.contains("Host key verification failed") {
        return Error::Other(format!(
            "First SSH contact with {host}: its host key isn't in your \
             ~/.ssh/known_hosts yet. Accept it once in a terminal, then \
             retry:\n$ ssh -T git@{host}"
        ));
    }
    if joined.contains("could not read Username")
        || joined.contains("terminal prompts disabled")
    {
        // GitHub answers a PRIVATE repo and a WRONG URL identically over
        // https (anti-enumeration) — the message covers both.
        return Error::Other(format!(
            "{host} asked for credentials — this is a PRIVATE repo (or the \
             URL is wrong; {host} answers both the same way). rexenv never \
             prompts for credentials: check the URL, use the SSH form \
             (git@{host}:owner/repo.git), or log the git CLI in once:\n\
             $ gh auth login"
        ));
    }
    // Before the generic not-found: "Remote branch X not found" contains it.
    if joined.contains("Remote branch") && joined.contains("not found") {
        return Error::Other(
            "That branch/tag doesn't exist on the remote anymore — hit Fetch \
             to refresh the list."
                .into(),
        );
    }
    if joined.contains("Repository not found")
        || joined.contains("not found")
        || joined.contains("does not appear to be a git repository")
    {
        return Error::Other(format!(
            "Repository not found at {url}. Check the URL — or if it's \
             private, use its SSH form (git@{host}:owner/repo.git)."
        ));
    }
    if joined.contains("Could not resolve host") {
        return Error::Other(format!(
            "Can't reach {host} — you look offline. Check your connection and retry."
        ));
    }
    let detail: Vec<&str> = tail.iter().rev().take(3).map(|s| s.as_str()).collect();
    let detail: Vec<&str> = detail.into_iter().rev().collect();
    Error::Other(format!("git failed:\n{}", detail.join("\n")))
}

fn host_of(url: &str) -> String {
    parse_source(url).map(|s| s.host).unwrap_or_else(|_| "the remote".into())
}

// ---------------------------------------------------------------------------
// Asset status (git status --porcelain=v2 --branch) — pure parser + runners
// ---------------------------------------------------------------------------

/// One checkout's working-tree state. Feeds the RepoPanel header AND the
/// delete-safety warning ([`loss_warning`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    /// Current branch; None when detached.
    pub branch: Option<String>,
    pub detached: bool,
    /// No commits yet (fresh `git init` / empty clone) — porcelain `(initial)`.
    pub unborn: bool,
    pub upstream: Option<String>,
    /// Some only when an upstream is set and resolvable (`branch.ab` line).
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    /// Tracked entries with changes (staged or not, renames, unmerged).
    pub changed: u32,
    /// Untracked files — also lost on delete, so counted separately.
    pub untracked: u32,
}

/// Parse `git status --porcelain=v2 --branch` output. PURE — unit-tested
/// against the tricky shapes (detached HEAD, no upstream, dirty+ahead,
/// unborn branch).
pub fn parse_status_v2(out: &str) -> GitStatus {
    let mut st = GitStatus::default();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            st.unborn = rest.trim() == "(initial)";
        } else if let Some(rest) = line.strip_prefix("# branch.head ") {
            let head = rest.trim();
            if head == "(detached)" {
                st.detached = true;
            } else {
                st.branch = Some(head.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            st.upstream = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            for part in rest.split_whitespace() {
                if let Some(a) = part.strip_prefix('+') {
                    st.ahead = a.parse().ok();
                } else if let Some(b) = part.strip_prefix('-') {
                    st.behind = b.parse().ok();
                }
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            st.changed += 1;
        } else if line.starts_with("? ") {
            st.untracked += 1;
        }
    }
    st
}

/// What deleting this checkout would destroy, as one honest sentence — or
/// None when nothing is provably at risk (clean tree, everything pushed).
/// The UI prepends context ("This folder is a git checkout…").
pub fn loss_warning(st: &GitStatus) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if st.changed > 0 {
        parts.push(format!(
            "{} changed file{}",
            st.changed,
            if st.changed == 1 { "" } else { "s" }
        ));
    }
    if st.untracked > 0 {
        parts.push(format!(
            "{} untracked file{}",
            st.untracked,
            if st.untracked == 1 { "" } else { "s" }
        ));
    }
    if let Some(ahead) = st.ahead.filter(|a| *a > 0) {
        parts.push(format!(
            "{ahead} unpushed commit{}",
            if ahead == 1 { "" } else { "s" }
        ));
    }
    let mut caveat = String::new();
    if st.detached {
        caveat.push_str(" (detached HEAD — commits made here may not be on any branch)");
    } else if st.upstream.is_none() && !st.unborn {
        caveat.push_str(" (no upstream set — local-only commits can't be counted)");
    }
    if parts.is_empty() {
        // Nothing countable at risk — but a no-upstream/detached checkout
        // still can't be proven pushed, so keep that caveat as the warning.
        if caveat.is_empty() {
            return None;
        }
        return Some(format!("This checkout can't be verified as pushed{caveat}."));
    }
    let list = match parts.len() {
        1 => parts.remove(0),
        2 => format!("{} and {}", parts[0], parts[1]),
        _ => format!("{}, {}, and {}", parts[0], parts[1], parts[2]),
    };
    Some(format!("{list} will be lost{caveat}."))
}

/// Run + parse git status for a checkout (local, fast, no network; runs no
/// repo code). 10s cap is generous — gutenberg answers in ~100ms.
pub fn read_git_status(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
) -> Result<GitStatus> {
    let env = with_git_env(env);
    let out = run_captured_with_cap(
        supervisor,
        git,
        &["-C", &dir.to_string_lossy(), "status", "--porcelain=v2", "--branch"],
        dir,
        &env,
        Duration::from_secs(10),
    )?;
    if !out.ok {
        return Err(Error::Other(format!(
            "git status failed in {}:\n{}",
            dir.display(),
            out.stderr_tail.join("\n")
        )));
    }
    Ok(parse_status_v2(&out.stdout))
}

/// The checkout's `origin` remote URL, if any (local read, no network).
pub fn read_remote_url(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
) -> Option<String> {
    let env = with_git_env(env);
    let out = run_captured_with_cap(
        supervisor,
        git,
        &["-C", &dir.to_string_lossy(), "remote", "get-url", "origin"],
        dir,
        &env,
        Duration::from_secs(10),
    )
    .ok()?;
    if !out.ok {
        return None;
    }
    let url = out.stdout.trim();
    (!url.is_empty()).then(|| url.to_string())
}

/// Human label for a detached HEAD: the exact tag name when HEAD sits on one,
/// else the short commit id (local read, no network). NOT `describe --all` —
/// that would print `heads/main` when detached at a branch tip, which reads
/// as if the branch were checked out. Call only when status says detached.
pub fn read_detached_at(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
) -> Option<String> {
    let env = with_git_env(env);
    let dir_s = dir.to_string_lossy().into_owned();
    for args in [
        &["-C", dir_s.as_str(), "describe", "--tags", "--exact-match", "HEAD"][..],
        &["-C", dir_s.as_str(), "rev-parse", "--short", "HEAD"][..],
    ] {
        if let Ok(out) =
            run_captured_with_cap(supervisor, git, args, dir, &env, Duration::from_secs(10))
        {
            if out.ok {
                let v = out.stdout.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Git ops (fetch / pull --ff-only / checkout / push) — phase B
// ---------------------------------------------------------------------------

/// Validate a branch/tag name before it reaches git argv (M7 class — comes
/// from the UI's branch dropdown, but the IPC boundary re-validates).
pub fn validate_ref(r: &str) -> Result<String> {
    let ok = !r.is_empty()
        && !r.starts_with('-')
        && !r.starts_with('/')
        && !r.ends_with('/')
        && !r.contains("..")
        && r.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
    if !ok {
        return Err(Error::Other(format!("\"{r}\" is not a valid branch or tag name")));
    }
    Ok(r.to_string())
}

#[allow(clippy::too_many_arguments)] // flat mirror of the step's inputs (clone_repo precedent)
fn run_git_op(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
    op: &str,
    args: &[String],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let env = with_git_env(env);
    on_line(&format!("$ git {}", args.join(" ")));
    // Git ops stream progress continuously — 300s of silence is a wedge (B25).
    let result =
        run_step_streamed(supervisor, git, args, dir, &env, cancel, on_line, Some(STEP_IDLE_LIMIT))?;
    if result.ok {
        return Ok(());
    }
    if result.cancelled {
        return Err(Error::Other(format!("{op} cancelled")));
    }
    Err(map_git_op_error(op, &result.tail))
}

/// Argv for the panel's Fetch — pure, unit-tested (clone_args precedent).
///
/// `--tags` fetches the FULL tag set (auto-follow only brings tags on fetched
/// commits). `--force` is required with it: since git 2.20 a remote tag that
/// MOVED (rolling `v1`/`latest`) is otherwise rejected and the whole fetch
/// exits non-zero — every Fetch on that repo would fail forever. The branch
/// refspec already carries `+`, so `--force` changes nothing else. NOT
/// `--prune-tags` — that would delete tags the user created locally.
pub fn fetch_args() -> Vec<String> {
    ["fetch", "--prune", "--tags", "--force"].map(String::from).to_vec()
}

/// `git fetch --prune --tags --force` (default remote) — refreshes the
/// branch/tag dropdown + ahead/behind counts.
pub fn git_fetch(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    run_git_op(supervisor, git, env, dir, "fetch", &fetch_args(), cancel, on_line)
}

/// `git pull --ff-only`: rexenv NEVER merges or rebases for the user — a
/// diverged branch is an honest error pointing at their editor/terminal.
pub fn git_pull_ff(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let args = vec!["pull".to_string(), "--ff-only".to_string()];
    run_git_op(supervisor, git, env, dir, "pull", &args, cancel, on_line)
}

/// `git checkout <ref> --`. Plain checkout DWIMs a remote-tracking branch
/// into a local tracking branch; a tag lands detached (the panel shows it).
pub fn git_checkout(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
    target: &str,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let target = validate_ref(target)?;
    let args = vec!["checkout".to_string(), target, "--".to_string()];
    run_git_op(supervisor, git, env, dir, "checkout", &args, cancel, on_line)
}

/// Check out a host PR/MR ref — two commands in the checkout step (the
/// `git_push` status-then-push precedent):
///   1. `git fetch origin <ref>` — ONE-SHOT argv refspec, no config write.
///      A permanent `refs/pull/*` refspec would drag thousands of refs into
///      every fetch on big repos, so the ref is fetched only when needed.
///      A single src-only refspec ⇒ FETCH_HEAD is overwritten with exactly
///      one entry (no `+` needed — there is no destination ref to force).
///   2. `git checkout --detach FETCH_HEAD` — lands detached (the panel shows
///      it honestly). FETCH_HEAD is hardcoded HERE; it never crosses IPC as
///      a user-selectable target.
pub fn git_checkout_pull_ref(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
    target: &str,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let target = validate_ref(target)?;
    if !is_pull_ref(&target) {
        return Err(Error::Other(format!("\"{target}\" is not a PR/MR ref")));
    }
    let fetch = vec!["fetch".to_string(), "origin".to_string(), target];
    run_git_op(supervisor, git, env, dir, "checkout", &fetch, cancel, on_line)?;
    let co = ["checkout", "--detach", "FETCH_HEAD", "--"].map(String::from).to_vec();
    run_git_op(supervisor, git, env, dir, "checkout", &co, cancel, on_line)
}

/// `git push` — with `--set-upstream origin <branch>` added automatically
/// when the current branch has none (the approved auto-upstream default).
/// Never force.
pub fn git_push(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let status = read_git_status(supervisor, git, env, dir)?;
    let mut args = vec!["push".to_string()];
    if status.upstream.is_none() {
        let branch = status.branch.ok_or_else(|| {
            Error::Other(
                "can't push a detached HEAD — check out a branch first (or push \
                 from your terminal with an explicit refspec)."
                    .into(),
            )
        })?;
        args = vec!["push".into(), "--set-upstream".into(), "origin".into(), branch];
    }
    run_git_op(supervisor, git, env, dir, "push", &args, cancel, on_line)
}

/// Op failures → honest messages; anything unrecognized falls through to the
/// shared clone-era mapping (auth/host-key/offline) with the raw tail last.
pub fn map_git_op_error(op: &str, tail: &[String]) -> Error {
    let joined = tail.join("\n");
    if joined.contains("Not possible to fast-forward")
        || joined.contains("have diverged")
        || joined.contains("Need to specify how to reconcile")
    {
        return Error::Other(
            "Your branch and the remote have DIVERGED — rexenv never merges or \
             rebases for you. Resolve it in your editor/terminal, then come back."
                .into(),
        );
    }
    if joined.contains("would be overwritten") {
        return Error::Other(format!(
            "{op} refused: you have local changes to files this would touch. \
             Commit or stash them first, then retry."
        ));
    }
    if joined.contains("did not match any file") || joined.contains("pathspec") {
        return Error::Other(
            "That branch/tag isn't known locally — hit Fetch first, then retry."
                .into(),
        );
    }
    if joined.contains("not currently on a branch") {
        return Error::Other(
            "You're on a detached HEAD (a tag or PR checkout) — there is no \
             branch for pull to update. Check out a branch first, then retry."
                .into(),
        );
    }
    if joined.contains("non-fast-forward")
        || (joined.contains("rejected") && joined.contains("fetch first"))
        || joined.contains("Updates were rejected")
    {
        return Error::Other(
            "Push rejected: the remote has commits you don't have yet. Pull \
             first, then push."
                .into(),
        );
    }
    map_git_error(tail, "the remote")
}

/// Run a short read-only git command in `dir`, returning trimmed non-empty
/// stdout lines (branch listings for the checkout dropdown).
pub fn run_git_lines(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    dir: &Path,
    args: &[&str],
) -> Result<Vec<String>> {
    let env = with_git_env(env);
    let dir_s = dir.to_string_lossy().into_owned();
    let mut full: Vec<&str> = vec!["-C", &dir_s];
    full.extend_from_slice(args);
    let out = run_captured_with_cap(supervisor, git, &full, dir, &env, Duration::from_secs(10))?;
    if !out.ok {
        return Err(Error::Other(format!(
            "git {} failed:\n{}",
            args.join(" "),
            out.stderr_tail.join("\n")
        )));
    }
    Ok(out
        .stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Fingerprint of the dependency lockfiles — compared before/after a
/// pull/checkout so the panel can say "dependencies changed — run install".
/// Non-cryptographic (change detection, not integrity).
pub fn lockfile_fingerprint(dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for name in [
        "composer.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        // No lockfile committed? Manifest changes still mean "re-install".
        "composer.json",
        "package.json",
    ] {
        name.hash(&mut h);
        if let Ok(bytes) = std::fs::read(dir.join(name)) {
            bytes.hash(&mut h);
        }
    }
    h.finish()
}

// ---------------------------------------------------------------------------
// Dependency check ("Check deps") — ZERO-EXEC: pure fs reads + stored-
// fingerprint compares. Never runs composer/npm/repo code.
// ---------------------------------------------------------------------------

/// Version/algorithm prefix on STORED dependency fingerprints. If the
/// algorithm ever changes, bump this — a stored value with a foreign prefix
/// reads as "unverified" (marker absent), NEVER as a false "stale" mismatch.
/// (The DefaultHasher lesson, made structural.)
const FP_PREFIX: &str = "fnv1a:1:";

/// Pinned FNV-1a 64-bit. Stored fingerprints must be stable across app
/// builds — `DefaultHasher` is documented as unstable across Rust releases
/// (a toolchain bump would silently flip every asset to "stale"), so this is
/// hand-pinned. The in-process [`lockfile_fingerprint`] keeps DefaultHasher —
/// it never persists.
fn fnv1a(bytes: &[u8], mut h: u64) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn dep_fingerprint(dir: &Path, files: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for name in files {
        h = fnv1a(name.as_bytes(), h);
        if let Ok(bytes) = std::fs::read(dir.join(name)) {
            h = fnv1a(&bytes, h);
        }
    }
    format!("{FP_PREFIX}{h:016x}")
}

/// Stored-format fingerprint of the composer dependency inputs.
pub fn composer_fingerprint(dir: &Path) -> String {
    dep_fingerprint(dir, &["composer.lock", "composer.json"])
}

/// Stored-format fingerprint of the node dependency inputs.
pub fn node_fingerprint(dir: &Path) -> String {
    dep_fingerprint(
        dir,
        &["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb", "package.json"],
    )
}

/// One dependency family's verdict — only what the check can PROVE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DepVerdict {
    /// No manifest — nothing to install.
    NotApplicable,
    /// Manifest present, installed dir missing → install needed.
    Missing,
    /// Installed dir present + stored fingerprint matches current inputs.
    UpToDate,
    /// Installed dir present + stored fingerprint differs — the lockfile
    /// changed since the last install that ran through rexenv.
    Stale,
    /// Installed dir present but no usable stored fingerprint (installed
    /// outside rexenv, adopted checkout, or foreign fingerprint format).
    /// An honest unknown — never an alarm.
    Unverified,
}

/// Both families' verdicts (the "Check deps" report).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepReport {
    pub composer: DepVerdict,
    pub node: DepVerdict,
    /// Effective composer vendor dir (config.vendor-dir honored).
    pub vendor_dir: String,
}

/// Composer's effective vendor dir: `config.vendor-dir` when it's a sane
/// RELATIVE path (absolute / `..` / empty falls back — we only probe inside
/// the checkout).
fn composer_vendor_dir(dir: &Path) -> String {
    let read = || -> Option<String> {
        let raw = std::fs::read_to_string(dir.join("composer.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let vd = v.get("config")?.get("vendor-dir")?.as_str()?.trim().to_string();
        let ok = !vd.is_empty() && !vd.starts_with('/') && !vd.contains("..");
        ok.then_some(vd)
    };
    read().unwrap_or_else(|| "vendor".to_string())
}

/// A stored fingerprint is usable only in the CURRENT format — anything else
/// (old scheme, foreign writer) degrades to "unverified", never false-stale.
fn usable_fp(stored: Option<&str>) -> Option<&str> {
    stored.filter(|s| s.starts_with(FP_PREFIX))
}

fn family_verdict(installed: bool, stored: Option<&str>, current: &str) -> DepVerdict {
    if !installed {
        return DepVerdict::Missing;
    }
    match usable_fp(stored) {
        None => DepVerdict::Unverified,
        Some(s) if s == current => DepVerdict::UpToDate,
        Some(_) => DepVerdict::Stale,
    }
}

/// The zero-exec check. `stored_*_fp` come from the provenance row (NULL for
/// every pre-v15 asset and for anything never installed through rexenv —
/// those MUST read unverified, not stale: existing users upgrade to a calm
/// panel, not a wall of false alarms). Known limit: Yarn PnP repos have no
/// node_modules at all and read as "missing".
pub fn check_deps(
    dir: &Path,
    inspection: &RepoInspection,
    stored_composer_fp: Option<&str>,
    stored_node_fp: Option<&str>,
) -> DepReport {
    let vendor_dir = composer_vendor_dir(dir);
    let composer = if !inspection.composer {
        DepVerdict::NotApplicable
    } else {
        family_verdict(
            dir.join(&vendor_dir).is_dir(),
            stored_composer_fp,
            &composer_fingerprint(dir),
        )
    };
    let node = if inspection.node.is_none() {
        DepVerdict::NotApplicable
    } else {
        family_verdict(
            dir.join("node_modules").is_dir(),
            stored_node_fp,
            &node_fingerprint(dir),
        )
    };
    DepReport { composer, node, vendor_dir }
}

/// Does this verdict warrant OFFERING an install step? Only provable needs —
/// unverified/up-to-date get a report line, not a button.
pub fn needs_install(v: DepVerdict) -> bool {
    matches!(v, DepVerdict::Missing | DepVerdict::Stale)
}

// ---------------------------------------------------------------------------
// package.json scripts (phase C) — list + watch heuristic + runner
// ---------------------------------------------------------------------------

/// One offerable package.json script.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoScript {
    pub name: String,
    /// The script's command line — shown so the user sees WHAT runs.
    pub command: String,
    /// Long-running by name (dev/watch/start/serve/hot…) → offered as
    /// "Start watching" instead of a one-shot Run.
    pub watchy: bool,
}

/// package.json scripts, name-sorted. Names that could read as argv flags or
/// contain whitespace/control chars are dropped (pathological; they'd need
/// shell quoting we refuse to do).
pub fn list_scripts(dir: &Path) -> Vec<RepoScript> {
    let Ok(raw) = std::fs::read_to_string(dir.join("package.json")) else {
        return Vec::new();
    };
    let pkg: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    let Some(scripts) = pkg.get("scripts").and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    let mut out: Vec<RepoScript> = scripts
        .iter()
        .filter_map(|(name, cmd)| {
            let command = cmd.as_str()?.trim().to_string();
            let ok = !name.is_empty()
                && !name.starts_with('-')
                && !command.is_empty()
                && name.chars().all(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-')
                });
            ok.then(|| RepoScript { name: name.clone(), command, watchy: is_watchy(name) })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Name heuristic for "this script doesn't end on its own": exact dev/watch/
/// start/serve/hot, a `dev:`/`watch:`-style prefix, or "watch" anywhere
/// (build:watch). wp-scripts' `start` IS watch mode — the WP-dev norm.
pub fn is_watchy(name: &str) -> bool {
    const WATCHY: &[&str] = &["dev", "watch", "start", "serve", "hot"];
    let lower = name.to_ascii_lowercase();
    WATCHY.iter().any(|w| {
        lower == *w || lower.starts_with(&format!("{w}:")) || lower.ends_with(&format!(":{w}"))
    }) || lower.contains("watch")
}

/// `<manager> run <script>` — one-shot scripts and watchers share this; the
/// difference is only who waits (a job worker vs the watch registry thread).
pub fn node_run_script(
    supervisor: &dyn ProcessSupervisor,
    manager: &Path,
    dir: &Path,
    script: &str,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<StepResult> {
    let name = tool_name(manager);
    on_line(&format!("$ {name} run {script}"));
    // USER script (repo `run` + the watch runner both land here): long silence
    // is legitimate (a quiet tsc, a watcher idling by design, forever) — exempt
    // from the idle guard BY POLICY, not by a bigger number. Cancel is the
    // user's tool here (B25).
    run_step_streamed(
        supervisor,
        manager,
        &["run".into(), script.to_string()],
        dir,
        env,
        cancel,
        on_line,
        None,
    )
}

// ---------------------------------------------------------------------------
// Unmanaged-checkout scan (adopt flow)
// ---------------------------------------------------------------------------

/// A wp-content dir that looks like a git checkout but has no provenance row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmanagedRepo {
    pub dir_name: String,
    /// The dir itself is a symlink (manual link — adopts as source "linked").
    pub linked: bool,
}

/// Depth-1 scan of a wp-content/{plugins,themes} dir for git checkouts not in
/// `known`. `.git` may be a dir OR a file (worktrees, submodules) — existence
/// is the signal. Hidden dirs skipped (the vhost guard 404s them anyway).
pub fn scan_unmanaged(content_dir: &Path, known: &[String]) -> Vec<UnmanagedRepo> {
    let Ok(entries) = std::fs::read_dir(content_dir) else {
        return Vec::new();
    };
    let mut out: Vec<UnmanagedRepo> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.starts_with('.') || known.iter().any(|k| k == &name) {
                return None;
            }
            let path = e.path();
            // is_dir follows symlinks — a linked checkout is still a dir here.
            if !path.is_dir() || !path.join(".git").exists() {
                return None;
            }
            let linked = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            Some(UnmanagedRepo { dir_name: name, linked })
        })
        .collect();
    out.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    out
}

// ---------------------------------------------------------------------------
// Link-folder assets (phase D) — validation + the unlink-only delete guard
// ---------------------------------------------------------------------------

/// Validate a link-folder request BEFORE any symlink is created. Both
/// containment directions are refused: a target inside the docroot
/// (self-link), and a docroot inside the target (a cycle — linking a parent
/// of wp-content would make WP serve itself recursively).
pub fn validate_link_target(docroot: &Path, dest: &Path, target: &Path) -> Result<std::path::PathBuf> {
    let target = target
        .canonicalize()
        .map_err(|_| Error::Other(format!("{} doesn't exist (or isn't readable)", target.display())))?;
    if !target.is_dir() {
        return Err(Error::Other(format!("{} is not a folder", target.display())));
    }
    let docroot = docroot
        .canonicalize()
        .map_err(|e| Error::Other(format!("site folder unreadable: {e}")))?;
    if target.starts_with(&docroot) {
        return Err(Error::Other(
            "that folder is already inside this site — linking it to itself \
             would nest the site into the plugin. Pick a folder outside the site."
                .into(),
        ));
    }
    if docroot.starts_with(&target) {
        return Err(Error::Other(
            "that folder CONTAINS this site — linking it would create a cycle \
             (WordPress would serve itself recursively). Pick the plugin/theme \
             folder itself, not a parent."
                .into(),
        ));
    }
    if dest.exists() || std::fs::symlink_metadata(dest).is_ok() {
        return Err(Error::Other(format!(
            "\"{}\" already exists — pick another name or remove it first.",
            dest.display()
        )));
    }
    Ok(target)
}

/// Split delete candidates by FILESYSTEM truth: a dir that IS a symlink must
/// be unlinked, never handed to wp-cli — `wp plugin delete` walks INTO the
/// link and destroys the user's real checkout elsewhere on disk. Provenance
/// is metadata; the fs is the guard (covers manually-linked dirs that were
/// never adopted).
pub fn partition_symlink_deletes(
    content_dir: &Path,
    names: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut linked = Vec::new();
    let mut normal = Vec::new();
    for name in names {
        let is_link = derive_dir_name(name)
            .ok()
            .map(|n| content_dir.join(n))
            .and_then(|p| std::fs::symlink_metadata(p).ok())
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if is_link {
            linked.push(name.clone());
        } else {
            normal.push(name.clone());
        }
    }
    (linked, normal)
}

// ---------------------------------------------------------------------------
// Detection (pure fs — runs right after a clone, before any button shows)
// ---------------------------------------------------------------------------

/// What a cloned repo needs. Read-only inspection — detection itself never
/// executes repo code; only the explicit install/build steps do.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInspection {
    /// composer.json present → offer `composer install` (vendor/ is required
    /// for the plugin to run at all).
    pub composer: bool,
    /// package.json present → offer `<manager> install` (+ build).
    pub node: Option<NodePlan>,
    /// `Plugin Name:` / `Theme Name:` header found (a monorepo warning when
    /// neither matches what the user is adding).
    pub wp: WpHeader,
    /// Raw `.nvmrc` / `engines.node` content, for display + the version
    /// warning ([`node_version_warning`]).
    pub node_want: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePlan {
    /// npm | pnpm | yarn | bun.
    pub manager: String,
    /// Why: "packageManager" (authoritative field) | "lockfile" | "default".
    pub pinned_by: String,
    /// package.json has a "build" script.
    pub has_build: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpHeader {
    /// "plugin" | "theme" | "none".
    pub kind: String,
    pub name: Option<String>,
}

pub fn inspect_repo(dir: &Path) -> RepoInspection {
    let composer = dir.join("composer.json").is_file();
    let mut node = None;
    let mut node_want = None;
    if let Ok(raw) = std::fs::read_to_string(dir.join("package.json")) {
        let pkg: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
        // packageManager ("pnpm@9.1.0") is authoritative; lockfiles next.
        let (manager, pinned_by) = match pkg
            .get("packageManager")
            .and_then(|v| v.as_str())
            .and_then(|v| v.split('@').next())
            .filter(|m| ["npm", "pnpm", "yarn", "bun"].contains(m))
        {
            Some(m) => (m.to_string(), "packageManager".to_string()),
            None => {
                let by_lock = [
                    ("pnpm-lock.yaml", "pnpm"),
                    ("yarn.lock", "yarn"),
                    ("bun.lockb", "bun"),
                    ("bun.lock", "bun"),
                    ("package-lock.json", "npm"),
                ]
                .iter()
                .find(|(f, _)| dir.join(f).is_file());
                match by_lock {
                    Some((_, m)) => (m.to_string(), "lockfile".to_string()),
                    None => ("npm".to_string(), "default".to_string()),
                }
            }
        };
        let has_build = pkg
            .get("scripts")
            .and_then(|s| s.get("build"))
            .and_then(|b| b.as_str())
            .is_some_and(|b| !b.trim().is_empty());
        node = Some(NodePlan { manager, pinned_by, has_build });
        node_want = pkg
            .get("engines")
            .and_then(|e| e.get("node"))
            .and_then(|n| n.as_str())
            .map(|s| s.trim().to_string());
    }
    // .nvmrc beats engines for display — it's what `nvm use` would pick.
    if let Ok(nvmrc) = std::fs::read_to_string(dir.join(".nvmrc")) {
        let v = nvmrc.lines().next().unwrap_or("").trim().to_string();
        if !v.is_empty() {
            node_want = Some(v);
        }
    }
    RepoInspection { composer, node, wp: wp_header(dir), node_want }
}

/// Depth-1 scan for the WordPress header: any root `*.php` with
/// `Plugin Name:`, or `style.css` with `Theme Name:` (first 8KB — WP itself
/// reads the first 8KB of headers).
fn wp_header(dir: &Path) -> WpHeader {
    const HEADER_BYTES: usize = 8192;
    let read_head = |p: &Path| -> String {
        std::fs::read(p)
            .map(|b| String::from_utf8_lossy(&b[..b.len().min(HEADER_BYTES)]).into_owned())
            .unwrap_or_default()
    };
    let style = dir.join("style.css");
    if style.is_file() {
        if let Some(name) = header_value(&read_head(&style), "Theme Name:") {
            return WpHeader { kind: "theme".into(), name: Some(name) };
        }
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return WpHeader { kind: "none".into(), name: None };
    };
    for entry in entries.flatten().take(50) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("php") {
            if let Some(name) = header_value(&read_head(&p), "Plugin Name:") {
                return WpHeader { kind: "plugin".into(), name: Some(name) };
            }
        }
    }
    WpHeader { kind: "none".into(), name: None }
}

fn header_value(head: &str, key: &str) -> Option<String> {
    let pos = head.find(key)?;
    let rest = &head[pos + key.len()..];
    let val = rest.lines().next()?.trim().trim_end_matches("*/").trim();
    (!val.is_empty()).then(|| val.to_string())
}

/// Amber pre-install warning when the repo pins a Node major the resolved
/// node doesn't match. Display-only — never blocks. `want` is raw `.nvmrc` /
/// `engines.node` text; anything unparseable (e.g. `lts/iron`) warns nothing.
pub fn node_version_warning(want: &str, have_version: &str) -> Option<String> {
    let have_major = leading_int(have_version.trim_start_matches('v'))?;
    let cleaned = want.trim().trim_start_matches(['^', '~', 'v', '=']);
    let want_major = leading_int(cleaned.trim_start_matches(">=").trim_start_matches('>').trim())?;
    let range_min = want.contains(">=") || want.contains('>');
    let mismatch = if range_min { have_major < want_major } else { have_major != want_major };
    mismatch.then(|| {
        format!(
            "This repo wants Node {want} — you have {have_version}. Installs \
             and builds may fail; switch with your version manager first \
             (e.g. `nvm install {want_major}`), then hit Re-detect."
        )
    })
}

fn leading_int(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// Install / build steps
// ---------------------------------------------------------------------------

/// `composer install` — ALWAYS the pinned composer.phar executed by the
/// SITE's bundled PHP: platform checks (`php` version, `ext-*`) then match
/// the PHP the plugin actually runs on, and a system "composer" is never
/// executed (it can be a non-phar wrapper — Herd's is). The user's env rides
/// along (their COMPOSER_HOME/auth.json for private packages).
pub fn composer_install(
    supervisor: &dyn ProcessSupervisor,
    php: &Path,
    composer_phar: &Path,
    dir: &Path,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let args: Vec<String> = vec![
        composer_phar.to_string_lossy().into_owned(),
        "install".into(),
        "--no-interaction".into(),
    ];
    on_line("$ composer install --no-interaction");
    // Package install: streams per package — total silence is a wedge (B25).
    let result = run_step_streamed(
        supervisor,
        php,
        &args,
        dir,
        env,
        cancel,
        on_line,
        Some(STEP_IDLE_LIMIT),
    )?;
    step_verdict(result, "composer install", map_composer_error)
}

/// `<manager> install` (npm/pnpm/yarn/bun — the repo's own pick).
pub fn node_install(
    supervisor: &dyn ProcessSupervisor,
    manager: &Path,
    dir: &Path,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let name = tool_name(manager);
    on_line(&format!("$ {name} install"));
    // Package install: streams per package — total silence is a wedge (B25).
    let result = run_step_streamed(
        supervisor,
        manager,
        &["install".into()],
        dir,
        env,
        cancel,
        on_line,
        Some(STEP_IDLE_LIMIT),
    )?;
    step_verdict(result, &format!("{name} install"), map_node_error)
}

/// `<manager> run build`.
pub fn node_build(
    supervisor: &dyn ProcessSupervisor,
    manager: &Path,
    dir: &Path,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let name = tool_name(manager);
    on_line(&format!("$ {name} run build"));
    // Runs the repo's OWN "build" script — user code, where a silent tsc /
    // webpack for many minutes is legitimate: exempt from the idle guard by
    // policy, exactly like `run` (B25). Cancel is the user's tool.
    let result = run_step_streamed(
        supervisor,
        manager,
        &["run".into(), "build".into()],
        dir,
        env,
        cancel,
        on_line,
        None,
    )?;
    step_verdict(result, &format!("{name} run build"), map_node_error)
}

fn tool_name(p: &Path) -> String {
    p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "tool".into())
}

fn step_verdict(
    result: StepResult,
    what: &str,
    map: fn(&[String]) -> Error,
) -> Result<()> {
    if result.ok {
        return Ok(());
    }
    if result.cancelled {
        return Err(Error::Other(format!("{what} cancelled")));
    }
    Err(map(&result.tail))
}

/// Composer failures → honest messages. The two structural ones are PHP
/// version and missing extensions — both statements about OUR bundled PHP.
pub fn map_composer_error(tail: &[String]) -> Error {
    let joined = tail.join("\n");
    if joined.contains("your php version") || joined.to_lowercase().contains("requires php") {
        return Error::Other(format!(
            "Composer refused: the repo requires a PHP version this site \
             isn't running. Switch the site's PHP version (Site → Settings) \
             and retry.\n{}",
            last_lines(tail, 3)
        ));
    }
    if joined.contains("ext-") {
        return Error::Other(format!(
            "Composer refused: the repo needs a PHP extension rexenv's \
             bundled PHP doesn't ship. Details:\n{}",
            last_lines(tail, 3)
        ));
    }
    if joined.contains("Could not resolve host")
        || joined.contains("curl error")
        || joined.contains("getaddrinfo")
    {
        return Error::Other(
            "Composer can't reach the package registry — you look offline. \
             Check your connection and retry."
                .into(),
        );
    }
    Error::Other(format!("composer install failed:\n{}", last_lines(tail, 3)))
}

/// npm/pnpm/yarn failures → honest messages. node-gyp is the classic one.
pub fn map_node_error(tail: &[String]) -> Error {
    let joined = tail.join("\n");
    if joined.contains("node-gyp") || joined.contains("gyp ERR") {
        return Error::Other(
            "A native module failed to compile (node-gyp). That needs the \
             Xcode Command Line Tools — install them, then retry:\n\
             $ xcode-select --install"
                .into(),
        );
    }
    if joined.contains("EBADENGINE") || joined.contains("Unsupported engine") {
        return Error::Other(
            "Your Node version doesn't match what this repo requires (see \
             the warning above the install button). Switch Node with your \
             version manager, then hit Re-detect and retry."
                .into(),
        );
    }
    if joined.contains("ENOTFOUND")
        || joined.contains("ETIMEDOUT")
        || joined.contains("ECONNRESET")
        || joined.contains("network")
    {
        return Error::Other(
            "The package registry is unreachable — you look offline. Check \
             your connection and retry."
                .into(),
        );
    }
    if joined.contains("Missing script") {
        return Error::Other(
            "This repo has no \"build\" script (package.json → scripts). \
             Nothing to build — the plugin may ship ready-to-run."
                .into(),
        );
    }
    Error::Other(format!("install/build failed:\n{}", last_lines(tail, 3)))
}

fn last_lines(tail: &[String], n: usize) -> String {
    let lines: Vec<&str> = tail.iter().rev().take(n).map(|s| s.as_str()).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> RepoSource {
        parse_source(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn https_forms_normalize_and_derive_the_folder() {
        for input in [
            "https://github.com/acme/my-plugin",
            "https://github.com/acme/my-plugin.git",
            "https://github.com/acme/my-plugin/",
            "  https://github.com/acme/my-plugin  ",
            "git+https://github.com/acme/my-plugin.git",
        ] {
            let src = parse(input);
            assert_eq!(src.host, "github.com", "{input}");
            assert_eq!(src.dir_name, "my-plugin", "{input}");
            assert!(src.url.starts_with("https://github.com/acme/my-plugin"), "{input}");
            assert_eq!(src.ref_candidate, None, "{input}");
        }
        // Host case folds; self-hosted https passes through untouched.
        assert_eq!(parse("https://GitHub.com/a/b").host, "github.com");
        let corp = parse("http://git.corp.internal/team/widget.git");
        assert_eq!(corp.url, "http://git.corp.internal/team/widget.git");
        assert_eq!(corp.dir_name, "widget");
    }

    #[test]
    fn github_web_suffixes_truncate_and_tree_preselects_the_ref() {
        let tree = parse("https://github.com/acme/my-plugin/tree/develop");
        assert_eq!(tree.url, "https://github.com/acme/my-plugin");
        assert_eq!(tree.ref_candidate.as_deref(), Some("develop"));
        // Slashed branch names arrive joined — a candidate, not a promise.
        let feat = parse("https://github.com/acme/my-plugin/tree/feat/fast-build");
        assert_eq!(feat.ref_candidate.as_deref(), Some("feat/fast-build"));
        // Fragment from a copied web URL is noise.
        assert_eq!(parse("https://github.com/acme/p#readme").url, "https://github.com/acme/p");
        // Other routes: repo only, no candidate.
        for input in [
            "https://github.com/acme/my-plugin/pull/12",
            "https://github.com/acme/my-plugin/blob/main/src/index.js",
            "https://github.com/acme/my-plugin/releases/tag/v1.2.0",
        ] {
            let src = parse(input);
            assert_eq!(src.url, "https://github.com/acme/my-plugin", "{input}");
            assert_eq!(src.ref_candidate, None, "{input}");
        }
    }

    #[test]
    fn gitlab_route_marker_handles_subgroups_and_query_noise() {
        let src = parse("https://gitlab.com/group/sub/widget/-/tree/main?ref_type=heads");
        assert_eq!(src.url, "https://gitlab.com/group/sub/widget");
        assert_eq!(src.dir_name, "widget");
        assert_eq!(src.ref_candidate.as_deref(), Some("main"));
        // Non-tree GitLab routes truncate to the repo.
        let blob = parse("https://gitlab.com/g/widget/-/blob/main/readme.md");
        assert_eq!(blob.url, "https://gitlab.com/g/widget");
        assert_eq!(blob.ref_candidate, None);
    }

    #[test]
    fn ssh_and_scp_forms_pass_through_for_private_repos() {
        let scp = parse("git@github.com:acme/my-plugin.git");
        assert_eq!(scp.url, "git@github.com:acme/my-plugin.git");
        assert_eq!(scp.host, "github.com");
        assert_eq!(scp.dir_name, "my-plugin");
        // Absolute path on a bare server is valid scp syntax.
        assert_eq!(parse("git@build.corp:/srv/git/tool.git").dir_name, "tool");
        let ssh = parse("ssh://git@gitlab.com/group/widget.git");
        assert_eq!(ssh.url, "ssh://git@gitlab.com/group/widget.git");
        assert_eq!(ssh.host, "gitlab.com");
        assert_eq!(ssh.dir_name, "widget");
        // Explicit port stays in the clone URL, not the display host.
        let port = parse("ssh://git@git.corp:2222/team/widget.git");
        assert_eq!(port.url, "ssh://git@git.corp:2222/team/widget.git");
        assert_eq!(port.host, "git.corp");
    }

    #[test]
    fn ssh_scp_forms_reject_dash_authority_option_injection() {
        // CVE-2017-1000117 class: a leading-`-` user or host would be handed to
        // ssh as an option flag (e.g. -oProxyCommand=… → RCE) BEFORE any clone.
        // $IFS (no literal whitespace) survives the earlier whitespace reject.
        for bad in [
            "-oProxyCommand=touch$IFS/tmp/x@github.com:acme/repo.git", // scp, dash USER
            "git@-oProxyCommand:acme/repo.git",                        // scp, dash host
            "git@-github.com:acme/repo.git",                           // scp, dash host (charset-clean)
            "ssh://-oProxyCommand=x@github.com/acme/repo.git",         // ssh, dash user
            "ssh://git@-github.com/acme/repo.git",                     // ssh, dash host
        ] {
            assert!(parse_source(bad).is_err(), "must reject option injection: {bad}");
        }
        // Real private-repo forms still parse (the sibling test asserts the shapes).
        assert!(parse_source("git@github.com:acme/repo.git").is_ok());
        assert!(parse_source("ssh://git@git.corp:2222/team/widget.git").is_ok());
    }

    #[test]
    fn clone_args_disable_submodule_recursion_and_harden_transports() {
        let args = clone_args("https://github.com/acme/repo.git", None, Path::new("/tmp/repo"));
        // The real guard: no submodule recursion, and the old flag is gone — an
        // untrusted repo's .gitmodules is never processed at clone time (B4).
        assert!(args.iter().any(|a| a == "--no-recurse-submodules"), "{args:?}");
        assert!(!args.iter().any(|a| a == "--recurse-submodules"), "{args:?}");
        // Transport hardening via global -c, which MUST precede the subcommand.
        let joined = args.join(" ");
        assert!(joined.contains("-c protocol.ext.allow=never"), "{joined}");
        assert!(joined.contains("-c protocol.file.allow=user"), "{joined}");
        let clone_i = args.iter().position(|a| a == "clone").unwrap();
        let ext_i = args.iter().position(|a| a == "protocol.ext.allow=never").unwrap();
        assert!(ext_i < clone_i, "global -c must precede `clone`: {args:?}");
        // URL/dest stay positional after `--`.
        let dd = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[dd + 1], "https://github.com/acme/repo.git");
        assert_eq!(args[dd + 2], "/tmp/repo");
        // A branch lands as --branch's value, before `--`.
        let with_ref =
            clone_args("https://github.com/acme/repo.git", Some("develop"), Path::new("/tmp/repo"));
        let bi = with_ref.iter().position(|a| a == "--branch").unwrap();
        assert_eq!(with_ref[bi + 1], "develop");
        assert!(bi < with_ref.iter().position(|a| a == "--").unwrap(), "--branch before --");
    }

    #[test]
    fn shorthand_is_github_but_never_swallows_a_domain() {
        let src = parse("acme/my-plugin");
        assert_eq!(src.url, "https://github.com/acme/my-plugin.git");
        assert_eq!(src.host, "github.com");
        // A dotted left side is a host someone forgot the scheme for — refuse
        // rather than silently cloning from github.com/example.com/foo.
        assert!(parse_source("example.com/foo").is_err());
    }

    #[test]
    fn rejects_are_specific() {
        for (input, needle) in [
            ("", "Paste a git repository URL"),
            ("not a url at all", "contains spaces"),
            ("git://github.com/a/b.git", "git://"),
            ("https://github.com/a/b/archive/main.zip", "archive"),
            ("https://example.com/repo.tar.gz", "archive"),
            ("ftp://host/a/b", "Unsupported scheme"),
            ("just-a-word", "Unrecognized"),
            ("https://github.com/onlyowner", "owner/repo"),
        ] {
            let err = parse_source(input).unwrap_err().to_string();
            assert!(err.contains(needle), "{input}: {err}");
        }
        // Dot-leading repo name: hidden under wp-content AND 404'd by the
        // vhost dotfile guard — refused with the reason.
        assert!(parse_source("https://github.com/acme/.dotplugin")
            .unwrap_err()
            .to_string()
            .contains("folder name"));
    }

    #[test]
    fn ls_remote_parse_extracts_default_branch_tags_and_skips_peeled() {
        let out = "ref: refs/heads/trunk\tHEAD\n\
                   aaa\tHEAD\n\
                   aaa\trefs/heads/trunk\n\
                   bbb\trefs/heads/feat/fast-build\n\
                   ccc\trefs/tags/v1.0.0\n\
                   ddd\trefs/tags/v1.0.0^{}\n\
                   eee\trefs/pull/12/head\n";
        let refs = parse_ls_remote(out);
        assert_eq!(refs.default_branch.as_deref(), Some("trunk"));
        assert_eq!(refs.branches, vec!["trunk", "feat/fast-build"]);
        // Peeled ^{} duplicates and PR refs never reach the picker.
        assert_eq!(refs.tags, vec!["v1.0.0"]);
    }

    #[test]
    fn git_env_forces_no_terminal_prompt() {
        let user = vec![
            ("PATH".to_string(), "/x".to_string()),
            ("GIT_TERMINAL_PROMPT".to_string(), "1".to_string()),
        ];
        let env = with_git_env(&user);
        let vals: Vec<&str> = env
            .iter()
            .filter(|(k, _)| k == "GIT_TERMINAL_PROMPT")
            .map(|(_, v)| v.as_str())
            .collect();
        // The user's own =1 must never survive — a hidden prompt is a hang.
        assert_eq!(vals, vec!["0"]);
        assert!(env.iter().any(|(k, v)| k == "PATH" && v == "/x"));
    }

    #[test]
    fn strip_ansi_removes_csi_sequences() {
        assert_eq!(strip_ansi("\u{1b}[32mok\u{1b}[0m done"), "ok done");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("\u{1b}[1;31mred\u{1b}[m"), "red");
    }

    #[test]
    fn git_errors_map_to_actionable_messages() {
        let cases: &[(&str, &str)] = &[
            ("git@github.com: Permission denied (publickey).", "ssh -T git@github.com"),
            ("Host key verification failed.", "known_hosts"),
            (
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
                "PRIVATE",
            ),
            ("remote: Repository not found.", "Repository not found"),
            ("fatal: Remote branch gone-branch not found in upstream origin", "Fetch"),
            ("fatal: unable to access 'x': Could not resolve host: github.com", "offline"),
        ];
        for (stderr, needle) in cases {
            let err = map_git_error(
                &[stderr.to_string()],
                "https://github.com/acme/my-plugin",
            )
            .to_string();
            assert!(err.contains(needle), "{stderr}: {err}");
        }
        // Unknown failures keep the raw tail — never swallowed.
        let raw = map_git_error(&["something exploded".into()], "https://github.com/a/b");
        assert!(raw.to_string().contains("something exploded"));
    }

    fn fixture_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir()
            .join(format!("rexenv-repo-inspect-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn inspection_prefers_package_manager_field_over_lockfile() {
        let d = fixture_dir("pm");
        std::fs::write(
            d.join("package.json"),
            r#"{"packageManager":"pnpm@9.1.0","scripts":{"build":"wp-scripts build"},"engines":{"node":">=18"}}"#,
        )
        .unwrap();
        std::fs::write(d.join("yarn.lock"), "").unwrap(); // lies — field wins
        std::fs::write(d.join("composer.json"), "{}").unwrap();
        std::fs::write(d.join("plugin.php"), "<?php\n/*\nPlugin Name: Fixture PM\n*/\n").unwrap();
        let i = inspect_repo(&d);
        let node = i.node.expect("node plan");
        assert_eq!(node.manager, "pnpm");
        assert_eq!(node.pinned_by, "packageManager");
        assert!(node.has_build);
        assert!(i.composer);
        assert_eq!(i.wp.kind, "plugin");
        assert_eq!(i.wp.name.as_deref(), Some("Fixture PM"));
        assert_eq!(i.node_want.as_deref(), Some(">=18"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn inspection_falls_back_lockfile_then_npm_and_nvmrc_beats_engines() {
        let d = fixture_dir("lock");
        std::fs::write(d.join("package.json"), r#"{"engines":{"node":"20"}}"#).unwrap();
        std::fs::write(d.join("pnpm-lock.yaml"), "").unwrap();
        std::fs::write(d.join(".nvmrc"), "18.19.0\n").unwrap();
        std::fs::write(d.join("style.css"), "/*\nTheme Name: Fixture Theme\n*/\n").unwrap();
        let i = inspect_repo(&d);
        let node = i.node.expect("node plan");
        assert_eq!((node.manager.as_str(), node.pinned_by.as_str()), ("pnpm", "lockfile"));
        assert!(!node.has_build);
        assert_eq!(i.node_want.as_deref(), Some("18.19.0")); // .nvmrc wins
        assert_eq!(i.wp.kind, "theme");
        let bare = fixture_dir("bare");
        std::fs::write(bare.join("package.json"), "{}").unwrap();
        let b = inspect_repo(&bare);
        assert_eq!(b.node.unwrap().pinned_by, "default"); // npm
        assert_eq!(b.wp.kind, "none");
        assert!(!b.composer);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&bare);
    }

    #[test]
    fn node_version_warning_fires_on_real_mismatches_only() {
        // .nvmrc-style exact major mismatch.
        assert!(node_version_warning("18", "v22.17.0").is_some());
        assert!(node_version_warning("v20.11.1", "v22.17.0").is_some());
        assert!(node_version_warning("22", "v22.17.0").is_none());
        // Range minimum: only warn when we're BELOW it.
        assert!(node_version_warning(">=18", "v22.17.0").is_none());
        assert!(node_version_warning(">=24", "v22.17.0").is_some());
        // Unparseable pins (lts aliases) never warn — display-only honesty.
        assert!(node_version_warning("lts/iron", "v22.17.0").is_none());
        let msg = node_version_warning("18", "v22.17.0").unwrap();
        assert!(msg.contains("nvm install 18"), "{msg}");
    }

    #[test]
    fn porcelain_v2_parses_the_tricky_shapes() {
        // Dirty + ahead simultaneously, upstream set: the everyday case.
        let dirty_ahead = "# branch.oid 3fe2\n\
                           # branch.head feat/x\n\
                           # branch.upstream origin/feat/x\n\
                           # branch.ab +2 -1\n\
                           1 .M N... 100644 100644 100644 a1 a2 src/index.js\n\
                           1 M. N... 100644 100644 100644 b1 b2 readme.md\n\
                           2 R. N... 100644 100644 100644 c1 c2 R100 new.js\told.js\n\
                           u UU N... 100644 100644 100644 100644 d1 d2 d3 conflict.js\n\
                           ? build/out.js\n";
        let st = parse_status_v2(dirty_ahead);
        assert_eq!(st.branch.as_deref(), Some("feat/x"));
        assert!(!st.detached && !st.unborn);
        assert_eq!(st.upstream.as_deref(), Some("origin/feat/x"));
        assert_eq!((st.ahead, st.behind), (Some(2), Some(1)));
        assert_eq!((st.changed, st.untracked), (4, 1)); // 1+1+rename+unmerged / ?

        // Detached HEAD: no branch, no upstream/ab lines.
        let detached = "# branch.oid 3fe2\n# branch.head (detached)\n";
        let st = parse_status_v2(detached);
        assert!(st.detached);
        assert_eq!(st.branch, None);
        assert_eq!(st.ahead, None);

        // No upstream: branch known, ahead/behind UNKNOWN (None, not 0).
        let no_up = "# branch.oid 3fe2\n# branch.head main\n1 .M N... 100644 100644 100644 a b f\n";
        let st = parse_status_v2(no_up);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.upstream, None);
        assert_eq!(st.ahead, None);
        assert_eq!(st.changed, 1);

        // Fresh clone/init with no commits: unborn.
        let unborn = "# branch.oid (initial)\n# branch.head main\n? plugin.php\n";
        let st = parse_status_v2(unborn);
        assert!(st.unborn);
        assert_eq!(st.untracked, 1);
    }

    #[test]
    fn loss_warning_names_exactly_what_dies() {
        let mk = |changed, untracked, ahead: Option<u32>, upstream: bool, detached: bool, unborn: bool| GitStatus {
            branch: (!detached).then(|| "main".into()),
            detached,
            unborn,
            upstream: upstream.then(|| "origin/main".into()),
            ahead,
            behind: None,
            changed,
            untracked,
        };
        // The case the human verify targets: dirty + unpushed, all named.
        assert_eq!(
            loss_warning(&mk(3, 2, Some(2), true, false, false)).unwrap(),
            "3 changed files, 2 untracked files, and 2 unpushed commits will be lost."
        );
        assert_eq!(
            loss_warning(&mk(1, 0, Some(0), true, false, false)).unwrap(),
            "1 changed file will be lost."
        );
        // Clean + pushed → no warning at all.
        assert_eq!(loss_warning(&mk(0, 0, Some(0), true, false, false)), None);
        // No upstream: honest that unpushed can't be counted.
        let no_up = loss_warning(&mk(2, 0, None, false, false, false)).unwrap();
        assert!(no_up.starts_with("2 changed files will be lost"), "{no_up}");
        assert!(no_up.contains("no upstream"), "{no_up}");
        // Clean but no upstream: still can't prove pushed — caveat-only warning.
        let clean_no_up = loss_warning(&mk(0, 0, None, false, false, false)).unwrap();
        assert!(clean_no_up.contains("can't be verified as pushed"), "{clean_no_up}");
        // Detached: the caveat names it.
        let det = loss_warning(&mk(0, 1, None, false, true, false)).unwrap();
        assert!(det.contains("detached HEAD"), "{det}");
        // Unborn with untracked work: counted, no upstream caveat (meaningless).
        let un = loss_warning(&mk(0, 3, None, false, false, true)).unwrap();
        assert_eq!(un, "3 untracked files will be lost.");
    }

    #[test]
    #[cfg(unix)] // symlink fixtures
    fn link_validation_refuses_nesting_cycles_and_collisions() {
        let base = fixture_dir("linkval");
        let docroot = base.join("site/public");
        let plugins = docroot.join("wp-content/plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        let outside = base.join("checkouts/my-plugin");
        std::fs::create_dir_all(&outside).unwrap();

        // Happy path: canonicalized target comes back.
        let dest = plugins.join("my-plugin");
        let ok = validate_link_target(&docroot, &dest, &outside).unwrap();
        assert!(ok.ends_with("checkouts/my-plugin"));
        // Target inside the docroot → self-link refused.
        let inner = plugins.join("existing");
        std::fs::create_dir_all(&inner).unwrap();
        let err = validate_link_target(&docroot, &dest, &inner).unwrap_err().to_string();
        assert!(err.contains("inside this site"), "{err}");
        // Target CONTAINING the docroot → cycle refused.
        let err = validate_link_target(&docroot, &dest, &base.join("site")).unwrap_err().to_string();
        assert!(err.contains("cycle"), "{err}");
        // Collision — including a DANGLING symlink at dest (exists() is false
        // for those; symlink_metadata catches it).
        std::os::unix::fs::symlink(base.join("gone"), plugins.join("dangling")).unwrap();
        let err = validate_link_target(&docroot, &plugins.join("dangling"), &outside)
            .unwrap_err()
            .to_string();
        assert!(err.contains("already exists"), "{err}");
        // Missing target.
        assert!(validate_link_target(&docroot, &dest, &base.join("nope")).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    #[cfg(unix)] // symlink fixtures
    fn symlink_deletes_partition_on_fs_truth_not_provenance() {
        let base = fixture_dir("delpart");
        let plugins = base.join("plugins");
        std::fs::create_dir_all(plugins.join("real-plugin")).unwrap();
        let target = base.join("elsewhere/linked-plugin");
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, plugins.join("linked-plugin")).unwrap();
        let (linked, normal) = partition_symlink_deletes(
            &plugins,
            &["real-plugin".into(), "linked-plugin".into(), "not-there".into()],
        );
        assert_eq!(linked, vec!["linked-plugin"]);
        // Missing dirs stay on the normal path (wp-cli reports them honestly).
        assert_eq!(normal, vec!["real-plugin", "not-there"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn script_listing_filters_pathological_names_and_flags_watchy() {
        let d = fixture_dir("scripts");
        std::fs::write(
            d.join("package.json"),
            r#"{"scripts":{
                "build":"wp-scripts build",
                "start":"wp-scripts start",
                "dev:hot":"vite --hot",
                "build:watch":"tsc -w",
                "lint":"eslint .",
                "-evil":"rm -rf /",
                "has space":"echo no",
                "empty":"  "
            }}"#,
        )
        .unwrap();
        let scripts = list_scripts(&d);
        let names: Vec<(&str, bool)> =
            scripts.iter().map(|s| (s.name.as_str(), s.watchy)).collect();
        assert_eq!(
            names,
            vec![
                ("build", false),
                ("build:watch", true), // contains watch
                ("dev:hot", true),     // dev: prefix
                ("lint", false),
                ("start", true), // wp-scripts start IS watch mode
            ]
        );
        assert!(scripts.iter().all(|s| !s.command.is_empty()));
        // No package.json / no scripts → empty, no error.
        assert!(list_scripts(&d.join("nope")).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ref_validation_blocks_argv_tricks() {
        for ok in ["main", "feat/fast-build", "v1.2.0", "release-2.x", "user/topic_1"] {
            assert!(validate_ref(ok).is_ok(), "{ok}");
        }
        for bad in ["-f", "--force", "", "a b", "a;b", "../x", "a..b", "/abs", "trail/"] {
            assert!(validate_ref(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn git_op_errors_map_to_actionable_messages() {
        let cases: &[(&str, &str, &str)] = &[
            ("pull", "fatal: Not possible to fast-forward, aborting.", "DIVERGED"),
            ("pull", "hint: Need to specify how to reconcile divergent branches.", "DIVERGED"),
            (
                "pull",
                "error: Your local changes to the following files would be overwritten by merge:",
                "Commit or stash",
            ),
            ("checkout", "error: pathspec 'nope' did not match any file(s)", "Fetch first"),
            (
                "pull",
                "fatal: You are not currently on a branch.",
                "detached HEAD",
            ),
            (
                "push",
                "! [rejected] main -> main (non-fast-forward)",
                "Pull first",
            ),
            ("push", "Updates were rejected because the remote contains work", "Pull first"),
        ];
        for (op, stderr, needle) in cases {
            let err = map_git_op_error(op, &[stderr.to_string()]).to_string();
            assert!(err.contains(needle), "{op}/{stderr}: {err}");
        }
        // Unknown op failures fall through to the shared mapping (raw tail kept).
        let raw = map_git_op_error("fetch", &["weird explosion".into()]).to_string();
        assert!(raw.contains("weird explosion"), "{raw}");
    }

    #[test]
    fn pull_ref_parse_reads_both_hosts_sorts_numerically_and_skips_noise() {
        // Lexicographic ls-remote order + GitLab + peeled + junk, all mixed.
        let out = "aaa1\trefs/pull/10/head\n\
                   bbb2\trefs/pull/100/head\n\
                   ccc3\trefs/pull/2/head\n\
                   ddd4\trefs/pull/2/merge\n\
                   eee5\trefs/merge-requests/7/head\n\
                   fff6\trefs/pull/3/head^{}\n\
                   ggg7\trefs/heads/main\n\
                   hhh8\trefs/pull/notanumber/head\n";
        let prs = parse_pull_refs(out);
        // Highest (newest) first; /merge, peeled, non-PR, non-numeric all skipped.
        assert_eq!(prs.iter().map(|p| p.number).collect::<Vec<_>>(), vec![100, 10, 7, 2]);
        assert_eq!(prs[0].sha, "bbb2");
        assert_eq!(prs[2].full_ref, "refs/merge-requests/7/head");

        // Routing predicate: PR refs take the fetch-then-detach path, the
        // rest (branches, tags) stay on plain checkout.
        assert!(is_pull_ref("refs/pull/12/head"));
        assert!(is_pull_ref("refs/merge-requests/7/head"));
        assert!(!is_pull_ref("refs/tags/v1.2.0"));
        assert!(!is_pull_ref("main"));
        // And both patterns pass the argv-injection validator as-is.
        assert!(validate_ref("refs/pull/12/head").is_ok());
        assert!(validate_ref("refs/merge-requests/7/head").is_ok());
    }

    #[test]
    fn dep_fingerprints_are_pinned_prefixed_and_input_sensitive() {
        let d = fixture_dir("depfp");
        std::fs::write(d.join("composer.json"), "{}").unwrap();
        std::fs::write(d.join("composer.lock"), "v1").unwrap();
        let a = composer_fingerprint(&d);
        // Format contract: algorithm-prefixed so a future scheme change reads
        // as FOREIGN (→ unverified), never as a silent mismatch (→ stale).
        assert!(a.starts_with("fnv1a:1:"), "{a}");
        // Deterministic + input-sensitive.
        assert_eq!(a, composer_fingerprint(&d));
        std::fs::write(d.join("composer.lock"), "v2").unwrap();
        assert_ne!(a, composer_fingerprint(&d));
        // Node inputs are independent of composer inputs.
        let n1 = node_fingerprint(&d);
        std::fs::write(d.join("package.json"), "{}").unwrap();
        assert_ne!(n1, node_fingerprint(&d));
        // Pinned algorithm: FNV-1a of a known input, locked to the exact
        // value so a hasher swap can't slip through unnoticed.
        assert_eq!(fnv1a(b"rexenv", 0xcbf2_9ce4_8422_2325), 0x57f4_61a6_df76_d9c7);
    }

    #[test]
    fn check_deps_verdicts_are_honest_and_never_cry_wolf() {
        let d = fixture_dir("depcheck");
        let inspect = |d: &std::path::Path| inspect_repo(d);

        // No manifests at all → both not-applicable.
        let r = check_deps(&d, &inspect(&d), None, None);
        assert_eq!((r.composer, r.node), (DepVerdict::NotApplicable, DepVerdict::NotApplicable));

        // composer.json present, vendor/ missing → MISSING (needs install).
        std::fs::write(d.join("composer.json"), "{}").unwrap();
        let r = check_deps(&d, &inspect(&d), None, None);
        assert_eq!(r.composer, DepVerdict::Missing);
        assert!(needs_install(r.composer));

        // vendor/ present + NO stored fp (pre-v15 rows, adopted checkouts,
        // terminal installs) → UNVERIFIED, never stale. The existing-user
        // upgrade bar: NULL must not scream "reinstall".
        std::fs::create_dir_all(d.join("vendor")).unwrap();
        let r = check_deps(&d, &inspect(&d), None, None);
        assert_eq!(r.composer, DepVerdict::Unverified);
        assert!(!needs_install(r.composer));

        // Stored fp matches current inputs → up to date.
        let fp = composer_fingerprint(&d);
        let r = check_deps(&d, &inspect(&d), Some(&fp), None);
        assert_eq!(r.composer, DepVerdict::UpToDate);

        // Lockfile changes after install → STALE (provable, offer install).
        std::fs::write(d.join("composer.lock"), "changed").unwrap();
        let r = check_deps(&d, &inspect(&d), Some(&fp), None);
        assert_eq!(r.composer, DepVerdict::Stale);
        assert!(needs_install(r.composer));

        // FOREIGN-format stored fp (future algorithm, old scheme) → treated
        // as absent → unverified, not a wall of false "stale".
        let r = check_deps(&d, &inspect(&d), Some("sha256:1:deadbeef"), None);
        assert_eq!(r.composer, DepVerdict::Unverified);

        // config.vendor-dir honored (sane relative only).
        std::fs::write(
            d.join("composer.json"),
            r#"{"config":{"vendor-dir":"deps"}}"#,
        )
        .unwrap();
        let r = check_deps(&d, &inspect(&d), None, None);
        assert_eq!(r.vendor_dir, "deps");
        assert_eq!(r.composer, DepVerdict::Missing); // deps/ doesn't exist
        std::fs::write(
            d.join("composer.json"),
            r#"{"config":{"vendor-dir":"../escape"}}"#,
        )
        .unwrap();
        assert_eq!(check_deps(&d, &inspect(&d), None, None).vendor_dir, "vendor");

        // Node family: package.json + node_modules missing → missing;
        // present + no marker → unverified.
        std::fs::write(d.join("package.json"), "{}").unwrap();
        let r = check_deps(&d, &inspect(&d), None, None);
        assert_eq!(r.node, DepVerdict::Missing);
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        let r = check_deps(&d, &inspect(&d), None, None);
        assert_eq!(r.node, DepVerdict::Unverified);
    }

    #[test]
    fn fetch_args_bring_the_full_tag_set_and_survive_moved_tags() {
        let args = fetch_args();
        assert_eq!(args.first().map(String::as_str), Some("fetch"));
        assert!(args.contains(&"--tags".to_string()));
        // --force is load-bearing: a MOVED remote tag (rolling v1/latest) is
        // rejected without it (git ≥2.20) and the whole fetch exits non-zero
        // — every Fetch on that repo would fail forever.
        assert!(args.contains(&"--force".to_string()));
        // --prune-tags must never appear: it deletes local user-created tags.
        assert!(!args.contains(&"--prune-tags".to_string()));
    }

    #[test]
    fn lockfile_fingerprint_tracks_dependency_files_only() {
        let d = fixture_dir("lockfp");
        std::fs::write(d.join("package.json"), "{}").unwrap();
        std::fs::write(d.join("package-lock.json"), "v1").unwrap();
        let a = lockfile_fingerprint(&d);
        // Unrelated file churn → no change.
        std::fs::write(d.join("readme.md"), "hello").unwrap();
        assert_eq!(a, lockfile_fingerprint(&d));
        // Lockfile content change → change.
        std::fs::write(d.join("package-lock.json"), "v2").unwrap();
        let b = lockfile_fingerprint(&d);
        assert_ne!(a, b);
        // A lockfile APPEARING (branch adds composer) → change.
        std::fs::write(d.join("composer.lock"), "x").unwrap();
        assert_ne!(b, lockfile_fingerprint(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)] // creates a symlink fixture
    fn unmanaged_scan_finds_unknown_checkouts_only() {
        let base = std::env::temp_dir().join(format!("rexenv-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let content = base.join("wp-content/plugins");
        std::fs::create_dir_all(content.join("known-git/.git")).unwrap();
        std::fs::create_dir_all(content.join("manual-clone/.git")).unwrap();
        std::fs::create_dir_all(content.join("plain-plugin")).unwrap(); // no .git
        // .git as a FILE (worktree/submodule form) still counts.
        std::fs::create_dir_all(content.join("worktree-form")).unwrap();
        std::fs::write(content.join("worktree-form/.git"), "gitdir: elsewhere").unwrap();
        std::fs::create_dir_all(content.join(".hidden/.git")).unwrap(); // skipped
        // A symlinked checkout elsewhere on disk.
        let target = base.join("elsewhere/my-linked");
        std::fs::create_dir_all(target.join(".git")).unwrap();
        std::os::unix::fs::symlink(&target, content.join("my-linked")).unwrap();

        let found = scan_unmanaged(&content, &["known-git".to_string()]);
        let names: Vec<(&str, bool)> =
            found.iter().map(|u| (u.dir_name.as_str(), u.linked)).collect();
        assert_eq!(
            names,
            vec![("manual-clone", false), ("my-linked", true), ("worktree-form", false)]
        );
        // Missing dir → empty, no error.
        assert!(scan_unmanaged(&base.join("nope"), &[]).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn composer_and_node_errors_map_to_actionable_messages() {
        let php = map_composer_error(&["  - Root composer.json requires php >=8.4 but your php version (8.3.31) does not satisfy that requirement.".into()]);
        assert!(php.to_string().contains("Switch the site's PHP version"), "{php}");
        let ext = map_composer_error(&["requires ext-imagick * -> it is missing".into()]);
        assert!(ext.to_string().contains("extension"), "{ext}");
        let gyp = map_node_error(&["gyp ERR! stack Error".into()]);
        assert!(gyp.to_string().ends_with("$ xcode-select --install"), "{gyp}");
        let engine = map_node_error(&["npm warn EBADENGINE Unsupported engine".into()]);
        assert!(engine.to_string().contains("Node version"), "{engine}");
        // Unknown failures keep the tail.
        let raw = map_node_error(&["ERR_PNPM_SOMETHING went sideways".into()]);
        assert!(raw.to_string().contains("went sideways"), "{raw}");
    }

    #[test]
    fn captured_cap_kills_the_whole_group_so_a_grandchild_cant_stall_the_join() {
        // The B7 scenario: the leader backgrounds a grandchild that inherits the
        // stdout pipe, then goes silent. The OLD leader-only child.kill() left
        // the grandchild alive holding the pipe → read_to_end never hit EOF →
        // the join hung ~30s. stop_group kills the WHOLE group (grandchild too),
        // the pipe closes, and the join returns fast — so the timeout Err comes
        // back in well under the grandchild's 30s lifetime.
        let plat = crate::platform::current();
        let start = std::time::Instant::now();
        let r = run_captured_with_cap(
            plat.supervisor(),
            Path::new("/bin/sh"),
            &["-c", "sleep 30 & echo started; wait"],
            &std::env::temp_dir(),
            &[],
            Duration::from_millis(500),
        );
        assert!(r.is_err(), "a stalled probe must time out");
        assert!(r.unwrap_err().to_string().contains("timed out"), "the timeout error");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the group kill closed the pipe so the join returned fast (not ~30s): {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn idle_watchdog_kills_a_silent_step_and_reports_the_stall() {
        // One line of output then total silence: with a short idle limit the
        // step must come back non-ok FAST with the stall notice in the tail —
        // previously a black-holed step hung until the user cancelled (B25).
        let plat = crate::platform::current();
        let cancel = CancelToken::new();
        let mut lines = Vec::new();
        let start = std::time::Instant::now();
        let r = run_step_streamed(
            plat.supervisor(),
            Path::new("/bin/sh"),
            &["-c".into(), "echo hello; sleep 30".into()],
            &std::env::temp_dir(),
            &[],
            &cancel,
            &mut |l| lines.push(l.to_string()),
            Some(Duration::from_millis(500)),
        )
        .unwrap();
        assert!(!r.ok, "a stalled step must not read as success");
        assert!(!r.cancelled, "stall is not a user cancel");
        assert!(
            r.tail.iter().any(|l| l.contains("killed as stalled")),
            "tail carries the stall notice: {:?}",
            r.tail
        );
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "bounded by the idle limit, not the child: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn idle_watchdog_resets_on_every_line_so_streaming_steps_survive() {
        // Emits a line every ~200ms for ~2s TOTAL — longer than the 1s idle
        // limit. Each line resets the clock, so a slow-but-STREAMING step
        // completes ok; only total silence trips the guard (the invariant).
        let plat = crate::platform::current();
        let cancel = CancelToken::new();
        let mut n = 0u32;
        let r = run_step_streamed(
            plat.supervisor(),
            Path::new("/bin/sh"),
            &["-c".into(), "for i in 1 2 3 4 5 6 7 8; do echo tick; sleep 0.2; done".into()],
            &std::env::temp_dir(),
            &[],
            &cancel,
            &mut |_| n += 1,
            Some(Duration::from_secs(1)),
        )
        .unwrap();
        assert!(r.ok, "a streaming step outliving the idle window must succeed");
        assert!(n >= 8, "all lines delivered: {n}");
    }

    #[test]
    fn no_idle_limit_keeps_a_silent_step_alive() {
        // The user-script exemption: None means silence never kills — a quiet
        // step finishes on its own terms (here: 1s of silence, longer than the
        // watchdog tests' limits, then a clean exit).
        let plat = crate::platform::current();
        let cancel = CancelToken::new();
        let r = run_step_streamed(
            plat.supervisor(),
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 1; echo done".into()],
            &std::env::temp_dir(),
            &[],
            &cancel,
            &mut |_| {},
            None,
        )
        .unwrap();
        assert!(r.ok, "silence with no idle limit must not be killed");
    }
}
