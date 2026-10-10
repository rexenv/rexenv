//! Every Winsock socket rexenv opens by hand is opened HERE, and opened uninheritable
//! (ledger #850).
//!
//! **Why this module exists — measured on the Dell, 10 Oct 2026.** Winsock's plain `socket()`
//! returns an INHERITABLE handle, and `std::process::Command` spawns with `bInheritHandles`
//! TRUE. The handle sweep (#600) clears inherit flags once, at process start — a socket opened
//! later is not covered by it. So when the app ran the resolver in-process (the agent had been
//! killed) and then started the stack, every php-cgi, nginx and MySQL it spawned got a copy of
//! the `127.0.0.1:53` socket. The app's handoff then closed ITS handle, the children's copies
//! kept the port bound, the agent could not bind (WSAEADDRINUSE every 10 s), the app's own rebind
//! failed the same way, and the watchdog named the app itself as the holder. `.rex` stopped
//! resolving until `rex stop` killed the children — and would have stayed dead after the app
//! quit, since the children outlive it. The AF_UNIX admin client (#611) had the same flag; a
//! spawn during its short life would hand a child a connection to the edge's admin socket.
//!
//! `WSA_FLAG_OVERLAPPED` is kept because it is what `socket()` itself sets — the only change
//! from `socket()` is the inherit flag.

use std::io;
use windows_sys::Win32::Networking::WinSock::{
    WSAGetLastError, WSASocketW, WSAStartup, INVALID_SOCKET, SOCKET, WSADATA, WSA_FLAG_NO_HANDLE_INHERIT,
    WSA_FLAG_OVERLAPPED,
};

/// `socket(af, kind, protocol)`, except a child process never inherits it.
pub(crate) fn uninheritable_socket(af: i32, kind: i32, protocol: i32) -> io::Result<SOCKET> {
    // SAFETY: plain Winsock calls; the returned socket is the caller's to close.
    unsafe {
        let mut wsa: WSADATA = std::mem::zeroed();
        WSAStartup(0x0202, &mut wsa);
        let s = WSASocketW(af, kind, protocol, std::ptr::null(), 0, WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT);
        if s == INVALID_SOCKET {
            return Err(io::Error::from_raw_os_error(WSAGetLastError()));
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::Networking::WinSock::{closesocket, AF_INET, AF_UNIX, IPPROTO_UDP, SOCK_DGRAM, SOCK_STREAM};

    fn inherit_flag(s: SOCKET) -> bool {
        let mut flags = 0u32;
        // SAFETY: a socket this test opened; a socket is a kernel handle.
        assert!(unsafe { GetHandleInformation(s as HANDLE, &mut flags) } != 0, "GetHandleInformation failed");
        flags & HANDLE_FLAG_INHERIT != 0
    }

    /// Both kinds rexenv opens — the resolver's UDP and the admin client's AF_UNIX stream.
    #[test]
    fn sockets_are_born_uninheritable() {
        for (af, kind, proto) in [(AF_INET as i32, SOCK_DGRAM, IPPROTO_UDP), (AF_UNIX as i32, SOCK_STREAM, 0)] {
            let s = uninheritable_socket(af, kind, proto).expect("socket");
            assert!(!inherit_flag(s), "af {af}: the socket is inheritable");
            // SAFETY: opened above, closed once.
            unsafe { closesocket(s) };
        }
    }

    /// The consequence that mattered: a child spawned while the resolver socket is open must not
    /// keep its port after we close it. With `socket()` back in `uninheritable_socket` the rebind
    /// fails with WSAEADDRINUSE (10048) — the Dell's `:53`, on an ephemeral port.
    #[test]
    fn a_child_spawned_while_the_resolver_is_open_does_not_keep_its_port() {
        let first = crate::platform::windows::resolver_socket::bind_resolver_udp(0).expect("bind");
        let port = first.local_addr().unwrap().port();
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 15 127.0.0.1 >NUL"])
            .spawn()
            .expect("spawn a long-lived child");
        drop(first);
        let again = crate::platform::windows::resolver_socket::bind_resolver_udp(port);
        let _ = child.kill();
        let _ = child.wait();
        assert!(again.is_ok(), "rebinding :{port} after close failed while a child lived: {:?}", again.err());
    }
}
