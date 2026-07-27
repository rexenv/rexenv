//! core::confedit — the connection-config rewrite: plan, diff, byte-preserving
//! apply (Stage 3, `docs/PLAN-valet-herd-rewrite.md` §3).
//!
//! This is the ONE place in the whole migration that prepares a write inside
//! the user's project, and three guarantees are structural rather than
//! behavioural:
//!
//! 1. **No password can be staged.** [`RewriteKey`] is a CLOSED enum — Host,
//!    Port, User — with no password variant, and [`RewritePlan`]'s fields are
//!    private behind shape-specific constructors. No plan, diff, or write can
//!    contain a password change, whatever future code does.
//! 2. **The diff IS the write.** [`rewrite`] produces the new file content and
//!    derives the diff FROM those bytes; the caller writes
//!    [`Rewrite::new_content`] verbatim (after checking the on-disk file still
//!    equals [`Rewrite::original`]). There is no second rendering path that
//!    could drift from the preview.
//! 3. **Refusing is a feature, and WRITES REFUSE MORE THAN READS — that is
//!    the rule here, not an incidental behaviour.** A wrong read costs one
//!    bad fact; a wrong write costs the user's file. So anything ambiguous —
//!    duplicate keys (even with equal values), values spanning lines,
//!    `${VAR}` interpolation, keys that exist only commented out, a heredoc
//!    anywhere in a wp-config — downgrades to tell-only with the reason
//!    ([`phpconf::Unreadable`]), never a guess. The sharpest instance: one
//!    unclosed quote refuses the WHOLE `.env`, whichever key it belongs to,
//!    because everything after that point may not be lines at all — the
//!    reader may shrug at it for an unrelated key; the editor must not.
//!
//! WordPress edits are OUR span editor, not `wp config set` — settled 28 Jul
//! 2026: wp-cli would make the preview a reconstruction while the written
//! bytes carry wp-cli's formatting (exactly the approved-vs-written gap rule
//! 2 exists to close), the parser already refuses every shape wp-cli would
//! have hedged for, and one fewer subprocess is one fewer place to reason
//! about argv and secrets.
//!
//! Byte-preserving means byte-preserving: everything except the edited value
//! spans (and one optionally appended `DB_PORT` line) comes out identical —
//! CRLF, BOM, spacing, quoting style, comments, a missing trailing newline.
//! A "harmless" normalisation would show up in the user's git diff and make
//! our change look bigger than it is.

use crate::core::phpconf::{self, EnvLineKind, Unreadable};
use crate::error::{Error, Result};

/// The keys a rewrite may touch — a CLOSED set with no password variant, so a
/// password change is unrepresentable in every downstream type (D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteKey {
    Host,
    Port,
    User,
}

impl RewriteKey {
    /// Exhaustive by construction: adding a variant breaks this constant (and
    /// the tests pinned to it) at compile time.
    pub const ALL: [RewriteKey; 3] = [RewriteKey::Host, RewriteKey::Port, RewriteKey::User];

    /// The literal key this maps to in a `.env` file.
    pub fn env_name(self) -> &'static str {
        match self {
            RewriteKey::Host => "DB_HOST",
            RewriteKey::Port => "DB_PORT",
            RewriteKey::User => "DB_USERNAME",
        }
    }

    /// The literal define this maps to in `wp-config.php`. Port has none:
    /// WordPress folds the port into `DB_HOST` (`host:port`), and
    /// [`RewritePlan::wp`] cannot stage a Port change.
    pub fn wp_name(self) -> Option<&'static str> {
        match self {
            RewriteKey::Host => Some("DB_HOST"),
            RewriteKey::Port => None,
            RewriteKey::User => Some("DB_USER"),
        }
    }
}

/// Which config shape a plan targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigShape {
    WpConfig,
    DotEnv,
}

/// What a rewrite will change: shape + (key, new value) pairs. Fields are
/// private — the shape-specific constructors are the only builders, so a
/// Port change in a wp-config plan is unrepresentable and every value has
/// passed the charset check before a plan exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewritePlan {
    shape: ConfigShape,
    changes: Vec<(RewriteKey, String)>,
}

