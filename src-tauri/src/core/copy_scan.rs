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
