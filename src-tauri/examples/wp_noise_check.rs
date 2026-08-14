//! Manual check: nothing but the command's own output reaches what rexenv parses
//! — at EITHER end (ledger #316 the tail, #317 the head) — the L1 leg.
//! Run: `cargo run --example wp_noise_check`
//!
//! # The bug this is about
//!
//! Elementor 4.2.2 under PHP 8.4 registers its own WP-CLI logger and, from a
//! shutdown hook, prints every PHP notice it collected — measured 14 Aug 2026,
//! by stack trace: `Manager::shutdown` → `Cli_Logger::save_log` →
//! `WP_CLI::log` → `fwrite(STDOUT)`. So `wp plugin list --format=json` came back
//! as a perfectly good array with a deprecation notice glued to the end of it,
//! rexenv said `bad JSON: trailing characters at line 1 column 814`, and every
//! WordPress screen for that site stopped working. `wp option get home` was
//! wrong the same way — the noise is not a JSON problem, it is a stdout problem.
//!
//! The same report came back for PHP 8.5, and it was the OTHER end: PHP's CLI
//! SAPI prints its own diagnostics to STDOUT, and the pinned 2.12.0 phar raises
//! one under 8.5 in its own vendored code (`Deprecated: Case statements followed
//! by a semicolon (;) … react/promise/src/functions.php on line 369`), before
//! wp-cli has printed a byte. Same dead screens, no plugin involved. Leg D is
//! that half.
//!
//! # Why an L1 leg, and why the fixture is a REQUIRE FILE
//!
//! L0 can prove the cut ([`split_at_eoo`]) and the argv. It cannot prove the one
//! thing the fix rests on: that rexenv's `--require` shutdown function runs
//! BEFORE a plugin's, inside the real phar. That is wp-cli's loading order, not
//! ours, so it has to be asked of the real binary.
//!
//! The fixture reproduces the mechanism rather than the plugin: a `--require`
//! file that registers a command printing JSON (`before_wp_load`, so no
//! WordPress is needed — the `wp_packages_check` canary trick) and a shutdown
//! function that writes to STDOUT with `fwrite`, which is the exact call the
//! real trace ends in. Leg A REQUIRES that this reproduces the disease; if it
//! ever stops doing so, this example says CONTROL BROKEN and stops rather than
//! reporting the fix as proven.
//!
//! # What a green run proves, and what it does not
//!
//! Proves: through both captured production entry points, against the real
//! pinned phar, output written to stdout after the command finished is (1) not
//! in what the caller parses and (2) not lost either — it is on stderr, saying
//! where it came from; and that a PHP diagnostic raised before the command's own
//! output does not land in front of it.
//!
//! Does not prove: anything about a plugin that `echo`es to stdout while the
//! command runs (not a diagnostic, not after the end — nothing in rexenv covers
//! that); anything about the streamed spawns, which deliberately carry no marker
//! (their output is a live log, not a parse); and nothing about `wp cli info` /
//! `wp --info`, whose early path ends without running shutdown functions at all
//! — measured, and harmless, because that path never loads WordPress, so no
//! plugin can print on it.
//!
//! # Fixture ownership
//!
//! Everything is under a temp dir this example created and removes on Drop.
//! `common::sandbox` supplies the Platform; binaries come from the shared cache
//! (the documented exception), which means production's own
//! `.rexenv-end-of-output-1.php` and `.rexenv-no-wp-packages` are created beside
//! the real phar — the same two files the app writes on its first wp command.

mod common;

use rexenv_lib::core::{binaries, php, wordpress, wp_packages};
use std::path::PathBuf;
use std::time::Duration;

/// A command the fixture registers, so a stray hit anywhere could only be ours.
const FIXTURE_COMMAND: &str = "rexenv-noise-fixture";
/// What that command answers — the shape `wp plugin list --format=json` has.
const FIXTURE_JSON: &str = r#"[{"name":"akismet","status":"inactive"}]"#;
/// The post-run write. Shaped like Elementor's real line so a reader of a failed
/// run recognises what this is imitating.
const NOISE_CANARY: &str = "REXENV-NOISE-CANARY";
/// The BEFORE-the-command write (#317). A real engine diagnostic rather than an
/// echo, because the claim is about where PHP sends diagnostics — and raised
/// with `trigger_error` rather than by borrowing the phar's own 8.5 deprecation,
/// which would make this check depend on a bug in someone else's release. It
/// therefore fires LATER than the real one (require time, not phar bootstrap) —
/// which changes nothing, because the flag is an ini value set at process start
/// and nothing between those two moments touches `display_errors`.
const HEAD_CANARY: &str = "REXENV-HEAD-CANARY";

