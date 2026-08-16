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
        fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<(String, String, usize)>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, prefix, out);
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "tsx" && ext != "ts" {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&path) else { continue };
                for (n, line) in strip_ts_comments(&raw).lines().enumerate() {
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
                            out.push((name, path.display().to_string(), n + 1));
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

    /// **A `text-rex-*` / `bg-rex-*` class that names no token generates NOTHING.**
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

        /// Names that are already wrong, with how many times — `(class, count)`.
        ///
        /// A debt list on the same terms as the contrast one: the set AND the
        /// counts must match exactly, so a NEW bad name is red and so is
        /// SPREADING an existing one, which a name-only list would have allowed.
        /// Removing the last use of a name is also red, because that is the
        /// moment the progress should be recorded rather than absorbed.
        ///
        /// What these actually do: nothing. Tailwind emits no rule, so the
        /// element inherits — and `DbImportCard.tsx` is written almost entirely
        /// against `text-rex-text-secondary` (16 of its 18 text classes), which
        /// means every "secondary" line in that card renders at FULL body
        /// brightness. The fix is a token that exists and passes AA, which is
        /// `text-muted`; it is a visual change to a shipped card, so it is the
        /// owner's call rather than a drive-by. `docs/TODO.md` carries it.
        const KNOWN_UNDEFINED: &[(&str, usize)] =
            &[("bg-rex-surface-0", 1), ("text-rex-text-secondary", 16)];

        let mut bad: Vec<String> = Vec::new();
        let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (prefix, role) in [("text-rex-", "text colour"), ("bg-rex-", "background")] {
            for (name, file, line) in used_rex_classes(prefix) {
                if !dark.contains_key(&name) {
                    *seen.entry(format!("{prefix}{name}")).or_default() += 1;
                    bad.push(format!(
                        "  {file}:{line}  {prefix}{name}  ({role}) — no `--rex-{name}` in tokens.css"
                    ));
                }
            }
        }
        bad.sort();

        let expected: std::collections::BTreeMap<String, usize> =
            KNOWN_UNDEFINED.iter().map(|(n, c)| (n.to_string(), *c)).collect();
        assert_eq!(
            seen, expected,
            "the set of Tailwind classes naming a NON-EXISTENT rex token changed. Such a \
             class generates no rule at all, so the element gets NO colour and inherits — \
             which looks deliberate on screen and cannot be reviewed. If a name is new or \
             spreading, point it at a token that exists; if you removed the last use, delete \
             its row from KNOWN_UNDEFINED.\n\nEvery occurrence:\n{}",
            bad.join("\n")
        );
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
            Exempt { text: "accent-teal", surface: "*", why: "icon-only (Onboarding Shield/Globe); WCAG 1.4.11 non-text is 3:1 and it clears 4.27:1" },
        ];

        let text_tokens: std::collections::BTreeSet<String> =
            used_rex_classes("text-rex-").into_iter().map(|(n, _, _)| n).collect();
        let bg_tokens: std::collections::BTreeSet<String> =
            used_rex_classes("bg-rex-").into_iter().map(|(n, _, _)| n).collect();
        assert!(!text_tokens.is_empty() && !bg_tokens.is_empty(), "no classes found — the walk is broken");

        /// Pairings that fail TODAY, recorded so the gate can be live while the
        /// fix is scoped — `(theme, text token, surface token)`.
        ///
        /// **This is a debt list, not an exemption list, and the difference is
        /// enforced below:** the failing set must equal this EXACTLY. A new
        /// failure is red, and so is a pairing that starts passing — because a
        /// stale debt list is how "we are working on it" turns into "we forgot",
        /// and making progress edit this file is what keeps the number honest.
        ///
        /// Every entry here is a real WCAG AA failure a user is looking at right
        /// now. Ledger #337 stays 🔨 until this list is EMPTY; the plan for
        /// emptying it is in `docs/TODO.md`, and the short version is that
        /// `text-dim`, `text-faint` and `text-label` cannot all be lightened to
        /// pass without collapsing onto `text-muted`, so the work is deciding
        /// which of their consumers are text and which are ornament.
        const KNOWN_DEBT: &[(&str, &str, &str)] = &[
            ("dark", "text-dim", "bg"),
            ("dark", "text-dim", "surface-1"),
            ("dark", "text-dim", "surface-2"),
            ("dark", "text-dim", "surface-2-hover"),
            ("dark", "text-dim", "surface-3"),
            ("dark", "text-dim", "well"),
            ("dark", "text-dim", "well-deep"),
            ("dark", "text-faint", "bg"),
            ("dark", "text-faint", "surface-1"),
            ("dark", "text-faint", "surface-2"),
            ("dark", "text-faint", "surface-2-hover"),
            ("dark", "text-faint", "surface-3"),
            ("dark", "text-faint", "well"),
            ("dark", "text-faint", "well-deep"),
            ("dark", "text-label", "bg"),
            ("dark", "text-label", "surface-1"),
            ("dark", "text-label", "surface-2"),
            ("dark", "text-label", "surface-2-hover"),
            ("dark", "text-label", "surface-3"),
            ("dark", "text-label", "well"),
            ("dark", "text-label", "well-deep"),
            ("light", "accent-blue", "surface-3"),
            ("light", "text-dim", "bg"),
            ("light", "text-dim", "surface-1"),
            ("light", "text-dim", "surface-2"),
            ("light", "text-dim", "surface-2-hover"),
            ("light", "text-dim", "surface-3"),
            ("light", "text-dim", "well"),
            ("light", "text-faint", "bg"),
            ("light", "text-faint", "surface-1"),
            ("light", "text-faint", "surface-2"),
            ("light", "text-faint", "surface-2-hover"),
            ("light", "text-faint", "surface-3"),
            ("light", "text-faint", "well"),
            ("light", "text-label", "bg"),
            ("light", "text-label", "surface-2"),
            ("light", "text-label", "surface-2-hover"),
            ("light", "text-label", "surface-3"),
            ("light", "text-label", "well"),
        ];

        let mut fails: Vec<String> = Vec::new();
        let mut failing: std::collections::BTreeSet<(String, String, String)> =
            std::collections::BTreeSet::new();
        let mut checked = 0usize;
        for t in &text_tokens {
            for b in &bg_tokens {
                if EXEMPT.iter().any(|e| (e.text == "*" || e.text == t) && (e.surface == "*" || e.surface == b)) {
                    continue;
                }
                for (theme, map) in [("dark", &dark), ("light", &light)] {
                    let (Some(tv), Some(bv)) = (map.get(t), map.get(b)) else { continue };
                    let Some(r) = contrast(tv, bv) else { continue };
                    checked += 1;
                    if r < 4.5 {
                        failing.insert((theme.to_string(), t.clone(), b.clone()));
                        fails.push(format!("  {theme:<5} text-rex-{t} on bg-rex-{b}  {r:.2}:1  ({tv} on {bv})"));
                    }
                }
            }
        }
        assert!(checked > 50, "only {checked} pairs computed — the cross-product is not being built");
        fails.sort();

        let debt: std::collections::BTreeSet<(String, String, String)> = KNOWN_DEBT
            .iter()
            .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
            .collect();
        assert_eq!(debt.len(), KNOWN_DEBT.len(), "KNOWN_DEBT has a duplicate row");

        let fresh: Vec<_> = failing.difference(&debt).collect();
        assert!(
            fresh.is_empty(),
            "{} NEW text/surface pairing(s) fall below WCAG AA 4.5:1 — these are not in \
             KNOWN_DEBT, so something got worse. These render at 9.5-10.5px in places, so \
             AA-large's 3:1 does not apply. Move the consumer to a passing token, or add an \
             EXEMPT entry WITH A REASON if it is ornament or never composed:\n{:#?}\n\n\
             (full current failing set:\n{}\n)",
            fresh.len(),
            fresh,
            fails.join("\n")
        );

        let repaid: Vec<_> = debt.difference(&failing).collect();
        assert!(
            repaid.is_empty(),
            "{} pairing(s) in KNOWN_DEBT now PASS. That is good news and it is still a \
             failure, on purpose: a debt list nobody removes from stops describing the debt, \
             and this is the one moment the progress is visible. Delete these rows from \
             KNOWN_DEBT — and if it is now empty, delete the list, flip ledger #337 to ✅, \
             and tick the row in docs/TODO.md:\n{:#?}",
            repaid.len(),
            repaid
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
