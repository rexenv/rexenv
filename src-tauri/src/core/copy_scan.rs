//! core::copy_scan — test-only support for the must-say copy guards.
//!
//! # Why this is a module and not a helper copied into each guard
//!
//! A copy guard asserts that a sentence a USER SEES is still there. The check
//! therefore has to read what RENDERS, and the one way it reliably goes wrong is
//! that the comment above the constants quotes the very phrases while explaining
//! that they are load-bearing — so the guard reads its own explanation and
//! passes with the copy deleted. That is not a hypothetical: it is how the #235
//! Build-zip guard first shipped, and it happened AGAIN in #228's source scan a
//! week later, in a guard written while citing #235. Two independent
//! rediscoveries of the same defect is the signal that the stripper belongs in
//! one place with the lesson attached, rather than being re-derived by whoever
//! writes the third one.
//!
//! [`production_lines`] makes the argument sharper, and it is the reason this
//! module exists rather than a note in a doc: **a correct implementation with a
//! warning written beside it did not stop the same mistake three times.**
//! `mcp_server/scratch.rs` already had the brace-depth version AND a comment
//! naming precisely the bug — that a test module can sit mid-file, so cutting at
//! the first `#[cfg(test)]` silently stops covering the rest — and the next two
//! scanners went naive anyway, one of them written while citing the other
//! defect.
//!
//! **The sharpest instance came later, 14 Aug 2026, and it is the one to quote:**
//! a guard was placed at path CONSTRUCTION rather than at the point of USE
//! (`proxy::admin_socket_path` vs `start_privileged`), turning a working example
//! into a hard failure. The lesson "a guard belongs where the thing is used" had
//! been WRITTEN UP THAT SAME DAY, by the same author, one commit earlier — the
//! Apache/FrankenPHP dotfile legs exist because a guard proven as a string had
//! never met the server that reads it. So the note did not fail because nobody
//! had read it; it failed on the person who wrote it, within an hour. Writing a
//! lesson down does not install it. What caught it was INTEGRATION — migrating a
//! real caller onto the new code in the same session — not the note, and not
//! review.
//!
//! Extraction is what documentation could not do here: the right code
//! and its explanation were both already in the repo, in the file the third
//! guard was written to scan.
//!
//! # The two rules a caller still owns
//!
//! Stripping is only half. Every guard using this must also assert, in both
//! directions, that the stripper worked:
//!
//! - a landmark that is CODE still survives (else every `contains` check below
//!   passes vacuously on an empty string), and
//! - a phrase that exists only in a COMMENT is gone (else prose can satisfy the
//!   guard again).
//!
//! The second canary has a trap of its own, found in #228: a phrase written as a
//! string literal in the test survives stripping as code, so the canary ends up
//! proving its own presence and nothing about the stripper. Assemble it at
//! runtime, or take it from a comment in the file being scanned.

/// A Rust file's PRODUCTION lines (1-indexed), with every `#[cfg(test)]` module
/// removed **by brace depth** rather than by cutting to end-of-file.
///
/// The naive version — `src.split("#[cfg(test)]").next()` — is wrong and fails
/// in the direction that hides things: a test module can sit ANYWHERE in a file,
/// and in `mcp_server/scratch.rs` it sits in the middle, so cutting at the first
/// occurrence silently drops 900 lines of production code while the guard keeps
/// passing. Both #228's spawn guards and #301's tell guard were written with the
/// naive split; the second one failed loudly on the first run, which is the only
/// reason this exists rather than a fourth copy.
pub(crate) fn production_lines(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut depth: Option<i32> = None;
    for (i, line) in src.lines().enumerate() {
        match depth.as_mut() {
            // A one-line item under the attribute — `#[cfg(test)] pub(crate)
            // mod copy_scan;` — has no brace to close, and the first version
            // waited for a `}` that never came: everything after line 15 of
            // `core/mod.rs` was dropped from every tree-wide scan. An item
            // that ends in `;` with no `{` is over on its own line.
            None if line.trim_start().starts_with("#[cfg(test)]") => {
                let rest = line.trim_start().trim_start_matches("#[cfg(test)]");
                if !rest.contains(';') || rest.contains('{') {
                    depth = Some(0);
                }
            }
            Some(0) if line.contains(';') && !line.contains('{') && !line.contains('}') => {
                // The item after the attribute was a one-liner on its own line.
                depth = None;
            }
            None => out.push((i + 1, line)),
            Some(d) => {
                *d += line.matches('{').count() as i32 - line.matches('}').count() as i32;
                if *d <= 0 && line.contains('}') {
                    depth = None;
                }
            }
        }
    }
    out
}

/// [`production_lines`] rejoined — for guards that scan text rather than report
/// a line number. Callers must still assert a code landmark survives, or an
/// empty result makes every `contains` check pass.
pub(crate) fn production_source(src: &str) -> String {
    production_lines(src).into_iter().map(|(_, l)| l).collect::<Vec<_>>().join("\n")
}

