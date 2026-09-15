//! The pure half of Windows' editors, browsers and terminals (W7 S3, plan §5 W7, ledger #622): which apps
//! rexenv knows, how each is found in what the registry says, which browser is the default, and the exact
//! process each open starts. No Win32 here — `WindowsShell` reads the registry into [`Installed`] — so this
//! is compiled into the macOS test build.
//!
//! Detection reads what the installers REGISTER (App Paths, the uninstall entries, `StartMenuInternet`) and
//! only then a known install folder, and every candidate must exist on disk — never a guessed path alone.
//! The ids are macOS's (`vscode`, `chrome`, …), so a `preferred_editor` or `preferred_browser` setting names
//! the same app on both. Opening starts the detected executable with its arguments — no shell in between,
//! so nothing in a path or a URL is ever parsed as a command. Icons are `None` for now: the UI draws its
//! glyph, which is the trait's honest fallback.
//!
//! Shapes from the Dell's inventory, 15 Sep 2026: App Paths `chrome.exe`, `firefox.exe`, `msedge.exe`,
//! `brave.exe`; uninstall entries "Microsoft Visual Studio Code (User)" (`DisplayIcon` …`\Code.exe`), "PhpStorm
//! 2025.1.1" (`DisplayIcon` …`\bin\phpstorm64.exe`), "Git" (`InstallLocation` `C:\Program Files\Git\`); the
//! `https` ProgId `ChromeHTML`; no Windows Terminal, no PowerShell 7.

use std::path::{Path, PathBuf};

/// One uninstall entry.
#[derive(Debug, Clone, Default)]
pub(crate) struct Program {
    pub name: String,
    pub location: String,
    pub icon: String,
}

/// What the Win32 half read, once per detection.
#[derive(Debug, Clone, Default)]
pub(crate) struct Installed {
    /// App Paths: an executable's file name, lower-cased → its registered full path.
    pub app_paths: Vec<(String, String)>,
    /// Uninstall entries (HKCU and both HKLM views).
    pub programs: Vec<Program>,
    /// `StartMenuInternet` clients' `shell\open\command`.
    pub browser_commands: Vec<String>,
    /// The `https` handler's `UserChoice` ProgId.
    pub https_prog_id: Option<String>,
    /// `%LOCALAPPDATA%`, `%ProgramFiles%`, `%ProgramFiles(x86)%`, `%SystemRoot%`.
    pub local_app_data: Option<String>,
    pub program_files: Vec<String>,
    pub system_root: Option<String>,
}

/// How to find one app.
struct Find {
    /// The executable's file name.
    exe: &'static str,
    /// Uninstall display-name prefixes, and the executable relative to `InstallLocation`.
    program: &'static [&'static str],
    program_exe: &'static str,
    /// Relative to `%LOCALAPPDATA%`, then relative to each Program Files folder.
    local: &'static [&'static str],
    program_files: &'static [&'static str],
}

const EDITORS: &[(&str, &str, Find)] = &[
    ("vscode", "Visual Studio Code", Find { exe: "Code.exe", program: &["Microsoft Visual Studio Code"], program_exe: "Code.exe", local: &[r"Programs\Microsoft VS Code\Code.exe"], program_files: &[r"Microsoft VS Code\Code.exe"] }),
    ("cursor", "Cursor", Find { exe: "Cursor.exe", program: &["Cursor"], program_exe: "Cursor.exe", local: &[r"Programs\cursor\Cursor.exe"], program_files: &[] }),
    ("phpstorm", "PhpStorm", Find { exe: "phpstorm64.exe", program: &["PhpStorm"], program_exe: r"bin\phpstorm64.exe", local: &[], program_files: &[] }),
    ("windsurf", "Windsurf", Find { exe: "Windsurf.exe", program: &["Windsurf"], program_exe: "Windsurf.exe", local: &[r"Programs\Windsurf\Windsurf.exe"], program_files: &[] }),
    ("zed", "Zed", Find { exe: "zed.exe", program: &["Zed"], program_exe: "zed.exe", local: &[r"Programs\Zed\zed.exe"], program_files: &[] }),
    ("sublime", "Sublime Text", Find { exe: "sublime_text.exe", program: &["Sublime Text"], program_exe: "sublime_text.exe", local: &[], program_files: &[r"Sublime Text\sublime_text.exe"] }),
    ("webstorm", "WebStorm", Find { exe: "webstorm64.exe", program: &["WebStorm"], program_exe: r"bin\webstorm64.exe", local: &[], program_files: &[] }),
    ("vscodium", "VSCodium", Find { exe: "VSCodium.exe", program: &["VSCodium"], program_exe: "VSCodium.exe", local: &[r"Programs\VSCodium\VSCodium.exe"], program_files: &[r"VSCodium\VSCodium.exe"] }),
];

