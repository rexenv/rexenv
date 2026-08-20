//! Live check: phase-B git ops (fetch / pull --ff-only / checkout / push) plus
//! the working-tree ops (stash / restore / reset / status) against a LOCAL bare
//! origin — every path exercised for real, zero network.
//! Writes only a throwaway temp dir; no services, no app-data.
//! Run: `cargo run --example repo_git_ops_check`
//!
//! The working-tree legs reproduce the situation they exist for, rather than
//! testing the commands in isolation: a tracked file edited by a build, a
//! checkout REFUSED because of it, then stash (recoverable) or reset (not)
//! clearing the way. Two assertions there are the ones worth keeping if the
//! rest is ever trimmed — a stash must not swallow ignored paths (`vendor/`,
//! `node_modules/`: minutes of install time, and `-a` would take them), and
//! reset must not delete untracked files, because that is exactly what its
//! confirm dialog promises the user.

use rexenv_lib::core::{devtools, repo};
use rexenv_lib::platform;
use std::path::{Path, PathBuf};

fn git_in(dir: &Path, args: &[&str]) {
    let st = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.email=fixtures@rexenv.test",
            "-c",
            "user.name=rexenv fixtures",
        ])
        .args(args)
        .output()
        .expect("git");
    assert!(
        st.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&st.stderr)
    );
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

