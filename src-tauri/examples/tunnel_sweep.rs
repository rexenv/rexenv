//! Live check — the tunnel launch sweep settles crash-survivor rows correctly
//! (lifecycle ruling 28 Jul 2026, `docs/PLAN-tunnel-lifecycle.md`).
//!
//! Three recorded rows, three fates:
//!   1. FOREIGN — the row's pid is alive but names an unrelated process (the
//!      recycled-pid case). The sweep must NOT kill it, and must still remove
//!      the mu-plugin and the row. This is the branch that would be silently
//!      wrong in either direction.
//!   2. OURS — the pid's argv carries the full identity (app-data binary path
//!      + exact `--http-host-header <domain>` pair). The sweep must kill it
//!      and clean up.
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

fn main() {
    let (plat, sandbox) = common::sandbox("tunnel-sweep");
    let conn = db::open_for_platform(plat.paths()).expect("open sandbox database");

    // 1) FOREIGN: a live process that is not ours — models a recycled pid.
    let foreign_doc = docroot_with_muplugin(sandbox.root(), "foreign");
    let foreign_child = Command::new("/bin/sleep").arg("300").spawn().expect("spawn sleep");
    let mut foreign = common::Reaped::new(foreign_child, 39997, "sleep");
    store::record_tunnel(&conn, "sweep-foreign.rex", foreign.id(), &foreign_doc.to_string_lossy())
        .expect("record foreign row");

    // 2) OURS: a stand-in with the full identity — a copy of /bin/bash named
    //    `cloudflared` under the SANDBOX app-data (the ownership marker), its
    //    argv carrying the exact --http-host-header pair. Sleeps like a real
    //    wedged tunnel would.
    let fake_bin_dir = plat.paths().app_data_dir().expect("sandbox app data").join("bin-fixture");
    std::fs::create_dir_all(&fake_bin_dir).expect("create fixture bin dir");
    let fake_cloudflared = fake_bin_dir.join("cloudflared");
    std::fs::copy("/bin/bash", &fake_cloudflared).expect("stage fake cloudflared");
    let ours_doc = docroot_with_muplugin(sandbox.root(), "ours");
    let ours_child = Command::new(&fake_cloudflared)
        .args([
            "-c",
            "sleep 300",
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
    store::record_tunnel(&conn, "sweep-ours.rex", ours.id(), &ours_doc.to_string_lossy())
        .expect("record ours row");

    // 3) DEAD: an impossible pid (macOS pid_max is 99998) — pure cleanup.
    let dead_doc = docroot_with_muplugin(sandbox.root(), "dead");
    store::record_tunnel(&conn, "sweep-dead.rex", 4_000_000, &dead_doc.to_string_lossy())
        .expect("record dead row");

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
    for (tag, doc) in [("foreign", &foreign_doc), ("ours", &ours_doc), ("dead", &dead_doc)] {
        assert!(!muplugin_exists(doc), "{tag}: mu-plugin must be removed in every branch");
    }
    assert!(
        store::list_tunnels(&conn).expect("list rows").is_empty(),
        "every row must be settled"
    );

    foreign.reap();
    ours.reap();
    println!("PASS tunnel_sweep: foreign survived, ours killed, dead cleaned; all files+rows settled");
}
