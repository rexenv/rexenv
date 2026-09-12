//! Live check: `wp dist-archive` runs from the tree rexenv CARRIES, and the
//! archive it produces is the one the button promises.
//!
//!   cargo run --example dist_archive_check
//!
//! # The negative control is the point of this file
//!
//! Asked whether the bundled WP-CLI phar has `dist-archive`, running
//! `wp dist-archive --help` on a developer machine says **yes** — from whatever
//! that person installed in `~/.wp-cli/packages/` at some point in the past
//! (here: December 2021). The phar has no such command. A check that only
//! asserted "the command works" would pass on that machine forever and fail on
//! every clean one, and nothing in the output would say why (ledger #228, and
//! the "machine was the fixture" defect-family entry).
//!
//! So this runs BOTH legs, with `WP_CLI_PACKAGES_DIR` pointed at an empty
//! directory in each:
//!
//! - **A (control)**: the same argv WITHOUT rexenv's `--require` must FAIL with
//!   `not a registered wp command`. That is the plant, baked in permanently — if
//!   the capability ever comes from somewhere other than our vendored tree, this
//!   leg goes green and the check fails loudly instead of quietly passing.
//! - **B**: with the `--require`, an archive is produced.
//!
//! Neither leg alone proves anything. A is what makes B mean "our tree did it".
//!
//! # What else it proves, live
//!
//! - `.distignore` is honoured: `.git` and `node_modules` are absent from the
//!   zip that a real `zip` binary wrote (L0 can only assert the argv).
//! - the refusal fires when the file is missing — the case that would otherwise
//!   ship those directories under a `Success:`.
//! - **nothing is written into the checkout**, which is the tooltip's
//!   load-bearing sentence (#235) and, for a linked asset, a promise about the
//!   user's own repository.
//! - the scratch dir and the tool's own `TMPDIR` litter are gone afterwards.
//!
//! # Fixture ownership (`examples/common/mod.rs` — read it first)
//!
//! Sandboxed `Platform`, so the vendored tree and every scratch dir land under a
//! temp root and never the real app-data. The fixture "site" and "checkout" are
//! created here and removed here. The shared binary cache is the documented
//! exception (php + the phar are resolved from it).
//!
//! **Downloads is deliberately NOT exercised.** `deliver_to_downloads` writes to
//! the real `~/Downloads`, which is not fixture-owned by any reading; the
//! delivery closure here writes into the fixture instead. The collision
//! numbering is L0 (`downloads_are_numbered_on_collision_and_never_overwrite`)
//! and the real folder is SMOKE's job. Stated rather than left as a gap.

use rexenv_lib::core::{binaries, dist_archive, php, repo, wordpress, wp_packages};
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

/// A plugin checkout shaped like one a developer would actually have: a real
/// git repo, a `node_modules` worth excluding, a versioned header — and reached
/// through a SYMLINK whose name differs from the folder's, which is the linked
/// -site case and the one every path rule in `dist_archive` is really about.
struct Fixture {
    root: PathBuf,
    real: PathBuf,
    link: PathBuf,
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("rexenv-distarch-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let real = root.join("code/my-awesome-plugin");
    let plugins = root.join("Sites/probe.rex/wp-content/plugins");
    std::fs::create_dir_all(real.join("src")).expect("src");
    std::fs::create_dir_all(real.join("node_modules/leftpad")).expect("node_modules");
    std::fs::create_dir_all(&plugins).expect("plugins");
    std::fs::write(
        real.join("my-awesome-plugin.php"),
        "<?php\n/**\n * Plugin Name: My Awesome Plugin\n * Version: 1.2.3\n */\n",
    )
    .expect("plugin file");
    std::fs::write(real.join("src/app.js"), "export const a = 1;\n").expect("src file");
    std::fs::write(real.join("node_modules/leftpad/index.js"), "// junk\n").expect("junk");
    std::fs::write(real.join(".distignore"), ".git\n.distignore\nnode_modules\n").expect("distignore");
    for args in [
        vec!["init", "-q"],
        vec!["add", "-A"],
        vec!["-c", "user.email=a@b.c", "-c", "user.name=probe", "commit", "-qm", "init"],
    ] {
        let ok = Command::new("git")
            .args(&args)
            .current_dir(&real)
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?} failed in the fixture");
    }
    // The link name differs from the folder name on purpose (#230/§1.5).
    let link = plugins.join("awesome-slug");
    common::symlink(&real, &link).expect("symlink");
    Fixture { root, real, link }
}

