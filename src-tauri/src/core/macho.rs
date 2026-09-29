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
const LC_ID_DYLIB: u32 = 0x0d;
const LC_LOAD_DYLIB: u32 = 0x0c;
const LC_LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
const LC_REEXPORT_DYLIB: u32 = 0x8000_001f;
const LC_LOAD_UPWARD_DYLIB: u32 = 0x8000_0023;
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
    let mut found = None;
    walk_load_commands(path, |cmd, body| {
        match cmd {
            LC_BUILD_VERSION => {
                if u32_le(body, 8)? == PLATFORM_MACOS {
                    found = Some(decode(u32_le(body, 12)?));
                    return Some(false);
                }
            }
            LC_VERSION_MIN_MACOSX => {
                found = Some(decode(u32_le(body, 8)?));
                return Some(false);
            }
            _ => {}
        }
        Some(true)
    })?;
    found
}

/// What a Mach-O links against, read from its own load commands.
///
/// `id` is the install name a dylib declares (`LC_ID_DYLIB`; `None` for an
/// executable). `deps` are every `LC_LOAD_DYLIB` / weak / re-export / upward
/// load command in file order — the same list `otool -L` prints after its
/// header line, minus the ID line it prepends for a dylib.
///
/// # Why this exists (18 Sep 2026, clean-Mac smoke test)
///
/// `prepare_binary` used to ask `otool -L` for this list. `/usr/bin/otool` is
/// an Xcode Command Line Tools SHIM: on a Mac without the tools — every clean
/// Mac — it pops the "install developer tools?" dialog and exits non-zero, so
/// the first cold run failed on all six components with
/// `otool -L failed: xcode-select: error: Unable to get active developer
/// directory`. Every dev machine has the tools, which is exactly why 0.7.0 →
/// 0.7.2 shipped with it. Reading the load commands here needs nothing
/// installed, and the tests cross-check it against `otool` where that exists.
///
/// `None` means the file is not a 64-bit Mach-O this reader understands (or a
/// load command is malformed) — no information, never "no deps".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LinkedDylibs {
    pub id: Option<String>,
    pub deps: Vec<String>,
}

pub fn linked_dylibs(path: &Path) -> Option<LinkedDylibs> {
    let mut out = LinkedDylibs::default();
    walk_load_commands(path, |cmd, body| {
        match cmd {
            LC_ID_DYLIB => out.id = Some(dylib_name(body)?),
            LC_LOAD_DYLIB | LC_LOAD_WEAK_DYLIB | LC_REEXPORT_DYLIB | LC_LOAD_UPWARD_DYLIB => {
                out.deps.push(dylib_name(body)?)
            }
            _ => {}
        }
        Some(true)
    })?;
    Some(out)
}

/// The path string of a `dylib_command`: `name.offset` (u32 at +8) points at a
/// NUL-terminated string inside the command's own bytes.
fn dylib_name(body: &[u8]) -> Option<String> {
    let off = u32_le(body, 8)? as usize;
    let rest = body.get(off..)?;
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    String::from_utf8(rest[..end].to_vec()).ok()
}

