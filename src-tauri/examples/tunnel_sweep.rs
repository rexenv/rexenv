//! Live check — the tunnel launch sweep settles crash-survivor rows correctly
//! (lifecycle ruling 28 Jul 2026, `docs/archive/PLAN-tunnel-lifecycle.md`).
//!
//! Three recorded rows, three fates:
//!   1. FOREIGN — the row's pid is alive but names an unrelated process (the
//!      recycled-pid case). The sweep must NOT kill it, and must still remove
//!      the mu-plugin and the row. This is the branch that would be silently
//!      wrong in either direction.
//!   2. OURS — the pid's argv carries the full identity (app-data binary
//!      path + exact `--http-host-header <domain>` pair). The sweep must
//!      kill it and clean up.
//!   3. DEAD — the pid doesn't exist. Cleanup only, no signal.
//!
//! Fixture-owned throughout (see `common`): sandbox platform + DB, docroots
//! under the sandbox root, spawned processes held by `Reaped` drop guards.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use rexenv_lib::core::tunnels;
use rexenv_lib::state::{db, store};

/// A fixture docroot with a planted tunnel mu-plugin, under the sandbox root.
fn docroot_with_muplugin(root: &Path, tag: &str) -> PathBuf {
    let docroot = root.join(format!("docroot-{tag}"));
    let mu = docroot.join("wp-content").join("mu-plugins");
    std::fs::create_dir_all(&mu).expect("create fixture mu-plugins dir");
    std::fs::write(mu.join("rexenv-tunnel.php"), "<?php /* fixture */")
        .expect("plant fixture mu-plugin");
    docroot
}

fn muplugin_exists(docroot: &Path) -> bool {
    docroot.join("wp-content/mu-plugins/rexenv-tunnel.php").is_file()
}

