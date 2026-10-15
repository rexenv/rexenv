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
    /// The `.desktop` file names the app installs as (deb, snap `<pkg>_<app>`, flatpak reverse-DNS).
    /// For browsers also what `xdg-settings get default-web-browser` can answer with; for every app
    /// the file whose `Icon=` names its icon ([`icon_name_in`]).
    pub desktop_ids: &'static [&'static str],
    /// The command-line flag that opens a private window, when one is known to work.
    pub private_flag: Option<&'static str>,
}

/// Editors, by rough popularity — the first detected is the default. `code`, `cursor`,
/// `phpstorm` and `webstorm` are what their debs, snaps and JetBrains Toolbox put on `PATH`.
pub(crate) const EDITORS: &[DesktopApp] = &[
    DesktopApp { id: "vscode", name: "VS Code", bins: &["code"], desktop_ids: &["code.desktop", "code_code.desktop", "com.visualstudio.code.desktop"], private_flag: None },
    DesktopApp { id: "cursor", name: "Cursor", bins: &["cursor"], desktop_ids: &["cursor.desktop"], private_flag: None },
    DesktopApp { id: "phpstorm", name: "PhpStorm", bins: &["phpstorm"], desktop_ids: &["jetbrains-phpstorm.desktop", "phpstorm.desktop", "phpstorm_phpstorm.desktop"], private_flag: None },
    DesktopApp { id: "windsurf", name: "Windsurf", bins: &["windsurf"], desktop_ids: &["windsurf.desktop"], private_flag: None },
    DesktopApp { id: "zed", name: "Zed", bins: &["zed", "zeditor"], desktop_ids: &["dev.zed.Zed.desktop", "zed.desktop"], private_flag: None },
    DesktopApp { id: "sublime", name: "Sublime Text", bins: &["subl"], desktop_ids: &["sublime_text.desktop", "sublime-text_subl.desktop"], private_flag: None },
    DesktopApp { id: "webstorm", name: "WebStorm", bins: &["webstorm"], desktop_ids: &["jetbrains-webstorm.desktop", "webstorm.desktop", "webstorm_webstorm.desktop"], private_flag: None },
    DesktopApp { id: "vscodium", name: "VSCodium", bins: &["codium"], desktop_ids: &["codium.desktop", "com.vscodium.codium.desktop", "codium_codium.desktop"], private_flag: None },
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
    /// The `.desktop` file names, for the icon (see [`DesktopApp::desktop_ids`]).
    pub desktop_ids: &'static [&'static str],
}

/// Terminals, by rough popularity. GNOME Terminal first because Ubuntu ships it.
pub(crate) const TERMINALS: &[TerminalApp] = &[
    TerminalApp { id: "gnome-terminal", name: "Terminal", bins: &["gnome-terminal"], cwd: CwdArg::FlagEq("--working-directory"), desktop_ids: &["org.gnome.Terminal.desktop"] },
    TerminalApp { id: "konsole", name: "Konsole", bins: &["konsole"], cwd: CwdArg::Args(&["--workdir"]), desktop_ids: &["org.kde.konsole.desktop"] },
    TerminalApp { id: "ghostty", name: "Ghostty", bins: &["ghostty"], cwd: CwdArg::FlagEq("--working-directory"), desktop_ids: &["com.mitchellh.ghostty.desktop"] },
    TerminalApp { id: "kitty", name: "kitty", bins: &["kitty"], cwd: CwdArg::Args(&["--directory"]), desktop_ids: &["kitty.desktop"] },
    TerminalApp { id: "alacritty", name: "Alacritty", bins: &["alacritty"], cwd: CwdArg::Args(&["--working-directory"]), desktop_ids: &["Alacritty.desktop"] },
    TerminalApp { id: "wezterm", name: "WezTerm", bins: &["wezterm"], cwd: CwdArg::Args(&["start", "--cwd"]), desktop_ids: &["org.wezfurlong.wezterm.desktop"] },
    TerminalApp { id: "tilix", name: "Tilix", bins: &["tilix"], cwd: CwdArg::FlagEq("--working-directory"), desktop_ids: &["com.gexperts.Tilix.desktop"] },
    TerminalApp { id: "xfce4-terminal", name: "Xfce Terminal", bins: &["xfce4-terminal"], cwd: CwdArg::FlagEq("--working-directory"), desktop_ids: &["xfce4-terminal.desktop"] },
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

/// Where `.desktop` files live, most specific first: the user's, local, the distro's, snapd's
/// exports, flatpak's (system and user).
pub(crate) fn desktop_file_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/var/lib/snapd/desktop/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        home.join(".local/share/flatpak/exports/share/applications"),
    ]
}

/// The `Icon=` of a `.desktop` file's `[Desktop Entry]` group: a theme icon NAME
/// (`firefox`) or, as snapd writes it, an absolute PATH (`/snap/firefox/current/default256.png`).
pub(crate) fn icon_name_in(desktop_file: &str) -> Option<String> {
    let mut in_entry = false;
    for line in desktop_file.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry {
            if let Some(v) = line.strip_prefix("Icon=") {
                let v = v.trim();
                return (!v.is_empty()).then(|| v.to_string());
            }
        }
    }
    None
}