/// New values are written verbatim inside the file's own quoting style, so
/// they must never need quoting or escaping themselves. Hosts, ports and our
/// `rex_<slug>` usernames all fit; anything else is a programming error, not
/// a file-shape refusal.
fn checked_value(key: RewriteKey, v: &str) -> Result<String> {
    let ok = !v.is_empty()
        && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'));
    if !ok {
        return Err(Error::Other(format!(
            "rewrite value for {key:?} contains characters the editor never writes: {v:?}"
        )));
    }
    Ok(v.to_string())
}

impl RewritePlan {
    /// A wp-config plan: `DB_HOST` gets the `host:port` form; `DB_USER` only
    /// for the root case (D1's dedicated user).
    pub fn wp(host_port: &str, user: Option<&str>) -> Result<Self> {
        let mut changes = vec![(RewriteKey::Host, checked_value(RewriteKey::Host, host_port)?)];
        if let Some(u) = user {
            changes.push((RewriteKey::User, checked_value(RewriteKey::User, u)?));
        }
        Ok(Self { shape: ConfigShape::WpConfig, changes })
    }

    /// A `.env` plan: host and port are separate keys (`DB_PORT` is appended
    /// beside `DB_HOST` if absent); `DB_USERNAME` only for the root case.
    pub fn env(host: &str, port: u16, user: Option<&str>) -> Result<Self> {
        let mut changes = vec![
            (RewriteKey::Host, checked_value(RewriteKey::Host, host)?),
            (RewriteKey::Port, port.to_string()),
        ];
        if let Some(u) = user {
            changes.push((RewriteKey::User, checked_value(RewriteKey::User, u)?));
        }
        Ok(Self { shape: ConfigShape::DotEnv, changes })
    }

    pub fn shape(&self) -> ConfigShape {
        self.shape
    }

    pub fn changes(&self) -> &[(RewriteKey, String)] {
        &self.changes
    }
}

/// One diff line, derived from the produced bytes (never from intent).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// `'-'` (a line of the original) or `'+'` (a line of the new content).
    pub sign: char,
    /// 1-based line number in its own file version.
    pub line: usize,
    /// The line without its terminator.
    pub text: String,
}

/// A prepared rewrite. The caller shows `diff`, and on consent writes
/// `new_content` verbatim — after re-reading the file and refusing if it no
/// longer equals `original` byte-for-byte (the file changed since the diff
/// was shown). `original` is also exactly what the backup stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    pub original: String,
    pub new_content: String,
    pub diff: Vec<DiffLine>,
}

/// Prepare a rewrite of `original` (the file's full text) per `plan`.
///
/// Pure: no filesystem, no execution. `Err` is a first-class refusal that
/// downgrades to tell-only. An all-no-op plan (every value already equals the
/// file's) returns `new_content == original` and an empty diff.
pub fn rewrite(original: &str, plan: &RewritePlan) -> std::result::Result<Rewrite, Unreadable> {
    let new_content = match plan.shape {
        ConfigShape::DotEnv => rewrite_env(original, plan)?,
        ConfigShape::WpConfig => rewrite_wp(original, plan)?,
    };

    // Self-check: the SAME reader Stage 2 trusts must see every new value in
    // the produced bytes. A mismatch is our bug, and it downgrades to
    // tell-only like every other refusal instead of staging a blind write.
    for (key, value) in &plan.changes {
        let read = match plan.shape {
            ConfigShape::DotEnv => phpconf::dotenv_value(&new_content, key.env_name()),
            ConfigShape::WpConfig => {
                let name = key.wp_name().expect("wp plans cannot stage Port");
                phpconf::wp_define_str(&new_content, name)
            }
        };
        if read.as_deref() != Ok(value.as_str()) {
            let name = match plan.shape {
                ConfigShape::DotEnv => key.env_name(),
                ConfigShape::WpConfig => key.wp_name().expect("checked above"),
            };
            return Err(Unreadable::EditUnverified { key: name.into() });
        }
    }

    let diff = diff_lines(original, &new_content);
    Ok(Rewrite { original: original.to_string(), new_content, diff })
}

