//! Manual check: the wp-cli COMMAND SET is pinned (ledger #228) — the L1 leg.
//! Run: `cargo run --example wp_packages_check`
//!
//! L0 can assert an argv and an environment pair. It cannot show that command
//! RESOLUTION differs, because resolution happens inside the real phar — which
//! is the entire claim. So this runs the pinned PHP against the pinned WP-CLI
//! and asks it.
//!
//! # The control leg is the point, and it plants its own canary
//!
//! The obvious version of this check — "ask for `dist-archive`, expect it to be
//! missing" — passes on a machine that never had the package, which is every CI
//! box and most laptops. It would then be green while proving nothing. #236 hit
//! the mirror image of this and the lesson is the same: the machine must not be
//! the fixture.
//!
//! So leg A **plants a canary package** in a directory this example created
//! (`vendor/autoload.php` registering `rexenv-canary` at `before_wp_load`, so it
//! needs no WordPress) and REQUIRES that WP-CLI resolves it when pointed there.
//! If it does not, the canary is broken and this example says so and stops —
//! **it does not report the pin as proven**. A control that silently fails turns
//! the whole check into "nothing resolved either way", which is green and
//! vacuous: the shape this project keeps catching one layer down, here one layer
//! up.
//!
//! Then, with `WP_CLI_PACKAGES_DIR` exported into this process — the shape a
//! user's login shell really has — legs B and C run the SAME canary through the
//! two production spawn paths and require `not a registered wp command`.
//!
//! # What a green run proves, and what it does not
//!
//! Proves: under the real pinned phar, a package that WOULD have resolved does
//! not, through both the captured and the streamed path, with the user's own
//! `WP_CLI_PACKAGES_DIR` exported and losing to the pin — and that the tell (#301)
//! fires off the phar's REAL error text rather than a fixture string.
//!
//! Does not prove: anything about a package that hooks WP-CLI differently from
//! `WP_CLI::add_command` (a `before_invoke` hook, a bootstrap step, a command
//! that replaces a core one); anything about `core/terminal.rs`'s `wp` wrapper,
//! which is deliberately ambient and outside the claim; and nothing about
//! machines that are not this one.
//!
//! # Fixture ownership
//!
//! The packages dir is created by this example under the temp dir and removed by
//! a Drop guard. `common::sandbox` supplies the Platform; binaries come from the
//! shared cache (the documented exception), which means production's own
//! `.rexenv-no-wp-packages` pin file is created beside the real phar — the same
//! 0-byte file the app writes on its first wp command, and nothing else real is
//! touched.

mod common;

use rexenv_lib::core::{binaries, php, repo, wordpress, wp_packages};
use std::path::{Path, PathBuf};

/// A packages dir this example owns, removed however the run ends.
struct Canary {
    dir: PathBuf,
}

impl Drop for Canary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The command the canary registers. Named so that a stray hit in someone's real
/// output could only have come from here.
const CANARY_COMMAND: &str = "rexenv-canary";
const CANARY_MARKER: &str = "REXENV-CANARY-RESOLVED";
const NOT_REGISTERED: &str = "not a registered wp command";

