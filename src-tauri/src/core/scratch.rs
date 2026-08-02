//! Scratch sites — the agent-owned, disposable ones (MCP M2a).
//!
//! **This module is the ONE door between "a site" and "a site an agent may
//! touch."** M1's boundary was about the VERB — a read-only tool had no mutating
//! method to call ([`crate::mcp_server`]'s `ReadCtx`). M2 mutates by design, so
//! its structural half is about the OBJECT instead: every scratch mutator takes
//! a [`ScratchSite`], and the only way to obtain one is [`claim`], which reads
//! the row and checks `origin`. A code path that mutates the user's site does
//! not compile.
//!
//! The gate lives in `core`, not in the tool layer, so the CLI and the UI
//! inherit the same refusal — the tool layer is never the only thing standing
//! between an agent and the user's work (the M7 rule).
//!
//! **What the witness is NOT: a lock.** It proves the path was gated at the
//! moment it was claimed; it does not freeze `origin`, and the user can press
//! Keep a millisecond later. So the pairing this codebase uses elsewhere applies
//! here too — the witness is the compile-time proof that a path checked, and the
//! destructive WRITE re-asserts the recorded fact in its own `WHERE`
//! (`origin = 'agent'`, the shape [`crate::state::store::touch_site_expiry`]
//! already uses). Belt on the same fact from the other side; a one-time check on
//! a mutable fact is a snapshot, and this project has been bitten by treating
//! one as a guarantee.

use crate::error::{Error, Result};
use crate::state::models::{Site, SiteOrigin};
use crate::state::store;
use rusqlite::Connection;

/// Proof that a site is **agent-owned scratch**, recorded — not a name test, not
/// a path test. The private field IS the guarantee: there is no constructor, no
/// `From<Site>`, and the field is private to this module rather than
/// `pub(crate)`, so nothing anywhere in the crate can mint one from a `Site` it
/// happens to hold. [`claim`] is the only door, and it reads the row itself, so
/// a caller cannot supply a `Site` value it built or edited in memory.
#[derive(Debug, Clone)]
pub struct ScratchSite(
    // KEEP THIS FIELD PRIVATE. Attempting to build one elsewhere is a compile
    // error — and rustc helpfully suggests `pub Site` to make it go away, which
    // would delete the guarantee this type exists for and leave every scratch
    // mutator accepting the user's sites. If you arrived here from that
    // suggestion, the fix is `claim()`, not `pub`. Proven by plant-and-capture:
    // `ScratchSite(site)` → E0423, `ScratchSite { 0: site }` → E0451,
    // `site.into()` → E0277 (ledger #208).
    Site,
);

impl ScratchSite {
    /// The underlying row — read-only. Handing out `&Site` cannot launder the
    /// proof back into one: a `Site` grants nothing on its own, and going the
    /// other way still requires [`claim`].
    pub fn site(&self) -> &Site {
        &self.0
    }

    /// The stable site id, the value every scratch operation keys on.
    pub fn id(&self) -> &str {
        &self.0.id
    }

    /// The domain, for refusal copy and feed labels.
    pub fn domain(&self) -> &str {
        &self.0.domain
    }
}

/// The label every agent-created domain sits under: `<name>.scratch.<tld>`.
///
/// This is **UX, never policy** — it lets a human scanning the Sites list tell
/// at a glance which sites are disposable. Every decision reads
/// [`crate::state::models::SiteOrigin`]; a site the user hand-creates at
/// `mine.scratch.rex` is a normal site of theirs (`claim` refuses it, tested).
pub const SCRATCH_LABEL: &str = "scratch";

/// How many scratch sites may exist at once (PLAN §4.2). The answer to "an agent
/// creates twenty": it can't.
pub const MAX_SCRATCH_SITES: usize = 5;

/// Build the domain for a scratch site called `name` under `tld`.
///
/// `name` must be a SINGLE label: no dots, so an agent can neither nest
/// namespaces nor squat `scratch.<tld>` itself, and no leading/trailing dash.
/// The full domain is validated downstream by `sites::validate_domain` as well —
/// this is the shape rule, not the character rule.
pub fn scratch_domain(name: &str, tld: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other(
            "A scratch site needs a name — a single word, like `plugin-test`.".into(),
        ));
    }
    if name.contains('.') {
        return Err(Error::Other(format!(
            "`{name}` can't be a scratch site name: use a single word with no dots (rexenv adds \
             `.{SCRATCH_LABEL}.{tld}` itself, so `plugin-test` becomes `plugin-test.{SCRATCH_LABEL}.{tld}`)."
        )));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(Error::Other(format!(
            "`{name}` can't be a scratch site name: it must not start or end with a dash."
        )));
    }
    Ok(format!("{name}.{SCRATCH_LABEL}.{tld}"))
}