// ---------------------------------------------------------------------------
// .env
// ---------------------------------------------------------------------------

fn rewrite_env(original: &str, plan: &RewritePlan) -> std::result::Result<String, Unreadable> {
    let lines = phpconf::dotenv_lines(original);

    // Any unclosed quote anywhere makes the line structure untrustworthy for
    // a WRITE (a later "line" may really be inside that value) — refuse the
    // whole file, whichever key it belongs to.
    for l in &lines {
        if let EnvLineKind::UnterminatedQuote { key } = &l.kind {
            return Err(Unreadable::MultiLineValue { key: key.clone(), line: l.line_no });
        }
    }

    // Resolve every planned key BEFORE editing anything: the apply is
    // all-or-nothing, because a half-rewritten config (host changed, user
    // not) would break the site in a shape nobody asked for.
    struct SpanEdit {
        span: (usize, usize),
        new: String,
    }
    let mut edits: Vec<SpanEdit> = Vec::new();
    let mut append_port: Option<String> = None;

    for (key, new_value) in &plan.changes {
        let name = key.env_name();
        let matches: Vec<&phpconf::EnvLine> = lines
            .iter()
            .filter(|l| matches!(&l.kind, EnvLineKind::Entry { key: k, .. } if k == name))
            .collect();
        match matches.as_slice() {
            [] => {
                // Absent. A commented copy usually means "switched off on
                // purpose" — we neither uncomment nor add a live twin (writes
                // refuse where reads simply treat it as absent). Only DB_PORT
                // may be appended when truly absent.
                let commented = lines.iter().find_map(|l| match &l.kind {
                    EnvLineKind::CommentedEntry { key: k } if k == name => Some(l.line_no),
                    _ => None,
                });
                if let Some(line) = commented {
                    return Err(Unreadable::CommentedOut { key: name.into(), line });
                }
                if *key == RewriteKey::Port {
                    append_port = Some(new_value.clone());
                } else {
                    return Err(Unreadable::MissingKey { key: name.into() });
                }
            }
            [one] => {
                let EnvLineKind::Entry { value, value_span, interpolates, .. } = &one.kind
                else {
                    unreachable!("filtered to Entry");
                };
                if *interpolates {
                    return Err(Unreadable::NonLiteral { key: name.into(), saw: value.clone() });
                }
                if value != new_value {
                    edits.push(SpanEdit { span: *value_span, new: new_value.clone() });
                }
            }
            [first, second, ..] => {
                // Even equal values refuse: an edit would have to choose
                // which line to change (plan §3), unlike a read where either
                // one is the answer.
                return Err(Unreadable::DuplicateKey {
                    key: name.into(),
                    first_line: first.line_no,
                    second_line: second.line_no,
                });
            }
        }
    }

    // Splice the value edits (already non-overlapping, in file order after
    // sorting) and remember where DB_HOST's line ends for the append.
    edits.sort_by_key(|e| e.span.0);
    let mut out = String::with_capacity(original.len() + 32);
    let mut cursor = 0usize;
    for e in &edits {
        out.push_str(&original[cursor..e.span.0]);
        out.push_str(&e.new);
        cursor = e.span.1;
    }
    out.push_str(&original[cursor..]);

    if let Some(port_value) = append_port {
        out = append_port_line(&out, original, &lines, &port_value)?;
    }
    Ok(out)
}

