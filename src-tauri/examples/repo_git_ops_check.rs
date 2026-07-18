//! Live check: phase-B git ops (fetch / pull --ff-only / checkout / push)
//! against a LOCAL bare origin — every path exercised for real, zero network.
//! Writes only a throwaway temp dir; no services, no app-data.
//! Run: `cargo run --example repo_git_ops_check`

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
        repo::read_git_status(&git, &env2, &seed).unwrap().branch.unwrap()
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
    match repo::git_fetch(&*sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => println!("fetch ok"),
        Err(e) => failures.push(format!("fetch failed: {e}")),
    }

    // 2. fast-forward pull.
    write(&seed, "new-file.txt", "hello");
    git_in(&seed, &["add", "-A"]);
    git_in(&seed, &["commit", "-qm", "ff change"]);
    git_in(&seed, &["push", "-q"]);
    match repo::git_pull_ff(&*sup, &git, &env, &a, &cancel, &mut quiet) {
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
    match repo::git_pull_ff(&*sup, &git, &env, &a, &cancel, &mut quiet) {
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
    match repo::git_pull_ff(&*sup, &git, &env, &a, &cancel, &mut quiet) {
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
    match repo::git_checkout(&*sup, &git, &env, &a, "feat", &cancel, &mut quiet) {
        Ok(()) => {
            let st = repo::read_git_status(&git, &env, &a).unwrap();
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
    match repo::git_push(&*sup, &git, &env, &a, &cancel, &mut quiet) {
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
    match repo::git_push(&*sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => {
            let st = repo::read_git_status(&git, &env, &a).unwrap();
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
    match repo::git_push(&*sup, &git, &env, &a, &cancel, &mut quiet) {
        Ok(()) => failures.push("non-ff push succeeded?!".into()),
        Err(e) if e.to_string().contains("Pull first") => println!("non-ff push mapped (good)"),
        Err(e) => failures.push(format!("non-ff push unmapped: {e}")),
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
