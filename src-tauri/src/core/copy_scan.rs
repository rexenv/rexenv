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
//! defect. Extraction is what documentation could not do here: the right code
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
            None if line.trim_start().starts_with("#[cfg(test)]") => depth = Some(0),
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
                if path.is_dir() {
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
}
// probe
