//! W7 S6, ledger #625: Windows' linked folders through the real platform — `ShellRunner::symlink_dir` makes a
//! junction, the delete guard sees it, and `remove_symlink` takes only the link.
//!
//! ```text
//! (desktop session) scripts/probes/windows-junction.ps1 through windows-limited-token.sh
//! ```
//!
//! Run as the desktop user (Medium token, no Developer Mode) — the account "Link folder" runs as. In a fixture
//! under `%TEMP%`: a `checkout` folder with a file, and a `site\wp-content\plugins` folder holding a real plugin
//! folder `normal`. In order:
//! - `validate_link_target` refuses a target inside the site and a target containing the site (Windows'
//!   canonical `\\?\` paths on both sides), and accepts the checkout;
//! - `symlink_dir` links `plugins\linked` to the checkout → `is_symlink` true, `read_link` names the checkout,
//!   and a file written through the link appears in the checkout;
//! - `partition_symlink_deletes` puts `linked` in the linked list and `normal` in the normal one — the guard
//!   that keeps `wp plugin delete` out of a linked checkout;
//! - `symlink_dir` over the existing link is refused; to a network path is refused;
//! - `remove_symlink` on the real `normal` folder is refused and leaves it; on `linked` removes the link and
//!   leaves every file in the checkout.
//!
//! Fixture-owned: any link is removed with `remove_dir` before the fixture folder, and only the fixture is
//! removed. `demo` tier: Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_junction_check: skipped — a Windows check (ledger #625, W7)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use rexenv_lib::core::repo::{partition_symlink_deletes, validate_link_target};
    use std::path::Path;
    use std::process::ExitCode;

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_junction_check");
        let root = std::env::temp_dir().join(format!("rexenv-s6-{}", std::process::id()));
        if root.exists() {
            check.is("the fixture folder is new", false, &root.display().to_string());
            return check.verdict();
        }
        let checkout = root.join("checkout");
        let site = root.join("site");
        let plugins = site.join(r"wp-content\plugins");
        std::fs::create_dir_all(&checkout).expect("checkout");
        std::fs::write(checkout.join("my-plugin.php"), "<?php // the user's real checkout\n").expect("checkout file");
        std::fs::create_dir_all(plugins.join("normal")).expect("normal plugin");
        std::fs::write(plugins.join(r"normal\normal.php"), "<?php // a plugin wp-cli may delete\n").expect("normal file");
        let linked = plugins.join("linked");

        let shell_platform = rexenv_lib::platform::current();
        let shell = shell_platform.shell();

        // ── validate_link_target on Windows' canonical paths ──
        let inside = validate_link_target(&site, &linked, &plugins.join("normal"));
        check.is("a target inside the site is refused", inside.as_ref().is_err_and(|e| e.to_string().contains("inside this site")), &format!("{inside:?}"));
        let parent = validate_link_target(&site, &linked, &root);
        check.is("a target containing the site is refused", parent.as_ref().is_err_and(|e| e.to_string().contains("CONTAINS")), &format!("{parent:?}"));
        let canonical = validate_link_target(&site, &linked, &checkout);
        check.is("the checkout is accepted", canonical.is_ok(), &format!("{canonical:?}"));
        let Ok(canonical) = canonical else { return finish(check, &root, &linked) };

        // ── symlink_dir makes a junction ──
        let made = shell.symlink_dir(&canonical, &linked);
        let is_link = std::fs::symlink_metadata(&linked).map(|m| m.file_type().is_symlink()).unwrap_or(false);
        check.is("symlink_dir links the folder and the link reads as a symlink", made.is_ok() && is_link, &format!("{made:?}, is_symlink {is_link}"));
        let target = std::fs::read_link(&linked).map(|p| p.to_string_lossy().to_lowercase());
        let want = checkout.to_string_lossy().to_lowercase();
        check.is("read_link names the checkout", target.as_ref().is_ok_and(|t| t.trim_start_matches(r"\\?\").ends_with(want.trim_start_matches(r"\\?\"))), &format!("{target:?} vs {want}"));
        let wrote = std::fs::write(linked.join("through-the-link.txt"), "written through the junction\n");
        check.is("a file written through the link lands in the checkout", wrote.is_ok() && checkout.join("through-the-link.txt").exists(), &format!("{wrote:?}"));

        // ── the delete guard ──
        let (link_names, normal_names) = partition_symlink_deletes(&plugins, &["linked".to_string(), "normal".to_string()]);
        check.is("partition_symlink_deletes puts the junction in the linked list and the real folder in the normal one",
            link_names == ["linked"] && normal_names == ["normal"], &format!("linked {link_names:?}, normal {normal_names:?}"));

        // ── refusals ──
        let again = shell.symlink_dir(&canonical, &linked);
        check.is("symlink_dir over an existing link is refused", again.is_err(), &format!("{again:?}"));
        let unc = shell.symlink_dir(Path::new(r"\\?\UNC\nas\share\plugin"), &plugins.join("remote"));
        check.is("symlink_dir to a network share is refused, and nothing is left behind",
            unc.as_ref().is_err_and(|e| e.to_string().contains("network")) && !plugins.join("remote").exists(), &format!("{unc:?}"));
        let real = shell.remove_symlink(&plugins.join("normal"));
        check.is("remove_symlink refuses a real folder and leaves it", real.is_err() && plugins.join(r"normal\normal.php").exists(), &format!("{real:?}"));

        // ── remove_symlink takes only the link ──
        let removed = shell.remove_symlink(&linked);
        check.is("remove_symlink removes the link", removed.is_ok() && std::fs::symlink_metadata(&linked).is_err(), &format!("{removed:?}"));
        check.is("…and every file in the checkout is still there",
            checkout.join("my-plugin.php").exists() && checkout.join("through-the-link.txt").exists(), &checkout.display().to_string());
        finish(check, &root, &linked)
    }

    fn finish(check: Check, root: &Path, linked: &Path) -> ExitCode {
        if std::fs::symlink_metadata(linked).is_ok() {
            let _ = std::fs::remove_dir(linked);
        }
        let removed = std::fs::symlink_metadata(linked).is_err() && std::fs::remove_dir_all(root).is_ok();
        println!("  · cleanup: fixture removed: {removed}");
        check.verdict()
    }
}
