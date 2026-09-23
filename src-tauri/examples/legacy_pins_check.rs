//! Live check: every LEGACY pin (`docs/PLAN-macos-13-floor.md` §6.2) downloads,
//! verifies, prepares and RUNS on this Mac — the bundles relinked and re-signed
//! by the same `prepare_binary_tree` a 13 host would use, the singles by
//! `prepare_binary`, each answering `--version` with the version it was pinned as.
//!
//!   cargo run --example legacy_pins_check      (network tier)
//!
//! # What this proves, and what it cannot
//!
//! A legacy row in `binaries.rs` is a version string and two digests. Three
//! things can be wrong with it that no unit test sees: the blob may not resolve
//! any more (a registry prune), the bottle's layout may differ from the standard
//! one the include lists were written against (a missing dylib is a relink
//! error, or worse a bundle that publishes and then cannot load), and the binary
//! may not run. This runs each one. It runs on THIS host's macOS, so it proves
//! the artifact is what it says and loads under a NEWER dyld — the claim that it
//! loads on 13 itself rests on the measured `minos` (`PORTS.md`) until T7's VM.
//!
//! # Fixture scope
//!
//! `common::sandbox` for the platform (no real config/prefix is ever held). The
//! shared binary cache is the documented mutable exception: these trees land
//! there under their OWN version dirs (`redis-8.2.1`, `mysql-8.4.3`, …), exactly
//! where the app would put them, and nothing here deletes or spawns a service —
//! every probe is a one-shot `--version` that exits.

use rexenv_lib::core::binaries::{self, BinaryTier, PinSet};
use rexenv_lib::core::{apache, mariadb, redis};
use rexenv_lib::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod common;

/// `otool -L` shows only system / `@loader_path` load commands — the relink did
/// its job (a surviving `@@HOMEBREW_*@@` placeholder is a dlopen failure at run).
fn load_commands_clean(path: &Path) -> bool {
    let Ok(out) = Command::new("otool").arg("-L").arg(path).output() else { return false };
    let listing = String::from_utf8_lossy(&out.stdout);
    listing.lines().skip(1).all(|line| match line.split_whitespace().next() {
        None => true,
        Some(dep) => dep.starts_with("/usr/lib/") || dep.starts_with("/System/") || dep.starts_with("@loader_path/"),
    })
}

