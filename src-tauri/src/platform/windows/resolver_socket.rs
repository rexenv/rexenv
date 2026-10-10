//! The embedded resolver's socket on Windows (ledger #615): UDP on `127.0.0.1:<port>`, with
//! `SO_EXCLUSIVEADDRUSE` and UDP connection-reset reports switched off.
//!
//! **Exclusive** — plan §3 D2 ruled it, and measured that it is not a cost: bound this way on
//! `127.0.0.1:53` the resolver binds and answers loopback beside a wildcard `0.0.0.0` or `[::]`
//! holder (ICS holds UDP `0.0.0.0:53` on any machine with WSL 2); the holders that refuse it are a
//! `127.0.0.1` holder (WSAEADDRINUSE) and an exclusive wildcard (WSAEACCES). What the option ADDS is
//! not shown: with it planted out, a same-account `SO_REUSEADDR` bind on the address was still
//! refused (the Dell, 14 Sep 2026) — Windows' default already does that; another account's socket was
//! not tried.
//!
//! **Connection resets off** — on Windows a UDP socket whose earlier datagram drew an ICMP
//! port-unreachable reports WSAECONNRESET on a later receive; for a server that is a client that gave
//! up before its answer. `SIO_UDP_CONNRESET = FALSE` turns the report off. Kept as a guard, NOT as a
//! measured need: `windows_dns_agent_check`'s twenty vanishing clients left the agent answering with
//! the ioctl planted out too (the Dell, 14 Sep 2026) — hickory's receive loop survived the reports, or
//! none arrived.
//!
//! **Uninheritable** — opened through `winsock::uninheritable_socket`, never `socket()`: a child the
//! app spawned while it served in-process kept `:53` bound after the app let go (ledger #850).

use std::io;
use std::net::{Ipv4Addr, UdpSocket};
use std::os::windows::io::FromRawSocket;
use windows_sys::Win32::Networking::WinSock::{
    bind, closesocket, htons, setsockopt, WSAGetLastError, WSAIoctl, AF_INET, IPPROTO_UDP, SIO_UDP_CONNRESET, SOCKADDR,
    SOCKADDR_IN, SOCK_DGRAM, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
};

pub(crate) fn bind_resolver_udp(port: u16) -> io::Result<UdpSocket> {
    // SAFETY: plain Winsock calls on values owned by this frame; the socket is closed on every
    // failure path and handed to std exactly once on success.
    unsafe {
        // Uninheritable: a child spawned while this is open must not keep `:53` bound after the
        // app closes it (ledger #850).
        let s = super::winsock::uninheritable_socket(AF_INET as i32, SOCK_DGRAM, IPPROTO_UDP)?;
        let fail = || {
            let code = WSAGetLastError();
            closesocket(s);
            io::Error::from_raw_os_error(code)
        };
        let one: i32 = 1;
        if setsockopt(s, SOL_SOCKET, SO_EXCLUSIVEADDRUSE, (&one as *const i32).cast(), 4) != 0 {
            return Err(fail());
        }
        let off: u32 = 0;
        let mut returned: u32 = 0;
        if WSAIoctl(
            s,
            SIO_UDP_CONNRESET,
            (&off as *const u32).cast(),
            4,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
            None,
        ) != 0
        {
            return Err(fail());
        }
        let mut addr: SOCKADDR_IN = std::mem::zeroed();
        addr.sin_family = AF_INET;
        addr.sin_port = htons(port);
        addr.sin_addr.S_un.S_addr = u32::from_ne_bytes(Ipv4Addr::LOCALHOST.octets());
        if bind(s, (&addr as *const SOCKADDR_IN).cast::<SOCKADDR>(), std::mem::size_of::<SOCKADDR_IN>() as i32) != 0 {
            return Err(fail());
        }
        Ok(UdpSocket::from_raw_socket(s as u64))
    }
}
