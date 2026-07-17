//! Live check: the repo probe + streamed clone + CANCEL path against real
//! remotes. Networked; writes only inside a throwaway temp dir; touches no
//! services, no app-data — no stack-guard concerns.
//! Run: `cargo run --example repo_clone_check`
//!
//! The load-bearing assertion is the CANCEL one: git clone spawns child
//! workers (`git-remote-https`, `index-pack`) — cancelling mid-transfer must
//! leave the whole process GROUP empty (ps evidence printed), and the partial
//! checkout must be removed.

use rexenv_lib::core::{devtools, repo};
use rexenv_lib::platform;
use std::path::Path;
use std::time::{Duration, Instant};

const SMALL_REPO: &str = "https://github.com/octocat/Hello-World.git";
const BIG_REPO: &str = "https://github.com/WordPress/gutenberg.git"; // long transfer → cancellable
const MISSING_REPO: &str = "https://github.com/rexenv-fixtures/definitely-not-here-xyz.git";

fn ps_group(pgid: u32) -> String {
    let out = std::process::Command::new("ps")
        .args(["-ax", "-o", "pid,pgid,command"])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.split_whitespace().nth(1) == Some(&pgid.to_string()))
        .map(|l| format!("  {l}\n"))
        .collect()
}

fn main() {
    let plat = platform::current();
    let scratch = std::env::temp_dir().join(format!("rexenv-repo-check-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    let mut failures: Vec<String> = Vec::new();

    let env = plat.shell().login_shell_env().expect("login_shell_env");
    let git = devtools::resolve_git(&*plat, &env).expect("git resolves").path;
    println!("git = {}\n", git.display());

    // 1. Probe a real public repo — default branch + refs.
    print!("probe {SMALL_REPO} … ");
    match repo::probe_remote(&git, &env, SMALL_REPO) {
        Ok(refs) => {
            println!(
                "ok: default={:?}, {} branches, {} tags",
                refs.default_branch,
                refs.branches.len(),
                refs.tags.len()
            );
            if refs.default_branch.is_none() || refs.branches.is_empty() {
                failures.push("probe returned no default branch/branches".into());
            }
        }
        Err(e) => failures.push(format!("probe failed: {e}")),
    }

    // 2. Probe a missing repo — mapped error, fast, no hang.
    print!("probe {MISSING_REPO} … ");
    let t = Instant::now();
    match repo::probe_remote(&git, &env, MISSING_REPO) {
        Ok(_) => failures.push("missing repo probed Ok?!".into()),
        Err(e) => {
            println!("errored in {:.1}s (good):\n  {e}", t.elapsed().as_secs_f32());
            let msg = e.to_string();
            if !(msg.contains("not found") || msg.contains("PRIVATE")) {
                failures.push(format!("missing-repo error not mapped: {msg}"));
            }
            if t.elapsed() > Duration::from_secs(35) {
                failures.push("missing-repo probe exceeded the 30s cap".into());
            }
        }
    }

    // 3. Full small clone — streams lines, lands a working checkout.
    let dest = scratch.join("hello-world");
    let cancel = repo::CancelToken::new();
    let mut lines = 0u32;
    print!("clone {SMALL_REPO} … ");
    match repo::clone_repo(
        &*plat.supervisor(),
        &git,
        &env,
        SMALL_REPO,
        None,
        &dest,
        &cancel,
        &mut |_l| lines += 1,
    ) {
        Ok(()) => {
            let git_dir = dest.join(".git").is_dir();
            println!("ok: {lines} streamed lines, .git present = {git_dir}");
            if !git_dir || lines == 0 {
                failures.push("clone landed without .git or without output".into());
            }
        }
        Err(e) => failures.push(format!("small clone failed: {e}")),
    }

    // 4. Collision guard: an existing dir is refused AND left untouched.
    let taken = scratch.join("taken");
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("keep.txt"), "keep").unwrap();
    match repo::clone_repo(
        &*plat.supervisor(),
        &git,
        &env,
        SMALL_REPO,
        None,
        &taken,
        &repo::CancelToken::new(),
        &mut |_| {},
    ) {
        Ok(()) => failures.push("collision clone succeeded?!".into()),
        Err(e) => {
            let survived = taken.join("keep.txt").is_file();
            println!("collision refused (good): {e}\n  existing content survived = {survived}");
            if !survived {
                failures.push("collision guard DELETED existing content".into());
            }
        }
    }

    // 5. THE cancel test: big clone, cancel mid-transfer, prove the group died.
    let big_dest = scratch.join("gutenberg");
    let cancel = repo::CancelToken::new();
    let cancel2 = cancel.clone();
    let sup = plat.supervisor();
    println!("\ncancel test: cloning {BIG_REPO} …");
    let plat2 = platform::current();
    let canceller = std::thread::spawn(move || {
        // Wait for the child group to exist + transfer to be underway.
        let deadline = Instant::now() + Duration::from_secs(20);
        while cancel2.current_pgid().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_secs(3)); // mid-transfer
        let pgid = cancel2.current_pgid().expect("clone never spawned");
        let before = ps_group(pgid);
        println!("process group {pgid} BEFORE cancel:\n{before}");
        let n_before = before.lines().count();
        cancel2.cancel(&*plat2.supervisor());
        (pgid, n_before)
    });
    let mut big_lines = 0u32;
    let clone_res = repo::clone_repo(
        &*sup,
        &git,
        &env,
        BIG_REPO,
        None,
        &big_dest,
        &cancel,
        &mut |_l| big_lines += 1,
    );
    let (pgid, n_before) = canceller.join().expect("canceller thread");
    std::thread::sleep(Duration::from_millis(300)); // let the OS reap
    let after = ps_group(pgid);
    println!("process group {pgid} AFTER cancel:\n{}", if after.is_empty() { "  (empty)\n".into() } else { after.clone() });
    match clone_res {
        Err(e) if e.to_string().contains("cancelled") => {
            println!("clone returned the cancel error (good), {big_lines} lines streamed before cancel");
        }
        other => failures.push(format!("cancelled clone returned {other:?}")),
    }
    if n_before == 0 {
        failures.push("cancel test never saw a live process group (cancelled too early?)".into());
    }
    if !after.is_empty() {
        failures.push(format!("ORPHANS survived cancel in group {pgid}:\n{after}"));
    }
    if big_lines == 0 {
        failures.push("big clone streamed no output before cancel".into());
    }
    if big_dest.exists() {
        failures.push("partial checkout NOT cleaned up after cancel".into());
    } else {
        println!("partial checkout removed = true");
    }

    let _ = std::fs::remove_dir_all(&scratch);
    println!();
    if failures.is_empty() {
        println!("repo_clone_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}

// Silence the unused-path warning when assertions compile out — the example
// keeps Path in scope for readability of signatures above.
#[allow(dead_code)]
fn _t(_: &Path) {}
