//! Live check: the in-app PHPMYADMIN update path, end to end, against the REAL
//! signed manifest. Run: `cargo run --example phpmyadmin_update_check`
//!
//! # What this proves that L0 cannot
//!
//! `core::updates` tests the family rules against generated keys and captured
//! fixtures; `core::phpmyadmin` tests the generator and the floor against hand-built
//! trees. None of that touches the network, the pinned key, or a byte of real
//! phpMyAdmin. What is unproven until something does this is the chain the console
//! card's Update button runs (`phpmyadmin_update_apply`):
//!
//!   fetch the published manifest → verify it against the COMPILED-IN key →
//!   find what the phpMyAdmin FAMILY offers → resolve a version this build was
//!   never made with → download the zip → have the existing digest gate accept the
//!   manifest's digest → extract it as a script tree → then the step a digest
//!   cannot answer: COPY it beside a generated config and PROBE it on the pinned
//!   PHP (`phpmyadmin::ensure_from`), and only then call it the docroot.
//!
//! SANDBOXED: `common::sandbox` paths and a sandbox database, so no real
//! selection is written and no real docroot is restaged. The BINARY CACHE is
//! deliberately the real one — the same documented hole as `php_update_check`,
//! and here it is the point: the download has to land where an apply would put it.
//!
//! "The manifest offers nothing newer than the pin" is a legitimate end, not a
//! failure: the day after a release re-pins the newest version, this is the
//! ordinary state. The floor/selection legs run either way, against the sandbox.

use rexenv_lib::core::{binaries, phpmyadmin, updates};
use std::process::ExitCode;

