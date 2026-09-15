//! W7's measure-first probe (plan §5 W7, items a, d, e): what the desktop user's token can do with a named
//! pipe as a single-instance lock, a directory junction as a linked folder, and the shell's open/reveal.
//!
//! ```text
//! (desktop session) scripts/probes/windows-desktop-probe.ps1 through windows-limited-token.sh
//! ```
//!
//! A PROBE, not a proof: it prints what each case did, and W7's steps are built on what it prints.
//!
//! - **(a) the pipe.** A server pipe created with `FILE_FLAG_FIRST_PIPE_INSTANCE` and a protected DACL
//!   naming only the current user (the SDDL shape `owner_only.rs` builds, with pipe rights). A SECOND
//!   process — this example re-run as `pipe-second <name>` — tries to create the same pipe first-instance
//!   (the lock question) and then connects as a client and sends one line (the hand-off question). After
//!   the server's handle closes, a new first-instance create must succeed: the lock dies with its holder.
//! - **(d) junctions.** In a fixture folder under the temp directory: `mklink /J` by this token (no
//!   elevation), what `symlink_metadata(..).file_type().is_symlink()` says of the junction — the exact
//!   call `core::repo::partition_symlink_deletes` guards a linked checkout with — `read_link`, and what
//!   `remove_file`, `remove_dir` and `remove_dir_all` do to a junction and to the files behind it (one
//!   target per case, so a walk into one damages only that case's own fixture file). And whether
//!   `std::os::windows::fs::symlink_dir` works without Developer Mode.
//! - **(e) open/reveal** — ONLY with `REXENV_PROBE_OPEN=1`, because it opens windows on the screen of
//!   whoever is at the machine: `ShellExecuteW("open")` on the fixture folder and on an `http` URL to a
//!   closed loopback port, and `explorer.exe /select,<file>`; then the Explorer windows (`CabinetWClass`)
//!   whose titles name the fixture folder.
//!
//! Fixture-owned: only paths this run created are removed, links before their targets, the root last.
//! `demo` tier: Windows-only.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_desktop_probe: skipped — a Windows probe (plan §5 W7)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("pipe-second") {
        windows::pipe_second(&args[2]);
        return std::process::ExitCode::SUCCESS;
    }
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode};
    use std::ptr::{null, null_mut};
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    /// This token's user SID, from `whoami` — the probe measures the pipe, not the SID lookup.
    fn user_sid() -> Option<String> {
        let out = Command::new("whoami").args(["/user", "/fo", "csv", "/nh"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.trim().rsplit(',').next().map(|s| s.trim_matches('"').to_string()).filter(|s| s.starts_with("S-1-"))
    }

    /// A first-instance server pipe with a protected DACL naming only `sid`: the handle, or the error code.
    fn create_first(name: &str, sid: Option<&str>) -> Result<HANDLE, u32> {
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 0,
        };
        if let Some(sid) = sid {
            let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
            // SAFETY: a nul-terminated SDDL string and out-pointers that live for the call.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, null_mut())
            };
            if ok == 0 {
                // SAFETY: plain query.
                return Err(unsafe { GetLastError() });
            }
            attributes.lpSecurityDescriptor = descriptor;
        }
        let pipe = wide(name);
        // SAFETY: a nul-terminated name and security attributes alive for the call.
        let handle = unsafe {
            CreateNamedPipeW(
                pipe.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                if sid.is_some() { &attributes } else { null() },
            )
        };
        // SAFETY: plain query, read before anything else can set it.
        let err = unsafe { GetLastError() };
        if !descriptor.is_null() {
            // SAFETY: allocated by the conversion above.
            unsafe { LocalFree(descriptor as _) };
        }
        if handle == INVALID_HANDLE_VALUE { Err(err) } else { Ok(handle) }
    }

    /// The second process: the lock question, then the hand-off question.
    pub fn pipe_second(name: &str) {
        match create_first(name, user_sid().as_deref()) {
            Ok(h) => {
                println!("second: first-instance create SUCCEEDED — the pipe is not a lock");
                // SAFETY: our own handle.
                unsafe { CloseHandle(h) };
            }
            Err(code) => println!("second: first-instance create refused, error {code}"),
        }
        match std::fs::OpenOptions::new().read(true).write(true).open(name) {
            Ok(mut client) => {
                let sent = client.write_all(b"{\"cmd\":\"app.open\",\"args\":{}}\n").and_then(|_| client.flush());
                let mut reply = String::new();
                let _ = BufReader::new(&client).read_line(&mut reply);
                println!("second: connected as a client; sent {:?}; reply {:?}", sent.map(|_| "ok"), reply.trim());
            }
            Err(e) => println!("second: client connect failed: {e} (os error {:?})", e.raw_os_error()),
        }
    }

    fn pipe_case() {
        println!("## (a) named pipe as a single-instance lock");
        let sid = user_sid();
        println!("  user SID: {sid:?}");
        let name = format!(r"\\.\pipe\rexenv-w7-probe-{}", std::process::id());
        let server = match create_first(&name, sid.as_deref()) {
            Ok(h) => h,
            Err(code) => {
                println!("  first-instance create FAILED, error {code} — nothing else to measure");
                return;
            }
        };
        println!("  server created: {name}");
        let raw = server as isize;
        let serve = std::thread::spawn(move || {
            let h = raw as HANDLE;
            // SAFETY: the server handle, alive until this thread's owner closes it after join.
            let ok = unsafe { ConnectNamedPipe(h, null_mut()) };
            // SAFETY: plain query.
            let err = unsafe { GetLastError() };
            // ERROR_PIPE_CONNECTED (535) means the client connected before the call — also a connection.
            let connected = ok != 0 || err == 535;
            let mut line = String::new();
            if connected {
                // SAFETY: borrow the handle as a File for reading; `into_raw_handle` below gives it back.
                let file = unsafe { <std::fs::File as std::os::windows::io::FromRawHandle>::from_raw_handle(h as _) };
                let mut reader = BufReader::new(&file);
                let _ = reader.read_line(&mut line);
                let _ = (&file).write_all(b"ok\n");
                let _ = std::os::windows::io::IntoRawHandle::into_raw_handle(file);
            }
            (connected, err, line)
        });
        let exe = std::env::current_exe().expect("own path");
        match Command::new(&exe).args(["pipe-second", &name]).output() {
            Ok(out) => print!("{}", String::from_utf8_lossy(&out.stdout).lines().map(|l| format!("  {l}\n")).collect::<String>()),
            Err(e) => println!("  could not start the second process: {e}"),
        }
        match serve.join() {
            Ok((connected, err, line)) => println!("  server: connected={connected} (last error {err}), read {:?}", line.trim()),
            Err(_) => println!("  server thread panicked"),
        }
        // SAFETY: our own handle.
        unsafe { CloseHandle(server) };
        match create_first(&name, sid.as_deref()) {
            Ok(h) => {
                println!("  after the holder closed: a new first-instance create SUCCEEDED (the lock dies with its handle)");
                // SAFETY: our own handle.
                unsafe { CloseHandle(h) };
            }
            Err(code) => println!("  after the holder closed: create still refused, error {code}"),
        }
        match create_first(&format!(r"\\.\pipe\rexenv-w7-probe-{}\sub", std::process::id()), sid.as_deref()) {
            Ok(h) => {
                println!("  a backslash inside the pipe name: accepted");
                // SAFETY: our own handle.
                unsafe { CloseHandle(h) };
            }
            Err(code) => println!("  a backslash inside the pipe name: refused, error {code}"),
        }
    }

    fn write_marker(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).expect("fixture target");
        let file = dir.join("keep-me.txt");
        std::fs::write(&file, b"the user's real checkout\n").expect("fixture file");
        file
    }

    fn mklink_junction(link: &Path, target: &Path) -> bool {
        let out = Command::new("cmd").args(["/c", "mklink", "/J"]).arg(link).arg(target).output();
        match out {
            Ok(o) => {
                println!(
                    "  mklink /J {} → exit {:?}: {}",
                    link.file_name().unwrap_or_default().to_string_lossy(),
                    o.status.code(),
                    String::from_utf8_lossy(&o.stdout).trim()
                );
                o.status.success()
            }
            Err(e) => {
                println!("  mklink could not run: {e}");
                false
            }
        }
    }

    fn describe(link: &Path) {
        match std::fs::symlink_metadata(link) {
            Ok(m) => println!(
                "  symlink_metadata: is_symlink={} is_dir={} (the delete guard reads is_symlink)",
                m.file_type().is_symlink(),
                m.file_type().is_dir()
            ),
            Err(e) => println!("  symlink_metadata failed: {e}"),
        }
        println!("  read_link: {:?}", std::fs::read_link(link).map_err(|e| e.to_string()));
        println!("  metadata (follows): is_dir={:?}", std::fs::metadata(link).map(|m| m.is_dir()).map_err(|e| e.to_string()));
    }

    fn junction_case(root: &Path) {
        println!("## (d) directory junctions (fixture {})", root.display());
        // Case 1 — remove_file on a junction.
        let t1 = root.join("target1");
        let f1 = write_marker(&t1);
        let l1 = root.join("link1");
        if mklink_junction(&l1, &t1) {
            describe(&l1);
            let r = std::fs::remove_file(&l1);
            println!(
                "  remove_file(link): {:?}; link still there: {}; target file still there: {}",
                r.map_err(|e| format!("{e} (os error {:?})", e.raw_os_error())),
                std::fs::symlink_metadata(&l1).is_ok(),
                f1.exists()
            );
        }
        // Case 2 — remove_dir on a junction.
        let t2 = root.join("target2");
        let f2 = write_marker(&t2);
        let l2 = root.join("link2");
        if mklink_junction(&l2, &t2) {
            let r = std::fs::remove_dir(&l2);
            println!(
                "  remove_dir(link): {:?}; link still there: {}; target file still there: {}",
                r.map_err(|e| format!("{e} (os error {:?})", e.raw_os_error())),
                std::fs::symlink_metadata(&l2).is_ok(),
                f2.exists()
            );
        }
        // Case 3 — remove_dir_all on a junction: does it walk into the target?
        let t3 = root.join("target3");
        let f3 = write_marker(&t3);
        let l3 = root.join("link3");
        if mklink_junction(&l3, &t3) {
            let r = std::fs::remove_dir_all(&l3);
            println!(
                "  remove_dir_all(link): {:?}; link still there: {}; target file still there: {}",
                r.map_err(|e| format!("{e} (os error {:?})", e.raw_os_error())),
                std::fs::symlink_metadata(&l3).is_ok(),
                f3.exists()
            );
        }
        // Case 4 — a symbolic link to a directory without Developer Mode.
        let t4 = root.join("target4");
        write_marker(&t4);
        let l4 = root.join("link4");
        match std::os::windows::fs::symlink_dir(&t4, &l4) {
            Ok(()) => {
                println!("  symlink_dir: CREATED (Developer Mode or a privilege allows it)");
                describe(&l4);
                let _ = std::fs::remove_dir(&l4);
            }
            Err(e) => println!("  symlink_dir: refused: {e} (os error {:?})", e.raw_os_error()),
        }
    }

    fn explorer_windows_naming(fragment: &str) -> Vec<String> {
        use windows_sys::Win32::Foundation::{HWND, LPARAM};
        use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowTextW, IsWindowVisible};
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
            // SAFETY: `lparam` is the Vec passed below, alive for the enumeration.
            unsafe { (*(lparam as *mut Vec<isize>)).push(hwnd as isize) };
            1
        }
        let mut all: Vec<isize> = Vec::new();
        // SAFETY: `collect` only pushes into `all`.
        unsafe { EnumWindows(Some(collect), &mut all as *mut Vec<isize> as LPARAM) };
        let mut found = Vec::new();
        for raw in all {
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
            let class = String::from_utf16_lossy(&class[..c.max(0) as usize]);
            let title = String::from_utf16_lossy(&title[..t.max(0) as usize]);
            if class == "CabinetWClass" && visible != 0 && title.contains(fragment) {
                found.push(format!("{title} [{class}]"));
            }
        }
        found
    }

    fn open_case(root: &Path) {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        println!("## (e) open and reveal (REXENV_PROBE_OPEN=1)");
        let folder = root.join("open-me");
        let file = write_marker(&folder);
        let verb = wide("open");
        for target in [folder.display().to_string(), "http://127.0.0.1:9/rexenv-w7-probe".to_string()] {
            let t = wide(&target);
            // SAFETY: nul-terminated strings alive for the call; no parent window.
            let r = unsafe { ShellExecuteW(null_mut(), verb.as_ptr(), t.as_ptr(), null(), null(), SW_SHOWNORMAL) };
            println!("  ShellExecuteW open {target} → {} (> 32 is success)", r as isize);
        }
        let reveal = Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", file.display())).status();
        println!("  explorer.exe /select → {:?} (explorer's exit code carries no meaning)", reveal.map(|s| s.code()));
        std::thread::sleep(Duration::from_secs(4));
        let name = folder.file_name().unwrap_or_default().to_string_lossy().to_string();
        println!("  Explorer windows naming {name:?}: {:?}", explorer_windows_naming(&name));
    }

    pub fn main() -> ExitCode {
        let root = std::env::temp_dir().join(format!("rexenv-w7-probe-{}", std::process::id()));
        if root.exists() {
            println!("fixture {} already exists — refusing to touch it", root.display());
            return ExitCode::FAILURE;
        }
        std::fs::create_dir_all(&root).expect("fixture root");
        pipe_case();
        junction_case(&root);
        if std::env::var("REXENV_PROBE_OPEN").as_deref() == Ok("1") {
            open_case(&root);
        } else {
            println!("## (e) skipped — set REXENV_PROBE_OPEN=1 with someone at the screen");
        }
        // Cleanup: links first (remove_dir removes a junction itself), then the fixture's own targets.
        for link in ["link1", "link2", "link3", "link4"] {
            let p = root.join(link);
            if std::fs::symlink_metadata(&p).is_ok() {
                let _ = std::fs::remove_dir(&p).or_else(|_| std::fs::remove_file(&p));
            }
        }
        let leftover_links: Vec<_> = ["link1", "link2", "link3", "link4"].iter().filter(|l| std::fs::symlink_metadata(root.join(l)).is_ok()).collect();
        if leftover_links.is_empty() {
            let _ = std::fs::remove_dir_all(&root);
            println!("## cleanup: fixture removed: {}", !root.exists());
        } else {
            println!("## cleanup: links still present {leftover_links:?} — fixture LEFT at {} (not walked)", root.display());
        }
        println!("windows_desktop_probe: done");
        ExitCode::SUCCESS
    }
}
