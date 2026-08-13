//! core::wp_packages — the WP-CLI commands rexenv chose, carried rather than resolved.
//!
//! # Why this module exists at all
//!
//! `wp dist-archive` is not core WP-CLI. It is `wp-cli/dist-archive-command`, a
//! separate composer package, and the bundled phar does not contain it — which
//! is easy to disbelieve, because on a machine that has ever run
//! `wp package install` the command answers perfectly well. It answers from
//! `~/.wp-cli/packages/`, a directory rexenv does not own and does not pin.
//! That is how this feature was nearly built on a package installed on one
//! laptop in December 2021. Since 13 Aug 2026 no `wp` rexenv runs for a user
//! reads that directory at all ([`neutral_packages_path`], ledger #228) — but
//! the rule below is what made the capability real, and it is what makes the
//! pin affordable: nothing rexenv needs went away when the inheritance did.
//!
//! So the rule this module implements: **a command rexenv depends on is one
//! rexenv carries.** The tree is vendored (`resources/wp-dist-archive`, built by
//! `scripts/build-wp-dist-archive.sh`), compiled into the binary, materialised
//! under app-data on first use, and handed to `php` as an explicit
//! `--require`. Nothing about whether it is available depends on the machine.
//!
//! # What is pinned, and how a bump is done
//!
//! [`DIST_ARCHIVE_VERSION`] is the pin, and it is checked against the tree
//! itself — `the_vendored_tree_is_the_version_we_pinned` reads
//! `vendor/composer/installed.json` out of the EMBEDDED bytes, so a tree
//! regenerated at a different version fails the build instead of quietly
//! changing behaviour. `composer.lock` beside the tree is the provenance record
//! (exact versions, dist references, licences); the bump procedure is in the
//! build script's header, including the constraint that bit us: v3.2.0 requires
//! `wp-cli/wp-cli ^2.13` and so cannot be used against our pinned 2.12.0 phar.
//!
//! # Materialising, and why it is version-stamped
//!
//! The destination carries the version (`wp-packages/dist-archive-<version>/`),
//! so an upgraded rexenv can never `--require` a previous version's autoloader
//! left on disk. The marker file is written LAST: a run interrupted halfway
//! leaves no marker, so the next call rewrites rather than loading a half-tree.

use crate::error::{Error, Result};
use crate::platform::traits::Paths;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/wp_dist_archive_files.rs"));

/// The pinned `wp-cli/dist-archive-command` version. Must match the vendored
/// tree — asserted by a test, not by care.
pub const DIST_ARCHIVE_VERSION: &str = "3.1.0";

/// Written last, so its presence means "the whole tree is here".
const MARKER: &str = ".rexenv-complete";

/// Where this version's tree lives once materialised.
fn tree_dir(paths: &dyn Paths) -> Result<PathBuf> {
    Ok(paths
        .app_data_dir()?
        .join("wp-packages")
        .join(format!("dist-archive-{DIST_ARCHIVE_VERSION}")))
}