/// Insert a `DB_PORT` line directly after `DB_HOST`'s line — keeping the
/// file's own grouping — copying that line's conventions: leading whitespace,
/// `export ` prefix, spacing around `=`, and line terminator. A `DB_HOST`
/// that ends the file without a newline keeps the file ending that way: the
/// new line goes after a single added terminator and itself gets none.
///
/// `edited` is the content after span edits; spans/offsets come from
/// `original`, which is only valid because value edits never change line
/// STRUCTURE — they replace bytes within a line. We still locate the host
/// line's text in `edited` by line index, not by byte offset, to be safe.
fn append_port_line(
    edited: &str,
    original: &str,
    lines: &[phpconf::EnvLine],
    port_value: &str,
) -> std::result::Result<String, Unreadable> {
    let host = lines
        .iter()
        .find(|l| {
            matches!(&l.kind, EnvLineKind::Entry { key, .. } if key == RewriteKey::Host.env_name())
        })
        // rewrite_env resolved Host before Port, so it exists; a plan without
        // a Host change cannot exist (both constructors stage one).
        .ok_or(Unreadable::MissingKey { key: RewriteKey::Host.env_name().into() })?;

    // The host line's conventions, read from the ORIGINAL bytes.
    let host_content = &original[host.start..host.content_end];
    let after_bom = host_content.trim_start_matches('\u{feff}');
    let leading_ws: String =
        after_bom.chars().take_while(|c| c.is_whitespace()).collect();
    let rest = after_bom.trim_start();
    let export = if rest.starts_with("export ") { "export " } else { "" };
    let rest = rest.strip_prefix(export).unwrap_or(rest);
    // Spacing around `=`: everything between the key's end and the value.
    let eq = rest.find('=').expect("host line is an Entry");
    let key_text = &rest[..eq];
    let pre_eq: String = key_text.chars().rev().take_while(|c| c.is_whitespace()).collect();
    let after_eq = &rest[eq + 1..];
    let post_eq: String = after_eq.chars().take_while(|c| c.is_whitespace()).collect();

    let new_line = format!("{leading_ws}{export}DB_PORT{pre_eq}={post_eq}{port_value}");

    // Re-locate the host line in the edited content by line index (1-based).
    let mut offset = 0usize;
    let mut line_no = 0usize;
    let bytes = edited;
    let mut insert_at = bytes.len();
    let mut host_terminator = host.terminator;
    let mut needs_lead_terminator = false;
    let mut rest_slice = bytes;
    while !rest_slice.is_empty() {
        line_no += 1;
        let (len, term) = match rest_slice.find('\n') {
            Some(nl) if nl > 0 && rest_slice.as_bytes()[nl - 1] == b'\r' => (nl - 1, "\r\n"),
            Some(nl) => (nl, "\n"),
            None => (rest_slice.len(), ""),
        };
        let line_end = offset + len + term.len();
        if line_no == host.line_no {
            insert_at = line_end;
            host_terminator = term;
            needs_lead_terminator = term.is_empty();
            break;
        }
        offset = line_end;
        rest_slice = &bytes[offset..];
    }

    let mut out = String::with_capacity(edited.len() + new_line.len() + 4);
    out.push_str(&edited[..insert_at]);
    if needs_lead_terminator {
        // Host ends the file with no newline: add one (the file's dominant
        // convention) and keep the file ending terminator-less as before.
        out.push_str(dominant_terminator(original));
        out.push_str(&new_line);
    } else {
        out.push_str(&new_line);
        out.push_str(host_terminator);
    }
    out.push_str(&edited[insert_at..]);
    Ok(out)
}

fn dominant_terminator(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    if crlf > lf {
        "\r\n"
    } else {
        "\n"
    }
}

// ---------------------------------------------------------------------------
// wp-config.php
// ---------------------------------------------------------------------------

