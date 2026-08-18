//! Live check: the in-app PHP update path, end to end, against the REAL signed
//! manifest. Run: `cargo run --example php_update_check [<minor>]`
//!
//! Default minor is 8.2 because it is the one this project's own install has no
//! site on — the pool can be restarted without interrupting anything. Pass
//! another to check it instead.
//!
//! # What this proves that L0 cannot
//!
//! `core::updates` tests the signature, the serial rule and the structural limits
//! against generated keys and captured fixtures. None of that touches the network,
//! the pinned key, or a byte of real PHP. What is unproven until something does
//! this is the whole chain a user's button press actually runs:
//!
//!   fetch the published manifest → verify it against the COMPILED-IN key →
//!   resolve a patch the app was never built with → download it → have the
//!   EXISTING digest gate accept the manifest's digest → prepare the Mach-O →
//!   run it → serve FastCGI from it → and put the selection back.
//!
//! Every one of those is a place two correct-looking halves disagree. The digest
//! gate in particular has only ever compared against a compiled-in `const`; this
//! is the first time it compares against a number that arrived over the network.
//!
//! SANDBOXED: `common::sandbox` paths, a sandbox database (so a real
//! `selected_patch` is never written by a test), a fixture port, and a `Reaped`
//! guard on the master. The BINARY CACHE is deliberately the real one — that is
//! the shared hole `examples/common/mod.rs` documents, and here it is the point:
//! the download has to land where a real apply would put it.

use rexenv_lib::core::{binaries, php, services, updates};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Duration;

mod common;
use common::Reaped;

