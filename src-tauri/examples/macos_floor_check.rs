//! Live check: the app's STATED macOS floor equals the highest floor its own
//! binaries declare — measured per arch, from the artifacts, on both slices.
//!
//!   cargo run --example macos_floor_check      (network tier)
//!
//! # The rule, and why it needed a check rather than a table
//!
//! `docs/PORTS.md` states it plainly: `minimumSystemVersion` must equal the MAX
//! `minos` across the binaries a default install runs. It was maintained by
//! hand, and a hand-maintained rule is as good as the last person to re-read it:
//! PostgreSQL's pins sat at `minos 26.0` — eleven majors ABOVE the stated floor
//! of 15.0 — from 15 to 30 Aug 2026, and nothing moved, because nothing was
//! comparing. (That one is now re-pinned to 15.0 and is not in the default stack
//! anyway; the point is that the fortnight passed unnoticed.)
//!
//! # The half that had never been measured at all
//!
//! PORTS.md's table is read from this machine's binary cache, which only ever
//! downloads the HOST arch. So every number in it was an arm64 number, and
//! `minimumSystemVersion: 15.0` was asserted for x86_64 purely on the assumption
//! that upstream builds both slices to the same deployment target. **This is the
//! arm64-DMG mistake's shape**: a universal artifact whose halves differ, perfect
//! on the machine that made it, broken for everyone on the other chip — and
//! worse than a thin binary, because it is invisible until an Intel user on
//! macOS 15 finds their web server will not start.
//!
//! No Intel Mac is needed to settle it. `minos` is metadata: fetch the x86_64
//! artifact and read its load commands. That is what this does, and it reports
//! any binary whose two slices DISAGREE — which is the thing the assumption
//! above is really about, and which no per-arch max would show on its own.
//!
//! # Honest scope
//!
//! - The list is `binaries::DEFAULT_STACK`, in production code beside the version
//!   constants so a pin bump and the floor question live in one file. Optional
//!   engines are deliberately out (a user-chosen engine's floor binds the user
//!   who enables it, and they are the multi-hundred-MB trees).
//! - Bytes are checksum-verified before their `minos` is believed. A number read
//!   from bytes that are not the pinned bytes is a number about something else.
//! - Nothing here refuses a spawn, and this check is not a gate on running the
//!   app — it is a gate on SHIPPING one whose stated floor is a fiction.

use rexenv_lib::core::binaries::{self, Archive, Checksum};
use rexenv_lib::core::macho;
use rexenv_lib::platform::traits::Arch;
use sha2::Digest;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

mod common;

/// Where the app states its floor. Read from the file rather than a constant, so
/// this compares against the value that actually SHIPS.
const TAURI_CONF: &str = "tauri.conf.json";

fn stated_floor() -> Option<(u32, u32, u32)> {
    let conf = Path::new(env!("CARGO_MANIFEST_DIR")).join(TAURI_CONF);
    let text = std::fs::read_to_string(conf).ok()?;
    // Deliberately a string scan and not a JSON parse: the file has no schema
    // here worth pulling a dependency for, and the key is unique in it.
    let at = text.find("\"minimumSystemVersion\"")?;
    let rest = &text[at..];
    let start = rest.find(':')? + 1;
    let open = rest[start..].find('"')? + start + 1;
    let close = rest[open..].find('"')? + open;
    macho::parse_version(&rest[open..close])
}

fn fmt(v: (u32, u32, u32)) -> String {
    if v.2 == 0 {
        format!("{}.{}", v.0, v.1)
    } else {
        format!("{}.{}.{}", v.0, v.1, v.2)
    }
}

