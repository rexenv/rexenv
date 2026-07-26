//! core::phpconf — static readers for the config files a PHP project keeps:
//! `wp-config.php` constants and Laravel-style `.env` files.
//!
//! **Nothing here executes anything.** wp-config.php is read as text, exactly
//! like WP-CLI's `config get` does, because running a stranger's config to learn
//! their database password is not a thing we will do.
//!
//! **There is ONE of each parser.** The WordPress `define()` reader started life
//! inside `core::logs` for `WP_DEBUG`; the database import needs the same reader
//! for credentials, so it lives here and `logs` calls it. Two readers that agree
//! today drift tomorrow, and this one is read for credentials.
//!
//! **Refusing is a feature.** Every function here answers "I could not read this
//! confidently" as a first-class outcome ([`Unreadable`]) rather than guessing.
//! A wrong `DB_NAME` dumps the wrong database; a truncated `DB_PASSWORD` fails a
//! connection that looked fine on screen. Conservative beats clever, and the
//! caller turns a refusal into a "needs attention" row, never an error that
//! reads like a failure of the user's project.

use std::path::{Path, PathBuf};

/// Why a config file could not be read confidently. Each variant names what we
/// SAW, so the row can explain itself instead of shrugging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// No `wp-config.php` / `.env` where we looked.
    NoConfigFile,
    /// The same key is set twice and we can't tell which one wins (a
    /// conditional define, an override further down the file).
    DuplicateKey { key: String, first_line: usize, second_line: usize },
    /// The value isn't a literal: `define('DB_NAME', env('DB_NAME'))`,
    /// `DB_DATABASE="${APP_NAME}_db"`, a concatenation, a constant.
    NonLiteral { key: String, saw: String },
    /// A quoted value that never closes on its line (`.env` only).
    MultiLineValue { key: String, line: usize },
    /// The key isn't in the file at all.
    MissingKey { key: String },
    /// A heredoc/nowdoc (`<<<`) starts before we found everything we needed.
    /// Its body can contain anything, including text that looks like a
    /// `define()`, so we stop there rather than risk reading the wrong thing.
    UnsupportedSyntax { detail: String },
}

impl Unreadable {
    /// One sentence for the user. Never phrased as their mistake — these are
    /// shapes rexenv chose not to guess at.
    pub fn message(&self) -> String {
        match self {
            Unreadable::NoConfigFile => {
                "rexenv couldn't find a wp-config.php or .env to read the database \
                 settings from.".into()
            }
            Unreadable::DuplicateKey { key, first_line, second_line } => format!(
                "{key} is set twice (lines {first_line} and {second_line}) with different \
                 values, so rexenv can't tell which one this site actually uses."
            ),
            Unreadable::NonLiteral { key, saw } => format!(
                "{key} is computed rather than written out ({saw}), and rexenv reads these \
                 files as text — it never runs them — so it can't resolve the value."
            ),
            Unreadable::MultiLineValue { key, line } => format!(
                "{key} (line {line}) opens a quote it doesn't close on the same line; \
                 rexenv doesn't guess at values that span lines."
            ),
            Unreadable::MissingKey { key } => {
                format!("{key} isn't set in the config rexenv read.")
            }
            Unreadable::UnsupportedSyntax { detail } => format!(
                "rexenv stopped reading this config at {detail} — it can't be sure what \
                 comes after it means."
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// wp-config.php
// ---------------------------------------------------------------------------

/// One `define()` we found, with the line it was on (1-based, for messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Define {
    pub value: Value,
    pub line: usize,
}

/// A define's value as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A literal single- or double-quoted string, already unescaped.
    Str(String),
    /// `true` / `false`.
    Bool(bool),
    /// A bare number, kept verbatim (we never need it as a number).
    Num(String),
    /// Anything else: a function call, a concatenation, another constant, an
    /// interpolating double-quoted string. The text is kept for the message.
    NonLiteral(String),
}

impl Value {
    /// The string a literal holds, or `None` for every other shape.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// PHP-ish truthiness for the shapes we accept (`true`, `1`, `'1'`, `"true"`).
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Num(n) => n.trim() != "0",
            Value::Str(s) => matches!(s.to_ascii_lowercase().as_str(), "true" | "1"),
            Value::NonLiteral(_) => false,
        }
    }
}

