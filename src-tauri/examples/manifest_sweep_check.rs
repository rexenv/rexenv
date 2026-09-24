//! The manifest HEAD+digest sweep (TESTING §1.1's audit item): every pinned
//! URL — both macOS arches, bundles part by part, and the Windows x64 set
//! (docs/PLAN-windows-port.md W2) — answered by its host, and the
//! small artifacts re-hashed against their pins.
//!
//!   cargo run --example manifest_sweep_check      (network tier)
//!
//! Why it exists: `every_offered_db_version_is_pinned_and_resolves` "resolves"
//! a HashMap, and 18 manifest tests were named as if they fetch. Whether a
//! pinned URL still ANSWERS is the host's fact — ghcr garbage-collects
//! formulae, projects re-tag, CDNs move — and the Intel digests were hashed
//! from downloads nobody has re-run since pin time. This sweep:
//!
//!   1. enumerates every pin from the SAME constants the manifest arms gate
//!      on (a version added to `PHP_VERSIONS` joins the sweep by existing);
//!   2. probes each URL (HEAD; ranged-GET fallback for hosts that refuse
//!      HEAD; ghcr gets its anonymous bearer) for BOTH arches;
//!   3. fully re-downloads and re-hashes artifacts under the size cap,
//!      comparing to the pinned digest — a re-issued artifact at an
//!      unchanged version fails HERE before any user's machine sees it;
//!   4. says what it skipped: every over-cap artifact is NAMED with its
//!      size, because a silent cap reads as "covered everything".
//!
//! Scope, honestly: the enumeration is this file's list. It is derived from
//! the version constants rather than retyped per artifact, and the count
//! floor below alarms if the list shrinks — but a NEW binary name still has
//! to be added here (the ledger row says so).

use rexenv_lib::core::binaries::{self, Checksum, PinSet};
use rexenv_lib::core::php;
use rexenv_lib::platform::traits::Arch;
use sha2::Digest;
use std::process::ExitCode;
use std::time::Duration;

mod common;

/// Full-download budget for the digest half — enough for every phar, static
/// binary and bottle, deliberately excluding the multi-hundred-MB trees
/// (MySQL, Postgres, MariaDB), which get HEAD-only and are NAMED when skipped.
///
/// **PHP builds are under this cap and ARE re-hashed** — every minor, both
/// arches, ~31–35 MB each. This comment used to list them beside MySQL and
/// Postgres as HEAD-only, which was wrong in the direction that matters: it
/// describes the pins rexenv is the DISTRIBUTOR of (7.4, self-built and
/// self-hosted) as byte-unverified by the sweep, when they are exactly the ones
/// it verifies. Ledger #335 records the real behaviour.
const DIGEST_CAP_BYTES: u64 = 40_000_000;

struct Target {
    label: String,
    url: String,
    checksum: Checksum,
}

fn push_single(out: &mut Vec<Target>, os: &str, name: &str, version: &str, arch: Arch) {
    match binaries::manifest(name, version, os, arch) {
        Some(spec) => out.push(Target {
            label: format!("{name} {version} {os}/{arch:?}"),
            url: spec.url,
            checksum: spec.checksum,
        }),
        None => panic!("{name} {version} {os}/{arch:?}: no manifest arm — the sweep's list and the manifest disagree"),
    }
}