/// One browser rexenv knows.
struct Browser {
    id: &'static str,
    name: &'static str,
    find: Find,
    /// The private-window flag, when the browser has a command line for one.
    private: Option<&'static str>,
    /// `https` ProgId prefixes that make it the default.
    prog_ids: &'static [&'static str],
}

const BROWSERS: &[Browser] = &[
    Browser { id: "chrome", name: "Google Chrome", find: Find { exe: "chrome.exe", program: &[], program_exe: "", local: &[r"Google\Chrome\Application\chrome.exe"], program_files: &[r"Google\Chrome\Application\chrome.exe"] }, private: Some("--incognito"), prog_ids: &["ChromeHTML"] },
    Browser { id: "firefox", name: "Firefox", find: Find { exe: "firefox.exe", program: &[], program_exe: "", local: &[], program_files: &[r"Mozilla Firefox\firefox.exe"] }, private: Some("-private-window"), prog_ids: &["FirefoxURL", "FirefoxHTML"] },
    Browser { id: "brave", name: "Brave", find: Find { exe: "brave.exe", program: &[], program_exe: "", local: &[r"BraveSoftware\Brave-Browser\Application\brave.exe"], program_files: &[r"BraveSoftware\Brave-Browser\Application\brave.exe"] }, private: Some("--incognito"), prog_ids: &["BraveHTML"] },
    Browser { id: "edge", name: "Microsoft Edge", find: Find { exe: "msedge.exe", program: &[], program_exe: "", local: &[], program_files: &[r"Microsoft\Edge\Application\msedge.exe"] }, private: Some("--inprivate"), prog_ids: &["MSEdgeHTM"] },
    Browser { id: "opera", name: "Opera", find: Find { exe: "opera.exe", program: &[], program_exe: "", local: &[r"Programs\Opera\opera.exe"], program_files: &[] }, private: Some("--private"), prog_ids: &["OperaStable"] },
    Browser { id: "vivaldi", name: "Vivaldi", find: Find { exe: "vivaldi.exe", program: &[], program_exe: "", local: &[r"Vivaldi\Application\vivaldi.exe"], program_files: &[r"Vivaldi\Application\vivaldi.exe"] }, private: Some("--incognito"), prog_ids: &["VivaldiHTM"] },
    Browser { id: "chromium", name: "Chromium", find: Find { exe: "", program: &[], program_exe: "", local: &[r"Chromium\Application\chrome.exe"], program_files: &[] }, private: Some("--incognito"), prog_ids: &["ChromiumHTM"] },
    Browser { id: "librewolf", name: "LibreWolf", find: Find { exe: "librewolf.exe", program: &[], program_exe: "", local: &[], program_files: &[r"LibreWolf\librewolf.exe"] }, private: Some("-private-window"), prog_ids: &["LibreWolfHTM"] },
    Browser { id: "zen", name: "Zen Browser", find: Find { exe: "zen.exe", program: &[], program_exe: "", local: &[], program_files: &[r"Zen Browser\zen.exe"] }, private: Some("-private-window"), prog_ids: &["ZenHTML"] },
];

/// How a terminal opens at a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TermStart {
    /// `wt.exe -d <dir>`.
    WindowsTerminal,
    /// `pwsh.exe -NoExit`, a new console, started IN the folder (no path on its command line).
    Pwsh,
    /// The shell with no arguments but its keep-open flag, a new console, started IN the folder.
    InFolder(&'static [&'static str]),
    /// `git-bash.exe --cd=<dir>`.
    GitBash,
}

