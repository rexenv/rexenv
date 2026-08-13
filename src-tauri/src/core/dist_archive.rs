//! core::dist_archive — build a distributable `.zip` from a plugin/theme checkout.
//!
//! Runs the vendored `wp dist-archive` ([`crate::core::wp_packages`]) against an
//! asset directory, writing the archive into a directory **we** choose. The job
//! plumbing (streamed log, cancel, one-per-dir) is `repo`'s existing runner —
//! this module is the argv and the rules about it, not a second job system.
//!
//! # The one rule this module exists to enforce
//!
//! `wp dist-archive <path>` with no target writes the zip to
//! `dirname(realpath(<path>))`. For a cloned asset that is `wp-content/plugins/`;
//! for a **linked** asset `realpath` resolves the symlink first, so the default
//! lands inside the **user's own repository's parent**. A build artefact turning
//! up in someone's `git status` is not a convenience we get to leave behind, so
//! [`argv`] REFUSES a target that is the source's parent, the source itself, or
//! anywhere inside it — it is not merely tested for, because the difference
//! between "no caller does that" and "no caller can" is the difference this
//! project keeps re-learning.
//!
//! The comparison is against the **canonical** source path, not the path as
//! given. That is the whole subtlety of the linked case: `wp-content/plugins/foo`
//! and `~/code/foo` are the same directory, and the tool resolves to the second
//! one. A guard that compared the unresolved path would pass while the zip
//! landed in `~/code/`.
//!
//! # Why `WP_CLI_PACKAGES_DIR` is neutralised for this spawn
//!
//! Not for the reason in ledger #228 — every rexenv-run `wp` has been pinned
//! since 13 Aug 2026, and this predates it. This is narrower and settles here
//! independently, which is why it did not wait for that decision: a user may
//! have their own
//! `dist-archive-command` installed globally at a different version, and the
//! behaviour this feature is built around is **version-specific** (`.distignore`
//! semantics, the missing `.gitignore` fallback, the filename format). Measured
//! 4 Aug 2026: with a global v2.0.1 present and our `--require` given, **ours
//! wins** — but that is undocumented precedence between `--require` and package
//! autoloading, exactly the "posture resting on a third party's behaviour" shape
//! this project has been burned by. Pointing the packages dir at an empty
//! rexenv-owned directory costs one environment variable and makes precedence
//! irrelevant. Since #228 landed, the same is true of every other rexenv-run
//! `wp` — the one place a user's global packages still apply is rexenv's
//! terminal, which is deliberately theirs.
//!
//! # Silence is legitimate here, so there is no idle watchdog
//!
//! dist-archive shells out to `zip` through `WP_CLI::launch(…, $capture = true)`,
//! so the child's output never reaches our pipe — a large tree is silent from
//! first line to last **by construction**. An idle limit would therefore fire on
//! correct runs, which is B34's mistake (a cap tighter than legitimate work).
//! The bound is the user's cancel, and the caller owns that.

use crate::core::repo::{self, CancelToken, StepResult};
use crate::error::{Error, Result};
use crate::platform::traits::ProcessSupervisor;
use std::path::{Path, PathBuf};

/// Everything one archive run needs. A struct rather than eight parameters
/// because `run` would otherwise trip the clippy bar, and because the
/// source/target pair is the thing the rules below are about.
pub struct ArchiveSpec<'a> {
    /// Bundled PHP.
    pub php_bin: &'a Path,
    /// Bundled WP-CLI phar.
    pub wp_phar: &'a Path,
    /// `vendor/autoload.php` of the vendored tree (`wp_packages`).
    pub autoload: &'a Path,
    /// An EMPTY rexenv-owned directory — see the module doc.
    pub packages_dir: &'a Path,
    /// `TMPDIR` for the child — inside the swept scratch dir. See [`ScratchDir`].
    pub tmp_dir: &'a Path,
    /// The asset checkout (`repo::asset_dest`), symlink or real directory.
    pub source: &'a Path,
    /// Where the archive is written. Ours, never the source's parent.
    pub out_dir: &'a Path,
}

/// The argv rexenv spawns — **the only place one is built**, so a guard that
/// reads this reads what actually runs.
///
/// `--force` is a belt, not the mechanism: callers hand us an empty directory of
/// our own, so an existing file is unreachable. If one ever were reachable, the
/// prompt dist-archive falls back to becomes an *uncaught PHP fatal* under a
/// non-TTY (measured), which is worth a flag to make impossible rather than a
/// handler to parse.
pub fn argv(spec: &ArchiveSpec<'_>) -> Result<Vec<String>> {
    let source = spec
        .source
        .canonicalize()
        .map_err(|e| Error::Other(format!("{}: {e}", spec.source.display())))?;
    if !source.is_dir() {
        return Err(Error::Other(format!(
            "{} is not a directory — nothing to archive",
            spec.source.display()
        )));
    }
    require_distignore(&source)?;
    let out = resolve_for_compare(spec.out_dir);
    // A linked checkout has TWO parents: `wp-content/plugins/` (where rexenv
    // points at it) and the user's own `~/code/` (where it lives, and where
    // dist-archive would default to). "Never beside the checkout" has to mean
    // both, or the rule is true only under the name the reader didn't pick.
    let link_parent = spec.source.parent().map(resolve_for_compare);
    refuse_bad_target(&source, link_parent.as_deref(), &out)?;

    let mut argv = crate::core::wordpress::wp_argv_prefix(spec.wp_phar);
    argv.extend([
        // Before the subcommand: this is what makes `dist-archive` exist at all.
        format!("--require={}", spec.autoload.display()),
        "dist-archive".into(),
        source.display().to_string(),
        out.display().to_string(),
        "--force".into(),
    ]);
    Ok(argv)
}

/// Does this checkout have the file that decides what ships?
///
/// Checked at the **canonical** source, because that is where dist-archive
/// looks — a linked asset's `.distignore` lives in the user's repository, not
/// beside the symlink. The UI asks this to decide whether the action is offered
/// at all; [`require_distignore`] is the same fact enforced on the spawn path,
/// so the button and the command can never disagree about it.
pub fn has_distignore(source: &Path) -> bool {
    source
        .canonicalize()
        .map(|dir| dir.join(".distignore").is_file())
        .unwrap_or(false)
}

