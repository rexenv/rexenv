//! An app's own icon, read out of its `.exe` — the Windows half of the rule the
//! macOS catalog keeps (`read_app_icon`): a REAL icon off the installed program,
//! never a hand-drawn brand table.
//!
//! Pure bytes, no Windows API, so the macOS test host runs these tests too
//! (`platform/mod.rs` includes this file under `cfg(test)` there, like `pe.rs`).
//! Until 16 Oct 2026 every Windows and Linux row carried `icon: None`, so the
//! browser and editor pickers there wore the monochrome fallback glyph for Chrome,
//! Firefox and VS Code alike while the same pickers on macOS showed each app's
//! icon (owner, 16 Oct 2026). "Honest fallback" was meant for an app whose icon
//! cannot be read — not for two whole operating systems.
//!
//! # What is read
//!
//! A PE image keeps icons as resources: `RT_GROUP_ICON` (14) is a directory of
//! sizes, each naming an `RT_ICON` (3) whose bytes are exactly what an `.ico`
//! file stores per image (a `BITMAPINFOHEADER` DIB, or a PNG for 256 px). The
//! first group is the application icon in every mainstream build. [`ico_from_exe`]
//! walks DOS header → PE signature → optional header's resource directory →
//! `.rsrc` section → the three-level resource tree, picks the best size for a
//! 16–32 px display (64 px preferred, 32-bit colour) and wraps that ONE image in an
//! `.ico` container, which a webview renders in an `<img>` as `data:image/x-icon`.
//!
//! Anything unexpected — a truncated header, a resource offset past the file, no
//! icon group — answers `None`, and the UI keeps its fallback glyph. A guess here
//! would be a wrong icon beside a right name.

/// `RT_ICON`.
const RT_ICON: u32 = 3;
/// `RT_GROUP_ICON`.
const RT_GROUP_ICON: u32 = 14;

/// Sizes in the order the picker prefers them, `0` meaning 256 (the `.ico`
/// convention). 64 first: crisp at 16 px on a 200 % display, small on the wire.
const PREFERRED_WIDTHS: [u8; 6] = [64, 48, 32, 128, 0, 16];

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// One section's address mapping.
#[derive(Clone, Copy)]
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_size: u32,
    raw_offset: u32,
}

/// How much of the file the headers can need: the DOS stub, the PE headers and
/// every section header all sit well inside this.
pub const HEAD_BYTES: usize = 64 * 1024;

/// The headers' section table, parsed from the first [`HEAD_BYTES`].
struct Headers {
    sections: Vec<Section>,
    /// The resource directory's RVA, from the optional header's data directory.
    rsrc_rva: u32,
}

/// The parsed pieces a resource walk needs: the section table and the bytes of
/// the ONE section holding the resource tree. Icons live inside `.rsrc` with the
/// tree, so nothing else of the file is ever read — `Code.exe` is ~170 MB, and
/// its icon is in the first few hundred KB of that section.
struct Image<'a> {
    rsrc: &'a [u8],
    section: Section,
    rsrc_rva: u32,
}

impl Headers {
    fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
            return None;
        }
        let pe = u32_at(bytes, 0x3c)? as usize;
        if bytes.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let coff = pe + 4;
        let section_count = u16_at(bytes, coff + 2)? as usize;
        let optional_size = u16_at(bytes, coff + 16)? as usize;
        let optional = coff + 20;
        let magic = u16_at(bytes, optional)?;
        // Data directories start at 96 (PE32) or 112 (PE32+); the resource table is
        // entry 2, eight bytes each (RVA, size).
        let directories = match magic {
            0x10b => optional + 96,
            0x20b => optional + 112,
            _ => return None,
        };
        let rsrc_rva = u32_at(bytes, directories + 2 * 8)?;
        if rsrc_rva == 0 {
            return None;
        }
        let mut sections = Vec::with_capacity(section_count);
        let mut at = optional + optional_size;
        for _ in 0..section_count {
            sections.push(Section {
                virtual_size: u32_at(bytes, at + 8)?,
                virtual_address: u32_at(bytes, at + 12)?,
                raw_size: u32_at(bytes, at + 16)?,
                raw_offset: u32_at(bytes, at + 20)?,
            });
            at += 40;
        }
        Some(Self { sections, rsrc_rva })
    }

    /// The section holding the resource directory.
    fn rsrc_section(&self) -> Option<&Section> {
        self.sections.iter().find(|s| {
            let span = s.virtual_size.max(s.raw_size);
            self.rsrc_rva >= s.virtual_address && self.rsrc_rva < s.virtual_address.saturating_add(span)
        })
    }
}