fn rewrite_wp(original: &str, plan: &RewritePlan) -> std::result::Result<String, Unreadable> {
    struct SpanEdit {
        span: (usize, usize),
        new: String,
    }
    let mut edits: Vec<SpanEdit> = Vec::new();

    for (key, new_value) in &plan.changes {
        let name = key.wp_name().expect("RewritePlan::wp cannot stage a Port change");
        let (defines, stopped) = phpconf::find_defines(original, name);
        // A read can use what it found before a heredoc; a WRITE cannot — a
        // second define past the unscanned region would be invisible to the
        // duplicate check, so the whole file refuses.
        if let Some(stop) = stopped {
            return Err(stop);
        }
        match defines.as_slice() {
            [] => return Err(Unreadable::MissingKey { key: name.into() }),
            [one] => match (&one.value, one.value_span) {
                (phpconf::Value::Str(s), Some(span)) => {
                    if s != new_value {
                        edits.push(SpanEdit { span, new: new_value.clone() });
                    }
                }
                (other, _) => {
                    let saw = match other {
                        phpconf::Value::NonLiteral(s) => s.clone(),
                        phpconf::Value::Bool(b) => b.to_string(),
                        phpconf::Value::Num(n) => n.clone(),
                        phpconf::Value::Str(_) => unreachable!("Str carries a span"),
                    };
                    return Err(Unreadable::NonLiteral { key: name.into(), saw });
                }
            },
            [first, second, ..] => {
                // Same rule as .env: even equal values refuse for a WRITE —
                // the edit would have to choose which one to change.
                return Err(Unreadable::DuplicateKey {
                    key: name.into(),
                    first_line: first.line,
                    second_line: second.line,
                });
            }
        }
    }

    edits.sort_by_key(|e| e.span.0);
    let mut out = String::with_capacity(original.len() + 32);
    let mut cursor = 0usize;
    for e in &edits {
        out.push_str(&original[cursor..e.span.0]);
        out.push_str(&e.new);
        cursor = e.span.1;
    }
    out.push_str(&original[cursor..]);
    Ok(out)
}

// ---------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------