/// A starter `.distignore`, quoted verbatim in the refusal.
///
/// Because the refusal has to be actionable by the person who hits it, and that
/// person is a plugin developer who may never have heard of `.distignore` —
/// "no .distignore found" alone sends them to a search engine rather than to a
/// fix. These are the entries wp-cli's own documentation opens with, plus the
/// two that cause the damage in practice (`node_modules`, `vendor` is
/// deliberately NOT here — plenty of plugins ship theirs).
pub const DISTIGNORE_STARTER: &str = ".git\n.github\n.gitignore\n.distignore\n\
                                     node_modules\ntests\n*.dist\n";

/// Refuse to archive a checkout with no `.distignore`.
///
/// **This is the feature, not a safety rail around it.** Without the file,
/// dist-archive archives *everything* — `.git`, `node_modules`, editor
/// droppings — and reports it as `Success:` with exit 0 (measured; see
/// `docs/PLAN-dist-archive.md` §1.2). There is no `.gitignore` fallback: that
/// existed before 3.0 and is gone, so the harmful outcome is the DEFAULT one and
/// it announces itself as a success. A zip like that, uploaded to wp.org or sent
/// to a client, is worse than no zip.
///
/// A PRECONDITION, deliberately — never a warning recovered from the child's
/// output. Reading it afterwards would mean the archive already exists, and then
/// the honest options are to delete something we just made or to hand over a
/// file we have told the user not to trust. It also lives inside [`argv`], the
/// one function that builds the spawn, so no future caller can route around it.
///
/// # Where the line is, and why it is there
///
/// Only ABSENCE is refused. An **empty** `.distignore`, or one that fails to
/// exclude `.git`, is not — and that is a decision, not an oversight. Judging
/// the CONTENT would mean deciding whether a given set of patterns actually
/// excludes a given path, which is gitignore-matching semantics: precisely the
/// compatibility claim this feature refused to own when it chose to vendor the
/// package rather than reimplement it (`PLAN-dist-archive.md` §2). Absence is a
/// fact rexenv can check without owning any semantics; content is not. Creating
/// the file at all is an explicit act by the developer, and past that point what
/// ships is theirs to decide.
pub fn require_distignore(canonical_source: &Path) -> Result<()> {
    if canonical_source.join(".distignore").is_file() {
        return Ok(());
    }
    let name = canonical_source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| canonical_source.display().to_string());
    Err(Error::Other(format!(
        "{name} has no .distignore, so the archive would contain everything in \
         the checkout — including .git and node_modules.\n\n\
         wp dist-archive decides what to leave out from a .distignore file, and \
         it does NOT fall back to .gitignore. With no such file it archives the \
         lot and still reports success, so rexenv stops here instead.\n\n\
         Create .distignore in {} listing what must not ship. A usual start:\n\n\
         {}\n\
         The syntax is .gitignore's.",
        canonical_source.display(),
        DISTIGNORE_STARTER
            .lines()
            .map(|l| format!("    {l}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )))
}

/// The rules, kept apart from argv assembly so they can be read as rules.
fn refuse_bad_target(source: &Path, link_parent: Option<&Path>, out: &Path) -> Result<()> {
    if out == source {
        return Err(Error::Other(format!(
            "refusing to write the archive into the checkout itself ({})",
            source.display()
        )));
    }
    if out.starts_with(source) {
        return Err(Error::Other(format!(
            "refusing to write the archive inside the checkout ({}) — it would \
             archive its own output",
            out.display()
        )));
    }
    if Some(out) == source.parent() {
        return Err(Error::Other(format!(
            "refusing to write the archive beside the checkout ({}) — that is \
             dist-archive's own default, and for a linked asset it is the \
             user's repository",
            out.display()
        )));
    }
    if Some(out) == link_parent {
        return Err(Error::Other(format!(
            "refusing to write the archive beside the linked checkout ({}) — \
             that is inside the site's wp-content",
            out.display()
        )));
    }
    Ok(())
}

/// Resolve the TARGET as far as the filesystem allows, so both sides of every
/// comparison below are the same kind of path.
///
/// Found by the tests in this module rather than reasoned out, and it is the
/// coverage family on a two-sided comparison: the source is `canonicalize`d, so
/// it comes back as `/private/var/…`, while a lexically-normalised target stays
/// `/var/…` — the same directory, and every `starts_with` / `==` between them is
/// false. On macOS `/var`, `/tmp` and `/etc` are all symlinks, and app-data
/// paths reach through them, so this is the ordinary case and not a corner.
///
/// `canonicalize` alone can't be used: the target usually does not exist yet.
/// So canonicalise the deepest ancestor that DOES exist and re-append the rest —
/// which resolves every symlinked prefix while still naming a path we are about
/// to create.
fn resolve_for_compare(p: &Path) -> PathBuf {
    let lexical = normalise(p);
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut probe = lexical.as_path();
    loop {
        if let Ok(real) = probe.canonicalize() {
            let mut out = real;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (probe.file_name(), probe.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                probe = parent;
            }
            // Nothing on the path exists (or we reached the root): the lexical
            // form is the best available answer, and it is still normalised.
            _ => return lexical,
        }
    }
}

/// Lexical normalisation for a path that need not exist yet — an un-normalised
/// `a/b/../b` would otherwise slip past `starts_with`.
fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in p.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The environment overrides layered on top of the caller's login-shell env.
///
/// Later entries win in `spawn_streamed`'s env application, so these override a
/// user's own values rather than being shadowed by them — which is the point:
/// a developer who exports `WP_CLI_PACKAGES_DIR` in their shell profile would
/// otherwise reintroduce exactly what this is here to exclude.
pub fn env_overrides(spec: &ArchiveSpec<'_>) -> Vec<(String, String)> {
    vec![
        (
            "WP_CLI_PACKAGES_DIR".to_string(),
            spec.packages_dir.display().to_string(),
        ),
        // dist-archive writes its work into `sys_get_temp_dir()` and NEVER
        // removes it — on success as much as on failure, and a whole filtered
        // copy of the plugin whenever the source contains a symlink (measured:
        // 304 KB for a toy fixture; tens of MB for anything with a vendor dir).
        // Pointing TMPDIR inside our scratch dir does not fix the tool; it puts
        // the litter inside the thing we already delete, which is the only fix
        // available to us. Verified 4 Aug: with TMPDIR set, the system temp dir
        // gained nothing and the copy appeared here instead.
        ("TMPDIR".to_string(), spec.tmp_dir.display().to_string()),
    ]
}

/// Run one archive build, streaming output to `on_line`. Blocking — callers use
/// `spawn_blocking`, the wp-cli convention.
///
/// A non-zero exit is reported in the [`StepResult`], not turned into an error:
/// the caller maps it with the tail in hand, exactly as the repo jobs do.
pub fn run(
    supervisor: &dyn ProcessSupervisor,
    spec: &ArchiveSpec<'_>,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<StepResult> {
    let args = argv(spec)?;
    let mut full_env = env.to_vec();
    full_env.extend(env_overrides(spec));
    std::fs::create_dir_all(spec.out_dir)?;
    std::fs::create_dir_all(spec.packages_dir)?;
    std::fs::create_dir_all(spec.tmp_dir)?;
    repo::run_step_streamed(
        supervisor,
        spec.php_bin,
        &args,
        // cwd is immaterial (the target is absolute and dist-archive chdirs
        // internally), so use the one directory we know exists and can read.
        spec.source,
        &full_env,
        cancel,
        on_line,
        // See the module doc: this command is silent by construction.
        None,
    )
}

/// A per-run working directory under app-data, removed when it goes out of
/// scope — **however** the scope is left.
///
/// # Why a `Drop` guard rather than three cleanup calls
///
/// The archive must be swept on ok, on failure, AND on cancel, and the three
/// are not equally likely to be remembered: cancel is the one a person writing
/// the happy path last, and it is also the one that litters hardest, because an
/// interrupted dist-archive has usually just finished copying the plugin into
/// its temp dir. Three explicit calls would be the coverage/surface family
/// written by hand — a claim about every exit, implemented at the exits someone
/// thought of. `Drop` runs on all of them, including the `?` returns in
/// between and a panic, so the exits do not have to be enumerated correctly.
///
/// # Layout
///
/// `out/` is the target handed to dist-archive; `tmp/` is the child's `TMPDIR`.
/// They are separate so that finding the produced archive is unambiguous — the
/// tool's own leftovers never land in the directory we scan.
///
/// The name carries a fresh UUID, so two runs cannot collide and the target
/// directory is always EMPTY. That is what makes dist-archive's overwrite prompt
/// (an uncaught PHP fatal under a non-TTY, measured) unreachable rather than
/// handled.
///
/// # Honest limit
///
/// `Drop` covers every way the SCOPE is left; it does not cover the process
/// dying under it. A SIGKILL or a hard crash mid-archive leaves one directory
/// behind, holding at most one run's work. Nothing reaps those today, and that
/// is a deliberate non-feature rather than an oversight: they sit at a known,
/// named path under app-data (`dist-archive-work/`), a build is seconds of work
/// so the window is small, and a reaper that deletes directories on a timer is a
/// larger risk than the litter it collects. Worth revisiting only if the path
/// ever becomes long-running.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    /// Create `<app-data>/dist-archive-work/<uuid>/{out,tmp}`.
    pub fn create(paths: &dyn crate::platform::traits::Paths) -> Result<Self> {
        let dir = paths
            .app_data_dir()?
            .join("dist-archive-work")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(dir.join("out"))?;
        std::fs::create_dir_all(dir.join("tmp"))?;
        Ok(Self(dir))
    }

    /// Where the archive is written.
    pub fn out_dir(&self) -> PathBuf {
        self.0.join("out")
    }

    /// The child's `TMPDIR`.
    pub fn tmp_dir(&self) -> PathBuf {
        self.0.join("tmp")
    }

    /// The root, for tests and messages.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            if e.kind() != std::io::ErrorKind::NotFound {
                // Never silent: a leak here grows by a copy of the plugin per
                // click, and the only symptom would be a disk filling up with
                // no one able to say what put it there.
                log::warn!("could not sweep {}: {e}", self.0.display());
            }
        }
    }
}