/// Everything under the checkout, with contents, so "wrote nothing" is a
/// comparison rather than a glance. Sorted for a stable diff.
fn snapshot(dir: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("read_dir").flatten() {
            let p = entry.path();
            let meta = entry.metadata().expect("metadata");
            if meta.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(dir).expect("under dir").display().to_string();
                out.push((rel, meta.len()));
            }
        }
    }
    out.sort();
    out
}

fn zip_entries(archive: &Path) -> Vec<String> {
    let out = Command::new("/usr/bin/unzip")
        .arg("-Z1")
        .arg(archive)
        .output()
        .expect("unzip -Z1");
    assert!(out.status.success(), "unzip failed on {}", archive.display());
    String::from_utf8_lossy(&out.stdout).lines().map(|l| l.to_string()).collect()
}

#[tokio::main]
async fn main() {
    let (plat, _guard) = common::sandbox("dist-archive");
    let f = fixture();

    // Binaries come from the shared cache (the documented exception); the
    // vendored tree is materialised INTO THE SANDBOX.
    let patch = php::patch_for_minor("8.3").expect("pinned 8.3");
    let php_bin = binaries::resolve(&*plat, "php", patch).await.expect("php");
    let wp_phar = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION)
        .await
        .expect("wp-cli phar");
    let autoload = wp_packages::ensure_dist_archive(plat.paths()).expect("vendored tree");
    let packages_dir = wp_packages::empty_packages_dir(plat.paths()).expect("empty packages dir");
    assert!(
        autoload.starts_with(plat.paths().app_data_dir().expect("sandbox data")),
        "the vendored tree escaped the sandbox: {}",
        autoload.display()
    );

    // ── A · THE NEGATIVE CONTROL ────────────────────────────────────────────
    // Without our --require, and with the ambient packages dir neutralised, the
    // command must NOT EXIST. If this ever succeeds, `dist-archive` is coming
    // from somewhere rexenv does not ship, and every other assertion in this
    // file has stopped meaning what it says.
    // The same argv prefix production runs (#228) — the control differs from a
    // real run in the two things it is controlling for, our `--require` and the
    // packages dir, and in nothing else.
    let control = Command::new(&php_bin)
        .args(wordpress::wp_argv_prefix(&wp_phar))
        .arg("dist-archive")
        .arg(&f.real)
        .arg(f.root.join("control-out"))
        .env("WP_CLI_PACKAGES_DIR", &packages_dir)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("spawn the control");
    let control_err = String::from_utf8_lossy(&control.stderr).to_string();
    assert!(
        !control.status.success() && control_err.contains("not a registered wp command"),
        "CONTROL LEG FAILED: the bundled phar answered `dist-archive` on its own.\n\
         That means this machine is supplying the command and every other check \
         here is passing for the wrong reason — exactly the December-2021 package \
         that nearly shipped this feature (#228).\n\
         exit={:?}\nstderr: {control_err}",
        control.status.code()
    );
    println!("A ok — the phar alone has no dist-archive: {}", control_err.trim());

    // ── B · the real run, through the production entry point ────────────────
    let before = snapshot(&f.real);
    let delivered_dir = f.root.join("delivered");
    std::fs::create_dir_all(&delivered_dir).expect("delivered dir");
    let cancel = repo::CancelToken::new();
    let mut lines: Vec<String> = Vec::new();
    let landed = dist_archive::build_and_deliver(
        plat.paths(),
        plat.supervisor(),
        &php_bin,
        &wp_phar,
        &autoload,
        &packages_dir,
        &f.link,
        &[("PATH".to_string(), "/usr/bin:/bin".to_string())],
        &cancel,
        &mut |l| lines.push(l.to_string()),
        &mut |archive| {
            let dest = delivered_dir.join(archive.file_name().expect("file name"));
            std::fs::copy(archive, &dest)?;
            Ok(dest)
        },
    )
    .unwrap_or_else(|e| panic!("archive failed: {e}\n{}", lines.join("\n")));
    println!("B ok — produced {}", landed.display());

    // The name carries the version from the plugin header, and the FOLDER's
    // name — not the wp-content link's. rexenv reports what was produced rather
    // than renaming someone's plugin to match its own label (§1.5).
    assert_eq!(
        landed.file_name().expect("name"),
        "my-awesome-plugin.1.2.3.zip",
        "unexpected archive name"
    );
    assert!(
        !dist_archive::version_missing_from_name(&landed, &f.real),
        "a versioned archive was reported as versionless"
    );

    // ── C · .distignore was honoured by the real zip ────────────────────────
    let entries = zip_entries(&landed);
    assert!(
        entries.iter().any(|e| e.ends_with("my-awesome-plugin.php")),
        "the plugin file is missing from the archive: {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e.ends_with("src/app.js")),
        "src/ is missing from the archive: {entries:?}"
    );
    for excluded in [".git/", "node_modules/", ".distignore"] {
        assert!(
            !entries.iter().any(|e| e.contains(excluded)),
            "`{excluded}` shipped in the archive — a zip like that is worse than none: {entries:?}"
        );
    }
    println!("C ok — {} entries, none of them .git or node_modules", entries.len());

    // ── D · nothing was written into the checkout ───────────────────────────
    // The tooltip's load-bearing sentence (#235), and for a linked asset a
    // promise about the user's own repository. Compared file-by-file, plus
    // git's own view, because a build artefact that git reports is the visible
    // half of the same failure.
    let after = snapshot(&f.real);
    assert_eq!(before, after, "the checkout changed during the archive");
    let porcelain = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&f.real)
        .output()
        .expect("git status");
    assert!(
        porcelain.stdout.is_empty(),
        "the archive left something in the user's git status:\n{}",
        String::from_utf8_lossy(&porcelain.stdout)
    );
    println!("D ok — the checkout is byte-identical and git-clean");

    // ── E · the scratch dir and the tool's litter are gone ──────────────────
    let work = plat
        .paths()
        .app_data_dir()
        .expect("sandbox data")
        .join("dist-archive-work");
    let leftovers: Vec<PathBuf> = std::fs::read_dir(&work)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "scratch dirs survived the run — dist-archive's TMPDIR copy leaks with them: {leftovers:?}"
    );
    println!("E ok — no scratch dir left behind");

    // ── F · no .distignore, no archive ──────────────────────────────────────
    // The default case, and the harmful one: without the file the tool would
    // archive .git and node_modules and report success.
    std::fs::remove_file(f.real.join(".distignore")).expect("remove .distignore");
    assert!(!dist_archive::has_distignore(&f.link), "the UI predicate still offers it");
    let refused = dist_archive::build_and_deliver(
        plat.paths(),
        plat.supervisor(),
        &php_bin,
        &wp_phar,
        &autoload,
        &packages_dir,
        &f.link,
        &[("PATH".to_string(), "/usr/bin:/bin".to_string())],
        &cancel,
        &mut |_| {},
        &mut |_| panic!("delivered an archive for a checkout with no .distignore"),
    )
    .expect_err("a checkout with no .distignore was archived");
    let msg = refused.to_string();
    for must_say in ["node_modules", "does NOT fall back to .gitignore", "reports success"] {
        assert!(msg.contains(must_say), "the refusal never says `{must_say}`:\n{msg}");
    }
    println!("F ok — refused, and said why");

    let _ = std::fs::remove_dir_all(&f.root);
    println!("\ndist_archive_check: PASS");
}
