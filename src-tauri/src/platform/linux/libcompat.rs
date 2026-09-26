//! Shared-library shims for the binaries rexenv downloads but did not build — the pure rules.
//!
//! Oracle's generic Linux MySQL links `libaio.so.1`. Ubuntu 24.04 renamed the package AND the
//! soname in the time_t transition: `libaio1t64` ships `libaio.so.1t64` and nothing called
//! `libaio.so.1`, so the deb's `libaio1 | libaio1t64` dependency is satisfied and `mysqld`
//! still dies at exec — `error while loading shared libraries: libaio.so.1`, exit 127,
//! measured on the Dell's WSL Ubuntu 26.04, 26 Sep 2026, on the first x86_64 site create
//! (the 22.04 VM has `libaio1` and never showed it). theseus-rs's PostgreSQL (a Debian 12
//! build) links `libxml2.so.2`, and libxml2 2.14 bumped its soname to `libxml2.so.16` —
//! Ubuntu 26.04's `libxml2-16` — so `postgres` died the same way when the owner pressed Start
//! (27 Sep 2026); with the shim a real `initdb`, server start, `xmlparse` and `xpath` all
//! answered (the loader prints "no version information available" once, harmlessly). The same
//! ABI under a different name is a symlink away: rexenv keeps a private `lib-compat/` under
//! its app data with `<wanted> -> /usr/lib/<multiarch>/<renamed>` and spawns its services
//! with that directory on `LD_LIBRARY_PATH`. Nothing outside app data is written; a system
//! that has the real soname gets no shim and no env.
//!
//! This file decides WHICH shims a system needs from what its library directory holds; the
//! symlinks and the env live in `mod.rs`.

use std::path::{Path, PathBuf};

/// A soname a bundled binary asks for, and the file that provides the same ABI under
/// another name on this distribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shim {
    /// The name the binary's `DT_NEEDED` asks for (`libaio.so.1`).
    pub wanted: String,
    /// The file to point it at (`/usr/lib/x86_64-linux-gnu/libaio.so.1t64`).
    pub target: PathBuf,
}

/// The distribution's multiarch library directory for this build's arch.
pub(crate) fn multiarch_lib_dir(rust_arch: &str) -> PathBuf {
    let triplet = match rust_arch {
        "aarch64" => "aarch64-linux-gnu",
        _ => "x86_64-linux-gnu",
    };
    PathBuf::from("/usr/lib").join(triplet)
}

/// The known renames: `(wanted soname, what the distribution calls it now)`. The list is the
/// place the next transition lands; each row was measured on a machine where it bit.
const RENAMES: &[(&str, &str)] = &[
    // Ubuntu 24.04's time_t transition (MySQL, `libaio1t64`).
    ("libaio.so.1", "libaio.so.1t64"),
    // libxml2 2.14's soname bump, Ubuntu 26.04's `libxml2-16` (PostgreSQL).
    ("libxml2.so.2", "libxml2.so.16"),
];

/// Which shims this system needs, from the names present in its library directory: a
/// wanted soname that is ABSENT while its renamed twin is PRESENT. A system carrying the
/// real name needs nothing, and a system carrying neither gets nothing (the package
/// dependency, not a shim, is the answer there).
pub(crate) fn shims_for(lib_dir: &Path, present: &[String]) -> Vec<Shim> {
    RENAMES
        .iter()
        .filter(|(wanted, renamed)| !present.iter().any(|n| n == wanted) && present.iter().any(|n| n == renamed))
        .map(|(wanted, renamed)| Shim { wanted: (*wanted).into(), target: lib_dir.join(renamed) })
        .collect()
}

/// `LD_LIBRARY_PATH` with our directory FIRST and whatever the environment already had after it.
pub(crate) fn library_path(compat_dir: &Path, existing: Option<&str>) -> String {
    match existing.filter(|s| !s.is_empty()) {
        Some(rest) => format!("{}:{rest}", compat_dir.display()),
        None => compat_dir.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_t64_only_system_gets_the_libaio_shim_and_nothing_else_does() {
        let dir = Path::new("/usr/lib/x86_64-linux-gnu");
        let t64 = vec!["libaio.so.1t64".to_string(), "libaio.so.1t64.0.2".into(), "libnuma.so.1".into()];
        assert_eq!(
            shims_for(dir, &t64),
            vec![Shim { wanted: "libaio.so.1".into(), target: dir.join("libaio.so.1t64") }],
            "24.04: the renamed twin is there, the wanted name is not"
        );
        let resolute = vec!["libaio.so.1t64".to_string(), "libxml2.so.16".into(), "libxml2.so.16.1.2".into()];
        assert_eq!(
            shims_for(dir, &resolute).iter().map(|s| s.wanted.as_str()).collect::<Vec<_>>(),
            vec!["libaio.so.1", "libxml2.so.2"],
            "26.04: both renames, in the table's order"
        );
        let noble_xml = vec!["libaio.so.1".to_string(), "libxml2.so.2".into()];
        assert!(shims_for(dir, &noble_xml).is_empty(), "a system with both real names needs nothing");
        let jammy = vec!["libaio.so.1".to_string(), "libaio.so.1.0.1".into()];
        assert!(shims_for(dir, &jammy).is_empty(), "22.04 has the real soname — no shim, no env");
        let neither: Vec<String> = vec!["libnuma.so.1".into()];
        assert!(shims_for(dir, &neither).is_empty(), "no libaio at all is the deb dependency's problem, not a shim's");
        assert_eq!(multiarch_lib_dir("aarch64"), PathBuf::from("/usr/lib/aarch64-linux-gnu"));
        assert_eq!(multiarch_lib_dir("x86_64"), PathBuf::from("/usr/lib/x86_64-linux-gnu"));
    }

    #[test]
    fn our_directory_goes_first_and_keeps_what_was_there() {
        let d = Path::new("/home/u/.local/share/rexenv/lib-compat");
        assert_eq!(library_path(d, None), "/home/u/.local/share/rexenv/lib-compat");
        assert_eq!(library_path(d, Some("")), "/home/u/.local/share/rexenv/lib-compat");
        assert_eq!(library_path(d, Some("/opt/x/lib")), "/home/u/.local/share/rexenv/lib-compat:/opt/x/lib");
    }
}
