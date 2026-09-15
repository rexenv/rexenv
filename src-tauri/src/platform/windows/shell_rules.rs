//! The pure half of Windows' `ShellRunner::open` and `reveal` (W7 S2, plan §5 W7, ledger #621): what the
//! shell may be handed, and explorer's select argument. No Win32 here — `WindowsShell` in `mod.rs` makes the
//! calls — so this is compiled into the macOS test build.
//!
//! **Why a rule at all** (owner's ruling, 15 Sep 2026): `ShellExecuteW("open")` runs whatever a file's
//! association says — a `.bat`, a `.ps1`, a `.lnk`, a `.php` wired to `php.exe` — silently. macOS `open`
//! carries the same class, but on Windows it is wider and makes no sound. The callers are link and folder
//! buttons plus one "open log" button, so the shell gets `http(s)` URLs, existing folders, and existing
//! files whose extension is for READING; anything else is refused by name, pointing at "Show in Explorer".

/// Extensions a file may have to be opened (compared case-insensitively). For reading only: nothing here is
/// run by a default Windows association.
pub(crate) const VIEW_EXTENSIONS: &[&str] =
    &["log", "txt", "conf", "ini", "json", "xml", "md", "csv", "sql", "yml", "yaml"];

/// What `target` is on disk, as the caller found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnDisk {
    Folder,
    File,
    Missing,
}

/// What `open` may do with a target.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OpenTarget {
    Url,
    Folder,
    ViewFile,
    Refused(String),
}

/// Classify `target`. `on_disk` is only consulted for something that is not an `http(s)` URL.
pub(crate) fn classify_open(target: &str, on_disk: OnDisk) -> OpenTarget {
    let lower = target.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return OpenTarget::Url;
    }
    if has_scheme(&lower) {
        return OpenTarget::Refused(format!("rexenv opens only web links and folders from here, not `{target}`"));
    }
    match on_disk {
        OnDisk::Folder => OpenTarget::Folder,
        OnDisk::Missing => OpenTarget::Refused(format!("`{target}` does not exist")),
        OnDisk::File => {
            let name = target.rsplit(['\\', '/']).next().unwrap_or(target);
            match name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()) {
                Some(ext) if VIEW_EXTENSIONS.contains(&ext.as_str()) => OpenTarget::ViewFile,
                Some(ext) => OpenTarget::Refused(format!(
                    "rexenv won't open .{ext} files from here — Windows could run them. Use Show in Explorer instead."
                )),
                None => OpenTarget::Refused(format!(
                    "rexenv won't open `{name}` from here — a file with no extension could be anything. Use Show in \
                     Explorer instead."
                )),
            }
        }
    }
}

/// Whether `lower` starts with a URI scheme (`letters:`, two or more before the colon). The shell runs bare
/// schemes with no `//` too — `ms-settings:`, `shell:startup` — so `://` alone is not the test. One letter is
/// a drive (`c:\…`), and a UNC path (`\\server\share`) has no colon to start with.
fn has_scheme(lower: &str) -> bool {
    let Some((head, _)) = lower.split_once(':') else { return false };
    head.len() > 1
        && head.starts_with(|c: char| c.is_ascii_alphabetic())
        && head.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `explorer.exe`'s argument that opens the folder holding `path` with `path` selected. Passed as ONE raw
/// argument: explorer parses `/select,` itself, and a path with spaces must stay quoted inside it. A
/// Windows path cannot contain `"`, so quoting is enough. Forward slashes become backslashes: explorer's
/// `/select,` does not read them as separators.
pub(crate) fn reveal_argument(path: &str) -> String {
    format!("/select,\"{}\"", path.replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ledger #621 — the shell gets web links, folders and reading files; nothing it could run.
    #[test]
    fn open_hands_the_shell_links_folders_and_reading_files_only() {
        assert_eq!(classify_open("https://a.rex/", OnDisk::Missing), OpenTarget::Url);
        assert_eq!(classify_open("HTTP://127.0.0.1:8025", OnDisk::Missing), OpenTarget::Url);
        assert_eq!(classify_open(r"C:\Users\A B\Sites\shop", OnDisk::Folder), OpenTarget::Folder);
        assert_eq!(classify_open(r"C:\Sites\shop\wp-content\debug.LOG", OnDisk::File), OpenTarget::ViewFile);
        for file in [r"C:\x\run.bat", r"C:\x\a.ps1", r"C:\x\link.lnk", r"C:\x\index.php", r"C:\x\setup.exe", r"C:\x\notes.txt.cmd"] {
            match classify_open(file, OnDisk::File) {
                OpenTarget::Refused(why) => assert!(why.contains("Show in Explorer"), "{file}: {why}"),
                other => panic!("{file} → {other:?}"),
            }
        }
        assert!(matches!(classify_open(r"C:\x\Makefile", OnDisk::File), OpenTarget::Refused(_)));
        assert!(matches!(classify_open(r"C:\x\gone", OnDisk::Missing), OpenTarget::Refused(_)));
        // A non-web scheme is refused whatever the disk says — even when the caller found a FILE with a
        // reading extension, which no other rule here would refuse (a weaker set of cases passed with this
        // guard planted out: every one of them was refused by another rule).
        for scheme in ["file:///C:/x/notes.txt", "javascript://x.log", "mailto:a@b.md", "ms-settings:x.log", "shell:startup.txt"] {
            match classify_open(scheme, OnDisk::File) {
                OpenTarget::Refused(why) => assert!(why.contains("web links and folders"), "{scheme}: {why}"),
                other => panic!("{scheme} → {other:?}"),
            }
        }
        // A drive letter and a UNC share are paths, not schemes.
        assert_eq!(classify_open(r"D:\logs\php.log", OnDisk::File), OpenTarget::ViewFile);
        assert_eq!(classify_open(r"\\nas\share\sites", OnDisk::Folder), OpenTarget::Folder);
    }

    #[test]
    fn the_reveal_argument_keeps_a_spaced_path_quoted_inside_select() {
        assert_eq!(
            reveal_argument(r"C:\Users\A B\exports\shop.sql"),
            r#"/select,"C:\Users\A B\exports\shop.sql""#
        );
        assert_eq!(reveal_argument("C:/Users/A B/exports/shop.sql"), r#"/select,"C:\Users\A B\exports\shop.sql""#);
    }
}