/// Pull the one file we need out of a downloaded artifact and return its path.
/// `TarGzTree` members are addressed under the single stripped top-level dir,
/// the same shape `binaries::resolve_dir` produces.
fn unpack(bytes: &[u8], spec: &binaries::BinarySpec, dest: &Path) -> Option<PathBuf> {
    match spec.archive {
        Archive::Raw => {
            let out = dest.join(spec.member);
            std::fs::write(&out, bytes).ok()?;
            Some(out)
        }
        Archive::TarGz | Archive::TarGzTree => {
            let gz = flate2::read::GzDecoder::new(bytes);
            let mut ar = tar::Archive::new(gz);
            for entry in ar.entries().ok()? {
                let mut e = entry.ok()?;
                let path = e.path().ok()?.to_path_buf();
                let matches = match spec.archive {
                    // Single-binary tarballs name the member directly.
                    Archive::TarGz => path.file_name().map(|f| f == spec.member).unwrap_or(false),
                    // Trees carry one top-level dir; the member is a path under it.
                    _ => path
                        .strip_prefix(path.components().next()?.as_os_str())
                        .map(|p| p == Path::new(spec.member))
                        .unwrap_or(false),
                };
                if matches {
                    let out = dest.join("artifact");
                    let mut buf = Vec::new();
                    e.read_to_end(&mut buf).ok()?;
                    std::fs::write(&out, buf).ok()?;
                    return Some(out);
                }
            }
            None
        }
        // Windows artifacts (docs/PLAN-windows-port.md W2). No macOS manifest arm
        // produces a zip, and there is no Mach-O inside one to measure.
        Archive::Zip | Archive::ZipTree { .. } => None,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let stated = match stated_floor() {
        Some(v) => v,
        None => {
            eprintln!("macos_floor_check: could not read minimumSystemVersion from {TAURI_CONF}");
            return ExitCode::FAILURE;
        }
    };
    println!("stated floor ({TAURI_CONF}): {}\n", fmt(stated));

    let dir = std::env::temp_dir().join(format!("rexenv-floor-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let client = reqwest::Client::builder()
        .user_agent("rexenv-macos-floor-check")
        .timeout(Duration::from_secs(300))
        .build()
        .expect("client");

    let mut failures: Vec<String> = Vec::new();
    let mut measured: Vec<(String, Arch, (u32, u32, u32))> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();

    for (name, version) in binaries::DEFAULT_STACK {
        for arch in [Arch::Arm64, Arch::X86_64] {
            let label = format!("{name} {version} {arch:?}");
            let Some(spec) = binaries::manifest(name, version, "macos", arch) else {
                failures.push(format!("{label}: no manifest — see the L0 guard, this cannot happen"));
                continue;
            };
            // Two attempts, like `manifest_sweep_check`: the first run of this
            // check lost PHP arm64 to "error decoding response body" mid-stream
            // and reported it beside a real defect, which is how a transient
            // teaches people to skim a red check.
            let mut fetched: Option<Result<Vec<u8>, String>> = None;
            for attempt in 0..2 {
                if attempt > 0 {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
                match client.get(&spec.url).send().await {
                    Ok(r) => match r.bytes().await {
                        Ok(b) => {
                            fetched = Some(Ok(b.to_vec()));
                            break;
                        }
                        Err(e) => fetched = Some(Err(format!("body: {e}"))),
                    },
                    Err(e) => fetched = Some(Err(format!("GET: {e}"))),
                }
            }
            let bytes = match fetched.expect("two attempts ran") {
                Ok(b) => b,
                Err(e) => {
                    failures.push(format!("{label}: {e} (after 2 attempts)"));
                    continue;
                }
            };
            // Verify BEFORE believing the number: a minos read from bytes that
            // are not the pinned bytes describes a different artifact.
            let (actual, pinned) = match &spec.checksum {
                Checksum::Sha256(h) => (format!("{:x}", sha2::Sha256::digest(&bytes)), h.to_lowercase()),
                Checksum::Sha512(h) => (format!("{:x}", sha2::Sha512::digest(&bytes)), h.to_lowercase()),
            };
            if actual != pinned {
                failures.push(format!(
                    "{label}: DIGEST MISMATCH — refusing to read a floor off unpinned bytes"
                ));
                continue;
            }
            let sub = dir.join(format!("{name}-{arch:?}"));
            std::fs::create_dir_all(&sub).ok();
            match unpack(&bytes, &spec, &sub).and_then(|p| macho::min_macos(&p)) {
                Some(v) => {
                    println!("  {label}: minos {}", fmt(v));
                    measured.push((format!("{name} {version}"), arch, v));
                }
                // `None` means "this tells us nothing" — a phar, a script, or a
                // Mach-O with no version command. Named, never counted as 0:
                // a silent skip in a MAX is how a max comes out too low.
                None => unreadable.push(label),
            }
        }
    }

    // Per-arch max — the number the app's floor claims to be.
    for arch in [Arch::Arm64, Arch::X86_64] {
        let mut top: Option<(String, (u32, u32, u32))> = None;
        for (label, a, v) in &measured {
            if *a == arch && top.as_ref().map(|(_, t)| v > t).unwrap_or(true) {
                top = Some((label.clone(), *v));
            }
        }
        match top {
            None => failures.push(format!("{arch:?}: nothing measured — the max is vacuous")),
            Some((who, v)) => {
                println!("\n{arch:?} max: {} ({who})", fmt(v));
                if v != stated {
                    failures.push(format!(
                        "{arch:?}: the stack needs macOS {} ({who}) but the app STATES {}. {}",
                        fmt(v),
                        fmt(stated),
                        if v > stated {
                            "Users between the two will install it and watch a service fail to \
                             start — the failure the stated floor exists to prevent."
                        } else {
                            "Nothing breaks, but the app is refusing to install for users it \
                             would run for. Lower it, or record why the higher floor is \
                             deliberate."
                        }
                    ));
                }
            }
        }
    }

    // The assumption the whole row rested on: that upstream builds both slices
    // to the same target. Reported per binary, because a max hides it.
    for (name, _) in binaries::DEFAULT_STACK {
        let of = |arch: Arch| {
            measured.iter().find(|(l, a, _)| l.starts_with(name) && *a == arch).map(|(_, _, v)| *v)
        };
        if let (Some(a), Some(x)) = (of(Arch::Arm64), of(Arch::X86_64)) {
            if a != x {
                failures.push(format!(
                    "{name}: arm64 declares {} and x86_64 declares {} — the two slices of a \
                     universal app do NOT share a floor, which is the case PORTS.md's arch \
                     caveat was written for and the one a host-arch-only table can never see",
                    fmt(a),
                    fmt(x)
                ));
            }
        }
    }

    if !unreadable.is_empty() {
        println!("\nno version command (named, not counted): {}", unreadable.join(", "));
    }
    let _ = std::fs::remove_dir_all(&dir);

    if failures.is_empty() {
        println!("\nmacos_floor_check: ALL PASS — stated floor == max(minos), both arches");
        ExitCode::SUCCESS
    } else {
        eprintln!("\nmacos_floor_check: FAILED");
        for f in &failures {
            eprintln!("  - {f}");
        }
        ExitCode::FAILURE
    }
}
