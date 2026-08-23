//! core::macho — read the macOS version a Mach-O binary DECLARES it needs.
//!
//! # Why this is read rather than recorded
//!
//! `docs/PORTS.md` carries a table of measured `minos` values per pinned binary,
//! and that table has already been wrong: it said "all PHP minors are 12.0"
//! while 8.0.30 was 14.0, from the day it was written. Nothing depended on the
//! error, which is exactly how it survived — a recorded number is only
//! load-bearing on the day something else moves.
//!
//! The number is in the artifact. Reading it there cannot drift with a re-pin.
//!
//! # What it is FOR, which is not refusing anything
//!
//! PostgreSQL's pinned builds declare `minos 26.0` while rexenv's own floor is
//! macOS 15, so every supported user below 26 is in the suspect band. Whether
//! dyld actually refuses them is unsettled and cannot be settled from a machine
//! on the newest macOS: measured 15 Aug 2026, dyld on macOS 26 enforces `minos`
//! for neither executables nor dylibs — postgres patched to `minos 99.0` runs
//! clean — while deterministic refusals of minos-15 binaries on macOS 14 are
//! documented in the wild. Enforcement is a property of the OLDER host's dyld.
//!
//! **So this must never gate a spawn.** Refusing on `minos` would block builds
//! that may run perfectly, on the strength of a prediction this project cannot
//! test. It is used only AFTER a service has already failed to start, to turn
//! "PostgreSQL did not start within 15s" into a sentence naming the version
//! mismatch — because a user reading the first one looks at Postgres, not at
//! their macOS version.

use std::path::Path;

const MH_MAGIC_64: u32 = 0xfeed_facf;
const FAT_MAGIC: u32 = 0xcafe_babe; // big-endian on disk
const LC_VERSION_MIN_MACOSX: u32 = 0x24;
const LC_BUILD_VERSION: u32 = 0x32;
/// `platform` value for macOS in LC_BUILD_VERSION. A binary built for iOS or the
/// simulator carries a `minos` that says nothing about this machine.
const PLATFORM_MACOS: u32 = 1;