/// The sizes a picker asks the hicolor theme for, best first: 64 px is crisp at 16 px on a
/// 2× display and small on the wire; `scalable` holds SVGs, which the webview draws itself.
const THEME_SIZES: [&str; 8] = ["64x64", "48x48", "128x128", "96x96", "256x256", "32x32", "512x512", "scalable"];

/// Every file the icon named by `Icon=` may be, in the order to try: an absolute path is the
/// file itself; a theme name is looked up under the user's, local and system icon dirs —
/// `hicolor` (every app installs there, the spec's fallback theme) — then `/usr/share/pixmaps`.
/// Only `.png` and `.svg`: a webview `<img>` renders both; `.xpm` it does not.
pub(crate) fn icon_candidates(icon: &str, home: &Path) -> Vec<PathBuf> {
    if icon.starts_with('/') {
        return vec![PathBuf::from(icon)];
    }
    // A name written WITH an extension (`code.png`) is looked up as its stem.
    let stem = icon.strip_suffix(".png").or_else(|| icon.strip_suffix(".svg")).unwrap_or(icon);
    let mut out = Vec::new();
    for root in [home.join(".local/share/icons"), PathBuf::from("/usr/local/share/icons"), PathBuf::from("/usr/share/icons")] {
        for size in THEME_SIZES {
            let dir = root.join("hicolor").join(size).join("apps");
            out.push(dir.join(format!("{stem}.png")));
            out.push(dir.join(format!("{stem}.svg")));
        }
    }
    out.push(PathBuf::from(format!("/usr/share/pixmaps/{stem}.png")));
    out.push(PathBuf::from(format!("/usr/share/pixmaps/{stem}.svg")));
    out
}

/// The `data:` media type for an icon file, by extension; `None` for one a webview cannot draw.
pub(crate) fn icon_media_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
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
    fn the_desktop_entry_icon_is_read_from_the_entry_group_only() {
        let snap = "[Desktop Entry]\nName=Firefox\nIcon=/snap/firefox/current/default256.png\nExec=firefox %u\n";
        assert_eq!(icon_name_in(snap).as_deref(), Some("/snap/firefox/current/default256.png"));
        let deb = "[Desktop Entry]\nName=Code\nIcon=vscode\n[Desktop Action new-empty-window]\nIcon=other\n";
        assert_eq!(icon_name_in(deb).as_deref(), Some("vscode"));
        let only_action = "[Desktop Action x]\nIcon=other\n";
        assert_eq!(icon_name_in(only_action), None, "an action's icon is not the app's");
        assert_eq!(icon_name_in("[Desktop Entry]\nIcon=\n"), None);
    }

    #[test]
    fn an_absolute_icon_is_the_file_and_a_name_walks_hicolor_then_pixmaps_png_before_svg() {
        let home = Path::new("/home/u");
        assert_eq!(icon_candidates("/snap/chromium/current/chromium.png", home), vec![PathBuf::from("/snap/chromium/current/chromium.png")]);
        let c = icon_candidates("firefox", home);
        assert_eq!(c[0], PathBuf::from("/home/u/.local/share/icons/hicolor/64x64/apps/firefox.png"), "the user's theme dir first");
        assert_eq!(c[1], PathBuf::from("/home/u/.local/share/icons/hicolor/64x64/apps/firefox.svg"));
        let sys64 = PathBuf::from("/usr/share/icons/hicolor/64x64/apps/firefox.png");
        let sys128 = PathBuf::from("/usr/share/icons/hicolor/128x128/apps/firefox.png");
        let scalable = PathBuf::from("/usr/share/icons/hicolor/scalable/apps/firefox.svg");
        let pixmap = PathBuf::from("/usr/share/pixmaps/firefox.png");
        let at = |p: &PathBuf| c.iter().position(|x| x == p).unwrap_or_else(|| panic!("{} missing", p.display()));
        assert!(at(&sys64) < at(&sys128) && at(&sys128) < at(&scalable) && at(&scalable) < at(&pixmap));
        assert_eq!(c.last(), Some(&PathBuf::from("/usr/share/pixmaps/firefox.svg")));
        // A name carrying its extension is looked up by stem, never as `code.png.png`.
        assert!(icon_candidates("code.png", home).iter().all(|p| !p.to_string_lossy().contains(".png.png")));
        assert_eq!(icon_media_type(Path::new("/x/a.PNG")), Some("image/png"));
        assert_eq!(icon_media_type(Path::new("/x/a.svg")), Some("image/svg+xml"));
        assert_eq!(icon_media_type(Path::new("/x/a.xpm")), None);
    }

    #[test]
    fn every_app_has_a_desktop_id_to_find_its_icon_by() {
        for a in EDITORS.iter().chain(BROWSERS) {
            assert!(!a.desktop_ids.is_empty(), "{} has no .desktop id", a.id);
            assert!(a.desktop_ids.iter().all(|d| d.ends_with(".desktop")), "{}", a.id);
        }
        for t in TERMINALS {
            assert!(!t.desktop_ids.is_empty(), "{} has no .desktop id", t.id);
        }
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