fn main() {
    let plat = platform::current();
    let env = plat.shell().login_shell_env().expect("login_shell_env");
    let git = devtools::resolve_git(&*plat, &env).expect("git").path;
    let sup = plat.supervisor();
    let scratch = std::env::temp_dir().join(format!("rexenv-git-ops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let mut failures: Vec<String> = Vec::new();
    let cancel = repo::CancelToken::new();
    let mut quiet = |_: &str| {};

    // Local bare origin + a seed clone S that plays "the other machine".
    let origin: PathBuf = scratch.join("origin.git");
    std::process::Command::new("git")
        .args(["init", "--bare", "-q"])
        .arg(&origin)
        .status()
        .expect("bare init");
    let seed = scratch.join("seed");
    git_in(&scratch, &["clone", "-q", origin.to_str().unwrap(), "seed"]);
    write(&seed, "plugin.php", "<?php\n/*\nPlugin Name: Ops Fixture\n*/\n");
    write(&seed, "package-lock.json", "lock-v1");
    git_in(&seed, &["add", "-A"]);
    git_in(&seed, &["commit", "-qm", "init"]);
    git_in(&seed, &["push", "-qu", "origin", "HEAD"]);
    let main_branch = {
        let env2 = env.clone();
        repo::read_git_status(sup, &git, &env2, &seed).unwrap().branch.unwrap()
    };
    // A feature branch whose LOCKFILE differs (the deps-changed hint case).
    git_in(&seed, &["checkout", "-qb", "feat"]);
    write(&seed, "package-lock.json", "lock-v2");
    git_in(&seed, &["commit", "-qam", "feat lock"]);
    git_in(&seed, &["push", "-qu", "origin", "feat"]);
    git_in(&seed, &["checkout", "-q", &main_branch]);

    // A = the managed asset checkout.
    let a = scratch.join("asset");
    git_in(&scratch, &["clone", "-q", origin.to_str().unwrap(), "asset"]);

    // 1. fetch — sees the feat branch.
    match repo::git_fetch(sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => println!("fetch ok"),
        Err(e) => failures.push(format!("fetch failed: {e}")),
    }

    // 2. fast-forward pull.
    write(&seed, "new-file.txt", "hello");
    git_in(&seed, &["add", "-A"]);
    git_in(&seed, &["commit", "-qm", "ff change"]);
    git_in(&seed, &["push", "-q"]);
    match repo::git_pull_ff(sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) if a.join("new-file.txt").is_file() => println!("pull --ff-only ok (file arrived)"),
        Ok(()) => failures.push("pull ok but file missing".into()),
        Err(e) => failures.push(format!("ff pull failed: {e}")),
    }

    // 3. DIVERGED pull → mapped error.
    write(&a, "local.txt", "local");
    git_in(&a, &["add", "-A"]);
    git_in(&a, &["commit", "-qm", "local only"]);
    write(&seed, "remote.txt", "remote");
    git_in(&seed, &["add", "-A"]);
    git_in(&seed, &["commit", "-qm", "remote moved"]);
    git_in(&seed, &["push", "-q"]);
    match repo::git_pull_ff(sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => failures.push("diverged pull succeeded?!".into()),
        Err(e) if e.to_string().contains("DIVERGED") => {
            println!("diverged pull mapped (good): {}", e.to_string().lines().next().unwrap())
        }
        Err(e) => failures.push(format!("diverged pull unmapped: {e}")),
    }
    git_in(&a, &["reset", "-q", "--hard", &format!("origin/{main_branch}")]);

    // 4. Dirty-tree pull → "commit or stash" mapped error.
    write(&a, "new-file.txt", "local edit");
    write(&seed, "new-file.txt", "remote edit");
    git_in(&seed, &["commit", "-qam", "touch shared file"]);
    git_in(&seed, &["push", "-q"]);
    match repo::git_pull_ff(sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => failures.push("dirty pull succeeded?!".into()),
        Err(e) if e.to_string().contains("Commit or stash") => {
            println!("dirty pull mapped (good)")
        }
        Err(e) => failures.push(format!("dirty pull unmapped: {e}")),
    }
    git_in(&a, &["checkout", "-q", "--", "."]);
    git_in(&a, &["pull", "-q", "--ff-only"]);

    // 5. checkout feat (remote-tracking DWIM) + the lockfile fingerprint flips.
    let fp_before = repo::lockfile_fingerprint(&a);
    match repo::git_checkout(sup, &git, &env, &a, "feat", &cancel, &mut quiet) {
        Ok(()) => {
            let st = repo::read_git_status(sup, &git, &env, &a).unwrap();
            let fp_after = repo::lockfile_fingerprint(&a);
            println!(
                "checkout ok: branch={:?}, lockfile changed={}",
                st.branch,
                fp_before != fp_after
            );
            if st.branch.as_deref() != Some("feat") {
                failures.push(format!("checkout landed on {:?}", st.branch));
            }
            if fp_before == fp_after {
                failures.push("lockfile fingerprint did not change across checkout".into());
            }
        }
        Err(e) => failures.push(format!("checkout failed: {e}")),
    }

    // 6. push with existing upstream.
    write(&a, "pushed.txt", "x");
    git_in(&a, &["add", "-A"]);
    git_in(&a, &["commit", "-qm", "push me"]);
    match repo::git_push(sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => {
            git_in(&seed, &["fetch", "-q"]);
            let seen = std::process::Command::new("git")
                .args(["-C", seed.to_str().unwrap(), "cat-file", "-e", "origin/feat:pushed.txt"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            println!("push ok (origin sees the commit = {seen})");
            if !seen {
                failures.push("push claimed ok but origin lacks the file".into());
            }
        }
        Err(e) => failures.push(format!("push failed: {e}")),
    }

    // 7. push with NO upstream → auto --set-upstream.
    git_in(&a, &["checkout", "-qb", "local-only"]);
    match repo::git_push(sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => {
            let st = repo::read_git_status(sup, &git, &env, &a).unwrap();
            println!("no-upstream push ok, upstream now = {:?}", st.upstream);
            if st.upstream.is_none() {
                failures.push("auto --set-upstream did not stick".into());
            }
        }
        Err(e) => failures.push(format!("no-upstream push failed: {e}")),
    }

    // 8. Non-fast-forward push → mapped "pull first".
    git_in(&seed, &["checkout", "-q", "feat"]);
    git_in(&seed, &["pull", "-q"]);
    write(&seed, "race.txt", "seed wins");
    git_in(&seed, &["add", "-A"]);
    git_in(&seed, &["commit", "-qm", "seed advances feat"]);
    git_in(&seed, &["push", "-q"]);
    git_in(&a, &["checkout", "-q", "feat"]);
    write(&a, "race2.txt", "asset raced");
    git_in(&a, &["add", "-A"]);
    git_in(&a, &["commit", "-qm", "asset advances feat"]);
    match repo::git_push(sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => failures.push("non-ff push succeeded?!".into()),
        Err(e) if e.to_string().contains("Pull first") => println!("non-ff push mapped (good)"),
        Err(e) => failures.push(format!("non-ff push unmapped: {e}")),
    }

    // ── Working-tree ops: stash / restore / reset / status ────────────────────
    // A SECOND checkout, so the legs above (which leave `a` mid-race on feat)
    // can't decide what these see. This is the user's actual sequence: a build
    // dirtied the tree, and the checkout they wanted is refused.
    let b = scratch.join("dirty");
    git_in(&scratch, &["clone", "-q", origin.to_str().unwrap(), "dirty"]);
    git_in(&b, &["checkout", "-q", "feat"]);
    // Ignored build output — the thing that must NEVER end up in a stash.
    write(&b, ".gitignore", "vendor/\n");
    git_in(&b, &["add", "-A"]);
    git_in(&b, &["commit", "-qm", "ignore vendor"]);
    std::fs::create_dir_all(b.join("vendor")).unwrap();
    write(&b.join("vendor"), "autoload.php", "<?php // 4 minutes of composer\n");
    // The dirt: an edited TRACKED file that differs between branches (so the
    // checkout is genuinely refused) plus an untracked new file.
    write(&b, "package-lock.json", "lock-local-edit");
    write(&b, "scratch-note.txt", "not added to git");

    // 9. The wedge itself: checkout refused on a dirty tree.
    match repo::git_checkout(sup, &git, &env, &b, &main_branch, &cancel, &mut quiet) {
        Ok(()) => failures.push("checkout over a dirty tracked file succeeded?! (no wedge to fix)".into()),
        Err(e) if e.to_string().contains("Commit or stash") => println!("dirty checkout refused (the wedge)"),
        Err(e) => failures.push(format!("dirty checkout unmapped: {e}")),
    }

    // 10. Stash clears it — and leaves the IGNORED tree alone.
    match repo::git_stash_push(sup, &git, &env, &b, &cancel, &mut quiet) {
        Ok(()) => {
            let st = repo::read_git_status(sup, &git, &env, &b).unwrap();
            if st.changed != 0 || st.untracked != 0 {
                failures.push(format!(
                    "after stash the tree is not clean: {} changed, {} untracked",
                    st.changed, st.untracked
                ));
            }
            // The claim `-a` would break: minutes of composer time must still
            // be on disk. A stash that swallowed vendor/ passes every other
            // assertion here and ruins the user's afternoon.
            if !b.join("vendor/autoload.php").is_file() {
                failures.push("STASH SWALLOWED AN IGNORED PATH — vendor/autoload.php is gone".into());
            }
            println!("stash ok (tree clean, ignored vendor/ untouched)");
        }
        Err(e) => failures.push(format!("stash failed: {e}")),
    }

    // 11. …so the checkout the user wanted now works.
    match repo::git_checkout(sup, &git, &env, &b, &main_branch, &cancel, &mut quiet) {
        Ok(()) => println!("checkout after stash ok (the wedge is gone)"),
        Err(e) => failures.push(format!("checkout after stash still failed: {e}")),
    }
    git_in(&b, &["checkout", "-q", "feat"]);

    // 12. The list names the entry, and restore brings BOTH files back.
    let stashes = repo::list_stashes(sup, &git, &env, &b).unwrap_or_default();
    match stashes.first() {
        Some(top) if top.reference == "stash@{0}" && top.message.contains("rexenv") => {
            println!("stash list: {} · {} · {}", top.reference, top.message, top.age);
            match repo::git_stash_pop(sup, &git, &env, &b, &top.reference, &cancel, &mut quiet) {
                Ok(()) => {
                    let back = std::fs::read_to_string(b.join("package-lock.json")).unwrap_or_default();
                    let untracked_back = b.join("scratch-note.txt").is_file();
                    if back != "lock-local-edit" || !untracked_back {
                        failures.push(format!(
                            "restore lost work: lockfile={back:?}, untracked file back={untracked_back}"
                        ));
                    }
                    if !repo::list_stashes(sup, &git, &env, &b).unwrap_or_default().is_empty() {
                        failures.push("pop left the entry in the list (a second copy of the same work)".into());
                    }
                    println!("restore ok (tracked edit + untracked file both back, list empty)");
                }
                Err(e) => failures.push(format!("stash pop failed: {e}")),
            }
        }
        Some(other) => failures.push(format!("unexpected stash entry: {other:?}")),
        None => failures.push("stash list is empty right after a successful stash".into()),
    }

    // 13. Reset reverts TRACKED changes and keeps untracked ones — the exact
    //     promise the confirm dialog makes, which nothing else here can check.
    match repo::git_reset_hard(sup, &git, &env, &b, &cancel, &mut quiet) {
        Ok(()) => {
            let lock = std::fs::read_to_string(b.join("package-lock.json")).unwrap_or_default();
            let kept = b.join("scratch-note.txt").is_file();
            if lock == "lock-local-edit" {
                failures.push("reset --hard left the tracked edit in place".into());
            }
            if !kept {
                failures.push(
                    "reset DELETED an untracked file — the confirm promises it keeps them".into(),
                );
            }
            if !b.join("vendor/autoload.php").is_file() {
                failures.push("reset removed an ignored path".into());
            }
            println!("reset ok (tracked reverted, untracked kept, ignored kept)");
        }
        Err(e) => failures.push(format!("reset failed: {e}")),
    }

    // 14. A clean tree refuses to stash rather than reporting a phantom entry
    //     (git's own "No local changes to save" exits 0).
    git_in(&b, &["clean", "-qfd"]); // fixture-owned: only this throwaway clone
    match repo::git_stash_push(sup, &git, &env, &b, &cancel, &mut quiet) {
        Ok(()) => failures.push("stashed a CLEAN tree — the list now shows an entry that isn't work".into()),
        Err(e) if e.to_string().contains("Nothing to stash") => println!("clean-tree stash refused (good)"),
        Err(e) => failures.push(format!("clean-tree stash unmapped: {e}")),
    }

    // 15. Revision syntax never reaches pop, and a missing entry is honest.
    for bad in ["HEAD@{0}", "stash@{0}^{/x}", ":/text", "--all"] {
        match repo::git_stash_pop(sup, &git, &env, &b, bad, &cancel, &mut quiet) {
            Ok(()) => failures.push(format!("pop accepted the revision expression {bad:?}")),
            Err(e) if e.to_string().contains("is not a stash entry") => {}
            Err(e) => failures.push(format!("pop refused {bad:?} for the wrong reason: {e}")),
        }
    }
    match repo::git_stash_pop(sup, &git, &env, &b, "stash@{7}", &cancel, &mut quiet) {
        Ok(()) => failures.push("popped a stash entry that does not exist".into()),
        Err(e) if e.to_string().contains("no longer exists") => println!("missing stash entry mapped (good)"),
        Err(e) => failures.push(format!("missing stash entry unmapped: {e}")),
    }

    // 16. Status reports WHICH files — the counts in the panel can't.
    write(&b, "package-lock.json", "dirty again");
    write(&b, "another-note.txt", "new");
    let mut seen: Vec<String> = Vec::new();
    {
        let mut sink = |l: &str| seen.push(l.to_string());
        if let Err(e) = repo::git_status_report(sup, &git, &env, &b, &cancel, &mut sink) {
            failures.push(format!("status failed: {e}"));
        }
    }
    let joined = seen.join("\n");
    if !joined.contains("package-lock.json") || !joined.contains("another-note.txt") {
        failures.push(format!("status named no files:\n{joined}"));
    }
    println!("status ok ({} lines, names the dirty files)", seen.len());
    git_in(&b, &["checkout", "-q", "--", "."]);
    let _ = std::fs::remove_file(b.join("another-note.txt"));
    let mut clean_lines: Vec<String> = Vec::new();
    {
        let mut sink = |l: &str| clean_lines.push(l.to_string());
        let _ = repo::git_status_report(sup, &git, &env, &b, &cancel, &mut sink);
    }
    if !clean_lines.iter().any(|l| l.contains("working tree clean")) {
        failures.push(format!(
            "a clean status printed only the branch line — reads as 'nothing happened':\n{}",
            clean_lines.join("\n")
        ));
    } else {
        println!("clean status says so in words (not an empty log pane)");
    }

    let _ = std::fs::remove_dir_all(&scratch);
    println!();
    if failures.is_empty() {
        println!("repo_git_ops_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