fn u32_le(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u32_be(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// `xxxx.yy.zz` packed into a u32, as Mach-O stores it.
fn decode(v: u32) -> (u32, u32, u32) {
    ((v >> 16) & 0xffff, (v >> 8) & 0xff, v & 0xff)
}

/// The minimum macOS version `path` declares, as `(major, minor, patch)`.
///
/// `None` when the file is not a Mach-O, carries no version load command, or
/// declares a non-macOS platform — all of which mean "this tells us nothing",
/// never "it is fine". Callers must treat `None` as no information.
pub fn min_macos(path: &Path) -> Option<(u32, u32, u32)> {
    // The load commands live at the front; reading the whole binary to find them
    // would mean reading ~50 MB of postgres to answer a question about 32 bytes.
    let bytes = read_head(path, 64 * 1024)?;
    let magic = u32_be(&bytes, 0)?;
    let start = if magic == FAT_MAGIC {
        // Fat: take the FIRST slice. rexenv's binaries are per-arch after
        // `prepare_binary`, so this is a robustness path rather than the norm —
        // and the first slice is the right answer only when every slice agrees,
        // which is why `docs/PORTS.md` still records the arch caveat.
        let nfat = u32_be(&bytes, 4)?;
        if nfat == 0 {
            return None;
        }
        u32_be(&bytes, 8 + 8)? as usize // fat_arch: cputype, cpusubtype, offset
    } else {
        0
    };
    if u32_le(&bytes, start)? != MH_MAGIC_64 {
        return None;
    }
    let ncmds = u32_le(&bytes, start + 16)?;
    let mut at = start + 32; // mach_header_64
    for _ in 0..ncmds {
        let cmd = u32_le(&bytes, at)?;
        let size = u32_le(&bytes, at + 4)? as usize;
        if size < 8 {
            return None; // malformed: a zero-size command would loop forever
        }
        match cmd {
            LC_BUILD_VERSION => {
                if u32_le(&bytes, at + 8)? == PLATFORM_MACOS {
                    return Some(decode(u32_le(&bytes, at + 12)?));
                }
            }
            LC_VERSION_MIN_MACOSX => return Some(decode(u32_le(&bytes, at + 8)?)),
            _ => {}
        }
        at += size;
    }
    None
}

fn read_head(path: &Path, max: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

/// `true` when `need` is strictly newer than `host` — i.e. the binary declares a
/// macOS this machine does not have.
pub fn newer_than(need: (u32, u32, u32), host: (u32, u32, u32)) -> bool {
    need > host
}

/// The running macOS version, or `None` if it cannot be determined.
pub fn host_macos() -> Option<(u32, u32, u32)> {
    parse_version(&sysinfo::System::os_version()?)
}

/// `"15.4"` / `"26.4.1"` → `(15, 4, 0)` / `(26, 4, 1)`.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().split('.').map(|p| p.parse::<u32>().ok());
    let major = it.next()??;
    let minor = it.next().unwrap_or(Some(0))?;
    let patch = it.next().unwrap_or(Some(0))?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_the_way_sw_vers_prints_them() {
        assert_eq!(parse_version("26.4"), Some((26, 4, 0)));
        assert_eq!(parse_version("15.4.1"), Some((15, 4, 1)));
        assert_eq!(parse_version(" 14 "), Some((14, 0, 0)));
        assert_eq!(parse_version("Sonoma"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn newer_than_orders_by_component_not_by_string() {
        // "9" > "26" as text, which is the comparison this exists to avoid.
        assert!(newer_than((26, 0, 0), (15, 4, 0)));
        assert!(!newer_than((15, 0, 0), (26, 0, 0)));
        assert!(!newer_than((15, 4, 0), (15, 4, 0)));
        assert!(newer_than((15, 4, 1), (15, 4, 0)));
    }

    #[test]
    fn a_file_that_is_not_a_macho_says_nothing_rather_than_ok() {
        let dir = std::env::temp_dir().join(format!("rexenv-macho-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("notmacho");
        std::fs::write(&f, b"#!/bin/sh\necho hi\n").unwrap();
        assert_eq!(min_macos(&f), None);
        assert_eq!(min_macos(&dir.join("absent")), None);
        // Truncated header: the loop must not run off the end or spin.
        std::fs::write(&f, [0xcf, 0xfa, 0xed, 0xfe]).unwrap();
        assert_eq!(min_macos(&f), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Read the REAL pinned binaries and cross-check every answer against
    /// `otool -l`, the tool `docs/PORTS.md`'s table was measured with.
    ///
    /// **The skip is loud on purpose.** This project has already been bitten by
    /// a test that skipped silently for weeks while a ledger row counted it
    /// (`REXENV_LARAVEL_DOTENV`): a guard against drift that is itself quiet is
    /// worth less than no guard, because the row goes on claiming it.
    #[test]
    fn the_parser_agrees_with_otool_on_every_binary_in_the_cache() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let bin = std::path::Path::new(&home)
            .join("Library/Application Support/dev.rexenv.rexenv/bin");
        if !bin.is_dir() || std::process::Command::new("otool").arg("--version").output().is_err() {
            eprintln!(
                "SKIPPED the_parser_agrees_with_otool: needs a populated rexenv binary cache \
                 ({}) and /usr/bin/otool. Run the app once, then `cargo test macho`.",
                bin.display()
            );
            return;
        }

        let mut checked = 0;
        for entry in std::fs::read_dir(&bin).into_iter().flatten().flatten() {
            for candidate in ["php", "bin/postgres", "nginx", "caddy", "mailpit"] {
                let p = entry.path().join(candidate);
                if !p.is_file() {
                    continue;
                }
                let Ok(out) = std::process::Command::new("otool").arg("-l").arg(&p).output() else {
                    continue;
                };
                let text = String::from_utf8_lossy(&out.stdout);
                let Some(line) = text.lines().map(str::trim).find(|l| l.starts_with("minos ")) else {
                    continue;
                };
                let expected = parse_version(line.trim_start_matches("minos ")).unwrap();
                assert_eq!(
                    min_macos(&p),
                    Some(expected),
                    "{} — this parser disagrees with otool",
                    p.display()
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "the cache exists but no binary was read — the scan is broken, not the tree clean"
        );
        eprintln!("the_parser_agrees_with_otool: {checked} cached binaries cross-checked");
    }
}
