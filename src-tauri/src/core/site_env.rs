//! core::site_env — validation + escaping for per-site environment variables
//! (Phase 3 §1.6). The vars travel per-REQUEST on the shared stack —
//! `fastcgi_param` lines in the site's nginx server block — so the shared
//! per-version php-fpm pools are never touched; FrankenPHP overrides get both
//! config `env` lines AND real process env at spawn (one process per site).
//! Live-verified visibility: `getenv()`, `$_SERVER` and `$_ENV` all see them on
//! both servers ($_ENV works on php-fpm because our static builds load no
//! php.ini, so `variables_order` is the compiled default `EGPCS` — see
//! docs/ARCHITECTURE.md).
//!
//! THE GENERATED CONFIGS ARE THE TRUST BOUNDARY. Split: REJECT what can't be
//! made safe, ESCAPE what can. nginx has no escape for `$` inside quoted
//! strings (it interpolates variables there), and Caddy expands `{placeholders}`
//! inside quoted strings — so `$`, `{`, `}` and control characters are
//! rejected; `\` and `"` are escapable in both languages and are escaped at
//! emission ([`escape_value`]).

use crate::error::{Error, Result};

/// Max variables per site, name length, value length — sanity bounds.
pub const MAX_VARS: usize = 50;
pub const MAX_NAME: usize = 64;
pub const MAX_VALUE: usize = 1024;

/// FastCGI param names a user variable may NOT use, checked case-insensitively.
/// Covers every param our nginx template emits (see
/// `services::TEMPLATE_FCGI_PARAMS` — a test asserts full coverage so a future
/// template addition can't silently escape this list), the CGI/PHP-consumed
/// params we don't emit but PHP gives routing/auth meaning to, the per-request
/// php.ini injectors, and PATH (shadowing it breaks anything PHP shells out to).
pub const RESERVED: &[&str] = &[
    // emitted by our nginx template (services::TEMPLATE_FCGI_PARAMS)
    "SCRIPT_FILENAME",
    "QUERY_STRING",
    "REQUEST_METHOD",
    "CONTENT_TYPE",
    "CONTENT_LENGTH",
    "SCRIPT_NAME",
    "REQUEST_URI",
    "DOCUMENT_URI",
    "DOCUMENT_ROOT",
    "SERVER_PROTOCOL",
    "GATEWAY_INTERFACE",
    "SERVER_SOFTWARE",
    "REMOTE_ADDR",
    "REMOTE_PORT",
    "SERVER_ADDR",
    "SERVER_PORT",
    "SERVER_NAME",
    "REQUEST_SCHEME",
    "HTTPS",
    // CGI/PHP-consumed (routing/auth semantics) even though we don't emit them
    "PATH_INFO",
    "PATH_TRANSLATED",
    "REDIRECT_STATUS",
    "AUTH_TYPE",
    "REMOTE_USER",
    "FCGI_ROLE",
    // per-request php.ini injection — a different feature wearing an env name
    "PHP_VALUE",
    "PHP_ADMIN_VALUE",
    // shadowing PATH per-request breaks PHP shell-outs
    "PATH",
];

/// Validate one (name, value) pair. Rejections name the exact problem — the UI
/// mirrors these rules for instant feedback, but THIS is the enforcement.
pub fn validate(name: &str, value: &str) -> Result<()> {
    let reject = |why: String| Error::Other(format!("env var '{name}': {why}"));

    if name.is_empty() || name.len() > MAX_NAME {
        return Err(reject(format!("name must be 1–{MAX_NAME} characters")));
    }
    let mut bytes = name.bytes();
    let head_ok = matches!(bytes.next(), Some(b) if b.is_ascii_alphabetic() || b == b'_');
    if !head_ok || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(reject(
            "name must match [A-Za-z_][A-Za-z0-9_]* (letters, digits, underscore)".into(),
        ));
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {
        return Err(reject(
            "this name is reserved — it's a FastCGI/PHP parameter the server already sets".into(),
        ));
    }
    if name.len() >= 5 && name[..5].eq_ignore_ascii_case("HTTP_") {
        return Err(reject(
            "names starting with HTTP_ are reserved — they'd be indistinguishable from \
             forged request headers in $_SERVER"
                .into(),
        ));
    }

    if value.len() > MAX_VALUE {
        return Err(reject(format!("value must be at most {MAX_VALUE} characters")));
    }
    for c in value.chars() {
        match c {
            '\u{0}'..='\u{1f}' | '\u{7f}' => {
                return Err(reject(format!(
                    "value contains a control character ({c:?}) — newlines/tabs can't be \
                     represented safely in the server config"
                )))
            }
            // nginx interpolates $var inside quoted strings and has NO escape for it.
            '$' => return Err(reject("value may not contain '$' (nginx interpolates it and provides no escape)".into())),
            // Caddy expands {placeholders} inside quoted strings, also unescapable.
            '{' | '}' => return Err(reject("value may not contain '{' or '}' (Caddy expands placeholders inside strings)".into())),
            _ => {}
        }
    }
    Ok(())
}

