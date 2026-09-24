//! Live check: the app's OWN update descriptor, fetched and verified against the
//! REAL published document and the COMPILED-IN key.
//! Run: `cargo run --example app_update_check`
//!
//! # What this proves that L0 cannot
//!
//! `core::app_update`'s lib tests drive the signature, the serial rule and every
//! offer rule through a generated keypair — none of which touches the network,
//! the pinned key, or a document anyone actually published. What is unproven
//! until something does this is the half a user's "Check now" actually runs:
//!
//!   fetch two files over TLS → verify the signature against the key compiled
//!   into THIS binary → accept it under the serial rule → decide, live, whether
//!   this build on this Mac has anything to install.
//!
//! That chain is where two correct-looking halves disagree: a publisher signing
//! bytes that differ from the ones it committed, a document whose fields parse
//! but whose version is not offerable, a key rotated on one side only. Each of
//! those passes every L0 test in the tree.
//!
//! # Before the first publish, "nothing is published yet" is a PASS
//!
//! The descriptor does not exist until the release flow publishes one (T9/T10 of
//! `docs/archive/PLAN-self-update.md`). Until then this check reports that plainly and
//! exits 0 — the same shape `php_update_check` uses for "nothing to apply". A
//! check that failed loudly for a document nobody has written yet would be
//! turned off, and then it would be off on the day it mattered.
//!
//! SANDBOXED: `common::sandbox` paths and a sandbox database, so the serial
//! high-water mark and the check cache of the developer's REAL install are never
//! written. Nothing is downloaded and nothing is installed: this example is the
//! DESCRIPTOR half. The swap is `app_bundle_swap_check` (T3) and the real
//! end-to-end update is SMOKE-TEST / PUBLISH-TESTING §M (T11).

use rexenv_lib::core::{app_update, macho, updates};
use rexenv_lib::state::store;
use std::process::ExitCode;

