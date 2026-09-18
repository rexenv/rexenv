//! W7 S2, ledger #621: Windows' `ShellRunner::open` and `reveal` through the real platform, in the desktop
//! session with someone at the screen (windows appear and are closed again).
//!
//! ```text
//! (desktop session) scripts/probes/windows-shell-open.ps1 through windows-limited-token.sh
//! ```
//!
//! In a fixture folder under `%TEMP%` — a folder, a `.log` file, and a `.bat` that would write a marker file
//! if it ever ran:
//! - `open(folder)` → Ok, and an Explorer window titled with the folder appears;
//! - `open(.log)` → Ok, and a window naming the log appears (the `.log` association — Notepad by default);
//! - `open(.bat)` → refused naming ".bat" and "Show in Explorer", and 4 s later the marker file does NOT
//!   exist — the shell never ran it (the load-bearing half: without the refusal, `ShellExecuteW` runs it);
//! - `open(missing)` → refused;
//! - `reveal(.log)` → Ok, and an Explorer window titled with the fixture folder appears;
//! - `reveal(missing)` → an error.
//!
//! Fixture-owned: only windows whose titles name the fixture's own folder or log are closed (`WM_CLOSE`), and
//! only the fixture folder is removed. `demo` tier: Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_shell_open_check: skipped — a Windows check (ledger #621, W7)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use std::path::Path;
    use std::process::ExitCode;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextW, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    /// Every visible top-level window: (handle, class, title).
    fn windows() -> Vec<(isize, String, String)> {
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
                let hwnd = raw as HWND;
                let mut class = [0u16; 128];
                let mut title = [0u16; 512];
                // SAFETY: buffers with their lengths.
                let (c, t, visible) = unsafe {
                    (
                        GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32),
                        GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32),
                        IsWindowVisible(hwnd),
                    )
                };
                (visible != 0).then(|| {
                    (
                        raw,
                        String::from_utf16_lossy(&class[..c.max(0) as usize]),
                        String::from_utf16_lossy(&title[..t.max(0) as usize]),
                    )
                })
            })
            .collect()
    }

    /// Wait up to 10 s for a visible window whose title contains `fragment` (and, when given, of `class`).
    fn window_naming(fragment: &str, class: Option<&str>) -> Option<(isize, String, String)> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(w) = windows()
                .into_iter()
                .find(|(_, c, t)| t.contains(fragment) && class.map_or(true, |want| c == want))
            {
                return Some(w);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        None
    }

    /// Close every visible window whose title contains `fragment` — the fixture's own names only.
    fn close_windows_naming(fragment: &str) -> usize {
        let mine: Vec<_> = windows().into_iter().filter(|(_, _, t)| t.contains(fragment)).collect();
        for (raw, _, _) in &mine {
            // SAFETY: a window handle just enumerated; WM_CLOSE asks it to close like its close button.
            unsafe { PostMessageW(*raw as HWND, WM_CLOSE, 0, 0) };
        }
        mine.len()
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_shell_open_check");
        let tag = format!("rexenv-s2-{}", std::process::id());
        let root = std::env::temp_dir().join(&tag);
        if root.exists() {
            check.is("the fixture folder is new", false, &root.display().to_string());
            return check.verdict();
        }
        let folder = root.join(format!("{tag}-folder"));
        std::fs::create_dir_all(&folder).expect("fixture folder");
        let log = root.join(format!("{tag}.log"));
        std::fs::write(&log, "rexenv S2 check — a log the shell may open\n").expect("fixture log");
        let marker = root.join("the-bat-ran.txt");
        let bat = root.join(format!("{tag}.bat"));
        std::fs::write(&bat, format!("@echo ran> \"{}\"\r\n", marker.display())).expect("fixture bat");

        let plat = rexenv_lib::platform::current();
        let shell = plat.shell();
        let show = |p: &Path| p.display().to_string();

        // ── open(folder) ──
        let opened = shell.open(&show(&folder));
        let explorer = window_naming(&format!("{tag}-folder"), Some("CabinetWClass"));
        check.is("open(folder) is Ok and an Explorer window titled with the folder appears", opened.is_ok() && explorer.is_some(),
            &format!("{opened:?}, {explorer:?}"));

        // ── open(.log) ──
        let opened = shell.open(&show(&log));
        let reader = window_naming(&format!("{tag}.log"), None);
        check.is("open(.log) is Ok and a window naming the log appears", opened.is_ok() && reader.is_some(),
            &format!("{opened:?}, {reader:?}"));

        // ── open(.bat) — refused, and never run ──
        let refused = shell.open(&show(&bat)).map_err(|e| e.to_string());
        std::thread::sleep(Duration::from_secs(4));
        check.is("open(.bat) is refused naming .bat and pointing at Show in Explorer",
            refused.as_ref().is_err_and(|e| e.contains(".bat") && e.contains("Show in Explorer")), &format!("{refused:?}"));
        check.is("…and the .bat never ran (no marker file 4 s later)", !marker.exists(), &show(&marker));

        // ── open(missing) ──
        let missing = shell.open(&show(&root.join("not-here.log"))).map_err(|e| e.to_string());
        check.is("open(missing) is refused", missing.as_ref().is_err_and(|e| e.contains("does not exist")), &format!("{missing:?}"));

        // ── reveal ── (the folder window from the first case closed first, so this one is new)
        close_windows_naming(&format!("{tag}-folder"));
        std::thread::sleep(Duration::from_millis(500));
        let revealed = shell.reveal(&show(&log));
        let explorer = window_naming(&tag, Some("CabinetWClass"));
        check.is("reveal(.log) is Ok and an Explorer window titled with the fixture folder appears",
            revealed.is_ok() && explorer.is_some(), &format!("{revealed:?}, {explorer:?}"));
        let gone = shell.reveal(&show(&root.join("not-here.sql"))).map_err(|e| e.to_string());
        check.is("reveal(missing) is an error", gone.as_ref().is_err_and(|e| e.contains("does not exist")), &format!("{gone:?}"));

        // ── cleanup: the fixture's own windows, then its folder ──
        std::thread::sleep(Duration::from_secs(1));
        let closed = close_windows_naming(&tag);
        std::thread::sleep(Duration::from_secs(2));
        let removed = std::fs::remove_dir_all(&root).is_ok();
        println!("  · cleanup: closed {closed} fixture window(s); fixture removed: {removed}");
        if !removed {
            println!("  · !! {} is left (a reader may still hold the log) — remove it by hand", root.display());
        }
        check.verdict()
    }
}