/// `ps` state of a pid: empty = gone, leading `Z` = zombie (dead, unreaped).
fn state_of(pid: u32) -> String {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "state="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Stage a stand-in `cloudflared` at `path` — a SYMLINK to `/bin/bash`, never a
/// copy.
///
/// Both halves of this are load-bearing, and both were wrong until 3 Aug 2026:
///
/// - **Symlink, not copy.** A copy of a system binary off the sealed system
///   volume is SIGKILLed on launch (exit 137, AMFI) on this machine, so every
///   fake below was dying instantly. The symlink executes the SSV image in place
///   — so it lives — while `argv[0]`, which is what `pids_named` and the
///   ownership check read, is still the path we chose.
/// - **The caller must pass a command bash cannot exec-optimise away.** `-c
///   "sleep 300"` makes bash `exec` sleep as its last command, replacing argv
///   with `sleep 300` — so the `cloudflared` name and the `--http-host-header`
///   pair, the exact things the sweep matches on, disappear. `"sleep 300; :"`
///   keeps bash resident with its original argv.
///
/// Why this mattered more than a flaky fixture: with the fakes dying on their
/// own, "ours must be KILLED" passed VACUOUSLY (the OS had already killed them)
/// and "a lookalike must SURVIVE" passed only when the check won the race. The
/// example was asserting almost nothing about `sweep_rowless` in either
/// direction — and it stands behind a Tier-1 blast-radius claim (never killing a
/// process that isn't provably ours).
fn stage_fake(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    std::os::unix::fs::symlink("/bin/bash", path).expect("stage the fake cloudflared");
}

fn main() {
    let (plat, sandbox) = common::sandbox("tunnel-sweep");
    let conn = db::open_for_platform(plat.paths()).expect("open sandbox database");

    // 1) FOREIGN: a live process that is not ours — models a recycled pid.
    let foreign_doc = docroot_with_muplugin(sandbox.root(), "foreign");
    let foreign_child = Command::new("/bin/sleep").arg("300").spawn().expect("spawn sleep");
    let mut foreign = common::Reaped::new(foreign_child, 39997, "sleep");
    assert!(store::try_claim_tunnel(
        &conn,
        "sweep-foreign.rex",
        foreign.id(),
        &foreign_doc.to_string_lossy()
    )
    .expect("claim foreign row"));

    // 2) OURS: a stand-in with the full identity — a copy of /bin/bash named
    //    `cloudflared` under the SANDBOX app-data (the ownership marker), its
    //    argv carrying the exact --http-host-header pair. Sleeps like a real
    //    wedged tunnel would.
    let fake_bin_dir = plat.paths().app_data_dir().expect("sandbox app data").join("bin-fixture");
    std::fs::create_dir_all(&fake_bin_dir).expect("create fixture bin dir");
    let fake_cloudflared = fake_bin_dir.join("cloudflared");
    stage_fake(&fake_cloudflared);
    let ours_doc = docroot_with_muplugin(sandbox.root(), "ours");
    let ours_child = Command::new(&fake_cloudflared)
        .args([
            "-c",
            "sleep 300; :",
            "cloudflared",
            "tunnel",
            "--no-autoupdate",
            "--url",
            "http://127.0.0.1:18088",
            "--http-host-header",
            "sweep-ours.rex",
        ])
        .spawn()
        .expect("spawn fake tunnel");
    let mut ours = common::Reaped::new(ours_child, 39998, "cloudflared");
    assert!(store::try_claim_tunnel(
        &conn,
        "sweep-ours.rex",
        ours.id(),
        &ours_doc.to_string_lossy()
    )
    .expect("claim ours row"));

    // 3) DEAD: an impossible pid (macOS pid_max is 99998) — pure cleanup.
    let dead_doc = docroot_with_muplugin(sandbox.root(), "dead");
    assert!(store::try_claim_tunnel(&conn, "sweep-dead.rex", 4_000_000, &dead_doc.to_string_lossy())
        .expect("claim dead row"));

    // 4) PENDING: a claim whose child never spawned (crash between claim and
    //    spawn) — sentinel pid, cleanup only, nothing to signal.
    let pending_doc = docroot_with_muplugin(sandbox.root(), "pending");
    assert!(store::try_claim_tunnel(
        &conn,
        "sweep-pending.rex",
        tunnels::PID_PENDING,
        &pending_doc.to_string_lossy()
    )
    .expect("claim pending row"));

    let killed = tunnels::sweep_startup(&conn, &*plat);

    // The foreign process SURVIVES (no bare-pid kill), everything else of its
    // row is gone.
    let foreign_state = state_of(foreign.id());
    assert!(
        !foreign_state.is_empty() && !foreign_state.starts_with('Z'),
        "foreign process (recycled-pid stand-in) must survive the sweep; ps state: {foreign_state:?}"
    );
    // Ours is dead (zombie until our Reaped guard reaps it, or already gone).
    let ours_state = state_of(ours.id());
    assert!(
        ours_state.is_empty() || ours_state.starts_with('Z'),
        "identified tunnel must be killed by the sweep; ps state: {ours_state:?}"
    );
    assert_eq!(killed, 1, "exactly the identified tunnel counts as killed");
    for (tag, doc) in [
        ("foreign", &foreign_doc),
        ("ours", &ours_doc),
        ("dead", &dead_doc),
        ("pending", &pending_doc),
    ] {
        assert!(!muplugin_exists(doc), "{tag}: mu-plugin must be removed in every branch");
    }
    assert!(
        store::list_tunnels(&conn).expect("list rows").is_empty(),
        "every row must be settled"
    );

    // ── Phase 2: the ROWLESS backstop (ruled 28 Jul 2026) ──────────────────
    // Any real cloudflared on this machine is safe BY CONSTRUCTION: identity
    // is checked against the SANDBOX app-data marker, which no real process's
    // argv contains. Snapshot real pids to prove that, not just claim it.
    let fixture_pids = |ours: u32, look: u32| move |p: &u32| *p != ours && *p != look;
    let before: Vec<u32> = rexenv_lib::platform::current()
        .supervisor()
        .pids_named("cloudflared");

    // OURS-ROWLESS: full identity under the sandbox app-data, NO row — the
    // "DB and process table disagree" class. Must die.
    let rowless_bin_dir =
        plat.paths().app_data_dir().expect("sandbox app data").join("bin-rowless");
    std::fs::create_dir_all(&rowless_bin_dir).expect("create rowless bin dir");
    let rowless_bin = rowless_bin_dir.join("cloudflared");
    stage_fake(&rowless_bin);
    let rowless_child = Command::new(&rowless_bin)
        .args(["-c", "sleep 300; :", "cloudflared", "tunnel", "--url", "http://127.0.0.1:18088", "--http-host-header", "rowless-ours.rex"])
        .spawn()
        .expect("spawn rowless fake");
    let mut rowless = common::Reaped::new(rowless_child, 39996, "cloudflared");

    // LOOKALIKE: named cloudflared, carries the host-header pair, but lives
    // OUTSIDE our app-data — not provably ours, must SURVIVE (the branch
    // that would be silently wrong in either direction).
    let look_dir = std::env::temp_dir().join(format!("rexenv-lookalike-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&look_dir);
    std::fs::create_dir_all(&look_dir).expect("create lookalike dir");
    let look_bin = look_dir.join("cloudflared");
    stage_fake(&look_bin);
    let look_child = Command::new(&look_bin)
        .args(["-c", "sleep 300; :", "cloudflared", "tunnel", "--url", "http://127.0.0.1:18088", "--http-host-header", "lookalike.rex"])
        .spawn()
        .expect("spawn lookalike");
    let mut look = common::Reaped::new(look_child, 39995, "cloudflared");

    // NON-VACUITY, asserted BEFORE the sweep runs — and scoped to what it can
    // actually catch, which is the half that used to fail SILENTLY.
    //
    // With the fakes dying on their own (the pre-3-Aug copy-of-/bin/bash, killed
    // by AMFI), "ours was killed" was satisfied by a process the OS had already
    // killed — a vacuous pass, invisible. This probe turns that into a named
    // failure: a fake that is not alive going into the sweep means the
    // assertions afterwards are about process scheduling, not about
    // `sweep_rowless`.
    //
    // What it does NOT catch, stated rather than assumed: the AMFI kill is
    // asynchronous, so a fake can pass here and die during the sweep. That case
    // is caught by the post-sweep SURVIVE assertion (it is how this was found),
    // and prevented outright by `stage_fake` symlinking rather than copying.
    // The probe is the belt for the direction that has no natural alarm.
    for (what, pid) in [("rowless-ours", rowless.id()), ("lookalike", look.id())] {
        let st = state_of(pid);
        assert!(
            !st.is_empty() && !st.starts_with('Z'),
            "the {what} fixture was not alive going INTO the sweep (ps state {st:?}) — whatever \
             the assertions below then said, they were not about the sweep"
        );
    }

    let killed_rowless = tunnels::sweep_rowless(&conn, &*plat);

    let rowless_state = state_of(rowless.id());
    assert!(
        rowless_state.is_empty() || rowless_state.starts_with('Z'),
        "rowless-but-ours must be killed by the backstop; ps state: {rowless_state:?}"
    );
    let look_state = state_of(look.id());
    assert!(
        !look_state.is_empty() && !look_state.starts_with('Z'),
        "a lookalike outside our app-data must SURVIVE; ps state: {look_state:?}"
    );
    assert_eq!(killed_rowless, 1, "exactly the provably-ours rowless process counts");
    // Every real cloudflared that predated the fixtures is still alive.
    let after: Vec<u32> = rexenv_lib::platform::current()
        .supervisor()
        .pids_named("cloudflared");
    let keep = fixture_pids(rowless.id(), look.id());
    for p in before.iter().filter(|p| keep(p)) {
        assert!(after.contains(p), "real cloudflared pid {p} must be untouched by the backstop");
    }

    rowless.reap();
    look.reap();
    let _ = std::fs::remove_dir_all(&look_dir);
    foreign.reap();
    ours.reap();
    println!(
        "PASS tunnel_sweep: foreign survived, ours killed, dead cleaned; rowless-ours killed, \
         lookalike survived, real tunnels untouched"
    );
}