/// Refuse when the scratch pool is full, naming a WAY FORWARD.
///
/// A refusal that states the rule and stops is where a model starts improvising
/// — it will try a different name, then another, then reach for something else
/// entirely. So this lists the sites it can delete, by domain, and says the two
/// things it can actually do: delete one of its own, or ask the user (who can
/// Keep or remove them in rexenv). The count is the recorded fact, not an
/// estimate.
pub fn ensure_capacity(conn: &Connection) -> Result<()> {
    let mine: Vec<String> = store::list_sites(conn)?
        .into_iter()
        .filter(|s| s.is_scratch())
        .map(|s| s.domain)
        .collect();
    if mine.len() < MAX_SCRATCH_SITES {
        return Ok(());
    }
    Err(Error::Other(format!(
        "There are already {} scratch sites, which is the limit ({MAX_SCRATCH_SITES}): {}.\n\
         Delete one you no longer need with scratch_delete_site, or ask the person you're working \
         with — in rexenv they can remove one, or Keep one, which makes it theirs and frees a slot \
         straight away. Scratch sites also expire on their own once nothing has used them for a \
         while.",
        mine.len(),
        mine.join(", ")
    )))
}

/// The ONE conversion: read the row by id and prove it is the agent's, or refuse
/// with a sentence the agent can act on.
///
/// Both failures are deliberately **policy statements, not type errors leaking
/// into a tool result**. An agent reads these; "expected ScratchSite, found
/// Site" would tell it nothing about what to do next, and a bare "refused" would
/// invite it to retry the same call. So: name the site, name the rule, name the
/// way forward.
pub fn claim(conn: &Connection, id: &str) -> Result<ScratchSite> {
    let Some(site) = store::get_site(conn, id)? else {
        return Err(Error::Other(format!(
            "There is no site with id `{id}`. Use list_sites to see the sites that exist."
        )));
    };
    if site.origin != SiteOrigin::Agent {
        return Err(Error::Other(format!(
            "`{}` is one of your own sites, so agent tools cannot change or delete it. \
             They only work on scratch sites the agent created itself — make one with \
             scratch_create_site.",
            site.domain
        )));
    }
    Ok(ScratchSite(site))
}

/// Is this site STILL the agent's, right now?
///
/// The witness proves the path was gated when it claimed; this is the re-read
/// immediately before a destructive step, for the case the witness deliberately
/// does not cover — the user pressing Keep in between.
pub fn still_the_agents(conn: &Connection, id: &str) -> Result<bool> {
    Ok(store::get_site(conn, id)?.is_some_and(|s| s.is_scratch()))
}

/// Every scratch site currently DUE for reaping at `now`, as witnesses.
///
/// Goes through the same [`ScratchSite`] door as everything else, so the reaper
/// cannot end up holding a "site to delete" that no path proved was the agent's.
/// The predicate is [`Site::reap_due`] — the single expression of it — so this
/// enumerates rather than re-states the rule (a second copy of "and NULL means
/// never" is exactly how the two drift apart).
pub fn due_for_reap(conn: &Connection, now: &str) -> Result<Vec<ScratchSite>> {
    Ok(store::list_sites(conn)?
        .into_iter()
        .filter(|s| s.reap_due(now))
        .map(ScratchSite)
        .collect())
}

// ---------------------------------------------------------------------------
// The dev-plugin loop: derive the kind, then CLONE (S1 — never a symlink)
// ---------------------------------------------------------------------------

