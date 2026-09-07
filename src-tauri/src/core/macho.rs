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

/// CPU types, as Mach-O writes them (the `0x0100_0000` bit is "64-bit").
const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;

/// Which architectures `path` actually contains, read from the file's own
/// headers.
///
/// # Why this is not a `lipo -archs` call
///
/// The self-update swap must refuse a staged bundle that is not universal —
/// shipping a thin build to the other half of the userbase turns an update into
/// a machine that cannot launch its own app. That check runs in `core`, and
/// `core` may never name an OS or shell out to a macOS tool (ledger #163); a
/// header read is also L0-testable against synthetic bytes, which `lipo` is not.
///
/// `None` means "this file tells us nothing" — not a Mach-O, truncated, or a
/// 64-bit fat header (`0xcafebabf`), which `lipo` can emit for very large
/// binaries and which rexenv's own `lipo -create` does not. A caller must treat
/// `None` as no information, never as a pass.
pub fn archs(path: &Path) -> Option<Vec<crate::platform::traits::Arch>> {
    // The fat header is 8 bytes plus 20 per slice; a thin header needs 8. 4 KiB
    // is enormously more than either and one read either way.
    let bytes = read_head(path, 4096)?;
    let magic = u32_be(&bytes, 0)?;
    let mut out = Vec::new();
    if magic == FAT_MAGIC {
        let nfat = u32_be(&bytes, 4)?;
        // A header claiming thousands of slices is malformed, not interesting.
        if nfat == 0 || nfat > 32 {
            return None;
        }
        for i in 0..nfat as usize {
            // fat_arch: cputype, cpusubtype, offset, size, align — big-endian.
            let cputype = u32_be(&bytes, 8 + i * 20)?;
            if let Some(a) = arch_of(cputype) {
                if !out.contains(&a) {
                    out.push(a);
                }
            }
        }
    } else if u32_le(&bytes, 0)? == MH_MAGIC_64 {
        // mach_header_64: magic, cputype, … — little-endian on both our targets.
        out.push(arch_of(u32_le(&bytes, 4)?)?);
    } else {
        return None;
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

fn arch_of(cputype: u32) -> Option<crate::platform::traits::Arch> {
    use crate::platform::traits::Arch;
    match cputype {
        CPU_TYPE_X86_64 => Some(Arch::X86_64),
        CPU_TYPE_ARM64 => Some(Arch::Arm64),
        _ => None,
    }
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
    /// **The app's macOS floor and the Homebrew cask's floor are ONE decision,
    /// and this is the only thing that connects them.**
    ///
    /// They live in different REPOSITORIES — `tauri.conf.json` here,
    /// `Casks/rexenv.rb` in `rexenv/homebrew-tap` — so nothing links them but a
    /// person remembering. Nobody did: the cask said `:big_sur` (11.0) through
    /// four releases while this app required 15.0, and a macOS 11–14 user could
    /// `brew install --cask rexenv`, get no refusal, and land on an app whose
    /// web server binary cannot start. The failure looks like a bug in rexenv
    /// rather than an unmet requirement, which is the worst shape it could take.
    ///
    /// **What made it invisible was a comment restating the number.** The cask
    /// carried `# minimumSystemVersion 11.0` beside the line — a NUMBER this
    /// repo is free to change without telling that file, and a reader who
    /// checks the line against its own comment finds them agreeing. So the fix
    /// is not "update the comment": it is this test, which fails HERE, in the
    /// repo that moves the floor, at the moment it moves.
    ///
    /// The floor is not set by our code — it is the highest deployment target
    /// among the binaries the default stack needs (nginx and cloudflared are
    /// both 15.0 today; `docs/PORTS.md`). So raising it is a routine
    /// consequence of a binary bump, which is exactly why it needs a tripwire
    /// rather than a convention.
    #[test]
    fn the_macos_floor_matches_the_shipped_cask() {
        // Homebrew's version symbols, so the failure can name the one to use
        // instead of leaving the reader to look it up (`macos_version.rb`).
        const HOMEBREW_SYMBOLS: &[(&str, &str)] = &[
            ("11", ":big_sur"),
            ("12", ":monterey"),
            ("13", ":ventura"),
            ("14", ":sonoma"),
            ("15", ":sequoia"),
            ("26", ":tahoe"),
        ];
        /// The major version the CASK currently demands, as a bare Homebrew
        /// symbol. Update BOTH this and `Casks/rexenv.rb` in the same change —
        /// that pairing is the whole point of the test.
        const CASK_FLOOR_MAJOR: &str = "15";

        let conf = include_str!("../../tauri.conf.json");
        let key = "\"minimumSystemVersion\"";
        let at = conf.find(key).expect(
            "tauri.conf.json has no minimumSystemVersion — if the key was renamed or removed, \
             this guard is no longer watching anything and the cask can drift again",
        );
        let value: String = conf[at + key.len()..]
            .trim_start()
            .trim_start_matches(':')
            .trim_start()
            .trim_start_matches('"')
            .chars()
            .take_while(|c| *c != '"')
            .collect();
        let major = value.split('.').next().unwrap_or_default().to_string();

        let symbol = HOMEBREW_SYMBOLS
            .iter()
            .find(|(m, _)| *m == major)
            .map(|(_, s)| *s)
            .unwrap_or("(no Homebrew symbol known for this major — check macos_version.rb)");

        assert_eq!(
            major, CASK_FLOOR_MAJOR,
            "\n\nThe app's macOS floor moved to {value} and the Homebrew cask still demands \
             macOS {CASK_FLOOR_MAJOR}.\n\nA user below the new floor installs with NO refusal \
             and lands on an app whose stack cannot start — it reads as a bug in rexenv.\n\n\
             Fix BOTH, in the same change:\n  \
             1. rexenv/homebrew-tap → Casks/rexenv.rb → `depends_on macos: {symbol}`\n     \
                (a bare symbol means \">= that version\"; the `\">= :sym\"` string form is \
                DEPRECATED in Homebrew and must not be used)\n  \
             2. this file → CASK_FLOOR_MAJOR = \"{major}\"\n\n\
             The floor follows the default stack's binaries (docs/PORTS.md), so it moves \
             whenever one of them is bumped. That is why this is a test and not a comment: \
             the cask carried `# minimumSystemVersion 11.0` beside `:big_sur` for four \
             releases, agreeing with itself and with nothing else."
        );
    }

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

    /// `archs` is the self-update swap's "is this build universal" check
    /// (`docs/archive/PLAN-self-update.md` §5), and it must be readable from `core`,
    /// which may not shell out to `lipo`. Synthetic headers, so the two shapes
    /// that matter are exercised without a 30 MB fixture.
    #[test]
    fn archs_reads_a_fat_header_and_a_thin_one_and_refuses_everything_else() {
        use crate::platform::traits::Arch;
        let dir = std::env::temp_dir().join(format!("rexenv-archs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // fat_header: magic, nfat_arch (big-endian) + one fat_arch per slice
        // (cputype, cpusubtype, offset, size, align).
        let mut fat = Vec::new();
        fat.extend_from_slice(&0xcafe_babeu32.to_be_bytes());
        fat.extend_from_slice(&2u32.to_be_bytes());
        for cputype in [0x0100_0007u32, 0x0100_000c] {
            fat.extend_from_slice(&cputype.to_be_bytes());
            fat.extend_from_slice(&0u32.to_be_bytes());
            fat.extend_from_slice(&4096u32.to_be_bytes());
            fat.extend_from_slice(&1024u32.to_be_bytes());
            fat.extend_from_slice(&12u32.to_be_bytes());
        }
        let f = dir.join("universal");
        std::fs::write(&f, &fat).unwrap();
        assert_eq!(archs(&f), Some(vec![Arch::X86_64, Arch::Arm64]));

        // mach_header_64: magic, cputype, … (little-endian).
        let mut thin = Vec::new();
        thin.extend_from_slice(&0xfeed_facfu32.to_le_bytes());
        thin.extend_from_slice(&0x0100_000cu32.to_le_bytes());
        thin.extend_from_slice(&[0u8; 24]);
        let t = dir.join("arm64only");
        std::fs::write(&t, &thin).unwrap();
        assert_eq!(archs(&t), Some(vec![Arch::Arm64]));
        // The check the swap actually makes: one slice is not universal.
        assert_ne!(archs(&t).unwrap().len(), 2);

        // Everything that tells us nothing says so, rather than passing. A
        // caller must never read `None` as "fine" — a shell script, a truncated
        // download and a fat header claiming 9000 slices are all "no answer".
        let s = dir.join("script");
        std::fs::write(&s, b"#!/bin/sh\n").unwrap();
        assert_eq!(archs(&s), None);
        assert_eq!(archs(&dir.join("absent")), None);
        std::fs::write(&s, &fat[..6]).unwrap();
        assert_eq!(archs(&s), None);
        let mut absurd = fat.clone();
        absurd[4..8].copy_from_slice(&9000u32.to_be_bytes());
        std::fs::write(&s, &absurd).unwrap();
        assert_eq!(archs(&s), None);
        // A fat binary of architectures we do not ship is not our universal.
        let mut foreign = fat.clone();
        foreign[8..12].copy_from_slice(&0x0000_000cu32.to_be_bytes()); // 32-bit arm
        foreign[28..32].copy_from_slice(&0x0000_0007u32.to_be_bytes()); // i386
        std::fs::write(&s, &foreign).unwrap();
        assert_eq!(archs(&s), None);

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
