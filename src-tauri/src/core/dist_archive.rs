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
//! Not for the reason in ledger #228 — that is a global posture question and an
//! owner decision. This is narrower and settles here: a user may have their own
//! `dist-archive-command` installed globally at a different version, and the
//! behaviour this feature is built around is **version-specific** (`.distignore`
//! semantics, the missing `.gitignore` fallback, the filename format). Measured
//! 4 Aug 2026: with a global v2.0.1 present and our `--require` given, **ours
//! wins** — but that is undocumented precedence between `--require` and package
//! autoloading, exactly the "posture resting on a third party's behaviour" shape
//! this project has been burned by. Pointing the packages dir at an empty
//! rexenv-owned directory costs one environment variable and makes precedence
//! irrelevant. It affects only this spawn: a user's global packages still work
//! everywhere else rexenv runs `wp`.
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

    Ok(vec![
        "-d".into(),
        "memory_limit=512M".into(),
        spec.wp_phar.display().to_string(),
        // Before the subcommand: this is what makes `dist-archive` exist at all.
        format!("--require={}", spec.autoload.display()),
        "dist-archive".into(),
        source.display().to_string(),
        out.display().to_string(),
        "--force".into(),
    ])
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
    vec![(
        "WP_CLI_PACKAGES_DIR".to_string(),
        spec.packages_dir.display().to_string(),
    )]
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