/// What a source tree is, read from its OWN header — never asserted by a caller.
///
/// This is what makes `kind` a fact rexenv derives: an agent cannot claim
/// "theme" for a plugin directory, because it never gets to claim anything. The
/// read happens on the SOURCE and BEFORE the clone, so a tree that is neither —
/// or ambiguously both — is refused rather than half-installed.
pub fn detect_kind(source: &std::path::Path) -> Result<&'static str> {
    let has_theme = source.join("style.css").is_file()
        && header_contains(&source.join("style.css"), "Theme Name:");
    let mut has_plugin = false;
    if let Ok(entries) = std::fs::read_dir(source) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "php") && header_contains(&p, "Plugin Name:") {
                has_plugin = true;
                break;
            }
        }
    }
    match (has_plugin, has_theme) {
        (true, false) => Ok("plugin"),
        (false, true) => Ok("theme"),
        (true, true) => Err(Error::Other(format!(
            "`{}` looks like BOTH a plugin and a theme (it has a plugin header and a theme \
             style.css). rexenv will not guess which — installing the wrong one means testing \
             against the wrong thing. Point at the plugin or theme directory itself.",
            source.display()
        ))),
        (false, false) => Err(Error::Other(format!(
            "`{}` has no plugin or theme header, so rexenv can't tell what it is. Point at the \
             directory that contains the plugin's main PHP file (with its `Plugin Name:` header) \
             or the theme's `style.css`.",
            source.display()
        ))),
    }
}

/// Does the first 8 KB of `path` contain `needle`? (WordPress headers live in
/// the file's opening comment block; reading the whole file would be wasteful on
/// a large plugin and pointless.)
fn header_contains(path: &std::path::Path, needle: &str) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = vec![0u8; 8 * 1024];
    let Ok(n) = f.read(&mut buf) else { return false };
    String::from_utf8_lossy(&buf[..n]).contains(needle)
}

/// A stat-only summary of a source tree: newest mtime, file count, total bytes.
///
/// **What it can and cannot tell.** A DIFFERENT fingerprint means the source
/// changed — reliable. An identical one is a strong hint that it did not, NOT a
/// proof: an edit that preserves mtime, size and file count simultaneously is
/// invisible to it. That is why the copy says "no changes detected since" rather
/// than "unchanged", and why the last-synced timestamp carries the rest.
///
/// Stat-only (no reads) so it costs milliseconds on a plugin tree and can run on
/// every status call; a content hash would be O(bytes) and could not.
pub fn fingerprint(source: &std::path::Path) -> Result<String> {
    let (mut newest, mut files, mut bytes) = (0u64, 0u64, 0u64);
    walk(source, &mut |md: &std::fs::Metadata| {
        files += 1;
        bytes += md.len();
        if let Ok(t) = md.modified() {
            if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                newest = newest.max(d.as_secs());
            }
        }
    })?;
    Ok(format!("{newest}-{files}-{bytes}"))
}

fn walk(dir: &std::path::Path, f: &mut impl FnMut(&std::fs::Metadata)) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let md = entry.metadata()?;
        if md.is_dir() {
            walk(&entry.path(), f)?;
        } else if md.is_file() {
            f(&md);
        }
        // Symlinks are neither walked nor followed — a source tree's link is
        // not ours to chase out of the directory the user pointed at.
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The raw runner's ONE screen: which site, never which command (D1)
// ---------------------------------------------------------------------------

/// The WP-CLI global parameters that would point a command at something other
/// than the site rexenv proved the agent owns.
///
/// Exhaustive for the argv surface: `--path` (a different install), `--url` (a
/// different site inside a network), `--ssh` (a different MACHINE), `--http` (a
/// remote install over HTTP). Aliases (`@name`) are handled separately because
/// they are positional, and an alias in `~/.wp-cli/config.yml` can carry `path:`
/// or `ssh:` — the same redirect wearing a different hat.
pub const WP_TARGET_PARAMS: &[&str] = &["--path", "--url", "--ssh", "--http"];