const TERMINALS: &[(&str, &str, Find, TermStart)] = &[
    ("windows-terminal", "Windows Terminal", Find { exe: "wt.exe", program: &[], program_exe: "", local: &[r"Microsoft\WindowsApps\wt.exe"], program_files: &[] }, TermStart::WindowsTerminal),
    ("pwsh", "PowerShell 7", Find { exe: "pwsh.exe", program: &[], program_exe: "", local: &[], program_files: &[r"PowerShell\7\pwsh.exe"] }, TermStart::Pwsh),
    ("git-bash", "Git Bash", Find { exe: "", program: &["Git"], program_exe: "git-bash.exe", local: &[], program_files: &[r"Git\git-bash.exe"] }, TermStart::GitBash),
    ("powershell", "Windows PowerShell", Find { exe: "", program: &[], program_exe: "", local: &[], program_files: &[] }, TermStart::InFolder(&["-NoExit"])),
    ("cmd", "Command Prompt", Find { exe: "", program: &[], program_exe: "", local: &[], program_files: &[] }, TermStart::InFolder(&["/K"])),
];

/// The executable inside a registry string: the quoted or leading path, an icon index (`,0`) and any
/// arguments dropped.
pub(crate) fn exe_in(command: &str) -> Option<String> {
    let s = command.trim();
    let path = if let Some(rest) = s.strip_prefix('"') {
        rest.split('"').next()?.to_string()
    } else {
        let upto = s.to_ascii_lowercase().find(".exe").map(|i| i + 4)?;
        s[..upto].to_string()
    };
    let path = path.split(',').next()?.trim().to_string();
    path.to_ascii_lowercase().ends_with(".exe").then_some(path)
}

fn file_name_is(path: &str, exe: &str) -> bool {
    !exe.is_empty() && path.rsplit(['\\', '/']).next().is_some_and(|n| n.eq_ignore_ascii_case(exe))
}

fn join(dir: &str, rel: &str) -> String {
    format!("{}\\{}", dir.trim_end_matches(['\\', '/']), rel)
}

/// Every place `find` says the app could be, registered places first.
fn candidates(find: &Find, installed: &Installed, extra: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if !find.exe.is_empty() {
        let exe = find.exe.to_ascii_lowercase();
        out.extend(installed.app_paths.iter().filter(|(name, _)| *name == exe).filter_map(|(_, p)| exe_in(p).or(Some(p.clone()))));
        out.extend(installed.browser_commands.iter().filter_map(|c| exe_in(c)).filter(|p| file_name_is(p, find.exe)));
    }
    for program in installed.programs.iter().filter(|p| find.program.iter().any(|prefix| p.name.starts_with(prefix))) {
        if let Some(icon) = exe_in(&program.icon).filter(|p| file_name_is(p, find.program_exe.rsplit('\\').next().unwrap_or(""))) {
            out.push(icon);
        }
        if !program.location.trim().is_empty() && !find.program_exe.is_empty() {
            out.push(join(&program.location, find.program_exe));
        }
    }
    if let Some(local) = &installed.local_app_data {
        out.extend(find.local.iter().map(|rel| join(local, rel)));
    }
    for pf in &installed.program_files {
        out.extend(find.program_files.iter().map(|rel| join(pf, rel)));
    }
    out.extend(extra.iter().cloned());
    out
}

/// The first candidate that exists.
fn locate(find: &Find, installed: &Installed, extra: &[String], exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    candidates(find, installed, extra).into_iter().map(PathBuf::from).find(|p| exists(p))
}

/// A detected app: its id, name and the executable that opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub id: &'static str,
    pub name: &'static str,
    pub exe: PathBuf,
}

pub(crate) fn editors(installed: &Installed, exists: &dyn Fn(&Path) -> bool) -> Vec<Found> {
    EDITORS
        .iter()
        .filter_map(|(id, name, find)| locate(find, installed, &[], exists).map(|exe| Found { id, name, exe }))
        .collect()
}

/// Detected browsers, each with whether it is the `https` default and whether it has a private flag.
pub(crate) fn browsers(installed: &Installed, exists: &dyn Fn(&Path) -> bool) -> Vec<(Found, bool, bool)> {
    let default = installed.https_prog_id.as_deref().unwrap_or("");
    BROWSERS
        .iter()
        .filter_map(|b| {
            let exe = locate(&b.find, installed, &[], exists)?;
            // Chromium installs a `chrome.exe` too; Chrome must not claim it.
            if b.id == "chrome" && exe.to_string_lossy().to_ascii_lowercase().contains(r"\chromium\") {
                return None;
            }
            let is_default = !default.is_empty() && b.prog_ids.iter().any(|p| default.starts_with(p));
            Some((Found { id: b.id, name: b.name, exe }, is_default, b.private.is_some()))
        })
        .collect()
}