mod common;

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _sandbox) = common::sandbox("app_upd");
    let mut checks = common::Check::new("app_update_check");
    let running = env!("CARGO_PKG_VERSION");
    println!("this build: {running}\n");

    // ── 1. A key must be pinned, or nothing below can mean anything ──────────
    checks.is(
        "this build has an update key pinned",
        app_update::enabled(),
        "the key const is empty — the card does not render, so there is nothing to check",
    );
    if !app_update::enabled() {
        return checks.verdict();
    }
    checks.is(
        "the app descriptor rides the SAME key as the PHP manifest",
        updates::enabled() == app_update::enabled(),
        "one key, one ceremony, one rotation — two would be two custody stories",
    );

    // ── 2. Fetch the real thing ──────────────────────────────────────────────
    //
    // The INTERACTIVE deadline, because a person is running this example and
    // watching it — the same choice the "Check now" button makes, so what this
    // measures is what a user's press measures.
    let started = std::time::Instant::now();
    // The platform names which descriptor this build reads (Linux has one per package
    // kind and arch; macOS and Windows have one) — the same question the button asks.
    let variant = plat.app_bundle().descriptor_variant();
    let fetched = app_update::fetch(variant.as_deref(), updates::INTERACTIVE_DEADLINE).await;
    // The deadline is a promise about the WORST case, so bound the check on it
    // whichever way the fetch went: the failure this guards (#540) was a fetch
    // that never returned at all, and a check that only looks at the happy path
    // would have passed straight through it.
    checks.is(
        "the fetch answered within the deadline a button is allowed to wait",
        started.elapsed() <= updates::INTERACTIVE_DEADLINE + std::time::Duration::from_secs(2),
        &format!(
            "took {:?}, and the button's ceiling is {:?} — reqwest's own client timeout \
             was trusted here once and did not fire",
            started.elapsed(),
            updates::INTERACTIVE_DEADLINE
        ),
    );
    let (doc, sig) = match fetched {
        Ok(pair) => pair,
        Err(e) => {
            // A 404 is the pre-publish state and a PASS; anything else — DNS,
            // TLS, a 500, a timeout — is a real failure and must not be filed
            // under "not published yet", which is the shape that turns a check
            // into a thing that always passes.
            // Matched against the SENTENCE the user reads, not against "404":
            // the number stopped appearing when those messages were written for
            // people rather than for logs (ledger #540), and a check keyed to a
            // string the product no longer produces is a check that silently
            // always fails — or, worse here, always passes.
            let missing = e
                .to_string()
                .contains(&updates::FetchFailure::Status(404).message());
            checks.is(
                "the descriptor is either published or cleanly absent",
                missing,
                &format!("the fetch failed for a reason that is not 'unpublished': {e}"),
            );
            if missing {
                println!(
                    "\nno descriptor is published yet ({}) — this is the state before the \n\
                     first release that carries one. Nothing further to check.",
                    app_update::APP_MANIFEST_URL
                );
            }
            return checks.verdict();
        }
    };
    println!("descriptor: {} bytes, signature {} chars", doc.len(), sig.trim().len());

    // ── 3. Verify against the COMPILED-IN key ────────────────────────────────
    //
    // The point of the whole example: the bytes on the server, checked against
    // the key in this binary. A publisher that signed with a rotated key fails
    // HERE and nowhere else in the tree.
    let verified = app_update::verify(&doc, sig.trim());
    checks.is(
        "the published descriptor verifies against the key compiled into this build",
        verified.is_ok(),
        &match &verified {
            Ok(_) => String::new(),
            Err(e) => format!("{e} — the publisher and this build disagree about the key"),
        },
    );
    let Ok(manifest) = verified else { return checks.verdict() };
    println!(
        "serial {} · release {} · {} bytes",
        manifest.serial, manifest.release.version, manifest.release.size_bytes
    );

    // A tampered copy must be refused. Planted here rather than assumed, because
    // "the signature verified" and "the signature is checked" are different
    // claims and only one of them is worth having.
    let mut tampered = doc.clone();
    if let Some(b) = tampered.last_mut() {
        *b ^= 0x01;
    }
    checks.is(
        "one flipped byte of the published document is refused",
        app_update::verify(&tampered, sig.trim()).is_err(),
        "a tampered document verified — the check is not looking at these bytes",
    );

    // ── 4. Accept it into a SANDBOX database, then replay it ─────────────────
    let conn = common::sandbox_db(&*plat);
    let accepted = app_update::accept(&conn, &doc, sig.trim());
    checks.is(
        "the descriptor is accepted and its serial recorded",
        accepted.is_ok(),
        &format!("{accepted:?}"),
    );
    let stored = store::get_setting(&conn, app_update::SERIAL_KEY).ok().flatten();
    checks.is(
        "the high-water mark holds the published serial",
        stored.as_deref() == Some(manifest.serial.to_string().as_str()),
        &format!("stored {stored:?}, published {}", manifest.serial),
    );

    // The same document again is the ordinary state of every launch after the
    // first: accepted, and it writes nothing.
    checks.is(
        "re-accepting the same serial is a no-op, not a replay refusal",
        app_update::accept(&conn, &doc, sig.trim()).is_ok(),
        "the every-launch case must not put 'refusing a replay' in a user's log daily",
    );

    // ── 5. The offer rule, against this build and this Mac ───────────────────
    let host = macho::host_macos();
    checks.is(
        "this Mac's macOS version can be read",
        host.is_some(),
        "without it the floor cannot be checked, and the offer fails closed",
    );
    match app_update::offer_for(&manifest.release, running, host, None) {
        Ok(o) => println!("\noffer for THIS build: {} ({} bytes)", o.version, o.size_bytes),
        Err(no) => println!("\nnothing offered to THIS build: {}", no.reason()),
    }

    // Whatever the live answer is, the RULE is exercised in both directions
    // against the real document — an ancient running version must be offered it,
    // and a version above it must not be.
    let ancient = "0.0.1";
    checks.is(
        "a much older build is offered the published release",
        app_update::offer_for(&manifest.release, ancient, host, None).is_ok(),
        "the published document is not offerable to anything, which is a publisher bug",
    );
    let segs: Vec<u32> = updates::version_segments(&manifest.release.version);
    let above = format!("{}.{}.{}", segs[0], segs[1], segs[2] + 1);
    checks.is(
        "a build newer than the published release is offered nothing",
        app_update::offer_for(&manifest.release, &above, host, None).is_err(),
        "an offer to a newer build would be a downgrade the serial rule cannot see",
    );
    checks.is(
        "skipping the published version withdraws the offer, live",
        app_update::offer_for(&manifest.release, ancient, host, Some(&manifest.release.version))
            .is_err(),
        "the skip is compared to the offer, never stored as a flag",
    );

    // ── 6. What this does NOT cover ──────────────────────────────────────────
    println!(
        "\nNOT covered here, and it is the remaining work: downloading the artifact and\n\
         checking its digest (T4), staging and swapping a real bundle (T3,\n\
         app_bundle_swap_check), and the actual replace-and-relaunch on a real Mac,\n\
         which no tier can run — SMOKE-TEST and PUBLISH-TESTING §M."
    );
    checks.verdict()
}