/// The whole build, start to finish: work in a scratch dir, hand the produced
/// archive to `deliver`, sweep whatever happened.
///
/// `deliver` exists so the archive leaves the scratch dir before it is removed,
/// and so this function does not need to know where it goes (Downloads, in
/// practice — task 5). It receives the produced file and returns the final
/// path.
///
/// Cancel is reported as an error rather than an empty success: a caller that
/// treated "no archive, exit fine" as a normal outcome would have no way to tell
/// it apart from a build that produced nothing for a reason worth showing.
#[allow(clippy::too_many_arguments)]
pub fn build_and_deliver(
    paths: &dyn crate::platform::traits::Paths,
    supervisor: &dyn ProcessSupervisor,
    php_bin: &Path,
    wp_phar: &Path,
    autoload: &Path,
    packages_dir: &Path,
    source: &Path,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
    deliver: &mut dyn FnMut(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    // Armed from here on: every `?` below sweeps.
    let scratch = ScratchDir::create(paths)?;
    let out_dir = scratch.out_dir();
    let tmp_dir = scratch.tmp_dir();
    let spec = ArchiveSpec {
        php_bin,
        wp_phar,
        autoload,
        packages_dir,
        tmp_dir: &tmp_dir,
        source,
        out_dir: &out_dir,
    };
    let result = run(supervisor, &spec, env, cancel, on_line)?;
    if result.cancelled {
        return Err(Error::Other("archive cancelled".into()));
    }
    if !result.ok {
        return Err(Error::Other(format!(
            "wp dist-archive failed (exit {:?}): {}",
            result.exit,
            result.tail.join(" / ")
        )));
    }
    let archive = sole_archive(&out_dir)?;
    deliver(&archive)
}

/// The user's Downloads folder.
///
/// A third copy of the four lines `logs::downloads_dir` and
/// `database::export_to_downloads` already carry. Left duplicated rather than
/// extracted mid-feature — the fact is one `UserDirs` call with nothing to
/// drift — but three is the number at which that stops being true, so it is
/// filed as a nit in `docs/TODO.md`.
fn downloads_dir() -> Result<PathBuf> {
    crate::core::downloads::user_downloads_dir()
}

/// Move the finished archive into Downloads, **never overwriting** — the same
/// convention as the database export and the log downloads: `name.zip`, then
/// `name-1.zip`, `name-2.zip`.
///
/// # Never overwriting is enforced by the filesystem, not by a check
///
/// The obvious shape — pick a name nothing occupies, then `rename` onto it —
/// leaves a window between the check and the write, and `rename` on Unix
/// replaces the destination **silently**. So "never overwrites" would rest on a
/// race being unlikely. `hard_link` fails with `AlreadyExists` instead, which
/// makes the reservation and the exclusion the same operation: if the name was
/// taken between the loop and the call, the link fails and the loop advances.
/// The link is to the scratch file, so when the scratch dir is swept moments
/// later the data stays and the user is left with an ordinary file.
///
/// The fallback covers the one case a link cannot: a Downloads folder on a
/// different volume (`EXDEV`). There the copy is opened with `create_new`, which
/// is the same exclusive guarantee, and a partial copy is removed rather than
/// left looking like a finished archive — the lesson `export_to_downloads`
/// already carries, where a truncated dump is worse than no dump.
///
/// # Why the overwrite prompt never needs handling
///
/// This is also the reason the archive is BUILT in a scratch dir rather than
/// straight into Downloads. dist-archive's own collision handling is an
/// interactive prompt that becomes an uncaught PHP fatal under a non-TTY
/// (measured; `PLAN-dist-archive.md` §1.4). Because the tool only ever writes
/// into an empty directory we just made, it can never reach that path — the
/// collision is ours to resolve, here, with a convention the user already knows
/// from every other file rexenv hands them.
pub fn deliver_to_downloads(archive: &Path) -> Result<PathBuf> {
    deliver_into(archive, &downloads_dir()?)
}

/// [`deliver_to_downloads`] with the destination supplied — the whole body, so
/// the tests exercise the real placement logic instead of a re-implementation of
/// it. Nothing in a lib test may write to the user's actual Downloads folder.
pub(crate) fn deliver_into(archive: &Path, dir: &Path) -> Result<PathBuf> {
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| Error::Other(format!("{} has no file name", archive.display())))?;
    let ext = archive.extension().map(|e| e.to_string_lossy().into_owned());
    let named = |n: u32| -> PathBuf {
        let base = if n == 0 { stem.clone() } else { format!("{stem}-{n}") };
        match &ext {
            Some(e) => dir.join(format!("{base}.{e}")),
            None => dir.join(base),
        }
    };

    for n in 0..1000 {
        let dest = named(n);
        match std::fs::hard_link(archive, &dest) {
            Ok(()) => return Ok(dest),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // Cross-volume, or a filesystem without links: copy, exclusively.
            Err(_) => return copy_exclusive(archive, dir, &named),
        }
    }
    Err(Error::Other(format!(
        "there are already 1000 copies of {stem} in {} — tidy some away first",
        dir.display()
    )))
}