/// Visit every load command of the FIRST 64-bit slice of `path`, in order.
/// `f(cmd, bytes)` gets the command's own bytes (header included) and returns
/// `Some(true)` to continue, `Some(false)` to stop early, `None` on malformed
/// input — which makes the whole walk `None`.
///
/// Reads exactly what the header says it needs (`sizeofcmds`), so a binary with
/// hundreds of load commands (postgres) is read whole and never truncated at an
/// arbitrary head size.
fn walk_load_commands(
    path: &Path,
    mut f: impl FnMut(u32, &[u8]) -> Option<bool>,
) -> Option<()> {
    let head = read_head(path, 4096)?;
    let magic = u32_be(&head, 0)?;
    let start = if magic == FAT_MAGIC {
        // Fat: take the FIRST slice. rexenv's binaries are per-arch after
        // `prepare_binary`, so this is a robustness path rather than the norm —
        // and the first slice is the right answer only when every slice agrees,
        // which is why `docs/PORTS.md` still records the arch caveat.
        let nfat = u32_be(&head, 4)?;
        if nfat == 0 {
            return None;
        }
        u32_be(&head, 8 + 8)? as usize // fat_arch: cputype, cpusubtype, offset
    } else {
        0
    };
    // mach_header_64: magic, cputype, cpusubtype, filetype, ncmds, sizeofcmds…
    let hdr = read_at(path, start, 32)?;
    if u32_le(&hdr, 0)? != MH_MAGIC_64 {
        return None;
    }
    let ncmds = u32_le(&hdr, 16)?;
    let sizeofcmds = u32_le(&hdr, 20)? as usize;
    // A header claiming megabytes of load commands is malformed, not interesting.
    if sizeofcmds > 4 * 1024 * 1024 {
        return None;
    }
    let cmds = read_at(path, start + 32, sizeofcmds)?;
    let mut at = 0;
    for _ in 0..ncmds {
        let cmd = u32_le(&cmds, at)?;
        let size = u32_le(&cmds, at + 4)? as usize;
        if size < 8 {
            return None; // malformed: a zero-size command would loop forever
        }
        let body = cmds.get(at..at + size)?;
        if !f(cmd, body)? {
            break;
        }
        at += size;
    }
    Some(())
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

/// Exactly `len` bytes at `offset`, or `None` when the file is shorter.
fn read_at(path: &Path, offset: usize, len: usize) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(offset as u64)).ok()?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn read_head(path: &Path, max: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

const LC_SEGMENT_64: u32 = 0x19;

/// Why [`rewrite_dylib_paths`] could not do its job.
#[derive(Debug, PartialEq, Eq)]
pub enum RewriteError {
    /// Not a 64-bit Mach-O this module reads (or malformed) — nothing was touched.
    NotMachO,
    /// The new load commands need more bytes than the header pad holds. Nothing was
    /// touched; the caller's fallback is `install_name_tool`, which cannot do better —
    /// it is the same pad — but says so in its own words.
    NoRoom { needed: usize, available: usize },
    Io(String),
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RewriteError::NotMachO => write!(f, "not a 64-bit Mach-O this build can rewrite"),
            RewriteError::NoRoom { needed, available } => write!(
                f,
                "the rewritten load commands need {needed} bytes and the header pad holds {available}"
            ),
            RewriteError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// What [`rewrite_dylib_paths`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rewritten {
    /// Load commands whose path changed.
    pub changed: usize,
    /// Whether the load-command region grew into the header pad (a longer path).
    pub grew: bool,
}

/// Rewrite the dylib load-command paths of `path` IN PLACE — what
/// `install_name_tool -id` / `-change` do, without the Xcode Command Line Tools.
///
/// `change(is_id, old)` answers `Some(new)` for a path to replace (`is_id` for the
/// dylib's own `LC_ID_DYLIB`, else a load of any flavour). Each rewritten command
/// is re-laid out as ld writes it (name at offset 24, NUL-padded to 8); the whole
/// region is rewritten at once and may grow into the header pad — the bytes between
/// the end of the load commands and the first section, which Homebrew links with
/// `-headerpad_max_install_names` for exactly this. A region that would not fit is
/// refused with [`RewriteError::NoRoom`] and the file is left byte-identical.
///
/// Why this exists: preparing a Homebrew-bottle bundle (redis / mariadb / httpd /
/// xdebug) rewrote `@@HOMEBREW_PREFIX@@/…` load commands to `@loader_path/…` with
/// `install_name_tool`, which IS the Command Line Tools — so on a clean Mac every
/// bundle install failed with "needs the Xcode Command Line Tools" (the clean-VM
/// smoke, 18 Sep 2026; ledger #676 closed the single-binary half by READING load
/// commands here, this closes the bundle half by WRITING them here). Signing is the
/// caller's, LAST, as before — a rewritten Mach-O has no valid signature until then.
///
/// Fat files: the FIRST slice, as [`linked_dylibs`] reads — rexenv's bundles are
/// thin, so this is the robustness path. The rewrite is a whole-file temp + rename
/// with the original permissions, so a crash mid-write leaves the old file.
pub fn rewrite_dylib_paths(
    path: &Path,
    mut change: impl FnMut(bool, &str) -> Option<String>,
) -> std::result::Result<Rewritten, RewriteError> {
    let mut file = std::fs::read(path).map_err(|e| RewriteError::Io(e.to_string()))?;
    let start = slice_start(&file).ok_or(RewriteError::NotMachO)?;
    let hdr = file.get(start..start + 32).ok_or(RewriteError::NotMachO)?;
    if u32_le(hdr, 0) != Some(MH_MAGIC_64) {
        return Err(RewriteError::NotMachO);
    }
    let ncmds = u32_le(hdr, 16).ok_or(RewriteError::NotMachO)? as usize;
    let sizeofcmds = u32_le(hdr, 20).ok_or(RewriteError::NotMachO)? as usize;
    if sizeofcmds > 4 * 1024 * 1024 {
        return Err(RewriteError::NotMachO);
    }
    let region_at = start + 32;
    let old = file.get(region_at..region_at + sizeofcmds).ok_or(RewriteError::NotMachO)?.to_vec();

    // Walk once: rebuild the region, and find where the pad ends (the first section's
    // file offset, relative to the slice).
    let mut new_region: Vec<u8> = Vec::with_capacity(sizeofcmds);
    let mut first_section: Option<usize> = None;
    let mut changed = 0;
    let mut at = 0;
    for _ in 0..ncmds {
        let cmd = u32_le(&old, at).ok_or(RewriteError::NotMachO)?;
        let size = u32_le(&old, at + 4).ok_or(RewriteError::NotMachO)? as usize;
        if size < 8 {
            return Err(RewriteError::NotMachO);
        }
        let body = old.get(at..at + size).ok_or(RewriteError::NotMachO)?;
        match cmd {
            LC_ID_DYLIB | LC_LOAD_DYLIB | LC_LOAD_WEAK_DYLIB | LC_REEXPORT_DYLIB | LC_LOAD_UPWARD_DYLIB => {
                let name = dylib_name(body).ok_or(RewriteError::NotMachO)?;
                match change(cmd == LC_ID_DYLIB, &name) {
                    Some(new) if new != name => {
                        new_region.extend_from_slice(&dylib_command(cmd, &body[8..24], &new));
                        changed += 1;
                    }
                    _ => new_region.extend_from_slice(body),
                }
            }
            LC_SEGMENT_64 => {
                // segment_command_64: cmd, cmdsize, segname[16], vmaddr, vmsize, fileoff,
                // filesize, maxprot, initprot, nsects, flags — then nsects × section_64
                // (80 bytes: sectname[16], segname[16], addr, size, offset u32 at +48, …).
                let nsects = u32_le(body, 64).ok_or(RewriteError::NotMachO)? as usize;
                for i in 0..nsects {
                    let sect = 72 + i * 80;
                    let size = u64_le(body, sect + 40).ok_or(RewriteError::NotMachO)?;
                    let offset = u32_le(body, sect + 48).ok_or(RewriteError::NotMachO)? as usize;
                    if size > 0 && offset > 0 {
                        first_section = Some(first_section.map_or(offset, |f| f.min(offset)));
                    }
                }
                new_region.extend_from_slice(body);
            }
            _ => new_region.extend_from_slice(body),
        }
        at += size;
    }
    if changed == 0 {
        return Ok(Rewritten::default());
    }
    // The room: up to the first section, or the old region when no section says.
    let available = first_section.map_or(sizeofcmds, |f| f.saturating_sub(32));
    if new_region.len() > available {
        return Err(RewriteError::NoRoom { needed: new_region.len(), available });
    }
    let grew = new_region.len() > sizeofcmds;
    let clear_to = region_at + sizeofcmds.max(new_region.len());
    if file.len() < clear_to {
        return Err(RewriteError::NotMachO);
    }
    file[region_at..clear_to].fill(0);
    file[region_at..region_at + new_region.len()].copy_from_slice(&new_region);
    file[start + 20..start + 24].copy_from_slice(&(new_region.len() as u32).to_le_bytes());

    // Whole-file temp + rename, the original mode kept: a torn write is never a file.
    let tmp = path.with_extension(format!("rewrite-{}.tmp", std::process::id()));
    let write = || -> std::io::Result<()> {
        std::fs::write(&tmp, &file)?;
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
        std::fs::rename(&tmp, path)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(RewriteError::Io(e.to_string()));
    }
    Ok(Rewritten { changed, grew })
}

/// A `dylib_command` as ld writes it: `cmd`, `cmdsize`, name offset 24, the 12 bytes of
/// timestamp / current / compatibility version carried over, the name NUL-padded to 8.
fn dylib_command(cmd: u32, versions: &[u8], name: &str) -> Vec<u8> {
    let mut body = name.as_bytes().to_vec();
    body.push(0);
    while (24 + body.len()) % 8 != 0 {
        body.push(0);
    }
    let mut c = Vec::with_capacity(24 + body.len());
    c.extend_from_slice(&cmd.to_le_bytes());
    c.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
    c.extend_from_slice(&24u32.to_le_bytes());
    c.extend_from_slice(&versions[..12.min(versions.len())]);
    while c.len() < 24 {
        c.push(0);
    }
    c.extend_from_slice(&body);
    c
}

fn u64_le(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at + 8)?;
    Some(u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

/// Where the first 64-bit slice begins: 0 for a thin file, the first `fat_arch`'s
/// offset for a fat one (the same choice [`walk_load_commands`] makes).
fn slice_start(file: &[u8]) -> Option<usize> {
    if u32_be(file, 0)? == FAT_MAGIC {
        let nfat = u32_be(file, 4)?;
        if nfat == 0 {
            return None;
        }
        return Some(u32_be(file, 8 + 8)? as usize);
    }
    Some(0)
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
    /// The floor is not set by our code — it is the LOWEST tier's floor
    /// (`docs/PLAN-macos-13-floor.md` §6.5: 13.0 since 23 Sep 2026; before the
    /// tiers it was the default stack's highest deployment target, 15.0). So
    /// moving it is a routine consequence of a pin decision, which is exactly
    /// why it needs a tripwire rather than a convention.
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
        const CASK_FLOOR_MAJOR: &str = "13";

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
    /// Build a thin 64-bit Mach-O header with the given load commands.
    fn thin_macho(cmds: &[Vec<u8>]) -> Vec<u8> {
        let size: usize = cmds.iter().map(Vec::len).sum();
        let mut b = Vec::new();
        b.extend_from_slice(&0xfeed_facfu32.to_le_bytes()); // magic
        b.extend_from_slice(&0x0100_000cu32.to_le_bytes()); // cputype arm64
        b.extend_from_slice(&0u32.to_le_bytes()); // cpusubtype
        b.extend_from_slice(&2u32.to_le_bytes()); // filetype MH_EXECUTE
        b.extend_from_slice(&(cmds.len() as u32).to_le_bytes());
        b.extend_from_slice(&(size as u32).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes()); // flags
        b.extend_from_slice(&0u32.to_le_bytes()); // reserved
        for c in cmds {
            b.extend_from_slice(c);
        }
        b
    }

    /// A `dylib_command` as ld writes it: name at offset 24, NUL-padded to 8.
    fn dylib_cmd(cmd: u32, name: &str) -> Vec<u8> {
        let mut body = name.as_bytes().to_vec();
        body.push(0);
        while (24 + body.len()) % 8 != 0 {
            body.push(0);
        }
        let mut c = Vec::new();
        c.extend_from_slice(&cmd.to_le_bytes());
        c.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
        c.extend_from_slice(&24u32.to_le_bytes()); // name.offset
        c.extend_from_slice(&[0u8; 12]); // timestamp, current, compat
        c.extend_from_slice(&body);
        c
    }

    #[test]
    fn linked_dylibs_reads_id_and_every_load_flavour_in_order_without_a_toolchain() {
        let dir = std::env::temp_dir().join(format!("rexenv-macho-deps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("lib.dylib");
        let mut cmds = vec![
            dylib_cmd(LC_ID_DYLIB, "@rpath/libme.dylib"),
            dylib_cmd(LC_LOAD_DYLIB, "/usr/lib/libSystem.B.dylib"),
            dylib_cmd(LC_LOAD_WEAK_DYLIB, "/opt/homebrew/opt/pcre2/lib/libpcre2-8.0.dylib"),
        ];
        // An unrelated command in the middle must be stepped over, not parsed.
        let mut other = Vec::new();
        other.extend_from_slice(&0x1du32.to_le_bytes()); // LC_CODE_SIGNATURE
        other.extend_from_slice(&16u32.to_le_bytes());
        other.extend_from_slice(&[0u8; 8]);
        cmds.push(other);
        cmds.push(dylib_cmd(LC_REEXPORT_DYLIB, "@loader_path/libre.dylib"));
        cmds.push(dylib_cmd(LC_LOAD_UPWARD_DYLIB, "@loader_path/libup.dylib"));
        std::fs::write(&f, thin_macho(&cmds)).unwrap();
        let got = linked_dylibs(&f).expect("a well-formed Mach-O reads");
        assert_eq!(got.id.as_deref(), Some("@rpath/libme.dylib"));
        assert_eq!(
            got.deps,
            [
                "/usr/lib/libSystem.B.dylib",
                "/opt/homebrew/opt/pcre2/lib/libpcre2-8.0.dylib",
                "@loader_path/libre.dylib",
                "@loader_path/libup.dylib",
            ]
        );

        // An executable has no ID; a dep-less one has an empty list, not None.
        let exe = dir.join("exe");
        std::fs::write(&exe, thin_macho(&[dylib_cmd(LC_LOAD_DYLIB, "/usr/lib/libSystem.B.dylib")]))
            .unwrap();
        let got = linked_dylibs(&exe).unwrap();
        assert_eq!(got.id, None);
        assert_eq!(got.deps, ["/usr/lib/libSystem.B.dylib"]);
        let bare = dir.join("bare");
        std::fs::write(&bare, thin_macho(&[])).unwrap();
        assert_eq!(linked_dylibs(&bare), Some(LinkedDylibs::default()));

        // Not a Mach-O, or a header whose commands run past the file: None.
        let junk = dir.join("junk");
        std::fs::write(&junk, b"#!/bin/sh\n").unwrap();
        assert_eq!(linked_dylibs(&junk), None);
        let truncated = dir.join("truncated");
        let full = thin_macho(&[dylib_cmd(LC_LOAD_DYLIB, "/usr/lib/libz.1.dylib")]);
        std::fs::write(&truncated, &full[..full.len() - 4]).unwrap();
        assert_eq!(linked_dylibs(&truncated), None, "a short read must not become 'no deps'");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `segment_command_64` with one section whose data starts at `first_section`
    /// (relative to the slice) — what bounds the header pad.
    fn segment_cmd(first_section: u32) -> Vec<u8> {
        let mut c = Vec::new();
        c.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
        c.extend_from_slice(&((72 + 80) as u32).to_le_bytes());
        c.extend_from_slice(b"__TEXT\0\0\0\0\0\0\0\0\0\0");
        c.extend_from_slice(&0u64.to_le_bytes()); // vmaddr
        c.extend_from_slice(&0x4000u64.to_le_bytes()); // vmsize
        c.extend_from_slice(&0u64.to_le_bytes()); // fileoff
        c.extend_from_slice(&0x4000u64.to_le_bytes()); // filesize
        c.extend_from_slice(&5u32.to_le_bytes()); // maxprot
        c.extend_from_slice(&5u32.to_le_bytes()); // initprot
        c.extend_from_slice(&1u32.to_le_bytes()); // nsects
        c.extend_from_slice(&0u32.to_le_bytes()); // flags
        // section_64
        c.extend_from_slice(b"__text\0\0\0\0\0\0\0\0\0\0");
        c.extend_from_slice(b"__TEXT\0\0\0\0\0\0\0\0\0\0");
        c.extend_from_slice(&(first_section as u64).to_le_bytes()); // addr
        c.extend_from_slice(&64u64.to_le_bytes()); // size
        c.extend_from_slice(&first_section.to_le_bytes()); // offset
        c.extend_from_slice(&[0u8; 28]); // align, reloff, nreloc, flags, reserved1-3
        c
    }

    /// **The rewriter changes exactly the paths it is asked to, in place when they fit
    /// and into the header pad when they do not, refuses without touching the file when
    /// even the pad is too small, and what it wrote is what the reader reads back.**
    /// A bottle's `@@HOMEBREW_PREFIX@@/…` → `@loader_path/…` shrink is the everyday
    /// case; the grow case is what `-headerpad_max_install_names` exists for.
    #[test]
    fn the_rewriter_relinks_in_place_grows_into_the_pad_and_refuses_beyond_it() {
        let dir = std::env::temp_dir().join(format!("rexenv-macho-rewrite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let brew = "@@HOMEBREW_PREFIX@@/opt/openssl@3/lib/libcrypto.3.dylib";
        let tree = "@loader_path/../lib/libcrypto.3.dylib";
        let build = |pad_to: u32, deps: &[&str]| {
            let mut cmds = vec![segment_cmd(pad_to), dylib_cmd(LC_ID_DYLIB, "@@HOMEBREW_PREFIX@@/opt/mariadb/lib/libme.dylib")];
            cmds.extend(deps.iter().map(|d| dylib_cmd(LC_LOAD_DYLIB, d)));
            let mut bytes = thin_macho(&cmds);
            bytes.resize(pad_to as usize + 64, 0xAA); // the "section" data after the pad
            bytes
        };
        // 1. Shrinking paths fit in place: no growth, the region's tail zeroed.
        let f = dir.join("shrink.dylib");
        let bytes = build(4096, &[brew, "/usr/lib/libSystem.B.dylib"]);
        std::fs::write(&f, &bytes).unwrap();
        let done = rewrite_dylib_paths(&f, |is_id, old| {
            if is_id { Some("@loader_path/../lib/libme.dylib".into()) } else if old == brew { Some(tree.into()) } else { None }
        })
        .unwrap();
        assert_eq!(done, Rewritten { changed: 2, grew: false });
        let got = linked_dylibs(&f).unwrap();
        assert_eq!(got.id.as_deref(), Some("@loader_path/../lib/libme.dylib"));
        assert_eq!(got.deps, [tree, "/usr/lib/libSystem.B.dylib"]);
        let after = std::fs::read(&f).unwrap();
        assert_eq!(after.len(), bytes.len(), "the file's length never changes");
        assert_eq!(&after[after.len() - 64..], &bytes[bytes.len() - 64..], "the section data is untouched");
        // 2. A longer path grows into the pad; sizeofcmds follows, ncmds does not.
        let f = dir.join("grow.dylib");
        std::fs::write(&f, build(4096, &[tree])).unwrap();
        let done = rewrite_dylib_paths(&f, |is_id, old| (!is_id && old == tree).then(|| brew.to_string())).unwrap();
        assert_eq!(done, Rewritten { changed: 1, grew: true });
        assert_eq!(linked_dylibs(&f).unwrap().deps, [brew]);
        let hdr = std::fs::read(&f).unwrap();
        assert_eq!(u32_le(&hdr, 16), Some(3), "ncmds unchanged");
        // 3. Beyond the pad: refused, and the file is byte-identical.
        let f = dir.join("noroom.dylib");
        let tight = build(32 + (72 + 80) + 72 + 64, &[tree]); // the pad ends right after the commands
        std::fs::write(&f, &tight).unwrap();
        let err = rewrite_dylib_paths(&f, |_, _| Some("x".repeat(300))).unwrap_err();
        assert!(matches!(err, RewriteError::NoRoom { .. }), "{err:?}");
        assert_eq!(std::fs::read(&f).unwrap(), tight, "a refusal touches nothing");
        // 4. Nothing to change: nothing written (mtime and bytes alike), and not a Mach-O is said.
        let before = std::fs::read(&f).unwrap();
        assert_eq!(rewrite_dylib_paths(&f, |_, _| None).unwrap(), Rewritten::default());
        assert_eq!(std::fs::read(&f).unwrap(), before);
        std::fs::write(dir.join("text"), b"not a mach-o at all").unwrap();
        assert_eq!(rewrite_dylib_paths(&dir.join("text"), |_, _| Some("y".into())).unwrap_err(), RewriteError::NotMachO);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The rewriter against a REAL bottle dylib, checked by the tools it replaces: a copy of
    /// the cached mariadb `libcrypto` gets a long Homebrew id through `install_name_tool`
    /// (the pad it was built with), the rewriter takes it back to the tree path, and
    /// `otool -L` reads exactly that; ad-hoc signing then succeeds and verifies. Skips
    /// (loudly) without the cache or the tools — the dev Mac has both.
    #[test]
    fn the_rewriter_agrees_with_otool_on_a_real_bottle_dylib() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let bin = std::path::Path::new(&home).join("Library/Application Support/dev.rexenv.rexenv/bin");
        let clt = std::process::Command::new("/usr/bin/xcode-select").arg("-p").output().is_ok_and(|o| o.status.success());
        let source = std::fs::read_dir(&bin).ok().into_iter().flatten().flatten()
            .map(|e| e.path().join("lib/libcrypto.3.dylib"))
            .find(|p| p.is_file());
        let (Some(source), true) = (source, clt) else {
            eprintln!("SKIPPED the_rewriter_agrees_with_otool: needs a cached mariadb bundle under {} and the Command Line Tools", bin.display());
            return;
        };
        let dir = std::env::temp_dir().join(format!("rexenv-macho-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("libcrypto.3.dylib");
        std::fs::copy(&source, &f).unwrap();
        let long = "/opt/homebrew/Cellar/openssl@3/3.5.1/lib/libcrypto.3.dylib";
        let st = std::process::Command::new("install_name_tool").args(["-id", long]).arg(&f).status().unwrap();
        assert!(st.success(), "install_name_tool could not set the long id (no pad?)");
        assert_eq!(linked_dylibs(&f).unwrap().id.as_deref(), Some(long));
        let tree = "@loader_path/../lib/libcrypto.3.dylib";
        let done = rewrite_dylib_paths(&f, |is_id, _| is_id.then(|| tree.to_string())).unwrap();
        assert_eq!(done.changed, 1);
        let out = std::process::Command::new("otool").arg("-L").arg(&f).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.lines().nth(1).is_some_and(|l| l.trim().starts_with(tree)), "otool -L:\n{text}");
        let sign = std::process::Command::new("codesign").args(["--force", "--sign", "-"]).arg(&f).output().unwrap();
        assert!(sign.status.success(), "codesign: {}", String::from_utf8_lossy(&sign.stderr));
        let verify = std::process::Command::new("codesign").args(["--verify", "--strict"]).arg(&f).status().unwrap();
        assert!(verify.success(), "the rewritten, signed dylib must verify");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
                // The same binary's dylib list — what `prepare_binary` now reads
                // here instead of asking `otool -L` (which a clean Mac lacks).
                let out = std::process::Command::new("otool").arg("-L").arg(&p).output().unwrap();
                let otool: Vec<String> = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .skip(1)
                    .filter_map(|l| l.split_whitespace().next().map(str::to_string))
                    .collect();
                let ours = linked_dylibs(&p).expect("cached binary reads");
                let mut ours_flat = ours.id.clone().into_iter().collect::<Vec<_>>();
                ours_flat.extend(ours.deps.clone());
                assert_eq!(ours_flat, otool, "{} — dylib list disagrees with otool -L", p.display());
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
