//! core::prompt — waiting on a privileged prompt without holding the async
//! runtime hostage.
//!
//! An admin-password dialog or a keychain dialog stays open for as long as the
//! user takes, and the platform call that raised it waits on a plain thread the
//! whole time. Called straight from an `async fn`, that wait sits on a tokio
//! worker: every task queued behind it on that worker — first-run downloads,
//! status polls, the download-progress bridge — stops until the user answers
//! (#567, found diagnosing #566). A SYNC Tauri command is worse: Tauri runs it
//! on the main thread, and the whole window freezes instead.
//!
//! So every call that can raise a prompt goes through [`while_prompting`] from
//! an `async` caller, and `every_prompt_call_waits_off_the_runtime` fails the
//! build for one that does not.

/// Run `f` — which may wait on a privileged prompt — without holding the async
/// runtime worker it was called on.
///
/// On a multi-threaded runtime this is `block_in_place`: the worker's queued
/// tasks move to another thread while `f` waits. Anywhere else (no runtime, a
/// current-thread runtime, a blocking-pool thread) it simply runs `f`, which is
/// already the right thing there and where `block_in_place` would panic.
pub fn while_prompting<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// **The wait leaves the worker free.** One worker, a task queued on it, and
    /// a blocking wait on that same worker for the queued task's message. Without
    /// the hand-off the queued task can never run — the only thread that could run
    /// it is the one waiting — and the wait times out.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn a_prompt_wait_leaves_the_runtime_worker_free_for_other_tasks() {
        let outcome = tokio::spawn(async {
            let (tx, rx) = std::sync::mpsc::channel();
            tokio::spawn(async move {
                let _ = tx.send(());
            });
            while_prompting(|| rx.recv_timeout(Duration::from_secs(2)))
        })
        .await
        .expect("the waiting task");
        assert!(
            outcome.is_ok(),
            "the queued task never ran while this one waited — the wait held the only worker"
        );
    }

    #[test]
    fn outside_a_runtime_it_simply_runs() {
        assert_eq!(while_prompting(|| 7), 7);
    }

    #[tokio::test]
    async fn on_a_current_thread_runtime_it_runs_instead_of_panicking() {
        assert_eq!(while_prompting(|| 7), 7);
    }

    /// Every function that raises a privileged prompt, as (module, fn). The first
    /// check below DERIVES the ones that call the platform primitive directly and
    /// fails when one is missing here; the rest are their in-module wrappers.
    const PROMPTING: &[(&str, &str)] = &[
        ("cli", "install"),
        ("dns", "configure_resolver"),
        ("dns", "ensure_resolver"),
        ("dns", "take_over_resolver"),
        ("dns", "hand_back_resolver"),
        ("dns", "remove_resolver"),
        ("proxy", "start_privileged"),
        ("proxy", "start_edge_daemon"),
        ("proxy", "stop_edge_daemon"),
        ("setup", "run_system_setup"),
        ("setup", "run_system_teardown"),
        ("ssl", "trust_ca"),
        ("ssl", "untrust_ca"),
    ];

    struct Func {
        file: String,
        module: String,
        name: String,
        is_async: bool,
        is_command: bool,
        body: String,
    }

    /// Top-level and impl-level functions of one file's production source.
    fn functions(file: &str, module: &str, src: &str) -> Vec<Func> {
        let prod = crate::core::copy_scan::production_source(src);
        let lines: Vec<&str> = prod.lines().collect();
        let mut out: Vec<Func> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let indent = line.len() - line.trim_start().len();
            let t = line.trim_start();
            let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
            let (is_async, t) = match t.strip_prefix("async ") {
                Some(rest) => (true, rest),
                None => (false, t),
            };
            if indent <= 4 {
                if let Some(rest) = t.strip_prefix("fn ") {
                    let name: String =
                        rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                    let is_command =
                        lines[i.saturating_sub(3)..i].iter().any(|l| l.contains("#[tauri::command"));
                    out.push(Func {
                        file: file.to_string(),
                        module: module.to_string(),
                        name,
                        is_async,
                        is_command,
                        body: String::new(),
                    });
                }
            }
            if let Some(f) = out.last_mut() {
                f.body.push_str(line);
                f.body.push('\n');
            }
        }
        out
    }

    /// `name(` at `at`, not as part of a longer identifier, a path or a method.
    fn bare_call_at(body: &str, at: usize) -> bool {
        body[..at].chars().next_back().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '.'))
    }

    /// Inside the parentheses of a `while_prompting(` or `spawn_blocking(` call.
    fn wrapped(body: &str, at: usize) -> bool {
        ["while_prompting(", "spawn_blocking("].iter().any(|w| {
            body[..at].rfind(w).is_some_and(|start| {
                let open = start + w.len() - 1;
                body[open..at].chars().fold(0i32, |d, c| match c {
                    '(' => d + 1,
                    ')' => d - 1,
                    _ => d,
                }) > 0
            })
        })
    }

    /// **Every call that can raise a privileged prompt waits off the runtime,
    /// from an async caller.** Derived, not listed by hand at the call sites: the
    /// prompt primitives are found in `core/`, and every call to a prompting
    /// function anywhere in the app's own source must sit inside
    /// `while_prompting(` (or `spawn_blocking(`) — and never in a sync Tauri
    /// command, which runs on the main thread where no hand-off helps.
    #[test]
    fn every_prompt_call_waits_off_the_runtime() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut funcs: Vec<Func> = Vec::new();
        for dir in ["", "core", "commands", "mcp_server"] {
            for entry in std::fs::read_dir(root.join(dir)).expect("source dir").flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let module = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
                let src = std::fs::read_to_string(&path).expect("read source");
                let file = format!("{dir}/{module}.rs");
                funcs.extend(functions(&file, &module, &src));
            }
        }

        // 1. The primitives, derived: a core function that calls the platform's
        //    prompt directly must be listed.
        let listed = |m: &str, n: &str| PROMPTING.iter().any(|(pm, pn)| *pm == m && *pn == n);
        let mut primitives = 0;
        for f in funcs.iter().filter(|f| f.file.starts_with("core/")) {
            if f.body.contains(".run_privileged(")
                || f.body.contains("cert_trust().trust_ca(")
                || f.body.contains("cert_trust().untrust_ca(")
            {
                primitives += 1;
                assert!(
                    listed(&f.module, &f.name),
                    "core::{}::{} raises a privileged prompt and is not in PROMPTING — list it, so \
                     its callers are held to waiting off the runtime",
                    f.module,
                    f.name
                );
            }
        }
        assert!(primitives >= 8, "found {primitives} prompt primitives — the detection stopped working");

        // 2. Every call to a prompting function, outside the prompting functions
        //    themselves, waits off the runtime from an async caller.
        let mut calls = 0;
        for f in funcs.iter().filter(|f| !(f.file.starts_with("core/") && listed(&f.module, &f.name))) {
            for (m, n) in PROMPTING {
                let qualified = format!("{m}::{n}(");
                let bare = format!("{n}(");
                let mut sites: Vec<usize> = f.body.match_indices(&qualified).map(|(i, _)| i).collect();
                if f.module == *m && f.file.starts_with("core/") {
                    sites.extend(
                        f.body.match_indices(&bare).map(|(i, _)| i).filter(|i| bare_call_at(&f.body, *i)),
                    );
                }
                for at in sites {
                    calls += 1;
                    assert!(
                        wrapped(&f.body, at),
                        "{}: `{}` calls {m}::{n} outside `while_prompting` — the prompt would hold \
                         the runtime worker (and everything queued on it) until the user answers",
                        f.file,
                        f.name
                    );
                    assert!(
                        !f.is_command || f.is_async,
                        "{}: `{}` is a SYNC Tauri command that raises a prompt — Tauri runs it on \
                         the main thread, so the window freezes until the dialog is answered; make \
                         it `async`",
                        f.file,
                        f.name
                    );
                }
            }
        }
        assert!(calls >= 12, "found {calls} prompt call sites — the detection stopped working");
    }
}