/// Refuse an argv that names its own target. **The tier boundary, restated at
/// the one place a raw runner could walk around it.**
///
/// rexenv supplies `--path` from the claimed [`ScratchSite`]'s recorded docroot.
/// Appending ours is not enough on its own: in WP-CLI a later `--path` wins, so
/// an agent that passes one is either overriding us or racing our position in
/// the argv. This refuses instead — the target is rexenv's to decide, and there
/// is exactly one way to say which site: `site_id`.
///
/// **Why this is NOT the guard S1 rejected.** That one screened VERBS — is this
/// subcommand safe? — over an open-ended set (`--force`, `wp eval`, `wp
/// package`, an alias, tomorrow's subcommand), which is the
/// guard-covers-a-narrower-surface-than-its-claim family that has already cost
/// this codebase a cross-site exposure, a data-destruction hole and a leak.
/// This screens TARGETS: a closed, small, documented set that IS the tier
/// boundary itself. Refusing "which site" is enforceable; refusing "which
/// command" isn't — which is exactly why there is no subcommand denylist here
/// (see [`crate::mcp_server::scratch`]'s `wp_run`).
///
/// **What it does not claim.** It is not containment. `wp eval` runs arbitrary
/// PHP as the user, and PHP can open any path on the machine (§3.1/#197) — the
/// screen keeps the TOOL's target honest, so `origin='agent'` still means
/// something for every command rexenv itself routes. It does not, and cannot,
/// bound what the code inside a scratch site reaches.
pub fn refuse_wp_target_override(argv: &[String]) -> Result<()> {
    for arg in argv {
        // An alias is positional and can carry `path:`/`ssh:` from the user's
        // own wp-cli config — a redirect rexenv cannot see the contents of.
        if arg.starts_with('@') {
            return Err(Error::Other(format!(
                "`{arg}` is a WP-CLI alias, and rexenv doesn't run commands through aliases: an \
                 alias can point at another install, or another machine, and rexenv decides which \
                 site a command runs against — the scratch site you named in `site_id`. Drop it \
                 and the command runs against that site."
            )));
        }
        let lower = arg.to_ascii_lowercase();
        if let Some(param) = WP_TARGET_PARAMS
            .iter()
            .find(|p| lower == **p || lower.starts_with(&format!("{p}=")))
        {
            return Err(Error::Other(format!(
                "`{param}` isn't allowed here: rexenv decides which site a command runs against, \
                 from the scratch site you named in `site_id`, and it adds `--path` itself. Drop \
                 `{param}` and run the command again. To act on a different site, name it in \
                 `site_id` — it has to be a scratch site the agent created."
            )));
        }
    }
    Ok(())
}

/// How long a single agent-run WP-CLI command may take before rexenv kills it.
///
/// Generous enough for a plugin install that downloads from wordpress.org,
/// bounded because a raw runner can reach an interactive or wedged child and a
/// hung tool call is an agent that never comes back.
pub const WP_RUN_TIMEOUT_SECS: u64 = 180;

