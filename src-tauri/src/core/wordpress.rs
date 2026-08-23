//! core::wordpress — WP-CLI wrapper (Phase 1 §9).
//!
//! WP-CLI is a `.phar` run through the bundled PHP: `php wp-cli.phar <args>`.
//! Invocation is identical on every OS (the per-OS bit is which `php` binary,
//! resolved via `BinaryProvider`), so this runs the command directly and
//! captures output (unlike `ProcessSupervisor`, which is for long-lived services).
//!
//! # The COMMAND SET is pinned too, not just the binary (ledger #228)
//!
//! Until 13 Aug 2026 every spawn here inherited the ambient environment and
//! none set `WP_CLI_PACKAGES_DIR`, so WP-CLI also loaded `~/.wp-cli/packages/`
//! — whatever that user composer-installed there, at whatever version,
//! whenever. Verified 4 Aug 2026: `wp dist-archive` answered from a package
//! installed on a dev machine in **December 2021**, while the same phar with an
//! empty packages dir replies `'dist-archive' is not a registered wp command`.
//! The cost was never the posture, it was REPRODUCIBILITY: a bug in anything
//! below could depend on a directory appearing in no log, no diff and no bug
//! report, so "works here" and "fails there" had no visible cause.
//!
//! Now every `wp` **rexenv runs for a user** is pinned to the bundled phar and
//! nothing else ([`wp_packages::neutral_packages_path`]). Two things about that
//! sentence are load-bearing:
//!
//! - **"runs for a user"** is the whole scope. `core::terminal`'s `wp` wrapper
//!   is deliberately NOT pinned: that is the user's command line, and pinning it
//!   would break `wp package install` from inside rexenv in a way that looks
//!   like our bug. The tell says so (Settings, `WP-CLI packages`), and that
//!   sentence is only true because of this exemption.
//! - **Coverage is a property of this tree, not a list.** The ledger row named
//!   four spawn sites; by the time it was worked there were seven, two added
//!   after it was written. So there is ONE argv builder ([`wp_argv_prefix`]) and
//!   ONE `Command::new(php_bin)` ([`wp_command`]), and
//!   `every_wp_cli_argv_in_the_tree_is_pinned` fails the build when a second of
//!   either appears. Counting the sites is what would have shipped a guard
//!   narrower than its own claim.
//!
//! **Anything that must run a command we chose bundles it and passes
//! `--require`** (`core::wp_packages`), rather than trusting resolution it does
//! not control — that rule is what makes the pin cost nothing.
//!
//! # STDOUT is the command's, at both ends (ledger #316, #317)
//!
//! wp-cli's stdout is not only wp-cli's, and it gets written to from BOTH
//! directions. Both halves were reported as the same bug — "the WordPress tab
//! is dead on this site" — and they need different fixes, which is why they are
//! written down separately.
//!
//! **The TAIL (#316) — a plugin's shutdown hook.** Elementor 4.2.2 registers its
//! own WP-CLI logger and prints every PHP notice it collected, AFTER the
//! command's own output: `Manager::shutdown` → `Cli_Logger::save_log` →
//! `WP_CLI::log` → `fwrite(STDOUT)` (traced 14 Aug 2026). So
//! `wp plugin list --format=json` returned valid JSON followed by a deprecation
//! notice, the WordPress screen said `bad JSON: trailing characters at line 1
//! column 814`, and nothing on it could be managed. `wp option get home` came
//! back with the notice glued to the URL, so it was never a JSON problem.
//!
//! Two plausible fixes were measured and neither touches THIS half, so don't
//! re-try them here: `-d display_errors=stderr` (the write is not PHP's error
//! display) and an output buffer opened from a shutdown function (the write is
//! not `echo` — it is `fwrite` straight to the stream, which the output layer
//! never sees). What holds whatever a plugin writes WITH is POSITION: rexenv
//! passes its own `--require` file, wp-cli loads it before WordPress and before
//! any plugin, so its `register_shutdown_function` is FIRST in the queue and its
//! marker prints after wp-cli's output and before anything a later shutdown hook
//! writes. Captured stdout is cut there ([`split_at_eoo`]) and the tail is
//! APPENDED TO STDERR, never dropped — the notice is a real problem on the
//! user's site, just not part of the answer to `plugin list`.
//!
//! **The HEAD (#317) — PHP's own diagnostics.** The CLI SAPI prints them to
//! STDOUT by default, so a deprecation raised before wp-cli has printed a byte
//! arrives glued to the FRONT of the answer. Measured on PHP 8.5.8 with the
//! pinned 2.12.0 phar: `Deprecated: Case statements followed by a semicolon (;)
//! are deprecated … react/promise/src/functions.php on line 369` — the phar's
//! OWN vendored code, on every command, so every read on every 8.5 site broke
//! and no plugin was involved at all. Here `-d display_errors=stderr` IS the fix
//! ([`wp_argv_prefix`]): it covers any diagnostic from any file at any moment,
//! which no marker can, and it loses nothing (stderr is where a diagnostic
//! belongs, and the streamed steps merge both streams into one live log).
//!
//! Scope, stated rather than implied:
//!
//! - the CUT covers the CAPTURED path — the one whose stdout is parsed. Streamed
//!   spawns don't carry the marker file: their output is shown line by line,
//!   where a trailing notice is noise rather than a parse failure, and a marker
//!   would have to be filtered out of the user's live log instead. The
//!   display_errors half is in the shared prefix and so covers both.
//! - what remains uncovered is a plugin that `echo`es to stdout while the
//!   command runs — not a diagnostic, not after the end, and broken for every
//!   wp-cli user alike. The JSON reads additionally tolerate trailing bytes
//!   ([`json_from_wp`]), the belt for a machine where the require file could not
//!   be written.

use super::wp_packages;
use crate::core::db::SqlClient;
use crate::error::{Error, Result};
use crate::state::models::SiteType;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

/// Run `php <wp_phar> <args>` (optionally in `cwd`) and return the raw `Output`.
/// The eight secrets WordPress derives its cookies and nonces from. Bedrock
/// keeps them in `.env`; stock WordPress keeps them in `wp-config.php`, where
/// `wp config create` already generates them for us.
pub const SALT_KEYS: [&str; 8] = [
    "AUTH_KEY",
    "SECURE_AUTH_KEY",
    "LOGGED_IN_KEY",
    "NONCE_KEY",
    "AUTH_SALT",
    "SECURE_AUTH_SALT",
    "LOGGED_IN_SALT",
    "NONCE_SALT",
];

/// Bedrock's `.env` seed, for the (rare) repository that ships no
/// `.env.example`. Only the keys `wire_bedrock_env` does not set itself.
pub const BEDROCK_ENV_SEED: &str = "WP_ENV=development\n";

/// Point a Bedrock/Radicle project's `.env` at this site.
///
/// The counterpart of `wp config create` for the composer-managed WordPress
/// layouts, which have no `wp-config.php` of ours to write: `config/`
/// application.php` reads every one of these through `env()`, so this file IS
/// the configuration.
///
/// Two things it does NOT do, both deliberate:
///
/// - **Salts already set are left alone.** A repository that committed real
///   salts — or a Retry after the first run generated them — must not have them
///   rotated underneath it: every logged-in session and every nonce dies with
///   the old value. Only a blank key (`AUTH_KEY=`, which is what `.env.example`
///   ships) gets one.
/// - **`WP_SITEURL` is written as the literal `${WP_HOME}/wp`**, not expanded.
///   That is Bedrock's own convention and phpdotenv resolves it at load; baking
///   the domain in twice means a rename fixes one of them.
///
/// Returns the rewritten text rather than writing it, so the whole edit is
/// unit-testable against real `.env` shapes without a filesystem.
///
/// **UNVERIFIED against a real Bedrock project** — the key set and the
/// `${WP_HOME}/wp` convention come from Bedrock's documented `.env.example`,
/// not from a live install (the same honest caveat `detect_content_dir_rel`
/// carries for Radicle). Whoever first clones one should confirm the site boots
/// before trusting this.
pub fn wire_bedrock_env(original: &str, home_url: &str, db: &BedrockDb) -> String {
    let mut text = crate::core::dotenv::set_keys(
        original,
        [
            ("DB_NAME", db.name.clone()),
            ("DB_USER", db.user.clone()),
            ("DB_PASSWORD", db.password.clone()),
            // WordPress accepts `host:port` in DB_HOST, which is how a bundled
            // engine on a non-default port is reachable at all.
            ("DB_HOST", db.host.clone()),
            ("WP_ENV", "development".to_string()),
            ("WP_HOME", home_url.to_string()),
            ("WP_SITEURL", "${WP_HOME}/wp".to_string()),
        ],
    );
    for key in SALT_KEYS {
        text = crate::core::dotenv::fill_if_blank(&text, key, crate::core::dotenv::generate_secret);
    }
    text
}

/// The database half of a Bedrock `.env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedrockDb {
    pub name: String,
    pub user: String,
    pub password: String,
    /// `host:port` — the bundled engines do not run on 3306.
    pub host: String,
}

/// The directory wp-cli must treat as the WordPress root for `docroot`.
///
/// Roots' layouts are the whole reason this exists: Composer installs core into
/// `<docroot>/wp`, so wp-cli started in `docroot` finds no `wp-load.php` and
/// refuses with *"This does not seem to be a WordPress installation"* — which is
/// exactly how a cloned Bedrock site failed its `wp core install` before this
/// (found by `git_site_provision_check`, 11 Aug 2026).
///
/// Derived at USE time rather than recorded, which is the opposite of what this
/// codebase does for `content_dir` — deliberately, and the difference is who
/// owns the directory. The content dir is a place rexenv WRITES into, so a
/// use-time probe could be poisoned by rexenv's own past mistakes (v24's
/// reasoning). The core dir is a place Composer owns and rexenv only reads;
/// there is nothing of ours in `<docroot>/wp` to mislead a later probe, and
/// deriving it covers a LINKED Bedrock checkout too — which has no creation
/// moment at which anything could have been recorded.
pub fn core_root(docroot: &Path) -> PathBuf {
    if !docroot.join("wp-load.php").is_file() && docroot.join("wp/wp-load.php").is_file() {
        return docroot.join("wp");
    }
    docroot.to_path_buf()
}

/// [`core_root`] as a wp-cli flag, or `None` when the docroot IS the root.
pub fn core_path_arg(docroot: &Path) -> Option<String> {
    let root = core_root(docroot);
    (root != docroot).then(|| format!("--path={}", root.display()))
}

/// The argv prefix EVERY rexenv-run wp-cli command starts with.
///
/// One builder rather than one line repeated at each spawn, because the marker
/// literal below is what `every_wp_cli_argv_in_the_tree_is_pinned` scans for: a
/// second copy anywhere in `src/` or `examples/` is a wp-cli invocation the pin
/// (ledger #228) has not been shown to cover, and the build says so. The
/// deliberate exception is `core::terminal`'s `wp` wrapper — the user's own
/// command line, exempt by decision, and named in the guard.
///
/// The limit: WP-CLI (esp. core extraction) needs more than the default 128M.
///
/// `display_errors=stderr` is the OTHER half of #316, and it is about what PHP
/// itself prints. The CLI SAPI's default is to write diagnostics to STDOUT, so a
/// deprecation raised anywhere — including inside the phar, before wp-cli has
/// printed a byte — arrives glued to the front of the answer. Measured 14 Aug
/// 2026 on PHP 8.5.8 with the pinned 2.12.0 phar: `Deprecated: Case statements
/// followed by a semicolon (;) are deprecated … react/promise/src/functions.php
/// on line 369`, on EVERY command, so every read broke on a site that had
/// nothing wrong with it. Sent to stderr the diagnostic is not lost — it is
/// where a diagnostic belongs, and the streamed steps merge both streams into
/// one live log, so nothing disappears from an install either. This is also the
/// one flag that could not fix the tail half (Elementor's shutdown write is not
/// PHP's error display), which is why #316 needs both.
///
/// Note wp-cli sets `display_errors` to `stderr` itself once WordPress loads
/// (`Utils\wp_debug_mode`, verified in the pinned phar) — this covers the window
/// BEFORE that, which is exactly where the 8.5 deprecation lands.
pub fn wp_argv_prefix(wp_phar: &Path) -> Vec<String> {
    vec![
        "-d".into(),
        "memory_limit=512M".into(),
        "-d".into(),
        "display_errors=stderr".into(),
        wp_phar.display().to_string(),
    ]
}

/// The literal that separates wp-cli's own output from whatever a plugin writes
/// to stdout after the command finished (module header, #316). Written by
/// [`eoo_require_php`]'s shutdown function; consumed by [`split_at_eoo`].
pub const EOO_MARKER: &str = "<<<rexenv:end-of-output>>>";

/// The require file's name, VERSION-STAMPED: it is written once and never
/// rewritten, so a changed body needs a new name or the old file wins forever
/// (the same rule as `wp_packages`' materialised tree).
const EOO_REQUIRE_FILE: &str = ".rexenv-end-of-output-1.php";

/// The file's body. Built from [`EOO_MARKER`] rather than repeating it, so the
/// PHP and the Rust that cuts on it cannot drift apart.
fn eoo_require_php() -> String {
    format!(
        "<?php\n\
         // rexenv — see core/wordpress.rs. wp-cli loads a `--require` file before\n\
         // WordPress and before any plugin, so this is the FIRST registered shutdown\n\
         // function: everything a plugin writes to stdout after the command finished\n\
         // lands after this marker, whatever it writes with. Nothing else happens here.\n\
         register_shutdown_function( static function () {{\n\
         \techo '{EOO_MARKER}' . \"\\n\";\n\
         }} );\n"
    )
}

/// `--require=<file>` for the marker, or `None` when the file is not there.
///
/// `None` rather than the flag matters: wp-cli REFUSES to run when a required
/// file is missing (`Error: Required file '…' doesn't exist`, exit 1), so a
/// flag passed hopefully would turn a failed write into every wp command
/// failing. Absent file ⇒ no flag ⇒ the pre-#316 behaviour, which is noisy
/// rather than broken.
///
/// Beside the phar for [`wp_packages::neutral_packages_path`]'s reason — it is
/// the one path every spawn site already holds. Materialised through a temp
/// file and a rename because rexenv runs wp commands CONCURRENTLY (status polls
/// against site reads): a half-written PHP file is a parse error in every
/// command that reads it mid-write, and a rename is the only write that no
/// reader can observe partially.
fn eoo_require_arg(wp_phar: &Path) -> Option<String> {
    let path = wp_phar.with_file_name(EOO_REQUIRE_FILE);
    if !path.is_file() {
        let tmp = path.with_file_name(format!("{EOO_REQUIRE_FILE}.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, eoo_require_php()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
        let _ = std::fs::remove_file(&tmp);
    }
    path.is_file().then(|| format!("--require={}", path.display()))
}

/// Split captured stdout at the marker: `(what the command printed, what was
/// printed after it finished)`.
///
/// The LAST occurrence wins, not the first. The marker is printed once by us,
/// but a site's own data can contain any string — an option value, a post
/// body — and cutting at the first hit would silently truncate a legitimate
/// answer. Cutting at the last one can only ever discard more of the tail,
/// which is the half that is already not the command's.
///
/// No marker ⇒ everything is the command's output. That is the honest reading
/// on a machine where the require file could not be written, and it is also
/// what `core::terminal`'s deliberately-unpinned `wp` produces.
pub fn split_at_eoo(stdout: &[u8]) -> (&[u8], &[u8]) {
    let needle = EOO_MARKER.as_bytes();
    match rfind(stdout, needle) {
        Some(at) => (&stdout[..at], &stdout[at + needle.len()..]),
        None => (stdout, &[]),
    }
}

/// Last index of `needle` in `hay`.
fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).rev().find(|&i| &hay[i..i + needle.len()] == needle)
}

/// What the post-run tail is introduced by when it is moved to stderr. Says
/// where it came from, because on a failing command it lands in an error a user
/// reads and "Deprecated: …" with no attribution reads as rexenv's own.
const POST_RUN_NOTE: &str =
    "\nrexenv: written to stdout AFTER the command finished (a plugin's shutdown hook), \
     so it is not part of the command's output:\n";

/// Cut a captured `Output` at the marker, carrying the tail over to stderr.
///
/// EVERY captured spawn goes through this — asserted by
/// `every_captured_wp_cli_spawn_cuts_the_post_run_tail`, because "the parsed
/// paths were updated" is exactly the coverage-by-list this module already
/// learned not to trust (#228's row named four spawn sites when there were
/// seven).
fn cut_post_run_tail(mut out: Output) -> Output {
    let (clean, tail) = split_at_eoo(&out.stdout);
    let (clean, tail) = (clean.to_vec(), tail.to_vec());
    // Our own trailing newline is not a diagnostic — only report a tail that
    // has something in it.
    if tail.iter().any(|b| !b.is_ascii_whitespace()) {
        out.stderr.extend_from_slice(POST_RUN_NOTE.as_bytes());
        out.stderr.extend_from_slice(&tail);
    }
    out.stdout = clean;
    out
}

// ── The premium-update context (#370) ───────────────────────────────────────
//
// wp-admin lists updates for BetterDocs Pro, BetterLinks Pro, Elementor Pro.
// rexenv's checked pass listed NONE of them, and the reason is not that wp-cli
// reads a different transient — it is that the vendors' updaters never register
// in it. Measured on a real 47-plugin site (18 Aug 2026), the gate is a
// CAPABILITY check, not the `is_admin()` one it looks like from the outside:
//
//     public function plugin_updater(): void {
//         $doing_cron = defined( 'DOING_CRON' ) && DOING_CRON;
//         if ( ! current_user_can( 'manage_options' ) && ! $doing_cron ) { return; }
//         new Updater( … );   // ← the pre_set_site_transient_update_plugins filter
//     }
//
// A wp-cli run has NO user, so `current_user_can` is false for every capability
// and the filter is never added; WordPress then builds its update data with
// every premium plugin missing. Of ten premium plugins on that site, two
// reported an update before this file existed and five after — and the three
// still silent are ones wp-admin says nothing about either.
//
// Two grants, because two gates were measured. Defining `WP_ADMIN` alone moved
// NOTHING (that experiment ran first); the capabilities moved four of the five;
// `WP_ADMIN` on top of them moved the fifth. Both are process-scoped: no user
// is logged in, no session or cookie exists, and the grant dies with the child.
//
// **The update ACTION needs the same context, and that is not a nicety.**
// `wp plugin update betterdocs-pro` without it answers "No plugin updates
// available", because the package URL lives in the same filter's output — so a
// badge shown without this on the update path would be a button that cannot
// work. Both carry it, or neither should.
//
// It is scoped to those two paths on purpose. The terminal, the MCP raw runner,
// install/activate/deactivate/delete and the FAST list all run without it —
// asserted by `the_premium_update_context_rides_only_the_update_paths`, because
// "we only pass it where we meant to" is exactly the kind of claim this module
// has already watched drift (#228's row named four spawn sites out of seven).

/// Version-stamped like [`EOO_REQUIRE_FILE`] and for the same reason: the file
/// is written once and never rewritten, so a changed body needs a new name or
/// the old one wins forever.
const UPDATE_CONTEXT_FILE: &str = ".rexenv-update-context-1.php";

/// The file's body. Nothing is impersonated: capabilities are added to a
/// process that has no user, rather than a user being logged in.
fn update_context_php() -> &'static str {
    "<?php\n\
     // rexenv — see core/wordpress.rs. Loaded ONLY by the commands that need\n\
     // WordPress's update data to include PREMIUM plugins and themes: the update\n\
     // check, and the update itself.\n\
     //\n\
     // A premium updater registers its `pre_set_site_transient_update_plugins`\n\
     // filter behind `current_user_can( 'manage_options' )` (or is_admin()). A\n\
     // wp-cli run has no user and satisfies neither, so WordPress builds its\n\
     // update data with every premium plugin missing.\n\
     //\n\
     // Nobody is impersonated here: no user is logged in, no session is created,\n\
     // no cookie is set. These capabilities exist inside THIS process, for the\n\
     // length of this one command.\n\
     if ( ! defined( 'WP_ADMIN' ) ) {\n\
     \tdefine( 'WP_ADMIN', true );\n\
     }\n\
     if ( class_exists( 'WP_CLI' ) ) {\n\
     \t// Queued BEFORE WordPress loads. The gates above run on `init`, which has\n\
     \t// already fired by the time wp-cli's own `after_wp_load` hook gets a turn,\n\
     \t// and `add_filter` does not exist yet at `after_wp_config_load` — measured,\n\
     \t// both of them, before this line was written.\n\
     \tWP_CLI::add_wp_hook( 'user_has_cap', static function ( $caps ) {\n\
     \t\t// Named one by one. Handing back a blanket grant would also switch on\n\
     \t\t// every OTHER capability-gated code path a plugin runs at load.\n\
     \t\t$caps['manage_options'] = true;\n\
     \t\t$caps['update_plugins'] = true;\n\
     \t\t$caps['update_themes']  = true;\n\
     \t\treturn $caps;\n\
     \t}, 99, 1 );\n\
     }\n"
}

