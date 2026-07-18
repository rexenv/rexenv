//! Live check: phase-C watch semantics at the core level — a long-running
//! `npm run` script streams, CANCEL kills the whole process group (npm →
//! node child), and a crashing script surfaces its exit code. No network
//! (scripts run plain node); writes only a throwaway temp dir.
//! Run: `cargo run --example repo_watch_check`

use rexenv_lib::core::{devtools, repo};
use rexenv_lib::platform;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn ps_group(pgid: u32) -> Vec<String> {
    let out = std::process::Command::new("ps")
        .args(["-ax", "-o", "pid,pgid,comm"])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.split_whitespace().nth(1) == Some(&pgid.to_string()))
        .map(|l| l.trim().to_string())
        .collect()
}

fn main() {
    let plat = platform::current();
    let env = plat.shell().login_shell_env().expect("login_shell_env");
    let npm = devtools::resolve_package_manager(&env, "npm").expect("npm").path;
    let sup = plat.supervisor();
    let scratch = std::env::temp_dir().join(format!("rexenv-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let mut failures: Vec<String> = Vec::new();

    std::fs::write(
        scratch.join("package.json"),
        r#"{"name":"watch-fixture","version":"1.0.0","scripts":{"watch":"node watch.js","boom":"node -e \"console.log('pre-crash output'); process.exit(3)\""}}"#,
    )
    .unwrap();
    std::fs::write(
        scratch.join("watch.js"),
        "let n=0; setInterval(()=>console.log('tick '+ ++n), 250);",
    )
    .unwrap();

    // Script listing + heuristic on the fixture.
    let scripts = repo::list_scripts(&scratch);
    let watchy = scripts.iter().find(|s| s.name == "watch").map(|s| s.watchy);
    println!("scripts: {:?}", scripts.iter().map(|s| (&s.name, s.watchy)).collect::<Vec<_>>());
    if watchy != Some(true) {
        failures.push("'watch' not flagged watchy".into());
    }

    // 1. The watcher: streams ticks, never ends — cancel kills the TREE.
    let cancel = repo::CancelToken::new();
    let ticks = Arc::new(AtomicU32::new(0));
    let t2 = ticks.clone();
    let npm2 = npm.clone();
    let env2 = env.clone();
    let dir2 = scratch.clone();
    let cancel2 = cancel.clone();
    let plat2 = platform::current();
    let runner = std::thread::spawn(move || {
        let mut on_line = |l: &str| {
            if l.contains("tick") {
                t2.fetch_add(1, Ordering::SeqCst);
            }
        };
        repo::node_run_script(
            &*plat2.supervisor(),
            &npm2,
            &dir2,
            "watch",
            &env2,
            &cancel2,
            &mut on_line,
        )
    });
    // Wait for real output (proves streaming while alive).
    let deadline = Instant::now() + Duration::from_secs(30);
    while ticks.load(Ordering::SeqCst) < 3 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let streamed = ticks.load(Ordering::SeqCst);
    println!("streamed ticks before cancel: {streamed}");
    if streamed < 3 {
        failures.push("watcher never streamed".into());
    }
    let pgid = cancel.current_pgid().expect("watch pgid");
    let before = ps_group(pgid);
    println!("group {pgid} BEFORE stop: {} processes", before.len());
    if before.is_empty() {
        failures.push("no live process group for the watcher".into());
    }
    cancel.cancel(&*sup);
    let result = runner.join().expect("runner join").expect("step result");
    std::thread::sleep(Duration::from_millis(300));
    let after = ps_group(pgid);
    println!(
        "group {pgid} AFTER stop: {} processes; cancelled={}",
        after.len(),
        result.cancelled
    );
    if !after.is_empty() {
        failures.push(format!("ORPHANS after watch stop: {after:?}"));
    }
    if !result.cancelled {
        failures.push("watch result not marked cancelled".into());
    }

    // 2. Crash: exit code surfaces (the UI shows 'exited (code 3)' + Restart).
    let cancel = repo::CancelToken::new();
    let mut lines = 0u32;
    let result = repo::node_run_script(
        &*sup,
        &npm,
        &scratch,
        "boom",
        &env,
        &cancel,
        &mut |_l| lines += 1,
    )
    .expect("boom run");
    println!("crash script: ok={}, exit={:?}, lines={lines}", result.ok, result.exit);
    if result.ok || result.cancelled {
        failures.push("crash script not reported as failure".into());
    }
    // npm forwards the child's exit code.
    if result.exit != Some(3) && result.exit != Some(1) {
        failures.push(format!("unexpected crash exit {:?}", result.exit));
    }
    if lines == 0 {
        failures.push("crash script output not streamed".into());
    }

    let _ = std::fs::remove_dir_all(&scratch);
    println!();
    if failures.is_empty() {
        println!("repo_watch_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
