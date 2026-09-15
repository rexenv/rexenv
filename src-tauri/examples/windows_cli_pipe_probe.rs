//! W8's measure-first probe (plan §5 W8, items a–e): what `rex` needs from a named pipe, before S1 builds on it.
//!
//! ```text
//! (elevated SSH)    windows_cli_pipe_probe.exe
//! (desktop session) scripts/probes/windows-cli-pipe-probe.ps1 through windows-limited-token.sh
//! ```
//!
//! A PROBE, not a proof: it prints what each case did, and W8's steps are built on what it prints.
//!
//! - **(a) + (e) the running app's pipe.** The config dir `rex` would build from `%LOCALAPPDATA%` beside the
//!   app's own (`directories`, the Known Folder), the lock pipe's name from it the app's way, and one request
//!   to the RUNNING app over it: a command no build dispatches (`probe.w8`), so the app answers with an error
//!   envelope and does nothing. A reply means the name matched and this token may connect. Run once from the
//!   elevated SSH token and once in the desktop session.
//! - **(b) busy.** A fixture pipe with ONE instance, held by a first client: what a second client's open
//!   answers, what `WaitNamedPipeW` does while it stays held, and whether it lets a client in once the server
//!   disconnects the first and listens again.
//! - **(c) order.** A tokio server that reads a request and writes three progress lines 150 ms apart, then an
//!   envelope, through one pipe split into halves; a blocking client prints when each line arrived.
//! - **(d) the `rex mcp` bridge.** A blocking client handle and its `try_clone`: one thread blocked in a read
//!   while another writes — does the write return, or wait for the read? The echo server answers only what it
//!   receives, and writes one unprompted line 4 s after its client connects, so a serialized pair cannot hang
//!   the probe. **(d2)** the same with a tokio client split into halves, the overlapped alternative.
//!
//! Fixture-owned: the fixture pipes are this process's and die with it; the app's pipe gets one request no
//! build acts on. A watchdog ends the run after 90 s. `demo` tier: Windows-only.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_cli_pipe_probe: skipped — a Windows probe (plan §5 W8)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use sha2::{Digest, Sha256};
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::ptr::{null, null_mut};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
        PIPE_WAIT,
    };

    /// `ERROR_PIPE_CONNECTED`: the client connected before `ConnectNamedPipe` was called — also a connection.
    const ERROR_PIPE_CONNECTED: u32 = 535;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn ms(since: Instant) -> u128 {
        since.elapsed().as_millis()
    }

    /// The app's lock pipe name, copied from `platform/windows/app_pipe_rules.rs` `pipe_name`: the probe
    /// measures whether a client outside the app can compute it.
    fn app_pipe_name(config_dir: &str) -> String {
        let folded = config_dir.trim_end_matches(['\\', '/']).to_lowercase();
        let digest = Sha256::digest(folded.as_bytes());
        let hex: String = digest.iter().take(10).map(|b| format!("{b:02x}")).collect();
        format!(r"\\.\pipe\rexenv-app-{hex}")
    }

    fn open_client(name: &str) -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new().read(true).write(true).open(name)
    }

    fn fixture_name(case: &str) -> String {
        format!(r"\\.\pipe\rexenv-w8-probe-{case}-{}", std::process::id())
    }

    pub fn main() -> ExitCode {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(90));
            println!("## watchdog: 90 s passed — ending the probe");
            std::process::exit(3);
        });
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a tokio runtime");
        app_pipe_case();
        busy_case();
        order_case(&rt);
        bridge_blocking_case(&rt);
        bridge_tokio_case(&rt);
        println!("## done");
        ExitCode::SUCCESS
    }

    fn app_pipe_case() {
        println!("## (a)+(e) the running app's lock pipe");
        let from_env = std::env::var_os("LOCALAPPDATA")
            .map(|la| PathBuf::from(la).join("rexenv").join("rexenv").join("data").join("config"));
        let from_known_folder =
            directories::ProjectDirs::from("dev", "rexenv", "rexenv").map(|d| d.data_local_dir().join("config"));
        println!("  config dir from %LOCALAPPDATA%: {from_env:?}");
        println!("  config dir from the Known Folder (directories): {from_known_folder:?}");
        println!("  equal: {}", from_env.is_some() && from_env == from_known_folder);
        let Some(dir) = from_env else {
            println!("  no LOCALAPPDATA — nothing to dial");
            return;
        };
        let name = app_pipe_name(&dir.to_string_lossy());
        println!("  pipe: {name}");
        let t = Instant::now();
        let client = match open_client(&name) {
            Ok(c) => c,
            Err(e) => {
                println!("  open failed after {} ms: {e} (os error {:?})", ms(t), e.raw_os_error());
                return;
            }
        };
        println!("  open: ok in {} ms", ms(t));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut client = client;
            let sent = client.write_all(b"{\"cmd\":\"probe.w8\",\"args\":{}}\n").is_ok();
            let mut reply = String::new();
            let read = BufReader::new(&client).read_line(&mut reply).map_err(|e| e.to_string());
            let _ = tx.send((sent, read, reply));
        });
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok((sent, read, reply)) => println!("  sent={sent} read={read:?} reply={:?} (+{} ms)", reply.trim(), ms(t)),
            Err(_) => println!("  no reply within 5 s"),
        }
    }

    /// A blocking server pipe that allows ONE instance: the handle, or the error code.
    fn create_one_instance(name: &str) -> Result<HANDLE, u32> {
        let pipe = wide(name);
        // SAFETY: a nul-terminated name; no security attributes (the default descriptor).
        let handle = unsafe {
            CreateNamedPipeW(pipe.as_ptr(), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 4096, 4096, 0, null())
        };
        // SAFETY: plain query, read before anything else can set it.
        let err = unsafe { GetLastError() };
        if handle == INVALID_HANDLE_VALUE { Err(err) } else { Ok(handle) }
    }

    /// Wait for a client on the server handle `raw`: (connected, last error).
    fn listen(raw: isize) -> (bool, u32) {
        // SAFETY: the server handle, alive until `busy_case` closes it after every thread using it is joined.
        let ok = unsafe { ConnectNamedPipe(raw as HANDLE, null_mut()) };
        // SAFETY: plain query.
        let err = unsafe { GetLastError() };
        (ok != 0 || err == ERROR_PIPE_CONNECTED, err)
    }

    fn wait_for(name: &str, timeout_ms: u32) -> (i32, u32, u128) {
        let pipe = wide(name);
        let t = Instant::now();
        // SAFETY: a nul-terminated name.
        let ok = unsafe { WaitNamedPipeW(pipe.as_ptr(), timeout_ms) };
        // SAFETY: plain query.
        let err = unsafe { GetLastError() };
        (ok, err, ms(t))
    }

    fn busy_case() {
        println!("## (b) every instance connected");
        let name = fixture_name("busy");
        let server = match create_one_instance(&name) {
            Ok(h) => h,
            Err(code) => {
                println!("  create failed, error {code}");
                return;
            }
        };
        let raw = server as isize;
        let accept = std::thread::spawn(move || listen(raw));
        let first = match open_client(&name) {
            Ok(f) => f,
            Err(e) => {
                // The accept thread stays blocked on the handle and ends with the process.
                println!("  first client failed: {e}");
                return;
            }
        };
        println!("  first client: connected; server saw {:?}", accept.join().ok());
        match open_client(&name) {
            Ok(_) => println!("  second client while held: CONNECTED (no busy answer)"),
            Err(e) => println!("  second client while held: {e} (os error {:?})", e.raw_os_error()),
        }
        let (ok, err, took) = wait_for(&name, 1500);
        println!("  WaitNamedPipeW(1500 ms) while held: returned {ok} after {took} ms, last error {err}");
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            drop(first);
            // SAFETY: the server handle, as in `listen`.
            let disconnected = unsafe { DisconnectNamedPipe(raw as HANDLE) } != 0;
            (disconnected, listen(raw))
        });
        let (ok, err, took) = wait_for(&name, 5000);
        println!("  WaitNamedPipeW(5000 ms) while the server frees it at ~700 ms: returned {ok} after {took} ms, last error {err}");
        match open_client(&name) {
            Ok(_) => println!("  open after the wait: ok"),
            Err(e) => println!("  open after the wait: {e} (os error {:?})", e.raw_os_error()),
        }
        println!("  server: (disconnected, (listened again, error)) = {:?}", release.join().ok());
        // SAFETY: our own handle; both threads that used it have been joined.
        unsafe { CloseHandle(server) };
    }

    fn fixture_server(rt: &tokio::runtime::Runtime, name: &str) -> Option<NamedPipeServer> {
        let _runtime = rt.enter();
        match ServerOptions::new().first_pipe_instance(true).create(name) {
            Ok(server) => Some(server),
            Err(e) => {
                println!("  server create failed: {e}");
                None
            }
        }
    }

    fn order_case(rt: &tokio::runtime::Runtime) {
        println!("## (c) progress lines, then the envelope, through one split pipe");
        let name = fixture_name("order");
        let Some(server) = fixture_server(rt, &name) else { return };
        rt.spawn(async move {
            if server.connect().await.is_err() {
                return;
            }
            let (read, mut write) = tokio::io::split(server);
            let mut request = String::new();
            let _ = tokio::io::BufReader::new(read).read_line(&mut request).await;
            for i in 1..=3 {
                let _ = write.write_all(format!("{{\"progress\":{i}}}\n").as_bytes()).await;
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            let _ = write.write_all(b"{\"ok\":true}\n").await;
        });
        std::thread::sleep(Duration::from_millis(100));
        let t = Instant::now();
        match open_client(&name) {
            Ok(mut client) => {
                let _ = client.write_all(b"{\"cmd\":\"probe\",\"stream\":true}\n");
                for line in BufReader::new(&client).lines().take(4) {
                    match line {
                        Ok(l) => println!("  +{} ms {l}", ms(t)),
                        Err(e) => {
                            println!("  read error: {e}");
                            break;
                        }
                    }
                }
            }
            Err(e) => println!("  open failed: {e}"),
        }
    }

    /// Echo each line back; `bye` ends it. One unprompted line 4 s after the client connects.
    async fn echo_server(server: NamedPipeServer) {
        if server.connect().await.is_err() {
            return;
        }
        let (read, mut write) = tokio::io::split(server);
        let mut lines = tokio::io::BufReader::new(read).lines();
        let unprompted_at = tokio::time::Instant::now() + Duration::from_secs(4);
        let mut unprompted = false;
        loop {
            tokio::select! {
                line = lines.next_line() => match line {
                    Ok(Some(l)) if l == "bye" => return,
                    Ok(Some(l)) => {
                        let _ = write.write_all(format!("echo {l}\n").as_bytes()).await;
                    }
                    _ => return,
                },
                _ = tokio::time::sleep_until(unprompted_at), if !unprompted => {
                    unprompted = true;
                    let _ = write.write_all(b"unprompted\n").await;
                }
            }
        }
    }

    const WORDS: [&str; 3] = ["one", "two", "bye"];

    fn bridge_blocking_case(rt: &tokio::runtime::Runtime) {
        println!("## (d) the bridge on a blocking handle and its try_clone");
        let name = fixture_name("bridge-blocking");
        let Some(server) = fixture_server(rt, &name) else { return };
        rt.spawn(echo_server(server));
        std::thread::sleep(Duration::from_millis(100));
        let client = match open_client(&name) {
            Ok(c) => c,
            Err(e) => {
                println!("  open failed: {e}");
                return;
            }
        };
        let writer = match client.try_clone() {
            Ok(w) => w,
            Err(e) => {
                println!("  try_clone failed: {e}");
                return;
            }
        };
        let t0 = Instant::now();
        let (read_tx, read_rx) = mpsc::channel::<(u128, String)>();
        std::thread::spawn(move || {
            for line in BufReader::new(&client).lines() {
                match line {
                    Ok(l) => {
                        let _ = read_tx.send((ms(t0), l));
                    }
                    Err(_) => break,
                }
            }
        });
        // The reader is blocked in ReadFile by now: the server writes nothing unprompted for 4 s.
        std::thread::sleep(Duration::from_millis(300));
        let (write_tx, write_rx) = mpsc::channel::<(u128, u128)>();
        std::thread::spawn(move || {
            let mut writer = writer;
            for word in WORDS {
                let start = ms(t0);
                let ok = writer.write_all(format!("{word}\n").as_bytes()).is_ok();
                let _ = write_tx.send((start, ms(t0)));
                if !ok {
                    break;
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        for word in WORDS {
            match write_rx.recv_timeout(Duration::from_secs(8)) {
                Ok((start, end)) => println!("  write {word:?}: started +{start} ms, returned +{end} ms ({} ms)", end - start),
                Err(_) => {
                    println!("  write {word:?}: did not return within 8 s");
                    break;
                }
            }
        }
        while let Ok((at, line)) = read_rx.recv_timeout(Duration::from_millis(1500)) {
            println!("  read +{at} ms {line:?}");
        }
    }

    fn bridge_tokio_case(rt: &tokio::runtime::Runtime) {
        println!("## (d2) the bridge on a tokio client split into halves");
        let name = fixture_name("bridge-tokio");
        let Some(server) = fixture_server(rt, &name) else { return };
        rt.spawn(echo_server(server));
        let log = rt.block_on(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let client = match ClientOptions::new().open(&name) {
                Ok(c) => c,
                Err(e) => return vec![format!("open failed: {e}")],
            };
            let (read, mut write) = tokio::io::split(client);
            let t0 = Instant::now();
            let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let reader_log = std::sync::Arc::clone(&log);
            let reader = tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(read).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    reader_log.lock().expect("the log").push(format!("read +{} ms {l:?}", ms(t0)));
                }
            });
            tokio::time::sleep(Duration::from_millis(300)).await;
            for word in WORDS {
                let start = ms(t0);
                let done = tokio::time::timeout(Duration::from_secs(8), write.write_all(format!("{word}\n").as_bytes())).await;
                let what = match done {
                    Ok(Ok(())) => "returned",
                    Ok(Err(_)) => "failed",
                    Err(_) => "timed out",
                };
                log.lock().expect("the log").push(format!("write {word:?}: started +{start} ms, {what} after {} ms", ms(t0) - start));
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            let _ = tokio::time::timeout(Duration::from_secs(8), reader).await;
            let lines = log.lock().expect("the log").clone();
            lines
        });
        for line in log {
            println!("  {line}");
        }
    }
}
