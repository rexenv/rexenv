//! Test-only: the WordPress LAYOUT fixture matrix (`docs/TESTING.md` §3.3).
//!
//! # The bug class this exists for
//!
//! Every path-building function in this tree was written against ONE layout —
//! stock WordPress, where `wp-load.php`, `wp-config.php` and `wp-content` all
//! sit in the docroot. Each time a second layout arrived, the assumption failed
//! somewhere new and was fixed in that one place:
//!
//! - **Bedrock** puts core in `web/wp` and config in `config/`, so `wp core
//!   install` was pinned to the docroot and every wp-cli call on a Bedrock site
//!   answered *"This does not seem to be a WordPress installation"* (#294/#299,
//!   found by RUNNING it against `roots/bedrock` the day it shipped).
//! - A **subdir docroot** (`public/`, how Laravel and some WP setups are laid
//!   out) is why `docroot_subdir` and `Site::served_root` exist at all.
//! - A **moved content dir** is why v24 records `content_dir` rather than
//!   deriving it, and why `logs::wp_debug_log_status` reports `indeterminate`
//!   instead of guessing.
//!
//! §3.3's answer is a matrix rather than another per-instance fix: *"the class
//! is fully testable once the matrix exists — no audit needed, but adding a NEW
//! supported layout must add a matrix row."* This is that matrix. A path
//! function tested over [`layouts`] is tested against every layout rexenv
//! claims to support, and the day a fourth arrives it is one row here rather
//! than a bug per function.
//!
//! # What a row is
//!
//! A real on-disk skeleton — the files these functions actually probe for
//! (`is_file`, `exists`), because a function that asks the filesystem cannot be
//! tested with a string. Each row also carries what the layout MEANS, so a test
//! asserts against the layout's own expectation rather than restating it.
//!
//! Deliberately small: this is the shape of a layout, not a WordPress install.

use std::path::{Path, PathBuf};

/// One supported layout, materialised on disk.
pub(crate) struct Layout {
    /// Name for assertion messages — the row that failed must be obvious.
    pub name: &'static str,
    /// What the site row records as its docroot (what rexenv serves).
    pub docroot: PathBuf,
    /// Where WordPress core really lives, which `wordpress::core_root` must find.
    pub core_root: PathBuf,
    /// The content dir RELATIVE to the docroot, as v24 records it.
    pub content_rel: &'static str,
    /// Whether the wp-config defines are readable from the docroot at all —
    /// false for layouts that keep them elsewhere (Bedrock's `config/`).
    pub config_readable: bool,
    /// Whether `logs::wp_debug_log_status` can DETERMINE the debug flags.
    ///
    /// **Not the same fact as `config_readable`, and separating them is what
    /// this row learned on 24 Aug 2026.** It used to be one field, on the
    /// assumption that wp-config.php was the only place defines could live. An
    /// env-configured Bedrock keeps them in `.env` (read by
    /// `config/application.php`), so it is unreadable by the wp-config reader
    /// and perfectly determinable anyway. A layout with neither is the case
    /// that must stay honest — see the `bedrock-no-env` row.
    pub debug_determinable: bool,
}

/// The matrix, owning the one tree every row lives in.
///
/// ONE guard for the whole matrix, and that is not tidiness — the first version
/// gave every row its own guard over the SAME root, and `for l in layouts(..)`
/// consumes the vec, so dropping row 1 deleted the tree row 2 was about to be
/// read from. Both the canary and the `core_root` test failed on the first run,
/// which is the canary earning its place: without it, "core_root pointed at the
/// wrong directory" would have read as a bug in `core_root`.
pub(crate) struct Matrix {
    rows: Vec<Layout>,
    _root: TempTree,
}

impl Matrix {
    pub fn iter(&self) -> impl Iterator<Item = &Layout> {
        self.rows.iter()
    }
}

