//! The words rexenv uses for the operating system's own things — its file manager, its login items, how a
//! missing tool gets installed (W7 S7, plan §5 W7 ruling Q3, ledger #626). Owned by the platform, as
//! `DnsManager::route_label` owns the words for a DNS route: the UI and the error messages ask for them
//! instead of writing "Finder" or `brew install` themselves, so a Windows screen never tells a user to open
//! Finder.
//!
//! Both sets are plain data compiled on every host, so their tests run everywhere; [`current`] picks this
//! build's. The macOS set is the text the app showed before this existed, byte for byte.

/// The words for one operating system's own things.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformWords {
    /// The button that shows a file in the file manager with it selected.
    pub reveal: &'static str,
    /// The file manager's name, as a sentence uses it.
    pub file_manager: &'static str,
    /// The "Open rexenv at login" toggle's description.
    pub login_item: &'static str,
    /// The fix a missing-git message gives.
    pub git_install: &'static str,
    /// The command that installs Node.js.
    pub node_install: &'static str,
    /// The command that installs Bun.
    pub bun_install: &'static str,
    /// What a native npm module (node-gyp) needs to compile, and how to get it.
    pub native_build: &'static str,
    /// The "Command-line tool" card before `rex` is installed: what Install does, and what it costs (#634).
    pub cli_install: &'static str,
    /// The card after the install path's name, when what is there is not this app's `rex`.
    pub cli_stale: &'static str,
    /// The toast after Install, before the install path in parentheses.
    pub cli_installed: &'static str,
    /// Where this OS keeps the user's own trusted certificate authorities, as a sentence uses it (W9 S3a,
    /// ledger #638).
    pub trust_store: &'static str,
    /// What a first privileged step costs the user, as a sentence uses it ("… asks for your password once").
    pub privileged_prompt: &'static str,
    /// This OS's name, for a sentence that must say it ("register it with macOS").
    pub os_name: &'static str,
    /// What a local CA is added TO, as a sentence uses it ("adds a private certificate authority to …").
    pub ca_target: &'static str,
    /// The note an agent's consent line carries when the step will also raise the OS's own prompt.
    pub elevation_note: &'static str,
    /// Whether this OS draws its window controls INSIDE the page — macOS's traffic lights over a
    /// `titleBarStyle: "Overlay"` window, which the shell reserves a row for. Windows draws its own title
    /// bar above the page, so reserving that row there leaves a dead strip (W9 S1, ruling Q1, ledger #636).
    pub window_controls_in_content: bool,
}

pub const MACOS: PlatformWords = PlatformWords {
    reveal: "Show in Finder",
    file_manager: "Finder",
    login_item: "rexenv launches when you sign in to your Mac (a macOS login item).",
    git_install: "On macOS it ships with the Xcode Command Line Tools — install them, then hit Re-detect:\n$ xcode-select --install",
    node_install: "$ brew install node",
    bun_install: "$ brew install oven-sh/bun/bun",
    native_build: "That needs the Xcode Command Line Tools — install them, then retry:\n$ xcode-select --install",
    cli_install: "Put the rex command on your PATH to manage rexenv from the terminal. One admin prompt.",
    cli_stale: "points elsewhere (an old copy or another tool) — reinstall to point it at this app.",
    cli_installed: "rex installed — run it from any terminal",
    trust_store: "login keychain",
    privileged_prompt: "asks for your password once",
    os_name: "macOS",
    ca_target: "your Mac",
    elevation_note: "macOS will also ask for your password",
    window_controls_in_content: true,
};

pub const WINDOWS: PlatformWords = PlatformWords {
    reveal: "Show in Explorer",
    file_manager: "File Explorer",
    login_item: "rexenv launches when you sign in to Windows (a startup app — Task Manager's Startup tab can turn it off too).",
    git_install: "On Windows install Git for Windows, then hit Re-detect:\n> winget install --id Git.Git -e",
    node_install: "> winget install --id OpenJS.NodeJS.LTS -e",
    bun_install: "> winget install --id Oven-sh.Bun -e",
    native_build: "That needs the Visual Studio Build Tools with the C++ workload — install them, then retry:\n> winget install --id Microsoft.VisualStudio.2022.BuildTools -e",
    cli_install: "Copy the rex command into rexenv's own folder and add that folder to your user Path, to manage rexenv from the terminal. No admin prompt.",
    cli_stale: "is an older copy — reinstall to update it to this app's rex.",
    cli_installed: "rex installed — open a new terminal to use it",
    trust_store: "Trusted Root store",
    privileged_prompt: "asks for an administrator's approval once",
    os_name: "Windows",
    ca_target: "Windows",
    elevation_note: "Windows will also ask for approval",
    window_controls_in_content: false,
};

