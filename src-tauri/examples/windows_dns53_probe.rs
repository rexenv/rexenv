//! W6's D2 measurement (plan §3 D2): on Windows, when something already holds port 53, can rexenv's
//! DNS agent still bind `127.0.0.1:53` with `SO_EXCLUSIVEADDRUSE` — and when both are bound, WHO
//! ANSWERS a query sent to `127.0.0.1`, over UDP and TCP?
//!
//! ```text
//! scripts/probes/windows-example.sh <host> windows_dns53_probe
//! ```
//!
//! A PROBE, not a proof: it prints what each case did. The plan's §6 matrix measured "a `127.0.0.1`
//! bind wins loopback traffic over a wildcard holder" for TCP answers, and only that UDP bound; DNS is
//! UDP first, and a real :53 holder (ICS's DNS proxy, WSL's, Docker's) is somebody else's process.
//! So each holder here is a SEPARATE process — this example re-run as `hold <addr> <exclusive>
//! <dualstack> <answer-ip>` — serving DNS on UDP and TCP and answering every A query with its own
//! marker address; the answer's address names the process that replied.
//!
//! Cases (holders started in the order listed; "agent" = `127.0.0.1`, exclusive, the shape W6 will
//! build): the agent alone; a `0.0.0.0` holder then the agent; the agent then a `0.0.0.0` holder;
//! a `[::]` IPv6-only holder then the agent; a `[::]` dual-stack holder then the agent; a
//! `127.0.0.1` holder then the agent; an exclusive `0.0.0.0` holder then the agent. Queries go
//! through Windows' own resolver: `Resolve-DnsName probe.rex -Server 127.0.0.1 -DnsOnly [-TcpOnly]`.
//!
//! Every holder runs as the same account as the example — a holder that is another account (a
//! LocalSystem service) is the real-state runs' business. Fixture-owned: every holder process is
//! killed at the end of its case; :53 is checked free first. `demo` tier: Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_dns53_probe: skipped — a Windows probe (plan §3 D2, W6)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("hold") {
        windows::hold(&args[2..]);
        return std::process::ExitCode::SUCCESS;
    }
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream, UdpSocket};
    use std::os::windows::io::FromRawSocket;
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_sys::Win32::Networking::WinSock::{
        bind, closesocket, htons, listen, setsockopt, socket, WSAGetLastError, WSAStartup, AF_INET, AF_INET6,
        INVALID_SOCKET, IPPROTO_IPV6, IPPROTO_TCP, IPPROTO_UDP, IPV6_V6ONLY, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
        SOCK_DGRAM, SOCK_STREAM, SOL_SOCKET, SO_EXCLUSIVEADDRUSE, WSADATA,
    };

    /// A socket bound to `addr:53` (listening, for TCP), or the Winsock error of the step that failed.
    fn bind53(addr: IpAddr, tcp: bool, exclusive: bool, dualstack: bool) -> Result<u64, String> {
        // SAFETY: plain Winsock calls on values owned by this frame; the socket is closed on failure.
        unsafe {
            let mut wsa: WSADATA = std::mem::zeroed();
            WSAStartup(0x0202, &mut wsa);
            let af = if addr.is_ipv4() { AF_INET } else { AF_INET6 };
            let (ty, proto) = if tcp { (SOCK_STREAM, IPPROTO_TCP) } else { (SOCK_DGRAM, IPPROTO_UDP) };
            let s = socket(af as i32, ty, proto);
            if s == INVALID_SOCKET {
                return Err(format!("socket {}", WSAGetLastError()));
            }
            let fail = |step: &str| {
                let e = WSAGetLastError();
                closesocket(s);
                Err(format!("{step} {e}"))
            };
            let one: i32 = 1;
            let zero: i32 = 0;
            if exclusive && setsockopt(s, SOL_SOCKET, SO_EXCLUSIVEADDRUSE, (&one as *const i32).cast(), 4) != 0 {
                return fail("SO_EXCLUSIVEADDRUSE");
            }
            if addr.is_ipv6() && dualstack && setsockopt(s, IPPROTO_IPV6, IPV6_V6ONLY, (&zero as *const i32).cast(), 4) != 0 {
                return fail("IPV6_V6ONLY=0");
            }
            let rc = match addr {
                IpAddr::V4(a) => {
                    let mut sa: SOCKADDR_IN = std::mem::zeroed();
                    sa.sin_family = AF_INET;
                    sa.sin_port = htons(53);
                    sa.sin_addr.S_un.S_addr = u32::from_ne_bytes(a.octets());
                    bind(s, (&sa as *const SOCKADDR_IN).cast::<SOCKADDR>(), std::mem::size_of::<SOCKADDR_IN>() as i32)
                }
                IpAddr::V6(a) => {
                    let mut sa: SOCKADDR_IN6 = std::mem::zeroed();
                    sa.sin6_family = AF_INET6;
                    sa.sin6_port = htons(53);
                    sa.sin6_addr.u.Byte = a.octets();
                    bind(s, (&sa as *const SOCKADDR_IN6).cast::<SOCKADDR>(), std::mem::size_of::<SOCKADDR_IN6>() as i32)
                }
            };
            if rc != 0 {
                return fail("bind");
            }
            if tcp && listen(s, 16) != 0 {
                return fail("listen");
            }
            Ok(s as u64)
        }
    }

    /// A DNS response to `query` answering its question with one A record, `answer`, TTL 0.
    fn respond(query: &[u8], answer: Ipv4Addr) -> Option<Vec<u8>> {
        if query.len() < 12 {
            return None;
        }
        let mut i = 12;
        while i < query.len() && query[i] != 0 {
            i += 1 + query[i] as usize;
        }
        let end = i + 5; // the zero label, QTYPE, QCLASS
        if end > query.len() {
            return None;
        }
        let mut r = Vec::with_capacity(end + 16);
        r.extend_from_slice(&query[0..2]);
        r.extend_from_slice(&[0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
        r.extend_from_slice(&query[12..end]);
        r.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 0, 0, 4]);
        r.extend_from_slice(&answer.octets());
        Some(r)
    }

    /// `hold <addr> <exclusive 0|1> <dualstack 0|1> <answer-ip>`: bind UDP and TCP, report, serve.
    pub fn hold(args: &[String]) {
        let addr: IpAddr = args[0].parse().expect("addr");
        let exclusive = args[1] == "1";
        let dualstack = args[2] == "1";
        let answer: Ipv4Addr = args[3].parse().expect("answer");
        let udp = bind53(addr, false, exclusive, dualstack);
        let tcp = bind53(addr, true, exclusive, dualstack);
        let show = |r: &Result<u64, String>| r.as_ref().map(|_| "ok".to_string()).unwrap_or_else(|e| e.clone());
        println!("BOUND udp={} tcp={}", show(&udp), show(&tcp));
        let _ = std::io::stdout().flush();
        if let Ok(raw) = udp {
            // SAFETY: a bound socket this process owns, handed to std exactly once.
            let sock = unsafe { UdpSocket::from_raw_socket(raw) };
            std::thread::spawn(move || {
                let mut buf = [0u8; 1500];
                while let Ok((n, from)) = sock.recv_from(&mut buf) {
                    if let Some(r) = respond(&buf[..n], answer) {
                        let _ = sock.send_to(&r, from);
                    }
                }
            });
        }
        if let Ok(raw) = tcp {
            // SAFETY: as above.
            let listener = unsafe { TcpListener::from_raw_socket(raw) };
            std::thread::spawn(move || {
                for mut conn in listener.incoming().flatten() {
                    let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut len = [0u8; 2];
                    if conn.read_exact(&mut len).is_err() {
                        continue;
                    }
                    let mut q = vec![0u8; u16::from_be_bytes(len) as usize];
                    if conn.read_exact(&mut q).is_err() {
                        continue;
                    }
                    if let Some(r) = respond(&q, answer) {
                        let _ = conn.write_all(&(r.len() as u16).to_be_bytes());
                        let _ = conn.write_all(&r);
                    }
                }
            });
        }
        // Serve until the parent kills this process (or two minutes, whichever is first).
        std::thread::sleep(Duration::from_secs(120));
    }

    struct Holder {
        label: &'static str,
        child: Child,
    }

    impl Drop for Holder {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn start(label: &'static str, addr: &str, exclusive: bool, dualstack: bool, answer: &str) -> Option<Holder> {
        let exe = std::env::current_exe().ok()?;
        let mut child = Command::new(exe)
            .args(["hold", addr, if exclusive { "1" } else { "0" }, if dualstack { "1" } else { "0" }, answer])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(out).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| "(no report)".into());
        println!(
            "    {label:<22} {addr:<10} exclusive={exclusive:<5} dualstack={dualstack:<5} answers {answer:<12} {}",
            line.trim()
        );
        Some(Holder { label, child })
    }

    /// The address Windows' resolver got back from `127.0.0.1`, or its error.
    fn resolve(tcp: bool) -> String {
        let script = format!(
            "try {{ (Resolve-DnsName probe.rex -Server 127.0.0.1 -DnsOnly -QuickTimeout -Type A {} -ErrorAction Stop | Where-Object Type -eq 'A').IPAddress }} catch {{ 'ERR ' + $_.Exception.Message }}",
            if tcp { "-TcpOnly" } else { "" }
        );
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| format!("ERR {e}"))
    }

    fn who(answer: &str, holders: &[Holder], markers: &[(&str, &str)]) -> String {
        match markers.iter().find(|(_, ip)| *ip == answer) {
            Some((label, _)) if holders.iter().any(|h| h.label == *label) => format!("{answer} = {label}"),
            _ => answer.to_string(),
        }
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_dns53_probe");
        let free = UdpSocket::bind("127.0.0.1:53").is_ok()
            && TcpStream::connect_timeout(&([127, 0, 0, 1], 53).into(), Duration::from_millis(300)).is_err();
        check.is("127.0.0.1:53 is free before the probe (UDP bind, no TCP answer)", free, "something holds it");
        if !free {
            return check.verdict();
        }
        const AGENT: (&str, &str) = ("agent", "127.0.0.11");
        let markers: [(&str, &str); 7] = [
            AGENT,
            ("wildcard v4", "127.0.0.21"),
            ("wildcard v6-only", "127.0.0.31"),
            ("wildcard dual-stack", "127.0.0.41"),
            ("loopback holder", "127.0.0.51"),
            ("exclusive wildcard v4", "127.0.0.61"),
            ("wildcard v4 (after)", "127.0.0.71"),
        ];
        type Step = (&'static str, &'static str, bool, bool, &'static str);
        let agent: Step = ("agent", "127.0.0.1", true, false, "127.0.0.11");
        let cases: Vec<(&str, Vec<Step>)> = vec![
            ("the agent alone", vec![agent]),
            ("0.0.0.0 holder, then the agent", vec![("wildcard v4", "0.0.0.0", false, false, "127.0.0.21"), agent]),
            ("the agent, then a 0.0.0.0 holder", vec![agent, ("wildcard v4 (after)", "0.0.0.0", false, false, "127.0.0.71")]),
            ("[::] v6-only holder, then the agent", vec![("wildcard v6-only", "::", false, false, "127.0.0.31"), agent]),
            ("[::] dual-stack holder, then the agent", vec![("wildcard dual-stack", "::", false, true, "127.0.0.41"), agent]),
            ("127.0.0.1 holder, then the agent", vec![("loopback holder", "127.0.0.1", false, false, "127.0.0.51"), agent]),
            ("exclusive 0.0.0.0 holder, then the agent", vec![("exclusive wildcard v4", "0.0.0.0", true, false, "127.0.0.61"), agent]),
        ];
        let mut summary = Vec::new();
        for (name, steps) in cases {
            println!("  · case: {name}");
            let mut holders = Vec::new();
            for (label, addr, exclusive, dualstack, answer) in steps {
                if let Some(h) = start(label, addr, exclusive, dualstack, answer) {
                    holders.push(h);
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            let udp = who(&resolve(false), &holders, &markers);
            let tcp = who(&resolve(true), &holders, &markers);
            println!("    Resolve-DnsName via UDP -> {udp}");
            println!("    Resolve-DnsName via TCP -> {tcp}");
            if name == "the agent alone" {
                check.is("the agent alone answers over UDP (the probe can see an answer)", udp.ends_with("= agent"), &udp);
                check.is("the agent alone answers over TCP", tcp.ends_with("= agent"), &tcp);
            }
            summary.push(format!("{name:<42} UDP {udp:<40} TCP {tcp}"));
            drop(holders);
            std::thread::sleep(Duration::from_millis(500));
        }
        println!("  · summary:");
        for line in summary {
            println!("    {line}");
        }
        check.verdict()
    }
}