/// An empty, rexenv-owned directory to point `WP_CLI_PACKAGES_DIR` at.
///
/// Named rather than built inline at the call site, because "an empty
/// directory" is the whole mechanism: it is what makes a user's own globally
/// installed packages irrelevant to a command rexenv chose the version of. It
/// stays empty by never being written to — nothing here creates anything
/// inside it.
///
/// Predates [`neutral_packages_path`] and is kept because `dist-archive`
/// already threads it (#230, shipped and proven). Both satisfy the same rule —
/// no `vendor/autoload.php` can appear inside — by different means; the newer
/// one is the stronger of the two and is what a new spawn site should use.
pub fn empty_packages_dir(paths: &dyn Paths) -> Result<PathBuf> {
    let dir = paths.app_data_dir()?.join("wp-packages").join("none");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The name of the file beside the phar that every rexenv-run `wp` has its
/// packages dir pointed at. Named for what it is, so the environment of a
/// spawned child reads as deliberate rather than as a bug (#228).
pub const NEUTRAL_PACKAGES_FILE: &str = ".rexenv-no-wp-packages";

/// The `WP_CLI_PACKAGES_DIR` value that pins the command set: a **file**, beside
/// the phar it pins.
///
/// # Why a file and not an empty directory
///
/// WP-CLI loads `<packages dir>/vendor/autoload.php` when it is readable, so an
/// empty directory is neutral *today*. It is not neutral permanently: a
/// `wp package install` — which the MCP raw runner can be handed by an agent —
/// populates whatever directory it is pointed at, and the pin would then be
/// quietly gone, with every test still passing. A regular file cannot contain
/// `vendor/autoload.php` and cannot be turned into a directory that does, so
/// the pin holds by the shape of the path rather than by nobody writing there.
/// Measured against the pinned phar (2.12.0, 13 Aug 2026): ordinary commands are
/// unaffected and silent (`--version`, `help plugin install`, `cli info`); a
/// package-provided command answers `not a registered wp command`;
/// `wp package list` refuses with `couldn't be created: mkdir(): File exists`,
/// which is the erosion path failing loudly instead of succeeding quietly.
///
/// # Why beside the phar
///
/// It is the one path every spawn site already holds. The alternative — passing
/// an app-data dir down — is ~145 call sites of signature churn to reach four
/// spawns, and a variable threaded through 145 places is a variable that can be
/// dropped at one of them.
///
/// Creating the file is best-effort: if the write fails the path still does not
/// resolve to a packages dir, so the pin degrades to "neutral but erodable"
/// rather than to "inherits the user's packages".
pub fn neutral_packages_path(wp_phar: &Path) -> PathBuf {
    let path = wp_phar.with_file_name(NEUTRAL_PACKAGES_FILE);
    // `create_new` — never truncate, and no extra stat on the common path.
    let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(&path);
    path
}

/// The one environment pair that pins the command set of a rexenv-run `wp`.
pub fn pin_packages_env(wp_phar: &Path) -> (String, String) {
    (
        "WP_CLI_PACKAGES_DIR".to_string(),
        neutral_packages_path(wp_phar).display().to_string(),
    )
}

/// A streamed spawn's environment with the command set pinned.
///
/// Any inbound `WP_CLI_PACKAGES_DIR` is **removed**, then ours is appended —
/// so the pin does not depend on the child applying a later entry over an
/// earlier one. A spawn env built in a different order by a future refactor
/// would otherwise undo the pin silently, with every other test still green.
/// The login-shell snapshot these are layered onto is the user's real
/// environment, so an exported value is the ordinary case, not a corner.
pub fn with_pinned_packages(env: &[(String, String)], wp_phar: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        env.iter().filter(|(k, _)| k != "WP_CLI_PACKAGES_DIR").cloned().collect();
    out.push(pin_packages_env(wp_phar));
    out
}

// ── The tell ────────────────────────────────────────────────────────────────
//
// Pinning alone hands someone who genuinely relies on a global package a bare
// `not a registered wp command` with no explanation — the same unreproducibility
// pointed the other way. So when a packages dir exists that WOULD have
// contributed, rexenv says so.

/// A packages dir on this machine that WP-CLI would have loaded.
pub struct GlobalPackages {
    /// The directory itself, `~`-abbreviated for display.
    pub dir: String,
    /// What the user installed there, from `composer.json`'s `require`.
    ///
    /// **Empty means "could not be named confidently", never "none".** The
    /// caller must then say the directory exists WITHOUT claiming a count: a
    /// card reading "the 0 packages" or listing nothing would be worse than not
    /// rendering, because it invites the reader to conclude something false
    /// about their own machine. Guessing is the one thing this must not do.
    pub names: Vec<String>,
}

/// Detect a packages dir that would have extended a rexenv-run `wp`.
///
/// `packages_dir_env` is the caller's `WP_CLI_PACKAGES_DIR` — the LOGIN-SHELL
/// one where the caller has it, since that is the environment the streamed
/// spawns inherit and the only place a user's export is visible to a
/// Finder-launched app. `home` mirrors WP-CLI's own `get_home_dir()`.
///
/// The test for "would have contributed" is `vendor/autoload.php` being a
/// readable file, because that is exactly what WP-CLI requires — not the
/// directory existing, which it does on plenty of machines that never installed
/// anything.
pub fn global_packages(packages_dir_env: Option<&str>, home: Option<&str>) -> Option<GlobalPackages> {
    let dir = match packages_dir_env.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => PathBuf::from(v),
        None => PathBuf::from(home?).join(".wp-cli").join("packages"),
    };
    if !dir.join("vendor").join("autoload.php").is_file() {
        return None;
    }
    Some(GlobalPackages { dir: abbreviate_home(&dir, home), names: installed_names(&dir) })
}