/// `--require=<file>` for the premium-update context, or `None` when the file
/// is not there.
///
/// `None` rather than the flag, for [`eoo_require_arg`]'s reason: wp-cli refuses
/// to run at all when a required file is missing, so a flag passed hopefully
/// would turn a failed write into a checked list that FAILS rather than one that
/// merely misses the premium rows.
fn update_context_arg(wp_phar: &Path) -> Option<String> {
    let path = wp_phar.with_file_name(UPDATE_CONTEXT_FILE);
    if !path.is_file() {
        let tmp = path.with_file_name(format!("{UPDATE_CONTEXT_FILE}.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, update_context_php()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
        let _ = std::fs::remove_file(&tmp);
    }
    path.is_file().then(|| format!("--require={}", path.display()))
}

/// A CHECKED list (plugins or themes) — run WITH the premium-update context,
/// and then, only if that failed for a reason other than the clock, again
/// without it.
///
/// The retry is the floor: the context runs vendor code that a plain list never
/// reached, on sites rexenv has never seen. If some plugin's licensing path dies
/// under it, the cost must be "no premium rows" — what the user had yesterday —
/// and never "no update badges at all". A CLOCK expiry is the one failure not
/// worth repeating: the second run would ask the same slow network the same
/// question, and buy a second full timeout of spinner with it.
fn checked_list<T: serde::de::DeserializeOwned>(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[&str],
) -> Result<T> {
    let Some(ctx) = update_context_arg(wp_phar) else {
        return wp_json_timed(php_bin, wp_phar, docroot, args, WP_LIST_TIMEOUT);
    };
    let mut with: Vec<&str> = args.to_vec();
    with.push(ctx.as_str());
    match wp_json_timed(php_bin, wp_phar, docroot, &with, WP_LIST_TIMEOUT) {
        Ok(list) => Ok(list),
        Err(e) if is_timeout_error(&e) => Err(e),
        Err(_) => wp_json_timed(php_bin, wp_phar, docroot, args, WP_LIST_TIMEOUT),
    }
}

/// Whether an error is [`run_with_timeout`]'s own clock expiry. Reads the
/// phrase both sides share ([`TIMEOUT_PHRASE`]) rather than a string spelled
/// twice — and `a_killed_child_reports_a_timeout_this_module_can_recognise`
/// runs the real kill path so the two cannot drift apart silently.
fn is_timeout_error(e: &Error) -> bool {
    e.to_string().contains(TIMEOUT_PHRASE)
}

/// The bundled PHP running the pinned phar with the command set pinned — the
/// ONE `Command::new(php_bin)` in the tree, so a captured spawn cannot be
/// assembled without the pin. Streamed spawns cannot use a `Command` (they go
/// through `ProcessSupervisor`) and pair [`wp_argv_prefix`] with
/// [`wp_packages::with_pinned_packages`] instead.
///
/// The end-of-output marker rides along here rather than in [`wp_argv_prefix`]
/// for the same reason: the prefix is shared with the STREAMED spawns, whose
/// output is a live log rather than something parsed, and a marker line in a
/// user's install log is a defect with no upside. `--require` is the first
/// wp-cli argument, so ours is the first required file — a later one cannot
/// register a shutdown function ahead of it.
fn wp_command(php_bin: &Path, wp_phar: &Path) -> Command {
    let mut cmd = Command::new(php_bin);
    cmd.args(wp_argv_prefix(wp_phar));
    if let Some(require) = eoo_require_arg(wp_phar) {
        cmd.arg(require);
    }
    let (key, value) = wp_packages::pin_packages_env(wp_phar);
    cmd.env(key, value);
    cmd
}

pub fn wp_cli(
    php_bin: &Path,
    wp_phar: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<Output> {
    let mut cmd = wp_command(php_bin, wp_phar);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
        // Added HERE, at the one place every wp-cli invocation is assembled, so
        // a composer-managed layout works for EVERY caller rather than for the
        // one that happened to be debugged. A caller that already said `--path`
        // keeps its own answer.
        if !args.iter().any(|a| a.starts_with("--path=")) {
            if let Some(path) = core_path_arg(dir) {
                cmd.arg(path);
            }
        }
    }
    Ok(cut_post_run_tail(cmd.output()?))
}

/// What [`run_with_timeout`]'s expiry error says, in the ONE place both the
/// formatter and [`is_timeout_error`] read it from — a retry that keys off a
/// phrase spelled twice starts retrying timeouts the day someone rewords one.
const TIMEOUT_PHRASE: &str = "timed out after";

/// Run a command with a hard wall-clock cap: poll `try_wait`, SIGKILL on
/// expiry. For wp-cli subcommands that download from the network — WP's
/// `download_url` waits up to **300s per attempt**, which offline reads as a
/// frozen spinner. Output is drained on reader THREADS while the child runs
/// (the `repo::run_captured_with_cap` lesson): a wait-then-read would deadlock
/// once a chatty child fills the ~64KB pipe buffer and read as a FAKE timeout —
/// a big `plugin list --format=json` must never trip the guard by being long.
pub(crate) fn run_with_timeout(mut cmd: Command, timeout: Duration, what: &str) -> Result<Output> {
    use std::io::Read;
    use std::process::Stdio;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stderr.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait(); // reap — no zombie
            // Deliberately no join here: the readers exit when the killed
            // child's pipes close; blocking on them could hang the caller if
            // anything else held a pipe end (the B7 lesson).
            return Err(Error::Other(format!(
                "{what} {TIMEOUT_PHRASE} {}s",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    Ok(Output {
        status,
        stdout: out_t.join().unwrap_or_default(),
        stderr: err_t.join().unwrap_or_default(),
    })
}

/// [`wp_cli`] with a wall-clock timeout (see [`run_with_timeout`]).
fn wp_cli_timed(
    php_bin: &Path,
    wp_phar: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Output> {
    let mut cmd = wp_command(php_bin, wp_phar);
    cmd.args(args);
    let what = format!("wp {}", args.first().copied().unwrap_or(""));
    Ok(cut_post_run_tail(run_with_timeout(cmd, timeout, &what)?))
}

/// Run WP-CLI and return stdout, erroring (with stderr) on a non-zero exit.
pub fn wp_cli_checked(
    php_bin: &Path,
    wp_phar: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String> {
    let out = wp_cli(php_bin, wp_phar, args, cwd)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        // The phar's own words first, ALWAYS — that is the string the user will
        // paste into a search box. The tell (#301) is appended to it, never in
        // place of it: a friendlier message that replaced this would cost them
        // the one line that finds an answer.
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let tell = wp_packages::explain_missing_command_here(&stderr)
            .map(|t| format!("\n\n{t}"))
            .unwrap_or_default();
        Err(Error::Other(format!(
            "wp {} failed (exit {:?}): {stderr}{tell}",
            args.first().copied().unwrap_or(""),
            out.status.code(),
        )))
    }
}

/// The ACTIVE theme's directory name (`wp option get stylesheet`) — the
/// unlink-only delete path refuses to remove the active theme's link.
pub fn active_stylesheet(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["option", "get", "stylesheet"])
}

/// Run a WP-CLI command scoped to a docroot (`--path=<docroot>` is appended for
/// the caller) and return trimmed stdout, erroring (with stderr) on non-zero exit.
pub fn wp_run(php_bin: &Path, wp_phar: &Path, docroot: &Path, args: &[&str]) -> Result<String> {
    let path = format!("--path={}", docroot.display());
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 1);
    full.extend_from_slice(args);
    full.push(&path);
    Ok(wp_cli_checked(php_bin, wp_phar, &full, None)?.trim().to_string())
}

/// Run an ARBITRARY WP-CLI argv against a docroot, capped by a wall clock, and
/// hand back the raw `Output` — exit code, stdout and stderr all intact.
///
/// For the MCP raw runner (`wp_run`, D1). Two deliberate differences from
/// [`wp_run`]:
///
/// - **A non-zero exit is not an error here.** `wp plugin is-active x` exits 1
///   to mean "no"; collapsing that into a failure would throw away the answer.
///   The caller reports the exit code and both streams.
/// - **A hard timeout, always.** A raw argv can reach an interactive or wedged
///   child (`wp shell`, a network fetch behind a black hole), and a tool call
///   that never returns is an agent that never comes back.
///
/// `--path` is appended by rexenv and is deliberately LAST — in wp-cli a later
/// `--path` wins. That ordering is a belt: the caller REFUSES a caller-supplied
/// one outright ([`crate::core::scratch::refuse_wp_target_override`]), because
/// winning a race is not the same as not having one. Command-line parameters
/// also beat any `wp-cli.yml` the child might otherwise pick up.
pub fn wp_run_raw(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<Output> {
    let path = format!("--path={}", docroot.display());
    let mut cmd = wp_command(php_bin, wp_phar);
    cmd.args(args).arg(&path);
    let what = format!("wp {}", args.first().map(String::as_str).unwrap_or(""));
    Ok(cut_post_run_tail(run_with_timeout(cmd, timeout, &what)?))
}

/// Typed JSON bridge: run a WP-CLI command scoped to a docroot with
/// `--format=json` and deserialize stdout into `T` (e.g. `Vec<PluginRow>`).
/// Non-zero exit / stderr surfaces as a clean `Error`, as does a parse failure.
pub fn wp_json<T: serde::de::DeserializeOwned>(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[&str],
) -> Result<T> {
    let path = format!("--path={}", docroot.display());
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 2);
    full.extend_from_slice(args);
    full.push(&path);
    full.push("--format=json");
    let out = wp_cli_checked(php_bin, wp_phar, &full, None)?;
    json_from_wp(&out, &format!("wp {}", args.first().copied().unwrap_or("")))
}

/// Deserialize the FIRST JSON value in some wp-cli stdout.
///
/// The difference from `serde_json::from_str` is what happens after that value:
/// trailing bytes are ignored instead of failing the read. That is the belt
/// behind the marker cut (#316) — the braces are `--require`ing the marker and
/// cutting there, and this is what still works when the require file could not
/// be written and a plugin's shutdown hook glued a deprecation notice onto the
/// end of a perfectly good array.
///
/// Deliberately one-sided: bytes BEFORE the value are still a parse error.
/// Skipping forward to the first `{`/`[` would mean guessing which brace starts
/// the answer, and a wrong guess returns a plausible object rather than an
/// error — the failure mode this project rates worse than a refusal.
fn json_from_wp<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> Result<T> {
    let trimmed = text.trim();
    let mut de = serde_json::Deserializer::from_str(trimmed);
    match T::deserialize(&mut de) {
        Ok(v) => Ok(v),
        Err(e) => Err(Error::Other(diagnose_bad_json::<T>(trimmed, what, &e))),
    }
}

/// How much of an unexpected prefix to quote back. Long enough to recognise the
/// plugin's own wording, short enough that the message stays a message.
const JSON_PREFIX_QUOTE: usize = 160;

/// Turn a parse failure into something the person reading it can act on.
///
/// **The third door into "every WordPress screen is dead on this site".** #316
/// closed the tail (a plugin writing from a shutdown hook, cut by rexenv's
/// marker) and #317 closed the head for DIAGNOSTICS (`-d display_errors=stderr`).
/// Neither touches a plugin that plainly `echo`es while the command runs:
/// `echo` is not PHP's error display, and the bytes land in FRONT of the answer
/// rather than after it, where the marker cut cannot reach. `docs/ARCHITECTURE.md`
/// §9 has named that gap since #317 shipped and nothing else did.
///
/// It is not closed here, and the reason is the one `json_from_wp` already
/// states: skipping to the first brace means guessing which one starts the
/// answer, and a wrong guess returns a plausible object instead of an error.
/// **So the guess is used for the MESSAGE and never for the data.** If a valid
/// value does parse from a later offset, that is proof the output had junk in
/// front of it, and the error says so, quotes it, and names the likely cause.
/// The read still fails.
///
/// Which is the whole gain: the failure was already total, and it was also
/// anonymous — `bad JSON: expected value at line 1 column 1` sends the reader at
/// rexenv, or at WordPress, or at their database. Every one of the three doors
/// arrived as the same useless report, and this is the one that can still only
/// be reported.
fn diagnose_bad_json<T: serde::de::DeserializeOwned>(
    trimmed: &str,
    what: &str,
    err: &serde_json::Error,
) -> String {
    let start = trimmed.find(['[', '{']).unwrap_or(0);
    if start > 0 {
        let tail = &trimmed[start..];
        let mut de = serde_json::Deserializer::from_str(tail);
        if T::deserialize(&mut de).is_ok() {
            return format!(
                "{what}: the site printed {start} bytes before its answer, so the reply \
                 could not be read. Something on this site writes to output while WP-CLI \
                 runs — usually a plugin or mu-plugin with a stray `echo` or `print`. \
                 rexenv will not skip past it: the bytes could be part of the answer, and \
                 guessing would show you plausible wrong data instead of this message. \
                 What was printed first: {}",
                quoted_prefix(&trimmed[..start])
            );
        }
    }
    format!("{what}: bad JSON: {err}")
}

/// One readable line of an unexpected prefix: bounded, control characters
/// flattened so a plugin cannot smuggle newlines or escapes into a toast.
fn quoted_prefix(prefix: &str) -> String {
    let flat: String = prefix
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut out: String = flat.chars().take(JSON_PREFIX_QUOTE).collect();
    if flat.chars().count() > JSON_PREFIX_QUOTE {
        out.push('…');
    }
    format!("\"{out}\"")
}

/// Fixed allowance for a download-capable command's non-download work (api
/// lookups, unpack, install, DB writes).
const WP_DOWNLOAD_TIMEOUT_BASE: Duration = Duration::from_secs(120);
/// Per-download allowance. A GENEROUS WALL-CLOCK, deliberately not a stall
/// guard: wp-cli is opaque mid-download under pipe capture (plugin/theme
/// install print nothing between "Downloading…" and "Unpacking…"; core's
/// progress bar is TTY-only), so watching output would read every legit
/// download as a stall. The cap is safe because wp-cli is the TIGHTER bound in
/// the stack: WP's `download_url` caps each plugin/theme download at 300s
/// (verified: wp-admin/includes/file.php) and core's own request at ~600s — a
/// too-slow link fails INSIDE wp-cli with its own error long before 900s, so
/// this only ever catches a wedge outside wp-cli's bounds (hung DNS, a wedged
/// PHP, stuck disk I/O). NOT the B34 mistake: B34's cap was tighter than legit
/// transfers; this one is provably looser than anything wp-cli lets succeed.
const WP_DOWNLOAD_TIMEOUT_PER_ITEM: Duration = Duration::from_secs(900);
/// Bound for the update-checking list calls. WP's update-check request is
/// internally capped at 3s interactive / 30s cron (verified:
/// wp-includes/update.php `'timeout' => $doing_cron ? 30 : 3`), so 300s is
/// ~10× looser than the worst legit case — it only catches a wedged child.
const WP_LIST_TIMEOUT: Duration = Duration::from_secs(300);

/// Wall-clock cap for a command that downloads `items` archives sequentially:
/// scaled, because a multi-slug install legitimately runs N internally-capped
/// downloads back to back — a fixed cap would false-trip exactly the slow-link
/// user the bound must never hurt.
pub(crate) fn download_timeout(items: usize) -> Duration {
    WP_DOWNLOAD_TIMEOUT_BASE + WP_DOWNLOAD_TIMEOUT_PER_ITEM * items as u32
}

// ---------------------------------------------------------------------------
// Streamed-install line semantics (the live-progress card). PURE — the only
// parsing is of wp-cli FRAMEWORK literals from the pinned phar; the WP-core-
// owned phase wording ("Downloading installation package…", varies by WP
// version) is never parsed — it's displayed verbatim instead.
// ---------------------------------------------------------------------------

/// wp-cli's per-item install header ("Installing bbPress (2.5.9)") — a phar
/// literal. The " (" requirement excludes WP-core's "Installing the
/// plugin..."/"Installing the theme..." phase lines. Drives the honest
/// ATTEMPT cursor ("installing item k of N" — never "k done": an
/// already-installed slug prints NO header yet counts as a summary success).
pub fn is_install_item_header(line: &str) -> bool {
    line.starts_with("Installing ") && line.contains(" (")
}

/// The directory wp-cli refused to unpack over, read from its own line.
///
/// Measured verbatim (wp-cli 2.12.0, 19 Aug 2026, re-uploading the zip of an
/// installed plugin):
///
/// ```text
/// Warning: Destination folder already exists. "/…/wp-content/plugins/betterlinks-pro/"
/// Plugin installation failed.
/// Warning: The '/…/betterlinks-pro.2.1.1.zip' plugin could not be found.
/// Error: No plugins installed.
/// ```
///
/// The summary line — the one the UI would otherwise report — says only what did
/// NOT happen. This is the reason, and it is the ONE place it is parsed: the
/// job's state carries the answer so the card and the toast read a FACT rather
/// than each re-reading the log and drifting apart.
///
/// The folder comes from the quoted path, never from the archive's name: a zip
/// is routinely `plugin.1.2.3.zip` for a folder called `plugin`.
pub fn install_blocked_dir(line: &str) -> Option<String> {
    if !line.contains("Destination folder already exists.") {
        return None;
    }
    let quoted = line.split('"').nth(1)?;
    let dir = quoted.trim_end_matches('/').rsplit('/').next()?;
    (!dir.is_empty()).then(|| dir.to_string())
}

/// The batch's terminal truth: the last verbatim `Success:`/`Error:` line
/// ("Success: Installed 2 of 2 plugins." / "Error: Only installed 1 of 2
/// plugins."). Shown as-is — never paraphrased.
pub fn install_summary_line(tail: &[String]) -> Option<String> {
    tail.iter().rev().find(|l| l.starts_with("Success:") || l.starts_with("Error:")).cloned()
}

/// Map a FINISHED (non-cancelled) install run to an honest status. A partial
/// batch exits 1 with some slugs genuinely installed — wp-cli's
/// framework-literal "Only installed X of N" (pinned phar) is the only
/// distinguishing signal, and "partial" must never be flattened to "failed".
/// NOTE exit 0 can still mean installed-but-NOT-activated (chained
/// `--activate` failures don't touch the exit code) — callers must not claim
/// activation from "ok"; the plugin/theme list refresh is that truth.
pub fn classify_install_exit(exit_ok: bool, tail: &[String]) -> &'static str {
    if exit_ok {
        "ok"
    } else if tail.iter().any(|l| l.contains("Only installed")) {
        "partial"
    } else {
        "failed"
    }
}

/// The batch-TERMINAL success literals (pinned-phar framework strings,
/// utils.php `report_batch_operation_results`): "Success: Installed N of M
/// plugins/themes[ (k skipped)]." and the single-item edge "Success: Plugin/
/// Theme already installed.". Bare `Success:` is NOT the test — chained theme
/// activation prints "Success: Switched to '…' theme." MID-batch (verified
/// unguarded by `chained_command` in the 2.12.0 phar), and matching it would
/// read "done" while items remain.
fn is_install_terminal_success(line: &str) -> bool {
    line.starts_with("Success: Installed ")
        || (line.starts_with("Success:") && line.contains("already installed"))
}

/// Highest per-item milestone a line implies (1-based; None = not a phase
/// line). Matchers are LOOSE on purpose: the wording of phases 1–4 is
/// WP-core-owned and varies by WP version, and [`InstallProgress`]'s forward
/// implication means a missed match costs one hop of granularity, never a
/// stall.
fn install_milestone(line: &str) -> Option<u8> {
    if line.starts_with("Activating ")
        || line.starts_with("Network-activating ")
        || line.starts_with("Success: Switched to ")
        || line.contains("' activated")
    {
        Some(5)
    } else if line.contains("installed successfully") {
        Some(4)
    } else if line.starts_with("Installing the ") {
        Some(3)
    } else if line.starts_with("Unpacking") {
        Some(2)
    } else if line.contains("Downloading") || line.starts_with("Using cached file") {
        Some(1)
    } else {
        None
    }
}

/// PHASE-based determinate progress for the streamed install card.
///
/// NOT a violation of the B25 "no invented percentage" rule — read this
/// before "fixing" it back to an indeterminate bar. That rule bars BYTE-level
/// download estimates, where no signal exists (wp-cli is silent mid-transfer
/// and WP-core's byte progress bar is TTY-only). This is OBSERVED DISCRETE
/// progress: every tick corresponds to a line wp-cli actually printed —
/// nothing is estimated, interpolated, or timed.
///
/// Rules (each load-bearing):
/// - MONOTONIC — the percentage never decreases, ever.
/// - FORWARD IMPLICATION — a later milestone implies all earlier ones, and a
///   new item header implies every previous item is complete. So a line that
///   never appears (cached file replaces "Downloading…"; no `--activate`
///   means no "Activating…"; an already-installed slug prints NO header at
///   all) can never stall the bar — the next observed line jumps it forward.
///   Corollary: the bar may run BEHIND reality (headerless items are
///   invisible until the summary) but never ahead of it.
/// - NEVER 100 EARLY — capped at 99 until the batch-terminal summary literal
///   ([`is_install_terminal_success`]). Failure summaries ("Error: Only
///   installed …") advance nothing: on failure/cancel/timeout the bar stops
///   exactly where it is.
pub struct InstallProgress {
    items_total: usize,
    /// 5 with `--activate` (activation gets a slice), else 4 — a slice that
    /// can never fill is never reserved.
    phases_per_item: u8,
    headers_seen: usize,
    item_phase: u8,
    pct: u8,
}

impl InstallProgress {
    pub fn new(items_total: usize, activate: bool) -> Self {
        Self {
            items_total: items_total.max(1),
            phases_per_item: if activate { 5 } else { 4 },
            headers_seen: 0,
            item_phase: 0,
            pct: 0,
        }
    }

    pub fn pct(&self) -> u8 {
        self.pct
    }

    /// Feed one output line; returns the overall percentage (0–100).
    pub fn observe(&mut self, line: &str) -> u8 {
        if is_install_terminal_success(line) {
            self.pct = 100;
            return self.pct;
        }
        if is_install_item_header(line) {
            // Header for item k ⇒ items 1..k-1 are done, item k starts.
            self.headers_seen = (self.headers_seen + 1).min(self.items_total);
            self.item_phase = 0;
        } else if let Some(m) = install_milestone(line) {
            let m = m.min(self.phases_per_item);
            if m <= self.item_phase {
                return self.pct;
            }
            self.item_phase = m;
        } else {
            return self.pct;
        }
        let done = self.headers_seen.saturating_sub(1) as f64
            + f64::from(self.item_phase) / f64::from(self.phases_per_item);
        let raw = (done / self.items_total as f64 * 100.0) as u8;
        self.pct = self.pct.max(raw.min(99));
        self.pct
    }
}

/// [`wp_run`] with a hard wall-clock cap (see [`run_with_timeout`]): same
/// `--path` scoping and non-zero-exit mapping, for the network-capable
/// commands a wedged child would otherwise hang forever (B25).
fn wp_run_timed(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String> {
    let path = format!("--path={}", docroot.display());
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 1);
    full.extend_from_slice(args);
    full.push(&path);
    let out = wp_cli_timed(php_bin, wp_phar, &full, timeout)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(Error::Other(format!(
            "wp {} failed (exit {:?}): {}",
            args.first().copied().unwrap_or(""),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// [`wp_json`] with a hard wall-clock cap — for the update-checking list calls
/// (`plugin list` / `theme list`), whose api.wordpress.org refresh hangs the
/// whole listing when the child wedges (B25).
fn wp_json_timed<T: serde::de::DeserializeOwned>(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<T> {
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 1);
    full.extend_from_slice(args);
    full.push("--format=json");
    let out = wp_run_timed(php_bin, wp_phar, docroot, &full, timeout)?;
    json_from_wp(&out, &format!("wp {}", args.first().copied().unwrap_or("")))
}

/// What `wp_info` reports about a docroot (mirrors the frontend `WpInfo`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpInfo {
    /// A present, installed WordPress lives at the docroot.
    pub is_wordpress: bool,
    /// Core version (`wp core version`) when WordPress; else `None`.
    pub version: Option<String>,
    /// Whether the install is a multisite network.
    pub multisite: bool,
}

/// Detect WordPress at a docroot via `core is-installed` (presence), `core version`,
/// and the `MULTISITE` constant. A non-WordPress docroot (e.g. a Blank-PHP site)
/// reports `is_wordpress: false` rather than erroring.
pub fn wp_info(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<WpInfo> {
    let path = format!("--path={}", docroot.display());

    // `core is-installed` exits 0 only for a present, installed WordPress.
    let is_wordpress = wp_cli(php_bin, wp_phar, &["core", "is-installed", &path], None)
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !is_wordpress {
        return Ok(WpInfo { is_wordpress: false, version: None, multisite: false });
    }

    let version = wp_run(php_bin, wp_phar, docroot, &["core", "version"]).ok();

    // `config get MULTISITE` prints the constant ("1") for a network; it errors
    // when the constant is unset — treat that as not-multisite.
    let multisite = wp_cli(php_bin, wp_phar, &["config", "get", "MULTISITE", &path], None)
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().eq_ignore_ascii_case("1"))
        .unwrap_or(false);

    Ok(WpInfo { is_wordpress, version, multisite })
}

/// WP-CLI's `update` field is a string ("none" | "available" | "version higher
/// than expected") for regular plugins/themes but a BOOLEAN for must-use +
/// drop-in rows (e.g. rexenv's own `rexenv-login` mu-plugin — present on every
/// rexenv site, so a strict `String` made the whole list fail to parse).
fn de_update<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Update {
        S(String),
        B(bool),
        Null,
    }
    Ok(match Update::deserialize(d)? {
        Update::S(s) => s,
        Update::B(true) => "available".into(),
        Update::B(false) => "none".into(),
        Update::Null => "none".into(),
    })
}

/// One plugin row from `wp plugin list --format=json` (§6.1). Field names match
/// WP-CLI's JSON (all lowercase) and the frontend DTO. `update == "available"`
/// drives the update-available badge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpPlugin {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, deserialize_with = "de_update")]
    pub update: String,
    /// Human title from the plugin header ("Title:" / "Plugin Name:"); may be
    /// empty for drop-ins whose file has no header — the UI falls back to the
    /// slug then.
    #[serde(default)]
    pub title: String,
    /// The version the update would install. WP-CLI fills this ONLY on a real
    /// update check — with `--skip-update-check` it comes back empty, so the
    /// fast first list has no arrow and the background pass supplies it.
    ///
    /// The only two-word field here, so it's also the only one where WP-CLI's
    /// name and the frontend's differ: `rename` gives the UI `updateVersion`,
    /// the `alias` keeps reading WP-CLI's `update_version`.
    #[serde(default, rename = "updateVersion", alias = "update_version")]
    pub update_version: String,
}

/// `wp plugin list` (name, status, version, update, update_version, title). `check_updates:
/// false` passes `--skip-update-check` — the default check hits
/// api.wordpress.org on EVERY list (seconds when slow, a hang when offline),
/// so the UI lists fast without it and refreshes update badges in a
/// background pass.
pub fn plugin_list(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    check_updates: bool,
) -> Result<Vec<WpPlugin>> {
    let mut args = vec![
        "plugin",
        "list",
        "--fields=name,status,update,update_version,version,title",
    ];
    if !check_updates {
        args.push("--skip-update-check");
        return wp_json_timed(php_bin, wp_phar, docroot, &args, WP_LIST_TIMEOUT);
    }
    // The checked pass is the one that answers "is there an update", so it is
    // the one that carries the premium-update context (#370).
    checked_list(php_bin, wp_phar, docroot, &args)
}

/// Run `wp <noun> <verb> <names…>` (bulk-capable: one call for many items).
/// A no-op (empty names) returns Ok without invoking WP-CLI.
fn item_verb(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    noun: &str,
    verb: &str,
    names: &[String],
) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args = item_verb_argv(noun, verb, names);
    // The same context the CHECK runs under: the premium package URL comes out
    // of the same filter, so without it `wp plugin update <paid-slug>` answers
    // "No plugin updates available" and the badge is a button that cannot work.
    let ctx = (verb == "update").then(|| update_context_arg(wp_phar)).flatten();
    if let Some(flag) = &ctx {
        args.push(flag.as_str());
    }
    match item_verb_timeout(verb, names.len()) {
        // "update" downloads one archive per item from wp.org — the same wedge
        // class as install, same scaled cap (B25). The other verbs
        // (activate/deactivate/delete) are local ops, left untimed.
        Some(t) => wp_run_timed(php_bin, wp_phar, docroot, &args, t),
        None => wp_run(php_bin, wp_phar, docroot, &args),
    }
}

/// The wall-clock cap for an item verb: `update` gets the scaled DOWNLOAD
/// bound (it fetches an archive per item, exactly like install — NOT the flat
/// list bound, which is for metadata-only calls); everything else is a local
/// operation and stays unbounded.
fn item_verb_timeout(verb: &str, items: usize) -> Option<Duration> {
    (verb == "update").then(|| download_timeout(items))
}

/// Argv for a plugin/theme verb over one-or-more items: `[noun, verb, ...names]`.
/// NO `--` separator: WP-CLI does NOT honor the getopt end-of-flags convention —
/// a bare `--` is passed through as a LITERAL positional (a phantom slug), which
/// broke `plugin activate` ("The '--' plugin could not be found"). The names here
/// are real installed slugs from the app's own listings; the arbitrary-source
/// vector is closed separately by `ensure_slugs`/`valid_slug` on INSTALL.
fn item_verb_argv<'a>(noun: &'a str, verb: &'a str, names: &'a [String]) -> Vec<&'a str> {
    let mut args: Vec<&str> = vec![noun, verb];
    args.extend(names.iter().map(String::as_str));
    args
}

fn plugin_verb(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    verb: &str,
    names: &[String],
) -> Result<String> {
    item_verb(php_bin, wp_phar, docroot, "plugin", verb, names)
}

/// Activate one or more plugins (`wp plugin activate …`).
pub fn plugin_activate(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    plugin_verb(php_bin, wp_phar, docroot, "activate", names)
}
/// Deactivate one or more plugins (`wp plugin deactivate …`).
pub fn plugin_deactivate(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    plugin_verb(php_bin, wp_phar, docroot, "deactivate", names)
}
/// Update one or more plugins (`wp plugin update …`).
pub fn plugin_update(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    plugin_verb(php_bin, wp_phar, docroot, "update", names)
}

/// Silence bound for a STREAMED update. Deliberately larger than
/// [`crate::core::repo::STEP_IDLE_LIMIT`] (300s) and than WP's own 300s
/// `download_url` per-attempt cap: a big archive (WooCommerce, Elementor)
/// prints nothing while it downloads, so a 300s bound would race WP's own
/// give-up and kill the child a moment before it could report the real
/// network error. 420s lets WP's message be the one the user reads.
pub const UPDATE_IDLE_LIMIT: Duration = Duration::from_secs(420);

/// `wp <plugin|theme> update …` / `wp core update` with WP-CLI's own progress
/// pumped to `on_line` as it arrives, so the UI can show which item is at
/// which phase instead of a silent spinner. Blocking — call under
/// `spawn_blocking`.
///
/// The captured [`plugin_update`] above stays the path for callers with no
/// sink (it is also the one with the hard total cap); this one is bounded by
/// SILENCE, which is the honest bound for a download whose duration nobody
/// can predict.
pub fn update_streamed(
    supervisor: &dyn crate::platform::traits::ProcessSupervisor,
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    kind: UpdateKind,
    names: &[String],
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    // Core has no item list; plugins/themes with an empty one have no work.
    if names.is_empty() && kind != UpdateKind::Core {
        return Ok(());
    }
    let mut args: Vec<String> = vec![
        "-d".into(),
        "memory_limit=512M".into(),
        wp_phar.display().to_string(),
        kind.noun().into(),
        "update".into(),
    ];
    args.extend(names.iter().cloned());
    // The premium-update context (#370) — the streamed path is the one the UI's
    // Update button runs, so it needs what the captured verb needs.
    if let Some(flag) = update_context_arg(wp_phar) {
        args.push(flag);
    }
    // `--no-color`: the phase text is parsed and shown to a user, not a TTY.
    args.push("--no-color".into());
    args.push(format!("--path={}", docroot.display()));
    let cancel = crate::core::repo::CancelToken::new();
    let res = crate::core::repo::run_step_streamed(
        supervisor,
        php_bin,
        &args,
        docroot,
        &[],
        &cancel,
        on_line,
        Some(UPDATE_IDLE_LIMIT),
    )?;
    if res.ok {
        return Ok(());
    }
    Err(Error::Other(format!(
        "wp {} update failed (exit {:?}): {}",
        kind.noun(),
        res.exit,
        res.tail.join(" / ")
    )))
}

/// What a streamed update is updating. The step sequence is shared (it is
/// WP's one `WP_Upgrader`), but the SETTLED line differs per noun and core
/// adds two of its own — so the noun is a parameter, not three parsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateKind {
    Plugin,
    Theme,
    Core,
}