/// Validate a whole replace-all set: per-pair rules, the per-site cap, and
/// duplicate names (case-sensitively exact — FastCGI params are).
pub fn validate_all(pairs: &[(String, String)]) -> Result<()> {
    if pairs.len() > MAX_VARS {
        return Err(Error::Other(format!("at most {MAX_VARS} env vars per site")));
    }
    let mut seen = std::collections::HashSet::new();
    for (name, value) in pairs {
        validate(name, value)?;
        if !seen.insert(name.as_str()) {
            return Err(Error::Other(format!("duplicate env var name: {name}")));
        }
    }
    Ok(())
}

/// Escape a VALIDATED value for a double-quoted nginx/Caddyfile string: both
/// languages support `\\` and `\"`; everything else dangerous was rejected by
/// [`validate`]. Callers must emit the result inside double quotes.
pub fn escape_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_vars() {
        for (n, v) in [
            ("API_URL", "https://api.example.test/v2"),
            ("_DEBUG", ""),
            ("stripe_key_1", "sk_test_abc123"),
            ("MESSAGE", "he said \"hi\" and C:\\path"),
        ] {
            validate(n, v).unwrap_or_else(|e| panic!("{n} should be valid: {e}"));
        }
    }

    #[test]
    fn rejects_bad_names() {
        for n in ["", "1BAD", "WITH-DASH", "WITH SPACE", "Ü", "a".repeat(65).as_str()] {
            assert!(validate(n, "x").is_err(), "{n:?} should be rejected");
        }
    }

    #[test]
    fn rejects_reserved_names_case_insensitively() {
        for n in ["SCRIPT_FILENAME", "script_filename", "Path", "php_value", "HTTPS"] {
            let e = validate(n, "x").unwrap_err().to_string();
            assert!(e.contains("reserved"), "{n}: {e}");
        }
        let e = validate("HTTP_X_FOO", "x").unwrap_err().to_string();
        assert!(e.contains("HTTP_"), "{e}");
    }

    #[test]
    fn rejects_unescapable_values_naming_the_character() {
        let cases: &[(&str, &str)] = &[
            ("$document_root", "$"),
            ("a{placeholder}b", "{"),
            ("line1\nline2", "control"),
            ("tab\there", "control"),
        ];
        for (v, needle) in cases {
            let e = validate("X", v).unwrap_err().to_string();
            assert!(e.contains(needle), "{v:?}: {e}");
        }
    }

    #[test]
    fn injection_attempts_are_rejected_or_inert() {
        // Newline breakout: rejected outright.
        assert!(validate("X", "\"; include /etc/passwd;\n").is_err());
        // Quote breakout without newline: allowed through validation but the
        // escaped emission keeps it inside the quoted string.
        let v = "\"; fastcgi_pass 127.0.0.1:9999; #";
        validate("X", v).unwrap();
        let esc = escape_value(v);
        assert_eq!(esc, "\\\"; fastcgi_pass 127.0.0.1:9999; #");
        assert!(!esc.contains("\n"));
        // Every double quote in the emitted text is escaped → can't close the string.
        let emitted = format!("fastcgi_param X \"{esc}\";");
        let unescaped_quotes =
            emitted.match_indices('"').filter(|(i, _)| *i > 0 && emitted.as_bytes()[i - 1] != b'\\').count();
        assert_eq!(unescaped_quotes, 2, "only the outer quotes may be unescaped: {emitted}");
    }

    #[test]
    fn every_template_fcgi_param_is_reserved() {
        // Regression guard: a param added to the nginx template MUST be added
        // to RESERVED, or user vars could duplicate/override it.
        for (name, _) in crate::core::services::TEMPLATE_FCGI_PARAMS {
            assert!(
                RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)),
                "template param {name} missing from site_env::RESERVED"
            );
        }
    }

    #[test]
    fn validate_all_catches_duplicates_and_cap() {
        let dup = vec![("A".into(), "1".into()), ("A".into(), "2".into())];
        assert!(validate_all(&dup).unwrap_err().to_string().contains("duplicate"));
        let many: Vec<_> = (0..51).map(|i| (format!("V{i}"), String::new())).collect();
        assert!(validate_all(&many).unwrap_err().to_string().contains("at most"));
    }
}