struct Fixture {
    dir: PathBuf,
    require: PathBuf,
    head_require: PathBuf,
    docroot: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn plant_fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!("rexenv-noise-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let require = dir.join("noise.php");
    std::fs::write(
        &require,
        format!(
            "<?php\n\
             WP_CLI::add_command(\n    '{FIXTURE_COMMAND}',\n    function () {{ WP_CLI::log( '{FIXTURE_JSON}' ); }},\n    array( 'when' => 'before_wp_load' )\n);\n\
             register_shutdown_function( static function () {{\n\
             \tfwrite( STDOUT, \"PHP: 2026-08-14 [notice X 0][fixture::1] {NOISE_CANARY} — implicitly marking parameter \\$key as nullable is deprecated\\n\" );\n\
             }} );\n"
        ),
    )
    .expect("fixture require file");
    let head_require = dir.join("head.php");
    std::fs::write(
        &head_require,
        format!(
            "<?php\ntrigger_error( '{HEAD_CANARY} — raised before the command printed anything', E_USER_DEPRECATED );\n"
        ),
    )
    .expect("fixture head require file");
    let docroot = dir.join("docroot");
    std::fs::create_dir_all(&docroot).expect("fixture docroot");
    Fixture { dir, require, head_require, docroot }
}

/// The production prefix with the `#317` flag taken out — argv exactly as it was
/// before the fix, for the control leg. Removed as a PAIR: leaving the bare `-d`
/// would make PHP read the phar path as the ini setting.
fn without_display_errors(argv: Vec<String>) -> Vec<String> {
    let at = argv.iter().position(|a| a == "display_errors=stderr").unwrap_or_else(|| {
        fail(
            "D — the prefix no longer carries `display_errors=stderr`",
            "PHP prints its diagnostics to STDOUT by default, so one deprecation inside the phar \
             lands in front of every answer rexenv parses (#317). There is nothing left for this \
             leg to control against.",
        )
    });
    let mut out = argv;
    out.remove(at);
    out.remove(at - 1); // its `-d`
    out
}

fn fail(step: &str, why: &str) -> ! {
    eprintln!("\n✗ {step}\n  {why}\n");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let (plat, _guard) = common::sandbox("wp-noise");
    let fixture = plant_fixture();
    let require_arg = format!("--require={}", fixture.require.display());

    let patch = php::patch_for_minor("8.4").expect("pinned 8.4");
    let php_bin = binaries::resolve(&*plat, "php", patch).await.expect("php");
    let wp_phar = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION)
        .await
        .expect("wp-cli phar");

    // ── A · THE CONTROL — the fixture must reproduce the disease ────────────
    //
    // Same phar, same argv, WITHOUT rexenv's end-of-output require. If this
    // parses cleanly, the fixture is no longer imitating the bug and everything
    // below would pass while proving nothing.
    let control = std::process::Command::new(&php_bin)
        .args(wordpress::wp_argv_prefix(&wp_phar))
        .arg(FIXTURE_COMMAND)
        .arg(&require_arg)
        .env("WP_CLI_PACKAGES_DIR", wp_packages::neutral_packages_path(&wp_phar))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("spawn the control");
    let control_out = String::from_utf8_lossy(&control.stdout).into_owned();
    if !control_out.contains(NOISE_CANARY) {
        fail(
            "CONTROL LEG BROKEN — this run proves NOTHING about the cut",
            &format!(
                "the fixture's shutdown write never reached stdout, so legs B and C would find \
                 clean output for a reason that has nothing to do with rexenv — and would \
                 pass.\n  Likely causes: wp-cli stopped running shutdown functions on this \
                 path, or `before_wp_load` is no longer how a command opts out of needing \
                 WordPress.\n  exit={:?}\n  stdout: {}\n  stderr: {}",
                control.status.code(),
                control_out.trim(),
                String::from_utf8_lossy(&control.stderr).trim()
            ),
        );
    }
    if serde_json::from_str::<serde_json::Value>(control_out.trim()).is_ok() {
        fail(
            "CONTROL LEG BROKEN — the noise did not break the parse",
            &format!(
                "stdout carried the canary and STILL parsed as JSON, so this fixture is not the \
                 bug (#316) any more.\n  stdout: {}",
                control_out.trim()
            ),
        );
    }
    println!("A ok — without the marker the answer is unparseable, exactly as reported");

    // ── B · the CAPTURED path (`wp_cli` → `wp_cli_checked`) ─────────────────
    let out = wordpress::wp_cli_checked(&php_bin, &wp_phar, &[FIXTURE_COMMAND, &require_arg], None)
        .unwrap_or_else(|e| fail("B — the captured path failed outright", &e.to_string()));
    check_clean("B (wp_cli_checked)", &out, "");

    // Where the noise ENDED UP is half the claim: dropping it would leave a
    // developer with a plugin printing into their tooling and nothing saying so.
    // `wp_cli_checked` returns only stdout on success, so the raw spawn below is
    // where both halves are visible at once.

    // ── C · the RAW runner (`wp_run_raw`, the MCP path) ─────────────────────
    let raw = wordpress::wp_run_raw(
        &php_bin,
        &wp_phar,
        &fixture.docroot,
        &[FIXTURE_COMMAND.to_string(), require_arg.clone()],
        Duration::from_secs(60),
    )
    .unwrap_or_else(|e| fail("C — the raw runner failed outright", &e.to_string()));
    let stdout = String::from_utf8_lossy(&raw.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&raw.stderr).into_owned();
    check_clean("C (wp_run_raw)", &stdout, &stderr);