/// Copy `source` into `dest` — a SNAPSHOT, so nothing the scratch site does can
/// reach back to the user's checkout.
///
/// The guarantee is the direction, not the mechanism: writes inside the copy
/// never touch the source, and writes in the source never appear in the copy
/// until the next sync. On APFS this can be a copy-on-write clone (near-free);
/// this implementation is the portable one and is correct everywhere — the
/// `cp -c` optimisation rides a platform trait and is a speed change, never a
/// behaviour change.
pub fn clone_tree(source: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let md = entry.metadata()?;
        let target = dest.join(entry.file_name());
        if md.is_dir() {
            clone_tree(&entry.path(), &target)?;
        } else if md.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::test_site;

    /// A db holding one scratch site, one of the user's own, and one scratch
    /// site that has expired — all in production shape.
    fn db() -> Connection {
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut scratch =
            test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        scratch.expires_at = Some("2099-01-01 00:00:00".into());
        store::insert_site(&conn, &scratch).unwrap();
        let real = test_site("7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30", "myblog.rex", SiteOrigin::User);
        store::insert_site(&conn, &real).unwrap();
        let mut expired =
            test_site("d17c9b30-5f2e-4a68-b1d4-9c3e7a2f5011", "old.scratch.rex", SiteOrigin::Agent);
        expired.expires_at = Some("2020-01-01 00:00:00".into());
        store::insert_site(&conn, &expired).unwrap();
        conn
    }

    #[test]
    fn claim_proves_an_agent_owned_site() {
        let conn = db();
        let s = claim(&conn, "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24").expect("scratch claims");
        assert_eq!(s.domain(), "probe.scratch.rex");
        assert_eq!(s.id(), "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24");
        assert!(s.site().is_scratch());
    }

    #[test]
    fn claiming_the_users_own_site_refuses_with_a_policy_statement() {
        // What an AGENT reads. It has to say which site, which rule, and what to
        // do instead — a type error or a bare "refused" would leave the agent to
        // guess, and guessing means retrying the same call on the same site.
        let conn = db();
        let err = claim(&conn, "7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30").unwrap_err().to_string();
        assert!(err.contains("myblog.rex"), "names the site: {err}");
        assert!(err.contains("your own sites"), "names the rule: {err}");
        assert!(err.contains("scratch_create_site"), "names the way forward: {err}");
        // And it never leaks an internal — the docroot is on the row we just read.
        assert!(!err.contains("/Users/"), "no path in a refusal: {err}");
        assert!(!err.contains("wp_"), "no database name in a refusal: {err}");
    }

    #[test]
    fn claiming_a_site_that_does_not_exist_says_so_distinctly() {
        // Distinct from the refusal above: "there is no such site" and "that site
        // is yours" send an agent to different next steps.
        let conn = db();
        let err = claim(&conn, "00000000-0000-4000-8000-000000000000").unwrap_err().to_string();
        assert!(err.contains("no site with id"), "{err}");
        assert!(err.contains("list_sites"), "names the way forward: {err}");
        assert!(!err.contains("your own sites"), "not the ownership refusal: {err}");
    }

    #[test]
    fn a_site_the_user_hand_named_scratch_is_still_the_users() {
        // The suffix is UX for humans scanning the Sites list. Policy reads
        // `origin`, never the name — so a user who creates `mine.scratch.rex`
        // themselves owns a normal site no agent tool can touch.
        let conn = db();
        let impostor = test_site("a1b2c3d4-1111-4222-8333-444455556666", "mine.scratch.rex", SiteOrigin::User);
        store::insert_site(&conn, &impostor).unwrap();
        assert!(claim(&conn, &impostor.id).is_err(), "the NAME must not grant scratch status");
    }

    #[test]
    fn due_for_reap_yields_witnesses_only_for_rows_the_predicate_admits() {
        let conn = db();
        let due = due_for_reap(&conn, "2026-08-01 12:00:00").unwrap();
        assert_eq!(due.len(), 1, "only the expired scratch site");
        assert_eq!(due[0].domain(), "old.scratch.rex");
        // Everything it hands back is, by type, something a path proved is the
        // agent's — the reaper can never hold a "site to delete" that isn't.
        assert!(due.iter().all(|s| s.site().is_scratch()));
    }

    fn plugin_src(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rexenv-clone-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inc")).unwrap();
        std::fs::write(dir.join("acme.php"), "<?php\n/**\n * Plugin Name: Acme\n */\n").unwrap();
        std::fs::write(dir.join("inc/lib.php"), b"<?php // lib\n").unwrap();
        std::fs::write(dir.join("inc/blob.bin"), [0u8, 159, 146, 150]).unwrap();
        dir
    }

    #[test]
    fn the_kind_is_read_from_the_source_and_ambiguity_is_refused() {
        let dir = plugin_src("kind");
        assert_eq!(detect_kind(&dir).unwrap(), "plugin");
        // A theme.
        let theme = dir.join("theme");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join("style.css"), "/*\nTheme Name: Acme\n*/\n").unwrap();
        assert_eq!(detect_kind(&theme).unwrap(), "theme");
        // BOTH — refused, never guessed: the wrong install ends with an agent
        // testing against the wrong thing and reporting confidently.
        std::fs::write(dir.join("style.css"), "/*\nTheme Name: Acme\n*/\n").unwrap();
        let err = detect_kind(&dir).unwrap_err().to_string();
        assert!(err.contains("BOTH") && err.contains("will not guess"), "{err}");
        // Neither.
        let plain = dir.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let err = detect_kind(&plain).unwrap_err().to_string();
        assert!(err.contains("no plugin or theme header"), "{err}");
        assert!(err.contains("Plugin Name:"), "says what it looked for: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_inside_the_clone_leaves_the_source_byte_identical() {
        // THE guarantee S1 rests on, asserted directly rather than through the
        // copy mechanism — this passes whether APFS cloned or bytes were copied.
        let src = plugin_src("bytes");
        let before: Vec<(std::path::PathBuf, Vec<u8>)> = ["acme.php", "inc/lib.php", "inc/blob.bin"]
            .iter()
            .map(|p| (src.join(p), std::fs::read(src.join(p)).unwrap()))
            .collect();
        let dest = src.with_extension("copy");
        let _ = std::fs::remove_dir_all(&dest);
        clone_tree(&src, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("inc/blob.bin")).unwrap(), vec![0u8, 159, 146, 150]);

        // Write inside the copy three ways.
        std::fs::write(dest.join("acme.php"), "<?php // clobbered by the agent\n").unwrap();
        std::fs::write(dest.join("new.php"), "<?php // added\n").unwrap();
        std::fs::remove_file(dest.join("inc/lib.php")).unwrap();

        for (path, bytes) in &before {
            assert!(path.exists(), "the source lost a file: {}", path.display());
            assert_eq!(&std::fs::read(path).unwrap(), bytes, "the source changed: {}", path.display());
        }
        assert!(!src.join("new.php").exists(), "a file created in the copy appeared in the source");

        // ...and the reverse: the site runs a SNAPSHOT, so a source edit after
        // the clone is invisible until the next sync. That is what the sync verb
        // exists for, so it is pinned rather than implied.
        std::fs::write(src.join("acme.php"), "<?php\n/**\n * Plugin Name: Acme 2\n */\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.join("acme.php")).unwrap(),
            "<?php // clobbered by the agent\n",
            "a source edit must not appear in the clone"
        );
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn the_fingerprint_changes_when_the_source_does() {
        let dir = plugin_src("fp");
        let a = fingerprint(&dir).unwrap();
        assert_eq!(a, fingerprint(&dir).unwrap(), "stable when nothing changed");
        std::fs::write(dir.join("inc/lib.php"), b"<?php // lib, edited and longer\n").unwrap();
        assert_ne!(a, fingerprint(&dir).unwrap(), "a changed source must be detectable");
        let b = fingerprint(&dir).unwrap();
        std::fs::write(dir.join("extra.php"), b"<?php\n").unwrap();
        assert_ne!(b, fingerprint(&dir).unwrap(), "a new file changes it too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn every_way_an_argv_can_name_its_own_target_is_refused() {
        // The closed set, in every FORM wp-cli accepts it: `--x=v`, `--x v`
        // (refusing the flag is enough — its value is the next token), and a
        // positional alias. Passing ours and hoping is not enough, because a
        // later `--path` wins in wp-cli: this refuses rather than races.
        for param in WP_TARGET_PARAMS {
            for form in [
                argv(&["plugin", "list", &format!("{param}=/somewhere/else")]),
                argv(&["plugin", "list", param, "/somewhere/else"]),
                // First position, before the subcommand, and upper-cased — a
                // screen that only looked at the tail, or only at the exact
                // lowercase spelling, would pass these.
                argv(&[&format!("{param}=/somewhere/else"), "plugin", "list"]),
                argv(&[&param.to_ascii_uppercase(), "/somewhere/else", "plugin", "list"]),
                // After a `--` separator, where a caller might expect parsing to stop.
                argv(&["eval-file", "x.php", "--", &format!("{param}=/elsewhere")]),
            ] {
                let err = refuse_wp_target_override(&form)
                    .expect_err(&format!("must refuse {form:?}"))
                    .to_string();
                assert!(err.contains(param), "the refusal names the parameter: {err}");
                assert!(err.contains("site_id"), "and the ONE way to say which site: {err}");
            }
        }
        // An alias is positional and can carry `path:`/`ssh:` from the user's own
        // wp-cli config, so it is the same redirect wearing a different hat.
        let err = refuse_wp_target_override(&argv(&["@prod", "plugin", "list"]))
            .expect_err("an alias must be refused")
            .to_string();
        assert!(err.contains("@prod") && err.contains("alias"), "{err}");
    }

    #[test]
    fn the_screen_refuses_targets_and_leaves_every_command_alone() {
        // The other half, and the one that says what this guard IS. There is no
        // subcommand denylist: `eval` (arbitrary PHP), `db query`, `plugin
        // install --force` and `option update` all pass, deliberately — plugin
        // activation already grants arbitrary user-level PHP (#197), so
        // screening verbs would buy nothing while LOOKING like protection. What
        // is screened is the target, which is the tier boundary itself.
        for allowed in [
            argv(&["eval", "echo WP_HOME;"]),
            argv(&["db", "query", "SELECT 1"]),
            argv(&["plugin", "install", "acme", "--force", "--activate"]),
            argv(&["option", "update", "home", "https://x.scratch.rex"]),
            // Arguments that merely CONTAIN a target word or an @ are not targets.
            argv(&["user", "create", "bob", "bob@example.com"]),
            argv(&["option", "update", "siteurl", "--skip-plugins"]),
            argv(&["config", "set", "WP_DEBUG", "true", "--raw"]),
        ] {
            refuse_wp_target_override(&allowed)
                .unwrap_or_else(|e| panic!("must not screen the COMMAND: {allowed:?} → {e}"));
        }
    }
}
