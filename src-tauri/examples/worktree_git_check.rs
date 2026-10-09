//! Live check for the **`git worktree` plumbing** under the worktree-sites
//! feature (W2 of `docs/PLAN-git-worktrees.md`). Run:
//! `cargo run --example worktree_git_check`
//!
//! Hermetic: the repository is built in this example's own temp dir, so it
//! needs no network, no credentials and no service. What it exercises is what
//! unit tests cannot — the REAL git's output and the REAL `.git` file a real
//! `git worktree add` writes.
//!
//! Proves:
//!   1. The machine's git passes the worktree floor (`require_worktree_git`).
//!   2. `worktree::list` on a fresh repo returns exactly the main worktree, on
//!      its branch.
//!   3. `worktree::add` checks out an EXISTING branch into a new folder, and a
//!      NEW branch made from a base; `list` then shows all three with the right
//!      branches and paths, as git prints them on this OS.
//!   4. The `.git` FILE git wrote is the shape `foreign_checkout_under` reads:
//!      a "site" folder holding the worktree is flagged, naming the worktree,
//!      and the same folder without it is not (ledger #814 against real git).
//!   5. Adding into a path that already exists is refused BEFORE git runs.
//!   6. A NEW-branch request whose branch already exists (a Retry) checks it out
//!      instead of failing on `-b`.
//!
//! Everything written lives under this example's own temp root; the sandbox
//! `Platform` is only used to resolve git and spawn it (examples/common).

use rexenv_lib::core::repo::CancelToken;
use rexenv_lib::core::{devtools, worktree};
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

fn git_in(dir: &Path, args: &[&str]) {
    // Signing off: a machine whose global config signs every commit (the Dell
    // does, through gpg) would otherwise wait on a pinentry no SSH session has.
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "rexenv checks")
        .env("GIT_AUTHOR_EMAIL", "checks@rexenv.invalid")
        .env("GIT_COMMITTER_NAME", "rexenv checks")
        .env("GIT_COMMITTER_EMAIL", "checks@rexenv.invalid")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
}

fn same(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).ok() == std::fs::canonicalize(b).ok()
}

fn main() {
    let (plat, _sandbox) = common::sandbox("worktree_git_check");
    let root: PathBuf =
        std::env::temp_dir().join(format!("rexenv-worktree-git-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root); // only ever this example's own dir
    std::fs::create_dir_all(&root).expect("temp root");

    let env = plat.shell().login_shell_env().expect("login_shell_env");
    let tool = devtools::resolve_git(&*plat, &env).expect("git resolves");
    let git = tool.path.clone();

    // 1. The floor.
    worktree::require_worktree_git(tool.version.as_deref()).expect("this git is new enough");
    println!("1 ok — git floor: {}", tool.version.as_deref().unwrap_or("(no version line)"));

    // A plugin repo with two branches.
    let repo = root.join("my-plugin");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("my-plugin.php"), "<?php // main\n").unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    git_in(&repo, &["add", "."]);
    git_in(&repo, &["commit", "-q", "-m", "main"]);
    git_in(&repo, &["branch", "feature/checkout-v2"]);

    // 2. A fresh repo lists only itself.
    let sup = plat.supervisor();
    let entries = worktree::list(sup, &git, &env, &repo).expect("list");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert!(same(&entries[0].path, &repo), "{entries:?}");
    assert_eq!(entries[0].branch.as_deref(), Some("main"));
    println!("2 ok — the main worktree alone, on main");

    // 3. Existing branch, then a new one from main — each into a "site" folder.
    let site_a = root.join("Sites/feature-checkout-v2.shop.rex");
    let wt_a = site_a.join("wp-content/plugins/my-plugin");
    std::fs::create_dir_all(wt_a.parent().unwrap()).unwrap();
    let cancel = CancelToken::default();
    let mut log = |l: &str| println!("    | {l}");
    worktree::add(
        sup,
        &git,
        &env,
        &repo,
        &wt_a,
        &worktree::AddSpec::Existing("feature/checkout-v2".into()),
        &cancel,
        &mut log,
    )
    .expect("add existing");
    assert!(wt_a.join("my-plugin.php").exists());

    let wt_b = root.join("Sites/fix-y.shop.rex/wp-content/plugins/my-plugin");
    std::fs::create_dir_all(wt_b.parent().unwrap()).unwrap();
    worktree::add(
        sup,
        &git,
        &env,
        &repo,
        &wt_b,
        &worktree::AddSpec::New { branch: "fix-y".into(), base: "main".into() },
        &cancel,
        &mut log,
    )
    .expect("add new");

    let entries = worktree::list(sup, &git, &env, &repo).expect("list again");
    assert_eq!(entries.len(), 3, "{entries:?}");
    let a = entries.iter().find(|e| same(&e.path, &wt_a)).expect("wt_a listed");
    assert_eq!(a.branch.as_deref(), Some("feature/checkout-v2"));
    let b = entries.iter().find(|e| same(&e.path, &wt_b)).expect("wt_b listed");
    assert_eq!(b.branch.as_deref(), Some("fix-y"));
    // Any worktree answers for all of them.
    assert_eq!(worktree::list(sup, &git, &env, &wt_b).expect("list from a worktree").len(), 3);
    println!("3 ok — existing + new branch added; list shows all three from any worktree");

    // 4. The real `.git` file is read by the guard.
    let flagged = worktree::foreign_checkout_under(&site_a).expect("the site holds a worktree");
    assert!(same(&flagged, &wt_a), "flagged {flagged:?}, expected {wt_a:?}");
    let plain = root.join("Sites/plain.rex");
    std::fs::create_dir_all(plain.join("wp-content/plugins/other")).unwrap();
    assert_eq!(worktree::foreign_checkout_under(&plain), None);
    // The repository itself (a `.git` DIRECTORY) is not foreign to its own folder.
    assert_eq!(worktree::foreign_checkout_under(&repo), None);
    println!("4 ok — git's own .git file is caught by foreign_checkout_under");

    // 5. An existing path is refused before git runs.
    let err = worktree::add(
        sup,
        &git,
        &env,
        &repo,
        &plain,
        &worktree::AddSpec::Existing("main".into()),
        &cancel,
        &mut log,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("already exists"), "{err}");
    assert_eq!(worktree::list(sup, &git, &env, &repo).unwrap().len(), 3);
    println!("5 ok — an existing path is refused, nothing added");

    // 6. A Retry of a NEW-branch request after the branch was already made: the
    //    worktree is gone, `fix-y` is not — `add` must check it out, not `-b` it.
    git_in(&repo, &["worktree", "remove", "--force", &wt_b.to_string_lossy()]);
    let wt_c = root.join("Sites/fix-y-again.shop.rex/wp-content/plugins/my-plugin");
    std::fs::create_dir_all(wt_c.parent().unwrap()).unwrap();
    worktree::add(
        sup,
        &git,
        &env,
        &repo,
        &wt_c,
        &worktree::AddSpec::New { branch: "fix-y".into(), base: "main".into() },
        &cancel,
        &mut log,
    )
    .expect("a retried new-branch add checks the existing branch out");
    let e = worktree::list(sup, &git, &env, &repo).unwrap();
    assert!(e.iter().any(|w| same(&w.path, &wt_c) && w.branch.as_deref() == Some("fix-y")), "{e:?}");
    println!("6 ok — a retried new-branch add checks out the branch an earlier attempt made");

    std::fs::remove_dir_all(&root).expect("cleanup of our own temp root");
    println!("worktree_git_check: all green");
}