/// WordPress looks for `wp-config.php` in the docroot or one directory above.
pub fn wp_config_path(docroot: &Path) -> Option<PathBuf> {
    let here = docroot.join("wp-config.php");
    if here.is_file() {
        return Some(here);
    }
    let above = docroot.parent()?.join("wp-config.php");
    above.is_file().then_some(above)
}

/// Read the `wp-config.php` for `docroot`, if there is one.
pub fn wp_config_text(docroot: &Path) -> Option<String> {
    std::fs::read_to_string(wp_config_path(docroot)?).ok()
}

/// Scan `text` for `define('NAME', <value>)` and return every occurrence of
/// `name`, in file order.
///
/// A real scan, not a line grep: it skips `//`, `#` and `/* */` comments and
/// steps over string literals, so a `define()` inside a comment or inside
/// another string is never read, and a define split across lines still is. It
/// stops at the first heredoc (`<<<`), whose body could contain anything.
///
/// Returning ALL occurrences rather than the first is deliberate — the caller
/// decides what a duplicate means, and for credentials it means "refuse".
pub fn find_defines(text: &str, name: &str) -> (Vec<Define>, Option<Unreadable>) {
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut out = Vec::new();

    macro_rules! bump {
        ($n:expr) => {{
            for k in i..(i + $n).min(b.len()) {
                if b[k] == b'\n' {
                    line += 1;
                }
            }
            i = (i + $n).min(b.len());
        }};
    }

    while i < b.len() {
        // Comments.
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b[i] == b'#' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            let end = text[i + 2..].find("*/").map(|p| i + 2 + p + 2).unwrap_or(b.len());
            bump!(end - i);
            continue;
        }
        // Heredoc/nowdoc: stop. Its body is arbitrary text.
        if b[i] == b'<' && text[i..].starts_with("<<<") {
            return (
                out,
                Some(Unreadable::UnsupportedSyntax {
                    detail: format!("a heredoc on line {line}"),
                }),
            );
        }
        // Skip over string literals so their contents are never scanned.
        if b[i] == b'\'' || b[i] == b'"' {
            match scan_string(text, i) {
                Some((_, end)) => {
                    bump!(end - i);
                    continue;
                }
                None => {
                    return (
                        out,
                        Some(Unreadable::UnsupportedSyntax {
                            detail: format!("an unterminated string on line {line}"),
                        }),
                    )
                }
            }
        }
        // `define` as a whole word, not `my_define` or `$define`.
        if (b[i] | 0x20) == b'd'
            && text[i..].len() >= 6
            && text[i..i + 6].eq_ignore_ascii_case("define")
            && (i == 0 || !is_ident_byte(b[i - 1]))
            && !text[i + 6..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
        {
            let at_line = line;
            if let Some((found_name, value, end)) = parse_define(text, i) {
                if found_name == name {
                    out.push(Define { value, line: at_line });
                }
                bump!(end - i);
                continue;
            }
            bump!(6);
            continue;
        }
        bump!(1);
    }
    (out, None)
}

