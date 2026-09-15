//! How this OS finds a command on `PATH` — what separates the directories, whether `PATH` and `Path` are
//! one variable, and which file names a bare command may be (ledger #628). Owned by the platform because
//! the core got it wrong once: `core/devtools.rs` looked up the key `PATH`, split it on `:` and asked for a
//! file named `git`. On Windows the key is `Path`, `C:\Program Files\Git\cmd` splits into `C` and
//! `\Program Files\Git\cmd`, and the file is `git.exe` — so "From Git" told the owner git was missing on a
//! Dell with Git for Windows and Node.js installed (W7, 15 Sep 2026).
//!
//! Both rule sets are plain data compiled on every host, so their tests run everywhere; [`current`] picks
//! this build's (the `platform/words.rs` shape).

use std::path::{Path, PathBuf};

/// One operating system's rules for finding a command on `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathLookup {
    /// What separates the directories in `PATH`.
    pub separator: char,
    /// Whether variable names ignore case (`PATH` from a process and `Path` from the registry are one).
    pub names_ignore_case: bool,
    /// The variable listing the extensions a bare command name may carry, if this OS has one.
    pub extensions_var: Option<&'static str>,
    /// The extensions used when that variable is unset or empty.
    pub default_extensions: &'static str,
}

pub const UNIX: PathLookup = PathLookup { separator: ':', names_ignore_case: false, extensions_var: None, default_extensions: "" };

/// cmd.exe's own list stands in when `PATHEXT` is missing.
pub const WINDOWS: PathLookup =
    PathLookup { separator: ';', names_ignore_case: true, extensions_var: Some("PATHEXT"), default_extensions: ".COM;.EXE;.BAT;.CMD" };

/// This build's rules.
pub fn current() -> &'static PathLookup {
    #[cfg(target_os = "windows")]
    {
        &WINDOWS
    }
    #[cfg(not(target_os = "windows"))]
    {
        &UNIX
    }
}

impl PathLookup {
    /// A variable's value from an env snapshot, by this OS's naming rule.
    pub fn var<'a>(&self, env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter()
            .find(|(k, _)| if self.names_ignore_case { k.eq_ignore_ascii_case(key) } else { k == key })
            .map(|(_, v)| v.as_str())
    }

    /// Every file `name` could be, in the order a shell tries them: each `PATH` directory in turn, and in
    /// one directory the name as written (on Windows only when it already has an extension), then the name
    /// with each listed extension.
    pub fn candidates(&self, env: &[(String, String)], name: &str) -> Vec<PathBuf> {
        let Some(path) = self.var(env, "PATH") else { return Vec::new() };
        let names: Vec<String> = match self.extensions_var {
            None => vec![name.to_string()],
            Some(var) => {
                let listed = self.var(env, var).filter(|v| !v.trim().is_empty()).unwrap_or(self.default_extensions);
                let as_written = Path::new(name).extension().is_some().then(|| name.to_string());
                let with_ext = listed
                    .split(';')
                    .map(str::trim)
                    .filter(|e| !e.is_empty())
                    .map(|e| format!("{name}{}", e.to_ascii_lowercase()));
                as_written.into_iter().chain(with_ext).collect()
            }
        };
        path.split(self.separator)
            .map(|d| d.trim_matches('"'))
            .filter(|d| !d.is_empty())
            .flat_map(|d| names.iter().map(move |n| Path::new(d).join(n)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn unix_splits_on_colons_and_takes_the_name_as_written() {
        let e = env(&[("PATH", "/opt/homebrew/bin::/usr/bin"), ("PATHEXT", ".EXE")]);
        assert_eq!(
            UNIX.candidates(&e, "git"),
            vec![Path::new("/opt/homebrew/bin").join("git"), Path::new("/usr/bin").join("git")]
        );
        assert!(UNIX.candidates(&env(&[("Path", "/usr/bin")]), "git").is_empty(), "a Unix name is case-sensitive");
    }

    #[test]
    fn windows_reads_path_under_any_spelling_splits_on_semicolons_and_tries_each_extension() {
        let e = env(&[("Path", r#"C:\Program Files\Git\cmd;"C:\Program Files\nodejs\";"#), ("PathExt", ".EXE;.CMD")]);
        let git = Path::new(r"C:\Program Files\Git\cmd");
        let node = Path::new(r"C:\Program Files\nodejs\");
        assert_eq!(
            WINDOWS.candidates(&e, "git"),
            vec![git.join("git.exe"), git.join("git.cmd"), node.join("git.exe"), node.join("git.cmd")],
            "the directory is split whole — `C:` is not a directory of its own"
        );
    }

    #[test]
    fn windows_tries_a_name_with_an_extension_as_written_first() {
        let e = env(&[("PATH", r"C:\composer"), ("PATHEXT", ".BAT")]);
        let dir = Path::new(r"C:\composer");
        assert_eq!(WINDOWS.candidates(&e, "composer.bat"), vec![dir.join("composer.bat"), dir.join("composer.bat.bat")]);
    }

    #[test]
    fn windows_without_pathext_uses_cmds_own_list() {
        let dir = Path::new(r"C:\bin");
        let expected = vec![dir.join("git.com"), dir.join("git.exe"), dir.join("git.bat"), dir.join("git.cmd")];
        assert_eq!(WINDOWS.candidates(&env(&[("PATH", r"C:\bin")]), "git"), expected);
        assert_eq!(WINDOWS.candidates(&env(&[("PATH", r"C:\bin"), ("PATHEXT", " ")]), "git"), expected);
    }

    #[test]
    fn this_build_uses_its_own_rules() {
        let expected = if cfg!(target_os = "windows") { &WINDOWS } else { &UNIX };
        assert_eq!(current(), expected);
    }
}
