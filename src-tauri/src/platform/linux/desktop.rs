//! The Linux desktop's own things, as pure text: the autostart `.desktop` entry, and the
//! catalog of editors, browsers and terminals rexenv knows how to find and start there
//! (docs/PLAN-linux-port.md L1). The PATH lookups and the spawns live in `mod.rs`.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::path::{Path, PathBuf};

/// `Exec=` quoting per the Desktop Entry spec: the argument in double quotes, with `"`, `` ` ``,
/// `$` and `\` escaped by a backslash. An install path with spaces stays one argument.
pub(crate) fn exec_quote(path: &Path) -> String {
    let s = path.display().to_string();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The autostart entry: `program --hidden` at sign-in. `--hidden` is the whole point — rexenv
/// starts its services and lives in the tray; a window thrown at the user on every login is an
/// app that must be closed before work can start (the macOS plist's reasoning, kept).
pub(crate) fn autostart_contents(program: &Path, hidden_flag: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=rexenv\n\
         Comment=Managed by rexenv — starts rexenv when you sign in. Do not edit.\n\
         Exec={} {hidden_flag}\n\
         Terminal=false\n\
         NoDisplay=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_quote(program)
    )
}

/// The program an autostart entry names: the first (quoted) `Exec=` argument.
pub(crate) fn autostart_program(entry: &str) -> Option<PathBuf> {
    let line = entry.lines().find_map(|l| l.strip_prefix("Exec="))?;
    let rest = line.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return Some(PathBuf::from(out)),
            c => out.push(c),
        }
    }
    None
}

/// An app rexenv can find on a Linux desktop: a stable id, a display name, the executables it
/// may be installed as (first found wins — a snap's `chromium`, a deb's `chromium-browser`),
/// and the `.desktop` ids `xdg-settings` may report for it (browsers only).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DesktopApp {
    pub id: &'static str,
    pub name: &'static str,
    pub bins: &'static [&'static str],
    /// The `.desktop` file names `xdg-settings get default-web-browser` can answer with.
    pub desktop_ids: &'static [&'static str],
    /// The command-line flag that opens a private window, when one is known to work.
    pub private_flag: Option<&'static str>,
}

/// Editors, by rough popularity — the first detected is the default. `code`, `cursor`,
/// `phpstorm` and `webstorm` are what their debs, snaps and JetBrains Toolbox put on `PATH`.
pub(crate) const EDITORS: &[DesktopApp] = &[
    DesktopApp { id: "vscode", name: "VS Code", bins: &["code"], desktop_ids: &[], private_flag: None },
    DesktopApp { id: "cursor", name: "Cursor", bins: &["cursor"], desktop_ids: &[], private_flag: None },
    DesktopApp { id: "phpstorm", name: "PhpStorm", bins: &["phpstorm"], desktop_ids: &[], private_flag: None },
    DesktopApp { id: "windsurf", name: "Windsurf", bins: &["windsurf"], desktop_ids: &[], private_flag: None },
    DesktopApp { id: "zed", name: "Zed", bins: &["zed", "zeditor"], desktop_ids: &[], private_flag: None },
    DesktopApp { id: "sublime", name: "Sublime Text", bins: &["subl"], desktop_ids: &[], private_flag: None },
    DesktopApp { id: "webstorm", name: "WebStorm", bins: &["webstorm"], desktop_ids: &[], private_flag: None },
    DesktopApp { id: "vscodium", name: "VSCodium", bins: &["codium"], desktop_ids: &[], private_flag: None },
];

/// Browsers. The private flags are the ones the macOS catalog has seen work for the same
/// engines; Firefox's snap on Ubuntu is still `firefox` on `PATH`, with the desktop id
/// `firefox_firefox.desktop`.
pub(crate) const BROWSERS: &[DesktopApp] = &[
    DesktopApp {
        id: "chrome",
        name: "Google Chrome",
        bins: &["google-chrome", "google-chrome-stable"],
        desktop_ids: &["google-chrome.desktop"],
        private_flag: Some("--incognito"),
    },
    DesktopApp {
        id: "firefox",
        name: "Firefox",
        bins: &["firefox"],
        desktop_ids: &["firefox.desktop", "firefox_firefox.desktop", "org.mozilla.firefox.desktop"],
        private_flag: Some("-private-window"),
    },
    DesktopApp {
        id: "chromium",
        name: "Chromium",
        bins: &["chromium", "chromium-browser"],
        desktop_ids: &["chromium.desktop", "chromium-browser.desktop", "chromium_chromium.desktop"],
        private_flag: Some("--incognito"),
    },
    DesktopApp {
        id: "brave",
        name: "Brave",
        bins: &["brave-browser", "brave"],
        desktop_ids: &["brave-browser.desktop", "brave_brave.desktop"],
        private_flag: Some("--incognito"),
    },
    DesktopApp {
        id: "edge",
        name: "Microsoft Edge",
        bins: &["microsoft-edge", "microsoft-edge-stable"],
        desktop_ids: &["microsoft-edge.desktop"],
        private_flag: Some("--inprivate"),
    },
    DesktopApp {
        id: "vivaldi",
        name: "Vivaldi",
        bins: &["vivaldi", "vivaldi-stable"],
        desktop_ids: &["vivaldi-stable.desktop", "vivaldi.desktop"],
        private_flag: Some("--incognito"),
    },
    DesktopApp {
        id: "opera",
        name: "Opera",
        bins: &["opera"],
        desktop_ids: &["opera.desktop", "opera_opera.desktop"],
        private_flag: Some("--private"),
    },
];