/// A directory removed when the matrix goes out of scope. The repo's own
/// example-cleanup rule, applied to unit tests: delete only what was created.
pub(crate) struct TempTree(PathBuf);

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn touch(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("layout fixture dir");
    }
    std::fs::write(path, body).expect("layout fixture file");
}

/// The three layouts rexenv supports today, each a fresh tree under `tag`.
///
/// Call it once per test: the trees are removed when the returned rows drop, so
/// two tests never share a skeleton (and a failing test still cleans up).
pub(crate) fn layouts(tag: &str) -> Matrix {
    let base = std::env::temp_dir()
        .join(format!("rexenv-layouts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    // ── stock: everything in the docroot ────────────────────────────────────
    let stock = base.join("stock");
    touch(&stock.join("wp-load.php"), "<?php");
    touch(&stock.join("wp-config.php"), "<?php define('WP_DEBUG', true);\n");
    touch(&stock.join("wp-content/index.php"), "<?php");

    // ── Bedrock: core under web/wp, config in config/, content is web/app ───
    // The docroot is `web`, which is what rexenv serves and records.
    let bedrock = base.join("bedrock");
    touch(&bedrock.join("web/wp/wp-load.php"), "<?php");
    touch(&bedrock.join("web/app/plugins/.keep"), "");
    touch(&bedrock.join("config/application.php"), "<?php // env-driven defines\n");
    touch(&bedrock.join(".env"), "WP_DEBUG=true\n");

    // ── Bedrock with NO env answer: the honest-indeterminate case ───────────
    // Same shape as the row above, minus any WP_DEBUG in `.env`. Without this
    // row the matrix would only prove the path where the answer is FOUND, and
    // the whole point of `indeterminate` is the path where it is not.
    let bare = base.join("bedrock-no-env");
    touch(&bare.join("web/wp/wp-load.php"), "<?php");
    touch(&bare.join("web/app/plugins/.keep"), "");
    touch(&bare.join("config/application.php"), "<?php // defines hardcoded here\n");
    touch(&bare.join(".env"), "DB_NAME=example\n");

    // ── a stray `.env` and NO Bedrock marker: must stay indeterminate ───────
    // A `.env` on its own says nothing — Laravel, Docker and a dozen tools write
    // one — so a WP_DEBUG in it is not evidence about THIS WordPress. Without
    // this row, dropping the `config/application.php` half of the marker check
    // passes every other assertion (verified by planting exactly that).
    let stray = base.join("stray-env");
    touch(&stray.join("web/wp/wp-load.php"), "<?php");
    touch(&stray.join("web/app/plugins/.keep"), "");
    touch(&stray.join(".env"), "WP_DEBUG=true\n");

    // ── subdir docroot: the project root is not the served root ─────────────
    let subdir = base.join("subdir");
    touch(&subdir.join("public/wp-load.php"), "<?php");
    touch(&subdir.join("public/wp-config.php"), "<?php define('WP_DEBUG', false);\n");
    touch(&subdir.join("public/wp-content/index.php"), "<?php");

    Matrix {
        rows: vec![
            Layout {
                name: "stock",
                docroot: stock.clone(),
                core_root: stock,
                content_rel: "wp-content",
                config_readable: true,
                debug_determinable: true,
            },
            Layout {
                name: "bedrock",
                docroot: bedrock.join("web"),
                core_root: bedrock.join("web/wp"),
                content_rel: "app",
                config_readable: false,
                // `.env` carries WP_DEBUG, so the reader CAN answer.
                debug_determinable: true,
            },
            Layout {
                name: "bedrock-no-env",
                docroot: bare.join("web"),
                core_root: bare.join("web/wp"),
                content_rel: "app",
                config_readable: false,
                // Nothing anywhere says WP_DEBUG — "off" here would be a guess.
                debug_determinable: false,
            },
            Layout {
                name: "stray-env",
                docroot: stray.join("web"),
                core_root: stray.join("web/wp"),
                content_rel: "app",
                config_readable: false,
                // A `.env` with no `config/application.php` beside it is not an
                // env-configured WordPress, so its WP_DEBUG proves nothing here.
                debug_determinable: false,
            },
            Layout {
                name: "subdir-docroot",
                docroot: subdir.join("public"),
                core_root: subdir.join("public"),
                content_rel: "wp-content",
                config_readable: true,
                debug_determinable: true,
            },
        ],
        _root: TempTree(base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The matrix itself has to be real, or every test over it passes vacuously
    /// against three empty directories. This is the landmark canary the ledger
    /// tally and the WCAG scan both carry, for the same reason.
    #[test]
    fn every_layout_row_is_a_real_tree_that_matches_what_it_claims() {
        for l in layouts("canary").iter() {
            assert!(l.docroot.is_dir(), "{}: docroot was never created", l.name);
            assert!(
                l.core_root.join("wp-load.php").is_file(),
                "{}: core_root has no wp-load.php, so it is not where core lives",
                l.name
            );
            let content = l.docroot.join(l.content_rel);
            assert!(
                content.is_dir(),
                "{}: content_rel {:?} does not exist under the docroot",
                l.name,
                l.content_rel
            );
            assert_eq!(
                l.docroot.join("wp-config.php").is_file(),
                l.config_readable,
                "{}: config_readable disagrees with the tree it describes",
                l.name
            );
        }
    }

    /// The one the class is named after: core is found wherever the layout puts
    /// it. Before #294 this was true of stock only, and every wp-cli call on a
    /// Bedrock site failed with "This does not seem to be a WordPress
    /// installation".
    #[test]
    fn core_root_finds_wordpress_in_every_supported_layout() {
        for l in layouts("core-root").iter() {
            assert_eq!(
                crate::core::wordpress::core_root(&l.docroot),
                l.core_root,
                "{}: core_root pointed at the wrong directory",
                l.name
            );
            // The wp-cli flag is the same fact in the form the spawns use: set
            // exactly when core is NOT the docroot, so a stock site's argv is
            // unchanged and a Bedrock one carries --path.
            let arg = crate::core::wordpress::core_path_arg(&l.docroot);
            assert_eq!(
                arg.is_some(),
                l.core_root != l.docroot,
                "{}: core_path_arg disagrees with core_root ({arg:?})",
                l.name
            );
        }
    }

    /// The debug-log reader must never report a confident answer for a layout
    /// whose defines it cannot see — and must not report `indeterminate` for one
    /// where it CAN.
    ///
    /// `indeterminate` was Bedrock's verdict wholesale until 24 Aug 2026, on the
    /// assumption that wp-config.php is the only place defines live. An
    /// env-configured Bedrock keeps them in `.env`, read by
    /// `config/application.php`, so the reader can answer truthfully; one with
    /// nothing in `.env` still cannot, and "off" there would be a silent wrong
    /// answer — the class docs/TESTING.md §3.3 is about. Both rows are here
    /// because a matrix that only carried the answerable one would call the
    /// improvement complete.
    #[test]
    fn the_debug_log_reader_says_indeterminate_exactly_where_it_cannot_see() {
        for l in layouts("debug-log").iter() {
            let st = crate::core::logs::wp_debug_log_status(&l.docroot, l.content_rel);
            assert_eq!(
                st.indeterminate, !l.debug_determinable,
                "{}: indeterminate={} for a layout whose debug_determinable={}",
                l.name, st.indeterminate, l.debug_determinable
            );
            // Wherever it points, the path is under the layout's OWN content
            // dir — never a stock `wp-content` guessed onto a tree that has none.
            assert!(
                st.path.starts_with(&l.docroot.join(l.content_rel).to_string_lossy().to_string()),
                "{}: debug.log path {:?} escaped the layout's content dir",
                l.name,
                st.path
            );
        }
    }
}
