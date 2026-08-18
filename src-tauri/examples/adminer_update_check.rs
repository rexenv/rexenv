//! Live check: the in-app ADMINER update path, end to end, against the REAL
//! signed manifest. Run: `cargo run --example adminer_update_check`
//!
//! # What this proves that L0 cannot
//!
//! `core::updates` tests the family rules against generated keys and captured
//! fixtures; `core::adminer` tests the binding probe against hand-built Adminers.
//! None of that touches the network, the pinned key, or a byte of real Adminer.
//! What is unproven until something does this is the chain a button press runs:
//!
//!   fetch the published manifest → verify it against the COMPILED-IN key →
//!   find what the Adminer FAMILY offers → resolve a version this build was
//!   never made with → download it → have the existing digest gate accept the
//!   manifest's digest → and then the step PHP has no equivalent of: RUN it and
//!   prove rexenv's login gate and frame protections still have something to
//!   hang on.
//!
//! SANDBOXED: `common::sandbox` paths and a sandbox database, so no real
//! selection is written and no real docroot is restaged. The BINARY CACHE is
//! deliberately the real one — the same documented hole as `php_update_check`,
//! and here it is the point: the download has to land where an apply would put
//! it.

use rexenv_lib::core::{adminer, binaries, updates};
use std::process::ExitCode;

mod common;

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _sandbox) = common::sandbox("adminer_upd");
    let mut checks = common::Check::new("adminer_update_check");
    let pinned = binaries::ADMINER_VERSION;
    println!("this build pins Adminer {pinned} (ceiling: major <= {})\n", updates::ADMINER_MAX_MAJOR);

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
    let offered = catalog.newer_than(updates::Family::Adminer, pinned, arch);
    // Both Macs must get the same answer: one file, one arch-free row.
    let other = if arch == "arm64" { "x86_64" } else { "arm64" };
    checks.is(
        "the Adminer row is arch-free — both Macs are offered the same version",
        offered == catalog.newer_than(updates::Family::Adminer, pinned, other),
        "an arch-free artifact answered differently per arch",
    );
    // A PHP version must never answer for the Adminer family, or vice versa.
    checks.is(
        "the families do not answer for each other",
        catalog
            .newer_than(updates::Family::Php, pinned, arch)
            .is_none(),
        "the PHP family offered something for an Adminer version string",
    );

    let Some(target) = offered else {
        println!(
            "\nthe manifest offers nothing newer than {pinned} for Adminer — nothing to apply.\n\
             That is a legitimate state, not a failure."
        );
        return checks.verdict();
    };
    println!("manifest offers Adminer {target}\n");
    binaries::install_catalog(catalog);

    // The digest gate, against a number that arrived over the network.
    let file = match binaries::resolve_file(&*plat, "adminer", &target).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("resolving adminer {target} FAILED: {e}");
            eprintln!("a digest mismatch here means the manifest and the artifact disagree.");
            return ExitCode::FAILURE;
        }
    };
    checks.is("a manifest-only Adminer resolves (download + digest verified)", true, "");
    println!("file={}", file.display());

    // It is a SCRIPT: resolve_file must not have made it executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        checks.is(
            "the downloaded Adminer is not marked executable",
            mode & 0o111 == 0,
            "resolve_file chmod-ed a PHP script",
        );
    }

    // THE STEP PHP HAS NO EQUIVALENT OF. A digest match proves we got bytes
    // somebody signed for; it says nothing about whether rexenv's login gate and
    // frame protections still have a hook to hang on.
    let php = match binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("resolving php FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };
    match adminer::verify_pair(&php, &file) {
        Ok(()) => checks.is(
            &format!("Adminer {target} still binds to rexenv's wrapper"),
            true,
            "",
        ),
        Err(e) => checks.is(
            &format!("Adminer {target} still binds to rexenv's wrapper"),
            false,
            &format!("{e}"),
        ),
    }

    // The selection, and the FLOOR — against the SANDBOX database.
    adminer::set_selected_version(&conn, Some(&target)).expect("record the selection");
    checks.is(
        "a recorded selection becomes the effective version",
        adminer::effective_version(&*plat, &conn) == target,
        "the docroot would still stage the pin",
    );
    adminer::set_selected_version(&conn, Some("0.0.1")).unwrap();
    checks.is(
        "a selection older than the pin is ignored (the pin is a FLOOR)",
        adminer::effective_version(&*plat, &conn) == pinned,
        "a stale selection held the console below the version this build ships",
    );
    adminer::set_selected_version(&conn, None).unwrap();
    checks.is(
        "clearing the selection returns to the pin",
        adminer::effective_version(&*plat, &conn) == pinned,
        "the selection could not be cleared",
    );

    println!(
        "\nNOT covered by this check, and it is the remaining L3 leg:\n  \
         that Adminer still CALLS the methods it still declares. Reflection cannot see a\n  \
         call site that is gone, so a build keeping `csp()` and no longer consulting it\n  \
         passes here while `headers()` strips X-Frame-Options — clickjacking on a database\n  \
         console. That is docs/SMOKE-TEST.md's step: open the console and look.\n"
    );
    checks.verdict()
}