/// Drop `/* … */` blocks and whole-line `//` / ` * ` comments from TS/TSX,
/// leaving code and string literals. Deliberately conservative: it never touches
/// a `//` that appears mid-line, so a URL inside a string survives.
pub(crate) fn strip_ts_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut depth = 0usize;
    for line in src.lines() {
        let t = line.trim_start();
        if depth == 0 && (t.starts_with("//") || t.starts_with('*')) {
            continue;
        }
        let mut rest = line;
        let mut kept = String::new();
        while !rest.is_empty() {
            if depth > 0 {
                match rest.find("*/") {
                    Some(i) => {
                        depth -= 1;
                        rest = &rest[i + 2..];
                    }
                    None => {
                        rest = "";
                    }
                }
            } else {
                match rest.find("/*") {
                    Some(i) => {
                        kept.push_str(&rest[..i]);
                        depth += 1;
                        rest = &rest[i + 2..];
                    }
                    None => {
                        kept.push_str(rest);
                        rest = "";
                    }
                }
            }
        }
        out.push_str(&kept);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_removes_prose_and_keeps_the_strings_a_guard_reads() {
        let src = r#"
// LOAD-BEARING: "Nothing is written into the checkout" explains why this is safe.
/* block prose mentioning "still work in rexenv's terminal" */
const TITLE = "Nothing is written into the checkout";
const LINK = "https://example.test/a//b";
"#;
        let out = strip_ts_comments(src);
        assert!(out.contains(r#"const TITLE = "Nothing is written into the checkout""#));
        assert!(!out.contains("LOAD-BEARING"));
        assert!(!out.contains("block prose"));
        // A mid-line `//` inside a string is not a comment.
        assert!(out.contains("https://example.test/a//b"));
    }

    /// **Every registered Tauri command is reachable from something.**
    ///
    /// `generate_handler!` is the app's whole IPC surface. A command listed
    /// there that nothing calls is not inert: it is an entry point a same-user
    /// process can invoke through the webview bridge, carrying whatever
    /// privileges the command has, with no UI, no CLI verb and no reviewer
    /// watching it. It is also the shape `every_ipc_wrapper_is_actually_called`
    /// found from the other side — a door built and left shut — which cost a
    /// user the PHP-update button for weeks.
    ///
    /// Reachable means: the UI can invoke it (its name appears in the ipc
    /// module), `rex` dispatches it, or the MCP server does. Three callers, one
    /// question — is there any way to get here.
    #[test]
    fn every_registered_command_is_reachable_from_a_caller() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("lib.rs");
        let handler = lib
            .split("tauri::generate_handler![")
            .nth(1)
            .and_then(|b| b.split("])").next())
            .expect("the invoke_handler list");

        // `commands::<module>::<name>,` — the only form the list uses.
        let mut registered: Vec<String> = Vec::new();
        for line in handler.lines() {
            let t = line.trim().trim_end_matches(',');
            if let Some(name) = t.strip_prefix("commands::").and_then(|r| r.rsplit("::").next()) {
                if !name.is_empty() && !name.contains(' ') {
                    registered.push(name.to_string());
                }
            }
        }
        assert!(
            registered.len() > 150,
            "only {} commands parsed from the handler — the scan is broken",
            registered.len()
        );

        let mut callers = String::new();
        for rel in ["../src/lib/ipc/index.ts", "src/cli_server.rs", "src/mcp_server.rs"] {
            callers.push_str(&std::fs::read_to_string(root.join(rel)).unwrap_or_default());
        }
        // The MCP server's tool modules.
        if let Ok(entries) = std::fs::read_dir(root.join("src/mcp_server")) {
            for e in entries.flatten() {
                callers.push_str(&std::fs::read_to_string(e.path()).unwrap_or_default());
            }
        }
        assert!(callers.len() > 50_000, "the caller corpus is too small — a path is wrong");

        let unreachable: Vec<&String> = registered
            .iter()
            .filter(|name| {
                // The NAME as a string (the UI invokes by name) or as an
                // identifier (the CLI and MCP call the fn). Either is a way in.
                !callers.contains(&format!("\"{name}\"")) && !callers.contains(name.as_str())
            })
            .collect();
        assert!(
            unreachable.is_empty(),
            "these commands are registered and nothing can reach them: {unreachable:?}\nA command \
             on the bridge with no caller is an entry point with no UI, no CLI verb and no \
             reviewer — delete it, or wire the thing that was supposed to use it"
        );
    }

    /// **Every test the ledger CITES must exist.**
    ///
    /// The ledger's verdict column is the project's evidence index: rows say
    /// "✅ `some_test_name`" and readers — including the next person deciding
    /// whether a claim is covered — take that as proof. A citation naming a test
    /// that was renamed, moved, or never written is worse than an empty verdict,
    /// because it stops anyone looking further. This repo has already found two
    /// rows whose 🔨 was stale in the other direction (#24, #333); this is the
    /// same rot pointing the other way.
    ///
    /// Test-shaped means: backticked, lowercase, and three or more underscores —
    /// this project's tests are sentences. Identifiers that merely look like
    /// that (database names, WordPress functions, settings keys, PHP hooks) are
    /// listed below with what they actually are.
    #[test]
    fn every_test_the_ledger_cites_exists() {
        /// Cited names that are NOT tests, each with what it is.
        const NOT_A_TEST: &[(&str, &str)] = &[
            ("action_scheduler_run_queue", "a WordPress cron HOOK, quoted as fixture data"),
            ("wp_set_auth_cookie", "a WordPress core function"),
            ("wp_set_current_user", "a WordPress core function"),
            ("php_update_manifest_serial", "a settings KEY"),
            ("rex_ro_agentprobe_rex", "a database name from a live run"),
            ("rex_ro_photocontest_test", "a database name from a live run"),
            ("rex_agent_mailfix_scratch_rex", "a database name from a live run"),
            ("tray_lifetime_check", "the L1 example that was measured AWAY, cited as the plan not written (#436)"),
        ];

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let ledger = std::fs::read_to_string(root.join("../docs/CLAIM-LEDGER.md"))
            .expect("the ledger must exist — this guard is about it");

        // Everything that can BE the evidence: a Rust test fn in either crate,
        // an example (L1), or a wk-check (L2).
        let mut known: Vec<String> = Vec::new();
        fn walk(dir: &std::path::Path, known: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let p = e.path();
                if p.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&p, known);
                    continue;
                }
                match p.extension().and_then(|x| x.to_str()) {
                    Some("rs") => {
                        if let Ok(src) = std::fs::read_to_string(&p) {
                            for (i, _) in src.match_indices("fn ") {
                                let rest = &src[i + 3..];
                                if let Some(name) = rest.split(['(', '<', ' ']).next() {
                                    if !name.is_empty() {
                                        known.push(name.to_string());
                                    }
                                }
                            }
                        }
                        if p.components().any(|c| c.as_os_str() == "examples") {
                            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                                known.push(stem.to_string());
                            }
                        }
                    }
                    Some("js") if p.components().any(|c| c.as_os_str() == "wk-checks") => {
                        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                            known.push(stem.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        for dir in ["src", "examples", "../cli/src", "../scripts/wk-checks"] {
            walk(&root.join(dir), &mut known);
        }
        assert!(known.len() > 500, "only {} names found — the walk is broken", known.len());

        let mut missing: Vec<(String, String)> = Vec::new();
        for line in ledger.lines().filter(|l| l.starts_with("| ")) {
            let row = line.split('|').nth(1).unwrap_or("?").trim().to_string();
            let verdict = line.rsplit(" | ").next().unwrap_or_default();
            for cite in verdict.split('`').skip(1).step_by(2) {
                // Three underscores, OR the `_check` suffix every live-check
                // example carries: `tray_lifetime_check` — cited by a row for
                // an example that was never written — has two underscores and
                // slipped through the count alone. (A plain two-underscore
                // rule flags thirteen real identifiers: std functions, MCP
                // tool names, ini keys.)
                let looks_like_a_test = (cite.matches('_').count() >= 3 || cite.ends_with("_check"))
                    && cite
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
                if !looks_like_a_test || NOT_A_TEST.iter().any(|(n, _)| *n == cite) {
                    continue;
                }
                // Prefix match: a row may cite a test by a shortened name when
                // the full one is a paragraph. It must still be a real prefix of
                // something that exists.
                if !known.iter().any(|k| k.starts_with(cite)) {
                    missing.push((row.clone(), cite.to_string()));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "the ledger cites tests that do not exist:\n{}\nA citation naming a renamed, moved \
             or never-written test is worse than an empty verdict — it stops the next reader \
             looking further. Fix the row to name what actually holds the claim, or add the \
             identifier to NOT_A_TEST saying what it really is.",
            missing
                .iter()
                .map(|(r, c)| format!("  row #{r}: `{c}`"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// #262 — **the preferred browser is applied at ONE choke point.**
    ///
    /// rexenv opens links from about a dozen places. The routing — preference,
    /// still-installed check, fall back to the OS handler — lives in the backend
    /// behind `open_external`, and the UI's job is to call it. A component that
    /// reached for `openInBrowser` instead would work, and then the NEXT call
    /// site anyone adds silently opens in the system default while the setting
    /// says otherwise: the whole-surface claim that checks one place inside the
    /// surface, which this ledger already records more than once.
    ///
    /// The one legitimate caller is the chevron menu's explicit pick — "open
    /// this link in THAT browser" is not the default action and must not
    /// consult the preference — and it is centralised in `lib/useBrowser.ts`
    /// (`openUrlIn`), which is what this allows.
    #[test]
    fn only_the_explicit_browser_pick_bypasses_the_open_choke_point() {
        /// Files allowed to name `openInBrowser`, with the reason.
        const ALLOWED: &[(&str, &str)] = &[
            ("lib/ipc/index.ts", "the wrapper itself"),
            (
                "lib/useBrowser.ts",
                "`openUrlIn` — the chevron menu's EXPLICIT pick, which is not the default \
                 action and deliberately does not touch the preference",
            ),
        ];

        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>, root: &std::path::Path) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let p = e.path();
                if p.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&p, out, root);
                    continue;
                }
                let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
                if ext != "ts" && ext != "tsx" {
                    continue;
                }
                if let Ok(raw) = std::fs::read_to_string(&p) {
                    let rel = p.strip_prefix(root).unwrap_or(&p).display().to_string();
                    out.push((rel, strip_ts_comments(&raw)));
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        let mut files = Vec::new();
        walk(&root, &mut files, &root);
        assert!(files.len() > 20, "only {} frontend files scanned — the walk is broken", files.len());

        // Whole identifier, not a prefix: `openInBrowserMoved` contains
        // `openInBrowser`, so a plain `contains` reported the allowed file as
        // still holding the call after it had been renamed away — the
        // stale-exception half of this guard was passing on nothing.
        let names_it = |src: &str| {
            src.match_indices("openInBrowser").any(|(i, _)| {
                let after = src[i + "openInBrowser".len()..].chars().next().unwrap_or(' ');
                !after.is_alphanumeric() && after != '_'
            })
        };
        let mut offenders: Vec<String> = Vec::new();
        let mut allowed_seen = 0usize;
        for (rel, src) in &files {
            if !names_it(src) {
                continue;
            }
            match ALLOWED.iter().any(|(f, _)| f == rel) {
                true => allowed_seen += 1,
                false => offenders.push(rel.clone()),
            }
        }
        assert!(
            offenders.is_empty(),
            "these files call `openInBrowser` directly: {offenders:?}\nThe default open action \
             goes through `openExternal` so the preference, the still-installed re-check and the \
             OS fallback happen in ONE place — a component that routes for itself means the next \
             call site anyone adds opens somewhere the setting did not choose"
        );
        assert_eq!(
            allowed_seen,
            ALLOWED.len(),
            "one of the allowed files no longer mentions `openInBrowser` — if the explicit-pick \
             path moved, move this exception with it rather than leaving a list that has \
             stopped being true"
        );
        // …and the choke point is still WIDELY used, or the rule above is
        // satisfied by a UI that opens nothing at all.
        let external_callers = files
            .iter()
            .filter(|(rel, src)| rel.as_str() != "lib/ipc/index.ts" && src.contains("openExternal("))
            .count();
        assert!(
            external_callers >= 4,
            "only {external_callers} files call `openExternal` — either the UI stopped opening \
             links, or the routing moved and this guard is now watching an empty rule"
        );
    }

    /// **Every IPC wrapper is actually CALLED somewhere.**
    ///
    /// `src/lib/ipc/` is the only door between the UI and the backend, and an
    /// exported wrapper nothing calls is a feature that cannot happen. That is not
    /// hypothetical: `phpUpdateCheck` shipped as a Tauri command, a Rust
    /// implementation, an IPC wrapper and a rendered button — and **nothing ever
    /// called it**, so the manifest was never fetched, the catalog stayed empty,
    /// and the Update button could not appear for anyone. Every layer existed and
    /// the chain had a hole in the middle. Found by a user asking why the button
    /// was missing, which is the worst way to find it.
    ///
    /// Same family as the `probeFor()` dispatch this repo tripped over the same
    /// day: the thing was written, and the one line that reaches it was not.
    #[test]
    fn every_ipc_wrapper_is_actually_called() {
        // Wrappers with no caller today, each with the reason. This list is
        // allowed to SHRINK, never to grow silently: a new entry means someone
        // built a door and left it shut.
        const UNCALLED: &[(&str, &str)] = &[
            // `createSite` was here until 21 Aug 2026 and is DELETED, not exempted:
            // "superseded by the job-based provision flow" had stopped being a
            // temporary state. An exemption that outlives the thing it was waiting
            // for is how this list grows silently, which the comment above forbids.
            // `wpThemeEnableNetwork`/`wpThemeDisableNetwork` were here until
            // 23 Aug 2026 and are DELETED, not exempted: the Network tab's
            // "Themes (network)" card calls both. Worth recording WHY they sat
            // shut for so long, because it was not laziness — `wp theme list`
            // has no field for network-enabled state (theme status is only
            // active/parent/inactive, with no `active-network` the way plugins
            // have), so there was nothing to render a toggle's current position
            // from. The missing piece was a read, not a button.
        ];

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        let ipc = std::fs::read_to_string(root.join("lib/ipc/index.ts"))
            .expect("the ipc module must exist — this guard is about it");

        // Exported wrappers, by their declaration form.
        let mut names: Vec<&str> = Vec::new();
        for line in ipc.lines() {
            let t = line.trim();
            for pre in ["export async function ", "export function "] {
                if let Some(rest) = t.strip_prefix(pre) {
                    if let Some(n) = rest.split(['(', '<']).next() {
                        if !n.is_empty() {
                            names.push(n);
                        }
                    }
                }
            }
        }
        assert!(names.len() > 50, "only {} wrappers parsed — the scan is broken", names.len());

        /// Drop `import … from "…"` statements.
        ///
        /// **Without this the guard proved the wrong thing.** It matched a
        /// whole-identifier occurrence anywhere outside the ipc module — and an
        /// IMPORT is such an occurrence, so a wrapper that was imported and then
        /// never used counted as called. That is not a corner case: it is the
        /// exact end-state of deleting the one line that used something, which
        /// is the defect this test is named after.
        ///
        /// Found 23 Aug 2026 by planting it — removing the two calls the new
        /// network-themes card makes, and watching the test stay green because
        /// the imports were still at the top of the file.
        ///
        /// Imports are dropped rather than requiring a following `(` so that a
        /// wrapper passed as a VALUE (`mutationFn: wpFoo`) still counts. Being
        /// referenced without being invoked is a real use; being named in an
        /// import list is not.
        fn strip_imports(src: &str) -> String {
            let mut out = String::with_capacity(src.len());
            let mut in_import = false;
            for line in src.lines() {
                let t = line.trim_start();
                if !in_import && (t == "import" || t.starts_with("import ")) {
                    // Single-line unless the `from` clause has not arrived yet.
                    in_import = !(t.contains(" from ") || t.ends_with(';'));
                    continue;
                }
                if in_import {
                    if t.contains(" from ") || t.ends_with(';') {
                        in_import = false;
                    }
                    continue;
                }
                out.push_str(line);
                out.push('\n');
            }
            out
        }

        // Every other .ts/.tsx file, comments stripped so a wrapper merely
        // MENTIONED in prose does not count as called, and imports stripped so
        // one merely IMPORTED does not either.
        fn walk(dir: &std::path::Path, skip: &std::path::Path, out: &mut String) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let p = e.path();
                if p.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&p, skip, out);
                    continue;
                }
                if p == skip {
                    continue;
                }
                let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
                if ext != "ts" && ext != "tsx" {
                    continue;
                }
                if let Ok(raw) = std::fs::read_to_string(&p) {
                    out.push_str(&strip_imports(&strip_ts_comments(&raw)));
                    out.push('\n');
                }
            }
        }
        let mut callers = String::new();
        walk(&root, &root.join("lib/ipc/index.ts"), &mut callers);

        let mut orphans: Vec<&str> = Vec::new();
        for n in &names {
            let called = callers
                .match_indices(n)
                .any(|(i, _)| {
                    // A whole identifier, not a prefix of a longer one.
                    let after = callers[i + n.len()..].chars().next().unwrap_or(' ');
                    let before = callers[..i].chars().last().unwrap_or(' ');
                    !after.is_alphanumeric() && after != '_'
                        && !before.is_alphanumeric() && before != '_'
                });
            if !called && !UNCALLED.iter().any(|(u, _)| u == n) {
                orphans.push(n);
            }
        }
        assert!(
            orphans.is_empty(),
            "these IPC wrappers are exported and NEVER called — a feature that cannot happen:\n  {}\n\n             Wire the call, or add it to UNCALLED with the reason. `phpUpdateCheck` shipped this \
             way: command, implementation, wrapper and button all present, and the one line that \
             reaches it missing, so the Update button could not appear for anyone.",
            orphans.join("\n  ")
        );
        // The exemption list must not rot either: an entry that IS called now
        // should be removed, or it hides the next real one.
        for (u, why) in UNCALLED {
            assert!(names.contains(u), "UNCALLED names `{u}`, which is not an ipc wrapper ({why})");
        }
    }

    /// **The app-update card never promises what it cannot measure.**
    ///
    /// A sibling of the PHP guard below, scoped to one file and therefore
    /// stricter: that one bans the phrases only on lines about PHP, because
    /// "update available" is legitimate elsewhere (WordPress genuinely has an
    /// updater). Here the whole file is about rexenv's own updates, so the
    /// phrases are banned outright.
    ///
    /// - **"up to date"** is unprovable before a check has ever succeeded, and
    ///   only ever true of the instant the check ran. The card says when it last
    ///   looked instead.
    /// - **"update available"** is the phrase this project already banned once
    ///   for promising what no button could deliver. Here a button DOES exist,
    ///   which makes the phrase tempting and still wrong: it says nothing about
    ///   whether this Mac can install it, and the refusals exist precisely
    ///   because sometimes it cannot.
    ///
    /// The positive half matters as much: the file must still SAY when it last
    /// checked and still offer an Install, or the ban could be satisfied by a
    /// card that says nothing at all.
    #[test]
    fn the_app_update_card_never_promises_what_it_cannot_measure() {
        const BANNED: &[&str] =
            &["update available", "updates available", "up to date", "up-to-date"];
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/components/settings/AppUpdateCard.tsx");
        let raw = std::fs::read_to_string(&path).expect("the update card exists");
        let text = strip_ts_comments(&raw).to_ascii_lowercase();
        assert!(text.len() > 500, "the card was emptied or moved — this guard now proves nothing");
        for b in BANNED {
            assert!(
                !text.contains(b),
                "AppUpdateCard.tsx says {b:?} — it cannot know that, and the footer's \
                 'checked N ago' is the honest form"
            );
        }
        for must in ["checked ", "install rexenv", "couldn't reach"] {
            assert!(
                text.contains(must),
                "AppUpdateCard.tsx no longer says {must:?} — the ban must not be satisfied \
                 by a card that says nothing"
            );
        }
    }

    /// **The New-site dialog renders core's PostgreSQL rules, it does not
    /// restate them.**
    ///
    /// Two conditions decide whether a site may be PostgreSQL-backed — the site
    /// type (`wpdb` speaks MySQL alone) and whether the chosen PHP's build has a
    /// working `pdo_pgsql` — and both live in `sites::ensure_engine_supports`.
    /// The second one MOVES: 7.4 and 8.0 have no driver, every minor rexenv
    /// builds itself does, and the day `rexenv/runtimes` publishes an 8.0 with it
    /// that set changes again. A list of minors written into the dialog would be
    /// a second copy free to disagree — the exact shape that made the Xdebug
    /// toggle hardcode `minor === "8.0"` (ledger #378) — and the copy that moves
    /// last is the one a user meets, as an option that produces an error.
    ///
    /// So: the dialog must READ `postgresSupported`, and must not compare a PHP
    /// version to a literal anywhere near the engine choice.
    /// **The cost of an update is core's sentence, not the row's.**
    ///
    /// An offer that REMOVES a capability looks exactly like one that does not —
    /// a version number is a version number. The rule behind it (a patch from
    /// static-php.dev has no PostgreSQL driver, so moving a minor rexenv builds
    /// onto upstream's newer patch takes it away from every site on that version)
    /// is not obvious enough to restate in a component, and a restatement is free
    /// to drift the day the rule changes — which it will, every time a release
    /// adds a version rexenv builds.
    #[test]
    fn the_update_cost_sentence_lives_in_core() {
        let core = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core/php.rs"),
        )
        .expect("core/php.rs");
        assert!(
            core.contains("cannot reach PostgreSQL"),
            "core no longer writes the cost sentence — if the wording moved, move this guard"
        );

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/routes/Settings.tsx");
        let raw = std::fs::read_to_string(&path).expect("the Settings route exists");
        let text = strip_ts_comments(&raw);
        assert!(text.len() > 1000, "Settings.tsx was emptied or moved");
        assert!(
            text.contains("v.updateCost"),
            "the row no longer renders core's sentence — the tell is the whole point"
        );
        // The banned shape: the component deciding, or restating, the RULE.
        for banned in ["pdo_pgsql", "upstream's build", "no PostgreSQL driver"] {
            assert!(
                !text.contains(banned),
                "Settings.tsx restates the update-cost rule ({banned:?}) — one source, in core"
            );
        }
    }

    /// **A Local import's panel never says the site still reads its old
    /// database** (ledger #575). That sentence is true of a Valet/Herd import
    /// and false of a Local one, whose config reaches nothing under rexenv — two
    /// real Local imports served WordPress's database error while the job log
    /// and the panel both said "still reads its old database". The panel reads
    /// the backend's `oldDatabaseUnreachable`; the Sites badge, which has no
    /// preview to read, asserts neither.
    #[test]
    fn the_import_panel_never_claims_a_local_site_reads_its_old_database() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        let card = strip_ts_comments(
            &std::fs::read_to_string(dir.join("components/sites/DbImportCard.tsx")).expect("the card exists"),
        );
        assert!(card.len() > 1000, "the card was emptied or moved — this guard proves nothing");
        let still = card
            .find("This site still reads and writes the old database")
            .expect("the Valet/Herd sentence is gone — if it was reworded, move this guard");
        assert!(
            card[still.saturating_sub(200)..still].contains("!preview.oldDatabaseUnreachable"),
            "the old-database sentence is no longer gated on the backend's Local fact"
        );
        assert!(
            card.contains("This site can't load under rexenv until you connect it"),
            "the Local site's own sentence is gone"
        );
        let sites = strip_ts_comments(
            &std::fs::read_to_string(dir.join("routes/Sites.tsx")).expect("the Sites route exists"),
        );
        assert!(
            !sites.contains("still reads and writes the old one"),
            "the Sites badge asserts what the site reads — it has no preview to know, and for Local it is false"
        );
    }

    #[test]
    fn the_new_site_dialog_reads_postgres_support_rather_than_deciding_it() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/components/sites/NewSiteDialog.tsx");
        let raw = std::fs::read_to_string(&path).expect("the New-site dialog exists");
        let text = strip_ts_comments(&raw);
        assert!(text.len() > 1000, "the dialog was emptied or moved — this guard proves nothing");

        assert!(
            text.contains("postgresSupported"),
            "the dialog no longer reads core's answer — if the option moved, move this guard"
        );
        // The banned shape, in the forms a hand-rolled rule would take. Comments
        // are stripped first: this file's own prose names those versions.
        for banned in ["phpVersion === \"8.", "phpVersion === \"7.", "phpVersion >= \"8."] {
            assert!(
                !text.contains(banned),
                "NewSiteDialog.tsx decides a PHP capability from a version literal ({banned:?}) \
                 — that is a second copy of a core rule, and it will disagree"
            );
        }
        // …and the option itself must still be gated on something, or the ban is
        // satisfied by a dialog that offers PostgreSQL unconditionally.
        assert!(
            text.contains("postgresOffered && <option value=\"postgres\">"),
            "the PostgreSQL option is no longer gated on postgresOffered"
        );
    }

    /// **The consent sentence has ONE source, and it is not the TSX.**
    ///
    /// The sentence in front of the Install button describes what the click
    /// does — it downloads, verifies, swaps, quits and reopens, your sites keep
    /// running, your terminals do not. That is a description of a RULE, and this
    /// project's honest-UI rule says such a sentence lives beside the rule it
    /// describes: a copy in the TSX is a copy that drifts the first time the
    /// behaviour changes, and nothing would fail.
    ///
    /// So the card renders a string Rust sent it, and this checks that the
    /// distinctive phrases exist in `core/app_update.rs` and in no `.tsx` at all.
    #[test]
    fn the_consent_sentence_has_one_source() {
        let core = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core/app_update.rs"),
        )
        .expect("core/app_update.rs");
        // Distinctive fragments — long enough that nothing else would contain
        // them by accident, short enough to survive ordinary rewording.
        let phrases = ["services outlive the app", "close with it", "Apple developer signature"];
        for p in phrases {
            assert!(
                core.contains(p),
                "the consent sentence no longer says {p:?} — if the wording moved, move this \
                 guard with it rather than deleting the promise"
            );
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        let mut files = Vec::new();
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let p = e.path();
                if p.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&p, out);
                } else if p.extension().and_then(|x| x.to_str()) == Some("tsx") {
                    out.push(p);
                }
            }
        }
        walk(&root, &mut files);
        assert!(files.len() > 20, "the tsx walk found {} files", files.len());
        for f in &files {
            let Ok(raw) = std::fs::read_to_string(f) else { continue };
            let text = strip_ts_comments(&raw);
            for p in phrases {
                assert!(
                    !text.contains(p),
                    "{} spells out the consent sentence — it must render the one Rust sends",
                    f.display()
                );
            }
        }
    }

    /// **The PHP version rows may say a newer patch EXISTS; they may never say
    /// one is AVAILABLE, or that this build is UP TO DATE.**
    ///
    /// rexenv installs pinned builds from static-php.dev; php.net is the source
    /// of the "newer exists" fact. **The two disagree by weeks.** Measured 16
    /// Aug 2026: php.net listed 8.4.24 and 8.5.9 while static-php.dev's newest
    /// were 8.4.23 and 8.5.8 — exactly rexenv's pins. So for those minors a
    /// newer version genuinely exists and rexenv cannot ship it.
    ///
    /// "8.4.24 exists · this build pins 8.4.23" survives that. "Update
    /// available" does not — it promises something no button can deliver, and
    /// there is deliberately no button (`docs/archive/PLAN-binary-updates.md` §12/§13).
    /// "Up to date" is worse in the other direction: unprovable before the
    /// first successful check, and false whenever static-php lags.
    ///
    /// The wording is the entire mechanism by which a read-only check stays
    /// honest, so it is guarded rather than remembered.
    #[test]
    fn the_version_rows_never_promise_an_update_they_cannot_deliver() {
        const BANNED: &[&str] = &["update available", "up to date", "up-to-date", "updates available"];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        let mut offences: Vec<String> = Vec::new();

        fn walk(dir: &std::path::Path, banned: &[&str], out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&path, banned, out);
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "tsx" && ext != "ts" {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&path) else { continue };
                // Comments stripped, so the prose EXPLAINING the ban does not
                // trip it — the scanner-reads-its-own-explanation shape this
                // module already records twice.
                let text = strip_ts_comments(&raw).to_ascii_lowercase();
                for (i, line) in text.lines().enumerate() {
                    // Only where a PHP version is being described. The words are
                    // fine elsewhere (WordPress core genuinely has an updater).
                    if !line.contains("php") && !line.contains("upstream") && !line.contains("patch")
                    {
                        continue;
                    }
                    for b in banned {
                        if line.contains(b) {
                            out.push(format!("{}:{} — {b}", path.display(), i + 1));
                        }
                    }
                }
            }
        }
        walk(&root, BANNED, &mut offences);
        assert!(
            offences.is_empty(),
            "a PHP version row promises an update rexenv cannot deliver:\n{}",
            offences.join("\n")
        );

        // …and the honest phrasing is actually present, so this cannot pass by
        // the feature having been deleted.
        let settings = std::fs::read_to_string(root.join("routes/Settings.tsx")).unwrap();
        let settings = strip_ts_comments(&settings);
        assert!(
            settings.contains("exists"),
            "the 'newer patch exists' line is gone — either restore it or delete this guard"
        );
    }

    /// A Tailwind class name built by interpolation is never generated, because
    /// the scanner reads SOURCE LITERALS — so `mt-${x ? "0" : "3"}` produces no
    /// margin at all and looks exactly like a margin of zero. That is the worst
    /// kind of frontend bug: it cannot be seen, only reasoned about.
    ///
    /// Found once, in `dialog.tsx`, where it had been harmless by luck — `mt-3`
    /// existed because another file used it, and `mt-0` never existed but its
    /// absence happens to look right. The next one will not be lucky.
    ///
    /// This lives in `copy_scan` because this module already owns "reading the
    /// frontend source from a Rust test"; it is a build-mechanism lint rather
    /// than a copy guard, and there is no other module that owns the fact.
    #[test]
    fn no_tailwind_class_name_is_built_by_interpolation() {
        /// Every `className={…}` expression in `src`, brace-matched.
        ///
        /// A line WINDOW was tried first and is wrong in both directions: it
        /// missed an interpolation a `cn(` call put on the next line, and it
        /// flagged `example={`site1.${domain}`}` several lines below an
        /// unrelated className. Scoping to the actual expression is the only
        /// version that means what the test's name says.
        fn class_spans(text: &str) -> Vec<(usize, String)> {
            let chars: Vec<char> = text.chars().collect();
            let mut out = Vec::new();
            let mut i = 0usize;
            let needle: Vec<char> = "className=".chars().collect();
            while i + needle.len() < chars.len() {
                if chars[i..i + needle.len()] != needle[..] {
                    i += 1;
                    continue;
                }
                let mut j = i + needle.len();
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j >= chars.len() || chars[j] != '{' {
                    // `className="…"` — a plain literal, nothing to interpolate.
                    i = j.max(i + 1);
                    continue;
                }
                let span_start = j;
                let mut depth = 0i32;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let line = text[..text
                    .char_indices()
                    .nth(span_start)
                    .map(|(b, _)| b)
                    .unwrap_or(0)]
                    .matches('\n')
                    .count()
                    + 1;
                out.push((line, chars[span_start..=j.min(chars.len() - 1)].iter().collect()));
                i = j.max(i + 1);
            }
            out
        }

        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&path, out);
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "tsx" && ext != "ts" {
                    continue;
                }
                // Comments stripped with this module's own function, because the
                // FIRST run flagged the comment above the fix that explains the
                // pattern. A guard reading its own explanation is now the fourth
                // instance here, and the whole argument for one stripper.
                let Ok(raw) = std::fs::read_to_string(&path) else { continue };
                let text = strip_ts_comments(&raw);
                for (line, span) in class_spans(&text) {
                    let chars: Vec<char> = span.chars().collect();
                    for w in 0..chars.len().saturating_sub(1) {
                        if chars[w] != '$' || chars[w + 1] != '{' {
                            continue;
                        }
                        // THE RULE: the character before `${` must be
                        // whitespace or a delimiter. A whole class interpolated
                        // in (`… ${color}`) is fine — the variable holds a
                        // complete literal the scanner finds where it is
                        // defined. A PARTIAL name is not, and it has more shapes
                        // than a `-${` check knows: `mt-${x}`, `text-[${n}]` and
                        // `hover:${c}` are all invisible the same way, which a
                        // plant found. The rule is the boundary, not the hyphen.
                        let before = if w == 0 { ' ' } else { chars[w - 1] };
                        if !before.is_whitespace()
                            && !matches!(before, '`' | '{' | '(' | ',')
                        {
                            out.push(format!(
                                "  {}:{line}  …{}…",
                                path.display(),
                                span.chars()
                                    .skip(w.saturating_sub(24))
                                    .take(40)
                                    .collect::<String>()
                                    .replace('\n', " ")
                            ));
                            break;
                        }
                    }
                }
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("src");
        assert!(root.is_dir(), "the frontend source is not where this expects it");
        let mut hits = Vec::new();
        walk(&root, &mut hits);
        assert!(
            hits.is_empty(),
            "a Tailwind class name is built by interpolation, so it is never generated and \
             silently does nothing — the class is simply absent, which looks identical to a \
             value of zero:\n{}",
            hits.join("\n")
        );
    }

    // ── Contrast: the design system's one measurable honesty claim ───────────
    //
    // `tokens.css` carried exactly one contrast assertion — `--rex-text-label`'s
    // light-theme comment, "mono section labels (darker: small type needs AA)" —
    // and `docs/DESIGN.md` stated no contrast rule at all, so that lone comment
    // read as the house position. It was true on pure white and false on every
    // other light surface (ledger #337). A comment is not a check; this is.
    //
    // Lives in `copy_scan` for the reason the module doc gives: it already owns
    // "reading the frontend source from a Rust test", and this is a
    // build-mechanism lint of the same shape as the Tailwind-interpolation one
    // above, not a claim about any single component.

    /// A parsed `tokens.css`: the two themes, each `name -> value`, where `name`
    /// is the Tailwind key (`text-dim`) rather than the CSS var (`--rex-text-dim`).
    ///
    /// **Light is dark OVERLAID with the light block, not the light block alone.**
    /// `[data-theme="light"]` redefines only some tokens; everything else keeps
    /// its `:root` value by inheritance. A parser that read the light block on
    /// its own would silently check a fraction of the light theme and pass — the
    /// same defect as cutting a Rust file at the first `#[cfg(test)]`, which is
    /// why that one is named in this module's doc.
    fn parse_tokens() -> (std::collections::BTreeMap<String, String>, std::collections::BTreeMap<String, String>) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("src/styles/tokens.css");
        let css = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

        let mut dark = std::collections::BTreeMap::new();
        let mut light = std::collections::BTreeMap::new();
        // Which block are we in: 0 = none, 1 = :root (dark), 2 = light.
        let mut block = 0u8;
        for line in css.lines() {
            let t = line.trim();
            if t.starts_with(":root") {
                block = 1;
            } else if t.starts_with("[data-theme=\"light\"]") {
                block = 2;
            } else if t == "}" {
                block = 0;
            } else if block != 0 {
                if let Some(rest) = t.strip_prefix("--rex-") {
                    if let Some((name, value)) = rest.split_once(':') {
                        let value = value.split(';').next().unwrap_or("").trim().to_string();
                        let name = name.trim().to_string();
                        if block == 1 {
                            dark.insert(name, value);
                        } else {
                            light.insert(name, value);
                        }
                    }
                }
            }
        }
        // The overlay. Do this AFTER parsing, never by seeding `light` with
        // `dark` up front — seeding would hide a light block that failed to
        // parse at all, because the result would look complete either way.
        let mut merged_light = dark.clone();
        for (k, v) in &light {
            merged_light.insert(k.clone(), v.clone());
        }
        assert!(!light.is_empty(), "the light theme block parsed to nothing — the parser missed it");
        (dark, merged_light)
    }

    /// WCAG 2.1 relative luminance of a `#rrggbb` value, or `None` if the value
    /// is not an opaque hex (an `rgba()` composites over whatever is beneath it,
    /// so it has no standalone contrast to compute).
    fn luminance(value: &str) -> Option<f64> {
        let h = value.strip_prefix('#')?;
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let chan = |i: usize| {
            let c = u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f64 / 255.0;
            if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        Some(0.2126 * chan(0) + 0.7152 * chan(2) + 0.0722 * chan(4))
    }

    /// WCAG contrast ratio, or `None` if either side is not an opaque hex.
    fn contrast(a: &str, b: &str) -> Option<f64> {
        let (la, lb) = (luminance(a)?, luminance(b)?);
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        Some((hi + 0.05) / (lo + 0.05))
    }

    /// Every `text-rex-*` / `bg-rex-*` Tailwind class used in the frontend, as
    /// `(token-name, file, line)`. Comments are stripped with this module's own
    /// stripper, because a guard that reads its own explanation is the defect
    /// this module exists for.
    fn used_rex_classes(prefix: &str) -> Vec<(String, String, usize)> {
        used_rex_classes_classified(prefix).into_iter().map(|(n, f, l, _)| (n, f, l)).collect()
    }

    /// Whether the `className` at `lines[i]` sits on an ICON rather than on text.
    ///
    /// An icon is a capitalised JSX component (lucide) carrying a size or stroke
    /// hint. The `className` is often on its own line, so the opening tag is
    /// found by walking BACK — a same-line-only test would classify every
    /// multi-line element as text and quietly stop exempting anything.
    fn sits_on_an_icon(lines: &[&str], i: usize) -> bool {
        let mut tag = "";
        for j in (i.saturating_sub(12)..=i).rev() {
            if let Some(k) = lines[j].rfind('<') {
                let rest = &lines[j][k + 1..];
                let end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '.')
                    .unwrap_or(rest.len());
                if end > 0 {
                    tag = &rest[..end];
                    break;
                }
            }
        }
        if !tag.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            return false;
        }
        let lo = i.saturating_sub(2);
        // Normalise `"` and `=` to spaces BEFORE tokenising. Without this the
        // first class in an attribute arrives glued to it — `className="h-3.5`
        // does not start with `h-` — and `Mail.tsx`'s <Globe/>, the one icon
        // with no `strokeWidth`, was classified as text. It cost a failing plant
        // to find, which is the argument for planting rather than eyeballing.
        let ctx: String = lines[lo..(i + 2).min(lines.len())]
            .join(" ")
            .replace(['"', '='], " ");
        let sized = |p: &str| {
            ctx.split_whitespace()
                .any(|w| w.starts_with(p) && w.len() <= 7 && w[p.len()..].starts_with(|c: char| c.is_ascii_digit() || c == '['))
        };
        ctx.contains("strokeWidth") || (sized("h-") && sized("w-"))
    }

    fn used_rex_classes_classified(prefix: &str) -> Vec<(String, String, usize, bool)> {
        fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<(String, String, usize, bool)>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&path, prefix, out);
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "tsx" && ext != "ts" {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&path) else { continue };
                let stripped = strip_ts_comments(&raw);
                let all: Vec<&str> = stripped.lines().collect();
                for (n, &line) in all.iter().enumerate() {
                    let mut rest = line;
                    while let Some(i) = rest.find(prefix) {
                        // Must start at a class boundary, or `hover:bg-rex-x`
                        // and `not-a-text-rex-y` would both match loosely.
                        let before = rest[..i].chars().last();
                        let ok = before.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '-');
                        rest = &rest[i + prefix.len()..];
                        let end = rest
                            .find(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-')
                            .unwrap_or(rest.len());
                        let name = rest[..end].trim_end_matches('-').to_string();
                        if ok && !name.is_empty() {
                            let icon = sits_on_an_icon(&all, n);
                            out.push((name, path.display().to_string(), n + 1, icon));
                        }
                    }
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("src");
        let mut out = Vec::new();
        walk(&root, prefix, &mut out);
        out
    }

    /// Token names used as a text colour in RAW CSS (`color: var(--rex-x)`),
    /// which Tailwind never sees. Kept deliberately narrow — `color:` only, in
    /// `src/styles` — because a broader sweep would start guessing at what is
    /// text.
    fn raw_css_text_tokens() -> Vec<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("src/styles");
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else { return out };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("css") {
                continue;
            }
            let Ok(css) = std::fs::read_to_string(&path) else { continue };
            for line in css.lines() {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix("color:") {
                    if let Some(v) = rest.trim().strip_prefix("var(--rex-") {
                        if let Some(name) = v.split(')').next() {
                            out.push(name.to_string());
                        }
                    }
                }
            }
        }
        out
    }

    /// Token names used as a text colour through an INLINE STYLE
    /// (`color: "var(--rex-x)"` in a `.tsx`), which Tailwind never sees either.
    ///
    /// The third route into the same hole. The first was raw CSS
    /// (`::placeholder`, the app's most widespread text, at 2.58:1); the second
    /// was raw Tailwind hues (the light-mode update badge); this is the one the
    /// site-type chips, the service groups and the mail avatars all take —
    /// their colour is a `style` prop because the tint and border travel with
    /// it, so a Tailwind-only scan sees none of them.
    fn inline_style_text_tokens() -> Vec<String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("src");
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&path, out);
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "tsx" && ext != "ts" {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&path) else { continue };
                let stripped = strip_ts_comments(&raw);
                let all: Vec<&str> = stripped.lines().collect();
                for (n, &line) in all.iter().enumerate() {
                    // Icons are excluded the SAME way as for Tailwind classes —
                    // by asking what the colour sits on, never by trusting a
                    // token name. `accent-teal` is a `<Shield>` in Onboarding
                    // and `lock-insecure` is the Tunnels lightbulb; WCAG 1.4.11
                    // asks 3:1 of those, not 1.4.3's 4.5:1. Exempting them BY
                    // NAME is the mistake this file already records paying for.
                    if sits_on_an_icon(&all, n) {
                        continue;
                    }
                    let mut rest = line;
                    // `color:` only — never `background`/`borderColor`, which are
                    // the fill and the rule around it rather than the text.
                    while let Some(i) = rest.find("color:") {
                        // `backgroundColor:` ends in `color:` too. The character
                        // before has to be a boundary, or every tint fill would
                        // be read as a text colour and the check would start
                        // asserting contrast against things nobody reads.
                        let boundary = rest[..i]
                            .chars()
                            .last()
                            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
                        rest = &rest[i + "color:".len()..];
                        if !boundary {
                            continue;
                        }
                        let Some(v) = rest.trim_start().strip_prefix("\"var(--rex-") else { continue };
                        if let Some(name) = v.split(')').next() {
                            out.push(name.to_string());
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&root, &mut out);
        out
    }

    /// The keys Tailwind actually defines under `rex.*` — read from
    /// `tailwind.config.js`, because tokens.css is NOT the authority for what a
    /// `text-rex-*` class resolves to: `brand.*`, `danger.*`, `status.*` and
    /// `toggle.*` are separate families whose tokens also live in tokens.css.
    /// Returns (rex keys, other families' keys → family name).
    fn tailwind_colour_keys() -> (std::collections::BTreeSet<String>, std::collections::BTreeMap<String, String>) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("repo root").join("tailwind.config.js");
        let js = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let mut rex = std::collections::BTreeSet::new();
        let mut others = std::collections::BTreeMap::new();
        let mut family: Option<String> = None;
        let mut depth = 0i32;
        for raw in js.lines() {
            let t = raw.trim();
            if family.is_none() {
                if let Some(name) = t.strip_suffix(": {") {
                    if ["rex", "brand", "danger", "status", "toggle"].contains(&name) {
                        family = Some(name.to_string());
                        depth = 1;
                    }
                }
                continue;
            }
            let fam = family.clone().unwrap();
            if t == "}" || t == "}," {
                depth -= 1;
                if depth == 0 {
                    family = None;
                }
                continue;
            }
            if t.ends_with('{') {
                depth += 1;
                continue;
            }
            if let Some(key) = t.split(':').next() {
                let key = key.trim().trim_matches('"').to_string();
                if key.is_empty() || !key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') { continue; }
                if fam == "rex" { rex.insert(key); } else {
                    let full = if key == "DEFAULT" { fam.clone() } else { format!("{fam}-{key}") };
                    others.insert(full, fam.clone());
                }
            }
        }
        assert!(rex.contains("surface-1") && others.contains_key("brand-active"), "tailwind.config.js parsed to nothing usable");
        (rex, others)
    }

    /// **A `text-rex-*` / `bg-rex-*` / `border-rex-*` class that names no
    /// Tailwind `rex.*` key generates NOTHING — and tokens.css is not the
    /// authority for that.**
    ///
    /// Tailwind emits nothing for a key that is not in the theme, so the element
    /// simply has no colour and inherits — which looks plausible on screen and
    /// is invisible to review. Exactly the failure the interpolation lint above
    /// describes ("the class is simply absent, which looks identical to a value
    /// of zero"), reached by a different route: that one catches names BUILT at
    /// runtime, this one catches names that were never real.
    #[test]
    fn every_rex_colour_class_names_a_token_that_exists() {
        let (dark, _light) = parse_tokens();
        assert!(dark.contains_key("surface-1"), "tokens.css parsed to nothing usable");
        let (rex_keys, other_families) = tailwind_colour_keys();

        // NO BASELINE HERE, and there was one for exactly one commit. 17 classes
        // named a token that does not exist — `text-rex-text-secondary` x16 (16 of
        // DbImportCard's 18 text classes, so every "secondary" line in that card
        // rendered at FULL body brightness) and `bg-rex-surface-0` x1. They were
        // held in a `KNOWN_UNDEFINED` ratchet while the fix was decided, then
        // repaid, and the ratchet was deleted with them — which is the shape a
        // debt list is supposed to have. The assertion below is the permanent
        // version: no undefined name, ever, no list to keep honest.
        let mut bad: Vec<String> = Vec::new();
        // `status-*` is the SAME token family under a second Tailwind name
        // (`status.warning-bright` → `--rex-warning-bright`), so a typo there
        // fails exactly as silently and belongs in the same guard.
        for (prefix, role) in [
            ("text-rex-", "text colour"),
            ("bg-rex-", "background"),
            ("text-status-", "text colour"),
            ("bg-status-", "background"),
            ("border-status-", "border"),
            // `border-rex-*` joined 4 Sep 2026: the Agent access dial's chosen
            // state was `border-rex-brand` + `bg-rex-brand-active` — tokens that
            // EXIST in tokens.css and are NOT `rex.*` Tailwind keys (brand lives
            // under `brand.*`), so the dial rendered with no chosen state at all
            // and this guard, checking names against tokens.css, passed.
            ("border-rex-", "border"),
        ] {
            for (name, file, line) in used_rex_classes(prefix) {
                // The `rex-*` prefixes resolve through Tailwind's `rex.*` map, and
                // ONLY that map — a token that exists in tokens.css under another
                // family (`brand.*`, `danger.*`, `status.*`, `toggle.*`) emits
                // nothing as `rex-<name>`. The dial's chosen state shipped that
                // way on 3 Sep 2026 and this guard, checking tokens.css, passed.
                if prefix.ends_with("rex-") && !rex_keys.contains(&name) {
                    let utility = prefix.trim_end_matches("rex-");
                    let hint = match other_families.get(&name) {
                        Some(fam) => format!("this token belongs to the `{fam}.*` family — write `{utility}{name}`"),
                        None => "no such key under `rex.*` in tailwind.config.js".to_string(),
                    };
                    bad.push(format!("  {file}:{line}  {prefix}{name}  ({role}) — {hint}"));
                    continue;
                }
                if !dark.contains_key(&name) {
                    bad.push(format!(
                        "  {file}:{line}  {prefix}{name}  ({role}) — no `--rex-{name}` in tokens.css"
                    ));
                }
            }
        }
        bad.sort();
        assert!(
            bad.is_empty(),
            "a Tailwind class names a rex token that does not exist. Tailwind emits NO rule \
             for it, so the element gets no colour and inherits — which looks deliberate on \
             screen, survives review, and is invisible to every other test here. Point it at \
             a token that exists:\n{}",
            bad.join("\n")
        );
    }

    /// **No raw Tailwind hue reaches the UI — every colour is a token.**
    ///
    /// This is the guard the light-mode report of 19 Aug 2026 asked for. The
    /// plugin list's update badge was `bg-amber-500/15 text-amber-400`: legible
    /// on the dark surface it was designed against, washed out on the light one,
    /// and — the part that matters — INVISIBLE to every check here, because
    /// `amber-400` is Tailwind's own palette rather than a rex token. The
    /// contrast guard below computes both themes for tokens; a raw hue has no
    /// light-theme value to compute, so it silently sat outside the check that
    /// exists to catch exactly this.
    ///
    /// Achromatic classes stay allowed and the reason is not "they are close
    /// enough": `text-white` on the brand button, the toggle KNOB, and the
    /// dialog scrim (`bg-black/50`) are the same colour in both themes ON
    /// PURPOSE — they sit on a fill that does not re-skin, so a token would
    /// only add a level of indirection that the light theme must then be
    /// careful NOT to change.
    #[test]
    fn no_raw_tailwind_hue_reaches_the_ui() {
        const HUES: &[&str] = &[
            "slate", "gray", "zinc", "neutral", "stone", "red", "orange", "amber", "yellow",
            "lime", "green", "emerald", "teal", "cyan", "sky", "blue", "indigo", "violet",
            "purple", "fuchsia", "pink", "rose",
        ];
        const PROPS: &[&str] = &[
            "text", "bg", "border", "ring", "fill", "stroke", "from", "to", "via", "divide",
            "outline", "decoration", "shadow", "caret", "accent",
        ];

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("src");
        let mut bad: Vec<String> = Vec::new();
        let mut scanned = 0usize;

        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    walk(&path, out);
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "tsx" && ext != "ts" {
                    continue;
                }
                if let Ok(raw) = std::fs::read_to_string(&path) {
                    out.push((path.display().to_string(), strip_ts_comments(&raw)));
                }
            }
        }
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(files.len() > 20, "only {} frontend files found — the walk is broken", files.len());

        for (file, text) in &files {
            scanned += 1;
            for (n, line) in text.lines().enumerate() {
                for prop in PROPS {
                    for hue in HUES {
                        let needle = format!("{prop}-{hue}-");
                        let mut rest = line;
                        while let Some(i) = rest.find(&needle) {
                            // A class boundary before it, and a DIGIT after —
                            // `border-red-500` is a hue, `bg-rex-accent-red-bg`
                            // is a token and must not be convicted for
                            // containing a colour word.
                            let before = rest[..i].chars().last();
                            let boundary =
                                before.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '-');
                            let after = &rest[i + needle.len()..];
                            let numeric = after.chars().next().is_some_and(|c| c.is_ascii_digit());
                            if boundary && numeric {
                                let shade: String =
                                    after.chars().take_while(|c| c.is_ascii_digit()).collect();
                                bad.push(format!(
                                    "  {file}:{}  {prop}-{hue}-{shade}",
                                    n + 1
                                ));
                            }
                            rest = &rest[i + needle.len()..];
                        }
                    }
                }
            }
        }
        bad.sort();
        assert!(
            bad.is_empty(),
            "{} raw Tailwind hue(s) in the UI. A palette class has ONE value, so it cannot \
             follow the theme — it is legible in whichever mode it was written against and \
             washed out in the other, and no guard here can compute it because there is no \
             light-theme value to read. Use the token that carries both (`status-*` for \
             running/warning/error, `rex-accent-*` for the hue chips), or add the token if \
             none fits:\n{}",
            bad.len(),
            bad.join("\n")
        );
        assert!(scanned > 20, "only {scanned} files scanned");
    }

    /// **Text a user reads meets WCAG AA (4.5:1) against the surface it sits on,
    /// in BOTH themes.**
    ///
    /// The sets are DERIVED from the frontend's own class usage, not listed here:
    /// a token used as a text colour joins the check by being used, and one that
    /// stops being used leaves. A hand-list would be a second copy of the design
    /// system, and the whole finding behind this test is that a hand-maintained
    /// claim about colour drifts from the colour.
    ///
    /// 4.5:1 is AA for NORMAL text, which is the right threshold here: the
    /// densest consumers render at 9.5-10.5px, and AA-large's 3:1 needs 24px
    /// (or 18.66px bold). Size is not an available lever.
    #[test]
    fn every_text_on_surface_pairing_meets_wcag_aa() {
        let (dark, light) = parse_tokens();

        // ── Landmark canary ──────────────────────────────────────────────────
        // A parser that returned empty maps would make every check below pass
        // vacuously. Assert a known-good pair in each theme, computed the same
        // way the real checks are, so a broken parser fails HERE and by name.
        for (theme, map) in [("dark", &dark), ("light", &light)] {
            let bright = map.get("text-bright").expect("text-bright must parse");
            let s1 = map.get("surface-1").expect("surface-1 must parse");
            let r = contrast(bright, s1).expect("both are opaque hex");
            assert!(r > 10.0, "{theme}: landmark pair reads {r:.2}:1 — the parser is wrong");
        }
        // ...and the two themes must not be the same map, or `light` silently
        // re-checks dark and the light theme goes uncovered.
        assert_ne!(
            dark.get("surface-1"),
            light.get("surface-1"),
            "the light overlay did not apply — light would just re-check dark"
        );

        /// A pairing that is exempt, and WHY. Every entry is itself a claim; a
        /// reasonless one silently shrinks the check, which is the
        /// guard-covers-claimed-surface defect this repo has paid for five times.
        struct Exempt {
            text: &'static str,
            surface: &'static str,
            why: &'static str,
        }
        const EXEMPT: &[Exempt] = &[
            // Translucent fills have no standalone contrast: they composite over
            // whichever surface is beneath, so the real pairing is text-on-that
            // surface, which this test already covers directly.
            Exempt { text: "*", surface: "hover", why: "rgba fill — composites over the surface beneath, which is checked on its own" },
            Exempt { text: "*", surface: "hover-strong", why: "rgba fill — as above" },
            Exempt { text: "*", surface: "active", why: "rgba fill — as above" },
            // These three are ORNAMENT used as a fill, never a text background.
            // Each was read at its call sites before being written here, because
            // an exemption asserted from the token's NAME rather than its use is
            // how an allow-list starts covering things it was never meant to.
            Exempt { text: "*", surface: "border-strong", why: "a 1px rule — `w-px`/`h-px` dividers in menu.tsx and NewSiteDialog.tsx; nothing sits on it" },
            Exempt { text: "*", surface: "text-dim", why: "the status DOT in AgentsMcpCard.tsx — a filled circle, never a text background" },
            Exempt { text: "*", surface: "text-bright", why: "the KNOB of the StartStopToggle switch — a moving circle, never a text background" },
            // Non-text content: WCAG 1.4.11 asks 3:1 of icons, not 1.4.3's 4.5:1.
            // `accent-teal` is ONLY ever an icon (`<Shield>`/`<Globe>` in
            // Onboarding.tsx) and clears 3:1 everywhere (worst 4.27:1).
            // `accent-blue` is deliberately NOT exempt beside it: it is an icon
            // colour in two places AND a tab LABEL in SiteDetail.tsx at 13.5px,
            // so the text threshold applies to it and exempting by token name
            // would have quietly covered the label too.
            // NOTE: there is no icon exemption here any more. `accent-teal` and
            // `text-dim` had one BY NAME, and a plant walked through it — moving
            // `text-rex-text-dim` onto a <span> kept the exemption and the guard
            // stayed green. Icons are now excluded STRUCTURALLY, where the class
            // is read, so the exemption cannot outlive the fact it rests on.
        ];

        // Only tokens used on something that is NOT an icon. This is the
        // exemption, and it is STRUCTURAL rather than declared: a token used
        // solely on lucide glyphs never enters the set, and the day one lands on
        // a <span> it does. The first version exempted `text-dim` BY NAME and a
        // plant walked straight through it — a class moved onto real text stayed
        // exempt because the exemption trusted a label instead of checking the
        // thing. Same family as every other guard-covers-claimed-surface row here.
        let mut text_tokens: std::collections::BTreeSet<String> =
            used_rex_classes_classified("text-rex-")
                .into_iter()
                .filter(|(_, _, _, icon)| !icon)
                .map(|(n, _, _, _)| n)
                .collect();
        // ...plus the same tokens reached through the `status-*` Tailwind name.
        // This was the hole the light-mode update badge fell through, and it had
        // TWO halves: the badge used a raw Tailwind hue (`text-amber-400`), which
        // is outside the design system entirely, and the fix moved it to
        // `text-status-warning-bright` — a real token this scan still could not
        // see, because it only ever looked for `text-rex-`. Moving a colour into
        // the system has to move it into the guard, or the repair is invisible
        // to the thing that was supposed to catch it.
        for (name, _, _, icon) in used_rex_classes_classified("text-status-") {
            if !icon {
                text_tokens.insert(name);
            }
        }
        // ...plus text colours set in RAW CSS rather than through Tailwind.
        // This was a hole and it was load-bearing: `globals.css` styled EVERY
        // input's `::placeholder` with `--rex-text-faint` — the app's most
        // widespread piece of text, at 2.58-3.35:1 — and a Tailwind-only scan
        // could not see it. A selector like `::placeholder` can sit on any
        // surface, so it is checked against all of them, which is the same
        // over-approximation the cross-product already makes.
        for name in raw_css_text_tokens() {
            text_tokens.insert(name);
        }
        // ...plus the ones set inline in a `style` prop (the chips).
        for name in inline_style_text_tokens() {
            text_tokens.insert(name);
        }
        // The BACKGROUND set stays the surfaces (`bg-rex-*`). `bg-status-*` is
        // deliberately not added, and the reason is the same one the rgba
        // exemptions rest on: the status fills are either translucent tints
        // (`warning-bg` and friends — they composite over the surface beneath,
        // and text-on-that-surface is what this check already computes) or
        // SOLID dots and pill fills that never host text. Adding them produced
        // 16 pairings nothing renders — muted body text on a solid green dot —
        // which is how a cross-product turns into noise nobody reads.
        let bg_tokens: std::collections::BTreeSet<String> =
            used_rex_classes("bg-rex-").into_iter().map(|(n, _, _)| n).collect();
        assert!(!text_tokens.is_empty() && !bg_tokens.is_empty(), "no classes found — the walk is broken");

        // NO DEBT LIST, and there was one for exactly two commits. It held 39
        // failing pairs while the fix was scoped, refused to let the number grow,
        // and then failed on "38 now PASS" — which is what forced the repayment
        // to be written down instead of absorbed. It was deleted with the debt.
        // The assertion below is the permanent one: no pairing under AA, ever.

        let mut fails: Vec<String> = Vec::new();
        // Every pairing an EXEMPT row skipped, with the reason it rests on. A
        // failing run has to say what it did NOT check: the exemptions are the
        // part of this guard that shrinks its own surface, and reading them
        // beside the failures is how a wrong one gets noticed.
        let mut skipped: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut checked = 0usize;
        for t in &text_tokens {
            for b in &bg_tokens {
                if let Some(e) = EXEMPT
                    .iter()
                    .find(|e| (e.text == "*" || e.text == t) && (e.surface == "*" || e.surface == b))
                {
                    skipped.insert(format!("  text-rex-{t} on bg-rex-{b} — {}", e.why));
                    continue;
                }
                for (theme, map) in [("dark", &dark), ("light", &light)] {
                    let (Some(tv), Some(bv)) = (map.get(t), map.get(b)) else { continue };
                    let Some(r) = contrast(tv, bv) else { continue };
                    checked += 1;
                    if r < 4.5 {
                        fails.push(format!("  {theme:<5} text-rex-{t} on bg-rex-{b}  {r:.2}:1  ({tv} on {bv})"));
                    }
                }
            }
        }
        assert!(checked > 50, "only {checked} pairs computed — the cross-product is not being built");
        fails.sort();

        assert!(
            fails.is_empty(),
            "{} text/surface pairing(s) fall below WCAG AA 4.5:1 for normal text (of {checked} \
             computed). Some of these render at 9.5-10.5px, so AA-large's 3:1 does not apply. \
             Move the consumer to a passing token, or add an EXEMPT entry WITH A REASON if it \
             is ornament or never composed — and read the call site before writing that reason, \
             which is how the four existing exemptions were decided:\n{}\n\n\
             NOT checked, because an EXEMPT row claims they are ornament or never composed \
             — if one of these is wrong, a real failure is hiding behind it:\n{}",
            fails.len(),
            fails.join("\n"),
            skipped.iter().cloned().collect::<Vec<_>>().join("\n")
        );
    }

    #[test]
    fn production_lines_survive_a_test_module_in_the_middle_of_a_file() {
        // The bug this exists for: cutting at the first `#[cfg(test)]` keeps the
        // head and silently drops everything after the tests, so a guard reads a
        // fraction of the file it claims to cover and passes.
        let src = "fn head() {}\n\
                   #[cfg(test)]\n\
                   mod tests {\n\
                       #[test]\n\
                       fn t() { let _ = 1; }\n\
                   }\n\
                   fn tail_after_the_tests() {}\n";
        let out = production_source(src);
        assert!(out.contains("fn head()"));
        assert!(out.contains("fn tail_after_the_tests()"), "code after the test module was lost");
        assert!(!out.contains("mod tests"), "the test module survived");
        assert!(!out.contains("fn t()"));
        // Line numbers stay true to the original file, which is what makes a
        // failure message point at something a reader can open.
        let lines = production_lines(src);
        assert_eq!(lines.first().map(|(n, _)| *n), Some(1));
        assert_eq!(lines.last().map(|(n, _)| *n), Some(7));
    }

    /// **Every IPC tally key a WebKit probe reads is a key the app can produce.**
    ///
    /// `DevGitPanel`'s mock counts each invoked command into `window.__ipcCalls`,
    /// and probes assert on those counts. A key nobody writes reads back as `0`
    /// forever — so `before === after` holds, and an assertion of the form "the
    /// costly pass did NOT ride along" passes while watching nothing. That is
    /// not hypothetical: `wpfocus.js` shipped reading `wp_plugins:updates` when
    /// the flag it needed was `checkUpdates`, and the control half of the probe
    /// was decoration until the key was made real.
    ///
    /// A key is `<command>` or `<command>:<suffix>`. The command must be
    /// registered on the bridge; the suffix must be one the tally actually
    /// synthesises, read out of `DevGitPanel.tsx` rather than listed here — a
    /// second copy of that list is how it would drift.
    #[test]
    fn every_ipc_tally_key_a_probe_reads_is_a_key_the_app_can_produce() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("lib.rs");
        let handler = lib
            .split("tauri::generate_handler![")
            .nth(1)
            .and_then(|b| b.split("])").next())
            .expect("the invoke_handler list");
        let registered: Vec<String> = handler
            .lines()
            .filter_map(|l| {
                let t = l.trim().trim_end_matches(',');
                t.strip_prefix("commands::").and_then(|r| r.rsplit("::").next()).map(str::to_string)
            })
            .filter(|n| !n.is_empty() && !n.contains(' '))
            .collect();
        assert!(registered.len() > 150, "only {} commands parsed — the scan is broken", registered.len());

        // The suffixes the tally invents, taken from the source that invents
        // them: `tally[`${cmd}:updates`]` and any sibling.
        let panel = std::fs::read_to_string(root.join("../src/routes/DevGitPanel.tsx"))
            .expect("DevGitPanel.tsx — the file that owns the tally");
        let mut suffixes: Vec<String> = Vec::new();
        for part in panel.split("${cmd}:").skip(1) {
            let s: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !s.is_empty() {
                suffixes.push(s);
            }
        }
        assert!(
            !suffixes.is_empty(),
            "no synthesised tally suffix found in DevGitPanel.tsx — either the tally changed shape \
             or this scan is looking at the wrong thing, and a green here would mean nothing"
        );

        // Every literal a probe reads through its `calls(...)` helper.
        let dir = root.join("../scripts/wk-checks");
        let mut keys: Vec<(String, String)> = Vec::new(); // (file, key)
        for e in std::fs::read_dir(&dir).expect("wk-checks/").flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("js") {
                continue;
            }
            let file = p.file_name().and_then(|x| x.to_str()).unwrap_or("?").to_string();
            let src = std::fs::read_to_string(&p).unwrap_or_default();
            for part in src.split("calls(\"").skip(1) {
                if let Some(k) = part.split('"').next() {
                    keys.push((file.clone(), k.to_string()));
                }
            }
        }
        assert!(
            keys.len() >= 4,
            "only {} tally reads found across wk-checks — the probes that assert on IPC counts \
             were not seen, so this test is watching an empty set",
            keys.len()
        );

        let bad: Vec<String> = keys
            .iter()
            .filter_map(|(file, key)| {
                let (base, suffix) = match key.split_once(':') {
                    Some((b, s)) => (b, Some(s)),
                    None => (key.as_str(), None),
                };
                if !registered.iter().any(|r| r == base) {
                    return Some(format!("{file}: \"{key}\" — no command named `{base}` is registered"));
                }
                match suffix {
                    Some(s) if !suffixes.iter().any(|x| x == s) => Some(format!(
                        "{file}: \"{key}\" — the tally never synthesises a `:{s}` key"
                    )),
                    _ => None,
                }
            })
            .collect();
        assert!(
            bad.is_empty(),
            "these probes read an IPC tally key nothing writes, so the count they assert on is \
             frozen at 0 and the assertion is decoration:\n  {}\nFix the key, or make the tally \
             produce it — see the `wp_plugins:updates` note in DevGitPanel.tsx",
            bad.join("\n  ")
        );
    }
}
// probe
