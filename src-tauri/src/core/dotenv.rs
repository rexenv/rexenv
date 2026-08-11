//! core::dotenv — the `.env` writer, shared by every framework that keeps its
//! configuration there.
//!
//! Extracted from `core::laravel` the moment a SECOND caller appeared
//! (Bedrock's WordPress config), which is the point at which one writer stops
//! being a Laravel detail and becomes a fact about `.env` files. Two copies of
//! this would have been two answers to "does a commented-out key count as
//! present" — and the one that got it wrong would leave the file with two
//! values for the same key and no way to tell which one PHP reads.
//!
//! Deliberately simple, and deliberately NOT
//! [`crate::core::confedit`]'s refuse-rather-than-guess machinery: the files
//! this touches are ones rexenv just wrote, or a repository's own
//! `.env.example` copied a second ago — not a config a developer has been
//! hand-editing for a year. It writes values UNQUOTED, so a caller's value must
//! not need quoting; every value that reaches it today is one of our own
//! identifiers, a port, a generated salt, or an https URL built from an
//! already-validated domain.

/// Set one `KEY=value`: replaces the first live line for the key, un-comments
/// and replaces a `# KEY=…` line, or appends the entry.
///
/// A commented-out key counts as PRESENT and is replaced in place. That is the
/// load-bearing half: `.env.example` files ship their database block commented
/// out, and appending beside those lines would leave the file with two answers
/// for one key.
pub fn set_key(text: &str, key: &str, value: &str) -> String {
    let line = format!("{key}={value}");
    let mut out: Vec<String> = Vec::new();
    let mut written = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start().trim_start_matches("export ").trim_start();
        let is_live = trimmed.starts_with(&format!("{key}="));
        let is_commented = trimmed
            .strip_prefix('#')
            .map(|rest| rest.trim_start().starts_with(&format!("{key}=")))
            .unwrap_or(false);
        if (is_live || is_commented) && !written {
            out.push(line.clone());
            written = true;
        } else if is_live || is_commented {
            // A duplicate for the same key: drop it rather than leave a second
            // answer below the one we just wrote.
            continue;
        } else {
            out.push(raw.to_string());
        }
    }
    if !written {
        out.push(line);
    }
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// [`set_key`] for a whole block, applied in order.
pub fn set_keys<'a>(text: &str, pairs: impl IntoIterator<Item = (&'a str, String)>) -> String {
    let mut out = text.to_string();
    for (key, value) in pairs {
        out = set_key(&out, key, &value);
    }
    out
}

/// The value of the FIRST live line for `key`, unquoted, or `None` when the key
/// is absent.
///
/// First rather than last on purpose: phpdotenv's immutable reader (what
/// Laravel and Bedrock both use) keeps the first assignment it sees. Callers
/// should not have to know that — [`set_key`] collapses duplicates on every
/// write, so a file this module has touched has exactly one line per key.
pub fn value_of(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    text.lines()
        .map(|raw| raw.trim_start().trim_start_matches("export ").trim_start())
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|rest| rest.trim().trim_matches(['"', '\'']).to_string())
}

/// Is a key absent or set to an empty value?
///
/// The question WordPress's salts need: Bedrock's `.env.example` ships them as
/// bare `AUTH_KEY=`, so "present" and "set" are different facts and only the
/// second one means leave it alone.
pub fn is_blank(text: &str, key: &str) -> bool {
    value_of(text, key).map_or(true, |v| v.is_empty())
}

/// Give `key` a value it does not have yet, and collapse any duplicate lines
/// for it either way.
///
/// The "either way" is the whole point, and a test caught it: skipping the
/// write when the key already had a value left a SECOND, blank line for that
/// key further down the file — exactly the two-answers state this module exists
/// to prevent, and one that a reader with last-wins semantics would resolve the
/// wrong way. So the existing value is re-written rather than left alone.
pub fn fill_if_blank(text: &str, key: &str, make: impl FnOnce() -> String) -> String {
    let value = value_of(text, key).filter(|v| !v.is_empty()).unwrap_or_else(make);
    set_key(text, key, &value)
}

