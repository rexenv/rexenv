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
    /// This machine, as a sentence points at it ("… won't resolve on this Mac", "this Mac has 2 GB free").
    /// W9 S3b, ledger #639.
    pub host: &'static str,
    /// Where an app with no window on screen lives, as a sentence uses it ("staying in the menu bar").
    pub tray_home: &'static str,
    /// Where the app looked for the user's editors, browsers and terminals, as the "none detected" tooltip
    /// says it — the places [`crate::platform::AppCatalog`] actually searches on this OS.
    pub app_search: &'static str,
    /// What ONE of this OS's DNS routes is, as a sentence counts them ("every rexenv file under
    /// /etc/resolver" / "every rexenv NRPT rule"). `DnsManager::route_label` says where a single TLD's
    /// route lives; this is the same thing with no TLD in hand. W9 S4, ledger #640.
    pub routes_label: &'static str,
    /// How a path in the user's home folder is written on this OS, for a config file the user must place
    /// themselves (`~/.cursor/mcp.json`).
    pub home_prefix: &'static str,
    /// Where the importer looked for other local-dev tools, as the empty state says it.
    pub import_search: &'static str,
    /// Whether rexenv knows where the OTHER local-dev tools (Valet, Herd, Local) keep their sites on this
    /// OS. macOS yes; Windows not yet — `valet_homes`, `herd_home` and `local_home` build macOS layouts
    /// (`Library/Application Support`), while Herd on Windows keeps its tree under `%LOCALAPPDATA%`. Scanning
    /// anyway finds nothing and reports "no sites found", which tells the user something false: the truth is
    /// that nobody looked. Owner ruled 16 Sep 2026 — say so now, port later (W9 S4 follow-up, ledger #641).
    pub imports_other_tools: bool,
    /// Whether this OS draws its window controls INSIDE the page — macOS's traffic lights over a
    /// `titleBarStyle: "Overlay"` window, which the shell reserves a row for. Windows draws its own title
    /// bar above the page, so reserving that row there leaves a dead strip (W9 S1, ruling Q1, ledger #636).
    pub window_controls_in_content: bool,
    /// What one PHP version's worker set IS on this OS, as a sentence names it. D1 ruled Windows runs a
    /// php-cgi GROUP, not a php-fpm pool; #651 gave the SERVICE ROW a platform label and stopped there, so
    /// Settings still told a Windows user their versions "each run a php-fpm pool" — the same wrong noun,
    /// one surface further out (seen on the Dell 19 Sep 2026).
    pub pool_kind: &'static str,
    /// The bundled third-party tools this build actually contains, for the credits line. Not one list with
    /// macOS's answer in it: D4 refuses Redis and MariaDB on Windows (`binaries::ships_on`), and the footer
    /// named both there — a credits line for software that is not in the download (seen the same day). The
    /// test below holds this against `DbEngine::available_on`, so a future pin cannot make it a half-truth.
    pub bundled_tools: &'static str,
    /// This OS's path separator, for the few places the UI must join one itself. SiteDetail built the
    /// wp-config path as `${site.path}/wp-config.php` and rendered
    /// `C:\Users\…\w7check.rex/wp-config.php` — mixed separators, because a literal slash is a path
    /// spelled the macOS way even when nothing about it looks like a macOS word.
    pub path_sep: &'static str,
    /// The ORIGIN this OS's webview serves rexenv's own custom schemes from,
    /// for the Database Browser's `rexdb:` frame.
    ///
    /// macOS answers `rexdb://localhost`; WebView2 serves the SAME handler at
    /// `http://rexdb.localhost/` and does not know the scheme form at all. The
    /// UI built the macOS shape on every platform, so on Windows the iframe's
    /// src resolved to nothing: no request ever reached the handler and the
    /// Database Browser was an empty panel (#703). A fact about the OS belongs
    /// here, next to its path separator, not in a `navigator` sniff.
    pub db_browser_origin: &'static str,
    /// What to do when rexenv is running from somewhere it was not installed to, so an
    /// update has nothing it may safely replace. macOS: drag the bundle into Applications;
    /// Windows: run the installer again. The refusal in `core::app_update` says this
    /// sentence and nothing OS-shaped of its own.
    pub reinstall_to_home: &'static str,
    /// The command that takes ownership of a folder this account cannot replace, ready for a path
    /// to be appended (the updater's two refusals that end in a copyable fix).
    pub take_ownership: &'static str,
    /// The update consent sentence's middle: what is replaced ("rexenv.app" / "rexenv's program
    /// files"). The macOS sentence leaked onto the first Windows update (19 Sep 2026).
    pub update_replaces: &'static str,
    /// The consent sentence's tail about re-granted permissions — macOS's ad-hoc identity;
    /// empty on Windows, where nothing of the kind happens.
    pub update_reprompt: &'static str,
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
    host: "this Mac",
    tray_home: "menu bar",
    app_search: "/Applications and ~/Applications",
    routes_label: "file under /etc/resolver",
    home_prefix: "~",
    import_search: "~/.config/valet and in Herd's and Local's application-support folders",
    imports_other_tools: true,
    window_controls_in_content: true,
    pool_kind: "php-fpm pool",
    bundled_tools: "nginx, PHP, MySQL, MariaDB, PostgreSQL, Redis, Mailpit, Adminer & cloudflared",
    path_sep: "/",
    db_browser_origin: "rexdb://localhost",
    reinstall_to_home: "Move rexenv.app into Applications in Finder, open it from there, then update.",
    take_ownership: "sudo chown -R \"$USER\"",
    update_replaces: "rexenv.app",
    update_reprompt: " macOS may ask again for permissions it had granted this copy: rexenv has no \
         Apple developer signature yet, so each build is a new identity to it.",
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
    host: "this PC",
    tray_home: "notification area",
    app_search: "the installed-programs list, Program Files and %LOCALAPPDATA%",
    routes_label: "NRPT rule",
    home_prefix: "%USERPROFILE%",
    import_search: "Valet's, Herd's and Local's own folders",
    imports_other_tools: false,
    window_controls_in_content: false,
    pool_kind: "php-cgi group",
    bundled_tools: "nginx, PHP, MySQL, PostgreSQL, Mailpit, Adminer & cloudflared",
    path_sep: "\\",
    db_browser_origin: "http://rexdb.localhost",
    reinstall_to_home: "Run the rexenv installer again so it lands in %LOCALAPPDATA%\\rexenv, open it from there, then update.",
    take_ownership: "takeown /R /F",
    update_replaces: "rexenv's program files (your data folder is not touched)",
    update_reprompt: "",
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

    fn fields(w: &PlatformWords) -> [&'static str; 21] {
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
            w.host,
            w.tray_home,
            w.app_search,
            w.routes_label,
            w.home_prefix,
            w.import_search,
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
        // The host, tray-home and app-search words, moved here by W9 S3b (#639) — again, the text those
        // sentences already read on a Mac.
        assert_eq!(MACOS.host, "this Mac");
        assert_eq!(MACOS.tray_home, "menu bar");
        assert_eq!(MACOS.app_search, "/Applications and ~/Applications");
        // The path words, moved here by W9 S4 (#640).
        assert_eq!(MACOS.routes_label, "file under /etc/resolver");
        assert_eq!(MACOS.home_prefix, "~");
        assert_eq!(MACOS.import_search, "~/.config/valet and in Herd's and Local's application-support folders");
    }

    /// **The credits line names only what this build SHIPS**, held against the same predicate the
    /// gates use (`DbEngine::available_on`) rather than against a reading of D4. The Windows footer
    /// listed MariaDB and Redis — two engines `binaries::ships_on` refuses there — so it credited
    /// software that is not in the download (seen on the Dell, 19 Sep 2026).
    #[test]
    fn the_credits_line_names_only_the_engines_this_os_ships() {
        use crate::core::db::DbEngine;
        for (words, os) in [(MACOS, "macos"), (WINDOWS, "windows")] {
            for engine in DbEngine::ALL {
                let name = engine.label();
                assert_eq!(
                    words.bundled_tools.contains(name),
                    engine.available_on(os),
                    "{os}: bundled_tools {:?} and available_on disagree about {name}",
                    words.bundled_tools
                );
            }
        }
    }

    /// Ledger #626 — no Windows word names a macOS thing.
    /// **Each OS's webview origin for rexenv's own schemes is ITS OWN.**
    ///
    /// macOS serves the Database Browser handler at `rexdb://localhost`;
    /// WebView2 serves the same handler at `http://rexdb.localhost/` and does
    /// not know the scheme form at all. The UI built the macOS spelling
    /// everywhere, so on Windows the iframe src resolved to nothing — no
    /// request reached the handler and the panel was empty (#703).
    #[test]
    fn each_os_names_its_own_db_browser_origin() {
        assert_eq!(MACOS.db_browser_origin, "rexdb://localhost");
        assert_eq!(WINDOWS.db_browser_origin, "http://rexdb.localhost");
        assert_ne!(
            MACOS.db_browser_origin, WINDOWS.db_browser_origin,
            "one spelling for both is the bug this pins"
        );
        // Windows' must be a URL a webview can resolve, not a custom scheme.
        assert!(WINDOWS.db_browser_origin.starts_with("http://"), "{}", WINDOWS.db_browser_origin);
    }

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
        assert_eq!(WINDOWS.host, "this PC");
        assert!(WINDOWS.tray_home.contains("notification area"), "{}", WINDOWS.tray_home);
        assert!(
            WINDOWS.app_search.contains("Program Files") && WINDOWS.app_search.contains("installed-programs"),
            "the tooltip must name where the Windows catalog actually looks: {}",
            WINDOWS.app_search
        );
        // Windows has no /etc/resolver: a TLD is routed by an NRPT rule, which is what
        // `WindowsDns::route_label` calls it too.
        assert_eq!(WINDOWS.routes_label, "NRPT rule");
        assert!(!WINDOWS.routes_label.contains("/etc/resolver"));
        assert_eq!(WINDOWS.home_prefix, "%USERPROFILE%");
        assert!(!WINDOWS.import_search.contains('~'), "{}", WINDOWS.import_search);
    }

    /// Ledger #641 — rexenv looks for the other tools' sites only where it knows their layout. Scanning a
    /// Windows home with macOS paths finds nothing and says "no sites found", which is a false answer to a
    /// question nobody asked.
    #[test]
    fn only_macos_knows_where_the_other_tools_keep_their_sites() {
        let (mac, windows) = (MACOS.imports_other_tools, WINDOWS.imports_other_tools);
        assert!(mac, "macOS is where Valet, Herd and Local's layouts are known");
        assert!(!windows, "Windows layouts are not ported yet — the card must say so, not report emptiness");
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
                    // The IPC wrappers' only macOS words are the `isTauri()`-false mock fallbacks —
                    // the browser shell's fixtures, like mock.ts, and shaped like a real macOS answer
                    // on purpose. Nothing here is rendered.
                    || path.to_string_lossy().replace('\\', "/").ends_with("lib/ipc/index.ts")
                {
                    continue;
                }
                scanned += 1;
                let text = std::fs::read_to_string(&path).expect("read a source file");
                for (i, line) in text.lines().enumerate() {
                    let t = line.trim_start();
                    if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") || t.starts_with("{/*") {
                        continue;
                    }
                    // (file, the phrase it may still write, why). A name that is a KEY, or a
                    // sentence about macOS-ORIGIN data that can turn up on any OS, is not a
                    // macOS word written where the platform should have been asked.
                    let excused = [
                        ("routes/Onboarding.tsx", "php-fpm", "a CHIP lookup key, not copy — the service's `name`, which #651 kept stable while moving the display label"),
                        ("components/wordpress/WordPressManager.tsx", "macOS", "`.DS_Store`/`__MACOSX` found INSIDE a WordPress install — macOS-origin files, which reach a Windows machine with any repo cloned from a Mac"),
                    ]
                    .iter()
                    .any(|(f, phrase, _)| path.to_string_lossy().replace('\\', "/").ends_with(f) && line.contains(phrase));
                    if excused {
                        continue;
                    }
                    if [
                        "Finder",
                        "xcode-select",
                        "brew install",
                        "keychain",
                        "your Mac",
                        // The BARE name, not only "with macOS": the Theme card read "System
                        // follows your macOS appearance" and no phrase in this list matched it
                        // (seen on the Dell 19 Sep 2026). The frontend never has a reason to
                        // write this word — the OS's name is `os_name`.
                        "macOS",
                        "this Mac",
                        // An implementation name that is macOS's answer, not the question:
                        // Windows runs a php-cgi group (D1). `pool_kind` is the word.
                        "php-fpm",
                        "/Applications",
                        // A path spelled the macOS way. Measured 16 Sep 2026: after S4 the frontend
                        // writes no `~/…` of its own — every path it shows comes from the backend or
                        // from `home_prefix` (W9 S4, #640).
                        "~/",
                        "/etc/resolver",
                    ]
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

    /// Ledger #639 — **the Rust source names no macOS thing either.** The frontend scan's twin, over both
    /// crates, because a sentence a user reads is written in Rust as often as in TSX: a prompt's reason, a
    /// disk-space message, a `Display` impl, a log line. Test modules are stripped
    /// (`copy_scan::production_source`), so a fixture path like `/Applications/Herd.app` — a macOS fixture on
    /// purpose — is not an offender, and neither is prose in a comment.
    #[test]
    fn the_rust_source_names_macos_things_only_through_the_platform() {
        // (file, the phrase it may still write, why) — a code path that only ever runs on macOS may say so.
        const ALLOWED: &[(&str, &str, &str)] = &[
            ("core/app_update.rs", "this Mac", "self-update is macOS-only until the Windows updater (W11)"),
            ("core/app_update.rs", "/Applications", "the .app bundle it swaps itself into lives there"),
            (
                "core/service_manager.rs",
                "this Mac",
                "macos_floor_note reads a Mach-O load command — macOS by construction",
            ),
        ];
        const FORBIDDEN: &[&str] = &[
            "this Mac",
            "your Mac",
            "login keychain",
            "macOS will also ask",
            "Show in Finder",
            "xcode-select",
            "brew install",
            "/Applications",
            "menu bar",
        ];

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let (mut offenders, mut scanned, mut landmark) = (Vec::new(), 0, false);
        let mut stack = vec![root.join("src"), root.join("../cli/src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read a rust source directory") {
                let path = entry.expect("a directory entry").path();
                let as_text = path.to_string_lossy().replace('\\', "/");
                if path.is_dir() {
                    // The macOS impls are where macOS things BELONG.
                    if !as_text.ends_with("/platform/macos") {
                        stack.push(path);
                    }
                    continue;
                }
                if !as_text.ends_with(".rs") || as_text.ends_with("/platform/words.rs") {
                    continue;
                }
                scanned += 1;
                let text = std::fs::read_to_string(&path).expect("read a rust source file");
                // `production_lines` drops test modules; the prose has to go too, or every
                // "on macOS this is what App Management looks like" comment reads as a violation —
                // and those comments are how the platform boundary is explained.
                let prod: String = crate::core::copy_scan::production_lines(&text)
                    .into_iter()
                    .map(|(_, l)| l)
                    .filter(|l| {
                        let t = l.trim_start();
                        !(t.starts_with("//") || t.starts_with('*') || t.starts_with("/*"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if as_text.ends_with("/core/dns.rs") {
                    landmark |= prod.contains("pub fn configure_resolver");
                }
                for phrase in FORBIDDEN {
                    if !prod.contains(phrase) {
                        continue;
                    }
                    if ALLOWED.iter().any(|(f, p, _)| p == phrase && as_text.ends_with(f)) {
                        continue;
                    }
                    offenders.push(format!("{as_text}: {phrase:?}"));
                }
            }
        }
        assert!(scanned > 100, "scanned only {scanned} files — the walk is not reading the source");
        assert!(landmark, "core/dns.rs came back without its production code — the strip ate the source");
        assert!(
            offenders.is_empty(),
            "macOS words written into the Rust source instead of asked for (platform::words::current()):\n{}",
            offenders.join("\n")
        );
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