impl UpdateKind {
    /// The wp-cli noun (`wp <noun> update`).
    pub fn noun(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Theme => "theme",
            Self::Core => "core",
        }
    }

    /// The wp.org download-URL segment a slug can be read out of. Core's
    /// archive is `/release/wordpress-6.8.2-no-content.zip` — a version, not a
    /// slug — so core reads none.
    fn url_segment(self) -> Option<&'static str> {
        match self {
            Self::Plugin => Some("/plugin/"),
            Self::Theme => Some("/theme/"),
            Self::Core => None,
        }
    }

    /// Lines that mean ONE item finished, with whether that item failed.
    /// Every phrase is WP's/WP-CLI's own (`class-plugin-upgrader.php`,
    /// `class-theme-upgrader.php`, wp-cli's `core update`).
    fn settled(self) -> &'static [(&'static str, bool)] {
        match self {
            Self::Plugin => {
                &[("Plugin updated successfully", false), ("Plugin update failed", true)]
            }
            Self::Theme => {
                &[("Theme updated successfully", false), ("Theme update failed", true)]
            }
            // wp-cli's own success line; `Error:` is how it ends a failed run.
            Self::Core => &[
                ("Success: WordPress updated successfully", false),
                ("Success: WordPress is up to date", false),
                ("Success: WordPress is at the latest version", false),
                ("Error:", true),
            ],
        }
    }

    /// Steps only this noun prints, tried BEFORE the shared table.
    fn extra_steps(self) -> &'static [(&'static str, &'static str, f32)] {
        match self {
            // wp-cli announces the target version before handing off to the
            // upgrader, and does its own file cleanup after it.
            Self::Core => &[
                ("Updating to version", "Preparing", 0.05),
                ("Starting update", "Preparing", 0.05),
                ("Downloading WordPress", "Downloading", 0.15),
                ("Cleaning up files", "Cleaning up", 0.9),
                ("No files found that need cleaning up", "Cleaning up", 0.9),
                ("File removed:", "Cleaning up", 0.9),
            ],
            _ => &[],
        }
    }
}

/// One step of WP-CLI's per-item update sequence. The order is WP's own
/// (`WP_Upgrader`'s string table), so `weight` is a real STEP POSITION — not a
/// guess at elapsed time and never a byte count, which WP-CLI does not report.
///
/// The translation entries matter more than they look: WP updates language
/// packs INSIDE a plugin/theme update ("Some of your translations need
/// updating…"), so `Translation updated successfully.` is NOT an item
/// finishing. Treating it as one over-counted the bar — it is a STEP here, and
/// deliberately never a settle.
const UPDATE_STEPS: &[(&str, &str, f32)] = &[
    ("Enabling Maintenance mode", "Preparing", 0.05),
    ("Downloading update from", "Downloading", 0.15),
    ("Using cached file", "Downloading (cached)", 0.35),
    ("Unpacking the update", "Unpacking", 0.55),
    ("Installing the latest version", "Installing", 0.75),
    ("Removing the old version", "Cleaning up", 0.9),
    ("Some of your translations need updating", "Updating translations", 0.92),
    ("Translation updated successfully", "Updating translations", 0.93),
    ("Translation update failed", "Updating translations", 0.93),
    ("Disabling Maintenance mode", "Finishing", 0.95),
];

/// A live view of a streamed update, fed one WP-CLI line at a time.
///
/// Progress is counted in ITEMS FINISHED (`done`/`total`) plus the current
/// item's step position — both facts WP-CLI actually announces. Nothing here
/// invents a percentage from a clock.
#[derive(Debug, Clone)]
pub struct UpdateTracker {
    kind: UpdateKind,
    names: Vec<String>,
    done: usize,
    current: String,
    phase: String,
    step: f32,
}

/// The snapshot an [`UpdateTracker`] hands to the UI after each line.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    pub total: usize,
    pub done: usize,
    /// Slug currently being updated ("" before the first item announces).
    pub current: String,
    /// Human phase — WP-CLI's own step, not a paraphrase of a timer.
    pub phase: String,
    /// 0..1 over the whole run: finished items + the current item's step.
    pub fraction: f32,
    /// The raw WP-CLI line this snapshot came from (log / failure text).
    pub line: String,
}

impl UpdateTracker {
    /// `names` is the argv list for plugins/themes. Core takes an empty list:
    /// it is one item, named for the screen ("WordPress"), which is also why
    /// its bar is a single item's step positions.
    pub fn new(kind: UpdateKind, names: &[String]) -> Self {
        let names = if kind == UpdateKind::Core && names.is_empty() {
            vec!["WordPress".to_string()]
        } else {
            names.to_vec()
        };
        Self {
            kind,
            current: names.first().cloned().unwrap_or_default(),
            names,
            done: 0,
            phase: "Starting".into(),
            step: 0.0,
        }
    }

    /// Feed one WP-CLI line. Returns `true` when the snapshot moved — an
    /// unrecognised line (a warning, a table row) is logged, not rendered as
    /// a phase, so the bar never advances on noise.
    pub fn feed(&mut self, line: &str) -> bool {
        let l = line.trim();
        if let Some(slug) = downloaded_slug(self.kind, l) {
            self.current = slug;
        }
        for (needle, failed) in self.kind.settled() {
            if l.starts_with(needle) {
                // An item settled: bank it and point at the next name we were
                // given (WP-CLI processes them in argv order).
                self.done = (self.done + 1).min(self.names.len());
                self.step = 0.0;
                self.phase = if *failed { "Failed".into() } else { "Updated".into() };
                if let Some(next) = self.names.get(self.done) {
                    self.current = next.clone();
                }
                return true;
            }
        }
        for (needle, label, weight) in self.kind.extra_steps().iter().chain(UPDATE_STEPS) {
            if l.starts_with(needle) {
                self.phase = (*label).into();
                self.step = *weight;
                return true;
            }
        }
        false
    }

    pub fn snapshot(&self, line: &str) -> UpdateSnapshot {
        let total = self.names.len().max(1);
        let fraction = ((self.done as f32 + self.step) / total as f32).clamp(0.0, 1.0);
        UpdateSnapshot {
            total: self.names.len(),
            done: self.done,
            current: self.current.clone(),
            phase: self.phase.clone(),
            fraction,
            line: line.to_string(),
        }
    }
}

/// The slug in a wp.org download line
/// (`Downloading update from https://downloads.wordpress.org/plugin/query-monitor.3.19.0.zip...`).
/// Premium/self-hosted plugins download from arbitrary URLs — those return
/// `None` and the tracker keeps the argv-order name rather than showing a
/// filename that is not a slug. Core has no slug in its URL at all.
fn downloaded_slug(kind: UpdateKind, line: &str) -> Option<String> {
    let segment = kind.url_segment()?;
    let rest = line.strip_prefix("Downloading update from ")?;
    let file = rest.rsplit_once(segment).map(|(_, f)| f)?;
    let slug = file.split('.').next()?;
    (!slug.is_empty() && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .then(|| slug.to_string())
}

/// Delete one or more plugins (`wp plugin delete …`).
pub fn plugin_delete(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    plugin_verb(php_bin, wp_phar, docroot, "delete", names)
}

/// A wp.org plugin/theme slug: `^[a-z0-9][a-z0-9-]*$`. Mirrors [`valid_locale`] /
/// `parse_wp_version` — refuses argv smuggling (a leading `-` can't become a
/// wp-cli flag) AND install-from-source tricks (a slug is NOT a URL / path / zip:
/// no `:`, `/`, `.`, `_`). Real wp.org slugs always match.
fn valid_slug(slug: &str) -> bool {
    let mut bytes = slug.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_lowercase() || b.is_ascii_digit())
        && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Refuse anything that isn't a bare wp.org slug BEFORE it reaches `wp … install`
/// — where a URL/path/zip would install arbitrary code and a leading `-` a flag.
/// The wp.org source stayed EXACTLY this strict when the zip source landed: a
/// local archive goes through [`ensure_zip_paths`], a separate gate on a
/// separate argument, never through a loosened slug.
pub(crate) fn ensure_slugs(kind: &str, slugs: &[String]) -> Result<()> {
    for slug in slugs {
        if !valid_slug(slug) {
            return Err(Error::Other(format!(
                "invalid {kind} slug \"{slug}\": expected a wp.org slug (lowercase letters, \
                 digits, hyphens) — to install a .zip, use the Upload zip source."
            )));
        }
    }
    Ok(())
}

/// Gate for the "Upload zip" source: a LOCAL archive the user picked in the
/// native file dialog, the only non-wp.org install source. Deliberately its
/// own gate rather than a hole in [`valid_slug`] — the wp.org path still
/// refuses every URL, path and zip.
///
/// Each check is load-bearing, and the ones that look cosmetic are not:
/// - **absolute** — wp-cli runs with `--path=<docroot>`, and a relative
///   argument resolves against the CWD, so "the file the user pointed at" and
///   "the file WordPress unpacks" could be two different files. It also makes
///   argv smuggling impossible without a separate check: a path starting with
///   `/` can never be read as a flag.
/// - **`.zip` extension** (ASCII-case-insensitive) — wp-cli treats an argument
///   as a local archive ONLY when `pathinfo(…, EXTENSION) === 'zip'`;
///   anything else silently falls through to a wp.org SLUG lookup. Without
///   this check, picking `theme.tar.gz` would not fail as "not a zip", it
///   would fail as "plugin not found in the directory" — the wrong story.
/// - **an existing regular file** — same fall-through: a directory or a
///   deleted path becomes a wp.org lookup for a filename-shaped slug.
pub(crate) fn ensure_zip_paths(kind: &str, paths: &[String]) -> Result<()> {
    for p in paths {
        let path = Path::new(p);
        if !path.is_absolute() {
            return Err(Error::Other(format!(
                "invalid {kind} archive \"{p}\": expected an absolute path to a .zip file."
            )));
        }
        let is_zip = path
            .extension()
            .is_some_and(|e| e.as_encoded_bytes().eq_ignore_ascii_case(b"zip"));
        if !is_zip {
            return Err(Error::Other(format!(
                "\"{p}\" is not a .zip file — WordPress installs a {kind} from a zip archive."
            )));
        }
        if !path.is_file() {
            return Err(Error::Other(format!("no such file: \"{p}\".")));
        }
    }
    Ok(())
}

// NOTE: the captured `plugin_install`/`theme_install` fns are RETIRED — every
// wp.org install now runs streamed: manual installs through
// `commands::wp_install`, blueprint items through `blueprints::apply_wordpress`
// (both `run_step_streamed` + CancelToken). One execution path.

/// One theme row from `wp theme list --format=json` (§6.2). `status == "active"`
/// marks the live theme. `screenshot` is filled AFTER parsing (it's not a WP-CLI
/// field): the theme's `screenshot.*` file as a `data:` URL, `None` when absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpTheme {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, deserialize_with = "de_update")]
    pub update: String,
    /// The version the update would install — same rule as [`WpPlugin`]: only
    /// a real update check fills it, so the fast list shows no arrow.
    #[serde(default, rename = "updateVersion", alias = "update_version")]
    pub update_version: String,
    /// The theme header's `Theme Name:` — "Twenty Twenty-Five" for the
    /// stylesheet `twentytwentyfive`. Same rule as [`WpPlugin::title`]: it is
    /// what the card is LABELLED with, so the slug stays beside it rather than
    /// being replaced (the slug is the folder name, and what `theme activate`
    /// takes). May be empty for a theme whose header has none — the UI falls
    /// back to the slug then.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub screenshot: Option<String>,
}

/// A theme's `screenshot.*` preview as a `data:` URL. WP-CLI's `name` field is
/// the stylesheet slug — the theme's directory under `<content_rel>/themes/`
/// (the RECORDED content dir, v24 — a hardcoded wp-content read nothing on
/// Bedrock and every screenshot silently vanished).
/// Missing file (screenshots are optional) ⇒ `None`, never an error.
fn theme_screenshot(docroot: &Path, content_rel: &str, slug: &str) -> Option<String> {
    use base64::Engine;
    let dir = docroot.join(content_rel).join("themes").join(slug);
    for (ext, mime) in [
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("webp", "image/webp"),
        ("gif", "image/gif"),
    ] {
        if let Ok(bytes) = std::fs::read(dir.join(format!("screenshot.{ext}"))) {
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            return Some(format!("data:{mime};base64,{b64}"));
        }
    }
    None
}

/// `wp theme list` (name, status, version, update, update_version) + each theme's screenshot as
/// a `data:` URL. `check_updates: false` passes `--skip-update-check` (same
/// api.wordpress.org round-trip as [`plugin_list`]).
pub fn theme_list(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    content_rel: &str,
    check_updates: bool,
) -> Result<Vec<WpTheme>> {
    // The field list is EXPLICIT because `update_version` is not in wp-cli's
    // default set for themes — without it the arrow would silently never show.
    let mut args =
        vec!["theme", "list", "--fields=name,status,update,update_version,version,title"];
    if !check_updates {
        args.push("--skip-update-check");
    }
    // A premium THEME's updater is gated exactly like a premium plugin's, so
    // the checked pass takes the same context (#370).
    let mut themes: Vec<WpTheme> = if check_updates {
        checked_list(php_bin, wp_phar, docroot, &args)?
    } else {
        wp_json_timed(php_bin, wp_phar, docroot, &args, WP_LIST_TIMEOUT)?
    };
    for t in &mut themes {
        t.screenshot = theme_screenshot(docroot, content_rel, &t.name);
    }
    Ok(themes)
}

/// Activate a theme (`wp theme activate <name>`) — only one can be live.
pub fn theme_activate(php_bin: &Path, wp_phar: &Path, docroot: &Path, name: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["theme", "activate", name])
}
/// Update one or more themes (`wp theme update …`).
pub fn theme_update(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    item_verb(php_bin, wp_phar, docroot, "theme", "update", names)
}
/// Delete one or more themes (`wp theme delete …`) — the active theme can't be deleted.
pub fn theme_delete(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    item_verb(php_bin, wp_phar, docroot, "theme", "delete", names)
}
/// A WordPress user for the Users sub-tab (§7.1).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpUser {
    pub id: u64,
    pub login: String,
    pub email: String,
    /// Comma-separated role list (WP-CLI's `roles` field).
    pub roles: String,
    pub name: String,
}

// `wp user list --format=json` wire shape (WP-CLI's snake_case keys).
#[derive(Deserialize)]
struct WireUser {
    #[serde(rename = "ID", default)]
    id: u64,
    #[serde(rename = "user_login", default)]
    login: String,
    #[serde(rename = "user_email", default)]
    email: String,
    #[serde(rename = "roles", default)]
    roles: String,
    #[serde(rename = "display_name", default)]
    name: String,
}

/// `wp user list` (id, login, email, roles, display name).
pub fn user_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpUser>> {
    let wire: Vec<WireUser> = wp_json(
        php_bin,
        wp_phar,
        docroot,
        &["user", "list", "--fields=ID,user_login,user_email,roles,display_name"],
    )?;
    Ok(wire
        .into_iter()
        .map(|u| WpUser { id: u.id, login: u.login, email: u.email, roles: u.roles, name: u.name })
        .collect())
}

/// The site's PRIMARY administrator: the lowest-ID user holding the
/// `administrator` role — the install's original admin on a one-click site
/// (rexenv installs create it as user 1). Backs the "Open admin" one-click
/// login, which must never guess: no administrator ⇒ clean error, the caller
/// falls back to the plain login page.
pub fn primary_admin_id(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<u64> {
    let out = wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["user", "list", "--role=administrator", "--field=ID", "--orderby=ID", "--order=ASC"],
    )?;
    out.lines()
        .next()
        .and_then(|l| l.trim().parse::<u64>().ok())
        .ok_or_else(|| Error::Other("no administrator user found on this site".into()))
}

/// Create a user (`wp user create <login> <email> --role=<role>`). WP-CLI
/// generates a random password; returns the new user's id (via `--porcelain`).
pub fn user_create(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    login: &str,
    email: &str,
    role: &str,
    password: &str,
) -> Result<String> {
    let role_arg = format!("--role={role}");
    // Explicit password (local dev default: a known throwaway) instead of
    // wp-cli's generated one that nobody ever sees. Passed as a single argv
    // element — no shell, no interpolation.
    let pass_arg = format!("--user_pass={password}");
    // Flags first, then `--`, then the positionals: a login/email starting with
    // `-` is a positional, never a wp-cli flag (e.g. can't smuggle --role=admin).
    wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["user", "create", &role_arg, &pass_arg, "--porcelain", login, email],
    )
}

/// Set an existing user's password (`wp user update --user_pass`). wp-cli
/// does not email the user; sessions stay valid per WordPress semantics.
pub fn user_set_password(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    user_id: u64,
    password: &str,
) -> Result<String> {
    if password.is_empty() {
        return Err(Error::Other("password must not be empty".into()));
    }
    let pass_arg = format!("--user_pass={password}");
    wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["user", "update", &user_id.to_string(), &pass_arg],
    )
}

/// Stock roles assignable from the UI. Whitelisted like [`DEBUG_FLAGS`]: the
/// role lands in wp-cli argv.
pub const USER_ROLES: [&str; 5] =
    ["administrator", "editor", "author", "contributor", "subscriber"];

/// Change a user's role (`wp user set-role` — replaces all current roles, same
/// as wp-admin's dropdown). Whitelisted roles only, and the PRIMARY
/// administrator ([`primary_admin_id`]) is protected: demoting it breaks
/// one-click admin login / site tools and can lock the install out of
/// wp-admin entirely.
pub fn user_set_role(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    user_id: u64,
    role: &str,
) -> Result<String> {
    if !USER_ROLES.contains(&role) {
        return Err(Error::Other(format!("not an assignable role: {role}")));
    }
    if user_id == primary_admin_id(php_bin, wp_phar, docroot)? {
        return Err(Error::Other(
            "the primary administrator's role is protected — one-click admin login and \
             rexenv's site tools depend on it. Create another administrator first."
                .into(),
        ));
    }
    wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["user", "set-role", &user_id.to_string(), role],
    )
}

/// Convert a single-site WordPress install to a network (`wp core
/// multisite-convert [--subdomains]`), writing the network constants into
/// wp-config. `subdomains` chooses subdomain vs subdirectory install (§10.1).
pub fn multisite_convert(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    subdomains: bool,
) -> Result<String> {
    let mut args: Vec<&str> = vec!["core", "multisite-convert"];
    if subdomains {
        args.push("--subdomains");
    }
    wp_run(php_bin, wp_phar, docroot, &args)
}

// ── Network / multisite management (§10.3) ───────────────────────────────────

/// One sub-site in a network for the Network sub-tab (`wp site list`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpNetworkSite {
    /// `blog_id` (1 = the main site).
    pub id: String,
    /// Full sub-site URL (e.g. `http://a.mysite.test/`).
    pub url: String,
    pub registered: String,
    /// Soft-deleted (archived) — kept in the list but flagged.
    pub deleted: bool,
}

// `wp site list --format=json` wire shape (all values arrive as strings).
#[derive(Deserialize)]
struct WireSite {
    #[serde(rename = "blog_id", default)]
    blog_id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    registered: String,
    #[serde(default)]
    deleted: String,
}

/// List the network's sub-sites (`wp site list`). Multisite-only — errors on a
/// single-site install (WP-CLI: "This is not a multisite installation").
pub fn network_site_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpNetworkSite>> {
    let wire: Vec<WireSite> = wp_json(
        php_bin,
        wp_phar,
        docroot,
        &["site", "list", "--fields=blog_id,url,registered,deleted"],
    )?;
    Ok(wire
        .into_iter()
        .map(|s| WpNetworkSite {
            id: s.blog_id,
            url: s.url,
            registered: s.registered,
            deleted: s.deleted == "1",
        })
        .collect())
}

/// Create a sub-site by slug (`wp site create --slug=<slug>`). The slug becomes a
/// subdomain (`<slug>.mysite.test`) or a path (`mysite.test/<slug>`) per the
/// network's install type. Returns the new `blog_id` (via `--porcelain`).
pub fn network_site_create(php_bin: &Path, wp_phar: &Path, docroot: &Path, slug: &str) -> Result<String> {
    let slug_arg = format!("--slug={slug}");
    wp_run(php_bin, wp_phar, docroot, &["site", "create", &slug_arg, "--porcelain"])
}

/// Delete a sub-site by `blog_id` (`wp site delete <id> --yes`). The main site
/// (id 1) can't be deleted — WP-CLI rejects it.
pub fn network_site_delete(php_bin: &Path, wp_phar: &Path, docroot: &Path, blog_id: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["site", "delete", "--yes", blog_id])
}

/// Network-activate one or more plugins (`wp plugin activate … --network`) — they
/// become `active-network` (the "Network active" badge) for every sub-site.
pub fn plugin_activate_network(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args: Vec<&str> = vec!["plugin", "activate"];
    args.extend(names.iter().map(String::as_str));
    args.push("--network");
    wp_run(php_bin, wp_phar, docroot, &args)
}

/// Network-deactivate one or more plugins (`wp plugin deactivate … --network`).
pub fn plugin_deactivate_network(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args: Vec<&str> = vec!["plugin", "deactivate"];
    args.extend(names.iter().map(String::as_str));
    args.push("--network");
    wp_run(php_bin, wp_phar, docroot, &args)
}

/// Network-enable a theme (`wp theme enable <name> --network`) — make it available
/// to every sub-site.
pub fn theme_enable_network(php_bin: &Path, wp_phar: &Path, docroot: &Path, name: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["theme", "enable", name, "--network"])
}

/// Network-disable a theme (`wp theme disable <name> --network`).
pub fn theme_disable_network(php_bin: &Path, wp_phar: &Path, docroot: &Path, name: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["theme", "disable", name, "--network"])
}

/// List the network's super-admins (`wp super-admin list` — one login per line).
pub fn super_admin_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<String>> {
    let out = wp_run(php_bin, wp_phar, docroot, &["super-admin", "list"])?;
    Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

/// Grant super-admin to a user (`wp super-admin add <user>`).
pub fn super_admin_add(php_bin: &Path, wp_phar: &Path, docroot: &Path, user: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["super-admin", "add", user])
}

// ── Tools (§7.2) ─────────────────────────────────────────────────────────────