    if !stderr.contains(NOISE_CANARY) {
        fail(
            "C — the post-run write was DROPPED",
            &format!(
                "the cut removed the noise from stdout and threw it away. It is a real problem \
                 on the user's site (a plugin deprecation under their PHP version) and the only \
                 place left to say so is stderr (#316).\n  stderr: {}",
                stderr.trim()
            ),
        );
    }
    if !stderr.contains("AFTER the command finished") {
        fail(
            "C — the tail reached stderr unattributed",
            &format!(
                "on a failing command this text lands in an error a user reads, and \
                 `Deprecated: …` with nothing around it reads as rexenv's own bug.\n  stderr: \
                 {}",
                stderr.trim()
            ),
        );
    }
    println!("C ok — the tail is on stderr, attributed, and out of the answer");

    // ── D · the HEAD — PHP's own diagnostics (#317) ─────────────────────────
    //
    // D1 is the control: the same argv WITHOUT the flag, which is what shipped
    // until 14 Aug 2026 and what PHP 8.5 turned into a broken WordPress tab.
    let head_arg = format!("--require={}", fixture.head_require.display());
    let control_head = std::process::Command::new(&php_bin)
        .args(without_display_errors(wordpress::wp_argv_prefix(&wp_phar)))
        .arg(FIXTURE_COMMAND)
        .arg(&require_arg)
        .arg(&head_arg)
        .env("WP_CLI_PACKAGES_DIR", wp_packages::neutral_packages_path(&wp_phar))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("spawn the head control");
    let head_out = String::from_utf8_lossy(&control_head.stdout).into_owned();
    if !head_out.contains(HEAD_CANARY) {
        fail(
            "CONTROL LEG BROKEN — D proves nothing about the head",
            &format!(
                "without `display_errors=stderr`, a PHP diagnostic did NOT reach stdout — so \
                 this machine's PHP is not doing the thing the flag exists to stop (check its \
                 php.ini: a `display_errors` already set to stderr or off makes this leg \
                 vacuous).\n  stdout: {}\n  stderr: {}",
                head_out.trim(),
                String::from_utf8_lossy(&control_head.stderr).trim()
            ),
        );
    }
    println!("D ok (control) — unflagged, a PHP diagnostic lands in front of the answer");

    let treated = wordpress::wp_run_raw(
        &php_bin,
        &wp_phar,
        &fixture.docroot,
        &[FIXTURE_COMMAND.to_string(), require_arg, head_arg],
        Duration::from_secs(60),
    )
    .unwrap_or_else(|e| fail("D — the flagged run failed outright", &e.to_string()));
    let head_stdout = String::from_utf8_lossy(&treated.stdout).into_owned();
    let head_stderr = String::from_utf8_lossy(&treated.stderr).into_owned();
    if head_stdout.contains(HEAD_CANARY) {
        fail(
            "D — the diagnostic is still in front of the answer",
            &format!(
                "`display_errors=stderr` is not reaching PHP (a `-d` after the script name is an \
                 argument to the script, not an ini setting) — #317.\n  stdout: {}",
                head_stdout.trim()
            ),
        );
    }
    if !head_stderr.contains(HEAD_CANARY) {
        fail(
            "D — the diagnostic was silenced rather than moved",
            &format!(
                "a deprecation on the user's site is a real finding; the fix moves it to stderr, \
                 it does not turn error reporting off (#317).\n  stderr: {}",
                head_stderr.trim()
            ),
        );
    }
    // …and the answer itself still parses, with BOTH ends of noise present.
    check_clean("D (both ends at once)", &head_stdout, &head_stderr);
    println!("D ok — PHP's diagnostics are on stderr, and the answer survives both ends");

    println!("\n✓ wp_noise_check: neither end of stdout can reach what rexenv parses (#316, #317)");
}

/// The half both production paths share: what the caller parses is the
/// command's own output and nothing else.
fn check_clean(leg: &str, stdout: &str, stderr: &str) {
    if stdout.contains(NOISE_CANARY) {
        fail(
            &format!("{leg} — the noise is still in the parsed output"),
            &format!(
                "the end-of-output marker did not do its job: either the require file was not \
                 passed, or a plugin's shutdown function ran BEFORE ours (#316).\n  stdout: \
                 {}\n  stderr: {}",
                stdout.trim(),
                stderr.trim()
            ),
        );
    }
    if stdout.contains(wordpress::EOO_MARKER) {
        fail(
            &format!("{leg} — the marker itself leaked into the output"),
            &format!("rexenv's own bookkeeping is being handed to a parser.\n  stdout: {}", stdout.trim()),
        );
    }
    match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        Ok(v) => {
            if v.to_string() != serde_json::from_str::<serde_json::Value>(FIXTURE_JSON).unwrap().to_string() {
                fail(
                    &format!("{leg} — the cut changed the answer"),
                    &format!("expected {FIXTURE_JSON}, got {v}"),
                );
            }
        }
        Err(e) => fail(
            &format!("{leg} — the answer does not parse"),
            &format!("{e}\n  stdout: {}", stdout.trim()),
        ),
    }
    println!("{leg} ok — the parsed answer is exactly the command's own output");
}
