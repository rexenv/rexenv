//! Ledger #597 and #598 on a real Windows machine: owner-only files and the binary
//! checks, through the platform traits `core` calls.
//!
//! ```text
//! windows_files_check.exe acl <dir>    # write the private files; the runner then reads them back
//!                                      # with Get-Acl and as NT AUTHORITY\LOCAL SERVICE
//! windows_files_check.exe motw <dir>   # Mark of the Web removal, image refusals, the tree check,
//!                                      # and whether rexenv's own downloads carry a mark at all
//! ```
//!
//! Driven by `scripts/probes/windows-files-check.sh`, which runs the cross-account read (a
//! process running as another account is the only honest test of "owner-only"). Fixture-owned:
//! everything it writes lives under `<dir>`; the binary-cache scan only reads. On macOS it prints
//! a skip line. Classified `demo`: it takes arguments and needs a Windows host.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_files_check: skipped — a Windows live check (ledger #597, #598)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;

    pub fn main() -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        let (Some(mode), Some(dir)) = (args.get(1), args.get(2)) else {
            eprintln!("usage: windows_files_check acl|motw <dir>");
            return ExitCode::FAILURE;
        };
        let dir = PathBuf::from(dir);
        let plat = rexenv_lib::platform::current();
        match mode.as_str() {
            "acl" => acl(&*plat, &dir),
            "motw" => motw(&*plat, &dir),
            other => {
                eprintln!("unknown mode {other:?}");
                ExitCode::FAILURE
            }
        }
    }

    /// Three files beside a control, for the runner to read back as another account:
    /// `born.txt` created by `write_private`; `hardened.txt` written world-readable, then
    /// `set_private`; `rewritten.txt` written world-readable, then `write_private` over it
    /// (the re-harden-before-write path); `control.txt` plain.
    fn acl(plat: &dyn rexenv_lib::platform::traits::Platform, dir: &Path) -> ExitCode {
        let mut check = Check::new("windows_files_check acl");
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).expect("fixture dir");
        let perms = plat.permissions();
        let secret = b"rexenv owner-only fixture";

        std::fs::write(dir.join("control.txt"), secret).unwrap();
        let born = perms.write_private(&dir.join("born.txt"), secret);
        check.is("write_private creates a new file", born.is_ok(), &format!("{born:?}"));
        std::fs::write(dir.join("hardened.txt"), secret).unwrap();
        let hardened = perms.set_private(&dir.join("hardened.txt"));
        check.is("set_private hardens an existing file", hardened.is_ok(), &format!("{hardened:?}"));
        std::fs::write(dir.join("rewritten.txt"), b"old world-readable contents").unwrap();
        let rewritten = perms.write_private(&dir.join("rewritten.txt"), secret);
        check.is("write_private over an existing file", rewritten.is_ok(), &format!("{rewritten:?}"));

        for name in ["born.txt", "hardened.txt", "rewritten.txt"] {
            let back = std::fs::read(dir.join(name)).unwrap_or_default();
            check.is(&format!("{name}: its owner still reads it back"), back == secret, &String::from_utf8_lossy(&back));
        }
        let missing = perms.set_private(&dir.join("no-such-file.txt"));
        check.is("set_private on a missing file is an error, never a silent success", missing.is_err(), "Ok");
        let data = plat.paths().app_data_dir().map(|p| p.display().to_string()).unwrap_or_default();
        println!("  · app_data_dir: {data}");
        check.is(
            "app_data_dir is under Local AppData",
            std::env::var("LOCALAPPDATA").is_ok_and(|l| data.starts_with(&l)) && data.ends_with(r"rexenv\rexenv\data"),
            &data,
        );
        check.verdict()
    }

    fn motw(plat: &dyn rexenv_lib::platform::traits::Platform, dir: &Path) -> ExitCode {
        let mut check = Check::new("windows_files_check motw");
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).expect("fixture dir");
        let bins = plat.binaries();
        let me = std::env::current_exe().expect("current exe");

        // A single executable with a Mark of the Web, as a browser would leave it.
        let app = dir.join("app.exe");
        std::fs::copy(&me, &app).unwrap();
        mark(&app);
        check.is("fixture: app.exe carries a Zone.Identifier", has_mark(&app), "no stream written");
        let prepared = bins.prepare_binary(&app);
        check.is("prepare_binary accepts a real x64 image", prepared.is_ok(), &format!("{prepared:?}"));
        check.is("prepare_binary removed the Mark of the Web", !has_mark(&app), "stream still there");
        check.is("prepare_binary on an unmarked file is fine too (idempotent)", bins.prepare_binary(&app).is_ok(), "errored");

        // The same bytes claiming to be arm64, and a file that is no image at all.
        let arm = dir.join("arm64.exe");
        std::fs::write(&arm, with_machine(&std::fs::read(&me).unwrap(), 0xAA64)).unwrap();
        let refused = bins.prepare_binary(&arm).map_err(|e| e.to_string());
        check.is(
            "an arm64 image is refused, naming the machine",
            refused.as_ref().is_err_and(|e| e.contains("0xaa64")),
            &format!("{refused:?}"),
        );
        let text = dir.join("notpe.exe");
        std::fs::write(&text, b"<html>404 Not Found</html>").unwrap();
        let refused = bins.prepare_binary(&text).map_err(|e| e.to_string());
        check.is(
            "bytes that are no executable are refused",
            refused.as_ref().is_err_and(|e| e.contains("not a Windows executable")),
            &format!("{refused:?}"),
        );

        // Directory distributions: every file loses its mark, every image is checked.
        let tree = dir.join("tree");
        std::fs::create_dir_all(tree.join("bin")).unwrap();
        std::fs::copy(&me, tree.join("bin").join("server.exe")).unwrap();
        std::fs::write(tree.join("README.txt"), b"docs").unwrap();
        mark(&tree.join("bin").join("server.exe"));
        mark(&tree.join("README.txt"));
        let ok = bins.prepare_binary_dir(&tree);
        check.is("prepare_binary_dir accepts a tree of x64 images", ok.is_ok(), &format!("{ok:?}"));
        check.is(
            "prepare_binary_dir removed every mark, images and not",
            !has_mark(&tree.join("bin").join("server.exe")) && !has_mark(&tree.join("README.txt")),
            "a stream survived",
        );
        std::fs::copy(&arm, tree.join("bin").join("helper.dll")).unwrap();
        let refused = bins.prepare_binary_dir(&tree).map_err(|e| e.to_string());
        check.is(
            "one arm64 DLL in a tree refuses the tree",
            refused.as_ref().is_err_and(|e| e.contains("helper.dll") && e.contains("0xaa64")),
            &format!("{refused:?}"),
        );
        let docs = dir.join("docs-only");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("a.txt"), b"x").unwrap();
        let refused = bins.prepare_binary_dir(&docs).map_err(|e| e.to_string());
        check.is(
            "a tree with no image is not a binary tree",
            refused.as_ref().is_err_and(|e| e.contains("no .exe or .dll")),
            &format!("{refused:?}"),
        );

        // The claim that was only believed: rexenv's own downloads carry no mark.
        match plat.paths().bin_dir() {
            Ok(bin) if bin.is_dir() => {
                let mut files = Vec::new();
                walk(&bin, &mut files);
                let marked: Vec<&PathBuf> = files.iter().filter(|f| has_mark(f)).collect();
                println!("  · scanned {} files under {}", files.len(), bin.display());
                check.is(
                    "no file rexenv downloaded and extracted itself carries a Mark of the Web",
                    !files.is_empty() && marked.is_empty(),
                    &format!("{} of {} marked: {:?}", marked.len(), files.len(), marked.iter().take(3).collect::<Vec<_>>()),
                );
            }
            _ => println!("  · no binary cache on this machine — the download scan is skipped"),
        }

        let _ = std::fs::remove_dir_all(dir);
        check.verdict()
    }

    fn mark(path: &Path) {
        let mut stream = path.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        std::fs::write(&stream, b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.invalid/\r\n").expect("write Zone.Identifier");
    }

    fn has_mark(path: &Path) -> bool {
        let mut stream = path.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        std::fs::metadata(&stream).is_ok()
    }

    /// A copy of a PE image with its COFF machine field rewritten.
    fn with_machine(image: &[u8], machine: u16) -> Vec<u8> {
        let mut out = image.to_vec();
        let pe = u32::from_le_bytes(out[0x3C..0x40].try_into().unwrap()) as usize;
        assert_eq!(&out[pe..pe + 4], b"PE\0\0", "this executable is not a PE image");
        out[pe + 4..pe + 6].copy_from_slice(&machine.to_le_bytes());
        out
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                walk(&entry.path(), out);
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
    }
}
