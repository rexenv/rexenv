//! A unix-domain stream socket client on Windows (AF_UNIX, Windows 10 1803+) — how rexenv reaches
//! Caddy's admin API there (`WindowsLocalIpc`, ledger #611). std and tokio have no unix sockets on
//! Windows; Winsock does. Measured on the Dell (14 Sep 2026, `windows_edge_probe`): `socket(AF_UNIX)`
//! + `connect` to Caddy's admin socket answered `GET /config/` with 200.

use super::ipc_rules;
use std::io;
use std::path::Path;
use std::time::Duration;
use windows_sys::Win32::Networking::WinSock::{
    closesocket, connect, recv, setsockopt, socket, WSAGetLastError, WSAStartup, AF_UNIX, INVALID_SOCKET, SOCKADDR,
    SOCKADDR_UN, SOCKET, SOCK_STREAM, SOL_SOCKET, SO_RCVTIMEO, WSADATA,
};

/// A connected AF_UNIX stream, closed on drop.
pub(crate) struct UnixStream(SOCKET);

// SAFETY: a Winsock socket handle may be used from any thread.
unsafe impl Send for UnixStream {}

impl Drop for UnixStream {
    fn drop(&mut self) {
        // SAFETY: opened by `connect_path`, closed exactly once.
        unsafe { closesocket(self.0) };
    }
}

impl io::Read for UnixStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = buf.len().min(i32::MAX as usize) as i32;
        // SAFETY: `len` bytes of `buf` are writable; the socket is open.
        let n = unsafe { recv(self.0, buf.as_mut_ptr(), len, 0) };
        if n < 0 {
            // SAFETY: a plain call.
            return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
        }
        Ok(n as usize)
    }
}

/// Connect to the AF_UNIX socket at `path`. `read_timeout` bounds each read (`SO_RCVTIMEO`). A
/// failed connect reports `NotFound` for no socket file and `ConnectionRefused` for a file nobody
/// serves (`ipc_rules::connect_error_kind`).
pub(crate) fn connect_path(path: &Path, read_timeout: Option<Duration>) -> io::Result<UnixStream> {
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "an AF_UNIX socket path must be UTF-8"))?;
    let bytes = text.as_bytes();
    // `sun_path` is 108 bytes and must end in a NUL.
    if bytes.len() >= 108 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the socket path is {} bytes; an AF_UNIX address holds 107", bytes.len()),
        ));
    }
    // SAFETY: plain Winsock calls on values owned by this frame; the socket is wrapped in
    // `UnixStream` right after it is created, so every return path closes it.
    unsafe {
        let mut wsa: WSADATA = std::mem::zeroed();
        WSAStartup(0x0202, &mut wsa);
        let raw = socket(AF_UNIX as i32, SOCK_STREAM, 0);
        if raw == INVALID_SOCKET {
            return Err(io::Error::from_raw_os_error(WSAGetLastError()));
        }
        let stream = UnixStream(raw);
        if let Some(timeout) = read_timeout {
            let ms = timeout.as_millis().clamp(1, u128::from(u32::MAX)) as u32;
            setsockopt(raw, SOL_SOCKET, SO_RCVTIMEO, (&ms as *const u32).cast(), 4);
        }
        let mut addr: SOCKADDR_UN = std::mem::zeroed();
        addr.sun_family = AF_UNIX;
        for (i, b) in bytes.iter().enumerate() {
            addr.sun_path[i] = *b as i8;
        }
        let size = std::mem::size_of::<SOCKADDR_UN>() as i32;
        if connect(raw, (&addr as *const SOCKADDR_UN).cast::<SOCKADDR>(), size) != 0 {
            let code = WSAGetLastError();
            let kind = ipc_rules::connect_error_kind(code, path.exists());
            return Err(io::Error::new(kind, io::Error::from_raw_os_error(code)));
        }
        Ok(stream)
    }
}
