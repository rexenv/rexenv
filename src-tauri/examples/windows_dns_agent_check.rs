//! W6 S1, ledger #615: rexenv's resolver on Windows serves `127.0.0.1:53` — the DNS agent process
//! (`core::dns::run_agent`, what `rexenv --dns-agent` runs) and the in-process fallback
//! (`DnsService::start_default`) — beside the holders a real Windows machine has, and refuses the
//! ones that really block it by name.
//!
//! ```text
//! scripts/probes/windows-example.sh <host> windows_dns_agent_check
//! ```
//!
//! 1. `DEFAULT_DNS_PORT` is 53 here; what already holds UDP :53 is printed (on the Dell: ICS's
//!    `0.0.0.0:53`, there since WSL 2 was installed — plan §3 D2).
//! 2. The agent, as a SEPARATE process (this example re-run as `agent`, calling `run_agent`):
//!    `answers_as_ours(53)`, its build identity is this build, and Windows' own resolver
//!    (`Resolve-DnsName x.rex -Server 127.0.0.1`) gets `127.0.0.1`.
//! 3. While the agent holds :53, a `SO_REUSEADDR` bind on `127.0.0.1:53` is refused — the address
//!    cannot be shared. NOT a proof of `SO_EXCLUSIVEADDRUSE`: with the option planted out this still
//!    held (Windows' default refuses a same-account reuse bind).
//! 4. Clients that vanish before their answer — a query sent from a socket closed at once, twenty
//!    times — do not stop it answering. A regression check of behaviour, NOT a proof of
//!    `SIO_UDP_CONNRESET`: it held with the ioctl planted out too.
//! 5. While the agent holds :53, the in-process `start_default` is refused, naming the port, the DNS
//!    resolver and THE AGENT'S pid — never ICS's `0.0.0.0:53` socket, which does not block it (the
//!    first run named `SharedAccess` and offered `Stop-Service SharedAccess` as the fix, and this check
//!    passed it: it looked only for "53" and "DNS resolver").
//! 6. With the agent gone and a plain `127.0.0.1:53` holder planted, `start_default` is refused the
//!    same way, naming THIS process's pid; with the holder gone it starts and answers.
//!
//! Fixture-owned: the agent child is killed with its process on drop; the planted holder is a socket
//! of this process. Run with no rexenv resolver on the machine (checked first). `demo` tier:
//! Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_dns_agent_check: skipped — a Windows check (ledger #615, W6)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("agent") {
        std::process::exit(rexenv_lib::core::dns::run_agent());
    }
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use rexenv_lib::core::dns;
    use std::net::{Ipv4Addr, UdpSocket};
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::time::{Duration, Instant};

    struct Agent(Child);

    impl Drop for Agent {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn wait(mut ok: impl FnMut() -> bool, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        ok()
    }

    /// Bind UDP `127.0.0.1:53` with `SO_REUSEADDR` from THIS process; the Winsock error on refusal.
    fn reuse_bind_127_53() -> Result<(), i32> {
        use windows_sys::Win32::Networking::WinSock::{
            bind, closesocket, htons, setsockopt, socket, WSAGetLastError, WSAStartup, AF_INET, INVALID_SOCKET, IPPROTO_UDP,
            SOCKADDR, SOCKADDR_IN, SOCK_DGRAM, SOL_SOCKET, SO_REUSEADDR, WSADATA,
        };
        // SAFETY: plain Winsock calls on values owned by this frame; the socket is closed on every path.
        unsafe {
            let mut wsa: WSADATA = std::mem::zeroed();
            WSAStartup(0x0202, &mut wsa);
            let s = socket(AF_INET as i32, SOCK_DGRAM, IPPROTO_UDP);
            if s == INVALID_SOCKET {
                return Err(WSAGetLastError());
            }
            let one: i32 = 1;
            setsockopt(s, SOL_SOCKET, SO_REUSEADDR, (&one as *const i32).cast(), 4);
            let mut addr: SOCKADDR_IN = std::mem::zeroed();
            addr.sin_family = AF_INET;
            addr.sin_port = htons(53);
            addr.sin_addr.S_un.S_addr = u32::from_ne_bytes(Ipv4Addr::LOCALHOST.octets());
            let rc = bind(s, (&addr as *const SOCKADDR_IN).cast::<SOCKADDR>(), std::mem::size_of::<SOCKADDR_IN>() as i32);
            let err = WSAGetLastError();
            closesocket(s);
            if rc == 0 { Ok(()) } else { Err(err) }
        }
    }

    fn resolve() -> String {
        let script = "try { (Resolve-DnsName check.rex -Server 127.0.0.1 -DnsOnly -QuickTimeout -Type A -ErrorAction Stop | Where-Object Type -eq 'A').IPAddress } catch { 'ERR ' + $_.Exception.Message }";
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| format!("ERR {e}"))
    }

    fn udp53_rows() -> String {
        let script = "Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue | ForEach-Object { '{0}:53 pid {1} {2}' -f $_.LocalAddress, $_.OwningProcess, (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName }";
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().replace("\r\n", "; "))
            .unwrap_or_default()
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_dns_agent_check");
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let plat = rexenv_lib::platform::current();

        // ── 1. The port, and who is already there. ──
        check.is("DEFAULT_DNS_PORT is 53 on Windows", dns::DEFAULT_DNS_PORT == 53, &dns::DEFAULT_DNS_PORT.to_string());
        let rows = udp53_rows();
        println!("  · UDP :53 before: {}", if rows.is_empty() { "nobody" } else { &rows });
        if dns::answers_as_ours(53) {
            check.is("no rexenv resolver is already answering on 127.0.0.1:53", false, "one answers — stop it first");
            return check.verdict();
        }

        // ── 2. The agent process. ──
        let exe = std::env::current_exe().expect("exe");
        let agent = Command::new(&exe).arg("agent").stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        let Ok(child) = agent else {
            check.is("the agent process starts", false, &format!("{:?}", agent.err()));
            return check.verdict();
        };
        let agent_pid = child.id();
        let agent = Agent(child);
        let up = wait(|| dns::answers_as_ours(53), Duration::from_secs(10));
        check.is("the agent answers on 127.0.0.1:53 (answers_as_ours)", up, "no answer within 10 s");
        println!("  · UDP :53 with the agent: {}", udp53_rows());
        let identity = dns::agent_build_identity(53);
        check.is("the agent names this build", identity.as_deref() == Some(dns::build_identity().as_str()), &format!("{identity:?}"));
        let answer = resolve();
        check.is("Windows' resolver gets 127.0.0.1 for check.rex from 127.0.0.1:53", answer == "127.0.0.1", &answer);

        // ── 2b. Exclusive: a reuse-address bind under the agent is refused. ──
        let hijack = reuse_bind_127_53();
        println!("  · SO_REUSEADDR bind on 127.0.0.1:53 while the agent holds it: {hijack:?}");
        check.is("a SO_REUSEADDR bind on 127.0.0.1:53 is refused while the agent holds it", hijack.is_err(), "it bound — the agent's address can be shared");

        // ── 3. Vanishing clients. ──
        for _ in 0..20 {
            if let Ok(s) = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)) {
                // A syntactically valid query; the socket is dropped before any answer can arrive.
                let q = [0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 5, b'g', b'h', b'o', b's', b't', 3, b'r', b'e', b'x', 0, 0, 1, 0, 1];
                let _ = s.send_to(&q, (Ipv4Addr::LOCALHOST, 53));
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        check.is("after 20 clients vanished before their answers, the agent still answers", dns::answers_as_ours(53), "it stopped answering");

        // ── 4. The in-process fallback while the agent holds :53. ──
        let refused = rt.block_on(dns::DnsService::start_default(plat.as_ref()));
        match &refused {
            Ok(_) => check.is("start_default is refused while the agent holds :53", false, "it started"),
            Err(e) => {
                let m = e.to_string();
                println!("  · refusal while the agent holds :53: {m}");
                check.is("start_default is refused while the agent holds :53, naming the port and the resolver", m.contains("53") && m.contains("DNS resolver"), &m);
                check.is("the refusal names the agent's pid (the process that really holds 127.0.0.1:53)", m.contains(&format!("pid {agent_pid}")), &m);
                check.is("the refusal does not name ICS's wildcard socket or offer to stop SharedAccess", !m.contains("SharedAccess"), &m);
            }
        }
        drop(refused);

        // ── 5. A planted loopback holder, then none. ──
        drop(agent);
        let gone = wait(|| !dns::answers_as_ours(53), Duration::from_secs(10));
        check.is("the agent is gone", gone, "still answering");
        let holder = UdpSocket::bind((Ipv4Addr::LOCALHOST, 53));
        check.is("a plain 127.0.0.1:53 holder can be planted", holder.is_ok(), &format!("{:?}", holder.as_ref().err()));
        if holder.is_ok() {
            match rt.block_on(dns::DnsService::start_default(plat.as_ref())) {
                Ok(_) => check.is("start_default is refused beside a 127.0.0.1:53 holder", false, "it started"),
                Err(e) => {
                    let m = e.to_string();
                    println!("  · refusal beside a loopback holder: {m}");
                    check.is("start_default is refused beside a 127.0.0.1:53 holder, naming the port and the resolver", m.contains("53") && m.contains("DNS resolver"), &m);
                    check.is("the refusal names this process's pid (the loopback holder)", m.contains(&format!("pid {}", std::process::id())), &m);
                    check.is("the refusal does not name ICS's wildcard socket", !m.contains("SharedAccess"), &m);
                }
            }
        }
        drop(holder);
        let started = rt.block_on(dns::DnsService::start_default(plat.as_ref()));
        check.is("with the holder gone, start_default starts", started.is_ok(), &format!("{:?}", started.as_ref().err()));
        if let Ok(service) = started {
            let answered = wait(|| dns::answers_as_ours(53), Duration::from_secs(5));
            check.is("the in-process resolver answers on 127.0.0.1:53", answered, "no answer");
            println!("  · UDP :53 with the in-process resolver: {}", udp53_rows());
            rt.block_on(service.shutdown());
        }
        check.verdict()
    }
}