pub(crate) fn terminals(installed: &Installed, exists: &dyn Fn(&Path) -> bool) -> Vec<Found> {
    let system = installed.system_root.clone().unwrap_or_else(|| r"C:\Windows".into());
    TERMINALS
        .iter()
        .filter_map(|(id, name, find, _)| {
            let extra = match *id {
                "powershell" => vec![join(&system, r"System32\WindowsPowerShell\v1.0\powershell.exe")],
                "cmd" => vec![join(&system, r"System32\cmd.exe")],
                _ => Vec::new(),
            };
            locate(find, installed, &extra, exists).map(|exe| Found { id, name, exe })
        })
        .collect()
}

/// The process one open starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Launch {
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub current_dir: Option<PathBuf>,
    pub new_console: bool,
}

/// An editor opening `dir` as a project.
pub(crate) fn editor_launch(found: &Found, dir: &Path) -> Launch {
    Launch { exe: found.exe.clone(), args: vec![dir.display().to_string()], current_dir: None, new_console: false }
}

/// The ONE scheme check for a chosen browser (the trait's rule: both modes, one place): `http://` and
/// `https://` only. `WindowsShell::open_in_browser` calls it before looking the browser up, so the refusal
/// never depends on what is installed; `browser_launch` calls it again, so no launch skips it.
pub(crate) fn web_url_only(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Ok(())
    } else {
        Err(format!("refusing to open {url} in a browser — only http:// and https:// URLs go to a chosen browser"))
    }
}

/// A browser opening `url` — `http(s)` only, checked before anything else (the trait's rule); `private`
/// errors for a browser with no private flag rather than opening a recorded window.
pub(crate) fn browser_launch(found: &Found, url: &str, private: bool) -> Result<Launch, String> {
    web_url_only(url)?;
    let flag = BROWSERS.iter().find(|b| b.id == found.id).and_then(|b| b.private);
    let mut args = Vec::new();
    if private {
        let flag = flag.ok_or_else(|| format!("{} has no private-window command line", found.name))?;
        args.push(flag.to_string());
    }
    args.push(url.to_string());
    Ok(Launch { exe: found.exe.clone(), args, current_dir: None, new_console: false })
}

/// A terminal opening at `dir` (the caller has checked it is an existing directory).
pub(crate) fn terminal_launch(found: &Found, dir: &Path) -> Launch {
    let start = TERMINALS.iter().find(|t| t.0 == found.id).map(|t| t.3).unwrap_or(TermStart::InFolder(&[]));
    let d = dir.display().to_string();
    let (args, current_dir, new_console) = match start {
        TermStart::WindowsTerminal => (vec!["-d".into(), d], None, false),
        TermStart::Pwsh => (vec!["-NoExit".into()], Some(dir.to_path_buf()), true),
        TermStart::GitBash => (vec![format!("--cd={d}")], None, false),
        TermStart::InFolder(flags) => (flags.iter().map(|f| f.to_string()).collect(), Some(dir.to_path_buf()), true),
    };
    Launch { exe: found.exe.clone(), args, current_dir, new_console }
}

