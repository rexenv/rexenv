//! The rexenv Sync plugin as an installable zip (plan §2.10, §11 Q1: the plugin
//! ships IN the app for v1 — "Download plugin" saves it to Downloads, the person
//! uploads it on the live site's Plugins → Add New → Upload).
//!
//! The files are compiled IN (`include_bytes!`), so the zip is always the plugin
//! this build speaks to — never a copy that drifted on disk. The list is written
//! out by hand, which is how a new plugin file gets left behind; the test below
//! walks `companion/rexenv-sync/` and fails on any file the list does not carry
//! (ledger #838). `tests/` stays out: it is the plugin's own test code, not the plugin.

use crate::error::{Error, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

macro_rules! plugin_file {
    ($rel:literal) => {
        ($rel, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../companion/rexenv-sync/", $rel)) as &[u8])
    };
}

/// Every file of the plugin, relative to its folder.
pub const FILES: [(&str, &[u8]); 8] = [
    plugin_file!("rexenv-sync.php"),
    plugin_file!("README.md"),
    plugin_file!("includes/class-rexenv-sync-admin.php"),
    plugin_file!("includes/class-rexenv-sync-pairing.php"),
    plugin_file!("includes/class-rexenv-sync-pusher.php"),
    plugin_file!("includes/class-rexenv-sync-reader.php"),
    plugin_file!("includes/class-rexenv-sync-rest.php"),
    plugin_file!("includes/class-rexenv-sync-signature.php"),
];

/// The zip's bytes: one `rexenv-sync/` folder, as WordPress's uploader expects.
pub fn zip_bytes() -> Result<Vec<u8>> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let err = |e: zip::result::ZipError| Error::Other(format!("building the plugin zip: {e}"));
    w.add_directory("rexenv-sync/", opts).map_err(err)?;
    w.add_directory("rexenv-sync/includes/", opts).map_err(err)?;
    for (rel, body) in FILES {
        w.start_file(format!("rexenv-sync/{rel}"), opts).map_err(err)?;
        w.write_all(body)?;
    }
    Ok(w.finish().map_err(err)?.into_inner())
}

/// Write the zip into `dir` as `rexenv-sync.zip`, numbered on collision
/// (`rexenv-sync-1.zip` …) — never over a file already there.
pub fn save_in(dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut dest = dir.join("rexenv-sync.zip");
    let mut n = 1;
    while dest.exists() {
        dest = dir.join(format!("rexenv-sync-{n}.zip"));
        n += 1;
    }
    std::fs::write(&dest, zip_bytes()?)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The zip carries every file of the plugin and nothing else, under one
    /// `rexenv-sync/` folder; saving never overwrites** (ledger #838). Plant:
    /// drop a line from `FILES` and the walk names the file left behind.
    #[test]
    fn the_zip_is_the_whole_plugin_and_never_overwrites() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../companion/rexenv-sync");
        let mut on_disk = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                let rel = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                if rel == "tests" || rel.ends_with(".DS_Store") {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    on_disk.push(rel);
                }
            }
        }
        on_disk.sort();
        let mut listed: Vec<String> = FILES.iter().map(|(r, _)| r.to_string()).collect();
        listed.sort();
        assert_eq!(listed, on_disk, "plugin_zip::FILES must list every plugin file (tests/ excepted)");

        let bytes = zip_bytes().unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
        assert!(names.iter().all(|n| n.starts_with("rexenv-sync/")), "{names:?}");
        let mut main = String::new();
        std::io::Read::read_to_string(&mut z.by_name("rexenv-sync/rexenv-sync.php").unwrap(), &mut main).unwrap();
        assert!(main.contains("Plugin Name:"), "the main file carries the plugin header");

        let dir = std::env::temp_dir().join(format!("rexenv-plugin-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = save_in(&dir).unwrap();
        let b = save_in(&dir).unwrap();
        assert_eq!(a.file_name().unwrap(), "rexenv-sync.zip");
        assert_eq!(b.file_name().unwrap(), "rexenv-sync-1.zip");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