/// Where a CLONED project's `.env` came from. Returned rather than logged
/// inside, so the caller can say it in the job log — "copied from
/// `.env.example`" and "kept the one the repository committed" are different
/// facts, and a developer debugging their config needs to know which happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvOrigin {
    /// The repository committed a `.env` (against every framework's own advice,
    /// but it happens). KEPT — the caller rewrites only the keys it owns.
    /// Replacing a file the repo shipped would silently drop the mail, queue
    /// and third-party keys the app needs.
    Repo,
    /// Copied from the repository's `.env.example` — the normal case, and the
    /// step `composer install` will NOT do: Laravel's copy is
    /// `create-project`'s `post-root-package-install` script, which never fires
    /// on a plain install, and Bedrock has no such script at all.
    Example,
    /// Neither existed, so the caller's `seed` was written. Named separately
    /// because "we invented this file" is a thing the log must be able to say.
    Seeded,
}

/// The `.env` a freshly cloned project needs, without ever overwriting one.
///
/// Idempotent by construction: an existing `.env` is REPORTED, not rewritten,
/// so a Retry after a later phase failed cannot discard a generated key or a
/// credential the first run (or the developer) already put there.
pub fn ensure_file(project: &std::path::Path, seed: &str) -> crate::error::Result<EnvOrigin> {
    let env = project.join(".env");
    if env.exists() {
        return Ok(EnvOrigin::Repo);
    }
    let example = project.join(".env.example");
    if example.is_file() {
        std::fs::copy(&example, &env).map_err(|e| {
            crate::error::Error::Other(format!("copying .env.example to .env failed: {e}"))
        })?;
        return Ok(EnvOrigin::Example);
    }
    std::fs::write(&env, seed).map_err(|e| {
        crate::error::Error::Other(format!("writing {} failed: {e}", env.display()))
    })?;
    Ok(EnvOrigin::Seeded)
}