impl<'a> Image<'a> {
    /// Over a whole file in memory (tests, small files).
    fn from_bytes(bytes: &'a [u8]) -> Option<Self> {
        let headers = Headers::parse(bytes)?;
        let section = headers.rsrc_section()?;
        let start = section.raw_offset as usize;
        let end = start.checked_add(section.raw_size as usize)?;
        let rsrc = bytes.get(start..end)?;
        Some(Self {
            rsrc,
            section: Section { ..*section },
            rsrc_rva: headers.rsrc_rva,
        })
    }

    /// A relative virtual address → index into the resource section's bytes.
    /// An RVA outside that section is refused: nothing an icon needs lives elsewhere.
    fn offset(&self, rva: u32) -> Option<usize> {
        let s = &self.section;
        let span = s.virtual_size.max(s.raw_size);
        (rva >= s.virtual_address && rva < s.virtual_address.checked_add(span)?)
            .then(|| (rva - s.virtual_address) as usize)
    }

    /// The resource tree's base: every directory offset in it is relative to this.
    fn rsrc_base(&self) -> Option<usize> {
        self.offset(self.rsrc_rva)
    }

    /// The entries of one resource directory table: `(id, offset)` pairs, the
    /// offset carrying the subdirectory bit (0x8000_0000) as the file stores it.
    fn directory(&self, table: usize) -> Option<Vec<(u32, u32)>> {
        let base = self.rsrc_base()?;
        let at = base + table;
        let named = u16_at(self.rsrc, at + 12)? as usize;
        let ids = u16_at(self.rsrc, at + 14)? as usize;
        let count = named + ids;
        if count > 4096 {
            return None;
        }
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let e = at + 16 + i * 8;
            out.push((u32_at(self.rsrc, e)?, u32_at(self.rsrc, e + 4)?));
        }
        Some(out)
    }

    /// The first data entry under a type → name → language path, as raw bytes.
    fn first_data(&self, type_table: &[(u32, u32)], type_id: u32, wanted_name: Option<u32>) -> Option<&'a [u8]> {
        let (_, type_off) = type_table.iter().find(|(id, _)| *id == type_id)?;
        let names = self.directory((type_off & 0x7fff_ffff) as usize)?;
        let (_, name_off) = match wanted_name {
            Some(n) => names.iter().find(|(id, _)| *id == n)?,
            None => names.first()?,
        };
        let langs = self.directory((name_off & 0x7fff_ffff) as usize)?;
        let (_, data_off) = langs.first()?;
        if data_off & 0x8000_0000 != 0 {
            return None;
        }
        let entry = self.rsrc_base()? + *data_off as usize;
        let rva = u32_at(self.rsrc, entry)?;
        let size = u32_at(self.rsrc, entry + 4)? as usize;
        let start = self.offset(rva)?;
        self.rsrc.get(start..start.checked_add(size)?)
    }
}

/// One `GRPICONDIRENTRY`, the part the picker needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GroupEntry {
    width: u8,
    height: u8,
    color_count: u8,
    planes: u16,
    bit_count: u16,
    bytes_in_res: u32,
    id: u16,
}

fn group_entries(group: &[u8]) -> Option<Vec<GroupEntry>> {
    if u16_at(group, 2)? != 1 {
        return None;
    }
    let count = u16_at(group, 4)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = 6 + i * 14;
        out.push(GroupEntry {
            width: *group.get(e)?,
            height: *group.get(e + 1)?,
            color_count: *group.get(e + 2)?,
            planes: u16_at(group, e + 4)?,
            bit_count: u16_at(group, e + 6)?,
            bytes_in_res: u32_at(group, e + 8)?,
            id: u16_at(group, e + 12)?,
        });
    }
    Some(out)
}

/// The entry to show: the first preferred width that exists, and among those
/// the deepest colour.
fn pick(entries: &[GroupEntry]) -> Option<GroupEntry> {
    PREFERRED_WIDTHS
        .iter()
        .find_map(|w| entries.iter().filter(|e| e.width == *w).max_by_key(|e| e.bit_count).copied())
        .or_else(|| entries.first().copied())
}