/// Exactly one literal `define(name, …)`, or the reason we won't use it.
///
/// This is what the database import calls: it refuses a duplicate with a
/// different value (a conditional define means the FIRST one textually is not
/// necessarily the one PHP ran) and refuses anything that isn't written out.
pub fn wp_define_str(text: &str, name: &str) -> Result<String, Unreadable> {
    let (found, stopped) = find_defines(text, name);
    let mut iter = found.iter();
    let Some(first) = iter.next() else {
        return Err(stopped.unwrap_or(Unreadable::MissingKey { key: name.into() }));
    };
    for other in iter {
        if other.value != first.value {
            return Err(Unreadable::DuplicateKey {
                key: name.into(),
                first_line: first.line,
                second_line: other.line,
            });
        }
    }
    match &first.value {
        Value::Str(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Num(n) => Ok(n.clone()),
        Value::NonLiteral(saw) => {
            Err(Unreadable::NonLiteral { key: name.into(), saw: saw.clone() })
        }
    }
}

/// `$table_prefix = 'wp_';` — an assignment, not a define, so it gets its own
/// tiny reader. Same rules: literal or nothing.
pub fn wp_table_prefix(text: &str) -> Option<String> {
    for raw in text.lines() {
        let t = raw.trim_start();
        if t.starts_with("//") || t.starts_with('#') || t.starts_with('*') {
            continue;
        }
        let Some(rest) = t.strip_prefix("$table_prefix") else { continue };
        let rest = rest.trim_start().strip_prefix('=')?.trim_start();
        let (s, _) = scan_string(rest, 0)?;
        return Some(s);
    }
    None
}

/// Parse `define ( 'NAME' , VALUE )` starting at the `define` keyword.
/// Returns the name, the value, and the index just past the closing paren.
fn parse_define(text: &str, start: usize) -> Option<(String, Value, usize)> {
    let b = text.as_bytes();
    let mut i = start + "define".len();
    i = skip_ws(text, i)?;
    if b.get(i)? != &b'(' {
        return None;
    }
    i = skip_ws(text, i + 1)?;
    let (name, next) = scan_string(text, i)?;
    i = skip_ws(text, next)?;
    if b.get(i)? != &b',' {
        return None;
    }
    i = skip_ws(text, i + 1)?;

    // The value. A literal string is scanned in full — INCLUDING any ')' or ';'
    // inside it, which a "find the next paren" reader would truncate at, handing
    // back half a password that fails to connect for reasons nobody can see.
    let quoted = (b[i] == b'\'' || b[i] == b'"')
        .then(|| scan_string(text, i))
        .flatten()
        // A lone literal ends the argument. `'a' . 'b'` does not — the string
        // scan succeeds and would hand back "a", so the follow char decides.
        .filter(|(_, next)| {
            skip_ws(text, *next).is_some_and(|k| b[k] == b',' || b[k] == b')')
        });

    let (value, mut end) = match quoted {
        Some((s, next)) => {
            // A double-quoted string interpolates: "$db" or "{$cfg['db']}" is
            // not a literal, whatever it looks like.
            let raw = &text[i..next];
            if b[i] == b'"' && (raw.contains('$') || raw.contains('{')) {
                (Value::NonLiteral(raw.to_string()), next)
            } else {
                (Value::Str(s), next)
            }
        }
        None => {
            let j = scan_to_close(text, i)?;
            let raw = text[i..j].trim();
            let v = match raw.to_ascii_lowercase().as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ if !raw.is_empty()
                    && raw.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') =>
                {
                    Value::Num(raw.to_string())
                }
                _ => Value::NonLiteral(raw.to_string()),
            };
            (v, j)
        }
    };

    // Past a trailing third argument (the legacy case-insensitive flag) and the
    // closing paren.
    end = skip_ws(text, end)?;
    if b.get(end) == Some(&b',') {
        end = scan_to_close(text, end + 1)?;
    }
    if b.get(end)? != &b')' {
        return None;
    }
    Some((name, value, end + 1))
}