/// How a terminal is told its working directory.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CwdArg {
    /// One `--flag=<dir>` token.
    FlagEq(&'static str),
    /// Fixed tokens, then the directory as its own token.
    Args(&'static [&'static str]),
}

pub(crate) struct TerminalApp {
    pub id: &'static str,
    pub name: &'static str,
    pub bins: &'static [&'static str],
    pub cwd: CwdArg,
}

/// Terminals, by rough popularity. GNOME Terminal first because Ubuntu ships it.
pub(crate) const TERMINALS: &[TerminalApp] = &[
    TerminalApp { id: "gnome-terminal", name: "Terminal", bins: &["gnome-terminal"], cwd: CwdArg::FlagEq("--working-directory") },
    TerminalApp { id: "konsole", name: "Konsole", bins: &["konsole"], cwd: CwdArg::Args(&["--workdir"]) },
    TerminalApp { id: "ghostty", name: "Ghostty", bins: &["ghostty"], cwd: CwdArg::FlagEq("--working-directory") },
    TerminalApp { id: "kitty", name: "kitty", bins: &["kitty"], cwd: CwdArg::Args(&["--directory"]) },
    TerminalApp { id: "alacritty", name: "Alacritty", bins: &["alacritty"], cwd: CwdArg::Args(&["--working-directory"]) },
    TerminalApp { id: "wezterm", name: "WezTerm", bins: &["wezterm"], cwd: CwdArg::Args(&["start", "--cwd"]) },
    TerminalApp { id: "tilix", name: "Tilix", bins: &["tilix"], cwd: CwdArg::FlagEq("--working-directory") },
    TerminalApp { id: "xfce4-terminal", name: "Xfce Terminal", bins: &["xfce4-terminal"], cwd: CwdArg::FlagEq("--working-directory") },
];

/// The argv (after the program) that opens a terminal at `dir`.
pub(crate) fn terminal_args(cwd: CwdArg, dir: &str) -> Vec<String> {
    match cwd {
        CwdArg::FlagEq(flag) => vec![format!("{flag}={dir}")],
        CwdArg::Args(fixed) => fixed.iter().map(|s| s.to_string()).chain(std::iter::once(dir.to_string())).collect(),
    }
}

/// Which catalog browser `xdg-settings get default-web-browser` named, by its `.desktop` id.
pub(crate) fn browser_for_desktop_id(id: &str) -> Option<&'static DesktopApp> {
    let id = id.trim();
    BROWSERS.iter().find(|b| b.desktop_ids.iter().any(|d| d.eq_ignore_ascii_case(id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_autostart_entry_quotes_the_program_and_asks_for_a_hidden_launch() {
        let e = autostart_contents(Path::new("/home/u/Apps/rexenv 0.9.AppImage"), "--hidden");
        assert!(e.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(e.contains("Exec=\"/home/u/Apps/rexenv 0.9.AppImage\" --hidden\n"), "{e}");
        assert!(e.contains("X-GNOME-Autostart-enabled=true\n"));
        assert_eq!(autostart_program(&e), Some(PathBuf::from("/home/u/Apps/rexenv 0.9.AppImage")));
        assert_eq!(autostart_program("[Desktop Entry]\n"), None);
        assert_eq!(exec_quote(Path::new("/a/$b")), "\"/a/\\$b\"");
    }

    #[test]
    fn terminal_arguments_take_the_shape_each_terminal_documents() {
        assert_eq!(terminal_args(CwdArg::FlagEq("--working-directory"), "/x y"), vec!["--working-directory=/x y"]);
        assert_eq!(terminal_args(CwdArg::Args(&["start", "--cwd"]), "/x"), vec!["start", "--cwd", "/x"]);
    }

    #[test]
    fn the_snap_firefox_desktop_id_is_recognised() {
        assert_eq!(browser_for_desktop_id("firefox_firefox.desktop\n").map(|b| b.id), Some("firefox"));
        assert_eq!(browser_for_desktop_id("google-chrome.desktop").map(|b| b.id), Some("chrome"));
        assert!(browser_for_desktop_id("org.gnome.Epiphany.desktop").is_none());
    }

    #[test]
    fn every_catalog_id_is_unique() {
        let mut ids: Vec<&str> = EDITORS.iter().chain(BROWSERS).map(|a| a.id).chain(TERMINALS.iter().map(|t| t.id)).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