/// This build's words.
pub fn current() -> &'static PlatformWords {
    #[cfg(target_os = "windows")]
    {
        &WINDOWS
    }
    #[cfg(not(target_os = "windows"))]
    {
        &MACOS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(w: &PlatformWords) -> [&'static str; 15] {
        [
            w.reveal,
            w.file_manager,
            w.login_item,
            w.git_install,
            w.node_install,
            w.bun_install,
            w.native_build,
            w.cli_install,
            w.cli_stale,
            w.cli_installed,
            w.trust_store,
            w.privileged_prompt,
            w.os_name,
            w.ca_target,
            w.elevation_note,
        ]
    }

    /// Ledger #626 — the macOS words are what the app already showed: moving them into the platform changes no
    /// macOS screen.
    #[test]
    fn macos_keeps_the_words_it_always_showed() {
        assert_eq!(MACOS.reveal, "Show in Finder");
        assert_eq!(MACOS.file_manager, "Finder");
        assert_eq!(MACOS.login_item, "rexenv launches when you sign in to your Mac (a macOS login item).");
        assert!(MACOS.git_install.ends_with("$ xcode-select --install"));
        assert_eq!(MACOS.node_install, "$ brew install node");
        // The CLI card's words, moved here from Settings.tsx by W8 S5 (#634), byte for byte.
        assert_eq!(MACOS.cli_install, "Put the rex command on your PATH to manage rexenv from the terminal. One admin prompt.");
        assert_eq!(MACOS.cli_stale, "points elsewhere (an old copy or another tool) — reinstall to point it at this app.");
        assert_eq!(MACOS.cli_installed, "rex installed — run it from any terminal");
        // The trust, consent and CA words, moved here by W9 S3a (#638) — the text those screens showed.
        assert_eq!(MACOS.trust_store, "login keychain");
        assert_eq!(MACOS.privileged_prompt, "asks for your password once");
        assert_eq!(MACOS.ca_target, "your Mac");
        assert_eq!(MACOS.elevation_note, "macOS will also ask for your password");
    }

    /// Ledger #626 — no Windows word names a macOS thing.
    #[test]
    fn windows_words_name_no_macos_thing() {
        for word in fields(&WINDOWS) {
            for mac in ["Finder", "Mac", "macOS", "Xcode", "xcode", "brew", "$ "] {
                assert!(!word.contains(mac), "Windows word {word:?} names {mac:?}");
            }
        }
        assert!(WINDOWS.reveal.contains("Explorer") && WINDOWS.login_item.contains("Windows"));
        assert!(WINDOWS.git_install.contains("Git for Windows"));
        assert!(WINDOWS.cli_install.contains("user Path") && WINDOWS.cli_install.contains("No admin prompt"), "{}", WINDOWS.cli_install);
        assert!(WINDOWS.cli_installed.contains("new terminal"), "a terminal already open does not see the new Path: {}", WINDOWS.cli_installed);
        assert!(WINDOWS.trust_store.contains("Trusted Root"), "{}", WINDOWS.trust_store);
        assert!(WINDOWS.privileged_prompt.contains("administrator"), "UAC asks for approval, not a password: {}", WINDOWS.privileged_prompt);
        assert_eq!(WINDOWS.os_name, "Windows");
    }

    /// Ledger #636 — the window's controls: macOS draws them over the page (the shell reserves a row),
    /// Windows draws its own title bar above it (reserving there is a dead strip).
    #[test]
    fn only_macos_draws_its_window_controls_inside_the_page() {
        // Through locals: a bare `assert!` on a const is `clippy::assertions_on_constants`, which verify denies.
        let (mac, windows) = (MACOS.window_controls_in_content, WINDOWS.window_controls_in_content);
        assert!(mac, "macOS draws its traffic lights over the page — the shell reserves that row");
        assert!(!windows, "Windows draws its own title bar above the page — reserving there is a dead strip");
    }

    /// Ledger #636 — **the shell reserves that row only where the OS draws its controls over the page**: the
    /// sidebar's spacer is rendered behind `windowControlsInContent`, never unconditionally. Read from the
    /// shell's own source, so a spacer put back by hand is seen.
    #[test]
    fn the_shell_reserves_the_window_control_row_only_where_the_flag_says_so() {
        let sidebar = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/components/shell/Sidebar.tsx"),
        )
        .expect("read the shell's sidebar");
        let spacers: Vec<&str> = sidebar.lines().filter(|l| l.contains(r#"className="h-3""#)).collect();
        assert_eq!(spacers.len(), 1, "expected exactly one reserved-row spacer:\n{spacers:?}");
        assert!(
            spacers[0].contains("windowControlsInContent &&"),
            "the reserved row is rendered unconditionally — on Windows that is a dead strip under the OS's own \
             title bar:\n{}",
            spacers[0].trim()
        );
    }

    /// Ledger #626 — **the UI writes no macOS thing itself**: "Finder", `xcode-select` and `brew install` appear
    /// in the frontend only through the platform's words. A whole-surface claim, so it reads every `.ts`/`.tsx`
    /// file under `src/` — except the browser shell's copy of `MACOS` (`mock.ts`, held to it below) and the two
    /// dev-only review pages, whose fixtures are macOS's on purpose. Comment lines are skipped.
    #[test]
    fn the_frontend_names_macos_things_only_through_the_platform() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        let (mut offenders, mut scanned, mut stack) = (Vec::new(), 0, vec![src]);
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read the frontend source") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                if !(name.ends_with(".ts") || name.ends_with(".tsx"))
                    || ["mock.ts", "DevUiReview.tsx", "DevGitPanel.tsx"].contains(&name.as_str())
                {
                    continue;
                }
                scanned += 1;
                let text = std::fs::read_to_string(&path).expect("read a source file");
                for (i, line) in text.lines().enumerate() {
                    let t = line.trim_start();
                    if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
                        continue;
                    }
                    if ["Finder", "xcode-select", "brew install", "keychain", "your Mac", "with macOS"]
                        .iter()
                        .any(|w| line.contains(w))
                    {
                        offenders.push(format!("{}:{}: {}", path.display(), i + 1, t));
                    }
                }
            }
        }
        assert!(scanned > 50, "scanned only {scanned} files — the walk is not reading the frontend");
        assert!(offenders.is_empty(), "macOS words written into the UI instead of asked for:\n{}", offenders.join("\n"));
    }

    /// The browser shell's words (`src/lib/mock.ts` `mockPlatformWords`) are `MACOS`, word for word.
    #[test]
    fn the_frontend_mock_is_the_macos_words() {
        let mock = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/mock.ts"))
            .expect("read mock.ts");
        for word in fields(&MACOS) {
            let literal = serde_json::to_string(word).expect("a string");
            assert!(mock.contains(&literal), "mock.ts lacks {literal}");
        }
    }
}