fn signature_valid(path: &Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--strict"])
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `bin args…`, return combined output (a version banner lands on either stream).
fn probe(bin: &Path, args: &[&str]) -> String {
    match Command::new(bin).args(args).output() {
        Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("(spawn failed: {e})"),
    }
}

fn check_line(ok: &mut bool, label: &str, pass: bool, detail: &str) {
    println!("  {} {label:<34} {detail}", if pass { "ok  " } else { "FAIL" });
    *ok &= pass;
}

/// Resolve a bundle, audit every Mach-O it published, probe its main binary.
async fn bundle(
    plat: &dyn Platform,
    ok: &mut bool,
    name: &str,
    version: &str,
    bin: fn(&Path) -> PathBuf,
    args: &[&str],
    expect: &str,
) -> Option<PathBuf> {
    let label = format!("{name} {version}");
    let base = match binaries::resolve_bundle(plat, name, version).await {
        Ok(b) => b,
        Err(e) => {
            check_line(ok, &label, false, &format!("resolve_bundle: {e}"));
            return None;
        }
    };
    let mut audited = 0;
    let mut clean = true;
    for entry in walkdir(&base) {
        if !is_macho(&entry) {
            continue;
        }
        audited += 1;
        clean &= load_commands_clean(&entry) && signature_valid(&entry);
    }
    let banner = probe(&bin(&base), args);
    let runs = banner.contains(expect);
    check_line(
        ok,
        &label,
        clean && runs && audited > 0,
        &format!("{audited} Mach-Os relinked+signed={clean} · runs={runs} · {}", banner.lines().next().unwrap_or("").trim()),
    );
    Some(base)
}

fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn is_macho(p: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(p) else { return false };
    let mut magic = [0u8; 4];
    std::io::Read::read_exact(&mut f, &mut magic).is_ok()
        && matches!(magic, [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe])
}

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _guard) = common::sandbox("legacy-pins");
    let plat: &dyn Platform = &*plat;
    let mut ok = true;

    // The tier is what a 13 host would install at launch; installing it here
    // makes `pins()` — and through it `xdebug_row` — answer the Legacy13 set.
    binaries::install_tier(BinaryTier::Legacy13);
    let l13 = PinSet::for_tier(BinaryTier::Legacy13);
    let l14 = PinSet::for_tier(BinaryTier::Legacy14);
    println!("=== Legacy13 pins (tier installed: {:?}) ===", binaries::tier());

    // ── singles ─────────────────────────────────────────────────────────────
    match binaries::resolve(plat, "cloudflared", l13.cloudflared).await {
        Ok(bin) => {
            let banner = probe(&bin, &["--version"]);
            check_line(&mut ok, &format!("cloudflared {}", l13.cloudflared), banner.contains("2025.4.0"), banner.trim());
        }
        Err(e) => check_line(&mut ok, "cloudflared", false, &e.to_string()),
    }
    for v in l13.mysql_versions {
        match binaries::resolve_dir(plat, "mysql", v).await {
            Ok(dir) => {
                let banner = probe(&dir.join("bin/mysqld"), &["--version"]);
                check_line(&mut ok, &format!("mysql {v} (macos14 build)"), banner.contains(v), banner.trim());
            }
            Err(e) => check_line(&mut ok, &format!("mysql {v}"), false, &e.to_string()),
        }
    }
    // Legacy14's one engine row: PostgreSQL 16.4.0 (refused on 13, offered on 14).
    for v in l14.postgres_versions {
        match binaries::resolve_dir(plat, "postgres", v).await {
            Ok(dir) => {
                let banner = probe(&dir.join("bin/postgres"), &["--version"]);
                let want = v.trim_end_matches(".0");
                check_line(&mut ok, &format!("postgres {v} (Legacy14)"), banner.contains(want), banner.trim());
            }
            Err(e) => check_line(&mut ok, &format!("postgres {v}"), false, &e.to_string()),
        }
    }

    // ── bottle bundles: ventura blobs through the same relink + re-sign ─────
    bundle(plat, &mut ok, "redis", l13.redis, redis::redis_server_bin, &["--version"], "v=8.2.1").await;
    for v in l13.mariadb_versions {
        bundle(plat, &mut ok, "mariadb", v, mariadb::mariadbd_bin, &["--version"], v).await;
    }
    bundle(plat, &mut ok, "httpd", l13.httpd, apache::httpd_bin, &["-v"], "Apache/2.4.65").await;

    // ── Xdebug 3.4.5 ventura .so loads into the (unchanged) static php of each minor ──
    // 8.5 has no row on purpose: its ventura blobs were built against the 8.4 Zend
    // API (this check found that, 23 Sep 2026) — the table says so, so assert it.
    check_line(
        &mut ok,
        "xdebug 8.5 under Legacy13",
        binaries::xdebug_bundle_id("8.5").is_none(),
        "no row — every ventura blob predates PHP 8.5 GA (expected)",
    );
    for minor in ["8.1", "8.2", "8.3", "8.4"] {
        let Some((bundle_name, version)) = binaries::xdebug_bundle_id(minor) else {
            check_line(&mut ok, &format!("xdebug {minor}"), false, "no bundle id under Legacy13");
            continue;
        };
        let full = l13.php_versions.iter().find(|v| v.starts_with(&format!("{minor}."))).copied().unwrap_or("");
        let so = match binaries::resolve_bundle(plat, &bundle_name, version).await {
            Ok(b) => b.join("xdebug.so"),
            Err(e) => {
                check_line(&mut ok, &format!("xdebug {minor} {version}"), false, &e.to_string());
                continue;
            }
        };
        let php = match binaries::resolve(plat, "php", full).await {
            Ok(p) => p,
            Err(e) => {
                check_line(&mut ok, &format!("php {full}"), false, &e.to_string());
                continue;
            }
        };
        let banner = probe(&php, &["-n", "-d", &format!("zend_extension={}", so.display()), "-v"]);
        check_line(
            &mut ok,
            &format!("xdebug {minor} {version} → php {full}"),
            banner.contains("with Xdebug v3.4.5"),
            banner.lines().find(|l| l.contains("Xdebug")).unwrap_or(banner.lines().next().unwrap_or("")).trim(),
        );
    }

    binaries::install_tier(BinaryTier::Standard);
    if ok {
        println!("\nlegacy_pins_check: ALL PASS — every Legacy13/14 pin resolved, prepared and ran here");
        ExitCode::SUCCESS
    } else {
        eprintln!("\nlegacy_pins_check: FAILED — see above");
        ExitCode::FAILURE
    }
}