const FPM_PORT: u16 = 9793; // fixture — never `services::PHP_FPM_PORT` or a real pool port

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _sandbox) = common::sandbox("php_upd");
    let mut checks = common::Check::new("php_update_check");
    let minor = std::env::args().nth(1).unwrap_or_else(|| "8.2".to_string());
    let pinned = match php::patch_for_minor(&minor) {
        Some(p) => p.to_string(),
        None => {
            eprintln!("{minor} is not a minor this build ships");
            return ExitCode::FAILURE;
        }
    };
    println!("minor under test: {minor} (this build pins {pinned})\n");

    // ── 1. The key must be pinned, or nothing below can mean anything ─────────
    checks.is(
        "this build has an update key pinned",
        updates::enabled(),
        "RELEASE_PUBKEY is empty — the button cannot appear, so there is nothing to check",
    );
    if !updates::enabled() {
        return checks.verdict();
    }

    // ── 2. Fetch and verify the REAL published manifest ───────────────────────
    let (doc, sig) = match updates::fetch().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("could not fetch the published manifest: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("fetched manifest: {} bytes, signature {} chars", doc.len(), sig.trim().len());
    let conn = common::sandbox_db(&*plat);
    let catalog = match updates::accept(&conn, &doc, sig.trim()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("the published manifest did NOT verify against the pinned key: {e}");
            return ExitCode::FAILURE;
        }
    };
    checks.is(
        "the published manifest verifies against the compiled-in key",
        true,
        "",
    );
    // Re-reading the SAME document is the ordinary state on every launch after
    // the first — it must be a no-op, not an error, and must not rewrite what is
    // stored. A genuine replay is an OLDER serial, which L0 covers against a
    // generated key (there is only ever one live serial to fetch here).
    let before = rexenv_lib::state::store::get_setting(&conn, "php_update_manifest_serial")
        .ok()
        .flatten();
    checks.is(
        "re-reading our own current manifest is accepted, not called a replay",
        updates::accept(&conn, &doc, sig.trim()).is_ok(),
        "the live manifest was refused on a second read — that sentence would land in a \
         user's log at every launch",
    );
    checks.is(
        "…and it rewrote nothing",
        rexenv_lib::state::store::get_setting(&conn, "php_update_manifest_serial").ok().flatten()
            == before,
        "a same-serial read displaced the stored serial",
    );

    let offered = catalog.newer_than(updates::Family::Php, &pinned, updates::catalog_arch(plat.binaries().arch()));
    let Some(target) = offered else {
        println!(
            "\nthe manifest offers nothing newer than {pinned} for {minor} — nothing to apply.\n\
             That is a legitimate state (static-php.dev trails php.net), not a failure."
        );
        return checks.verdict();
    };
    println!("manifest offers {target} for {minor}\n");
    binaries::install_catalog(catalog);

    // ── 3. Resolve it — the digest gate now compares against a NETWORK number ─
    // `resolve` streams, hashes in flight and compares to `spec.checksum`. Until
    // now that checksum has only ever come from a compiled-in const.
    let fpm = match binaries::resolve(&*plat, "php-fpm", &target).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("resolving php-fpm {target} FAILED: {e}");
            eprintln!("a digest mismatch here means the manifest and the artifact disagree.");
            return ExitCode::FAILURE;
        }
    };
    let cli = match binaries::resolve(&*plat, "php", &target).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("resolving php (cli) {target} FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };
    checks.is("a manifest-only patch resolves (download + digest verified)", true, "");
    println!("FPM={}", fpm.display());

    // ── 4. The bytes are REALLY that version, and they run ────────────────────
    // A digest match proves we got the bytes somebody signed for. It does not
    // prove they are the version the manifest CLAIMS, and nothing upstream of
    // here checks that: a mislabelled entry would pass every gate above.
    let v = Command::new(&cli).arg("-v").output();
    let reported = v
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").to_string())
        .unwrap_or_default();
    println!("php -v: {reported}");
    checks.is(
        &format!("the downloaded interpreter reports {target}"),
        reported.contains(&target),
        &format!("it reported {reported:?} — the manifest entry names a version these bytes are not"),
    );
    // mysqli is what WordPress needs; a patch bump that silently dropped it would
    // serve a broken site rather than fail to start.
    let mods = Command::new(&cli).arg("-m").output();
    let has_mysqli = mods
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == "mysqli"))
        .unwrap_or(false);
    checks.is("the updated build still has mysqli", has_mysqli, "WordPress cannot run without it");

    // ── 5. It serves FastCGI, on a fixture port ───────────────────────────────
    let conf = match services::write_fpm_config(&*plat, &minor, FPM_PORT, None, &[]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("writing the pool config failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = services::test_fpm_config(&*plat, &fpm, &conf) {
        eprintln!("`php-fpm -t` REJECTED the generated config on {target}: {e}");
        return ExitCode::FAILURE;
    }
    checks.is("php-fpm -t accepts the generated config on the new patch", true, "");

    let child = match services::start_fpm(&*plat, &fpm, &conf) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("starting the pool failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let _reaped = Reaped::new(child, FPM_PORT, "php-fpm");
    let mut up = false;
    for _ in 0..40 {
        if services::fpm_running(FPM_PORT) {
            up = true;
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    checks.is(
        &format!("a pool on {target} answers on {FPM_PORT}"),
        up,
        "the master started but never accepted a connection",
    );

    // ── 6. The selection, and the FLOOR ───────────────────────────────────────
    // Written to the SANDBOX database: a check must not leave a real install on a
    // patch nobody pressed a button for.
    php::seed_registry(&conn).expect("seed the sandbox registry");
    rexenv_lib::state::store::set_php_selected_patch(&conn, &minor, Some(&target))
        .expect("record the selection");
    checks.is(
        "a recorded selection becomes the minor's effective patch",
        php::effective_patch(&conn, &minor).unwrap().as_deref() == Some(target.as_str()),
        "the pool would still have started the pin",
    );
    // An OLDER selection must be ignored — the pin is a floor, not a default.
    let stale = format!("{minor}.0");
    rexenv_lib::state::store::set_php_selected_patch(&conn, &minor, Some(&stale)).unwrap();
    checks.is(
        "a selection older than the pin is ignored (the pin is a FLOOR)",
        php::effective_patch(&conn, &minor).unwrap().as_deref() == Some(pinned.as_str()),
        "a stale selection held the minor below the patch this build ships",
    );
    // And the revert the apply path performs on a failed restart.
    rexenv_lib::state::store::set_php_selected_patch(&conn, &minor, None).unwrap();
    checks.is(
        "clearing the selection reverts to the pin",
        php::effective_patch(&conn, &minor).unwrap().as_deref() == Some(pinned.as_str()),
        "the revert path cannot restore the previous patch",
    );

    // ── 7. What this did NOT do, said out loud ────────────────────────────────
    println!(
        "\nNOT covered by this check, and it is the remaining L3 leg:\n  \
         the LIVE pool swap on the production port — stop the running {minor} master, start\n  \
         the new one on {}, and revert if it does not come back. That needs the real\n  \
         ServiceManager and a deliberately broken tree; it is docs/SMOKE-TEST.md's step.\n",
        php::fpm_port(&minor).unwrap_or(0)
    );
    checks.verdict()
}