/// `require` keys from the packages dir's own `composer.json` — what
/// `wp package install` writes, so it is the list the user recognises.
///
/// Every failure returns EMPTY rather than a guess: unreadable, not JSON, no
/// `require`, not an object. `php` and the `wp-cli/wp-cli` self-reference are
/// dropped — they are not packages anyone installed.
fn installed_names(dir: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(dir.join("composer.json")) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let Some(require) = json.get("require").and_then(|r| r.as_object()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = require
        .keys()
        .filter(|k| *k != "php" && *k != "wp-cli/wp-cli")
        .cloned()
        .collect();
    names.sort();
    names
}

fn abbreviate_home(dir: &Path, home: Option<&str>) -> String {
    let shown = dir.display().to_string();
    match home.filter(|h| !h.is_empty()) {
        Some(h) if shown.starts_with(h) => format!("~{}", &shown[h.len()..]),
        _ => shown,
    }
}

/// The phrase WP-CLI prints when a command does not exist. Its own words, so
/// the match is against what the phar really says.
const NO_SUCH_COMMAND: &str = "is not a registered wp command";

/// [`global_packages`] resolved from this process's own environment.
///
/// The app is Finder-launched, so `WP_CLI_PACKAGES_DIR` is usually absent here
/// and the answer is `~/.wp-cli/packages` — right on the overwhelming majority
/// of machines. A caller holding the login-shell snapshot (the environment the
/// streamed spawns actually inherit) should pass its value to
/// [`global_packages`] instead, and the Settings card does.
pub fn global_packages_here() -> Option<GlobalPackages> {
    global_packages(
        std::env::var("WP_CLI_PACKAGES_DIR").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// [`explain_missing_command`] against this machine, with the lookup skipped
/// unless the failure is the one it explains — every other wp-cli failure must
/// cost nothing.
pub fn explain_missing_command_here(stderr: &str) -> Option<String> {
    if !stderr.contains(NO_SUCH_COMMAND) {
        return None;
    }
    explain_missing_command(stderr, global_packages_here().as_ref())
}

/// The explanation appended to a wp-cli failure that says the command does not
/// exist, when this machine has a packages dir that would have supplied it.
///
/// APPENDED, never substituted: WP-CLI's own line is what the user will paste
/// into a search box, and replacing it with something friendlier would cost them
/// the one string that finds an answer. `None` when the failure is anything else
/// or nothing would have contributed — this must be silent on the machines it
/// has nothing to explain.
pub fn explain_missing_command(stderr: &str, global: Option<&GlobalPackages>) -> Option<String> {
    if !stderr.contains(NO_SUCH_COMMAND) {
        return None;
    }
    let global = global?;
    let named = match global.names.is_empty() {
        true => String::new(),
        false => format!(" ({})", global.names.join(", ")),
    };
    Some(format!(
        "This is not one of the commands rexenv bundles, and rexenv runs `wp` without your {}{}. \
         That is why it resolves in your own shell and in rexenv's terminal, but not in what \
         rexenv runs for you.",
        global.dir, named
    ))
}

/// Materialise the vendored tree (idempotent) and return the path to hand to
/// `wp --require=…`.
///
/// Cheap on the common path: one `is_file` on the marker.
pub fn ensure_dist_archive(paths: &dyn Paths) -> Result<PathBuf> {
    let dir = tree_dir(paths)?;
    let autoload = dir.join("autoload.php");
    if dir.join(MARKER).is_file() && autoload.is_file() {
        return Ok(autoload);
    }
    // A previous partial write (no marker) is replaced wholesale rather than
    // merged into — a half-tree that gained the missing files later would still
    // be a mix of two versions.
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    for (rel, bytes) in FILES {
        let dest = safe_join(&dir, rel)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, bytes)?;
    }
    if !autoload.is_file() {
        return Err(Error::Other(format!(
            "the vendored dist-archive tree has no autoload.php ({} files written to {})",
            FILES.len(),
            dir.display()
        )));
    }
    std::fs::write(dir.join(MARKER), DIST_ARCHIVE_VERSION)?;
    Ok(autoload)
}

/// Join a generated relative path under `base`, refusing anything that climbs
/// out. These paths come from our own build script, so this is defense in depth
/// — the same layer `wp_mailtag::validate_domain` sits at, and worth the four
/// lines for the same reason: the cost of being wrong is writing outside
/// app-data.
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf> {
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.split('/').any(|seg| seg == ".." || seg == "." || seg.is_empty())
    {
        return Err(Error::Other(format!("refusing to write vendored path: {rel}")));
    }
    Ok(base.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read a package's version out of composer's own installed.json, which
    /// ships INSIDE the tree — so this reads the artefact, never a second copy
    /// of the number that could agree with the constant while the files differ.
    fn version_in_tree(package: &str) -> Option<String> {
        let (_, bytes) = FILES.iter().find(|(rel, _)| *rel == "composer/installed.json")?;
        let json: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        let packages = json.get("packages")?.as_array()?;
        let entry = packages
            .iter()
            .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(package))?;
        Some(entry.get("version_normalized").or_else(|| entry.get("version"))?.as_str()?.to_string())
    }

    #[test]
    fn the_vendored_tree_is_the_version_we_pinned() {
        // The point of the pin. A regenerated tree at a different version is a
        // BUILD FAILURE, not a behaviour change discovered later by someone
        // whose zip suddenly contains different files. `.distignore` semantics
        // are the product here, and they are the package's, not ours.
        let found = version_in_tree("wp-cli/dist-archive-command")
            .expect("the vendored tree must carry composer/installed.json");
        assert!(
            found.starts_with(DIST_ARCHIVE_VERSION),
            "vendored dist-archive is {found}, but DIST_ARCHIVE_VERSION says \
             {DIST_ARCHIVE_VERSION}. Regenerate with scripts/build-wp-dist-archive.sh \
             or update the constant — and move the THIRD-PARTY-NOTICES rows with it."
        );
    }

    #[test]
    fn the_embedded_tree_is_actually_there() {
        // The coverage canary. Every assertion in this module is over `FILES`,
        // and a codegen bug that produced an EMPTY list would leave all of them
        // vacuously true — the shape that has passed while covering nothing
        // five times in this project. So assert the tree's own landmarks.
        assert!(
            FILES.len() > 40,
            "only {} embedded files — the build.rs codegen produced a stub, and \
             every other check in this module would pass on it",
            FILES.len()
        );
        for required in [
            "autoload.php",
            "composer/installed.json",
            "wp-cli/dist-archive-command/dist-archive-command.php",
            "wp-cli/dist-archive-command/src/Dist_Archive_Command.php",
            "inmarelibero/gitignore-checker/src/GitIgnoreChecker.php",
        ] {
            assert!(
                FILES.iter().any(|(rel, _)| rel == &required),
                "the vendored tree is missing {required}"
            );
        }
    }

    #[test]
    fn the_transitive_dependency_is_carried_too() {
        // dist-archive's `.distignore` matching IS inmarelibero/gitignore-checker
        // — a tree with the command and not the library would install, register,
        // and then fatal on the first archive. Carrying "the package" means
        // carrying what it runs on.
        assert!(
            version_in_tree("inmarelibero/gitignore-checker").is_some(),
            "the gitignore-checker library is not in the vendored tree"
        );
    }

    #[test]
    fn a_vendored_path_can_never_climb_out_of_app_data() {
        let base = Path::new("/tmp/rexenv-wp-packages");
        assert!(safe_join(base, "composer/autoload.php").is_ok());
        for bad in ["../escape.php", "a/../../b.php", "/etc/passwd", "", "./x.php", "a//b.php"] {
            assert!(safe_join(base, bad).is_err(), "`{bad}` was accepted");
        }
    }

    /// Local throwaway app-data root — the house pattern (see `service_manager`'s
    /// `TestPaths`). Everything this module writes lands under it, so the test
    /// can never touch real app data.
    struct TmpPaths(PathBuf);
    impl Paths for TmpPaths {
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

    #[test]
    fn materialising_is_idempotent_and_replaces_a_partial_tree() {
        let root = std::env::temp_dir().join(format!("rexenv-wppkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sandbox = TmpPaths(root.clone());
        let first = ensure_dist_archive(&sandbox).expect("first materialise");
        assert!(first.is_file(), "autoload.php not written");
        let stamp = std::fs::metadata(&first).unwrap().modified().unwrap();

        // Second call is a no-op — it must not rewrite 77 files on every run.
        let second = ensure_dist_archive(&sandbox).expect("second materialise");
        assert_eq!(first, second);
        assert_eq!(stamp, std::fs::metadata(&second).unwrap().modified().unwrap());

        // A tree without its marker is a PARTIAL write, and must be redone
        // rather than trusted — the file that is missing may be the one the
        // command needs.
        let dir = second.parent().unwrap().to_path_buf();
        std::fs::remove_file(dir.join(MARKER)).unwrap();
        std::fs::remove_file(dir.join("composer/installed.json")).unwrap();
        let third = ensure_dist_archive(&sandbox).expect("re-materialise");
        assert!(
            third.parent().unwrap().join("composer/installed.json").is_file(),
            "the missing file was not restored — a partial tree was trusted"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn phar_scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexenv-pin-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Production-shaped: the phar always arrives from `binaries::resolve_file`
        // as `<app-data>/bin/wp-cli-<version>/wp-cli.phar`.
        let phar_dir = dir.join("bin").join("wp-cli-2.12.0");
        std::fs::create_dir_all(&phar_dir).unwrap();
        let phar = phar_dir.join("wp-cli.phar");
        std::fs::write(&phar, b"#!/usr/bin/env php\n").unwrap();
        phar
    }

    fn sandbox_root(phar: &Path) -> PathBuf {
        phar.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
    }

    #[test]
    fn the_pinned_packages_path_can_never_become_a_packages_dir() {
        // The whole reason it is a FILE. An empty directory is neutral today
        // and populated by the first `wp package install` an agent runs through
        // the raw runner — after which the pin is gone and every test here
        // still passes.
        let phar = phar_scratch("file");
        let path = neutral_packages_path(&phar);
        assert_eq!(path.parent(), phar.parent(), "the pin must sit beside the phar it pins");
        assert!(path.is_file(), "the pinned path is not a file — an empty dir can be filled");
        assert!(
            std::fs::create_dir_all(path.join("vendor")).is_err(),
            "a packages tree could be created inside the pinned path — WP-CLI would then load \
             `vendor/autoload.php` from it and the command set would be unpinned again"
        );
        // Idempotent, and never truncating: a second call must not clobber.
        std::fs::write(&path, b"keep").unwrap();
        assert_eq!(neutral_packages_path(&phar), path);
        assert_eq!(std::fs::read(&path).unwrap(), b"keep", "the pin file was rewritten");
        let _ = std::fs::remove_dir_all(sandbox_root(&phar));
    }

    #[test]
    fn a_user_who_exports_the_variable_cannot_reintroduce_their_packages() {
        // The login-shell snapshot IS the user's environment, so this is the
        // ordinary case. Asserted as UNIQUENESS, not as ordering: the pin must
        // not depend on a later entry beating an earlier one, or a future
        // refactor that builds the env in a different order undoes it silently
        // while every other test stays green.
        let phar = phar_scratch("env");
        let user_env = vec![
            ("HOME".to_string(), "/Users/dev".to_string()),
            ("WP_CLI_PACKAGES_DIR".to_string(), "/Users/dev/.wp-cli/packages".to_string()),
            ("PATH".to_string(), "/usr/bin".to_string()),
        ];
        let pinned = with_pinned_packages(&user_env, &phar);
        let ours = neutral_packages_path(&phar).display().to_string();
        let mine: Vec<&(String, String)> =
            pinned.iter().filter(|(k, _)| k == "WP_CLI_PACKAGES_DIR").collect();
        assert_eq!(mine.len(), 1, "the user's value survived alongside ours");
        assert_eq!(mine[0].1, ours);
        assert_eq!(
            pinned.last().map(|(k, _)| k.as_str()),
            Some("WP_CLI_PACKAGES_DIR"),
            "ours must also be last, so a consumer that applies in order agrees with one that \
             takes the first match"
        );
        // Everything else the child needs is carried through untouched — HOME
        // especially, or wp-cli's own cache stops working (B25's neighbour).
        assert!(pinned.iter().any(|(k, v)| k == "HOME" && v == "/Users/dev"));
        assert!(pinned.iter().any(|(k, v)| k == "PATH" && v == "/usr/bin"));
        let _ = std::fs::remove_dir_all(sandbox_root(&phar));
    }

    /// A packages dir shaped like a real one: the composer project WP-CLI
    /// writes, with a `vendor/autoload.php` — the file that decides whether it
    /// would have contributed at all.
    fn packages_fixture(tag: &str, composer_json: Option<&str>, autoload: bool) -> PathBuf {
        let home = std::env::temp_dir().join(format!("rexenv-glob-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let dir = home.join(".wp-cli").join("packages");
        std::fs::create_dir_all(dir.join("vendor")).unwrap();
        if autoload {
            std::fs::write(dir.join("vendor").join("autoload.php"), b"<?php").unwrap();
        }
        if let Some(json) = composer_json {
            std::fs::write(dir.join("composer.json"), json).unwrap();
        }
        home
    }

    /// The real file from the dev machine that found #228, trimmed to the shape
    /// that matters. Production-shaped on purpose: a friendly `{"require":{}}`
    /// fixture would not have caught a reader that expected an array.
    const REAL_COMPOSER_JSON: &str = r#"{
        "name": "wp-cli/wp-cli",
        "description": "Installed community packages used by WP-CLI",
        "require": {
            "danielbachhuber/php-compat-command": "dev-master",
            "wp-cli/dist-archive-command": "3.1.0"
        },
        "require-dev": {},
        "minimum-stability": "dev"
    }"#;

    #[test]
    fn a_packages_dir_that_would_have_contributed_is_named_from_its_own_composer_json() {
        let home = packages_fixture("named", Some(REAL_COMPOSER_JSON), true);
        let found = global_packages(None, home.to_str()).expect("the dir would have contributed");
        assert_eq!(
            found.names,
            vec![
                "danielbachhuber/php-compat-command".to_string(),
                "wp-cli/dist-archive-command".to_string()
            ]
        );
        assert_eq!(found.dir, "~/.wp-cli/packages", "the path is shown as the user knows it");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_dir_with_no_autoloader_would_not_have_contributed_and_says_nothing() {
        // The common shape on a machine that once ran `wp package` and no
        // longer has anything installed: the directory exists, the autoloader
        // does not. Telling that user their packages were excluded would be
        // inventing a loss.
        let home = packages_fixture("empty", Some(REAL_COMPOSER_JSON), false);
        assert!(global_packages(None, home.to_str()).is_none());
        assert!(global_packages(None, None).is_none(), "no home to resolve — nothing to claim");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_unreadable_or_unexpected_composer_json_names_nothing_rather_than_guessing() {
        // Four ways the read can go wrong. Every one must yield NO names, so the
        // caller says the directory exists without claiming a count — "the 0
        // packages" would be worse than not rendering, because it invites the
        // reader to conclude something false about their own machine.
        for (tag, json) in [
            ("missing", None),
            ("garbage", Some("not json at all")),
            ("no-require", Some(r#"{"name":"wp-cli/wp-cli"}"#)),
            ("require-is-an-array", Some(r#"{"require":["a/b"]}"#)),
        ] {
            let home = packages_fixture(tag, json, true);
            let found = global_packages(None, home.to_str())
                .unwrap_or_else(|| panic!("{tag}: the dir still would have contributed"));
            assert!(found.names.is_empty(), "{tag}: named packages it could not read");
            let _ = std::fs::remove_dir_all(&home);
        }
        // …and a require that lists only things nobody installed is the same
        // case: present, unnameable, never "0 packages".
        let home = packages_fixture("self-only", Some(r#"{"require":{"php":">=7.2"}}"#), true);
        assert!(global_packages(None, home.to_str()).unwrap().names.is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_exported_packages_dir_is_the_one_reported() {
        // The user's login shell is what the streamed spawns inherit, so an
        // export is the dir that WOULD have contributed — reporting `~/.wp-cli`
        // instead would name the wrong directory in the one sentence whose job
        // is to name it.
        let home = packages_fixture("export", Some(REAL_COMPOSER_JSON), true);
        let exported = home.join(".wp-cli").join("packages");
        let found = global_packages(exported.to_str(), Some("/Users/someone-else"))
            .expect("the exported dir would have contributed");
        assert_eq!(found.dir, exported.display().to_string());
        assert_eq!(found.names.len(), 2);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// The must-say list for the failure-moment tell (#301), with the reason per
    /// phrase — the reason is the durable half, because it is what tells the
    /// next person whether their shorter wording still does the job.
    #[test]
    fn the_failure_moment_tell_says_what_the_user_needs_to_know() {
        let global = GlobalPackages {
            dir: "~/.wp-cli/packages".into(),
            names: vec!["wp-cli/dist-archive-command".into()],
        };
        let phar_said = "Error: 'dist-archive' is not a registered wp command. \
                         See 'wp help' for available commands.";
        let tell = explain_missing_command(phar_said, Some(&global)).expect("the tell");

        for (phrase, why) in [
            (
                "wp-cli/dist-archive-command",
                "LOAD-BEARING: names WHAT was excluded. Without it the user knows only that \
                 something is missing, which is the unexplained loss this tell exists to prevent",
            ),
            (
                "rexenv's terminal",
                "LOAD-BEARING: the relief valve, and the clause a trim reads as reassurance. \
                 Cut it and the tell is a bare capability removal instead of a scoped change",
            ),
            (
                "in what rexenv runs for you",
                "the SCOPE claim — the whole reason the terminal sentence above is true",
            ),
            (
                "resolves in your own shell",
                "why it works there and not here, which is the question being asked at the \
                 moment this fires",
            ),
        ] {
            assert!(tell.contains(phrase), "the tell no longer says {phrase:?} — {why}");
        }

        // APPENDED, never substituted: the caller keeps wp-cli's own line, which
        // is the string the user will paste into a search box.
        assert!(
            !tell.contains("Error: 'dist-archive'"),
            "the tell must not restate the phar's error — it is appended to it, not instead of it"
        );

        // Silent where it has nothing to explain.
        assert!(explain_missing_command(phar_said, None).is_none());
        assert!(explain_missing_command("Error: could not connect to the database", Some(&global))
            .is_none());

        // Unnameable packages: the sentence still lands, without a parenthetical
        // that would be empty or invented.
        let unnamed = GlobalPackages { dir: "~/.wp-cli/packages".into(), names: Vec::new() };
        let tell = explain_missing_command(phar_said, Some(&unnamed)).expect("the tell");
        assert!(tell.contains("without your ~/.wp-cli/packages."), "{tell}");
        assert!(!tell.contains("()"), "an empty parenthetical reached the user: {tell}");
    }

    /// D2's rule at both sites: the tell is APPENDED to a failure the user is
    /// already reading, never substituted for it. WP-CLI's own line is the
    /// string they will paste into a search box, and a friendlier message that
    /// replaced it would be a net loss — the explanation is worth having only in
    /// addition to the thing it explains.
    #[test]
    fn both_tell_sites_append_to_what_wp_cli_said_rather_than_replacing_it() {
        let captured = crate::core::copy_scan::production_source(include_str!("wordpress.rs"));
        assert!(
            captured.contains("pub fn wp_cli_checked("),
            "the scan lost the function it is about — every check here would pass vacuously"
        );
        assert!(
            captured.contains("{stderr}{tell}"),
            "the captured path no longer carries wp-cli's own stderr through to the caller with \
             the tell after it (core::wordpress::wp_cli_checked)"
        );

        // NOT `split("#[cfg(test)]")`: scratch.rs's test module sits in the
        // MIDDLE of the file, so that cut drops the wp_run tool entirely and
        // this guard passes while covering nothing. It did, on the first run.
        let agent =
            crate::core::copy_scan::production_source(include_str!("../mcp_server/scratch.rs"));
        assert!(
            agent.contains("fn agent_stream"),
            "the scan lost the production half of scratch.rs — the same defect this guard's              own comment describes"
        );
        assert!(
            agent.contains("detail.push_str(&super::view::scrub_log_line(&tell, &known))"),
            "the agent path no longer appends the tell to `detail` through the path scrubber — \
             either the explanation is gone, or an exported packages dir can carry the OS \
             username onto the agent surface"
        );
    }

    /// The must-say list for the STANDING tell — the Settings card (#301).
    ///
    /// Guard lives here, in the module owning the facts the copy states, so a
    /// rule change sits next to the sentence promising it (#197's lesson). It
    /// scans what RENDERS, with both canaries — the #235 defect, which this
    /// project has now committed twice.
    #[test]
    fn the_settings_tell_says_what_changed_and_what_still_works() {
        const SETTINGS_SRC: &str = include_str!("../../../src/routes/Settings.tsx");
        let src = crate::core::copy_scan::strip_ts_comments(SETTINGS_SRC);
        assert!(
            src.contains("const PACKAGES_SCOPE"),
            "the stripper ate the source — every check below would pass on an empty string"
        );
        assert!(
            !src.contains("LOAD-BEARING"),
            "comment text survived the strip; prose can satisfy this guard again"
        );

        for (phrase, why) in [
            (
                "only the commands it bundles",
                "the SCOPE claim. This is what the user is being told changed; without it the \
                 card is a list of packages with no statement about them",
            ),
            (
                "are not loaded into the commands rexenv runs for you",
                "WHAT changed, and the redline: 'not loaded into them' referred back across a \
                 sentence boundary and read as ambiguous — into the commands, or into rexenv?",
            ),
            (
                "the same thing here as on a machine that never installed one",
                "LOAD-BEARING: the REASON. Cut it and this is a trade the user is told about, \
                 not a fix — reproducibility is the entire argument for taking something away",
            ),
            (
                "They still work in rexenv's terminal",
                "LOAD-BEARING and the first thing a trim removes as reassurance. It is the \
                 relief valve: without it the card is a capability removal rather than a scoped \
                 change, and the scope narrowing that makes it TRUE is a deliberate exemption \
                 (core/terminal.rs is never pinned), not a nicety",
            ),
            (
                "your command line, not ours",
                "WHY the terminal is exempt, in the user's terms — otherwise the exemption \
                 reads as an inconsistency someone will later 'fix'",
            ),
        ] {
            assert!(src.contains(phrase), "the Settings tell no longer says {phrase:?} — {why}");
        }

        // The don't-guess rule, asserted where it renders: an empty `names` must
        // reach a variant that claims NO count. A card reading "the 0 packages"
        // would invite the reader to conclude something false about their own
        // machine, which is worse than not rendering at all.
        assert!(
            src.contains("data.names.length > 0"),
            "the card no longer branches on whether the packages could be NAMED — an \
             unreadable composer.json would render a count it cannot support"
        );
        assert!(
            src.contains("The packages in"),
            "the unnamed variant is gone; there is now only a copy that claims a count"
        );
    }
}