/// Whether `WP_DEBUG` is enabled (`wp config get WP_DEBUG`). A missing constant
/// reads as off.
pub fn wp_debug_get(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<bool> {
    let v = wp_run(php_bin, wp_phar, docroot, &["config", "get", "WP_DEBUG"]).unwrap_or_default();
    Ok(matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

/// Toggle full WordPress debugging (`wp config set … --raw` so values are
/// boolean constants, not the string `"true"`).
///
/// On ⇒ the recommended development trio: `WP_DEBUG` + `WP_DEBUG_LOG` (write to
/// `wp-content/debug.log` — PHP creates the file on the first entry) with
/// `WP_DEBUG_DISPLAY` false (log, don't print; core's `wp_debug_mode()` also
/// runs `ini_set('display_errors', 0)` for us when it's false).
/// Off ⇒ `WP_DEBUG`/`WP_DEBUG_LOG` false and `WP_DEBUG_DISPLAY` removed, i.e.
/// back to a stock wp-config.
pub fn wp_debug_set(php_bin: &Path, wp_phar: &Path, docroot: &Path, on: bool) -> Result<String> {
    let val = if on { "true" } else { "false" };
    let out = wp_run(php_bin, wp_phar, docroot, &["config", "set", "WP_DEBUG", val, "--raw"])?;
    wp_run(php_bin, wp_phar, docroot, &["config", "set", "WP_DEBUG_LOG", val, "--raw"])?;
    if on {
        wp_run(php_bin, wp_phar, docroot, &["config", "set", "WP_DEBUG_DISPLAY", "false", "--raw"])?;
    } else {
        // May not exist (e.g. debugging was enabled by hand) — not an error.
        let _ = wp_run(php_bin, wp_phar, docroot, &["config", "delete", "WP_DEBUG_DISPLAY"]);
    }
    Ok(out)
}

/// One scheduled cron event (Tools → Cron).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpCronEvent {
    pub hook: String,
    /// GMT timestamp, WP-CLI's `next_run_gmt` (e.g. `2026-07-11 12:00:00`).
    pub next_run: String,
    /// Human offset, WP-CLI's `next_run_relative` (e.g. `11 hours 4 minutes`).
    pub next_run_relative: String,
    /// `1 hour`, `1 day`, … or `Non-repeating`.
    pub recurrence: String,
}

// `wp cron event list --format=json` wire shape (WP-CLI's snake_case keys).
#[derive(Deserialize)]
struct WireCronEvent {
    #[serde(default)]
    hook: String,
    #[serde(default)]
    next_run_gmt: String,
    #[serde(default)]
    next_run_relative: String,
    #[serde(default)]
    recurrence: String,
}

/// List scheduled cron events, soonest first (WP-CLI's default order).
pub fn cron_event_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpCronEvent>> {
    let wire: Vec<WireCronEvent> = wp_json(
        php_bin,
        wp_phar,
        docroot,
        &["cron", "event", "list", "--fields=hook,next_run_gmt,next_run_relative,recurrence"],
    )?;
    Ok(wire
        .into_iter()
        .map(|e| WpCronEvent {
            hook: e.hook,
            next_run: e.next_run_gmt,
            next_run_relative: e.next_run_relative,
            recurrence: e.recurrence,
        })
        .collect())
}

/// Run every cron event that is currently due (`wp cron event run --due-now`).
/// Returns WP-CLI's "Executed a total of N cron events" message.
pub fn cron_run_due(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["cron", "event", "run", "--due-now"])
}

/// Run ONE hook's scheduled event(s) immediately, due or not
/// (`wp cron event run <hook>`). WP-CLI addresses cron events by hook name —
/// there is no per-instance id — so a hook scheduled more than once runs every
/// instance. Hook names are site-defined (no whitelist possible); they pass as
/// a single argv element, never through a shell.
pub fn cron_run_hook(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    hook: &str,
) -> Result<String> {
    let hook = hook.trim();
    if hook.is_empty() {
        return Err(Error::Other("empty cron hook name".into()));
    }
    wp_run(php_bin, wp_phar, docroot, &cron_run_hook_argv(hook))
}

/// Argv for running one cron hook: `[cron, event, run, hook]`. NO `--` — WP-CLI
/// treats a bare `--` as a literal positional, so `["cron","event","run","--",h]`
/// made WP-CLI see `--` as the hook name ("Invalid cron event '--'"). The hook
/// passes as a single argv element, never through a shell.
fn cron_run_hook_argv(hook: &str) -> [&str; 4] {
    ["cron", "event", "run", hook]
}

/// Result of `wp core verify-checksums` (Tools → Maintenance). A failed
/// verification is a RESULT, not an `Err` — errors are reserved for wp-cli
/// itself failing to run. Findings are split so the UI can tell harmless OS
/// clutter from a genuinely modified install:
/// - `real`: modified core files, missing core files, and foreign files that
///   are NOT known OS noise (plus any warning we don't recognize — unknown
///   stays loud, never silently benign).
/// - `benign`: foreign files whose basename is known OS/Finder clutter
///   (`.DS_Store`, AppleDouble `._*`, `Thumbs.db`, …). ONLY "should not
///   exist" findings can be benign — a modified/missing core file never is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpChecksumReport {
    /// Raw wp-cli exit verdict. CAVEAT (verified live): extra "should not
    /// exist" files do NOT fail the command — it exits 0 with a Success line
    /// despite those warnings; only modified/missing core files exit 1. So
    /// `ok` alone must never drive a pass decision; `real` is the signal.
    pub ok: bool,
    pub real: Vec<String>,
    pub benign: Vec<String>,
    /// Raw combined stdout+stderr (warnings arrive on stderr).
    pub output: String,
}

/// OS/editor clutter that Finder & co. drop into directories — matched on the
/// path's basename only, so `foo/.DS_Store-backdoor.php` stays a real finding.
fn is_os_noise(path: &str) -> bool {
    const NOISE: [&str; 7] = [
        ".DS_Store",
        "Thumbs.db",
        "desktop.ini",
        ".Spotlight-V100",
        ".fseventsd",
        ".Trashes",
        ".localized",
    ];
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    // AppleDouble `._*` sidecars are benign clutter — EXCEPT an executable
    // `._x.php`: a planted `wp-includes/._x.php` is real (PHP runs it), so it
    // must surface as a finding, not hide in the noise bucket (B30).
    NOISE.contains(&name) || (name.starts_with("._") && !name.ends_with(".php"))
}

/// Split WP-CLI's verify-checksums warnings into real vs benign (see
/// [`WpChecksumReport`]). The trailing "Error: … doesn't verify" summary line
/// carries no per-file info and is skipped.
fn classify_checksum_output(text: &str) -> (Vec<String>, Vec<String>) {
    let mut real = Vec::new();
    let mut benign = Vec::new();
    for line in text.lines() {
        let Some(msg) = line.trim().strip_prefix("Warning: ") else {
            continue;
        };
        match msg.strip_prefix("File should not exist: ") {
            Some(path) if is_os_noise(path) => benign.push(path.to_string()),
            // Non-noise extras, modified ("doesn't verify against checksum"),
            // missing ("doesn't exist"), and anything unrecognized: loud.
            _ => real.push(msg.to_string()),
        }
    }
    (real, benign)
}

/// Verify core files against wordpress.org's checksums for the installed
/// version. Detects modified/missing core files and foreign files in core dirs.
pub fn core_verify_checksums(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
) -> Result<WpChecksumReport> {
    let path = format!("--path={}", docroot.display());
    let out = wp_cli(php_bin, wp_phar, &["core", "verify-checksums", &path], None)?;
    let mut text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !err.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&err);
    }
    let (real, benign) = classify_checksum_output(&text);
    Ok(WpChecksumReport { ok: out.status.success(), real, benign, output: text })
}

/// One file [`cleanup_os_noise`] did NOT delete, with the reason.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedNoiseFile {
    pub path: String,
    pub reason: String,
}

/// Result of [`cleanup_os_noise`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoiseCleanup {
    pub removed: u32,
    pub skipped: Vec<SkippedNoiseFile>,
}

/// Delete known-noise files (the checksum panel's `benign` list) from a
/// docroot. The paths come from the UI and are NEVER trusted — every file is
/// re-validated from scratch by [`noise_delete_one`]'s guards; anything that
/// fails a guard or errors is SKIPPED with a reason, never aborting the rest.
/// Deletion is a direct `remove_file` (not Trash): the entire reachable scope
/// is regenerable Finder metadata.
pub fn cleanup_os_noise(docroot: &Path, paths: &[String]) -> Result<NoiseCleanup> {
    let root = docroot
        .canonicalize()
        .map_err(|e| Error::Other(format!("site folder {}: {e}", docroot.display())))?;
    let mut removed = 0u32;
    let mut skipped = Vec::new();
    for rel in paths {
        match noise_delete_one(&root, rel) {
            Ok(()) => removed += 1,
            Err(reason) => skipped.push(SkippedNoiseFile { path: rel.clone(), reason }),
        }
    }
    Ok(NoiseCleanup { removed, skipped })
}

/// [`cleanup_os_noise`] outcome + the fresh post-cleanup verify report, so the
/// UI panel updates in the same round-trip (mirrors frontend `WpChecksumCleanup`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumCleanup {
    pub removed: u32,
    pub skipped: Vec<SkippedNoiseFile>,
    pub report: WpChecksumReport,
}

/// The four delete guards, per file (`canon_root` is the canonicalized docroot):
/// 1. basename passes [`is_os_noise`] — the SAME list classification uses;
/// 2. lexical: relative with only normal components (no `..`, no absolute);
/// 3. the entry itself (lstat) is a regular file — this, not canonicalize,
///    is what stops symlinks: a symlink named `.DS_Store` canonicalizes to its
///    TARGET, and if that target is a regular file inside the docroot (say
///    `wp-config.php`) the canonical-path checks all pass — deleting the
///    target. lstat sees the link itself and refuses anything but a plain file;
/// 4. the canonicalized path stays under the canonical docroot — catches the
///    remaining escape: a symlinked intermediate DIRECTORY resolving outside.
fn noise_delete_one(canon_root: &Path, rel: &str) -> std::result::Result<(), String> {
    if !is_os_noise(rel) {
        return Err("not a known macOS system file".into());
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || !rel_path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err("path escapes the site folder".into());
    }
    let joined = canon_root.join(rel_path);
    let lmeta = joined
        .symlink_metadata()
        .map_err(|e| format!("cannot stat: {e}"))?;
    if lmeta.file_type().is_symlink() {
        return Err("symbolic link — not followed".into());
    }
    if !lmeta.is_file() {
        return Err("not a regular file".into());
    }
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("cannot resolve: {e}"))?;
    if !canon.starts_with(canon_root) {
        return Err("resolves outside the site folder".into());
    }
    std::fs::remove_file(&canon).map_err(|e| format!("cannot delete: {e}"))
}

/// Export site content (posts, pages, comments, menus, terms) as WXR XML into
/// the user's Downloads folder (`wp export --dir=…` — same destination
/// convention as the DB export). WP-CLI names the files itself
/// (`<site>.WordPress.<date>.xml`) and may split a large export into several;
/// the paths are parsed from its "Writing to file" lines.
pub fn content_export_to_downloads(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
) -> Result<Vec<String>> {
    let downloads = super::downloads::user_downloads_dir()?;
    let dir_arg = format!("--dir={}", downloads.display());
    let out = wp_run(php_bin, wp_phar, docroot, &["export", &dir_arg])?;
    let files: Vec<String> = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Writing to file "))
        .map(|p| p.trim().to_string())
        .collect();
    if files.is_empty() {
        return Err(Error::Other(format!(
            "wp export reported no output file — output was: {out}"
        )));
    }
    Ok(files)
}

/// Flush the WordPress object cache (`wp cache flush`).
pub fn cache_flush(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["cache", "flush"])
}

/// Delete ALL transients — expired or not (`wp transient delete --all`).
/// Returns WP-CLI's "N transients deleted" message for the UI toast.
pub fn transient_delete_all(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["transient", "delete", "--all"])
}

/// Permalink structures the UI picker offers (wp-admin's stock choices; `""` =
/// Plain). Whitelisted like [`DEBUG_FLAGS`]: the value lands in wp-cli argv.
pub const PERMALINK_STRUCTURES: [&str; 5] = [
    "",
    "/%year%/%monthnum%/%day%/%postname%/",
    "/%year%/%monthnum%/%postname%/",
    "/archives/%post_id%",
    "/%postname%/",
];

/// The site's current permalink structure (`""` = Plain).
pub fn permalink_structure_get(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    Ok(
        wp_run(php_bin, wp_phar, docroot, &["option", "get", "permalink_structure"])
            .unwrap_or_default()
            .trim()
            .to_string(),
    )
}

/// Set the permalink structure to one of [`PERMALINK_STRUCTURES`] and flush the
/// rewrite rules. `option update` (not `rewrite structure`) so Plain's empty
/// string takes the same path as every preset.
pub fn permalink_structure_set(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    structure: &str,
) -> Result<String> {
    if !PERMALINK_STRUCTURES.contains(&structure) {
        return Err(Error::Other(format!(
            "not a supported permalink structure: {structure:?}"
        )));
    }
    wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["option", "update", "permalink_structure", structure],
    )?;
    rewrite_flush(php_bin, wp_phar, docroot)
}

/// Boolean wp-config constants the UI may toggle individually (Tools →
/// Debugging). A whitelist: the name lands in wp-cli argv, so free-form input
/// would be config injection at the trust boundary.
pub const DEBUG_FLAGS: [&str; 3] = ["WP_DEBUG_LOG", "WP_DEBUG_DISPLAY", "SCRIPT_DEBUG"];

fn ensure_debug_flag(name: &str) -> Result<()> {
    if DEBUG_FLAGS.contains(&name) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "not a toggleable debug constant: {name}"
        )))
    }
}

/// Read one whitelisted boolean wp-config constant (unset ⇒ false).
pub fn config_flag_get(php_bin: &Path, wp_phar: &Path, docroot: &Path, name: &str) -> Result<bool> {
    ensure_debug_flag(name)?;
    let v = wp_run(php_bin, wp_phar, docroot, &["config", "get", name]).unwrap_or_default();
    Ok(matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

/// Set one whitelisted boolean wp-config constant (`--raw` ⇒ a real boolean,
/// not the string `"true"`). Explicit `false` rather than delete, so the state
/// the UI shows is the state wp-config declares.
pub fn config_flag_set(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    name: &str,
    on: bool,
) -> Result<String> {
    ensure_debug_flag(name)?;
    let val = if on { "true" } else { "false" };
    wp_run(php_bin, wp_phar, docroot, &["config", "set", name, val, "--raw"])
}

/// Whether WP-CLI maintenance mode is active for the site (the `.maintenance`
/// file WP-CLI manages in the docroot). `is-active` exits 0 when active,
/// non-zero when not — same status-as-answer shape as `core is-installed`.
pub fn maintenance_mode_get(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<bool> {
    let path = format!("--path={}", docroot.display());
    Ok(
        wp_cli(php_bin, wp_phar, &["maintenance-mode", "is-active", &path], None)
            .map(|o| o.status.success())
            .unwrap_or(false),
    )
}

/// Toggle maintenance mode. Idempotent: WP-CLI errors on activate-when-active /
/// deactivate-when-inactive, so an already-in-state site is a no-op success.
pub fn maintenance_mode_set(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    on: bool,
) -> Result<String> {
    if maintenance_mode_get(php_bin, wp_phar, docroot)? == on {
        return Ok(String::new());
    }
    let sub = if on { "activate" } else { "deactivate" };
    wp_run(php_bin, wp_phar, docroot, &["maintenance-mode", sub])
}

/// Run `wp search-replace <from> <to> [--dry-run] --format=count` and return the
/// number of replacements (a dry-run reports the count WITHOUT changing data).
/// wp-cli walks serialized PHP data correctly — this is the safe way to rewrite
/// URLs, never raw SQL. `all_tables` adds `--all-tables` (every table in the
/// site's database, not just the ones matching the WP prefix) — each site owns
/// its database, so this is safe and what a domain change needs.
pub fn search_replace(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    from: &str,
    to: &str,
    dry_run: bool,
    all_tables: bool,
) -> Result<u64> {
    let mut args: Vec<&str> = vec!["search-replace", "--format=count"];
    if all_tables {
        args.push("--all-tables");
    }
    if dry_run {
        args.push("--dry-run");
    }
    // NO `--` separator: WP-CLI doesn't honor getopt end-of-flags (a bare `--`
    // becomes a literal positional). from/to follow the flags directly; a term
    // that itself looks like a flag is a WP-CLI limitation, not something `--`
    // could fix — and it's the user's own dev DB (same-privilege footgun).
    args.push(from);
    args.push(to);
    let out = wp_run(php_bin, wp_phar, docroot, &args)?;
    out.trim()
        .lines()
        .last()
        .unwrap_or("0")
        .trim()
        .parse::<u64>()
        .map_err(|e| Error::Other(format!("search-replace count: {e} (output: {out:?})")))
}

/// Regenerate permalinks (`wp rewrite flush`).
pub fn rewrite_flush(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["rewrite", "flush"])
}

/// Update WordPress core to the latest release (`wp core update`).
pub fn core_update(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run_timed(php_bin, wp_phar, docroot, &["core", "update"], download_timeout(1))
}

/// Re-download core files of the current version (`wp core download --force`) —
/// repairs a corrupt/modified core without touching the DB or wp-content.
pub fn core_reinstall(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run_timed(
        php_bin,
        wp_phar,
        docroot,
        &["core", "download", "--force", "--skip-content"],
        download_timeout(1),
    )
}

/// Input/validation kind of a whitelisted option (drives the UI input AND the
/// backend validation — both sides of the same rule).
#[derive(Debug, Clone, Copy, PartialEq)]
enum OptionKind {
    Text,
    Email,
    /// Integer within `[min, max]` inclusive.
    IntRange(i64, i64),
    /// `"0"` / `"1"` (WP stores booleans as those strings).
    Bool,
    /// Day-of-week int `0..=6` (0 = Sunday).
    Weekday,
    /// A PHP `timezone_identifiers_list()` entry, or empty (site uses a raw
    /// UTC offset via `gmt_offset` — a legitimate state, seen live).
    Timezone,
    /// A role slug from `wp role list` (covers custom roles).
    Role,
}

struct OptionField {
    name: &'static str,
    label: &'static str,
    kind: OptionKind,
}

/// The options editor's ENTIRE reachable surface — default-deny. Nothing
/// outside this list can be read for editing or written, so foot-guns
/// (`siteurl`, `home`, `active_plugins`, `template`, any serialized option…)
/// aren't "blocked", they're unreachable by construction. Every entry is a
/// scalar on a standard install (verified live); non-scalar values are refused
/// at read AND write time anyway. `WPLANG` is deliberately absent — the
/// Language card owns it (install/download flow).
const OPTION_FIELDS: &[OptionField] = &[
    OptionField { name: "blogname", label: "Site title", kind: OptionKind::Text },
    OptionField { name: "blogdescription", label: "Tagline", kind: OptionKind::Text },
    OptionField { name: "admin_email", label: "Admin email", kind: OptionKind::Email },
    OptionField { name: "timezone_string", label: "Timezone", kind: OptionKind::Timezone },
    OptionField { name: "date_format", label: "Date format", kind: OptionKind::Text },
    OptionField { name: "time_format", label: "Time format", kind: OptionKind::Text },
    OptionField { name: "start_of_week", label: "Week starts on", kind: OptionKind::Weekday },
    OptionField {
        name: "posts_per_page",
        label: "Posts per page",
        kind: OptionKind::IntRange(1, 1000),
    },
    OptionField { name: "default_role", label: "New user default role", kind: OptionKind::Role },
    OptionField {
        name: "users_can_register",
        label: "Anyone can register",
        kind: OptionKind::Bool,
    },
    OptionField {
        name: "blog_public",
        label: "Visible to search engines",
        kind: OptionKind::Bool,
    },
];

fn option_field(name: &str) -> Option<&'static OptionField> {
    OPTION_FIELDS.iter().find(|f| f.name == name)
}

/// Frontend tag for an [`OptionKind`].
fn option_kind_str(kind: OptionKind) -> &'static str {
    match kind {
        OptionKind::Text => "text",
        OptionKind::Email => "email",
        OptionKind::IntRange(..) => "int",
        OptionKind::Bool => "bool",
        OptionKind::Weekday => "weekday",
        OptionKind::Timezone => "timezone",
        OptionKind::Role => "role",
    }
}

/// A JSON scalar as its WP display/storage string; `None` for array/object/
/// null — the "never round-trip PHP serialization" guard.
fn scalar_display(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(if *b { "1" } else { "0" }.into()),
        _ => None,
    }
}

/// Minimal email shape check (`local@domain.tld`, no whitespace) — enough to
/// stop typos; WP itself does no validation on a direct option write.
fn valid_email(v: &str) -> bool {
    let Some((local, domain)) = v.split_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !v.contains(char::is_whitespace)
}

/// Backend per-kind validation — runs on every write regardless of what the
/// UI already checked. `timezones`/`roles` are only consulted for those kinds.
fn validate_option_value(
    kind: OptionKind,
    value: &str,
    timezones: &[String],
    roles: &[String],
) -> std::result::Result<(), String> {
    let int_in = |min: i64, max: i64| {
        value
            .parse::<i64>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .map(|_| ())
            .ok_or(format!("must be a whole number between {min} and {max}"))
    };
    match kind {
        OptionKind::Text => Ok(()),
        OptionKind::Email => {
            if valid_email(value) {
                Ok(())
            } else {
                Err("not a valid email address".into())
            }
        }
        OptionKind::IntRange(min, max) => int_in(min, max),
        OptionKind::Bool => {
            if matches!(value, "0" | "1") {
                Ok(())
            } else {
                Err("must be 0 or 1".into())
            }
        }
        OptionKind::Weekday => int_in(0, 6),
        OptionKind::Timezone => {
            if value.is_empty() || timezones.iter().any(|t| t == value) {
                Ok(())
            } else {
                Err("not a known timezone".into())
            }
        }
        OptionKind::Role => {
            if roles.iter().any(|r| r == value) {
                Ok(())
            } else {
                Err("not an existing role".into())
            }
        }
    }
}

/// One editable (or refused) option row (mirrors the frontend `WpOptionRow`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpOptionRow {
    pub name: String,
    pub label: String,
    pub kind: String,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub value: String,
    pub editable: bool,
    pub note: Option<String>,
}

/// A role (`wp role list` row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpRole {
    pub name: String,
    pub role: String,
}

/// The whole options form: rows + the choice lists the pickers need.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpOptionsForm {
    pub fields: Vec<WpOptionRow>,
    pub timezones: Vec<String>,
    pub roles: Vec<WpRole>,
}

/// Fixed `wp eval` script reading every whitelisted option in ONE wp-cli call
/// (11 separate `option get`s would cost ~10s of WP boots) plus the timezone
/// list. Built ONLY from `OPTION_FIELDS` consts — no user input reaches it.
fn options_eval_script() -> String {
    let names = OPTION_FIELDS
        .iter()
        .map(|f| format!("\"{}\"", f.name))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "$n=[{names}];$v=[];foreach($n as $x){{$v[$x]=get_option($x);}}\
         echo json_encode([\"values\"=>$v,\"timezones\"=>timezone_identifiers_list()]);"
    )
}

/// Read the options form. `get_option` unserializes, so a serialized value
/// arrives as a JSON array/object → refused per row (`editable: false`,
/// "not editable (non-scalar value)") rather than shown as corruptible text.
pub fn options_get(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<WpOptionsForm> {
    #[derive(Deserialize)]
    struct Eval {
        values: serde_json::Map<String, serde_json::Value>,
        timezones: Vec<String>,
    }
    let out = wp_run(php_bin, wp_phar, docroot, &["eval", &options_eval_script()])?;
    let ev: Eval = json_from_wp(&out, "options read")?;
    let roles: Vec<WpRole> = wp_json(php_bin, wp_phar, docroot, &["role", "list"])?;

    let fields = OPTION_FIELDS
        .iter()
        .map(|f| {
            let (min, max) = match f.kind {
                OptionKind::IntRange(a, b) => (Some(a), Some(b)),
                OptionKind::Weekday => (Some(0), Some(6)),
                _ => (None, None),
            };
            let mut row = WpOptionRow {
                name: f.name.into(),
                label: f.label.into(),
                kind: option_kind_str(f.kind).into(),
                min,
                max,
                value: String::new(),
                editable: false,
                note: None,
            };
            match ev.values.get(f.name).map(scalar_display) {
                Some(Some(v)) => {
                    row.value = v;
                    row.editable = true;
                }
                Some(None) => row.note = Some("not editable (non-scalar value)".into()),
                None => row.note = Some("could not read".into()),
            }
            row
        })
        .collect();
    Ok(WpOptionsForm { fields, timezones: ev.timezones, roles })
}

/// Update ONE whitelisted option. Guards, in order:
/// 1. `name` must be in `OPTION_FIELDS` — anything else ("siteurl", "home",
///    "active_plugins", …) is "not an editable option", checked BEFORE any
///    wp-cli call, so a bypassed UI still can't write them;
/// 2. per-kind value validation (choice kinds fetch their live list);
/// 3. the CURRENT value must be a JSON scalar — a serialized option is never
///    overwritten even if its name were whitelisted.
pub fn option_update(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    name: &str,
    value: &str,
) -> Result<()> {
    let field =
        option_field(name).ok_or_else(|| Error::Other(format!("not an editable option: {name}")))?;

    let timezones: Vec<String> = if field.kind == OptionKind::Timezone {
        let out =
            wp_run(php_bin, wp_phar, docroot, &["eval", "echo json_encode(timezone_identifiers_list());"])?;
        json_from_wp(&out, "timezone list")?
    } else {
        Vec::new()
    };
    let roles: Vec<String> = if field.kind == OptionKind::Role {
        let rows: Vec<WpRole> = wp_json(php_bin, wp_phar, docroot, &["role", "list"])?;
        rows.into_iter().map(|r| r.role).collect()
    } else {
        Vec::new()
    };
    validate_option_value(field.kind, value, &timezones, &roles)
        .map_err(|reason| Error::Other(format!("{}: {reason}", field.label)))?;

    let cur = wp_run(php_bin, wp_phar, docroot, &["option", "get", name, "--format=json"])?;
    let cur_v: serde_json::Value = json_from_wp(&cur, &format!("option {name}"))?;
    if scalar_display(&cur_v).is_none() {
        return Err(Error::Other(format!("{name} is not editable (non-scalar value)")));
    }

    wp_run(php_bin, wp_phar, docroot, &["option", "update", name, value])?;
    Ok(())
}

/// One row of `wp language core list` (mirrors the frontend `WpLanguage`).
/// `status` is `active` | `installed` | `uninstalled`; `en_US` is always
/// present (the built-in default — activating it needs no files).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpLanguage {
    pub language: String,
    #[serde(default, alias = "english_name")]
    pub english_name: String,
    #[serde(default, alias = "native_name")]
    pub native_name: String,
    #[serde(default)]
    pub status: String,
}

