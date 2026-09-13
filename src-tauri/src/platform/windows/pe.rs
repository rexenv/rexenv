//! Just enough of the PE format to tell whether a file is a Windows executable for
//! the machine rexenv targets.
//!
//! Pure bytes, no Windows API, so the macOS test host runs these tests too
//! (`platform/mod.rs` includes this file under `cfg(test)` there). Used by
//! `WindowsBinaryProvider` to refuse publishing an artifact that could never run:
//! a pin that names the wrong archive member, an x86 or arm64 build where every
//! Windows pin is x64 (plan D6), or bytes that are not an executable at all
//! (ledger #598).

/// `IMAGE_FILE_MACHINE_AMD64` — the COFF machine of every Windows artifact rexenv pins.
pub const MACHINE_AMD64: u16 = 0x8664;

/// How many leading bytes [`classify`] is given. Real executables put the PE header
/// within the first few hundred bytes (`e_lfanew` after the DOS stub); a header past
/// this reads as [`PeKind::NotPe`], which fails loud rather than passing a file.
pub const HEAD_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeKind {
    /// A PE image whose COFF machine is x64.
    X64,
    /// A PE image for another machine (`0x014c` x86, `0xaa64` arm64, …).
    OtherMachine(u16),
    /// Not a PE image: no `MZ`, no `PE\0\0` where `e_lfanew` points, or too short.
    NotPe,
}

/// Classify the first bytes of a file: the `MZ` DOS header, the little-endian
/// offset at `0x3C` to the `PE\0\0` signature, and the COFF `Machine` field after it.
pub fn classify(head: &[u8]) -> PeKind {
    if head.len() < 0x40 || &head[..2] != b"MZ" {
        return PeKind::NotPe;
    }
    let offset = u32::from_le_bytes([head[0x3C], head[0x3D], head[0x3E], head[0x3F]]) as usize;
    let Some(end) = offset.checked_add(6) else {
        return PeKind::NotPe;
    };
    let Some(header) = head.get(offset..end) else {
        return PeKind::NotPe;
    };
    if &header[..4] != b"PE\0\0" {
        return PeKind::NotPe;
    }
    match u16::from_le_bytes([header[4], header[5]]) {
        MACHINE_AMD64 => PeKind::X64,
        other => PeKind::OtherMachine(other),
    }
}

/// Whether a file name is one the tree check must read as a PE image: `.exe` or
/// `.dll`, in any case (Windows file names are case-insensitive, and PHP's zip ships
/// both `php.exe` and `ext/php_*.dll`). Everything else in a tree — configs, licences,
/// `.pdb` symbols — is data and is not classified.
pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".exe") || lower.ends_with(".dll")
}

#[cfg(test)]
mod tests {
    use super::{classify, is_image_name, PeKind, MACHINE_AMD64};

    #[test]
    fn executables_and_libraries_are_images_whatever_their_case() {
        for yes in ["php.exe", "PHP-CGI.EXE", "php8ts.dll", "ext\\Php_Pdo_Pgsql.DLL"] {
            assert!(is_image_name(yes), "{yes}");
        }
        for no in ["php.ini-development", "license.txt", "php.pdb", "exe", "dll.txt", "nginx.conf"] {
            assert!(!is_image_name(no), "{no}");
        }
    }

    /// The layout a linker writes: a DOS header whose `e_lfanew` (at 0x3C) points
    /// past a DOS stub to `PE\0\0`, then the COFF file header starting with Machine.
    fn image(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x100];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn an_x64_image_is_x64_and_other_machines_are_named() {
        assert_eq!(classify(&image(MACHINE_AMD64)), PeKind::X64);
        assert_eq!(classify(&image(0x014c)), PeKind::OtherMachine(0x014c));
        assert_eq!(classify(&image(0xaa64)), PeKind::OtherMachine(0xaa64));
    }

    #[test]
    fn bytes_that_are_not_an_image_are_not_pe() {
        let html = b"<!doctype html><html><body>Not Found</body></html>".repeat(4);
        assert_eq!(classify(&html), PeKind::NotPe, "an error page");
        assert_eq!(classify(b"MZ"), PeKind::NotPe, "too short for a DOS header");
        let mut zip = image(MACHINE_AMD64);
        zip[..2].copy_from_slice(b"PK");
        assert_eq!(classify(&zip), PeKind::NotPe, "a zip, not its member");
        let mut past_end = image(MACHINE_AMD64);
        past_end[0x3C..0x40].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        assert_eq!(classify(&past_end), PeKind::NotPe, "e_lfanew beyond the bytes read");
        let mut no_signature = image(MACHINE_AMD64);
        no_signature[0x80..0x84].copy_from_slice(b"NE\0\0");
        assert_eq!(classify(&no_signature), PeKind::NotPe, "an old NE image, not PE");
    }
}
