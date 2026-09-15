//! The pure half of putting a folder on the user's `Path` (W8 S5, plan §5 W8 rulings Q3 and Q4, ledger #634):
//! which entry names the folder, the value with it added, the value without it. No Win32 here — `user_path.rs`
//! reads and writes `HKEY_CURRENT_USER\Environment` — so this is compiled into the macOS test build.
//!
//! Only the folder rexenv itself adds is ever matched or removed, and every other entry is kept exactly as the
//! user wrote it, in order: the value is the user's, and an install that rewrote it tidily would still have
//! rewritten it.

/// Whether one `Path` entry names `dir`: the same folder whatever its case (Windows paths are
/// case-insensitive), with or without a trailing separator or surrounding quotes. An entry written with an
/// environment variable (`%LOCALAPPDATA%\…`) is not expanded, so it is not this folder — rexenv writes the
/// literal path and manages only that.
pub(crate) fn same_dir(entry: &str, dir: &str) -> bool {
    fn fold(s: &str) -> &str {
        s.trim().trim_matches('"').trim_end_matches(['\\', '/'])
    }
    let (a, b) = (fold(entry), fold(dir));
    !a.is_empty() && a.eq_ignore_ascii_case(b)
}

/// Whether the `Path` value holds an entry naming `dir`.
pub(crate) fn contains(path: &str, dir: &str) -> bool {
    path.split(';').any(|entry| same_dir(entry, dir))
}

/// The value with `dir` appended at the end — unchanged when an entry already names it, a trailing `;` not
/// doubled, and just `dir` when the value is empty.
pub(crate) fn with_dir(path: &str, dir: &str) -> String {
    if contains(path, dir) {
        path.to_string()
    } else if path.trim().is_empty() {
        dir.to_string()
    } else if path.ends_with(';') {
        format!("{path}{dir}")
    } else {
        format!("{path};{dir}")
    }
}

/// The value without any entry naming `dir`; every other entry — empty ones included — kept as written, in
/// order.
pub(crate) fn without_dir(path: &str, dir: &str) -> String {
    path.split(';').filter(|entry| !same_dir(entry, dir)).collect::<Vec<_>>().join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"C:\Users\A B\AppData\Local\rexenv\bin";

    /// Ledger #634 — the folder is found whatever its case or trailing separator, and nothing else is.
    #[test]
    fn the_folder_is_found_in_any_case_and_nothing_else_is() {
        assert!(same_dir(r"c:\users\a b\appdata\local\rexenv\bin\", DIR));
        assert!(same_dir(r#""C:\Users\A B\AppData\Local\rexenv\bin""#, DIR));
        assert!(!same_dir(r"C:\Users\A B\AppData\Local\rexenv\bin\old", DIR));
        assert!(!same_dir(r"%LOCALAPPDATA%\rexenv\bin", DIR), "an unexpanded entry is not rexenv's literal folder");
        assert!(!same_dir("", DIR), "an empty entry names nothing");
        assert!(contains(&format!(r"C:\Windows;{DIR}\;D:\tools"), DIR));
        assert!(!contains(r"C:\Windows;D:\tools", DIR));
    }

    /// Ledger #634 — adding keeps the user's value and appends once.
    #[test]
    fn adding_appends_once_and_keeps_the_users_value() {
        // The Dell's shape: a REG_SZ value ending without a separator.
        let dell = r"C:\Users\DELL\AppData\Local\Microsoft\WindowsApps;C:\Users\DELL\AppData\Roaming\npm";
        let added = with_dir(dell, DIR);
        assert_eq!(added, format!("{dell};{DIR}"));
        assert_eq!(with_dir(&added, DIR), added, "a second install changes nothing");
        assert_eq!(with_dir(r"C:\a;", DIR), format!(r"C:\a;{DIR}"), "a trailing separator is not doubled");
        assert_eq!(with_dir("", DIR), DIR);
        assert_eq!(with_dir("  ", DIR), DIR);
    }

    /// Ledger #634 — removing takes only rexenv's folder and keeps every other entry as written.
    #[test]
    fn removing_takes_only_the_folder_and_keeps_every_other_entry() {
        let value = format!(r"C:\Windows;;{DIR};%USERPROFILE%\bin;{}\", DIR.to_lowercase());
        assert_eq!(without_dir(&value, DIR), r"C:\Windows;;%USERPROFILE%\bin");
        assert_eq!(without_dir(r"C:\Windows;D:\tools;", DIR), r"C:\Windows;D:\tools;", "nothing of ours: unchanged");
        let dell = r"C:\Users\DELL\AppData\Roaming\npm";
        assert_eq!(without_dir(&with_dir(dell, DIR), DIR), dell, "add then remove is the value the user had");
    }
}
