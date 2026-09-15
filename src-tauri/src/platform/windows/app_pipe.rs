//! Windows' single-instance lock (W7 S1, plan §5 W7 ruling Q1, ledger #620) — the Win32 half of
//! `app_pipe_rules.rs`.
//!
//! The claim runs before Tauri boots, so before any tokio runtime: the first instance is a plain
//! `CreateNamedPipeW` with `FILE_FLAG_FIRST_PIPE_INSTANCE`, overlapped so tokio can adopt the handle later,
//! remote clients rejected, and the owner-only descriptor `acl.rs` gives private files (ledger #597) — only
//! this user may connect. Every later instance comes from tokio's `ServerOptions` with the same
//! descriptor. Measured shape (the Dell, 15 Sep 2026): a second process's first-instance create is refused
//! with error 5 and its client connect still reaches the holder.

use super::acl::OwnerOnlyDescriptor;
use super::app_pipe_rules::{claim_from, pipe_name, Claim, APP_OPEN};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
    PIPE_WAIT,
};

/// `ERROR_PIPE_BUSY`: every instance is connected; the holder makes the next one a moment later.
const ERROR_PIPE_BUSY: i32 = 231;

/// The lock, held: the first instance's handle, closed on drop unless tokio adopted it.
pub struct HeldAppPipe {
    name: String,
    handle: isize,
}

impl HeldAppPipe {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Hand the first instance to tokio. Must run inside the runtime.
    pub fn into_server(self) -> std::io::Result<NamedPipeServer> {
        let handle = self.handle;
        std::mem::forget(self);
        // SAFETY: an owned, overlapped pipe handle this struct created and never closed; forgotten above,
        // so it has exactly one owner from here.
        unsafe { NamedPipeServer::from_raw_handle(handle as _) }
    }
}

impl Drop for HeldAppPipe {
    fn drop(&mut self) {
        // SAFETY: the handle this struct owns, closed once.
        unsafe { CloseHandle(self.handle as HANDLE) };
    }
}

/// What claiming the lock found.
pub enum AppPipeClaim {
    Ours(HeldAppPipe),
    AnotherInstance,
    Unclear(u32),
}

/// Claim the lock for the app-data directory whose config folder is `config_dir`.
pub fn claim(config_dir: &Path) -> AppPipeClaim {
    let name = pipe_name(&config_dir.to_string_lossy());
    let created = create_first(&name);
    match claim_from(created.as_ref().map(|_| ()).map_err(|code| *code)) {
        Claim::Ours => match created {
            Ok(handle) => AppPipeClaim::Ours(HeldAppPipe { name, handle: handle as isize }),
            Err(code) => AppPipeClaim::Unclear(code),
        },
        Claim::AnotherInstance => AppPipeClaim::AnotherInstance,
        Claim::Unclear(code) => AppPipeClaim::Unclear(code),
    }
}

/// The first instance, or the Win32 error. A descriptor that cannot be built is `u32::MAX` — unclear, so
/// the app starts without the lock rather than with a pipe anyone could connect to.
fn create_first(name: &str) -> Result<HANDLE, u32> {
    let descriptor = OwnerOnlyDescriptor::for_current_user().map_err(|_| u32::MAX)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.as_ptr(),
        bInheritHandle: 0,
    };
    let wide: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().chain(Some(0)).collect();
    // SAFETY: a nul-terminated name and security attributes whose descriptor lives until the end of this
    // function, past the call.
    let handle = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        // SAFETY: plain query, read straight after the failing call.
        Err(unsafe { GetLastError() })
    } else {
        Ok(handle)
    }
}

/// The next instance for the next client, with the same descriptor. Must run inside the runtime.
pub fn next_instance(name: &str) -> std::io::Result<NamedPipeServer> {
    let descriptor = OwnerOnlyDescriptor::for_current_user().map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.as_ptr(),
        bInheritHandle: 0,
    };
    // SAFETY: `attributes` points at a valid SECURITY_ATTRIBUTES whose descriptor outlives the call.
    unsafe {
        ServerOptions::new()
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, &mut attributes as *mut SECURITY_ATTRIBUTES as *mut _)
    }
}

/// Ask the instance holding the lock to show its window. True when a holder was reached — the caller
/// exits; false when nobody is there any more (it died between the claim and now), and the caller starts.
/// Best-effort past the connect, as on macOS: a wedged app that never answers still owns the stack.
pub fn hand_off(config_dir: &Path) -> bool {
    let name = pipe_name(&config_dir.to_string_lossy());
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut pipe = loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(&name) {
            Ok(pipe) => break pipe,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    };
    let _ = pipe.write_all(format!("{{\"cmd\":\"{APP_OPEN}\",\"args\":{{}}}}\n").as_bytes());
    let _ = pipe.flush();
    // A pipe read has no timeout; read on a thread and stop waiting after 1.5 s.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 64];
        let _ = tx.send(pipe.read(&mut buf).is_ok());
    });
    let _ = rx.recv_timeout(Duration::from_millis(1500));
    eprintln!("rexenv is already running — brought its window to the front. This second copy has exited.");
    true
}