/// The `EXDEV` path: `create_new` reserves the name atomically, and a failed
/// copy takes its own partial file with it.
fn copy_exclusive(
    archive: &Path,
    dir: &Path,
    named: &dyn Fn(u32) -> PathBuf,
) -> Result<PathBuf> {
    for n in 0..1000 {
        let dest = named(n);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&dest) {
            Ok(_) => {
                if let Err(e) = std::fs::copy(archive, &dest) {
                    let _ = std::fs::remove_file(&dest);
                    return Err(e.into());
                }
                return Ok(dest);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::Other(format!("could not find a free name in {}", dir.display())))
}

/// Did dist-archive find a version to put in the name?
///
/// It names the file `{dir}.{version}.zip`, and `{dir}.zip` when no version is
/// discoverable from `style.css`, the plugin header or `composer.json` — with no
/// warning and exit 0. The UI says so quietly rather than failing: shipping an
/// unversioned zip is a real thing to do, but silently handing someone
/// `my-plugin.zip` when they expected `my-plugin.1.2.3.zip` is the kind of
/// surprise found later, at the worst moment.
///
/// The knowledge lives here because the naming rule is this module's, not the
/// UI's.
pub fn version_missing_from_name(archive: &Path, canonical_source: &Path) -> bool {
    match (archive.file_stem(), canonical_source.file_name()) {
        (Some(stem), Some(dir)) => stem == dir,
        _ => false,
    }
}

/// The one file dist-archive produced. Scans `out/`, which the tool's leftovers
/// never reach (they go to `tmp/`), so "exactly one file" is a real invariant
/// and not a hopeful filter.
fn sole_archive(out_dir: &Path) -> Result<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(out_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    match found.len() {
        1 => Ok(found.remove(0)),
        // The measured shape of a "successful" run that made nothing: with an
        // occupied target dist-archive prints Skipping and exits 0. Our target
        // is always empty so this should be unreachable — which is exactly why
        // it must not be silent if it ever happens.
        0 => Err(Error::Other(
            "wp dist-archive reported success but produced no archive".into(),
        )),
        n => Err(Error::Other(format!(
            "wp dist-archive produced {n} files where one was expected: {found:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        real: PathBuf,
        link: PathBuf,
    }

    fn fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("rexenv-distarch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // Production shape: the checkout lives in the user's own tree, and the
        // wp-content entry is a SYMLINK to it — the linked-site case, which is
        // the one the target rules are really about.
        let real = root.join("code/my-awesome-plugin");
        let plugins = root.join("Sites/site.rex/wp-content/plugins");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&plugins).unwrap();
        let link = plugins.join("awesome-slug");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        // Archivable by default: the target rules are what these tests are
        // about, and a missing .distignore would refuse every one of them for
        // the wrong reason — a fixture that passes a test by failing earlier
        // proves nothing about the rule under test.
        std::fs::write(real.join(".distignore"), ".git\nnode_modules\n").unwrap();
        Fixture { root, real, link }
    }

    fn spec<'a>(source: &'a Path, out: &'a Path, paths: &'a [PathBuf]) -> ArchiveSpec<'a> {
        ArchiveSpec {
            php_bin: &paths[0],
            wp_phar: &paths[1],
            autoload: &paths[2],
            packages_dir: &paths[3],
            tmp_dir: &paths[4],
            source,
            out_dir: out,
        }
    }

    fn stub_paths() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/bin/php"),
            PathBuf::from("/bin/wp-cli.phar"),
            PathBuf::from("/data/wp-packages/dist-archive-3.1.0/autoload.php"),
            PathBuf::from("/data/wp-packages/none"),
            PathBuf::from("/data/dist-archive-work/x/tmp"),
        ]
    }

    #[test]
    fn the_target_can_never_be_the_place_dist_archive_would_have_chosen() {
        // The rule the module exists for. Every one of these is a directory the
        // tool itself would write to if we passed no target — and the last two
        // are only recognisable AFTER resolving the symlink, which is why the
        // guard canonicalises. A guard that compared the path as given would
        // pass here and drop a zip in the user's repo.
        let f = fixture("target");
        let p = stub_paths();
        let ours = f.root.join("tmp/out");

        assert!(argv(&spec(&f.link, &ours, &p)).is_ok(), "our own temp dir must be fine");

        for forbidden in [
            f.real.clone(),                       // the checkout itself
            f.real.join("build"),                 // inside the checkout
            f.real.parent().unwrap().to_path_buf(), // ~/code — the tool's default, via the LINK
            f.link.parent().unwrap().to_path_buf(), // wp-content/plugins — the default when cloned
        ] {
            let err = argv(&spec(&f.link, &forbidden, &p))
                .expect_err(&format!("{} was accepted as a target", forbidden.display()));
            assert!(
                err.to_string().contains("refusing"),
                "unhelpful refusal for {}: {err}",
                forbidden.display()
            );
        }
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_target_that_climbs_back_into_the_checkout_is_still_refused() {
        // `starts_with` is lexical, so an un-normalised `../` could walk back in
        // under a prefix that looks unrelated. Normalise first, then compare.
        let f = fixture("climb");
        let p = stub_paths();
        let sneaky = f.root.join("tmp/../code/my-awesome-plugin/dist");
        assert!(
            argv(&spec(&f.link, &sneaky, &p)).is_err(),
            "a target that normalises back into the checkout was accepted"
        );
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn the_spawned_argv_carries_the_require_and_the_resolved_source() {
        let f = fixture("argv");
        let p = stub_paths();
        let out = f.root.join("tmp/out");
        let args = argv(&spec(&f.link, &out, &p)).unwrap();

        // --require must precede the subcommand: it is what makes the command
        // exist, and wp-cli parses global parameters before dispatch.
        let require_at = args.iter().position(|a| a.starts_with("--require=")).unwrap();
        let cmd_at = args.iter().position(|a| a == "dist-archive").unwrap();
        assert!(require_at < cmd_at, "--require after the subcommand: {args:?}");
        assert!(
            args[require_at].ends_with("autoload.php"),
            "--require does not point at the vendored autoloader: {args:?}"
        );

        // The source handed over is the RESOLVED directory. dist-archive
        // canonicalises anyway; passing it resolved keeps our guard and the
        // tool's behaviour reading the same path.
        assert_eq!(args[cmd_at + 1], f.real.canonicalize().unwrap().display().to_string());
        assert_eq!(args[cmd_at + 2], resolve_for_compare(&out).display().to_string());

        // The belt against a prompt that is a fatal error under a non-TTY.
        assert!(args.contains(&"--force".to_string()), "{args:?}");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn the_packages_dir_override_is_applied_last_so_a_users_export_cannot_win() {
        // A developer with WP_CLI_PACKAGES_DIR in their shell profile would
        // otherwise reintroduce the very thing this excludes — and the login
        // shell env is where that value arrives from.
        let f = fixture("env");
        let p = stub_paths();
        let out = f.root.join("tmp/out");
        let s = spec(&f.link, &out, &p);
        let base = vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("WP_CLI_PACKAGES_DIR".to_string(), "/home/dev/.wp-cli/packages".to_string()),
        ];
        let mut full = base.clone();
        full.extend(env_overrides(&s));
        let last = full
            .iter()
            .filter(|(k, _)| k == "WP_CLI_PACKAGES_DIR")
            .next_back()
            .expect("override present");
        assert_eq!(last.1, p[3].display().to_string(), "the user's value won: {full:?}");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_missing_or_non_directory_source_is_refused_before_anything_spawns() {
        let f = fixture("missing");
        let p = stub_paths();
        let out = f.root.join("tmp/out");
        assert!(argv(&spec(&f.root.join("nope"), &out, &p)).is_err());

        let file = f.root.join("code/a-file.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(argv(&spec(&file, &out, &p)).is_err(), "a file was accepted as a checkout");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_checkout_without_a_distignore_never_reaches_a_spawn() {
        // The feature's point. Without the file the tool archives .git and
        // node_modules and reports Success — so this must fail BEFORE the
        // command runs, not be read back out of its output afterwards.
        let f = fixture("nodistignore");
        let p = stub_paths();
        let out = f.root.join("tmp/out");
        std::fs::remove_file(f.real.join(".distignore")).unwrap();

        let err = argv(&spec(&f.link, &out, &p))
            .expect_err("a checkout with no .distignore was archived")
            .to_string();

        // Actionable by someone who has never heard of .distignore: WHAT is
        // missing, WHERE to put it, WHY it matters, and something to paste.
        for must_say in [
            ".distignore",
            "node_modules",
            ".git",
            "does NOT fall back to .gitignore",
            "reports success",
        ] {
            assert!(err.contains(must_say), "the refusal never says `{must_say}`:\n{err}");
        }
        assert!(
            err.contains(&f.real.display().to_string()),
            "the refusal does not say WHERE to create the file:\n{err}"
        );
        for starter in DISTIGNORE_STARTER.lines() {
            assert!(err.contains(starter), "the starter file omits `{starter}`:\n{err}");
        }
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn the_distignore_is_looked_for_in_the_users_repository_not_beside_the_link() {
        // Same lesson as the target rules, on the other input: for a linked
        // asset the file lives in the user's own checkout. A check against the
        // symlink's directory would refuse a plugin that has one, and — worse —
        // accept one that only has a stray file in wp-content/plugins.
        let f = fixture("wherefile");
        let p = stub_paths();
        let out = f.root.join("tmp/out");
        std::fs::remove_file(f.real.join(".distignore")).unwrap();
        std::fs::write(f.link.parent().unwrap().join(".distignore"), ".git\n").unwrap();

        assert!(
            argv(&spec(&f.link, &out, &p)).is_err(),
            "a .distignore in wp-content/plugins was mistaken for the plugin's own"
        );
        assert!(!has_distignore(&f.link), "and the UI predicate agrees");

        std::fs::write(f.real.join(".distignore"), ".git\n").unwrap();
        assert!(argv(&spec(&f.link, &out, &p)).is_ok(), "the real one was not found");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn the_button_and_the_command_can_never_disagree_about_the_file() {
        // `has_distignore` decides whether the action is OFFERED and
        // `require_distignore` decides whether it RUNS. Two predicates for one
        // fact is the redundant-computation family: this pins them to the same
        // answer in every state, including the one where the path is gone.
        let f = fixture("agree");
        let p = stub_paths();
        let out = f.root.join("tmp/out");
        for present in [true, false, true] {
            if present {
                std::fs::write(f.real.join(".distignore"), ".git\n").unwrap();
            } else {
                let _ = std::fs::remove_file(f.real.join(".distignore"));
            }
            assert_eq!(
                has_distignore(&f.link),
                argv(&spec(&f.link, &out, &p)).is_ok(),
                "the offer and the run disagree with .distignore present={present}"
            );
        }
        assert!(!has_distignore(&f.root.join("gone")), "a missing path is not archivable");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn an_empty_distignore_is_accepted_and_that_is_a_decision() {
        // The stated limit, asserted so it stays a decision rather than becoming
        // an oversight someone quietly "fixes". Judging the CONTENT of the file
        // means deciding whether a pattern set excludes a given path — that is
        // gitignore-matching semantics, exactly the compatibility claim this
        // feature declined to own when it vendored the package instead of
        // reimplementing it. Absence is checkable without owning any semantics;
        // adequacy is not, and creating the file is the developer's explicit act.
        let f = fixture("empty");
        let p = stub_paths();
        let out = f.root.join("tmp/out");
        std::fs::write(f.real.join(".distignore"), "").unwrap();
        assert!(
            argv(&spec(&f.link, &out, &p)).is_ok(),
            "an empty .distignore was refused — that is a content judgement we do not make"
        );
        let _ = std::fs::remove_dir_all(&f.root);
    }

    /// Runs `/bin/sh -c <script>` in place of the real command, with the cwd and
    /// env `run` chose. Only the child's IDENTITY is faked — the process group,
    /// the pipes, the cancel path and the scratch-dir lifecycle are all the
    /// production ones, which is what these tests are about. The script reaches
    /// the target as `$TMPDIR/../out`, so it also proves `TMPDIR` really is set.
    struct FakeSupervisor {
        script: String,
        seen_tmpdir: std::sync::Mutex<Option<PathBuf>>,
    }

    impl FakeSupervisor {
        fn new(script: &str) -> Self {
            Self { script: script.into(), seen_tmpdir: std::sync::Mutex::new(None) }
        }
        fn scratch(&self) -> PathBuf {
            self.seen_tmpdir
                .lock()
                .unwrap()
                .clone()
                .expect("the child was never spawned")
                .parent()
                .expect("tmp has a parent")
                .to_path_buf()
        }
    }

    impl ProcessSupervisor for FakeSupervisor {
        fn spawn_streamed(
            &self,
            _program: &Path,
            _args: &[String],
            cwd: &Path,
            env: &[(String, String)],
        ) -> Result<std::process::Child> {
            use std::os::unix::process::CommandExt;
            let tmp = env
                .iter()
                .filter(|(k, _)| k == "TMPDIR")
                .next_back()
                .map(|(_, v)| PathBuf::from(v))
                .expect("TMPDIR must be set for the child");
            *self.seen_tmpdir.lock().unwrap() = Some(tmp);
            Ok(std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(&self.script)
                .current_dir(cwd)
                .env_clear()
                .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .process_group(0)
                .spawn()?)
        }
        fn spawn(&self, _p: &Path, _a: &[String]) -> Result<std::process::Child> {
            unimplemented!("the archive path only ever uses spawn_streamed")
        }
        fn spawn_logged(&self, _p: &Path, _a: &[String], _l: &Path) -> Result<std::process::Child> {
            unimplemented!("the archive path only ever uses spawn_streamed")
        }
        fn stop(&self, pid: u32) -> Result<()> {
            self.stop_group(pid)
        }
        fn stop_group(&self, pgid: u32) -> Result<()> {
            // The real group kill, via /bin/kill — `platform::macos` is private
            // and this test needs the grandchildren to die, not just the shell.
            let _ = std::process::Command::new("/bin/kill")
                .arg("-KILL")
                .arg(format!("-{pgid}"))
                .status();
            Ok(())
        }
    }

    struct TmpPaths(PathBuf);
    impl crate::platform::traits::Paths for TmpPaths {
        fn app_data_dir(&self) -> Result<PathBuf> {
            Ok(self.0.clone())
        }
        fn config_dir(&self) -> Result<PathBuf> {
            Ok(self.0.join("config"))
        }
        fn log_dir(&self) -> Result<PathBuf> {
            Ok(self.0.join("logs"))
        }
        fn bin_dir(&self) -> Result<PathBuf> {
            Ok(self.0.join("bin"))
        }
        fn hosts_file(&self) -> PathBuf {
            self.0.join("hosts")
        }
    }

    /// Litter exactly as dist-archive does — a directory in `$TMPDIR` that
    /// nothing ever removes — then behave as `outcome` says.
    fn script_for(outcome: &str) -> String {
        let litter = r##"mkdir -p "$TMPDIR/my-plugin.zip6a72" && echo copy > "$TMPDIR/my-plugin.zip6a72/big.bin";"##;
        match outcome {
            "ok" => format!(
                r##"{litter} echo PK > "$TMPDIR/../out/my-plugin.1.2.3.zip"; echo Success"##
            ),
            "failed" => format!(r##"{litter} echo failed >&2; exit 1"##),
            "cancelled" => format!(r##"{litter} sleep 300; :"##),
            other => panic!("unknown outcome {other}"),
        }
    }

    /// Only `stop_group` is needed to cancel from another thread.
    struct Killer;
    impl ProcessSupervisor for Killer {
        fn spawn(&self, _p: &Path, _a: &[String]) -> Result<std::process::Child> {
            unimplemented!()
        }
        fn spawn_logged(&self, _p: &Path, _a: &[String], _l: &Path) -> Result<std::process::Child> {
            unimplemented!()
        }
        fn stop(&self, pid: u32) -> Result<()> {
            self.stop_group(pid)
        }
        fn stop_group(&self, pgid: u32) -> Result<()> {
            let _ = std::process::Command::new("/bin/kill")
                .arg("-KILL")
                .arg(format!("-{pgid}"))
                .status();
            Ok(())
        }
    }

    fn run_build(outcome: &str, f: &Fixture) -> (Result<PathBuf>, PathBuf, PathBuf) {
        let paths = TmpPaths(f.root.join("app-data"));
        let sup = FakeSupervisor::new(&script_for(outcome));
        let cancel = CancelToken::new();
        let delivered = f.root.join("Downloads/my-plugin.1.2.3.zip");
        std::fs::create_dir_all(delivered.parent().unwrap()).unwrap();

        let go = |cancel: &CancelToken| {
            let mut deliver = |src: &Path| -> Result<PathBuf> {
                std::fs::copy(src, &delivered)?;
                Ok(delivered.clone())
            };
            build_and_deliver(
                &paths,
                &sup,
                Path::new("/bin/php"),
                Path::new("/bin/wp-cli.phar"),
                Path::new("/bin/autoload.php"),
                &f.root.join("packages"),
                &f.link,
                &[("PATH".to_string(), "/usr/bin:/bin".to_string())],
                cancel,
                &mut |_| {},
                &mut deliver,
            )
        };

        let out = if outcome == "cancelled" {
            // Cancel a RUNNING child, not a pending one. Pre-cancelling is a
            // real path but a different one: `run_step_streamed` returns before
            // it spawns, so nothing has littered yet and the interesting case —
            // a killed child that has already written into TMPDIR — never
            // happens. Found by this test failing with "the child was never
            // spawned", which is the fixture telling the truth about itself.
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    for _ in 0..500 {
                        if cancel.current_pgid().is_some() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    // Let the child get its litter written, so the sweep has
                    // something real to remove.
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    cancel.cancel(&Killer);
                });
                go(&cancel)
            })
        } else {
            go(&cancel)
        };
        (out, sup.scratch(), delivered)
    }

    #[test]
    fn the_scratch_dir_is_swept_on_all_three_exits_not_just_the_happy_one() {
        // The claim is about EVERY exit, so every exit is asserted. Cancel is
        // the one that matters most and the one a happy-path test would miss:
        // it is when dist-archive litters hardest, because an interrupted run
        // has usually just finished copying the plugin into its temp dir.
        for outcome in ["ok", "failed", "cancelled"] {
            let f = fixture(&format!("sweep-{outcome}"));
            let (result, scratch, delivered) = run_build(outcome, &f);

            assert!(
                !scratch.exists(),
                "`{outcome}` left the scratch dir behind: {}",
                scratch.display()
            );
            // And the tool's own litter went with it, which is the only reason
            // TMPDIR is redirected at all.
            assert!(
                !scratch.join("tmp/my-plugin.zip6a72").exists(),
                "`{outcome}` left the child's temp copy behind"
            );

            match outcome {
                "ok" => {
                    assert_eq!(result.unwrap(), delivered);
                    assert!(delivered.is_file(), "the archive did not survive the sweep");
                }
                _ => {
                    let e = result.expect_err(&format!("`{outcome}` reported success"));
                    assert!(!delivered.is_file(), "`{outcome}` delivered an archive anyway");
                    if outcome == "cancelled" {
                        assert!(e.to_string().contains("cancelled"), "{e}");
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&f.root);
        }
    }

    #[test]
    fn the_delivered_archive_outlives_the_sweep_because_deliver_runs_first() {
        // The ordering the whole design rests on: `deliver` is called while the
        // scratch dir is still alive, and the sweep happens when the guard
        // drops afterwards. Getting this backwards would delete the archive on
        // the way out and report success.
        let f = fixture("order");
        let (result, scratch, delivered) = run_build("ok", &f);
        assert_eq!(result.unwrap(), delivered);
        assert!(delivered.is_file());
        assert!(!scratch.exists());
        assert_eq!(std::fs::read_to_string(&delivered).unwrap().trim(), "PK");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_success_that_produced_nothing_is_an_error_not_an_empty_delivery() {
        // dist-archive exits 0 after printing "Skipping" when its target is
        // occupied. Ours never is — so if this ever fires something else is
        // wrong, and silence would hand the user a stale file or nothing at all.
        let f = fixture("empty-out");
        let empty = f.root.join("nothing");
        std::fs::create_dir_all(&empty).unwrap();
        let err = sole_archive(&empty).expect_err("an empty out dir passed").to_string();
        assert!(err.contains("produced no archive"), "{err}");

        std::fs::write(empty.join("a.zip"), "x").unwrap();
        std::fs::write(empty.join("b.zip"), "x").unwrap();
        assert!(sole_archive(&empty).is_err(), "two archives passed as one");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn downloads_are_numbered_on_collision_and_never_overwrite() {
        // The same convention the database export and the log downloads use, so
        // a user meets one rule for every file rexenv hands them.
        let dir = std::env::temp_dir().join(format!("rexenv-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.bin");

        let mut landed = Vec::new();
        for i in 0..3 {
            std::fs::write(&src, format!("build-{i}")).unwrap();
            // The archive keeps its produced name; only the destination moves.
            let staged = dir.join("stage");
            std::fs::create_dir_all(&staged).unwrap();
            let archive = staged.join("my-plugin.1.2.3.zip");
            let _ = std::fs::remove_file(&archive);
            std::fs::copy(&src, &archive).unwrap();
            landed.push(deliver_into(&archive, &dir).unwrap());
        }
        assert_eq!(
            landed
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["my-plugin.1.2.3.zip", "my-plugin.1.2.3-1.zip", "my-plugin.1.2.3-2.zip"],
        );
        // Nothing was overwritten: each landing still holds its own build.
        for (i, p) in landed.iter().enumerate() {
            assert_eq!(std::fs::read_to_string(p).unwrap(), format!("build-{i}"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_exclusion_comes_from_the_write_itself_not_from_a_look_beforehand() {
        // This test earns its place by DISCRIMINATING. The first version
        // occupied the name with an ordinary file, which "check `exists()`,
        // then `rename`" survives just as well — so it asserted the claim
        // without testing it. Planting the racy shape passed, which is how that
        // was found.
        //
        // A DANGLING SYMLINK separates them. `Path::exists()` follows links, so
        // it answers *false* for a name that is unmistakably taken; `rename`
        // would then destroy it. `hard_link` gets EEXIST from the kernel and
        // moves on. The gap between what a check believes and what the
        // filesystem holds is the whole reason the reservation and the
        // exclusion have to be one operation.
        let dir = std::env::temp_dir().join(format!("rexenv-dlrace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("stage")).unwrap();
        let occupied = dir.join("my-plugin.1.2.3.zip");
        std::os::unix::fs::symlink(dir.join("no-such-target"), &occupied).unwrap();
        assert!(!occupied.exists(), "the fixture needs a name `exists()` denies");
        assert!(occupied.symlink_metadata().is_ok(), "but that is really there");

        let archive = dir.join("stage/my-plugin.1.2.3.zip");
        std::fs::write(&archive, "ours").unwrap();
        let landed = deliver_into(&archive, &dir).unwrap();

        assert_eq!(landed.file_name().unwrap(), "my-plugin.1.2.3-1.zip");
        assert!(
            occupied.symlink_metadata().is_ok(),
            "the occupied name was clobbered — placement looked before it wrote"
        );
        assert_eq!(std::fs::read_to_string(&landed).unwrap(), "ours");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ordinary_existing_file_of_the_same_name_survives_too() {
        let dir = std::env::temp_dir().join(format!("rexenv-dlkeep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("stage")).unwrap();
        std::fs::write(dir.join("my-plugin.1.2.3.zip"), "SOMEONE ELSE'S FILE").unwrap();
        let archive = dir.join("stage/my-plugin.1.2.3.zip");
        std::fs::write(&archive, "ours").unwrap();

        let landed = deliver_into(&archive, &dir).unwrap();
        assert_eq!(landed.file_name().unwrap(), "my-plugin.1.2.3-1.zip");
        assert_eq!(
            std::fs::read_to_string(dir.join("my-plugin.1.2.3.zip")).unwrap(),
            "SOMEONE ELSE'S FILE"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cross_volume_fallback_reserves_exclusively_and_leaves_no_partial() {
        // EXDEV: a Downloads folder on another volume can't take a hard link.
        // The copy path has to keep both guarantees — exclusive naming, and no
        // half-written file left looking like a finished archive.
        let dir = std::env::temp_dir().join(format!("rexenv-dlx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("stage")).unwrap();
        let archive = dir.join("stage/my-plugin.1.2.3.zip");
        std::fs::write(&archive, "PK-payload").unwrap();
        std::fs::write(dir.join("my-plugin.1.2.3.zip"), "existing").unwrap();

        let stem = "my-plugin.1.2.3".to_string();
        let named = |n: u32| -> PathBuf {
            let base = if n == 0 { stem.clone() } else { format!("{stem}-{n}") };
            dir.join(format!("{base}.zip"))
        };
        let landed = copy_exclusive(&archive, &dir, &named).unwrap();
        assert_eq!(landed.file_name().unwrap(), "my-plugin.1.2.3-1.zip");
        assert_eq!(std::fs::read_to_string(&landed).unwrap(), "PK-payload");
        assert_eq!(std::fs::read_to_string(dir.join("my-plugin.1.2.3.zip")).unwrap(), "existing");

        // A copy that fails must take its own placeholder with it, or the user
        // is left with an empty .zip that looks like a build.
        let gone = dir.join("stage/vanished.zip");
        let named2 = |n: u32| -> PathBuf {
            dir.join(if n == 0 { "vanished.zip".into() } else { format!("vanished-{n}.zip") })
        };
        assert!(copy_exclusive(&gone, &dir, &named2).is_err());
        assert!(
            !dir.join("vanished.zip").exists(),
            "a failed copy left its placeholder behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_versionless_archive_is_recognisable_from_its_name() {
        // dist-archive names it {dir}.{version}.zip, or {dir}.zip when no
        // version is discoverable — silently, exit 0. The UI says so quietly;
        // the rule for knowing lives here, where the naming knowledge is.
        let src = Path::new("/Users/dev/code/my-plugin");
        assert!(version_missing_from_name(Path::new("/out/my-plugin.zip"), src));
        assert!(!version_missing_from_name(Path::new("/out/my-plugin.1.2.3.zip"), src));
        assert!(!version_missing_from_name(Path::new("/out/my-plugin.0.zip"), src));
    }

    use crate::core::copy_scan::strip_ts_comments as strip_comments;

    #[test]
    fn the_archive_button_copy_says_what_the_feature_actually_does() {
        // The copy guard, in the module that owns the FACTS the copy states —
        // so when a rule here changes, the sentence that promises it is next to
        // the change rather than three files away. That placement is #197's
        // lesson: a careful ledger row does not immunise a careless paragraph
        // elsewhere.
        //
        // EROSION is the live risk, not regression. The strings are long, they
        // sit in a tooltip, and the obvious edit is to shorten them. Two
        // clauses are load-bearing and are the two a trim removes first:
        //
        //   - "Nothing is written into the checkout" — what makes the button
        //     safe to click on a folder the user cares about. dist-archive's
        //     own default writes BESIDE the source, and for a linked asset that
        //     is the user's repository (#230).
        //   - "would report that as a success" — the specific dishonesty, and
        //     therefore the reason the button REFUSES rather than warns (#231).
        //     "silently succeed" or "may include unwanted files" would both
        //     pass a vaguer guard while dropping the point.
        const PANEL_SRC: &str = include_str!("../../../src/components/wordpress/RepoPanel.tsx");
        // Scan what RENDERS, not what the file says about itself.
        //
        // The first version scanned the whole file and PASSED with both
        // load-bearing clauses deleted from the tooltip — because the comment
        // above those constants quotes them while explaining that they are
        // load-bearing. The guard was reading its own explanation. That is this
        // project's own defect family, committed inside the guard written to
        // prevent it: the claim is about what the USER SEES, and the check read
        // a superset that includes prose merely mentioning the words.
        let panel = strip_comments(PANEL_SRC);
        let panel = panel.as_str();
        assert!(
            panel.contains("const ARCHIVE_TITLE"),
            "the comment stripper ate the source — every check below would pass \
             on an empty string"
        );
        assert!(
            !panel.contains("LOAD-BEARING"),
            "comment text survived the strip; the guard can be satisfied by prose again"
        );
        const MUST_SAY: &[(&str, &str)] = &[
            ("Nothing is written into the checkout", "LOAD-BEARING: why it is safe on a linked asset"),
            ("would report that as a success", "LOAD-BEARING: the specific dishonesty the refusal exists for"),
            (".distignore", "the file the whole feature turns on, named"),
            (".git and node_modules", "what a zip without one would actually contain"),
            (
                "Add a .distignore file at the top of this checkout",
                "WHERE to put it — 'add one' leaves the location a guess for the reader who needs this most",
            ),
            (".gitignore syntax", "how to write it, for someone meeting the file for the first time"),
            ("saved to Downloads", "where the result went"),
            (
                "no version found in the plugin header, style.css or composer.json",
                "the quiet note: an unversioned name is legitimate, discovering it after upload is not",
            ),
        ];
        for (phrase, why) in MUST_SAY {
            assert!(
                panel.contains(phrase),
                "the archive copy no longer says `{phrase}` — {why}.\n\
                 If the wording genuinely changed, change it HERE too and say why; \
                 do not delete the row to make the build pass."
            );
        }
        // And the button is offered on the same fact the command enforces. A
        // hard-coded `true`, or a second local check, would put the offer and
        // the run back in disagreement — the thing #231 pins.
        assert!(
            panel.contains("!s.hasDistignore"),
            "the archive button no longer gates on the recorded .distignore fact"
        );
    }

    #[test]
    fn only_one_place_in_this_module_builds_a_dist_archive_argv() {
        // The drift guard behind "a guard that reads the argv reads what runs".
        // If a second call site ever assembles its own arguments, every test
        // above keeps passing while the spawned command stops matching them.
        let src = include_str!("dist_archive.rs");
        let production: String = src
            .split("#[cfg(test)]")
            .next()
            .expect("source has a production half")
            .to_string();
        let occurrences = production.matches("\"dist-archive\"").count();
        assert_eq!(
            occurrences, 1,
            "expected exactly one `\"dist-archive\"` literal in the production half \
             (the argv builder); found {occurrences}. A second one means a second argv."
        );
    }
}