/// A 64-character secret for a `.env` key, from the same CSPRNG `uuid` v4 uses.
///
/// Hex rather than WordPress's own wider alphabet, on purpose: these values are
/// written UNQUOTED, and hex cannot contain a quote, a backslash, a `#` or a
/// `$` that phpdotenv would try to expand. Two v4 UUIDs is 244 bits of entropy
/// in 64 characters — more than the 256-bit-equivalent alphabet soup WP ships,
/// and impossible to mangle on the way in.
pub fn generate_secret() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape that matters: a `.env.example` with its database block
    /// COMMENTED OUT. An appended value beside those lines is the bug this
    /// writer exists to avoid — the file would hold two answers and the reader
    /// could not tell which one PHP takes.
    #[test]
    fn a_commented_key_is_replaced_in_place_never_appended_beside() {
        let text = "APP_NAME=Shop\n# DB_HOST=127.0.0.1\nMAIL_MAILER=log\n";
        let out = set_key(text, "DB_HOST", "127.0.0.1:13306");
        assert_eq!(out.matches("DB_HOST").count(), 1, "one answer per key: {out}");
        assert!(out.contains("DB_HOST=127.0.0.1:13306"));
        assert!(!out.contains("# DB_HOST"));
        // Its neighbours keep their places.
        assert!(out.starts_with("APP_NAME=Shop\n"));
        assert!(out.contains("MAIL_MAILER=log"));
        assert!(out.ends_with('\n'), "the trailing newline is preserved");
    }

    #[test]
    fn duplicates_collapse_and_an_exported_key_is_replaced_not_shadowed() {
        // Two live lines for one key: the FIRST is rewritten, the second is
        // dropped rather than left below the new value.
        let out = set_key("K=a\nX=1\nK=b\n", "K", "final");
        assert_eq!(out, "K=final\nX=1\n");
        // `export K=…` is valid `.env` and the readers tolerate it.
        let out = set_key("export DB_HOST=db.internal\n", "DB_HOST", "127.0.0.1");
        assert_eq!(out.matches("DB_HOST=").count(), 1);
        assert!(!out.contains("db.internal"));
        // A key that is not there at all is appended.
        assert_eq!(set_key("A=1\n", "B", "2"), "A=1\nB=2\n");
        // No trailing newline in, none invented out.
        assert_eq!(set_key("A=1", "B", "2"), "A=1\nB=2");
    }

    #[test]
    fn set_keys_applies_a_block_in_order() {
        let out = set_keys(
            "DB_CONNECTION=sqlite\n# DB_HOST=x\n",
            [("DB_CONNECTION", "mysql".to_string()), ("DB_HOST", "127.0.0.1".to_string())],
        );
        assert!(out.contains("DB_CONNECTION=mysql") && out.contains("DB_HOST=127.0.0.1"));
        assert!(!out.contains("sqlite") && !out.contains("# DB_HOST"));
    }

    /// The bug a Bedrock test caught: a key that already had a value was left
    /// alone, and a duplicate BLANK line for it survived further down the file.
    #[test]
    fn filling_a_key_collapses_its_duplicates_even_when_it_keeps_the_old_value() {
        let text = "AUTH_KEY=already-real\nOTHER=1\nAUTH_KEY=\n";
        let out = fill_if_blank(text, "AUTH_KEY", || "NEW".into());
        assert_eq!(out.lines().filter(|l| l.starts_with("AUTH_KEY=")).count(), 1, "{out}");
        assert!(out.contains("AUTH_KEY=already-real"), "the real value wins: {out}");
        assert!(out.contains("OTHER=1"));

        // Blank → filled.
        let out = fill_if_blank("AUTH_KEY=\n", "AUTH_KEY", || "NEW".into());
        assert_eq!(out, "AUTH_KEY=NEW\n");
        // Absent → appended.
        assert_eq!(fill_if_blank("A=1\n", "B", || "x".into()), "A=1\nB=x\n");
        // A neighbouring key that merely CONTAINS this one's name is untouched.
        let out = fill_if_blank("SECURE_AUTH_KEY=keep\nAUTH_KEY=\n", "AUTH_KEY", || "NEW".into());
        assert!(out.contains("SECURE_AUTH_KEY=keep") && out.contains("AUTH_KEY=NEW"));
    }

    #[test]
    fn value_of_reads_the_first_assignment_which_is_the_one_phpdotenv_keeps() {
        assert_eq!(value_of("K=first\nK=second\n", "K").as_deref(), Some("first"));
        assert_eq!(value_of("export K=\"quoted\"\n", "K").as_deref(), Some("quoted"));
        assert_eq!(value_of("K=\n", "K").as_deref(), Some(""));
        assert_eq!(value_of("OTHER=1\n", "K"), None, "absent is not empty");
        // A commented line is not an assignment.
        assert_eq!(value_of("# K=old\n", "K"), None);
    }

    #[test]
    fn a_generated_secret_is_unquotable_and_never_repeats() {
        let a = generate_secret();
        assert_eq!(a.len(), 64);
        // Written unquoted into `.env`, so nothing in it may need quoting or
        // mean something to phpdotenv.
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
        assert_ne!(a, generate_secret());
        // And it survives the writer intact.
        let out = set_key("AUTH_KEY=\n", "AUTH_KEY", &a);
        assert!(out.contains(&format!("AUTH_KEY={a}")));
        assert!(!is_blank(&out, "AUTH_KEY"));
    }

    #[test]
    fn blank_means_absent_or_empty_never_merely_present() {
        // Bedrock ships its salts as bare `AUTH_KEY=` — present, unset.
        assert!(is_blank("AUTH_KEY=\n", "AUTH_KEY"));
        assert!(is_blank("AUTH_KEY=   \n", "AUTH_KEY"));
        assert!(is_blank("AUTH_KEY=''\n", "AUTH_KEY"));
        assert!(is_blank("AUTH_KEY=\"\"\n", "AUTH_KEY"));
        assert!(is_blank("OTHER=1\n", "AUTH_KEY"), "absent is blank");
        // A real value is not blank, and must therefore be left alone.
        assert!(!is_blank("AUTH_KEY=generated-secret\n", "AUTH_KEY"));
        assert!(!is_blank("export AUTH_KEY=generated-secret\n", "AUTH_KEY"));
        // A COMMENTED key is not a value — the first live line decides, and a
        // commented one leaves the key unset.
        assert!(is_blank("# AUTH_KEY=old\n", "AUTH_KEY"));
    }
}