/// Available + installed core languages (`wp language core list`). Hits
/// api.wordpress.org for the available-translations list (~3s; needs network).
pub fn language_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpLanguage>> {
    wp_json(php_bin, wp_phar, docroot, &["language", "core", "list"])
}

/// Locale shape guard (`fr_FR`, `pt_BR`, `de_DE_formal`, `ceb`): 2–20 chars,
/// leading lowercase ASCII letter, then letters/digits/underscore only. The UI
/// is a picker fed by [`language_list`]; this is defense in depth so a locale
/// string can never look like a wp-cli flag or smuggle anything into argv.
pub fn valid_locale(locale: &str) -> bool {
    (2..=20).contains(&locale.len())
        && locale.starts_with(|c: char| c.is_ascii_lowercase())
        && locale.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a core language pack is on disk (`wp language core is-installed`
/// exits 0/1). This is the ONLY trustworthy install signal — see
/// [`switch_language`].
fn language_is_installed(php_bin: &Path, wp_phar: &Path, docroot: &Path, locale: &str) -> bool {
    let path = format!("--path={}", docroot.display());
    wp_cli(php_bin, wp_phar, &["language", "core", "is-installed", locale, &path], None)
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Switch the site language in one step: install the core pack if missing, then
/// activate via `wp site switch-language` (`language core activate` is
/// deprecated in wp-cli 2.12). Reversible — switching to `en_US` restores the
/// default (empties `WPLANG`).
///
/// `language core install` LIES on failure: an unavailable locale or a failed/
/// offline download still exits **0** ("Installed 0 of 1 languages (1 skipped)")
/// with only a stderr warning. Success is therefore gated on `is-installed`
/// AFTER the install — never on install's exit code or output. On a failed
/// download nothing was activated, so the site language is unchanged.
/// Cap on the language-pack download. WP's own `download_url` waits up to 300s
/// per attempt, so offline the install "hangs" for minutes at a spinner. 60s is
/// generous for a ~4MB pack on a slow line and keeps the failure user-visible.
const LANG_INSTALL_TIMEOUT: Duration = Duration::from_secs(60);

pub fn switch_language(php_bin: &Path, wp_phar: &Path, docroot: &Path, locale: &str) -> Result<()> {
    if !valid_locale(locale) {
        return Err(Error::Other(format!("invalid locale: {locale:?}")));
    }
    if !language_is_installed(php_bin, wp_phar, docroot, locale) {
        let path = format!("--path={}", docroot.display());
        // Timed AND its result deliberately not trusted: install lies (exit 0
        // on a failed download) and stalls for minutes offline. Whatever it
        // claims — success, error, or timeout — `is-installed` below is the
        // only verdict; the outcome only feeds the error detail.
        let detail = match wp_cli_timed(
            php_bin,
            wp_phar,
            &["language", "core", "install", locale, &path],
            LANG_INSTALL_TIMEOUT,
        ) {
            Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_string(),
            Err(e) => e.to_string(),
        };
        if !language_is_installed(php_bin, wp_phar, docroot, locale) {
            return Err(Error::Other(format!(
                "Couldn't download the {locale} language pack — check your connection.{}",
                if detail.is_empty() { String::new() } else { format!(" ({detail})") }
            )));
        }
    }
    wp_run(php_bin, wp_phar, docroot, &["site", "switch-language", locale])?;
    Ok(())
}

/// One WordPress release from the stable-check API (mirrors the frontend
/// `WpCoreVersion`). `status` ∈ `latest` | `outdated` | `insecure`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpCoreVersion {
    pub version: String,
    pub status: String,
}

/// `"X.Y"` / `"X.Y.Z"` → sortable tuple; `None` for anything else. Doubles as
/// the argv shape guard: digits and dots only, so a version can never read as
/// a wp-cli flag.
fn parse_wp_version(v: &str) -> Option<(u64, u64, u64)> {
    if v.is_empty() || !v.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let mut it = v.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = match it.next() {
        Some(s) => s.parse().ok()?,
        None => 0,
    };
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

const STABLE_CHECK_URL: &str = "https://api.wordpress.org/core/stable-check/1.0/";

/// Installable releases, newest first, from wordpress.org's stable-check API
/// (`version → status`; 800+ entries live). Filtered to ≥ 6.0 — older cores
/// predate the bundled PHP versions. Needs network; offline surfaces the
/// download error, and the picker simply doesn't load.
pub async fn core_versions() -> Result<Vec<WpCoreVersion>> {
    let body = super::binaries::http_get(STABLE_CHECK_URL).await?;
    parse_stable_check(&body)
}

fn parse_stable_check(body: &[u8]) -> Result<Vec<WpCoreVersion>> {
    let map: std::collections::BTreeMap<String, String> = serde_json::from_slice(body)
        .map_err(|e| Error::Other(format!("version list: bad JSON: {e}")))?;
    let mut rows: Vec<((u64, u64, u64), WpCoreVersion)> = map
        .into_iter()
        .filter_map(|(version, status)| {
            parse_wp_version(&version)
                .filter(|t| t.0 >= 6)
                .map(|t| (t, WpCoreVersion { version, status }))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(rows.into_iter().map(|(_, v)| v).collect())
}

/// Result of a core version switch (mirrors the frontend `WpCoreSwitch`).
/// `db_update_required` tells the panel — explicitly, not guessed — whether
/// wp-admin will show the "Database Update Required" screen: WP redirects on
/// ANY `db_version` mismatch (`wp-admin/admin.php`), including DB-newer-than-
/// code after a downgrade; running it re-stamps the option (`upgrade.php`) —
/// the schema itself is never downgraded.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpCoreSwitch {
    pub version: String,
    pub db_update_required: bool,
}

/// Cap on the core zip download (~25 MB — needs more headroom than the 60s
/// language cap; still bounded so offline fails visibly, never a frozen
/// spinner).
const CORE_SWITCH_TIMEOUT: Duration = Duration::from_secs(300);

/// Switch core to an exact version: `wp core update --version=<v> --force`
/// (`--force` is the official downgrade path — "update even when installed WP
/// version is greater than the requested version"). `allowed` is a fresh
/// stable-check list; the version must be on it (picker-only, defense in
/// depth). Success is gated on `wp core version` reporting the target
/// afterward — never on the update command's claim (the language/checksum
/// exit-code lesson). wp-content and the DB are untouched.
pub fn core_switch_version(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    version: &str,
    allowed: &[String],
) -> Result<WpCoreSwitch> {
    if parse_wp_version(version).is_none() {
        return Err(Error::Other(format!("invalid version: {version:?}")));
    }
    if !allowed.iter().any(|v| v == version) {
        return Err(Error::Other(format!("{version} is not a known WordPress release")));
    }

    let path = format!("--path={}", docroot.display());
    let varg = format!("--version={version}");
    let update = wp_cli_timed(
        php_bin,
        wp_phar,
        &["core", "update", &varg, "--force", &path],
        CORE_SWITCH_TIMEOUT,
    );
    // The gate: what does core ACTUALLY report now?
    let now = wp_run(php_bin, wp_phar, docroot, &["core", "version"])?;
    if now.trim() != version {
        let detail = match update {
            Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_string(),
            Err(e) => e.to_string(),
        };
        return Err(Error::Other(format!(
            "Couldn't switch to WordPress {version} — core still reports {now}. Check your connection.{}",
            if detail.is_empty() { String::new() } else { format!(" ({detail})") }
        )));
    }

    // Explicit DB answer for the panel: code's $wp_db_version vs the stored option.
    #[derive(Deserialize)]
    struct DbProbe {
        code: i64,
        db: i64,
    }
    let probe = wp_run(
        php_bin,
        wp_phar,
        docroot,
        &[
            "eval",
            "global $wp_db_version; echo json_encode([\"code\"=>(int)$wp_db_version,\"db\"=>(int)get_option(\"db_version\")]);",
        ],
    )?;
    let db: DbProbe = json_from_wp(&probe, "db-version probe")?;

    Ok(WpCoreSwitch { version: version.into(), db_update_required: db.code != db.db })
}

/// The database-name prefix for a site type: `wp_` WordPress, `lv_` Laravel,
/// `php_` plain PHP.
///
/// Until 13 Aug 2026 EVERY site type got `wp_`, because the rule lived here and
/// took only a domain — a Laravel app landed in `wp_myapp_test`, a WordPress
/// label on a database WordPress never touches, and the one string a developer
/// reads in Adminer/TablePlus. Kept SHORT on purpose: the prefix spends the
/// 64-char identifier budget ([`DB_NAME_MAX`]) that the domain slug also needs.
pub fn db_name_prefix(site_type: SiteType) -> &'static str {
    match site_type {
        SiteType::Wordpress => "wp_",
        SiteType::Laravel => "lv_",
        SiteType::Php => "php_",
    }
}

/// A valid MySQL database name derived from a site's TYPE and domain
/// (WordPress `blog.test` → `wp_blog_test`; Laravel → `lv_blog_test`).
/// CREATION-TIME ONLY: the result is stored on the site row (`Site::db_name`,
/// backfilled by migration v6 with the then-universal `wp_` prefix) and every
/// runtime operation reads the stored value — deriving from the domain at
/// runtime would break sites whose domain has changed, and re-deriving with
/// today's prefix would point every pre-existing Laravel/PHP site at a database
/// that does not exist. Nothing renames a database under a live site.
///
/// The type is a PARAMETER rather than defaulted, so a new call site cannot
/// silently inherit `wp_` for a non-WordPress site — the bug this shape fixes.
pub fn db_name_for(site_type: SiteType, domain: &str) -> String {
    let safe: String = domain
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{}{safe}", db_name_prefix(site_type))
}

/// MySQL/MariaDB identifier limit — a database name may not exceed this.
pub const DB_NAME_MAX: usize = 64;

/// A hash-disambiguated database name for a domain: the [`db_name_for`] base
/// truncated to fit, plus a stable FNV-1a hash of the FULL domain. Used at
/// create ONLY when the clean base would collide with an existing site or exceed
/// the 64-char identifier limit ([`crate::core::sites`] decides). `db_name_for`
/// is NOT injective — it maps every non-alphanumeric char to `_`, so
/// `a-b.test` and `a.b.test` both reduce to `wp_a_b_test`; the domain hash makes
/// two such sites land in DISTINCT databases instead of silently sharing one.
pub fn db_name_disambiguated(site_type: SiteType, domain: &str) -> String {
    let base = db_name_for(site_type, domain);
    let suffix = format!("{:08x}", fnv1a(domain.as_bytes()));
    let keep = DB_NAME_MAX - 1 - suffix.len(); // reserve "_" + the 8-hex suffix
    let head: String = base.chars().take(keep).collect();
    format!("{head}_{suffix}")
}

/// FNV-1a (32-bit) — a small, dependency-free stable hash (the same primitive
/// the per-site override-port allocator uses). Shared with
/// `core::dbmirror::dedicated_user_name`, which caps at MySQL's 32-char USER
/// limit with the same disambiguation reasoning.
pub(crate) fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Inputs for a one-click WordPress install. Phase 1 installs **single-site**.
pub struct WpInstall<'a> {
    pub docroot: &'a Path,
    pub db_name: &'a str,
    /// `host:port`, e.g. `127.0.0.1:13306`.
    pub db_host: &'a str,
    /// Bundled MySQL-protocol client BINARY that creates the DB (`bin/mysql`
    /// from the MySQL tree, or `bin/mariadb` from the mariadb bundle).
    pub db_client: &'a SqlClient,
    /// Full site URL, e.g. `https://mysite.test`.
    pub url: &'a str,
    pub title: &'a str,
    pub admin_user: &'a str,
    pub admin_password: &'a str,
    pub admin_email: &'a str,
    /// WordPress locale for `core download` (e.g. `fr_FR`); empty = default en_US.
    pub locale: &'a str,
}

/// Per-site WordPress install options from the New Site dialog. Empty fields fall
/// back to sensible defaults derived from the site (mirrors the frontend DTO).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstallOptions {
    pub title: String,
    pub admin_user: String,
    pub admin_email: String,
    pub admin_password: String,
    pub language: String,
}

/// One-click WordPress install via WP-CLI: download core → write wp-config →
/// create the DB → `wp core install` (single-site). Each step is skipped if
/// already done, so the flow is re-runnable.
pub fn install_wordpress(php_bin: &Path, wp_phar: &Path, opts: &WpInstall) -> Result<()> {
    let path = format!("--path={}", opts.docroot.display());

    // 1) WordPress core (optionally a localized build).
    if !opts.docroot.join("wp-load.php").exists() {
        let locale = format!("--locale={}", opts.locale);
        let mut args = vec!["core", "download", &path];
        if !opts.locale.trim().is_empty() {
            args.push(&locale);
        }
        wp_cli_checked(php_bin, wp_phar, &args, None)?;
    }

    // 2) wp-config.php (skip the live DB check — the DB is created next).
    if !opts.docroot.join("wp-config.php").exists() {
        let dbname = format!("--dbname={}", opts.db_name);
        let dbhost = format!("--dbhost={}", opts.db_host);
        wp_cli_checked(
            php_bin,
            wp_phar,
            &[
                "config",
                "create",
                &path,
                &dbname,
                "--dbuser=root",
                "--dbpass=",
                &dbhost,
                "--skip-check",
            ],
            None,
        )?;
    }

    // 3) Create the database (idempotent) with the BUNDLED mysql client.
    //    `wp db create` shells out to a `mysql` found on PATH — absent in a
    //    Finder-launched app — and its swallowed failure used to surface later
    //    as `wp core install`'s "Cannot select database".
    let port = opts
        .db_host
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .unwrap_or(super::database::MYSQL_PORT);
    super::database::create_database(opts.db_client, port, opts.db_name)?;

    // 4) Install (single-site) if not already installed.
    let installed = wp_cli(php_bin, wp_phar, &["core", "is-installed", &path], None)
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !installed {
        let url = format!("--url={}", opts.url);
        let title = format!("--title={}", opts.title);
        let au = format!("--admin_user={}", opts.admin_user);
        let ap = format!("--admin_password={}", opts.admin_password);
        let ae = format!("--admin_email={}", opts.admin_email);
        wp_cli_checked(
            php_bin,
            wp_phar,
            &[
                "core", "install", &path, &url, &title, &au, &ap, &ae,
            ],
            None,
        )?;
    }
    Ok(())
}

/// One-click install for a provisioned site (Phase 3 §1.2): fill the install
/// fields from `opts`, defaulting from `domain`/`name` where empty, and delegate
/// to [`install_wordpress`]. The canonical URL is `https://<domain>`; `db_name`
/// is the site's STORED database name (`Site::db_name` — derived once at
/// creation, never from the current domain). `db_host` is `host:port` (e.g.
/// `127.0.0.1:13306`); `db_client` is the site engine's bundled client binary
/// (creates the DB).
#[allow(clippy::too_many_arguments)] // flat mirror of the New Site dialog inputs
pub fn install_for_site(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    domain: &str,
    name: &str,
    db_name: &str,
    db_host: &str,
    db_client: &SqlClient,
    opts: &InstallOptions,
) -> Result<()> {
    let r = resolve_install_options(domain, name, opts);
    install_wordpress(
        php_bin,
        wp_phar,
        &WpInstall {
            docroot,
            db_name,
            db_host,
            db_client,
            url: &r.url,
            title: &r.title,
            admin_user: &r.admin_user,
            admin_password: &r.admin_password,
            admin_email: &r.admin_email,
            locale: &r.locale,
        },
    )
}

/// Everything a STREAMED wp-cli step needs besides its own args — bundled so
/// the provision job's step calls stay readable. Streamed steps run through
/// `repo::run_step_streamed` (supervisor spawn, pgid CancelToken, ANSI-strip
/// line splitting) with `idle_limit: None` — the B25 rule: wp-cli's silent
/// mid-download stretch would tie-race a 300s idle guard; the outer
/// wall-clock cap is the caller's timer.
pub struct WpStream<'a> {
    pub sup: &'a dyn crate::platform::traits::ProcessSupervisor,
    pub env: &'a [(String, String)],
    pub cancel: &'a super::repo::CancelToken,
}

/// Run one streamed wp-cli step in `docroot` (`--path=` single-token appended
/// — a bare `--path <dir>` parses as a boolean + a positional "slug").
pub fn wp_step_streamed(
    stream: &WpStream,
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[&str],
    on_line: &mut dyn FnMut(&str),
) -> Result<super::repo::StepResult> {
    let mut full: Vec<String> = wp_argv_prefix(wp_phar);
    full.extend(args.iter().map(|s| s.to_string()));
    // The WordPress ROOT, which is the docroot everywhere except Roots'
    // layouts — there Composer installs core into `<docroot>/wp`, and this line
    // pinned wp-cli to a directory with no `wp-load.php` in it. A cloned
    // Bedrock site's `wp core install` failed with "This does not seem to be a
    // WordPress installation" until this stopped assuming (found by
    // `git_site_provision_check`, 11 Aug 2026).
    full.push(format!("--path={}", core_root(docroot).display()));
    // A streamed spawn REPLACES the child's environment with this list (macOS
    // `spawn_streamed` env_clears), and the list is the user's login shell — so
    // an exported `WP_CLI_PACKAGES_DIR` arrives here as the ordinary case.
    let env = wp_packages::with_pinned_packages(stream.env, wp_phar);
    super::repo::run_step_streamed(
        stream.sup, php_bin, &full, docroot, &env, stream.cancel, on_line, None,
    )
}

