//! W7 S3, ledger #622: Windows' editors, browsers and terminals through the real platform, in the desktop
//! session with someone at the screen (windows appear; the ones naming the fixture are closed again).
//!
//! ```text
//! (desktop session) scripts/probes/windows-app-open.ps1 through windows-limited-token.sh
//! ```
//!
//! Printed first: what `detect_editors`, `detect_browsers` and `detect_terminals` found, each with its
//! executable. Then, in a fixture folder under `%TEMP%` (a folder, and a `.bat` that would write a marker
//! file if it ever ran):
//! - the first detected editor opens the folder → a window naming the folder appears;
//! - Windows PowerShell opens at the folder → a window whose title names the folder's path appears;
//! - `open_in_terminal` handed the `.bat` → refused, and 4 s later the marker does NOT exist;
//! - `open_in_browser` handed a file path → refused; an unknown browser id → refused;
//! - the default browser opens `http://127.0.0.1:9/…` in a private window when it has a flag → Ok (that
//!   window is left for the person at the screen to close: its title does not name the fixture).
//!
//! With `REXENV_EXPECT_EDITORS`, `REXENV_EXPECT_BROWSERS`, `REXENV_EXPECT_TERMINALS` (comma-separated ids,
//! in detection order) and `REXENV_EXPECT_DEFAULT_BROWSER`, the lists are held to what the machine is known
//! to have. Fixture-owned: only windows naming the fixture are closed (`WM_CLOSE`) and only its folder is
//! removed. `demo` tier: Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_app_open_check: skipped — a Windows check (ledger #622, W7)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use std::process::ExitCode;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, IsWindowVisible, PostMessageW, WM_CLOSE};

    fn titles() -> Vec<(isize, String)> {
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
            // SAFETY: `lparam` is the Vec passed below, alive for the enumeration.
            unsafe { (*(lparam as *mut Vec<isize>)).push(hwnd as isize) };
            1
        }
        let mut all: Vec<isize> = Vec::new();
        // SAFETY: `collect` only pushes into `all`.
        unsafe { EnumWindows(Some(collect), &mut all as *mut Vec<isize> as LPARAM) };
        all.into_iter()
            .filter_map(|raw| {
                let mut title = [0u16; 512];
                // SAFETY: a buffer with its length; plain queries on a handle just enumerated.
                let (t, visible) = unsafe {
                    (GetWindowTextW(raw as HWND, title.as_mut_ptr(), title.len() as i32), IsWindowVisible(raw as HWND))
                };
                (visible != 0).then(|| (raw, String::from_utf16_lossy(&title[..t.max(0) as usize])))
            })
            .collect()
    }

    /// Visible console windows (`ConsoleWindowClass`) right now.
    fn consoles() -> Vec<isize> {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
        titles()
            .into_iter()
            .filter(|(raw, _)| {
                let mut class = [0u16; 64];
                // SAFETY: a buffer with its length; a handle just enumerated.
                let n = unsafe { GetClassNameW(*raw as HWND, class.as_mut_ptr(), class.len() as i32) };
                String::from_utf16_lossy(&class[..n.max(0) as usize]) == "ConsoleWindowClass"
            })
            .map(|(raw, _)| raw)
            .collect()
    }

    fn wait_for_title(fragment: &str, secs: u64) -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if let Some((_, t)) = titles().into_iter().find(|(_, t)| t.contains(fragment)) {
                return Some(t);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        None
    }

    fn close_titles(fragment: &str) -> usize {
        let mine: Vec<_> = titles().into_iter().filter(|(_, t)| t.contains(fragment)).collect();
        for (raw, _) in &mine {
            // SAFETY: a handle just enumerated; WM_CLOSE asks the window to close as its close button does.
            unsafe { PostMessageW(*raw as HWND, WM_CLOSE, 0, 0) };
        }
        mine.len()
    }

    fn expect_list(check: &mut Check, var: &str, what: &str, got: &[String]) {
        if let Ok(want) = std::env::var(var) {
            let want: Vec<String> = want.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            check.is(&format!("detected {what} are the machine's known set, in order"), *got == want, &format!("got {got:?}, want {want:?}"));
        }
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_app_open_check");
        let plat = rexenv_lib::platform::current();
        let shell = plat.shell();

        let editors = shell.detect_editors();
        let browsers = shell.detect_browsers();
        let terminals = shell.detect_terminals();
        println!("  · editors:   {:?}", editors.iter().map(|e| &e.id).collect::<Vec<_>>());
        println!("  · browsers:  {:?}", browsers.iter().map(|b| (&b.id, b.system_default, b.supports_private)).collect::<Vec<_>>());
        println!("  · terminals: {:?}", terminals.iter().map(|t| &t.id).collect::<Vec<_>>());
        expect_list(&mut check, "REXENV_EXPECT_EDITORS", "editors", &editors.iter().map(|e| e.id.clone()).collect::<Vec<_>>());
        expect_list(&mut check, "REXENV_EXPECT_BROWSERS", "browsers", &browsers.iter().map(|b| b.id.clone()).collect::<Vec<_>>());
        expect_list(&mut check, "REXENV_EXPECT_TERMINALS", "terminals", &terminals.iter().map(|t| t.id.clone()).collect::<Vec<_>>());
        if let Ok(want) = std::env::var("REXENV_EXPECT_DEFAULT_BROWSER") {
            let defaults: Vec<_> = browsers.iter().filter(|b| b.system_default).map(|b| b.id.clone()).collect();
            check.is("exactly the known default browser is flagged", defaults == [want.clone()], &format!("{defaults:?} vs {want}"));
        }

        let tag = format!("rexenv-s3-{}", std::process::id());
        let root = std::env::temp_dir().join(&tag);
        if root.exists() {
            check.is("the fixture folder is new", false, &root.display().to_string());
            return check.verdict();
        }
        std::fs::create_dir_all(&root).expect("fixture folder");
        let marker = root.join("the-bat-ran.txt");
        let bat = root.join(format!("{tag}.bat"));
        std::fs::write(&bat, format!("@echo ran> \"{}\"\r\n", marker.display())).expect("fixture bat");

        // ── an editor opens the folder ──
        if let Some(editor) = editors.first() {
            let opened = shell.open_in_editor(&editor.id, &root.display().to_string());
            let window = wait_for_title(&tag, 60);
            check.is(&format!("{} opens the folder and a window naming it appears", editor.name), opened.is_ok() && window.is_some(),
                &format!("{opened:?}, {window:?}"));
        }

        // ── a terminal opens in a console of its own ──
        // Held to a NEW console window, not a title: the editor's window above already names the fixture, and
        // a first version of this check passed on it while the PowerShell wrote its prompt into this check's
        // redirected output instead of its window.
        let mut new_consoles: Vec<isize> = Vec::new();
        if terminals.iter().any(|t| t.id == "powershell") {
            let before: Vec<isize> = consoles();
            let opened = shell.open_in_terminal("powershell", &root);
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline && new_consoles.is_empty() {
                new_consoles = consoles().into_iter().filter(|h| !before.contains(h)).collect();
                std::thread::sleep(Duration::from_millis(300));
            }
            check.is("Windows PowerShell opens and a NEW console window appears", opened.is_ok() && !new_consoles.is_empty(),
                &format!("{opened:?}, new console windows {new_consoles:?}"));
        }

        // ── refusals ──
        let term_file = terminals.first().map(|t| shell.open_in_terminal(&t.id, &bat).map_err(|e| e.to_string()));
        std::thread::sleep(Duration::from_secs(4));
        check.is("a terminal handed a .bat is refused", term_file.as_ref().is_some_and(|r| r.is_err()), &format!("{term_file:?}"));
        check.is("…and the .bat never ran (no marker 4 s later)", !marker.exists(), &marker.display().to_string());
        if let Some(b) = browsers.first() {
            let file = shell.open_in_browser(&b.id, &bat.display().to_string(), false).map_err(|e| e.to_string());
            check.is("a browser handed a file path is refused", file.as_ref().is_err_and(|e| e.contains("http")), &format!("{file:?}"));
        }
        let unknown = shell.open_in_browser("not-a-browser", "https://a.rex/", false);
        check.is("an unknown browser id is refused", unknown.is_err(), &format!("{unknown:?}"));

        // ── the default browser, private when it can ──
        if let Some(b) = browsers.iter().find(|b| b.system_default).or(browsers.first()) {
            let opened = shell.open_in_browser(&b.id, &format!("http://127.0.0.1:9/{tag}"), b.supports_private);
            check.is(&format!("{} opens a loopback URL (private: {})", b.name, b.supports_private), opened.is_ok(), &format!("{opened:?}"));
        }

        std::thread::sleep(Duration::from_secs(2));
        let closed = close_titles(&tag);
        for hwnd in &new_consoles {
            // SAFETY: a console window this check opened; WM_CLOSE asks it to close as its close button does.
            unsafe { PostMessageW(*hwnd as HWND, WM_CLOSE, 0, 0) };
        }
        // A window still closing keeps the folder as its working directory for a moment.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut removed = false;
        while Instant::now() < deadline && !removed {
            std::thread::sleep(Duration::from_millis(500));
            removed = std::fs::remove_dir_all(&root).is_ok() || !root.exists();
        }
        println!(
            "  · cleanup: closed {closed} window(s) naming the fixture and {} console(s); fixture removed: {removed} (the browser window is the person's to close)",
            new_consoles.len()
        );
        if !removed {
            println!("  · !! {} is left — something still has it open", root.display());
        }
        check.verdict()
    }
}