fn plant_canary() -> Canary {
    let dir = std::env::temp_dir().join(format!("rexenv-canary-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("vendor")).expect("canary vendor dir");
    // WP-CLI includes `<packages dir>/vendor/autoload.php` when it is readable.
    // `before_wp_load` is what lets the canary answer without a WordPress
    // install — without it the command still RESOLVES but exits on "this does
    // not seem to be a WordPress installation", and the control leg could not
    // tell a resolved command from a missing one.
    std::fs::write(
        dir.join("vendor").join("autoload.php"),
        format!(
            "<?php\nWP_CLI::add_command(\n    '{CANARY_COMMAND}',\n    function () {{ WP_CLI::log( '{CANARY_MARKER}' ); }},\n    array( 'when' => 'before_wp_load' )\n);\n"
        ),
    )
    .expect("canary autoloader");
    // Production-shaped: the tell names packages from this file, so the fixture
    // has the one `wp package install` writes.
    std::fs::write(
        dir.join("composer.json"),
        r#"{
    "name": "wp-cli/wp-cli",
    "description": "Installed community packages used by WP-CLI",
    "require": {
        "rexenv/canary-command": "1.0.0"
    }
}"#,
    )
    .expect("canary composer.json");
    Canary { dir }
}

fn fail(step: &str, why: &str) -> ! {
    eprintln!("\n✗ {step}\n  {why}\n");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let (plat, _guard) = common::sandbox("wp-packages");
    let canary = plant_canary();

    let patch = php::patch_for_minor("8.3").expect("pinned 8.3");
    let php_bin = binaries::resolve(&*plat, "php", patch).await.expect("php");
    let wp_phar = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION)
        .await
        .expect("wp-cli phar");

    // ── A · THE CONTROL — the canary must RESOLVE without the pin ───────────
    //
    // Everything below is meaningless if this fails, so it is checked first and
    // reported as a BROKEN CONTROL rather than as a passing pin.
    let control = std::process::Command::new(&php_bin)
        .args(wordpress::wp_argv_prefix(&wp_phar))
        .arg(CANARY_COMMAND)
        .env("WP_CLI_PACKAGES_DIR", &canary.dir)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("spawn the control");
    let control_out = format!(
        "{}{}",
        String::from_utf8_lossy(&control.stdout),
        String::from_utf8_lossy(&control.stderr)
    );
    if !control_out.contains(CANARY_MARKER) {
        fail(
            "CONTROL LEG BROKEN — this run proves NOTHING about the pin",
            &format!(
                "The canary package did not resolve even with `WP_CLI_PACKAGES_DIR` pointed \
                 straight at it, so legs B and C would find `{CANARY_COMMAND}` missing for a \
                 reason that has nothing to do with rexenv — and would pass. Fix the canary \
                 before believing anything here.\n  Likely causes: WP-CLI stopped including \
                 `<packages dir>/vendor/autoload.php`, or `before_wp_load` is no longer how a \
                 command opts out of needing WordPress.\n  exit={:?}\n  output: {}",
                control.status.code(),
                control_out.trim()
            ),
        );
    }
    println!("A ok — the canary resolves when WP-CLI is pointed at it: {CANARY_MARKER}");

    // From here on the canary dir is EXPORTED into this process, which is the
    // shape a user's login shell really has. Every leg below therefore runs
    // against an environment that would resolve the canary if the pin were not
    // applied — which is what makes each of them a test of the pin rather than
    // of an empty machine.
    std::env::set_var("WP_CLI_PACKAGES_DIR", &canary.dir);

    // ── B · the CAPTURED path (`wp_cli` → `wp_command`) ─────────────────────
    let err = match wordpress::wp_cli_checked(&php_bin, &wp_phar, &[CANARY_COMMAND], None) {
        Ok(out) => fail(
            "B — the captured path RESOLVED the canary",
            &format!(
                "`wp {CANARY_COMMAND}` succeeded, so the exported packages dir reached wp-cli \
                 through `wordpress::wp_cli`. The pin is not applied on the captured path.\n  \
                 stdout: {}",
                out.trim()
            ),
        ),
        Err(e) => e.to_string(),
    };
    if !err.contains(NOT_REGISTERED) {
        fail(
            "B — the captured path failed for the WRONG reason",
            &format!(
                "expected `{NOT_REGISTERED}` (the pin working); got:\n  {}",
                err.trim()
            ),
        );
    }
    println!("B ok — captured path: the exported packages dir lost to the pin");

    // …and the TELL rode along on the phar's own words (#301). This is the half
    // no lib test can reach: the real error text, the real detection, appended.
    if !err.contains("rexenv/canary-command") || !err.contains("rexenv's terminal") {
        fail(
            "B — the failure carried no explanation",
            &format!(
                "the tell did not fire on a REAL `{NOT_REGISTERED}` failure, so a user loses a \
                 capability with nothing telling them why (#301).\n  got:\n  {}",
                err.trim()
            ),
        );
    }
    // Appended, not substituted: WP-CLI's own line has to survive alongside it.
    if !err.contains("Error:") {
        fail(
            "B — the tell REPLACED what wp-cli said",
            &format!("the phar's own line is the one a user searches for; got:\n  {}", err.trim()),
        );
    }
    println!("B ok — the tell is appended to the phar's own error, naming the package");

    // ── C · the STREAMED path (`wp_step_streamed` → `with_pinned_packages`) ──
    //
    // The other production spawn, and the one where the pin is a line in a list
    // rather than a `Command` that cannot be built without it.
    let docroot = canary.dir.join("docroot");
    std::fs::create_dir_all(&docroot).expect("fixture docroot");
    let cancel = repo::CancelToken::default();
    // The login-shell shape: the user's own value is IN the environment handed
    // to the spawn, and must lose.
    let env = vec![
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        ("HOME".to_string(), std::env::var("HOME").unwrap_or_default()),
        ("WP_CLI_PACKAGES_DIR".to_string(), canary.dir.display().to_string()),
    ];
    let stream = wordpress::WpStream { sup: plat.supervisor(), env: &env, cancel: &cancel };
    let mut lines: Vec<String> = Vec::new();
    let mut on_line = |l: &str| lines.push(l.to_string());
    let step = wordpress::wp_step_streamed(
        &stream,
        &php_bin,
        &wp_phar,
        &docroot,
        &[CANARY_COMMAND],
        &mut on_line,
    )
    .expect("streamed step ran");
    let streamed = lines.join("\n");
    if step.ok || !streamed.contains(NOT_REGISTERED) {
        fail(
            "C — the streamed path did not pin the environment it was handed",
            &format!(
                "a user's exported `WP_CLI_PACKAGES_DIR` reached wp-cli through \
                 `wp_step_streamed` (ok={}), which is every provisioning and blueprint step.\n  \
                 output: {}",
                step.ok,
                streamed.trim()
            ),
        );
    }
    println!("C ok — streamed path: the login-shell value lost to the pin");

    // ── D · the terminal wrapper is DELIBERATELY not pinned ─────────────────
    //
    // Asserted, not assumed. It is the exemption the tell's last sentence rests
    // on ("they still work in rexenv's terminal"), so if it ever became pinned
    // the copy would start lying and nothing else here would notice.
    let wrapper_dir =
        rexenv_lib::core::terminal::ensure_wp_wrapper(&*plat, &php_bin, &wp_phar)
            .expect("wp wrapper");
    let wrapper = wrapper_dir.join("wp");
    let script = std::fs::read_to_string(&wrapper).expect("read the wrapper");
    if script.contains("WP_CLI_PACKAGES_DIR") {
        fail(
            "D — the terminal wrapper is pinned",
            "the user's own `wp` was pinned too. That breaks `wp package install` from inside \
             rexenv in a way that looks like our bug, and it makes the tell's \"they still work \
             in rexenv's terminal\" false (D1, 13 Aug 2026).",
        );
    }
    let via_wrapper = std::process::Command::new(&wrapper)
        .arg(CANARY_COMMAND)
        .env("WP_CLI_PACKAGES_DIR", &canary.dir)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("spawn the wrapper");
    let wrapper_out = format!(
        "{}{}",
        String::from_utf8_lossy(&via_wrapper.stdout),
        String::from_utf8_lossy(&via_wrapper.stderr)
    );
    if !wrapper_out.contains(CANARY_MARKER) {
        fail(
            "D — the terminal wrapper stopped honouring the user's packages",
            &format!(
                "the sentence the Settings card promises is no longer true.\n  output: {}",
                wrapper_out.trim()
            ),
        );
    }
    println!("D ok — the terminal wrapper still runs the user's own packages");

    // ── E · the pin is a FILE, so nothing can turn it into a packages dir ────
    //
    // Two halves, because the first version of this leg had only the second and
    // PASSED a planted `create_dir_all`: the real pin file already existed from
    // an earlier run, so the check read sticky state and never exercised
    // creation. A guard that can only observe what a previous run left behind is
    // not testing this run.
    let fresh_phar = canary.dir.join("bin").join("wp-cli-2.12.0").join("wp-cli.phar");
    std::fs::create_dir_all(fresh_phar.parent().expect("phar dir")).expect("fixture bin dir");
    std::fs::write(&fresh_phar, b"#!/usr/bin/env php\n").expect("fixture phar");
    let fresh_pin = wp_packages::neutral_packages_path(&fresh_phar);
    if !fresh_pin.is_file() || std::fs::create_dir_all(fresh_pin.join("vendor")).is_ok() {
        fail(
            "E — a freshly created pin is not a file",
            &format!(
                "{} can be turned into a real packages dir, so the first `wp package install` \
                 an agent sends through the raw runner would unpin the command set silently.",
                fresh_pin.display()
            ),
        );
    }
    // …and the one production actually wrote beside the real phar during this
    // run, which is the half the fixture cannot speak for.
    let pin = wp_packages::neutral_packages_path(&wp_phar);
    if !pin.is_file() {
        fail(
            "E — production did not leave a pin file beside the phar",
            &format!("{} is missing or is not a regular file", pin.display()),
        );
    }
    println!("E ok — the pin is a file, on a fresh path and at {}", short(&pin));

    println!("\n✓ wp_packages_check: the command set is pinned on both production paths, the \
              user's exported dir loses, the failure explains itself, and the terminal is \
              still theirs");
}

fn short(p: &Path) -> String {
    match std::env::var("HOME") {
        Ok(h) if p.starts_with(&h) => format!("~{}", &p.display().to_string()[h.len()..]),
        _ => p.display().to_string(),
    }
}