fn push_bundle(out: &mut Vec<Target>, name: &str, version: &str, arch: Arch) {
    match binaries::bundle_manifest(name, version, "macos", arch) {
        Some(spec) => {
            for part in spec.parts {
                out.push(Target {
                    label: format!("{name} {version} {arch:?} · {}", part.formula),
                    url: part.url,
                    checksum: part.checksum,
                });
            }
        }
        None => panic!("{name} {version} {arch:?}: no bundle arm — the sweep's list and the manifest disagree"),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut checks = common::Check::new("manifest_sweep_check");

    // 1. Enumerate — from the pin sets, not a retyped list — and from EVERY
    //    tier's set: a macOS 13 host resolves the Legacy13 pins, and a sweep that
    //    read only this host's tier would never probe them. Dedup by (os, name,
    //    version, arch): a version two tiers share is one artifact.
    let mut targets: Vec<Target> = Vec::new();
    for tier in PinSet::ALL_TIERS {
        let p = PinSet::for_tier(tier);
        for arch in [Arch::Arm64, Arch::X86_64] {
            push_single(&mut targets, "macos", "caddy", p.caddy, arch);
            push_single(&mut targets, "macos", "nginx", p.nginx, arch);
            push_single(&mut targets, "macos", "mailpit", p.mailpit, arch);
            push_single(&mut targets, "macos", "cloudflared", p.cloudflared, arch);
            push_single(&mut targets, "macos", "frankenphp", p.frankenphp, arch);
            push_single(&mut targets, "macos", "adminer", p.adminer, arch);
            push_single(&mut targets, "macos", "wp-cli", p.wp_cli, arch);
            push_single(&mut targets, "macos", "composer", p.composer, arch);
            for v in p.php_versions {
                push_single(&mut targets, "macos", "php", v, arch);
                push_single(&mut targets, "macos", "php-fpm", v, arch);
            }
            for v in p.mysql_versions {
                push_single(&mut targets, "macos", "mysql", v, arch);
            }
            for v in p.postgres_versions {
                push_single(&mut targets, "macos", "postgres", v, arch);
            }
            for v in p.redis_versions {
                push_bundle(&mut targets, "redis", v, arch);
            }
            for v in p.mariadb_versions {
                push_bundle(&mut targets, "mariadb", v, arch);
            }
            push_bundle(&mut targets, "httpd", p.httpd, arch);
            // Xdebug: the tier's version per minor, asked of the table directly
            // (`xdebug_bundle_id` reads the HOST's tier). A minor with no row at
            // that version (7.4, 8.0) simply has no target.
            for minor in php::all_minors() {
                let bundle = format!("xdebug-{minor}");
                if binaries::bundle_manifest(&bundle, p.xdebug, "macos", arch).is_some() {
                    push_bundle(&mut targets, &bundle, p.xdebug, arch);
                }
            }
        }
    }
    let before = targets.len();
    targets.sort_by(|a, b| a.label.cmp(&b.label));
    targets.dedup_by(|a, b| a.label == b.label);
    eprintln!("targets: {} unique of {before} enumerated across {} tiers", targets.len(), PinSet::ALL_TIERS.len());
    // Windows x64 (port W2). ONE target per pin: every `Arch` resolves the same x64
    // URL there, so asking for both would probe it twice. The .phar/.php artifacts
    // are OS-agnostic and were swept above.
    push_single(&mut targets, "windows", "caddy", binaries::pins().caddy, Arch::X86_64);
    push_single(&mut targets, "windows", "nginx", binaries::pins().nginx, Arch::X86_64);
    push_single(&mut targets, "windows", "mailpit", binaries::pins().mailpit, Arch::X86_64);
    push_single(&mut targets, "windows", "cloudflared", binaries::pins().cloudflared, Arch::X86_64);
    for v in binaries::pins().php_versions {
        push_single(&mut targets, "windows", "php", v, Arch::X86_64);
    }
    for v in binaries::pins().mysql_versions {
        push_single(&mut targets, "windows", "mysql", v, Arch::X86_64);
    }
    for v in binaries::pins().postgres_versions {
        push_single(&mut targets, "windows", "postgres", v, Arch::X86_64);
    }
    // Linux (port L2): both archs, every upstream publishes aarch64. PHP 7.4 has no static
    // Linux build to pin (D-L8), so it is filtered here — `push_single` panics on a missing arm.
    for arch in [Arch::X86_64, Arch::Arm64] {
        push_single(&mut targets, "linux", "caddy", binaries::pins().caddy, arch);
        push_single(&mut targets, "linux", "nginx", binaries::pins().nginx, arch);
        push_single(&mut targets, "linux", "mailpit", binaries::pins().mailpit, arch);
        push_single(&mut targets, "linux", "cloudflared", binaries::pins().cloudflared, arch);
        push_single(&mut targets, "linux", "frankenphp", binaries::pins().frankenphp, arch);
        for v in binaries::pins().php_versions.iter().filter(|v| !v.starts_with("7.4")) {
            push_single(&mut targets, "linux", "php", v, arch);
            push_single(&mut targets, "linux", "php-fpm", v, arch);
        }
        for v in binaries::pins().mysql_versions {
            push_single(&mut targets, "linux", "mysql", v, arch);
        }
        for v in binaries::pins().postgres_versions {
            push_single(&mut targets, "linux", "postgres", v, arch);
        }
    }
    // The count floor is the drift alarm for the LIST itself: pins only ever
    // grow, so a shrink means an enumeration line was lost, not a pin.
    checks.is(
        &format!("enumeration floor ({} targets ≥ 130)", targets.len()),
        targets.len() >= 130,
        "the sweep's list shrank — an enumeration line was lost",
    );

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) rexenv-manifest-sweep")
        .timeout(Duration::from_secs(180))
        .build()
        .expect("client");

    // 2+3. Probe each URL; digest the small ones.
    let mut failures: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut digested = 0u32;
    for t in &targets {
        let ghcr = t.url.contains("ghcr.io");
        let auth = |mut req: reqwest::RequestBuilder| -> reqwest::RequestBuilder {
            if ghcr {
                req = req.header("Authorization", "Bearer QQ==");
            }
            req
        };
        // HEAD first; a zero/absent length is NOT trusted — a redirecting
        // host answers HEAD with `content-length: 0`, and `Some(0) ≤ cap`
        // silently full-downloaded every 60 MB PHP build on the first run.
        // The ranged GET is both the HEAD fallback and the length oracle:
        // its Content-Range carries the artifact's REAL total.
        let mut status = None;
        let mut len: Option<u64> = None;
        if let Ok(resp) = auth(client.head(&t.url)).send().await {
            if resp.status().is_success() {
                status = Some(resp.status());
                len = resp.content_length().filter(|l| *l > 0);
            }
        }
        if status.is_none() || len.is_none() {
            match auth(client.get(&t.url)).header("Range", "bytes=0-0").send().await {
                Ok(resp) if resp.status().is_success() => {
                    status = Some(resp.status());
                    // Content-Range: bytes 0-0/<total>
                    len = resp
                        .headers()
                        .get("content-range")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.rsplit('/').next())
                        .and_then(|v| v.parse().ok())
                        .filter(|l| *l > 0);
                }
                // A 5xx is the HOST having a moment, not evidence about the PIN,
                // and this sweep asks 88 URLs in a burst. Measured 21 Aug 2026:
                // three postgres targets came back `504 Gateway Timeout` in one
                // run and all three answered 200 on a re-check minutes later.
                // Retry those; a 404 still fails on the first answer, because a
                // 404 IS evidence about the pin.
                Ok(resp) if resp.status().is_server_error() => {
                    let mut last = resp.status();
                    let mut recovered = false;
                    for wait in [2u64, 5] {
                        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                        match auth(client.get(&t.url)).header("Range", "bytes=0-0").send().await {
                            Ok(r) if r.status().is_success() => {
                                status = Some(r.status());
                                len = r
                                    .headers()
                                    .get("content-range")
                                    .and_then(|v| v.to_str().ok())
                                    .and_then(|v| v.rsplit('/').next())
                                    .and_then(|v| v.parse().ok())
                                    .filter(|l| *l > 0);
                                recovered = true;
                                break;
                            }
                            Ok(r) => last = r.status(),
                            Err(_) => {}
                        }
                    }
                    if !recovered && status.is_none() {
                        failures.push(format!(
                            "{} → HTTP {last} after 3 attempts (server-side; this does NOT \
                             disprove the pin — re-check the host before re-pinning)",
                            t.label
                        ));
                    }
                }
                Ok(resp) if status.is_none() => {
                    failures.push(format!("{} → HTTP {}", t.label, resp.status()))
                }
                Err(e) if status.is_none() => failures.push(format!("{} → {e}", t.label)),
                _ => {}
            }
        }
        let Some(_) = status else { continue };

        match len {
            Some(l) if l <= DIGEST_CAP_BYTES => {
                // Retried once with a backoff: the first full run hammered
                // static-php.dev with ~28 sequential 30 MB pulls and six died
                // mid-body ("error decoding response body") — every one
                // downloaded clean by hand a minute later. A transfer cut is
                // the network's fact, not the pin's; only a SECOND failure is
                // reported. Digest mismatches are never retried — the same
                // bytes twice would prove nothing the first run didn't.
                let mut fetched: Option<Result<Vec<u8>, String>> = None;
                for attempt in 0..2 {
                    if attempt > 0 {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                    match auth(client.get(&t.url)).send().await {
                        Ok(resp) => match resp.bytes().await {
                            Ok(bytes) => {
                                fetched = Some(Ok(bytes.to_vec()));
                                break;
                            }
                            Err(e) => fetched = Some(Err(format!("body: {e}"))),
                        },
                        Err(e) => fetched = Some(Err(format!("GET: {e}"))),
                    }
                }
                match fetched.expect("two attempts ran") {
                    Ok(bytes) => {
                            let actual = match &t.checksum {
                                Checksum::Sha256(_) => {
                                    format!("{:x}", sha2::Sha256::digest(&bytes))
                                }
                                Checksum::Sha512(_) => {
                                    format!("{:x}", sha2::Sha512::digest(&bytes))
                                }
                            };
                            let pinned = match &t.checksum {
                                Checksum::Sha256(h) | Checksum::Sha512(h) => h.to_lowercase(),
                            };
                            if actual == pinned {
                                digested += 1;
                                println!("  ✓ {} — answers, digest matches ({l} bytes)", t.label);
                            } else {
                                failures.push(format!(
                                    "{} → DIGEST MISMATCH: pinned {}… got {}… (a re-issued \
                                     artifact at an unchanged version — the #328 class, \
                                     caught before a user's machine)",
                                    t.label,
                                    &pinned[..16],
                                    &actual[..16]
                                ));
                            }
                        }
                    Err(e) => failures.push(format!("{} → {e} (twice, with backoff)", t.label)),
                }
            }
            Some(l) => {
                skipped.push(format!("{} ({l} bytes)", t.label));
                println!("  ✓ {} — answers (HEAD only, {l} bytes > cap)", t.label);
            }
            None => {
                skipped.push(format!("{} (length unknown)", t.label));
                println!("  ✓ {} — answers (HEAD only, no length)", t.label);
            }
        }
    }

    checks.is(
        &format!("every pinned URL answers ({} targets)", targets.len()),
        failures.is_empty(),
        &failures.join("\n      "),
    );
    // The digest half needs a floor too, or a host that stopped sending
    // Content-Length would silently demote every artifact to HEAD-only.
    checks.is(
        &format!("digest floor ({digested} re-hashed ≥ 10)"),
        digested >= 10,
        "almost nothing was re-hashed — the digest half went vacuous",
    );
    println!(
        "  — HEAD-only (over the {}MB cap or unsized), stated not silent: {}",
        DIGEST_CAP_BYTES / 1_000_000,
        skipped.len()
    );
    for s in &skipped {
        println!("      {s}");
    }
    checks.verdict()
}