/// The command line of a new-console launch: the quoted executable and its fixed flags, or `None` when an
/// argument is not a plain flag (`-Word` or `/Word`). A console launch is started without `std::process`'s
/// escaping (`WindowsShell`'s `start_in_new_console`), so it must never carry a path or a URL — the folder
/// goes in as the working directory instead. A Windows path cannot contain `"`, so quoting the executable
/// is enough.
pub(crate) fn console_command_line(launch: &Launch) -> Option<String> {
    let flag = |a: &String| {
        let mut chars = a.chars();
        matches!(chars.next(), Some('-' | '/')) && a.len() > 1 && chars.all(|c| c.is_ascii_alphabetic())
    };
    if !launch.args.iter().all(flag) {
        return None;
    }
    let mut line = format!("\"{}\"", launch.exe.display());
    for arg in &launch.args {
        line.push(' ');
        line.push_str(arg);
    }
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Dell's registry, 15 Sep 2026 (plan §5 W7 inventory).
    fn dell() -> Installed {
        Installed {
            app_paths: vec![
                ("brave.exe".into(), r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe".into()),
                ("chrome.exe".into(), r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".into()),
                ("firefox.exe".into(), r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe".into()),
                ("msedge.exe".into(), r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".into()),
                ("opera.exe".into(), String::new()),
            ],
            programs: vec![
                Program { name: "Git".into(), location: r"C:\Program Files\Git\".into(), icon: r"C:\Program Files\Git\mingw64\share\git\git-for-windows.ico".into() },
                Program { name: "PhpStorm 2025.1.1".into(), location: r"C:\Program Files\JetBrains\PhpStorm 2025.1.1".into(), icon: r"C:\Program Files\JetBrains\PhpStorm 2025.1.1\bin\phpstorm64.exe".into() },
                Program { name: "Microsoft Visual Studio Code (User)".into(), location: r"C:\Users\DELL\AppData\Local\Programs\Microsoft VS Code\".into(), icon: r"C:\Users\DELL\AppData\Local\Programs\Microsoft VS Code\Code.exe".into() },
            ],
            browser_commands: vec![
                r#""C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe""#.into(),
                r#""C:\Program Files (x86)\Mozilla Firefox\firefox.exe""#.into(),
                r#""C:\Program Files (x86)\Google\Chrome\Application\chrome.exe""#.into(),
                r"C:\Program Files\Internet Explorer\iexplore.exe".into(),
                r"C:\Program Files (x86)\Maxthon\Bin\Maxthon.exe".into(),
                r#""C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe""#.into(),
            ],
            https_prog_id: Some("ChromeHTML".into()),
            local_app_data: Some(r"C:\Users\DELL\AppData\Local".into()),
            program_files: vec![r"C:\Program Files".into(), r"C:\Program Files (x86)".into()],
            system_root: Some(r"C:\Windows".into()),
        }
    }

    /// What exists on the Dell's disk for these checks.
    fn on_dell(p: &Path) -> bool {
        let s = p.to_string_lossy().to_ascii_lowercase();
        [
            r"c:\program files\braveSoftware\brave-browser\application\brave.exe",
            r"c:\program files (x86)\google\chrome\application\chrome.exe",
            r"c:\program files (x86)\mozilla firefox\firefox.exe",
            r"c:\program files (x86)\microsoft\edge\application\msedge.exe",
            r"c:\program files\jetbrains\phpstorm 2025.1.1\bin\phpstorm64.exe",
            r"c:\users\dell\appdata\local\programs\microsoft vs code\code.exe",
            r"c:\program files\git\git-bash.exe",
            r"c:\windows\system32\windowspowershell\v1.0\powershell.exe",
            r"c:\windows\system32\cmd.exe",
        ]
        .iter()
        .any(|e| s == e.to_ascii_lowercase())
    }

    fn ids(found: &[Found]) -> Vec<&str> {
        found.iter().map(|f| f.id).collect()
    }

    /// Ledger #622 — what the Dell registers and has on disk is what is detected; nothing is guessed.
    #[test]
    fn the_dell_detects_what_it_registers_and_has_on_disk() {
        let installed = dell();
        let editors = editors(&installed, &on_dell);
        assert_eq!(ids(&editors), ["vscode", "phpstorm"]);
        assert!(editors[0].exe.to_string_lossy().ends_with(r"Microsoft VS Code\Code.exe"));

        let browsers = browsers(&installed, &on_dell);
        let names: Vec<_> = browsers.iter().map(|(f, d, _)| (f.id, *d)).collect();
        assert_eq!(names, [("chrome", true), ("firefox", false), ("brave", false), ("edge", false)]);
        assert!(browsers.iter().all(|(_, _, private)| *private));

        assert_eq!(ids(&terminals(&installed, &on_dell)), ["git-bash", "powershell", "cmd"]);

        // Registered but not on disk is not installed.
        assert!(editors_none_when_missing(&installed));
    }

    fn editors_none_when_missing(installed: &Installed) -> bool {
        editors(installed, &|_| false).is_empty() && browsers(installed, &|_| false).is_empty()
    }

    #[test]
    fn a_firefox_prog_id_with_its_install_hash_is_still_firefox() {
        let mut installed = dell();
        installed.https_prog_id = Some("FirefoxURL-308046B0AF4A39CB".into());
        let defaults: Vec<_> = browsers(&installed, &on_dell).into_iter().filter(|b| b.1).map(|b| b.0.id).collect();
        assert_eq!(defaults, ["firefox"]);
    }

    #[test]
    fn the_executable_inside_a_registry_string() {
        assert_eq!(exe_in(r#""C:\Program Files\x\a.exe" --flag"#).as_deref(), Some(r"C:\Program Files\x\a.exe"));
        assert_eq!(exe_in(r"C:\Program Files\x\a.exe,0").as_deref(), Some(r"C:\Program Files\x\a.exe"));
        assert_eq!(exe_in(r"C:\Program Files\Git\mingw64\share\git\git-for-windows.ico"), None);
        assert_eq!(exe_in(""), None);
    }

    /// Ledger #622 — a browser gets only http(s), a private window only with a flag, and every open is one
    /// executable with its arguments (a path with spaces is one argument, never re-split).
    #[test]
    fn every_open_is_one_executable_with_its_arguments() {
        let installed = dell();
        let chrome = browsers(&installed, &on_dell).into_iter().find(|b| b.0.id == "chrome").unwrap().0;
        let l = browser_launch(&chrome, "https://shop.rex/", true).unwrap();
        assert_eq!(l.args, ["--incognito", "https://shop.rex/"]);
        for bad in [r"C:\x\run.bat", "file:///C:/x", "javascript:alert(1)", "-–help"] {
            assert!(browser_launch(&chrome, bad, false).is_err(), "{bad}");
        }
        let nameless = Found { id: "not-a-browser", name: "X", exe: PathBuf::from(r"C:\x.exe") };
        assert!(browser_launch(&nameless, "https://a.rex/", true).is_err());

        let dir = Path::new(r"C:\Users\A B\Sites\shop");
        let code = &editors(&installed, &on_dell)[0];
        assert_eq!(editor_launch(code, dir).args, [r"C:\Users\A B\Sites\shop"]);
        let terms = terminals(&installed, &on_dell);
        let bash = terminal_launch(&terms[0], dir);
        assert_eq!(bash.args, [r"--cd=C:\Users\A B\Sites\shop"]);
        let ps = terminal_launch(&terms[1], dir);
        assert_eq!((ps.args.as_slice(), ps.current_dir.as_deref(), ps.new_console), (&["-NoExit".to_string()][..], Some(dir), true));
    }

    /// Ledger #622 — a new-console launch carries fixed flags only, the folder as its working directory: its
    /// command line is built without escaping, so a path or a URL must never reach it.
    #[test]
    fn a_console_launch_carries_flags_only_and_the_folder_as_its_working_directory() {
        let installed = dell();
        let dir = Path::new(r"C:\Users\A B\Sites\shop\");
        let pwsh = Found { id: "pwsh", name: "PowerShell 7", exe: PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe") };
        let mut consoles = vec![terminal_launch(&pwsh, dir)];
        consoles.extend(terminals(&installed, &on_dell).iter().map(|t| terminal_launch(t, dir)).filter(|l| l.new_console));
        assert_eq!(consoles.len(), 3, "pwsh, powershell and cmd start in a new console");
        for launch in &consoles {
            assert_eq!(launch.current_dir.as_deref(), Some(dir));
            let line = console_command_line(launch).unwrap_or_else(|| panic!("{launch:?}"));
            assert!(!line.contains("Sites"), "no path on a console command line: {line}");
        }
        assert_eq!(console_command_line(&consoles[0]).as_deref(), Some(r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoExit"#));
        let with_path = Launch { exe: pwsh.exe.clone(), args: vec![r"C:\x\run.bat".into()], current_dir: None, new_console: true };
        assert_eq!(console_command_line(&with_path), None);
        let with_url = Launch { args: vec!["-NoExit".into(), "https://a.rex/".into()], ..with_path };
        assert_eq!(console_command_line(&with_url), None);
    }
}