/// Wrap one image's resource bytes in an `.ico` container.
fn ico_container(entry: GroupEntry, image: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(22 + image.len());
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&1u16.to_le_bytes()); // one image
    out.push(entry.width);
    out.push(entry.height);
    out.push(entry.color_count);
    out.push(0);
    out.extend_from_slice(&entry.planes.to_le_bytes());
    out.extend_from_slice(&entry.bit_count.to_le_bytes());
    out.extend_from_slice(&(image.len() as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());
    out.extend_from_slice(image);
    out
}

/// The application icon of a PE image as a one-image `.ico`, or `None` when the
/// bytes are not a PE image, carry no icon group, or point outside themselves.
pub fn ico_from_exe(bytes: &[u8]) -> Option<Vec<u8>> {
    ico_from_image(Image::from_bytes(bytes)?)
}

/// The same read from a file on disk, touching only the headers and the resource
/// section: `None` for anything that is not a PE image with an icon group.
pub fn ico_from_file(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; HEAD_BYTES];
    let n = f.read(&mut head).ok()?;
    head.truncate(n);
    let headers = Headers::parse(&head)?;
    let section = headers.rsrc_section()?;
    // A resource section larger than this is not an application's icon table.
    if section.raw_size > 64 * 1024 * 1024 {
        return None;
    }
    let mut rsrc = vec![0u8; section.raw_size as usize];
    f.seek(SeekFrom::Start(section.raw_offset as u64)).ok()?;
    f.read_exact(&mut rsrc).ok()?;
    ico_from_image(Image { rsrc: &rsrc, section: Section { ..*section }, rsrc_rva: headers.rsrc_rva })
}

fn ico_from_image(image: Image<'_>) -> Option<Vec<u8>> {
    let types = image.directory(0)?;
    let group = image.first_data(&types, RT_GROUP_ICON, None)?;
    let entries = group_entries(group)?;
    let chosen = pick(&entries)?;
    let data = image.first_data(&types, RT_ICON, Some(chosen.id as u32))?;
    // `bytes_in_res` is what the group claims; the resource's own size is what is
    // there. Trust the resource, cap by the claim only when the claim is smaller
    // (a trailing pad is harmless, a short read is not).
    let len = (chosen.bytes_in_res as usize).min(data.len()).max(1);
    Some(ico_container(chosen, &data[..len]))
}

/// `data:image/x-icon;base64,…` for `.ico` bytes.
pub fn ico_data_uri(ico: &[u8]) -> String {
    use base64::Engine;
    format!("data:image/x-icon;base64,{}", base64::engine::general_purpose::STANDARD.encode(ico))
}

