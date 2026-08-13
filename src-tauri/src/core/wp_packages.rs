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
}