mod common;

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _sandbox) = common::sandbox("phpmyadmin_upd");
    let mut checks = common::Check::new("phpmyadmin_update_check");
    let pinned = binaries::pins().phpmyadmin;
    println!("this build pins phpMyAdmin {pinned} (ceiling: major <= {})\n", updates::PHPMYADMIN_MAX_MAJOR);

    checks.is(
        "this build has an update key pinned",
        updates::enabled(),
        "RELEASE_PUBKEY is empty — the button cannot appear, so there is nothing to check",
    );
    if !updates::enabled() {
        return checks.verdict();
    }

    let (doc, sig) = match updates::fetch().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("could not fetch the published manifest: {e}");
            return ExitCode::FAILURE;
        }
    };
    let conn = common::sandbox_db(&*plat);
    let catalog = match updates::accept(&conn, &doc, sig.trim()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("the published manifest did NOT verify against the pinned key: {e}");
            return ExitCode::FAILURE;
        }
    };
    checks.is("the published manifest verifies against the compiled-in key", true, "");

    let arch = updates::catalog_arch(plat.binaries().arch());
    let offered = catalog.newer_than(updates::Family::PhpMyAdmin, pinned, arch);
    let other = if arch == "arm64" { "x86_64" } else { "arm64" };
    checks.is(
        "the phpMyAdmin row is arch-free — both arches are offered the same version",
        offered == catalog.newer_than(updates::Family::PhpMyAdmin, pinned, other),
        "an arch-free artifact answered differently per arch",
    );
    // The offer comes from phpMyAdmin's OWN rows. A version string is not typed per
    // family — ask the Adminer family about "5.2.2" and it will happily offer its 6.1.2,
    // because its rule is "any newer version within the ceiling" (found on the first real
    // run, 15 Oct 2026: this leg once asserted the Adminer family answers NOTHING here and
    // failed against the live document). The property that matters: what the phpMyAdmin
    // family offers resolves as a `phpmyadmin` row, never as another family's, and the PHP
    // family (minor-tracked) offers nothing for a phpMyAdmin version.
    checks.is(
        "the phpMyAdmin offer is a phpmyadmin row, and PHP offers nothing for it",
        offered.as_deref().map_or(true, |v| catalog.artifact("phpmyadmin", v, updates::ANY_ARCH).is_some())
            && catalog.newer_than(updates::Family::Php, pinned, arch).is_none(),
        "the offer is not a phpmyadmin row, or the PHP family answered for a phpMyAdmin version",
    );

    // The selection and the FLOOR — against the SANDBOX database, with or without
    // an offer (an unvouched selection is the pin; so is one below it).
    phpmyadmin::set_selected_version(&conn, Some("0.0.1")).unwrap();
    checks.is(
        "a selection older than the pin is ignored (the pin is a FLOOR)",
        phpmyadmin::effective_version(&*plat, &conn) == pinned,
        "a stale selection held the console below the version this build ships",
    );
    phpmyadmin::set_selected_version(&conn, Some("99.0.0")).unwrap();
    checks.is(
        "a selection nothing vouches for reads as the pin",
        phpmyadmin::effective_version(&*plat, &conn) == pinned,
        "an unvouched selection became the effective version",
    );
    phpmyadmin::set_selected_version(&conn, None).unwrap();
    checks.is(
        "clearing the selection returns to the pin",
        phpmyadmin::effective_version(&*plat, &conn) == pinned,
        "the selection could not be cleared",
    );

    let Some(target) = offered else {
        println!(
            "\nthe manifest offers nothing newer than {pinned} for phpMyAdmin — nothing to apply.\n\
             That is a legitimate state, not a failure."
        );
        return checks.verdict();
    };
    println!("manifest offers phpMyAdmin {target}\n");
    binaries::install_catalog(catalog);

    // The digest gate, against a number that arrived over the network, and the
    // script-tree extraction (no executable is looked for — `prepare_script_dir`).
    let tree = match binaries::resolve_dir(&*plat, "phpmyadmin", &target).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("resolving phpmyadmin {target} FAILED: {e}");
            eprintln!("a digest mismatch here means the manifest and the artifact disagree.");
            return ExitCode::FAILURE;
        }
    };
    checks.is("a manifest-only phpMyAdmin resolves (download + digest verified + extracted)", true, "");
    checks.is(
        "the resolved tree is a phpMyAdmin tree",
        tree.join("index.php").is_file() && tree.join("vendor/autoload.php").is_file(),
        "no index.php / vendor/autoload.php under the extracted tree",
    );
    println!("tree={}", tree.display());

    // THE STEP A DIGEST CANNOT ANSWER: copy, generate, probe on the pinned PHP, publish
    // — into the SANDBOX app data. A tree that fails never becomes the docroot (#866).
    let php = match binaries::resolve_program(&*plat, "php", binaries::pins().php).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("resolving php FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };
    // The pinned dark theme goes in beside it, as `ensure` does — and the probe checks
    // it still supports the OFFERED tree's series (the gate that catches a tree bump
    // outrunning the theme pin).
    let theme = match binaries::resolve_dir(&*plat, "phpmyadmin-theme", binaries::pins().phpmyadmin_theme).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("resolving the dark theme FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };
    match phpmyadmin::ensure_from(&*plat, &target, &tree, Some(&theme), &php) {
        Ok(dir) => {
            checks.is(
                &format!("phpMyAdmin {target} runs on the bundled PHP with rexenv's generated config"),
                phpmyadmin::staged_version(&*plat).as_deref() == Some(target.as_str())
                    && dir.join(phpmyadmin::CONFIG_FILE).is_file(),
                "the stage returned Ok but the marker or the config is not there",
            );
        }
        Err(e) => checks.is(
            &format!("phpMyAdmin {target} runs on the bundled PHP with rexenv's generated config"),
            false,
            &format!("{e}"),
        ),
    }

    phpmyadmin::set_selected_version(&conn, Some(&target)).expect("record the selection");
    checks.is(
        "a recorded, vouched selection becomes the effective version",
        phpmyadmin::effective_version(&*plat, &conn) == target,
        "the next stage would still copy the pin",
    );
    // The sweep's rule, against the real cache: with the offer effective, the PIN's
    // tree is still kept (the revert works offline) and the offer's is too.
    let keep = vec![pinned.to_string(), target.clone()];
    checks.is(
        "the cache sweep keeps both the pin and the effective tree",
        !binaries::is_outdated_phpmyadmin_cache(&format!("phpmyadmin-{pinned}"), &keep)
            && !binaries::is_outdated_phpmyadmin_cache(&format!("phpmyadmin-{target}"), &keep),
        "the sweep would delete a tree the console needs",
    );
    phpmyadmin::set_selected_version(&conn, None).unwrap();

    println!(
        "\nNOT covered by this check: that the UPDATED console renders logged in through the\n  \
         proxy — that is `phpmyadmin_check`'s real-stack leg after `rex db console phpmyadmin`,\n  \
         and docs/SMOKE-TEST.md's row (open it and look).\n"
    );
    checks.verdict()
}