/// The icon for a detected program: its `.exe`'s own icon group, or the file
/// itself when the catalog pointed at an `.ico`. The data URI the pickers render.
pub fn data_uri_for_file(path: &std::path::Path, bytes: &[u8]) -> Option<String> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".ico") {
        return (bytes.len() > 6 && u16_at(bytes, 2) == Some(1)).then(|| ico_data_uri(bytes));
    }
    ico_from_exe(bytes).map(|ico| ico_data_uri(&ico))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE32+ image with ONE section (`.rsrc`) holding one icon group of
    /// `sizes` and one `RT_ICON` per size whose payload is `payload(id)`.
    fn synthetic_exe(sizes: &[(u8, u16)], payload: impl Fn(u16) -> Vec<u8>) -> Vec<u8> {
        // --- resource section, built first so its offsets are known ---
        let mut rsrc = Vec::new();
        // Root table: two id entries (RT_ICON, RT_GROUP_ICON).
        let root = 0usize;
        let icon_names = 16 + 2 * 8; // after root
        let icon_name_count = sizes.len();
        let group_names = icon_names + 16 + icon_name_count * 8;
        let icon_langs: Vec<usize> = (0..icon_name_count).map(|i| group_names + 16 + 8 + i * (16 + 8)).collect();
        let group_lang = group_names + 16 + 8 + icon_name_count * (16 + 8);
        let data_entries = group_lang + 16 + 8; // (icons.., group) each 16 bytes
        let data_start = data_entries + (icon_name_count + 1) * 16;
        let _ = root;

        let table = |named: u16, ids: u16| -> Vec<u8> {
            let mut t = vec![0u8; 12];
            t.extend_from_slice(&named.to_le_bytes());
            t.extend_from_slice(&ids.to_le_bytes());
            t
        };
        let entry = |id: u32, off: u32| -> Vec<u8> {
            let mut e = id.to_le_bytes().to_vec();
            e.extend_from_slice(&off.to_le_bytes());
            e
        };
        // root
        rsrc.extend(table(0, 2));
        rsrc.extend(entry(RT_ICON, 0x8000_0000 | icon_names as u32));
        rsrc.extend(entry(RT_GROUP_ICON, 0x8000_0000 | group_names as u32));
        // RT_ICON names
        assert_eq!(rsrc.len(), icon_names);
        rsrc.extend(table(0, icon_name_count as u16));
        for (i, lang) in icon_langs.iter().enumerate() {
            rsrc.extend(entry(i as u32 + 1, 0x8000_0000 | *lang as u32));
        }
        // RT_GROUP_ICON names
        assert_eq!(rsrc.len(), group_names);
        rsrc.extend(table(0, 1));
        rsrc.extend(entry(1, 0x8000_0000 | group_lang as u32));
        // language tables → data entries
        for (i, lang) in icon_langs.iter().enumerate() {
            assert_eq!(rsrc.len(), *lang);
            rsrc.extend(table(0, 1));
            rsrc.extend(entry(0x409, (data_entries + i * 16) as u32));
        }
        assert_eq!(rsrc.len(), group_lang);
        rsrc.extend(table(0, 1));
        rsrc.extend(entry(0x409, (data_entries + icon_name_count * 16) as u32));
        assert_eq!(rsrc.len(), data_entries);

        const RSRC_RVA: u32 = 0x3000;
        // payloads, then the group dir, laid out after the data entries
        let mut blobs: Vec<Vec<u8>> = (0..icon_name_count).map(|i| payload(i as u16 + 1)).collect();
        let mut group = vec![0, 0, 1, 0];
        group.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
        for (i, (w, bits)) in sizes.iter().enumerate() {
            group.push(*w);
            group.push(*w);
            group.push(0);
            group.push(0);
            group.extend_from_slice(&1u16.to_le_bytes());
            group.extend_from_slice(&bits.to_le_bytes());
            group.extend_from_slice(&(blobs[i].len() as u32).to_le_bytes());
            group.extend_from_slice(&(i as u16 + 1).to_le_bytes());
        }
        blobs.push(group);
        let mut at = data_start;
        for blob in &blobs {
            rsrc.extend_from_slice(&(RSRC_RVA + at as u32).to_le_bytes());
            rsrc.extend_from_slice(&(blob.len() as u32).to_le_bytes());
            rsrc.extend_from_slice(&[0u8; 8]);
            at += blob.len();
        }
        assert_eq!(rsrc.len(), data_start);
        for blob in &blobs {
            rsrc.extend_from_slice(blob);
        }

        // --- headers ---
        const PE_AT: usize = 0x80;
        let mut exe = vec![0u8; PE_AT];
        exe[0] = b'M';
        exe[1] = b'Z';
        exe[0x3c..0x40].copy_from_slice(&(PE_AT as u32).to_le_bytes());
        exe.extend_from_slice(b"PE\0\0");
        // COFF: machine, 1 section, stamp, symtab, nsym, optional size 240 (PE32+), characteristics
        exe.extend_from_slice(&0x8664u16.to_le_bytes());
        exe.extend_from_slice(&1u16.to_le_bytes());
        exe.extend_from_slice(&[0u8; 12]);
        exe.extend_from_slice(&240u16.to_le_bytes());
        exe.extend_from_slice(&0u16.to_le_bytes());
        let optional_at = exe.len();
        let mut optional = vec![0u8; 240];
        optional[..2].copy_from_slice(&0x20bu16.to_le_bytes());
        let rsrc_dir = 112 + 2 * 8;
        optional[rsrc_dir..rsrc_dir + 4].copy_from_slice(&RSRC_RVA.to_le_bytes());
        optional[rsrc_dir + 4..rsrc_dir + 8].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        exe.extend_from_slice(&optional);
        assert_eq!(exe.len(), optional_at + 240);
        // one section header
        let raw_offset = 0x400u32;
        let mut sec = vec![0u8; 40];
        sec[..5].copy_from_slice(b".rsrc");
        sec[8..12].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        sec[12..16].copy_from_slice(&RSRC_RVA.to_le_bytes());
        sec[16..20].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        sec[20..24].copy_from_slice(&raw_offset.to_le_bytes());
        exe.extend_from_slice(&sec);
        exe.resize(raw_offset as usize, 0);
        exe.extend_from_slice(&rsrc);
        exe
    }

    fn fake_png(id: u16) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend(std::iter::repeat(id as u8).take(40 + id as usize));
        v
    }

    #[test]
    fn the_64px_32bit_image_is_chosen_and_wrapped_as_a_one_image_ico() {
        let exe = synthetic_exe(&[(16, 32), (64, 32), (0, 32), (32, 8)], fake_png);
        let ico = ico_from_exe(&exe).expect("an icon");
        // ICONDIR: reserved 0, type 1, count 1; entry: 64×64, 32-bit, offset 22.
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(ico[6], 64, "width");
        assert_eq!(u16_at(&ico, 12), Some(32), "bit count");
        assert_eq!(u32_at(&ico, 18), Some(22), "image offset");
        assert_eq!(&ico[22..], &fake_png(2)[..], "the 64 px resource's own bytes, whole");
        let uri = ico_data_uri(&ico);
        assert!(uri.starts_with("data:image/x-icon;base64,"));
    }

    #[test]
    fn without_a_64px_image_the_next_preferred_size_is_taken_and_colour_depth_breaks_ties() {
        let exe = synthetic_exe(&[(16, 32), (48, 8), (48, 32), (0, 32)], fake_png);
        let ico = ico_from_exe(&exe).unwrap();
        assert_eq!(ico[6], 48);
        assert_eq!(u16_at(&ico, 12), Some(32));
        assert_eq!(&ico[22..], &fake_png(3)[..]);
    }

    #[test]
    fn a_file_that_is_not_a_pe_image_or_has_no_icon_group_answers_none() {
        assert_eq!(ico_from_exe(b"not an exe at all"), None);
        assert_eq!(ico_from_exe(&[]), None);
        // A PE whose resource directory points past the file.
        let mut exe = synthetic_exe(&[(64, 32)], fake_png);
        let pe = u32_at(&exe, 0x3c).unwrap() as usize;
        let dir = pe + 24 + 112 + 16;
        exe[dir..dir + 4].copy_from_slice(&0x7fff_0000u32.to_le_bytes());
        assert_eq!(ico_from_exe(&exe), None, "an RVA no section holds is refused, not read as zeroes");
        // Truncated right after the headers.
        let exe = synthetic_exe(&[(64, 32)], fake_png);
        assert_eq!(ico_from_exe(&exe[..0x400]), None);
    }

    #[test]
    fn an_ico_file_is_served_as_is_and_an_exe_through_its_resources() {
        let ico = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/icon.ico")).expect("the app's own .ico");
        let uri = data_uri_for_file(std::path::Path::new(r"C:\Program Files\Git\git.ico"), &ico).unwrap();
        assert!(uri.starts_with("data:image/x-icon;base64,"));
        assert_eq!(data_uri_for_file(std::path::Path::new(r"C:\x\thing.ico"), b"garbage"), None);
        let exe = synthetic_exe(&[(64, 32)], fake_png);
        assert!(data_uri_for_file(std::path::Path::new(r"C:\x\Code.exe"), &exe).is_some());
    }

    /// A REAL image when one is on disk: the cross-compiled `rexenv.exe` (tauri's
    /// `winres` embeds the app icon). Says so and passes when there is none.
    #[test]
    fn a_real_cross_built_exe_yields_its_icon_when_present() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/xwin/x86_64-pc-windows-msvc/debug/rexenv.exe");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("no cross-built rexenv.exe at {path} — the real-image leg did not run");
            return;
        };
        let ico = ico_from_exe(&bytes).expect("rexenv.exe carries an icon group");
        assert!(ico.len() > 22 + 100, "an image, not an empty container: {} bytes", ico.len());
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        // The file reader — headers + `.rsrc` only — lands on the same bytes.
        assert_eq!(ico_from_file(std::path::Path::new(path)), Some(ico));
    }
}