/// A line diff DERIVED from the two texts — not from the edit intent — so
/// what it shows is provably what changed. Greedy two-pointer: sufficient for
/// this editor's output (in-place value edits and one inserted line), and
/// truthful for anything else it might ever be handed.
fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let o: Vec<&str> = old.split_inclusive('\n').collect();
    let n: Vec<&str> = new.split_inclusive('\n').collect();
    let strip = |s: &str| s.trim_end_matches('\n').trim_end_matches('\r').to_string();

    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < o.len() && j < n.len() {
        if o[i] == n[j] {
            i += 1;
            j += 1;
        } else if j + 1 < n.len() && o[i] == n[j + 1] {
            // A line was inserted before o[i].
            out.push(DiffLine { sign: '+', line: j + 1, text: strip(n[j]) });
            j += 1;
        } else {
            out.push(DiffLine { sign: '-', line: i + 1, text: strip(o[i]) });
            out.push(DiffLine { sign: '+', line: j + 1, text: strip(n[j]) });
            i += 1;
            j += 1;
        }
    }
    while i < o.len() {
        out.push(DiffLine { sign: '-', line: i + 1, text: strip(o[i]) });
        i += 1;
    }
    while j < n.len() {
        out.push(DiffLine { sign: '+', line: j + 1, text: strip(n[j]) });
        j += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── the pins ───────────────────────────────────────────────────────────

    #[test]
    fn the_key_vocabulary_cannot_name_a_password() {
        // The D1 guarantee. RewriteKey::ALL is exhaustive by construction
        // (adding a variant breaks the constant at compile time), and no key
        // maps to a password-ish name in either shape. If someone adds a
        // Password variant back, this test — and the exhaustive matches in
        // env_name/wp_name — fail before any code could stage one.
        assert_eq!(RewriteKey::ALL.len(), 3);
        for k in RewriteKey::ALL {
            for name in [Some(k.env_name()), k.wp_name()].into_iter().flatten() {
                let lower = name.to_lowercase();
                assert!(!lower.contains("pass"), "{name} could carry a secret");
                assert!(!lower.contains("secret"), "{name} could carry a secret");
            }
        }
    }

    #[test]
    fn plans_refuse_values_the_editor_would_have_to_escape() {
        for bad in ["it's", "a b", "x\"y", "", "a\nb", "p#c", "${X}"] {
            assert!(RewritePlan::env(bad, 13306, None).is_err(), "{bad:?} accepted as host");
            assert!(RewritePlan::wp("127.0.0.1:13306", Some(bad)).is_err(), "{bad:?} accepted as user");
        }
        assert!(RewritePlan::env("127.0.0.1", 13306, Some("rex_ea_test")).is_ok());
        assert!(RewritePlan::wp("127.0.0.1:13306", None).is_ok());
    }

    // ── .env: byte preservation ────────────────────────────────────────────

    #[test]
    fn env_rewrite_changes_exactly_the_value_bytes_and_nothing_else() {
        let original = "APP_NAME=ea\nDB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT=3306\n\
                        DB_DATABASE=ea\nDB_USERNAME=root\nDB_PASSWORD=hunter2\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, Some("rex_ea_test")).unwrap();
        let r = rewrite(original, &plan).unwrap();
        // Host is unchanged in value → its line must be byte-identical; the
        // password line must be byte-identical; only port and user change.
        assert_eq!(
            r.new_content,
            "APP_NAME=ea\nDB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT=13306\n\
             DB_DATABASE=ea\nDB_USERNAME=rex_ea_test\nDB_PASSWORD=hunter2\n"
        );
        assert!(r.new_content.contains("DB_PASSWORD=hunter2\n"));
        assert_eq!(r.original, original);
    }

    #[test]
    fn env_rewrite_preserves_crlf_bom_spacing_quotes_comments_and_no_trailing_newline() {
        // Every convention in one file: BOM, CRLF, indentation, spaces around
        // `=`, export prefix, quote styles, an inline comment, and no final
        // newline. The whole-file byte comparison is the point: a "harmless"
        // normalisation would show up in their git diff.
        let original = "\u{feff}# db\r\n  DB_HOST = \"10.0.0.5\" # local\r\n\
                        export DB_PORT='3306'\r\nDB_USERNAME=root\r\nDB_PASSWORD=x";
        let plan = RewritePlan::env("127.0.0.1", 13306, Some("rex_ea_test")).unwrap();
        let r = rewrite(original, &plan).unwrap();
        assert_eq!(
            r.new_content,
            "\u{feff}# db\r\n  DB_HOST = \"127.0.0.1\" # local\r\n\
             export DB_PORT='13306'\r\nDB_USERNAME=rex_ea_test\r\nDB_PASSWORD=x"
        );
    }

    #[test]
    fn env_noop_plan_returns_identical_bytes_and_an_empty_diff() {
        let original = "DB_HOST=127.0.0.1\nDB_PORT=13306\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        let r = rewrite(original, &plan).unwrap();
        assert_eq!(r.new_content, original);
        assert!(r.diff.is_empty());
    }

    // ── .env: the DB_PORT append ───────────────────────────────────────────

    #[test]
    fn env_appends_db_port_after_db_host_copying_its_conventions() {
        let original = "APP_NAME=ea\nexport DB_HOST = localhost\nDB_DATABASE=ea\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        let r = rewrite(original, &plan).unwrap();
        assert_eq!(
            r.new_content,
            "APP_NAME=ea\nexport DB_HOST = 127.0.0.1\nexport DB_PORT = 13306\nDB_DATABASE=ea\n"
        );
    }

    #[test]
    fn env_append_keeps_a_crlf_file_crlf() {
        let original = "DB_HOST=old.host\r\nDB_DATABASE=ea\r\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        let r = rewrite(original, &plan).unwrap();
        assert_eq!(r.new_content, "DB_HOST=127.0.0.1\r\nDB_PORT=13306\r\nDB_DATABASE=ea\r\n");
    }

    #[test]
    fn env_append_to_a_file_ending_without_a_newline_keeps_it_ending_without_one() {
        let original = "APP_NAME=ea\nDB_HOST=old.host";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        let r = rewrite(original, &plan).unwrap();
        assert_eq!(r.new_content, "APP_NAME=ea\nDB_HOST=127.0.0.1\nDB_PORT=13306");
    }

    // ── .env: refusals ─────────────────────────────────────────────────────

    #[test]
    fn env_refuses_duplicates_even_with_equal_values() {
        // A read can shrug at an equal repeat; an edit would have to choose
        // which line to change.
        let original = "DB_HOST=127.0.0.1\nDB_HOST=127.0.0.1\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        assert!(matches!(
            rewrite(original, &plan),
            Err(Unreadable::DuplicateKey { first_line: 1, second_line: 2, .. })
        ));
    }

    #[test]
    fn env_refuses_the_whole_file_on_any_unclosed_quote_even_another_keys() {
        // Past an unclosed quote, "lines" may really be inside that value —
        // the reader tolerates this for other keys, the editor must not.
        let original = "APP_KEY=\"abc\ndef\"\nDB_HOST=127.0.0.1\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        assert!(matches!(
            rewrite(original, &plan),
            Err(Unreadable::MultiLineValue { line: 1, .. })
        ));
    }

    #[test]
    fn env_refuses_interpolation_on_a_target_key_but_tolerates_it_elsewhere() {
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        assert!(matches!(
            rewrite("DB_HOST=${DB_HOST_OVERRIDE}\n", &plan),
            Err(Unreadable::NonLiteral { .. })
        ));
        // ${} on a NON-target key is none of our business.
        let original = "CACHE_PREFIX=${APP_NAME}_c\nDB_HOST=old.host\nDB_PORT=3306\n";
        let r = rewrite(original, &plan).unwrap();
        assert!(r.new_content.contains("CACHE_PREFIX=${APP_NAME}_c\n"));
    }

    #[test]
    fn env_refuses_a_commented_out_key_rather_than_guessing_intent() {
        // "# DB_PORT=3306" usually means deliberately off — appending a live
        // twin (or uncommenting) would guess at intent, so: tell-only.
        let original = "DB_HOST=old.host\n# DB_PORT=3306\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        assert!(matches!(
            rewrite(original, &plan),
            Err(Unreadable::CommentedOut { line: 2, .. })
        ));
        // Same for a rewrite target that exists ONLY commented out — and the
        // message names the commented line rather than a bare "missing".
        let original = "#DB_HOST=old.host\nDB_PORT=3306\n";
        assert!(matches!(
            rewrite(original, &plan),
            Err(Unreadable::CommentedOut { line: 1, .. })
        ));
    }

    #[test]
    fn env_a_commented_copy_beside_a_live_key_is_just_a_comment() {
        // Commented alternates beside a live key are everywhere in real .env
        // files; only a MISSING live key makes the comment ambiguous.
        let original = "# DB_HOST=192.168.1.10\nDB_HOST=old.host\nDB_PORT=3306\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        let r = rewrite(original, &plan).unwrap();
        assert_eq!(r.new_content, "# DB_HOST=192.168.1.10\nDB_HOST=127.0.0.1\nDB_PORT=13306\n");
    }

    #[test]
    fn env_refuses_a_missing_rewrite_target() {
        let plan = RewritePlan::env("127.0.0.1", 13306, Some("rex_ea")).unwrap();
        assert!(matches!(
            rewrite("DB_HOST=old.host\nDB_PORT=3306\n", &plan),
            Err(Unreadable::MissingKey { .. }) // DB_USERNAME absent
        ));
    }

    // ── wp-config ──────────────────────────────────────────────────────────

    #[test]
    fn wp_rewrite_changes_the_value_inside_the_quotes_and_nothing_else() {
        let original = "<?php\ndefine( 'DB_NAME', 'ea' );\ndefine( 'DB_USER', 'root' );\n\
                        define( 'DB_PASSWORD', 'hunter2' );\ndefine( 'DB_HOST', '127.0.0.1' );\n\
                        $table_prefix = 'wp_';\n";
        let plan = RewritePlan::wp("127.0.0.1:13306", Some("rex_ea_test")).unwrap();
        let r = rewrite(original, &plan).unwrap();
        assert_eq!(
            r.new_content,
            "<?php\ndefine( 'DB_NAME', 'ea' );\ndefine( 'DB_USER', 'rex_ea_test' );\n\
             define( 'DB_PASSWORD', 'hunter2' );\ndefine( 'DB_HOST', '127.0.0.1:13306' );\n\
             $table_prefix = 'wp_';\n"
        );
        // The password line's exact bytes survive — no path touches them.
        assert!(r.new_content.contains("define( 'DB_PASSWORD', 'hunter2' );"));
    }

    #[test]
    fn wp_rewrite_handles_a_define_split_across_lines() {
        let original = "<?php\ndefine(\n    'DB_HOST',\n    '127.0.0.1'\n);\n";
        let plan = RewritePlan::wp("127.0.0.1:13306", None).unwrap();
        let r = rewrite(original, &plan).unwrap();
        assert_eq!(r.new_content, "<?php\ndefine(\n    'DB_HOST',\n    '127.0.0.1:13306'\n);\n");
    }

    #[test]
    fn wp_refuses_non_literals_duplicates_and_heredocs() {
        let plan = RewritePlan::wp("127.0.0.1:13306", None).unwrap();
        assert!(matches!(
            rewrite("<?php define('DB_HOST', getenv('DB_HOST'));", &plan),
            Err(Unreadable::NonLiteral { .. })
        ));
        // Equal duplicates refuse for a WRITE (a read tolerates them).
        assert!(matches!(
            rewrite("<?php define('DB_HOST','x'); define('DB_HOST','x');", &plan),
            Err(Unreadable::DuplicateKey { .. })
        ));
        // A heredoc leaves part of the file unscanned — a duplicate could
        // hide past it, so writes refuse outright.
        assert!(matches!(
            rewrite("<?php define('DB_HOST','x');\n$s = <<<SQL\nx\nSQL;\n", &plan),
            Err(Unreadable::UnsupportedSyntax { .. })
        ));
        assert!(matches!(
            rewrite("<?php // empty", &plan),
            Err(Unreadable::MissingKey { .. })
        ));
    }

    // ── the diff ───────────────────────────────────────────────────────────

    #[test]
    fn the_diff_is_derived_from_the_produced_bytes() {
        let original = "APP_NAME=ea\nDB_HOST=old.host\nDB_DATABASE=ea\n";
        let plan = RewritePlan::env("127.0.0.1", 13306, None).unwrap();
        let r = rewrite(original, &plan).unwrap();
        // Exactly: one changed line (-/+) and one inserted line (+).
        assert_eq!(
            r.diff,
            vec![
                DiffLine { sign: '-', line: 2, text: "DB_HOST=old.host".into() },
                DiffLine { sign: '+', line: 2, text: "DB_HOST=127.0.0.1".into() },
                DiffLine { sign: '+', line: 3, text: "DB_PORT=13306".into() },
            ]
        );
        // And every diff line is genuinely present in its file version — the
        // diff cannot describe bytes that don't exist.
        for d in &r.diff {
            let hay = if d.sign == '-' { &r.original } else { &r.new_content };
            assert!(hay.lines().any(|l| l == d.text), "{:?} not in its file", d.text);
        }
    }

    #[test]
    fn no_plan_no_diff_no_path_can_show_a_password() {
        // The whole-file property behind D1: for every plan either constructor
        // can produce, a diff never contains the password line — because no
        // key in the vocabulary can address it.
        let env = "DB_HOST=old\nDB_PORT=3306\nDB_USERNAME=root\nDB_PASSWORD=hunter2\n";
        let wp = "<?php define('DB_HOST','old'); define('DB_USER','root');\n\
                  define('DB_PASSWORD','hunter2');";
        for (original, plan) in [
            (env, RewritePlan::env("127.0.0.1", 13306, Some("rex_ea")).unwrap()),
            (wp, RewritePlan::wp("127.0.0.1:13306", Some("rex_ea")).unwrap()),
        ] {
            let r = rewrite(original, &plan).unwrap();
            for d in &r.diff {
                assert!(!d.text.contains("hunter2"), "a secret reached the diff: {:?}", d.text);
                assert!(!d.text.to_lowercase().contains("password"), "{:?}", d.text);
            }
        }
    }
}