/// Index of the `)` that closes the call whose arguments start at `i`, stepping
/// over nested parens and over string literals (so a `)` inside a password is
/// not mistaken for the end of the call).
fn scan_to_close(text: &str, mut i: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut depth = 0i32;
    loop {
        let c = *b.get(i)?;
        if c == b'\'' || c == b'"' {
            i = scan_string(text, i)?.1;
            continue;
        }
        if c == b'(' {
            depth += 1;
        }
        if c == b')' {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
        i += 1;
    }
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

fn skip_ws(text: &str, mut i: usize) -> Option<usize> {
    let b = text.as_bytes();
    loop {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        // A comment between the arguments is legal PHP.
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            i = text[i + 2..].find("*/").map(|p| i + 2 + p + 2)?;
            continue;
        }
        break;
    }
    (i < b.len()).then_some(i)
}

/// Scan a PHP string literal starting at `i` (which must be a quote). Returns
/// the UNESCAPED contents and the index just past the closing quote.
///
/// Single quotes: only `\'` and `\\` are escapes — `\n` is a literal backslash
/// and an n, which matters for passwords. Double quotes: the common escapes.
fn scan_string(text: &str, i: usize) -> Option<(String, usize)> {
    let b = text.as_bytes();
    let quote = *b.get(i)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let mut out = String::new();
    let mut j = i + 1;
    while j < b.len() {
        let c = b[j];
        if c == b'\\' && j + 1 < b.len() {
            let n = b[j + 1];
            if quote == b'\'' {
                if n == b'\'' || n == b'\\' {
                    out.push(n as char);
                    j += 2;
                    continue;
                }
                out.push('\\');
                j += 1;
                continue;
            }
            let mapped = match n {
                b'n' => Some('\n'),
                b't' => Some('\t'),
                b'r' => Some('\r'),
                b'"' => Some('"'),
                b'\\' => Some('\\'),
                b'$' => Some('$'),
                _ => None,
            };
            match mapped {
                Some(ch) => {
                    out.push(ch);
                    j += 2;
                }
                None => {
                    out.push('\\');
                    j += 1;
                }
            }
            continue;
        }
        if c == quote {
            return Some((out, j + 1));
        }
        // Multi-byte characters travel whole.
        let ch = text[j..].chars().next()?;
        out.push(ch);
        j += ch.len_utf8();
    }
    None
}

// ---------------------------------------------------------------------------
// .env
// ---------------------------------------------------------------------------

/// Read one key from a `.env`, conservatively.
///
/// Refuses rather than guesses on: a duplicate key, a value whose quote doesn't
/// close on the line, and `${VAR}` interpolation. Tolerates `export ` prefixes,
/// `#` comments, CRLF, blank lines and unquoted values.
pub fn dotenv_value(text: &str, key: &str) -> Result<String, Unreadable> {
    let mut found: Option<(String, usize)> = None;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((k, v)) = line.split_once('=') else { continue };
        if k.trim() != key {
            continue;
        }
        let v = v.trim_start();
        let value = if v.starts_with('"') || v.starts_with('\'') {
            let quote = v.as_bytes()[0];
            let body = &v[1..];
            // The closing quote must be on this line, or we are looking at a
            // value that spans lines and we don't guess at those.
            let Some(close) = body.find(quote as char) else {
                return Err(Unreadable::MultiLineValue { key: key.into(), line: line_no });
            };
            body[..close].to_string()
        } else {
            // Unquoted: an inline comment ends the value, per dotenv.
            match v.find(" #") {
                Some(p) => v[..p].trim_end().to_string(),
                None => v.trim_end().to_string(),
            }
        };
        if value.contains("${") {
            return Err(Unreadable::NonLiteral { key: key.into(), saw: value });
        }
        if let Some((_, first_line)) = &found {
            return Err(Unreadable::DuplicateKey {
                key: key.into(),
                first_line: *first_line,
                second_line: line_no,
            });
        }
        found = Some((value, line_no));
    }
    found.map(|(v, _)| v).ok_or(Unreadable::MissingKey { key: key.into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_stock_wordpress_shape() {
        let text = r#"<?php
define( 'DB_NAME', 'ea' );
define( 'DB_USER', 'root' );
define( 'DB_PASSWORD', 'secret' );
define( 'DB_HOST', '127.0.0.1' );
$table_prefix = 'wp_';
"#;
        assert_eq!(wp_define_str(text, "DB_NAME").unwrap(), "ea");
        assert_eq!(wp_define_str(text, "DB_USER").unwrap(), "root");
        assert_eq!(wp_define_str(text, "DB_PASSWORD").unwrap(), "secret");
        assert_eq!(wp_table_prefix(text).as_deref(), Some("wp_"));
    }

    #[test]
    fn a_password_containing_a_paren_or_semicolon_survives_whole() {
        // The bug a "find the next ')'" reader has: it hands back half a
        // password, and the connection then fails for a reason nobody can see
        // on screen because the config LOOKS right.
        let text = r#"<?php
define('DB_PASSWORD', 'p)ss;w(rd');
define('DB_NAME', 'ea'); // ) not the end
"#;
        assert_eq!(wp_define_str(text, "DB_PASSWORD").unwrap(), "p)ss;w(rd");
        assert_eq!(wp_define_str(text, "DB_NAME").unwrap(), "ea");
    }

    #[test]
    fn single_quote_escapes_are_php_escapes_not_c_escapes() {
        let text = r#"<?php define('DB_PASSWORD', 'a\'b\\c\nd');"#;
        // \n inside single quotes is a backslash and an n — not a newline.
        assert_eq!(wp_define_str(text, "DB_PASSWORD").unwrap(), "a'b\\c\\nd");
    }

    #[test]
    fn defines_split_across_lines_and_past_comments_are_found() {
        let text = r#"<?php
/* define('DB_NAME', 'commented-out'); */
// define('DB_NAME', 'also-not-this');
define(
    'DB_NAME',
    'real'
);
"#;
        assert_eq!(wp_define_str(text, "DB_NAME").unwrap(), "real");
    }

    #[test]
    fn a_define_inside_a_string_is_not_read() {
        let text = r#"<?php
$doc = "define('DB_NAME', 'from-a-string')";
define('DB_NAME', 'real');
"#;
        assert_eq!(wp_define_str(text, "DB_NAME").unwrap(), "real");
    }

    #[test]
    fn computed_values_are_refused_not_guessed() {
        for (src, saw) in [
            ("<?php define('DB_NAME', getenv('DB_NAME'));", "getenv('DB_NAME')"),
            ("<?php define('DB_NAME', $name);", "$name"),
            ("<?php define('DB_NAME', 'a' . 'b');", "'a' . 'b'"),
        ] {
            match wp_define_str(src, "DB_NAME") {
                Err(Unreadable::NonLiteral { key, saw: s }) => {
                    assert_eq!(key, "DB_NAME");
                    assert_eq!(s, saw);
                }
                other => panic!("expected NonLiteral for {src:?}, got {other:?}"),
            }
        }
        // A double-quoted string that interpolates is computed too.
        assert!(matches!(
            wp_define_str(r#"<?php define('DB_NAME', "{$cfg['db']}");"#, "DB_NAME"),
            Err(Unreadable::NonLiteral { .. })
        ));
    }

    #[test]
    fn a_conditional_duplicate_is_refused_but_a_harmless_repeat_is_not() {
        // PHP takes the first define it RUNS, which is not necessarily the first
        // one written — so differing values are genuinely ambiguous to a reader.
        let conditional = r#"<?php
if (getenv('CI')) { define('DB_NAME', 'ci'); }
else { define('DB_NAME', 'local'); }
"#;
        assert!(matches!(
            wp_define_str(conditional, "DB_NAME"),
            Err(Unreadable::DuplicateKey { .. })
        ));
        // The same value twice is not ambiguous: either one is the answer.
        let repeated = "<?php define('DB_NAME','ea'); define('DB_NAME','ea');";
        assert_eq!(wp_define_str(repeated, "DB_NAME").unwrap(), "ea");
    }

    #[test]
    fn a_heredoc_stops_the_scan_rather_than_risking_its_body() {
        let text = "<?php\n$sql = <<<SQL\ndefine('DB_NAME', 'inside-a-heredoc');\nSQL;\n";
        assert!(matches!(
            wp_define_str(text, "DB_NAME"),
            Err(Unreadable::UnsupportedSyntax { .. })
        ));
        // Anything found BEFORE the heredoc is still good.
        let before = "<?php\ndefine('DB_NAME','ea');\n$sql = <<<SQL\nx\nSQL;\n";
        assert_eq!(wp_define_str(before, "DB_NAME").unwrap(), "ea");
    }

    #[test]
    fn missing_keys_say_so() {
        assert_eq!(
            wp_define_str("<?php // nothing here", "DB_NAME"),
            Err(Unreadable::MissingKey { key: "DB_NAME".into() })
        );
    }

    #[test]
    fn wp_debug_shapes_still_read_the_way_the_logs_tab_needs() {
        let text = r#"<?php
define('WP_DEBUG', true);
define('WP_DEBUG_LOG', '/tmp/custom.log');
define('SCRIPT_DEBUG', 0);
"#;
        let (d, _) = find_defines(text, "WP_DEBUG");
        assert!(d[0].value.is_truthy());
        let (l, _) = find_defines(text, "WP_DEBUG_LOG");
        assert_eq!(l[0].value.as_str(), Some("/tmp/custom.log"));
        let (s, _) = find_defines(text, "SCRIPT_DEBUG");
        assert!(!s[0].value.is_truthy());
    }

    #[test]
    fn dotenv_reads_the_ordinary_shapes() {
        let env = "# comment\nDB_CONNECTION=mysql\nexport DB_HOST=127.0.0.1\n\
                   DB_DATABASE=\"my db\"\nDB_USERNAME='root'\nDB_PASSWORD=p@ss#1 \n\
                   DB_PORT=3306 # the port\n";
        assert_eq!(dotenv_value(env, "DB_CONNECTION").unwrap(), "mysql");
        assert_eq!(dotenv_value(env, "DB_HOST").unwrap(), "127.0.0.1");
        assert_eq!(dotenv_value(env, "DB_DATABASE").unwrap(), "my db");
        assert_eq!(dotenv_value(env, "DB_USERNAME").unwrap(), "root");
        // A '#' with no leading space is part of the value, not a comment.
        assert_eq!(dotenv_value(env, "DB_PASSWORD").unwrap(), "p@ss#1");
        assert_eq!(dotenv_value(env, "DB_PORT").unwrap(), "3306");
    }

    #[test]
    fn dotenv_refuses_the_ambiguous_shapes() {
        assert!(matches!(
            dotenv_value("DB_DATABASE=one\nDB_DATABASE=two\n", "DB_DATABASE"),
            Err(Unreadable::DuplicateKey { first_line: 1, second_line: 2, .. })
        ));
        assert!(matches!(
            dotenv_value("DB_PASSWORD=\"starts here\nand ends there\"\n", "DB_PASSWORD"),
            Err(Unreadable::MultiLineValue { line: 1, .. })
        ));
        assert!(matches!(
            dotenv_value("DB_DATABASE=${APP_NAME}_db\n", "DB_DATABASE"),
            Err(Unreadable::NonLiteral { .. })
        ));
        assert!(matches!(
            dotenv_value("DB_HOST=127.0.0.1\n", "DB_DATABASE"),
            Err(Unreadable::MissingKey { .. })
        ));
    }

    #[test]
    fn every_refusal_explains_itself_without_blaming_the_project() {
        let cases = [
            Unreadable::NoConfigFile,
            Unreadable::DuplicateKey { key: "DB_NAME".into(), first_line: 1, second_line: 9 },
            Unreadable::NonLiteral { key: "DB_NAME".into(), saw: "env('X')".into() },
            Unreadable::MultiLineValue { key: "DB_PASSWORD".into(), line: 4 },
            Unreadable::MissingKey { key: "DB_NAME".into() },
            Unreadable::UnsupportedSyntax { detail: "a heredoc on line 3".into() },
        ];
        for c in cases {
            let m = c.message();
            assert!(m.len() > 30, "{m:?} is too terse to act on");
            assert!(m.ends_with('.'), "{m:?} should read as a sentence");
            // These are shapes we declined to guess at, not user errors.
            for blame in ["invalid", "malformed", "error", "wrong", "bad "] {
                assert!(!m.to_lowercase().contains(blame), "{m:?} reads as blame ({blame})");
            }
        }
    }
}