/// Whether core is already installed in `docroot` (captured, instant —
/// `wp core is-installed` exits 0/1 and prints nothing useful).
pub fn core_is_installed(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> bool {
    let path = format!("--path={}", docroot.display());
    wp_cli(php_bin, wp_phar, &["core", "is-installed", &path], None)
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The install field set with every default resolved (mirrors
/// [`install_for_site`]'s rules — factored so the streamed provision job and
/// the captured path share ONE defaulting truth).
pub struct ResolvedInstall {
    pub url: String,
    pub title: String,
    pub admin_user: String,
    pub admin_password: String,
    pub admin_email: String,
    pub locale: String,
}

pub fn resolve_install_options(domain: &str, name: &str, opts: &InstallOptions) -> ResolvedInstall {
    let nonempty = |s: &str| !s.trim().is_empty();
    ResolvedInstall {
        url: format!("https://{domain}"),
        title: if nonempty(&opts.title) { opts.title.trim().into() } else { name.into() },
        admin_user: if nonempty(&opts.admin_user) {
            opts.admin_user.trim().into()
        } else {
            "admin".into()
        },
        admin_password: if nonempty(&opts.admin_password) {
            opts.admin_password.clone()
        } else {
            DEFAULT_ADMIN.into() // local-dev default, consistent with reset_site
        },
        admin_email: if nonempty(&opts.admin_email) {
            opts.admin_email.trim().into()
        } else {
            format!("admin@{domain}")
        },
        locale: opts.language.trim().into(),
    }
}

/// Multisite constants `wp core multisite-convert` writes into wp-config.php.
/// A reset must clear them: a wp-config that still defines `MULTISITE` over a
/// fresh single-site database is a fatal config error on every request.
const MULTISITE_CONSTANTS: &[&str] = &[
    "WP_ALLOW_MULTISITE",
    "MULTISITE",
    "SUBDOMAIN_INSTALL",
    "DOMAIN_CURRENT_SITE",
    "PATH_CURRENT_SITE",
    "SITE_ID_CURRENT_SITE",
    "BLOG_ID_CURRENT_SITE",
];

/// Default local-dev admin credentials (`admin` / `admin`) — used by the
/// site reset and the New-site fallback. A deliberate convenience for a
/// LOCAL-only site; the tunnels UI warns when a site still accepting these is
/// shared publicly (see [`default_creds_active`]).
pub const DEFAULT_ADMIN: &str = "admin";

/// Reset a WordPress site to a clean **single-site** install: DROP the
/// database, clear any multisite constants from wp-config.php, and re-run the
/// step-skipping installer with the default local-dev credentials
/// (admin / admin, `admin@<domain>`). `db_name` is the site's STORED database
/// name (`Site::db_name`), never re-derived from the domain. Files stay on
/// disk — core, wp-config (same DB name/salts), plugins, themes, uploads; only
/// the database is recreated. Re-runnable: each step skips or tolerates
/// already-done work, so a failure partway is fixed by running it again.
#[allow(clippy::too_many_arguments)] // flat per-site tool set, mirrors install_for_site
pub fn reset_site(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    domain: &str,
    site_name: &str,
    db_name: &str,
    db_client: &SqlClient,
    db_port: u16,
) -> Result<()> {
    // 1) Erase: drop the database with the bundled client (PATH-safe).
    super::database::drop_database(db_client, db_port, db_name)?;

    // 2) Clear multisite constants — best effort per constant (`wp config
    //    delete` errors on one that isn't defined, which is the common case).
    let path = format!("--path={}", docroot.display());
    for constant in MULTISITE_CONSTANTS {
        let _ = wp_cli(php_bin, wp_phar, &["config", "delete", constant, &path], None);
    }

    // 3) Fresh install via the existing re-runnable flow: core download and
    //    wp-config creation skip (files kept), the DB is recreated, and
    //    `wp core install` runs because `is-installed` is now false.
    let db_host = format!("127.0.0.1:{db_port}");
    let url = format!("https://{domain}");
    let admin_email = format!("admin@{domain}");
    install_wordpress(
        php_bin,
        wp_phar,
        &WpInstall {
            docroot,
            db_name,
            db_host: &db_host,
            db_client,
            url: &url,
            title: site_name,
            admin_user: DEFAULT_ADMIN,
            admin_password: DEFAULT_ADMIN,
            admin_email: &admin_email,
            locale: "",
        },
    )
}

/// Whether the site still accepts the default local-dev credentials
/// (admin / admin). Backs the tunnel-share warning: locally the default is a
/// convenience, but on a public tunnel URL it's an open wp-admin. Any failure
/// (no `admin` user, broken site, non-WP docroot) reads as `false`.
pub fn default_creds_active(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> bool {
    let path = format!("--path={}", docroot.display());
    wp_cli(
        php_bin,
        wp_phar,
        &["user", "check-password", DEFAULT_ADMIN, DEFAULT_ADMIN, &path],
        None,
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

/// The wp-config.php path for a docroot (used by callers/tests).
pub fn wp_config_path(docroot: &Path) -> PathBuf {
    docroot.join("wp-config.php")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line the Replace offer rests on. wp-cli's wording is a THIRD
    /// PARTY's, so the fixture is its real output (captured 19 Aug 2026) and
    /// `wp_install_stream_check` jobs 5/6 re-measure it against the live tool —
    /// a parser agreeing with a remembered string proves only that memory.
    #[test]
    fn the_blocked_directory_is_read_from_wp_clis_own_line() {
        let real = "Warning: Destination folder already exists. \
                    \"/Users/wpdev/rexenv/Sites/bl.rex/wp-content/plugins/betterlinks-pro/\"";
        assert_eq!(install_blocked_dir(real).as_deref(), Some("betterlinks-pro"));
        // A theme says the same thing about a different tree.
        assert_eq!(
            install_blocked_dir(
                "Warning: Destination folder already exists. \"/srv/wp/wp-content/themes/astra/\""
            )
            .as_deref(),
            Some("astra")
        );
        // Every other line of that run, and of a healthy one — none of which
        // may produce a folder name.
        for other in [
            "Plugin installation failed.",
            "Warning: The '/Users/dev/betterlinks-pro.2.1.1.zip' plugin could not be found.",
            "Error: No plugins installed.",
            "Unpacking the package...",
            "Installing the plugin...",
            "Success: Installed 1 of 1 plugins.",
        ] {
            assert_eq!(install_blocked_dir(other), None, "{other}");
        }
        // Malformed rather than absent: the marker with no quoted path at all.
        assert_eq!(install_blocked_dir("Warning: Destination folder already exists."), None);
    }

    /// The theme row's shape, CAPTURED from a real site (19 Aug 2026, wp-cli
    /// 2.12.0 against `tr.rex`) rather than written by hand: the field list is
    /// explicit in `theme_list`, so what proves it is what WP-CLI actually
    /// answers with. `title` is the theme header's own name — the card is
    /// labelled with it, and a fixture whose title equalled its slug would
    /// render identically whether the label read the title or fell back.
    #[test]
    fn a_theme_row_carries_the_header_name_beside_the_slug() {
        let rows: Vec<WpTheme> = serde_json::from_str(
            r#"[
                {"name":"twentytwentyfive","status":"active","update":"none",
                 "update_version":"","version":"1.5","title":"Twenty Twenty-Five"},
                {"name":"Divi","status":"inactive","update":"none",
                 "update_version":"","version":"5.9.0","title":"Divi"},
                {"name":"custom-child","status":"inactive","update":false,
                 "version":"1.0"}
            ]"#,
        )
        .expect("the captured theme-list shape must parse");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "twentytwentyfive");
        assert_eq!(rows[0].title, "Twenty Twenty-Five", "the human name is what the card shows");
        // A theme whose directory IS its name — the two are allowed to match.
        assert_eq!(rows[1].title, "Divi");
        // No title at all: empty, never a parse failure. The card falls back to
        // the slug, which is the pre-title behaviour rather than a blank label.
        assert!(rows[2].title.is_empty(), "a missing title must read as empty");
        assert_eq!(rows[2].update, "none", "the boolean-update tolerance still holds");
    }

    /// The field list is EXPLICIT for themes (wp-cli's default set has neither
    /// `update_version` nor `title`), so a field dropped from that string is a
    /// column that silently comes back empty — the exact trap the arrow hit
    /// once already.
    #[test]
    fn the_theme_field_list_still_asks_for_every_column_the_ui_reads() {
        let src = include_str!("wordpress.rs");
        let line = src
            .lines()
            .find(|l| l.contains("\"theme\", \"list\""))
            .expect("the theme_list argv moved — this guard is now reading nothing");
        for field in ["name", "status", "update", "update_version", "version", "title"] {
            assert!(line.contains(field), "`{field}` is no longer requested from wp-cli");
        }
    }

    /// A two-plugin `wp plugin update` transcript. Every line is the string
    /// WP-CLI really prints, checked against BOTH ends of the pipeline rather
    /// than remembered (9 Aug 2026):
    ///
    /// - the phrases come from WP core's upgrader string tables
    ///   (`class-wp-upgrader.php` `maintenance_start`/`maintenance_end`/
    ///   `installing_package`, `class-plugin-upgrader.php`
    ///   `downloading_package`/`unpack_package`/`remove_old`/
    ///   `process_success`/`process_failed`), where they carry `&#8230;` and
    ///   a `<span class="code pre">` around the URL;
    /// - the pinned phar (wp-cli 2.12.0) runs each through
    ///   `str_replace('&#8230;','...', strip_tags($string))` before logging,
    ///   which is why the fixture has plain `...` and a bare URL — and it is
    ///   also where `Using cached file '%s'...` comes from (WP-CLI's own line,
    ///   printed INSTEAD of the download one on a cache hit).
    ///
    /// The trailing table and `Success:` line are kept deliberately: they are
    /// the noise the tracker must not mistake for progress.
    const REAL_UPDATE_OUTPUT: &[&str] = &[
        "Enabling Maintenance mode...",
        "Downloading update from https://downloads.wordpress.org/plugin/query-monitor.3.19.0.zip...",
        "Unpacking the update...",
        "Installing the latest version...",
        "Removing the old version of the plugin...",
        "Plugin updated successfully.",
        "Downloading update from https://downloads.wordpress.org/plugin/woocommerce.10.9.0.zip...",
        "Unpacking the update...",
        "Installing the latest version...",
        "Removing the old version of the plugin...",
        "Plugin updated successfully.",
        "Disabling Maintenance mode...",
        "+----------------+-------------+-------------+---------+",
        "| name           | old_version | new_version | status  |",
        "+----------------+-------------+-------------+---------+",
        "| query-monitor  | 3.18.0      | 3.19.0      | Updated |",
        "+----------------+-------------+-------------+---------+",
        "Success: Updated 2 of 2 plugins.",
    ];

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tracker_follows_wp_cli_through_a_two_plugin_update() {
        let ns = names(&["query-monitor", "woocommerce"]);
        let mut t = UpdateTracker::new(UpdateKind::Plugin, &ns);
        let mut seen: Vec<(usize, String, String)> = Vec::new();
        for line in REAL_UPDATE_OUTPUT {
            if t.feed(line) {
                let s = t.snapshot(line);
                seen.push((s.done, s.current, s.phase));
            }
        }
        // Item 1 downloads before anything is banked; item 2 downloads with
        // exactly one banked — the "which plugin am I waiting on" answer.
        assert!(seen.contains(&(0, "query-monitor".into(), "Downloading".into())));
        assert!(seen.contains(&(1, "woocommerce".into(), "Downloading".into())));
        let end = t.snapshot("");
        assert_eq!((end.done, end.total), (2, 2));
        assert_eq!(end.fraction, 1.0);
    }

    #[test]
    fn table_rows_and_noise_never_move_the_bar() {
        let ns = names(&["query-monitor"]);
        let mut t = UpdateTracker::new(UpdateKind::Plugin, &ns);
        for line in ["| query-monitor | 3.18.0 | 3.19.0 | Updated |", "Warning: something", ""] {
            assert!(!t.feed(line), "{line:?} must not count as a phase");
        }
        assert_eq!(t.snapshot("").fraction, 0.0);
    }

    #[test]
    fn a_cache_hit_still_reports_a_phase_and_the_right_plugin() {
        // WP-CLI prints "Using cached file …" INSTEAD of the download line, so
        // a tracker that only knew the download line would sit at "Starting"
        // for the whole run — the exact dead air this exists to remove.
        let ns = names(&["query-monitor"]);
        let mut t = UpdateTracker::new(UpdateKind::Plugin, &ns);
        assert!(t.feed("Using cached file '/Users/x/.wp-cli/cache/plugin/query-monitor-3.19.0.zip'..."));
        let s = t.snapshot("");
        assert_eq!((s.phase.as_str(), s.current.as_str()), ("Downloading (cached)", "query-monitor"));
        assert!(s.fraction > 0.0);
    }

    #[test]
    fn a_failed_item_is_banked_and_named_failed() {
        let ns = names(&["broken", "fine"]);
        let mut t = UpdateTracker::new(UpdateKind::Plugin, &ns);
        assert!(t.feed("Plugin update failed."));
        let s = t.snapshot("");
        // Banked (the run moved on) but NOT reported as updated.
        assert_eq!((s.done, s.phase.as_str(), s.current.as_str()), (1, "Failed", "fine"));
    }

    #[test]
    fn a_premium_download_url_keeps_the_argv_name_instead_of_a_filename() {
        assert_eq!(
            downloaded_slug(
                UpdateKind::Plugin,
                "Downloading update from https://downloads.wordpress.org/plugin/woocommerce.10.9.0.zip..."
            ),
            Some("woocommerce".into())
        );
        // Not a wp.org /plugin/ URL → no slug to trust.
        assert_eq!(
            downloaded_slug(UpdateKind::Plugin, "Downloading update from https://example.com/dl?token=abc123..."),
            None
        );
        let ns = names(&["elementor-pro"]);
        let mut t = UpdateTracker::new(UpdateKind::Plugin, &ns);
        t.feed("Downloading update from https://my.elementor.com/download/xyz.zip...");
        assert_eq!(t.snapshot("").current, "elementor-pro");
    }

    #[test]
    fn a_translation_pass_inside_an_update_is_a_step_and_never_a_finished_item() {
        // WP updates language packs INSIDE a plugin/theme update
        // (`class-language-pack-upgrader.php` — "Some of your translations
        // need updating…"), and its success line reads "Translation updated
        // successfully." Counting that as an item banked one plugin per
        // translation pass: the bar ran ahead of the work and could hit 100%
        // with plugins still updating.
        let ns = names(&["query-monitor", "woocommerce"]);
        let mut t = UpdateTracker::new(UpdateKind::Plugin, &ns);
        assert!(t.feed("Some of your translations need updating. Sit tight for a few more seconds while they are updated as well."));
        assert!(t.feed("Translation updated successfully."));
        let s = t.snapshot("");
        assert_eq!(s.done, 0, "a translation pass is not a plugin finishing");
        assert_eq!(s.phase, "Updating translations");
        // The real item completion still banks.
        assert!(t.feed("Plugin updated successfully."));
        assert_eq!(t.snapshot("").done, 1);
    }

    /// `wp theme update` — the same upgrader with the theme string table
    /// (`class-theme-upgrader.php`: `remove_old` says "of the theme",
    /// `process_success` "Theme updated successfully.").
    #[test]
    fn tracker_follows_a_theme_update_and_reads_the_theme_url_segment() {
        let ns = names(&["twentytwentyfour"]);
        let mut t = UpdateTracker::new(UpdateKind::Theme, &ns);
        for line in [
            "Enabling Maintenance mode...",
            "Downloading update from https://downloads.wordpress.org/theme/twentytwentyfour.1.3.zip...",
            "Unpacking the update...",
            "Installing the latest version...",
            "Removing the old version of the theme...",
        ] {
            assert!(t.feed(line), "{line:?} should be a phase");
        }
        assert_eq!(t.snapshot("").current, "twentytwentyfour");
        // A PLUGIN tracker must not settle on the theme's line, and vice
        // versa — the nouns are the whole reason `kind` exists.
        assert!(t.feed("Theme updated successfully."));
        assert_eq!(t.snapshot("").done, 1);
        let mut p = UpdateTracker::new(UpdateKind::Plugin, &ns);
        p.feed("Theme updated successfully.");
        assert_eq!(p.snapshot("").done, 0);
    }

    /// `wp core update` — wp-cli's own lines around WP's upgrader (verified in
    /// the pinned 2.12.0 phar: "Updating to version %s (%s)...",
    /// "Cleaning up files...", "No files found that need cleaning up.",
    /// "Success: WordPress updated successfully.").
    #[test]
    fn tracker_follows_a_core_update_as_a_single_item() {
        let mut t = UpdateTracker::new(UpdateKind::Core, &[]);
        assert_eq!(t.snapshot("").current, "WordPress", "core names itself");
        for line in [
            "Updating to version 6.8.2 (en_US)...",
            "Downloading update from https://downloads.wordpress.org/release/wordpress-6.8.2-no-content.zip...",
            "Unpacking the update...",
            "Cleaning up files...",
            "No files found that need cleaning up.",
        ] {
            assert!(t.feed(line), "{line:?} should be a phase");
        }
        // Core's archive is named for a VERSION, not a slug — nothing in that
        // URL may be mistaken for one.
        assert_eq!(t.snapshot("").current, "WordPress");
        assert!(t.feed("Success: WordPress updated successfully."));
        let s = t.snapshot("");
        assert_eq!((s.done, s.total, s.fraction), (1, 1, 1.0));
    }

    #[test]
    fn a_core_update_that_errors_ends_as_failed_not_as_done() {
        let mut t = UpdateTracker::new(UpdateKind::Core, &[]);
        assert!(t.feed("Error: Download failed. cURL error 6: Could not resolve host"));
        assert_eq!(t.snapshot("").phase, "Failed");
    }

    #[test]
    fn db_name_sanitizes_domain() {
        assert_eq!(db_name_for(SiteType::Wordpress, "blog.test"), "wp_blog_test");
        assert_eq!(db_name_for(SiteType::Wordpress, "my-site.test"), "wp_my_site_test");
        assert_eq!(db_name_for(SiteType::Wordpress, "a.b.c.test"), "wp_a_b_c_test");
    }

    #[test]
    fn db_name_prefix_is_per_site_type_and_never_wp_for_a_non_wp_site() {
        // The bug: every type derived `wp_`, so a Laravel app owned a database
        // labelled WordPress. The prefix is the type's, and no other type may
        // reuse WordPress's — checked over the WHOLE enum, so a type added
        // later cannot quietly inherit `wp_` (the shape of the original bug).
        assert_eq!(db_name_for(SiteType::Laravel, "myapp.test"), "lv_myapp_test");
        assert_eq!(db_name_for(SiteType::Php, "myapp.test"), "php_myapp_test");
        for t in [SiteType::Wordpress, SiteType::Laravel, SiteType::Php] {
            let p = db_name_prefix(t);
            assert!(p.ends_with('_') && p.len() <= 4, "prefix stays short: {p}");
            assert_eq!(t == SiteType::Wordpress, p == "wp_", "only WP owns wp_: {p}");
        }
        // Distinct prefixes are what keep a same-domain pair apart, so the
        // names must actually differ (not merely the prefixes).
        assert_ne!(
            db_name_for(SiteType::Laravel, "x.test"),
            db_name_for(SiteType::Wordpress, "x.test")
        );
    }

    #[test]
    fn db_name_disambiguated_is_injective_and_bounded() {
        // The whole point: two domains that `db_name_for` reduces to the SAME
        // slug get DISTINCT disambiguated names (the domain hash differs).
        let a = db_name_disambiguated(SiteType::Wordpress, "my-shop.test");
        let b = db_name_disambiguated(SiteType::Wordpress, "my.shop.test");
        assert_ne!(a, b, "slug-colliding domains must not share a database");
        assert!(a.starts_with("wp_my_shop_test_"), "{a}");
        assert!(b.starts_with("wp_my_shop_test_"), "{b}");
        // Deterministic (stored once at create; must be stable within a build).
        assert_eq!(a, db_name_disambiguated(SiteType::Wordpress, "my-shop.test"));
        // Always within MySQL's 64-char identifier limit, even for a long domain.
        let long = format!("{}.test", "a".repeat(250));
        let d = db_name_disambiguated(SiteType::Wordpress, &long);
        assert!(d.len() <= DB_NAME_MAX, "must fit the 64-char limit, got {}", d.len());
        // Only valid identifier characters ([a-z0-9_]).
        assert!(
            a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "valid identifier chars only: {a}"
        );
    }

    #[test]
    fn valid_slug_accepts_real_slugs_and_rejects_argv_and_source_smuggling() {
        for ok in ["akismet", "wp-super-cache", "woocommerce", "jetpack", "classic-editor", "2fa"] {
            assert!(valid_slug(ok), "{ok}");
        }
        for bad in [
            "", "--all", "-akismet", "Akismet", "my_plugin", "a b",
            "https://evil.example/x.zip", "/tmp/x.zip", "../x", "evil.zip", "a.b",
        ] {
            assert!(!valid_slug(bad), "{bad:?}");
        }
    }

    #[test]
    fn install_slug_guard_refuses_a_url_or_flag_slug() {
        // The wp.org-slugs-only guard every install path runs BEFORE any
        // wp-cli spawn (wp_install job + blueprint apply both call it first —
        // structural: it's their first statement per item).
        let refuse = |slug: &str| ensure_slugs("plugin", &[slug.to_string()]).unwrap_err().to_string();
        assert!(refuse("https://evil.example/x.zip").contains("invalid plugin slug"));
        assert!(refuse("--activate").contains("invalid plugin slug"));
        assert!(ensure_slugs("plugin", &["akismet".to_string()]).is_ok());
    }

    #[test]
    fn zip_source_takes_only_an_absolute_existing_zip_file() {
        // The "Upload zip" gate. Every rejection here is a case wp-cli would
        // otherwise turn into a wp.org SLUG lookup (it only treats a
        // `pathinfo == zip` argument as a local archive) — i.e. the wrong
        // failure message for the wrong reason, which is why the gate exists
        // at all rather than letting wp-cli sort it out.
        let dir = std::env::temp_dir().join(format!("rexenv-zipgate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("my-plugin.zip");
        std::fs::write(&zip, b"PK\x03\x04").unwrap();
        let upper = dir.join("Other-Plugin.ZIP"); // an export from a Mac zips as .ZIP
        std::fs::write(&upper, b"PK\x03\x04").unwrap();
        let tarball = dir.join("my-plugin.tar.gz");
        std::fs::write(&tarball, b"\x1f\x8b").unwrap();
        let subdir = dir.join("unpacked.zip"); // a DIRECTORY that ends in .zip
        std::fs::create_dir_all(&subdir).unwrap();

        let ok = |p: &std::path::Path| ensure_zip_paths("plugin", &[p.display().to_string()]);
        assert!(ok(&zip).is_ok());
        assert!(ok(&upper).is_ok(), "the extension check is case-insensitive");
        assert!(ensure_zip_paths(
            "plugin",
            &[zip.display().to_string(), upper.display().to_string()]
        )
        .is_ok());

        let err = |p: String| ensure_zip_paths("theme", &[p]).unwrap_err().to_string();
        assert!(err("my-plugin.zip".into()).contains("absolute"), "relative path");
        assert!(err("--activate".into()).contains("absolute"), "a flag is not absolute");
        assert!(err(tarball.display().to_string()).contains("not a .zip file"));
        assert!(err(dir.join("gone.zip").display().to_string()).contains("no such file"));
        assert!(err(subdir.display().to_string()).contains("no such file"), "a dir is not a file");
        // One bad entry fails the whole batch — no partial spawn.
        assert!(ensure_zip_paths("plugin", &[zip.display().to_string(), "x".into()]).is_err());

        // Fixture-owned cleanup only (the dir this test created, by name).
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wp_positional_argv_has_no_stray_end_of_flags_separator() {
        // Regression: B24 inserted a `--` "end-of-flags" token, but WP-CLI does
        // NOT honor getopt `--` — it read the bare `--` as a literal positional,
        // so `plugin activate` saw a phantom "--" slug and `cron event run` saw a
        // "--" hook. The argv must carry the real positionals ONLY, no `--`.
        let one = ["akismet".to_string()];
        assert_eq!(
            item_verb_argv("plugin", "activate", &one),
            ["plugin", "activate", "akismet"],
            "no stray -- before the plugin slug"
        );
        let two = ["akismet".to_string(), "jetpack".to_string()];
        assert_eq!(
            item_verb_argv("plugin", "activate", &two),
            ["plugin", "activate", "akismet", "jetpack"],
            "multiple slugs, still no --"
        );
        assert_eq!(
            cron_run_hook_argv("my_cron_hook"),
            ["cron", "event", "run", "my_cron_hook"],
            "no stray -- as the cron hook name"
        );
        // Guard against the token creeping back into either builder.
        assert!(!item_verb_argv("plugin", "activate", &one).contains(&"--"));
        assert!(!cron_run_hook_argv("my_cron_hook").contains(&"--"));
    }

    #[test]
    fn checksum_findings_split_real_from_os_noise() {
        let out = "\
Warning: File doesn't verify against checksum: wp-includes/version.php
Warning: File should not exist: wp-admin/.DS_Store
Warning: File should not exist: wp-includes/._blocks
Warning: File should not exist: wp-admin/backdoor.php
Warning: File doesn't exist: wp-includes/functions.php
Warning: something new wp-cli might say: mystery.php
Error: WordPress installation doesn't verify against checksums.";
        let (real, benign) = classify_checksum_output(out);
        // Benign: basename-matched OS clutter among "should not exist" only.
        assert_eq!(benign, vec!["wp-admin/.DS_Store", "wp-includes/._blocks"]);
        // Real: modified, non-noise extra, missing, and the unknown warning.
        assert_eq!(real.len(), 4, "real: {real:?}");
        assert!(real.iter().any(|r| r.contains("version.php")));
        assert!(real.iter().any(|r| r.contains("backdoor.php")));
        assert!(real.iter().any(|r| r.contains("functions.php")));
        assert!(real.iter().any(|r| r.contains("mystery.php")));
        // A noise-looking name buried in a real filename stays real.
        let (real2, benign2) =
            classify_checksum_output("Warning: File should not exist: x/.DS_Store-backdoor.php");
        assert!(benign2.is_empty() && real2.len() == 1);
        // Modified/missing core files are NEVER benign, even with noise names.
        let (real3, benign3) = classify_checksum_output(
            "Warning: File doesn't verify against checksum: wp-admin/.DS_Store",
        );
        assert!(benign3.is_empty() && real3.len() == 1);
        // B30: an AppleDouble `._x` sidecar is benign, but an executable
        // `._x.php` is REAL (PHP would run it) — it must not hide in noise.
        let (real4, benign4) = classify_checksum_output(
            "Warning: File should not exist: wp-includes/._evil.php\n\
             Warning: File should not exist: wp-includes/._blocks",
        );
        assert_eq!(benign4, vec!["wp-includes/._blocks"], "the non-php sidecar stays benign");
        assert_eq!(real4.len(), 1, "the ._*.php is flagged as real: {real4:?}");
        assert!(real4[0].contains("._evil.php"), "{real4:?}");
    }

    #[test]
    fn role_whitelist_is_the_stock_five() {
        for ok in ["administrator", "editor", "author", "contributor", "subscriber"] {
            assert!(USER_ROLES.contains(&ok), "{ok} should be assignable");
        }
        for bad in ["super-admin", "Administrator", "", "none", "custom_role"] {
            assert!(!USER_ROLES.contains(&bad), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn permalink_whitelist_covers_presets_and_only_presets() {
        assert!(PERMALINK_STRUCTURES.contains(&""), "Plain must be offered");
        assert!(PERMALINK_STRUCTURES.contains(&"/%postname%/"));
        // Anything else is rejected before reaching wp-cli.
        for bad in ["/custom/%postname%/", "%postname%", "/index.php/%postname%/"] {
            assert!(!PERMALINK_STRUCTURES.contains(&bad), "{bad:?} should not pass");
        }
    }

    #[test]
    fn debug_flag_whitelist_blocks_arbitrary_constants() {
        for ok in DEBUG_FLAGS {
            assert!(ensure_debug_flag(ok).is_ok(), "{ok} should be allowed");
        }
        // Free-form names would reach wp-cli argv — config injection.
        for bad in ["WP_DEBUG", "DISALLOW_FILE_MODS", "X; rm -rf", "", "wp_debug_log"] {
            assert!(ensure_debug_flag(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn plugin_rows_parse_boolean_update() {
        // Verbatim `wp plugin list --format=json` from a live rexenv site: the
        // must-use rexenv-login row reports `update` as a BOOLEAN — a strict
        // String field made the entire plugin list fail to deserialize.
        let json = r#"[
            {"name":"akismet","status":"inactive","update":"none","version":"5.7"},
            {"name":"rexenv-login","status":"must-use","update":false,"version":""},
            {"name":"other-mu","status":"must-use","update":true,"version":""}
        ]"#;
        let rows: Vec<WpPlugin> = serde_json::from_str(json).unwrap();
        assert_eq!(rows[0].update, "none");
        assert_eq!(rows[1].update, "none");
        assert_eq!(rows[2].update, "available");
    }

    #[test]
    fn language_rows_parse_captured_wp_cli_output() {
        // Verbatim rows from `wp language core list --format=json` (wp-cli 2.12,
        // WP 7.0): snake_case keys, en_US with empty `updated`.
        let json = r#"[
            {"language":"en_US","english_name":"English (United States)","native_name":"English (United States)","status":"active","update":"none","updated":""},
            {"language":"fr_FR","english_name":"French (France)","native_name":"Français","status":"uninstalled","update":"none","updated":"2026-06-17 09:57:00"}
        ]"#;
        let rows: Vec<WpLanguage> = serde_json::from_str(json).unwrap();
        assert_eq!(rows[0].language, "en_US");
        assert_eq!(rows[0].status, "active");
        assert_eq!(rows[1].english_name, "French (France)");
        assert_eq!(rows[1].native_name, "Français");
        // Serializes camelCase for the frontend.
        let out = serde_json::to_string(&rows[1]).unwrap();
        assert!(out.contains("\"englishName\""), "{out}");
    }

    #[test]
    fn valid_locale_accepts_real_locales_and_rejects_argv_smuggling() {
        for ok in ["fr_FR", "de_DE_formal", "pt_BR", "ceb", "zh_CN"] {
            assert!(valid_locale(ok), "{ok}");
        }
        for bad in ["", "e", "--skip-plugins", "-f", "fr_FR; rm -rf /", "fr FR", "FR_fr", "../x", "fr\u{2013}FR"] {
            assert!(!valid_locale(bad), "{bad:?}");
        }
    }

    #[test]
    fn wp_version_shape_accepts_releases_and_rejects_argv_smuggling() {
        for ok in ["7.0", "7.0.1", "6.8.5"] {
            assert!(parse_wp_version(ok).is_some(), "{ok}");
        }
        for bad in ["", "nightly", "6", "6.8.5.1", "+6.8", "-6.8", "--force", "6..8", "6.8 ", "6.8.5; rm -rf /"] {
            assert!(parse_wp_version(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn stable_check_parses_filters_and_sorts_newest_first() {
        // Shape verbatim from api.wordpress.org/core/stable-check/1.0/.
        let body = br#"{"1.0.2":"insecure","5.9.3":"insecure","6.5.2":"insecure","6.9.4":"outdated","7.0":"outdated","7.0.1":"latest"}"#;
        let rows = parse_stable_check(body).unwrap();
        let versions: Vec<&str> = rows.iter().map(|r| r.version.as_str()).collect();
        // < 6.0 filtered out; newest first ("7.0" sorts as 7.0.0 below 7.0.1).
        assert_eq!(versions, ["7.0.1", "7.0", "6.9.4", "6.5.2"]);
        assert_eq!(rows[0].status, "latest");
        assert_eq!(rows[3].status, "insecure");
    }

    #[test]
    fn core_switch_rejects_bad_versions_before_any_wp_call() {
        // Nonexistent binaries — reaching wp-cli would yield an io error, not
        // these messages, proving both guards fire first.
        let allowed = vec!["7.0.1".to_string(), "6.9.4".to_string()];
        let run = |v: &str| {
            core_switch_version(
                Path::new("/nonexistent/php"),
                Path::new("/nonexistent/wp.phar"),
                Path::new("/nonexistent/docroot"),
                v,
                &allowed,
            )
            .unwrap_err()
            .to_string()
        };
        assert!(run("--force").contains("invalid version"));
        assert!(run("6.8.5.1").contains("invalid version"));
        // Well-shaped but not a real release (not in the fresh stable-check list).
        assert!(run("6.9.9").contains("not a known WordPress release"));
    }

    #[test]
    fn option_whitelist_makes_dangerous_options_unreachable() {
        // Editable: on the list.
        assert!(option_field("blogname").is_some());
        assert!(option_field("posts_per_page").is_some());
        // The foot-guns are not "blocked" — they simply don't exist here.
        for dangerous in ["siteurl", "home", "active_plugins", "template", "stylesheet", "db_version", "WPLANG", ""] {
            assert!(option_field(dangerous).is_none(), "{dangerous} must not be editable");
        }
        // option_update refuses BEFORE any wp-cli call: nonexistent binaries
        // would yield an io error, not this message.
        let e = option_update(
            Path::new("/nonexistent/php"),
            Path::new("/nonexistent/wp.phar"),
            Path::new("/nonexistent/docroot"),
            "siteurl",
            "https://evil.example",
        )
        .unwrap_err();
        assert!(e.to_string().contains("not an editable option"), "{e}");
    }

    #[test]
    fn option_values_validate_per_kind_in_the_backend() {
        let tz = vec!["Europe/Paris".to_string()];
        let roles = vec!["subscriber".to_string(), "shop_manager".to_string()];
        let v = |kind, val: &str| validate_option_value(kind, val, &tz, &roles);

        // posts_per_page: 1..=1000.
        for bad in ["0", "-1", "1001", "abc", "", "10.5"] {
            assert!(v(OptionKind::IntRange(1, 1000), bad).is_err(), "{bad:?}");
        }
        assert!(v(OptionKind::IntRange(1, 1000), "1").is_ok());
        assert!(v(OptionKind::IntRange(1, 1000), "1000").is_ok());
        // Email shape.
        for bad in ["", "nope", "@x.com", "a@b", "a b@c.d", "a@.com", "a@com."] {
            assert!(v(OptionKind::Email, bad).is_err(), "{bad:?}");
        }
        assert!(v(OptionKind::Email, "admin@site.test").is_ok());
        // Toggles are the two WP strings only.
        assert!(v(OptionKind::Bool, "2").is_err());
        assert!(v(OptionKind::Bool, "true").is_err());
        assert!(v(OptionKind::Bool, "1").is_ok());
        // Weekday 0..=6.
        assert!(v(OptionKind::Weekday, "7").is_err());
        assert!(v(OptionKind::Weekday, "0").is_ok());
        // Timezone: from the list, or empty (gmt_offset mode — seen live).
        assert!(v(OptionKind::Timezone, "Mars/Olympus").is_err());
        assert!(v(OptionKind::Timezone, "Europe/Paris").is_ok());
        assert!(v(OptionKind::Timezone, "").is_ok());
        // Role: live list incl. custom roles.
        assert!(v(OptionKind::Role, "administrator2").is_err());
        assert!(v(OptionKind::Role, "shop_manager").is_ok());
    }

    #[test]
    fn scalar_display_refuses_everything_serialized() {
        use serde_json::json;
        assert_eq!(scalar_display(&json!("Think Rank")).as_deref(), Some("Think Rank"));
        assert_eq!(scalar_display(&json!(10)).as_deref(), Some("10"));
        assert_eq!(scalar_display(&json!(false)).as_deref(), Some("0"));
        // Arrays/objects (unserialized PHP data) and null: never editable.
        assert_eq!(scalar_display(&json!(["a.php", "b.php"])), None);
        assert_eq!(scalar_display(&json!({"k": "v"})), None);
        assert_eq!(scalar_display(&serde_json::Value::Null), None);
    }

    #[test]
    fn options_eval_script_contains_exactly_the_whitelist() {
        let s = options_eval_script();
        for f in OPTION_FIELDS {
            assert!(s.contains(&format!("\"{}\"", f.name)), "{} missing", f.name);
        }
        assert!(!s.contains("siteurl") && !s.contains("active_plugins"));
    }

    #[test]
    fn cleanup_os_noise_deletes_only_validated_noise_files() {
        let root = std::env::temp_dir().join(format!("rexenv-noise-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("rexenv-noise-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(root.join("wp-admin/css")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        // Legit noise (nested) — deleted.
        std::fs::write(root.join(".DS_Store"), "x").unwrap();
        std::fs::write(root.join("wp-admin/css/.DS_Store"), "x").unwrap();
        std::fs::write(root.join("wp-admin/._resource"), "x").unwrap();
        // Non-noise basename — refused even though the UI sent it.
        std::fs::write(root.join("wp-admin/evil.php"), "x").unwrap();
        // Victims for the symlink cases.
        std::fs::write(outside.join(".DS_Store"), "outside-victim").unwrap();
        std::fs::write(root.join("wp-config.php"), "inside-victim").unwrap();
        // Symlink named like noise → OUTSIDE file: must not be followed.
        std::os::unix::fs::symlink(outside.join(".DS_Store"), root.join("wp-admin/.DS_Store"))
            .unwrap();
        // Symlink named like noise → INSIDE non-noise file: the canonical path
        // passes the prefix check — only the lstat guard saves wp-config.php.
        std::os::unix::fs::symlink(root.join("wp-config.php"), root.join("wp-admin/css/._cfg"))
            .unwrap();
        // Directory named like noise — files only.
        std::fs::create_dir(root.join(".Trashes")).unwrap();

        let paths: Vec<String> = [
            ".DS_Store",
            "wp-admin/css/.DS_Store",
            "wp-admin/._resource",
            "wp-admin/evil.php",             // guard 1: basename
            "../escape/.DS_Store",           // guard 2: `..`
            "/tmp/.DS_Store",                // guard 2: absolute
            "wp-admin/.DS_Store",            // guard 3: symlink → outside
            "wp-admin/css/._cfg",            // guard 3: symlink → inside victim
            ".Trashes",                      // guard 3: directory
            "wp-includes/.DS_Store",         // vanished: skip, not abort
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let res = cleanup_os_noise(&root, &paths).unwrap();

        assert_eq!(res.removed, 3, "skipped: {:?}", res.skipped);
        assert_eq!(res.skipped.len(), 7);
        // The real noise is gone…
        assert!(!root.join(".DS_Store").exists());
        assert!(!root.join("wp-admin/css/.DS_Store").exists());
        assert!(!root.join("wp-admin/._resource").exists());
        // …every victim/refusal survives.
        assert_eq!(std::fs::read_to_string(outside.join(".DS_Store")).unwrap(), "outside-victim");
        assert_eq!(std::fs::read_to_string(root.join("wp-config.php")).unwrap(), "inside-victim");
        assert!(root.join("wp-admin/evil.php").exists());
        assert!(root.join(".Trashes").is_dir());
        // Reasons name the guard, not a generic failure.
        let reason = |p: &str| {
            res.skipped.iter().find(|s| s.path == p).map(|s| s.reason.clone()).unwrap_or_default()
        };
        assert!(reason("wp-admin/evil.php").contains("not a known macOS system file"));
        assert!(reason("../escape/.DS_Store").contains("escapes"));
        assert!(reason("/tmp/.DS_Store").contains("escapes"));
        assert!(reason("wp-admin/.DS_Store").contains("symbolic link"));
        assert!(reason("wp-admin/css/._cfg").contains("symbolic link"));
        assert!(reason(".Trashes").contains("not a regular file"));

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn run_with_timeout_kills_a_stalled_child_fast() {
        // Simulates the offline language download: a child that would sit for
        // 30s (WP's download_url waits 300s) must be killed at the cap, not
        // waited out — the UI spinner rides on this returning.
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "sleep 30"]);
        let start = Instant::now();
        let e = run_with_timeout(cmd, Duration::from_millis(400), "sleep-test").unwrap_err();
        assert!(e.to_string().contains("timed out after 0s"), "{e}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "took {:?} — child not killed at the cap",
            start.elapsed()
        );
    }

    #[test]
    fn run_with_timeout_returns_output_of_a_fast_child() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo out; echo err 1>&2"]);
        let out = run_with_timeout(cmd, Duration::from_secs(10), "echo-test").unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "out");
        assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "err");
    }

    #[test]
    fn run_with_timeout_drains_a_chatty_child_without_a_fake_timeout() {
        // A child that writes far past the ~64KB pipe buffer before exiting.
        // The old wait-then-read shape deadlocked here (child blocked writing,
        // try_wait never Some) and reported a FAKE timeout — a long
        // `plugin list --format=json` must never trip the guard by being long.
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "head -c 300000 /dev/zero | tr '\\0' 'x'; echo done"]);
        let out = run_with_timeout(cmd, Duration::from_secs(10), "chatty-test").unwrap();
        assert!(out.status.success());
        assert!(out.stdout.len() > 300_000, "full output drained: {}", out.stdout.len());
    }

    #[test]
    fn download_timeout_scales_with_item_count_and_stays_loose() {
        // base 120s + 900s/item — 900 ≥ 1.5× WP's verified 300s per-download
        // internal bound (download_url, file.php) and dominates core's ~600s,
        // so a legit slow download always fails INSIDE wp-cli first; only a
        // wedge outside wp-cli's own bounds can reach this cap.
        assert_eq!(download_timeout(1), Duration::from_secs(1020));
        assert_eq!(download_timeout(5), Duration::from_secs(4620));
        // List calls: WP's update-check request is internally capped at
        // 3s/30s (update.php), so 300s is ~10× the worst legit case.
        assert_eq!(WP_LIST_TIMEOUT, Duration::from_secs(300));
    }

    #[test]
    fn install_line_semantics_match_the_pinned_phar_literals() {
        // Per-item header: wp-cli literal, WITH the " (" that excludes
        // WP-core's phase lines.
        assert!(is_install_item_header("Installing bbPress (2.5.9)"));
        assert!(is_install_item_header("Installing Twenty Sixteen (1.2)"));
        assert!(!is_install_item_header("Installing the plugin..."));
        assert!(!is_install_item_header("Installing the theme..."));
        assert!(!is_install_item_header("Downloading installation package from https://x..."));

        let tail = |lines: &[&str]| lines.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        // Terminal truth = last Success:/Error: line, verbatim.
        let t = tail(&[
            "Plugin installed successfully.",
            "Success: Installed 2 of 2 plugins.",
        ]);
        assert_eq!(install_summary_line(&t).as_deref(), Some("Success: Installed 2 of 2 plugins."));

        // All-success → ok (warnings never touch the exit code).
        assert_eq!(classify_install_exit(true, &t), "ok");
        // Partial batch: exit 1 but SOME slugs installed — must read
        // "partial", never flattened to "failed".
        let t = tail(&["Warning: akismet: latest already…", "Error: Only installed 1 of 2 plugins."]);
        assert_eq!(classify_install_exit(false, &t), "partial");
        assert_eq!(
            install_summary_line(&t).as_deref(),
            Some("Error: Only installed 1 of 2 plugins.")
        );
        // Total failure → failed.
        let t = tail(&["Error: No plugins installed."]);
        assert_eq!(classify_install_exit(false, &t), "failed");
    }

    /// Feed a sequence, return the pct after each line.
    fn observe_all(p: &mut InstallProgress, lines: &[&str]) -> Vec<u8> {
        lines.iter().map(|l| p.observe(l)).collect()
    }

    #[test]
    fn install_progress_full_sequence_hits_the_table_values() {
        // N=1, no --activate: 4 slices of 25.
        let mut p = InstallProgress::new(1, false);
        let pcts = observe_all(
            &mut p,
            &[
                "Installing Hello Dolly (1.7.2)",
                "Downloading installation package from https://x...",
                "Unpacking the package...",
                "Installing the plugin...",
                "Plugin installed successfully.", // raw 100 → capped: work continues
                "Success: Installed 1 of 1 plugins.",
            ],
        );
        assert_eq!(pcts, vec![0, 25, 50, 75, 99, 100]);

        // N=1, --activate: 5 slices of 20; activation slice fills from the
        // "Activating" line (wp-cli literal).
        let mut p = InstallProgress::new(1, true);
        let pcts = observe_all(
            &mut p,
            &[
                "Installing bbPress (2.5.9)",
                "Downloading installation package from https://x...",
                "Unpacking the package...",
                "Installing the plugin...",
                "Plugin installed successfully.",
                "Activating 'bbpress'...",
                "Plugin 'bbpress' activated.",
                "Success: Installed 1 of 1 plugins.",
            ],
        );
        assert_eq!(pcts, vec![0, 20, 40, 60, 80, 99, 99, 100]);
    }

    #[test]
    fn install_progress_cached_file_fills_the_download_slice() {
        // "Using cached file" REPLACES "Downloading" — same slice, no stall.
        let mut p = InstallProgress::new(1, false);
        assert_eq!(p.observe("Installing Hello Dolly (1.7.2)"), 0);
        assert_eq!(p.observe("Using cached file '/Users/x/.wp-cli/cache/…'..."), 25);
    }

    #[test]
    fn install_progress_missing_lines_jump_forward_never_stall() {
        // A later milestone implies the earlier ones: header → straight to
        // "installed successfully" fills all four slices.
        let mut p = InstallProgress::new(1, false);
        assert_eq!(p.observe("Installing Hello Dolly (1.7.2)"), 0);
        assert_eq!(p.observe("Plugin installed successfully."), 99);

        // A new item header implies the previous item is COMPLETE, even if
        // its last milestones were never seen (e.g. activation output miss).
        let mut p = InstallProgress::new(2, true);
        p.observe("Installing A (1.0)");
        assert_eq!(p.observe("Unpacking the package..."), 20); // (0 + 2/5)/2
        assert_eq!(p.observe("Installing B (2.0)"), 50); // item 1 done by implication
    }

    #[test]
    fn install_progress_headerless_already_installed_slug_cannot_stall() {
        // An already-installed slug prints NO header and NO phase lines —
        // just a Warning — yet counts as a summary success. The bar simply
        // stays put (behind reality, never ahead) until the next real signal.
        let mut p = InstallProgress::new(2, false);
        assert_eq!(p.observe("Warning: akismet: Plugin already installed."), 0);
        assert_eq!(p.observe("Installing bbPress (2.5.9)"), 0);
        assert_eq!(p.observe("Plugin installed successfully."), 50); // item "1" of 2 done
        assert_eq!(p.observe("Success: Installed 2 of 2 plugins."), 100);

        // ALL slugs already installed: zero observed progress, then done —
        // honest (nothing was watched happening), the silence ticker covers
        // the "not frozen" signal.
        let mut p = InstallProgress::new(1, false);
        assert_eq!(p.observe("Warning: akismet: Plugin already installed."), 0);
        assert_eq!(p.observe("Success: Plugin already installed."), 100);
    }

    #[test]
    fn install_progress_without_activate_reserves_no_unfillable_slice() {
        // No --activate → 4 slices: the bar reaches 99 with no Activating
        // line ever printed (the slice that can't fill doesn't exist).
        let mut p = InstallProgress::new(1, false);
        for l in [
            "Installing A (1.0)",
            "Downloading installation package from https://x...",
            "Unpacking the package...",
            "Installing the plugin...",
        ] {
            p.observe(l);
        }
        assert_eq!(p.observe("Plugin installed successfully."), 99);
    }

    #[test]
    fn install_progress_is_monotonic_under_repeats_and_disorder() {
        let mut p = InstallProgress::new(2, false);
        let mut last = 0;
        for l in [
            "Installing A (1.0)",
            "Installing the plugin...", // out of order: jumps to slice 3
            "Downloading installation package from https://x...", // earlier slice — no move back
            "Unpacking the package...",
            "Plugin installed successfully.",
            "Installing B (2.0)",
            "Downloading installation package from https://x...",
            "Downloading installation package from https://x...", // repeat
            "Unpacking the package...",
        ] {
            let pct = p.observe(l);
            assert!(pct >= last, "{l}: {pct} < {last}");
            last = pct;
        }
    }

    #[test]
    fn install_progress_never_reads_done_before_the_terminal_summary() {
        // Everything short of the summary caps at 99…
        let mut p = InstallProgress::new(1, true);
        for l in [
            "Installing A (1.0)",
            "Using cached file '/x'...",
            "Unpacking the package...",
            "Installing the plugin...",
            "Plugin installed successfully.",
            "Activating 'a'...",
            "Plugin 'a' activated.",
        ] {
            assert!(p.observe(l) <= 99, "{l}");
        }
        // …including the mid-batch Success: line CHAINED THEME ACTIVATION
        // prints (unguarded in the 2.12.0 phar) — it fills the activation
        // slice, it is NOT the terminal summary.
        let mut p = InstallProgress::new(2, true);
        p.observe("Installing Twenty Sixteen (3.2)");
        p.observe("Theme installed successfully.");
        assert!(p.observe("Success: Switched to 'Twenty Sixteen' theme.") <= 99);

        // Failure summaries advance NOTHING — the bar stops where it is.
        let mut p = InstallProgress::new(2, false);
        p.observe("Installing A (1.0)");
        p.observe("Plugin installed successfully.");
        p.observe("Installing B (2.0)");
        let frozen = p.observe("Downloading installation package from https://x...");
        assert_eq!(p.observe("Warning: b: Plugin not found."), frozen);
        assert_eq!(p.observe("Error: Only installed 1 of 2 plugins."), frozen);
    }

    #[test]
    fn item_verb_update_gets_the_scaled_download_bound_local_verbs_stay_unbounded() {
        // plugin/theme `update` downloads an archive per item (same wedge class
        // as install) → the SCALED download cap, not the flat list bound.
        assert_eq!(item_verb_timeout("update", 3), Some(download_timeout(3)));
        assert_eq!(item_verb_timeout("update", 1), Some(Duration::from_secs(1020)));
        // Local verbs never spawn a network wait — left untimed.
        for local in ["activate", "deactivate", "delete"] {
            assert_eq!(item_verb_timeout(local, 3), None, "{local}");
        }
    }

    #[test]
    fn switch_language_rejects_malformed_locale_before_any_wp_call() {
        // Nonexistent binaries: reaching wp-cli would error with an io message,
        // NOT the validation message — proving the guard runs first.
        let e = switch_language(
            Path::new("/nonexistent/php"),
            Path::new("/nonexistent/wp.phar"),
            Path::new("/nonexistent/docroot"),
            "--skip-plugins",
        )
        .unwrap_err();
        assert!(e.to_string().contains("invalid locale"), "{e}");
    }

    #[test]
    fn theme_screenshot_data_url_and_missing() {
        let docroot = std::env::temp_dir().join(format!("rexenv-shot-{}", std::process::id()));
        let dir = docroot.join("wp-content/themes/twentytwentyfive");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("screenshot.png"), [0x89, b'P', b'N', b'G']).unwrap();

        let url = theme_screenshot(&docroot, "wp-content", "twentytwentyfive").expect("screenshot found");
        assert!(url.starts_with("data:image/png;base64,"), "{url}");
        assert!(theme_screenshot(&docroot, "wp-content", "no-such-theme").is_none());

        std::fs::remove_dir_all(&docroot).unwrap();
    }
}

#[cfg(test)]
mod core_root_tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexenv-coreroot-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The bug this exists for: `wp_step_streamed` pinned `--path` to the
    /// docroot, and a cloned Bedrock site's `wp core install` answered "This
    /// does not seem to be a WordPress installation" — its core is one folder
    /// in, at `web/wp`.
    #[test]
    fn the_wordpress_root_is_the_docroot_except_where_composer_put_core_one_level_in() {
        // Stock: the docroot IS the root.
        let d = scratch("stock");
        std::fs::write(d.join("wp-load.php"), "<?php").unwrap();
        assert_eq!(core_root(&d), d);
        assert_eq!(core_path_arg(&d), None, "no flag when there is nothing to correct");

        // Roots: core in `wp/`.
        let d = scratch("bedrock");
        std::fs::create_dir_all(d.join("wp")).unwrap();
        std::fs::write(d.join("wp/wp-load.php"), "<?php").unwrap();
        assert_eq!(core_root(&d), d.join("wp"));
        assert_eq!(core_path_arg(&d), Some(format!("--path={}", d.join("wp").display())));

        // BOTH present — a stock install that happens to have a `wp/` folder.
        // The docroot wins: it is the one WordPress actually boots from, and
        // guessing the other way would point wp-cli at somebody's plugin.
        std::fs::write(d.join("wp-load.php"), "<?php").unwrap();
        assert_eq!(core_root(&d), d);

        // Neither: a docroot before `wp core download` has run. Unchanged, so
        // the download lands where it always did.
        let d = scratch("empty");
        assert_eq!(core_root(&d), d);
        assert_eq!(core_path_arg(&d), None);
        for t in ["stock", "bedrock", "empty"] {
            let _ = std::fs::remove_dir_all(scratch(t));
        }
    }
}

#[cfg(test)]
mod bedrock_env_tests {
    use super::*;

    fn db() -> BedrockDb {
        BedrockDb {
            name: "wp_shop_rex".into(),
            user: "root".into(),
            password: String::new(),
            host: "127.0.0.1:13306".into(),
        }
    }

    /// Bedrock's own `.env.example`, copied from the shape Roots documents:
    /// blank salts, a commented database block, and `WP_SITEURL` written as a
    /// reference rather than a URL.
    const EXAMPLE: &str = "DB_NAME=\n\
         DB_USER=\n\
         DB_PASSWORD=\n\
         \n\
         # DB_HOST=localhost\n\
         # DATABASE_URL=mysql://user:password@127.0.0.1:3306/db_name\n\
         \n\
         WP_ENV=development\n\
         WP_HOME=http://example.com\n\
         WP_SITEURL=${WP_HOME}/wp\n\
         \n\
         AUTH_KEY=\n\
         SECURE_AUTH_KEY=\n\
         LOGGED_IN_KEY=\n\
         NONCE_KEY=\n\
         AUTH_SALT=\n\
         SECURE_AUTH_SALT=\n\
         LOGGED_IN_SALT=\n\
         NONCE_SALT=\n";

    #[test]
    fn wiring_bedrock_fills_the_blanks_and_leaves_the_convention_alone() {
        let out = wire_bedrock_env(EXAMPLE, "https://shop.rex", &db());

        assert!(out.contains("DB_NAME=wp_shop_rex"));
        assert!(out.contains("DB_USER=root"));
        assert!(out.contains("DB_PASSWORD="));
        // The commented DB_HOST is REPLACED in place — a value appended beside
        // it would leave the file with two answers.
        assert!(out.contains("DB_HOST=127.0.0.1:13306"));
        assert!(!out.contains("# DB_HOST"), "a commented twin is a second answer: {out}");
        assert_eq!(out.matches("DB_HOST=").count(), 1);
        assert!(out.contains("WP_HOME=https://shop.rex"));
        assert!(!out.contains("http://example.com"));
        // Bedrock's own convention, kept literal: phpdotenv expands it, and
        // baking the domain in twice means a rename fixes only one of them.
        assert!(out.contains("WP_SITEURL=${WP_HOME}/wp"));

        // Every blank salt got a real value, and no two are the same.
        let mut seen = std::collections::HashSet::new();
        for key in SALT_KEYS {
            let line = out
                .lines()
                .find(|l| l.starts_with(&format!("{key}=")))
                .unwrap_or_else(|| panic!("{key} missing from {out}"));
            let value = line.split_once('=').unwrap().1;
            assert_eq!(value.len(), 64, "{key} is not a full-length secret");
            assert!(seen.insert(value.to_string()), "{key} repeats another salt");
        }
        // DATABASE_URL is Bedrock's alternative to the DB_* block and we do not
        // write it — but leaving a COMMENTED one is fine, and it must not have
        // been un-commented by accident.
        assert!(out.contains("# DATABASE_URL="), "an untouched comment must survive: {out}");
    }

    #[test]
    fn salts_that_already_exist_are_never_rotated() {
        // A Retry, or a repo that committed real salts. Rotating these logs
        // every session out and invalidates every nonce — silently.
        let existing = format!("AUTH_KEY=already-a-real-secret\n{EXAMPLE}");
        let out = wire_bedrock_env(&existing, "https://shop.rex", &db());
        assert!(out.contains("AUTH_KEY=already-a-real-secret"));
        // Counted by LINE, not by substring: `SECURE_AUTH_KEY=` contains
        // `AUTH_KEY=`, and the writer keys off the line prefix for exactly
        // that reason — a substring check here would have "caught" a bug the
        // code does not have.
        let lines_for = |t: &str, key: &str| {
            t.lines().filter(|l| l.starts_with(&format!("{key}="))).count()
        };
        assert_eq!(lines_for(&out, "AUTH_KEY"), 1, "and no second answer: {out}");
        assert_eq!(lines_for(&out, "SECURE_AUTH_KEY"), 1, "the neighbour is its own key");
        // Its blank neighbours still get filled.
        assert!(!out.contains("NONCE_SALT=\n"));

        // Idempotent: wiring twice changes nothing about the secrets.
        let twice = wire_bedrock_env(&out, "https://shop.rex", &db());
        for key in SALT_KEYS {
            let take = |t: &str| {
                t.lines()
                    .find(|l| l.starts_with(&format!("{key}=")))
                    .map(str::to_string)
                    .unwrap()
            };
            assert_eq!(take(&out), take(&twice), "{key} changed on a second run");
        }
    }

    #[test]
    fn a_repo_with_no_example_still_gets_a_usable_env_from_the_seed() {
        let out = wire_bedrock_env(BEDROCK_ENV_SEED, "https://shop.rex", &db());
        assert!(out.contains("WP_ENV=development"));
        assert!(out.contains("DB_NAME=wp_shop_rex"));
        assert!(out.contains("WP_SITEURL=${WP_HOME}/wp"));
        for key in SALT_KEYS {
            assert!(!crate::core::dotenv::is_blank(&out, key), "{key} was left unset");
        }
    }
}

/// The guards behind "every `wp` rexenv runs for a user is pinned" (#228).
///
/// They exist in this shape because the ledger row named FOUR spawn sites and
/// there were seven by the time it was worked — two of them added after the row
/// was written, by people who had no reason to know a list existed. A guard
/// that asserted the four would have passed while the two it never heard of ran
/// a user's `~/.wp-cli/packages`: a guard narrower than its own claim, which is
/// the family this project keeps paying for.
///
/// So none of them counts spawn sites — the tree itself is the coverage. They
/// assert there is ONE place a wp-cli argv can be built and ONE place a
/// captured wp-cli process can be started, which a seventh site cannot be added
/// around without turning the build red.
#[cfg(test)]
mod packages_pin_guards {
    use super::*;

    /// Source with comments removed, because a guard that scans prose reads its
    /// OWN explanation and passes: these very tests quote both the marker and
    /// `Command::new(php_bin)` while explaining why there may be only one of
    /// each. That is not hypothetical — the #235 copy guard shipped defective
    /// for exactly this reason and only planting found it. Canary in
    /// [`the_scan_reads_code_and_not_its_own_comments`].
    /// The premium-update context is a CAPABILITY GRANT, so where it rides is
    /// the whole of its safety story. It belongs on the two paths that need
    /// WordPress's update data — the checked list and the update itself — and
    /// nowhere else: not on the fast list, not on install/activate/delete, and
    /// above all not on `wp_run_raw`, which is the MCP raw runner an agent
    /// drives. A list of call sites in a comment is what this module already
    /// watched drift (#228 named four spawn sites when there were seven), so
    /// the list is read out of the source instead.
    #[test]
    fn the_premium_update_context_rides_only_the_update_paths() {
        let src = strip_comments(include_str!("wordpress.rs"));
        // Assembled, never written: a literal here is a call site as far as the
        // scan is concerned, and the guard would convict itself (the RepoPanel
        // copy guard shipped defective for exactly that reason).
        let needle = format!("update_context{}(", "_arg");
        let mut current = String::new();
        let mut callers: std::collections::BTreeSet<String> = Default::default();
        for line in src.lines() {
            if let Some(name) = fn_name_of(line) {
                current = name;
            }
            if line.contains(&needle) {
                callers.insert(current.clone());
            }
        }
        assert!(!callers.is_empty(), "the scan found nothing — it stopped looking");
        let expected: std::collections::BTreeSet<String> = [
            // The definition itself.
            "update_context_arg",
            // The checked list (plugins AND themes go through this one fn).
            "checked_list",
            // The captured `plugin update` / `theme update`.
            "item_verb",
            // The streamed update — what the UI's Update button runs.
            "update_streamed",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            callers, expected,
            "the faked-capability context reached a path nobody argued for (or left one \
             it was needed on) — every entry here is a command that runs a vendor's \
             licensing code with capabilities it would not otherwise have"
        );
    }

    /// `fn foo(` at the start of a top-level item, whatever it is prefixed with.
    fn fn_name_of(line: &str) -> Option<String> {
        let t = line.trim_start();
        let rest = ["pub(crate) fn ", "pub async fn ", "pub fn ", "async fn ", "fn "]
            .iter()
            .find_map(|p| t.strip_prefix(p))?;
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        (!name.is_empty()).then_some(name)
    }

    /// The retry in `checked_list` skips CLOCK expiries, and it recognises one
    /// by the phrase the killer writes. Both halves are real here: a child that
    /// genuinely outlives its cap, and an ordinary failure that must NOT read as
    /// a timeout (or the fallback would stop happening exactly when it matters).
    #[test]
    fn a_killed_child_reports_a_timeout_this_module_can_recognise() {
        let mut sleeper = Command::new("/bin/sh");
        sleeper.args(["-c", "sleep 5"]);
        let err = run_with_timeout(sleeper, Duration::from_millis(150), "wp plugin")
            .expect_err("a 5s child under a 150ms cap must be killed");
        assert!(
            is_timeout_error(&err),
            "the expiry message stopped matching what the retry reads: {err}"
        );

        let other = Error::Other("wp plugin list failed (exit Some(1)): PHP Fatal error".into());
        assert!(
            !is_timeout_error(&other),
            "an ordinary failure read as a timeout — the plain-list fallback would never run"
        );
    }

    /// The grant is enumerated, not blanket. A `return true` here would switch
    /// on every OTHER capability-gated path a plugin runs at load, in a process
    /// that already has no user to answer for it.
    #[test]
    fn the_context_grants_named_capabilities_and_impersonates_nobody() {
        let php = update_context_php();
        for cap in ["manage_options", "update_plugins", "update_themes"] {
            assert!(php.contains(cap), "{cap} is not granted");
        }
        assert!(php.contains("WP_CLI::add_wp_hook( 'user_has_cap'"), "the grant must be queued pre-load");
        assert!(php.contains("define( 'WP_ADMIN', true )"), "the second measured gate is gone");
        for banned in ["wp_set_current_user", "wp_set_auth_cookie", "return true;"] {
            assert!(!php.contains(banned), "{banned} in the context file: that is impersonation, not a grant");
        }
        // Version-stamped, like every other write-once file beside the phar.
        assert!(
            UPDATE_CONTEXT_FILE.ends_with("-1.php"),
            "the body changed without the name changing — the old file wins forever"
        );
    }

    fn strip_comments(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for line in text.lines() {
            if line.trim_start().starts_with("//") {
                out.push('\n');
                continue;
            }
            // Line-based and deliberately simple. The one case that needs care
            // is a `//` inside a string literal, which in this tree is always a
            // URL — so a `//` preceded by `:` is not a comment. Cutting a real
            // trailing comment short of a `://` would only ever leave MORE text
            // for the scans to find, never less, so the failure direction is a
            // noisy guard rather than a blind one.
            let cut = line
                .match_indices("//")
                .find(|(i, _)| *i == 0 || !line[..*i].ends_with(':'))
                .map(|(i, _)| i);
            out.push_str(cut.map_or(line, |i| &line[..i]));
            out.push('\n');
        }
        out
    }

    /// Every `.rs` under `src/` and `examples/`, so a new spawn site cannot
    /// hide by being in a file nobody thought to list. Comments stripped.
    fn rust_sources() -> Vec<(String, String)> {
        fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
                        let rel = path.strip_prefix(root).unwrap_or(&path);
                        out.push((rel.display().to_string(), strip_comments(&text)));
                    }
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut out = Vec::new();
        walk(&root.join("src"), &mut out);
        walk(&root.join("examples"), &mut out);
        out
    }

    /// The marker every wp-cli argv carries. Split so this file's own guard
    /// text is not a hit — the RepoPanel copy guard shipped defective for
    /// exactly that reason: it read its own explanation and passed.
    fn marker() -> String {
        format!("memory_limit={}", "512M")
    }

    /// Without this, every assertion below can pass on an empty string, and
    /// prose can satisfy the ones that check for presence. Both directions.
    #[test]
    fn the_scan_reads_code_and_not_its_own_comments() {
        let this = strip_comments(include_str!("wordpress.rs"));
        assert!(
            this.contains("pub fn wp_argv_prefix(wp_phar: &Path) -> Vec<String> {"),
            "the stripper ate code — every scan here would now pass vacuously"
        );
        // A phrase that exists ONLY in a doc comment in this file — the
        // assertion messages below quote the marker and the spawn expression
        // themselves, so a canary taken from one of those would prove nothing.
        // Assembled, not written: a literal here would itself survive the
        // stripper as code and the canary would be testing its own presence.
        let prose = format!("the tree itself {} the coverage", "is");
        assert!(include_str!("wordpress.rs").contains(&prose), "the canary phrase was edited away");
        assert!(
            !this.contains(&prose),
            "comment text survived the stripper — a guard that reads prose reads its own \
             explanation (#235)"
        );
        // The tests below take the tree from `rust_sources`, so it must strip too.
        let scanned = rust_sources();
        let me = scanned.iter().find(|(p, _)| p == "src/core/wordpress.rs").expect("this file");
        assert!(!me.1.contains(&prose));
    }

    #[test]
    fn every_wp_cli_argv_in_the_tree_is_pinned() {
        let sources = rust_sources();
        assert!(sources.len() > 50, "the source walk found {} files — it has stopped working", sources.len());
        let marker = marker();
        let mut carriers: Vec<&str> =
            sources.iter().filter(|(_, t)| t.contains(&marker)).map(|(p, _)| p.as_str()).collect();
        carriers.sort();
        assert_eq!(
            carriers,
            vec!["src/core/terminal.rs", "src/core/wordpress.rs"],
            "a wp-cli argv is built somewhere that is not `wp_argv_prefix`. Every rexenv-run \
             `wp` must be pinned to the bundled command set (#228), and a second argv is a \
             spawn this pin has not been shown to cover. The ONE exemption is \
             `core::terminal`'s `wp` wrapper — the user's own command line, left ambient by \
             decision (D1, 13 Aug 2026), which is what makes the tell's \"they still work in \
             rexenv's terminal\" true. Build yours from `wordpress::wp_argv_prefix`."
        );
    }

    #[test]
    fn every_captured_wp_cli_spawn_goes_through_the_pinned_command() {
        let sources = rust_sources();
        // A WP-CLI spawn, not merely a PHP one. `Command::new(php_bin)` alone
        // convicted `adminer::verify_pair`, which runs PHP against a candidate
        // Adminer and never goes near wp-cli — a guard matching more than its own
        // claim, which is the mirror image of the family this file's other
        // guards exist for. The pin being protected is the wp-cli PACKAGES dir,
        // so the spawn is in scope exactly when it also carries the wp phar.
        let spawns: Vec<&str> = sources
            .iter()
            .filter(|(_, t)| t.contains("Command::new(php_bin)") && t.contains("wp_phar"))
            .map(|(p, _)| p.as_str())
            .collect();
        assert_eq!(
            spawns,
            vec!["src/core/wordpress.rs"],
            "a captured wp-cli process is started outside `wp_command`, which is the only \
             place the packages-dir pin is applied to a `Command` (#228)."
        );
        // The narrowed matcher must still MATCH — an AND that quietly matches
        // nothing is a guard that passes because it stopped looking.
        assert_eq!(spawns.len(), 1, "the wp-cli spawn matcher found nothing at all");
        // Brace-depth, not a cut at the first occurrence — a test module can sit
        // anywhere in a file, and the naive split drops everything after it.
        let this = strip_comments(&crate::core::copy_scan::production_source(include_str!(
            "wordpress.rs"
        )));
        assert!(this.contains("fn wp_command("), "the scan lost the function it is about");
        assert_eq!(
            this.matches("Command::new(php_bin)").count(),
            1,
            "`wp_command` is no longer the only `Command::new(php_bin)` in this module — the \
             others are wp-cli spawns running whatever the user installed globally."
        );
    }

    /// The structural guards above say there is one door; this says the door
    /// is locked. Without it, deleting the `.env` line from `wp_command` leaves
    /// every other test in this module green while three spawn sites go back to
    /// loading the user's packages.
    #[test]
    fn the_one_captured_spawn_actually_carries_the_pin() {
        let phar = std::env::temp_dir()
            .join(format!("rexenv-cmdpin-{}", std::process::id()))
            .join("bin/wp-cli-2.12.0/wp-cli.phar");
        std::fs::create_dir_all(phar.parent().unwrap()).unwrap();
        std::fs::write(&phar, b"phar").unwrap();
        let cmd = wp_command(Path::new("/usr/bin/php"), &phar);

        let args: Vec<String> =
            cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let prefix = wp_argv_prefix(&phar);
        assert_eq!(
            args[..prefix.len()],
            prefix[..],
            "the shared prefix is not what gets spawned"
        );
        // …and the ONLY thing after it is the end-of-output require (#316),
        // which is what makes "ours is the first required file" true.
        assert_eq!(
            args[prefix.len()..],
            [format!("--require={}", phar.with_file_name(EOO_REQUIRE_FILE).display())],
            "the captured spawn's argv is no longer prefix + the end-of-output require"
        );

        let (key, value) = wp_packages::pin_packages_env(&phar);
        let pinned = cmd
            .get_envs()
            .find(|(k, _)| k.to_string_lossy() == key)
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()));
        assert_eq!(
            pinned.as_deref(),
            Some(value.as_str()),
            "the captured spawn does not pin `WP_CLI_PACKAGES_DIR` — it runs whatever the \
             machine has in `~/.wp-cli/packages` (#228)"
        );
        let _ = std::fs::remove_dir_all(phar.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    // ── #316 · stdout is the command's only up to the marker ────────────────

    /// A temp dir this test owns, removed however the test ends.
    struct TempPhar(PathBuf);
    impl Drop for TempPhar {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn temp_phar(tag: &str) -> (TempPhar, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rexenv-eoo-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let phar = dir.join("wp-cli.phar");
        std::fs::write(&phar, b"phar").unwrap();
        (TempPhar(dir), phar)
    }

    /// #317: PHP's own diagnostics must not be on the stream the answer is on.
    /// In the SHARED prefix, so the streamed spawns get it too, and before the
    /// phar — a `-d` after the script name is an argument to the script.
    #[test]
    fn every_wp_cli_runs_php_with_its_diagnostics_on_stderr() {
        let phar = Path::new("/tmp/wp-cli.phar");
        let argv = wp_argv_prefix(phar);
        let at = argv
            .iter()
            .position(|a| a == "display_errors=stderr")
            .expect(
                "PHP prints diagnostics to STDOUT by default, so one deprecation inside the phar \
                 (measured: PHP 8.5.8 + wp-cli 2.12.0, react/promise) lands in front of every \
                 answer rexenv parses (#317)",
            );
        assert_eq!(argv[at - 1], "-d", "the value is not attached to a -d flag");
        assert!(
            at < argv.iter().position(|a| a.ends_with(".phar")).expect("the phar"),
            "the flag is after the script name, where PHP hands it to the script instead"
        );
    }

    #[test]
    fn no_marker_means_every_byte_is_the_commands_output() {
        let out = b"6.8.3\n";
        let (clean, tail) = split_at_eoo(out);
        assert_eq!(clean, out);
        assert!(tail.is_empty());
    }

    /// The 14 Aug 2026 shape verbatim: valid JSON, then Elementor's shutdown
    /// notice. Before #316 this was `bad JSON: trailing characters`.
    #[test]
    fn the_cut_keeps_the_json_and_hands_back_the_notice() {
        let json = r#"[{"name":"elementor","status":"active"}]"#;
        let notice = "PHP: 2026-08-14 [notice X 0][…] Implicitly marking parameter $key as \
                      nullable is deprecated\n";
        let raw = format!("{json}\n{EOO_MARKER}\n{notice}");
        let (clean, tail) = split_at_eoo(raw.as_bytes());
        assert_eq!(String::from_utf8_lossy(clean).trim(), json);
        assert!(String::from_utf8_lossy(tail).contains("Implicitly marking parameter"));
        let _: Vec<serde_json::Value> = json_from_wp(&String::from_utf8_lossy(clean), "t").unwrap();
    }

    /// A site's own data can contain any string, and the marker is printed
    /// AFTER all of it — so the last hit is ours and the first may not be.
    #[test]
    fn the_last_marker_wins_so_site_data_cannot_truncate_the_answer() {
        let raw = format!("value with {EOO_MARKER} inside it\n{EOO_MARKER}\nnoise\n");
        let (clean, tail) = split_at_eoo(raw.as_bytes());
        assert_eq!(String::from_utf8_lossy(clean), format!("value with {EOO_MARKER} inside it\n"));
        assert_eq!(String::from_utf8_lossy(tail).trim(), "noise");
    }

    #[test]
    fn the_tail_is_carried_to_stderr_not_dropped() {
        let out = Output {
            status: std::process::Command::new("true").status().unwrap(),
            stdout: format!("https://xyz.rex\n{EOO_MARKER}\nDeprecated: something\n").into_bytes(),
            stderr: b"".to_vec(),
        };
        let cut = cut_post_run_tail(out);
        assert_eq!(String::from_utf8_lossy(&cut.stdout), "https://xyz.rex\n");
        let err = String::from_utf8_lossy(&cut.stderr);
        assert!(err.contains("Deprecated: something"), "the tail was dropped: {err}");
        assert!(
            err.contains("AFTER the command finished"),
            "the tail reached stderr unattributed, so it reads as rexenv's own error: {err}"
        );
    }

    /// Our own trailing newline is not a diagnostic — a quiet run must not grow
    /// a stderr, because a caller that treats any stderr as trouble would then
    /// see trouble on every command.
    #[test]
    fn a_quiet_run_gains_no_stderr() {
        let out = Output {
            status: std::process::Command::new("true").status().unwrap(),
            stdout: format!("6.8.3\n{EOO_MARKER}\n").into_bytes(),
            stderr: b"".to_vec(),
        };
        let cut = cut_post_run_tail(out);
        assert_eq!(String::from_utf8_lossy(&cut.stdout), "6.8.3\n");
        assert!(cut.stderr.is_empty(), "stderr: {}", String::from_utf8_lossy(&cut.stderr));
    }

    #[test]
    fn the_require_file_is_materialised_and_prints_the_marker() {
        let (_own, phar) = temp_phar("write");
        let arg = eoo_require_arg(&phar).expect("the require file is written beside the phar");
        let path = phar.with_file_name(EOO_REQUIRE_FILE);
        assert_eq!(arg, format!("--require={}", path.display()));
        let php = std::fs::read_to_string(&path).unwrap();
        assert!(php.contains("register_shutdown_function"), "{php}");
        assert!(php.contains(EOO_MARKER), "the file prints a marker the cut does not know: {php}");
        // Second call: the file is not rewritten under a concurrent reader.
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(eoo_require_arg(&phar).is_some());
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    /// wp-cli REFUSES to run when a required file is missing, so an
    /// unwritable location must yield no flag at all — noisy, never broken.
    #[test]
    fn an_unwritable_location_yields_no_require_flag() {
        let phar = std::env::temp_dir()
            .join(format!("rexenv-eoo-nodir-{}", std::process::id()))
            .join("nope")
            .join("wp-cli.phar");
        assert!(eoo_require_arg(&phar).is_none(), "a flag was passed for a file that isn't there");
    }

    /// The belt: trailing bytes are ignored, LEADING bytes are still an error.
    #[test]
    fn the_json_read_tolerates_a_tail_but_never_guesses_at_a_head() {
        let rows: Vec<serde_json::Value> =
            json_from_wp(r#"[{"name":"akismet"}]PHP: notice…"#, "wp plugin").unwrap();
        assert_eq!(rows.len(), 1);
        let err = json_from_wp::<Vec<serde_json::Value>>(r#"notice…[{"name":"akismet"}]"#, "wp plugin")
            .unwrap_err()
            .to_string();
        assert!(!err.is_empty());
    }

    /// **The third door names itself, and still refuses to walk through.**
    ///
    /// A plugin that `echo`es mid-command is the one wp-cli stdout problem
    /// neither #316 (the shutdown-hook tail, cut by the marker) nor #317
    /// (diagnostics, moved to stderr) covers: `echo` is not PHP's error display,
    /// and the bytes land in FRONT of the answer where the marker cannot reach.
    /// `docs/ARCHITECTURE.md` §9 had named it and nothing else had.
    ///
    /// The refusal is deliberate and unchanged — skipping to the first brace
    /// means guessing which one starts the answer, and a wrong guess shows
    /// plausible wrong data. What changes is that the failure stops being
    /// anonymous: `bad JSON: expected value at line 1 column 1` is the message
    /// all three doors produced, and it sends the reader at rexenv, or at
    /// WordPress, or at their database.
    #[test]
    fn a_plugin_echoing_before_the_answer_is_named_rather_than_skipped() {
        let echoed = r#"Notice: undefined index in my-plugin.php on line 12
[{"name":"akismet"}]"#;
        let err = json_from_wp::<Vec<serde_json::Value>>(echoed, "wp plugin")
            .unwrap_err()
            .to_string();

        // Still a refusal. This is the half that must never soften.
        assert!(
            json_from_wp::<Vec<serde_json::Value>>(echoed, "wp plugin").is_err(),
            "the read recovered — it must not guess which brace starts the answer"
        );
        // …and it says WHAT happened, WHERE to look, and WHY it will not guess.
        assert!(err.contains("before its answer"), "{err}");
        assert!(err.contains("mu-plugin"), "no place to look: {err}");
        assert!(err.contains("guessing"), "does not say why it refuses: {err}");
        assert!(err.contains("my-plugin.php"), "the prefix is not quoted back: {err}");

        // Junk that is NOT hiding a valid answer keeps the plain parse error —
        // claiming "bytes before the answer" when there is no answer would be
        // the same guessing, one layer up. **This input must contain a brace**:
        // without one the prefix branch is never entered, and the assertion
        // below passes without exercising anything. The first version of this
        // test used "Fatal error: out of memory" and proved nothing — caught by
        // planting the invention and watching the test stay green.
        let garbage = json_from_wp::<Vec<serde_json::Value>>(
            "Fatal error: Allowed memory size exhausted in wp-content/plugins/x.php:9 {",
            "wp plugin",
        )
        .unwrap_err()
        .to_string();
        assert!(garbage.contains("bad JSON"), "{garbage}");
        assert!(!garbage.contains("before its answer"), "invented a prefix: {garbage}");

        // Mid-VALUE output is the genuinely unrecoverable shape: nothing parses
        // from any later offset, so it must not be dressed up either.
        let mid = json_from_wp::<Vec<serde_json::Value>>(r#"[{"name":"aki"OOPS"smet"}]"#, "wp plugin")
            .unwrap_err()
            .to_string();
        assert!(mid.contains("bad JSON"), "{mid}");
    }

    /// The quoted prefix is a MESSAGE, not a channel: a plugin cannot use it to
    /// inject newlines into a log or run away with the length of a toast.
    #[test]
    fn the_quoted_prefix_is_bounded_and_flattened() {
        let noisy = quoted_prefix("line one\nline\ttwo\r\n   spaced   out");
        assert_eq!(noisy, "\"line one line two spaced out\"");
        assert!(!noisy.contains('\n') && !noisy.contains('\t'));

        let long = quoted_prefix(&"x".repeat(JSON_PREFIX_QUOTE * 3));
        assert!(long.ends_with("…\""), "{long}");
        assert!(long.chars().count() <= JSON_PREFIX_QUOTE + 3, "unbounded: {}", long.len());
    }

    /// Coverage as a property of the module, not a list of the three sites that
    /// were remembered on the day (#228's lesson, applied to #316).
    #[test]
    fn every_captured_wp_cli_spawn_cuts_the_post_run_tail() {
        let this = strip_comments(&crate::core::copy_scan::production_source(include_str!(
            "wordpress.rs"
        )));
        let mut fns: Vec<(String, String)> = Vec::new();
        let (mut name, mut body) = (String::new(), String::new());
        for line in this.lines() {
            let starts_fn = !line.starts_with(char::is_whitespace)
                && (line.starts_with("fn ")
                    || line.starts_with("pub fn ")
                    || line.starts_with("pub(crate) fn "));
            if starts_fn {
                if !name.is_empty() {
                    fns.push((std::mem::take(&mut name), std::mem::take(&mut body)));
                }
                name = line.split('(').next().unwrap_or(line).to_string();
            }
            body.push_str(line);
            body.push('\n');
        }
        fns.push((name, body));

        let mut checked = 0;
        for (name, body) in &fns {
            if !body.contains("wp_command(php_bin, wp_phar)") {
                continue;
            }
            checked += 1;
            assert!(
                body.contains("cut_post_run_tail("),
                "`{name}` spawns the captured wp-cli command and returns its stdout uncut — a \
                 plugin's shutdown hook (Elementor on PHP 8.4) writes to that stdout after the \
                 command finished, and whatever parses it reads the notice as part of the \
                 answer (#316)."
            );
        }
        assert_eq!(
            checked, 3,
            "the scan found {checked} captured spawn sites (expected 3: wp_cli, wp_cli_timed, \
             wp_run_raw) — either a site was added without a cut, or the scan has stopped working"
        );
    }

    /// The streamed spawn cannot use a `Command`, so it is the one site where
    /// the pin is a line that could be dropped without the door disappearing.
    #[test]
    fn the_streamed_spawn_pins_the_env_it_was_handed() {
        let this = strip_comments(&crate::core::copy_scan::production_source(include_str!(
            "wordpress.rs"
        )));
        let body = this
            .split("pub fn wp_step_streamed(")
            .nth(1)
            .and_then(|b| b.split("\npub fn ").next())
            .expect("wp_step_streamed");
        assert!(
            body.contains("with_pinned_packages(stream.env"),
            "the streamed step passes its caller's environment through unpinned — that env is \
             the user's login shell, so an exported `WP_CLI_PACKAGES_DIR` reaches wp-cli (#228)"
        );
        // …and that the pinned value is what the spawn is actually handed, not
        // computed into a variable the call then ignores. Whitespace-normalised
        // so a rustfmt reflow cannot turn this into a false alarm.
        let dense: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            dense.contains("docroot,&env,"),
            "the pinned environment is built but `run_step_streamed` is handed something else"
        );
    }

    /// Every file that builds a wp-cli argv must also pin the packages dir.
    /// The pin can arrive three ways — `wp_command` (captured),
    /// `with_pinned_packages` (streamed), or a module's own override
    /// (`dist_archive`, #230, which pins to its own empty dir for its own
    /// reason) — and the point is that there is no fourth way: none.
    #[test]
    fn nothing_builds_a_wp_cli_argv_without_pinning_the_packages_dir() {
        let pins = ["wp_command(", "with_pinned_packages(", "pin_packages_env(", "WP_CLI_PACKAGES_DIR"];
        let mut checked = 0;
        for (path, text) in rust_sources() {
            let body = crate::core::copy_scan::production_source(&text);
            if !body.contains("wp_argv_prefix(") || path == "src/core/wordpress.rs" {
                continue;
            }
            checked += 1;
            assert!(
                pins.iter().any(|p| body.contains(p)),
                "{path} builds a wp-cli argv but never pins the packages dir — the command set \
                 it runs is whatever that machine has in `~/.wp-cli/packages` (#228)."
            );
        }
        assert!(checked > 0, "the detection found no callers — it has stopped working");
    }
}
