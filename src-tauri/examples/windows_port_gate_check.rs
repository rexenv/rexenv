//! The Windows port gate and process identity, on a real Windows machine (ledger #599,
//! docs/PLAN-windows-port.md §6).
//!
//! Cross-build on the Mac, copy, run on the Dell:
//!
//! ```text
//! cd src-tauri && CARGO_TARGET_DIR=target/xwin cargo xwin build --example windows_port_gate_check \
//!   --target x86_64-pc-windows-msvc
//! scp target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_port_gate_check.exe dell@<host>:
//! ssh dell@<host> .\windows_port_gate_check.exe
//! ```
//!
//! What it proves there: a port another process holds on `0.0.0.0`, `[::]` or
//! `127.0.0.1` — TCP and UDP — is BUSY to `core::ports::is_free`, even where the §6
//! matrix says a trial bind alone would succeed; the conflict names the holder by pid
//! and image with a `Stop-Process` line; our own leftover (the app-data path on its
//! command line) is recognised case-insensitively and gets the leftover message; a
//! service host and the System process are named for what they are; the port reads
//! free again once the holder is gone.
//!
//! Fixture-owned: the holders are copies of this executable on throwaway ports it picked,
//! each killed on drop. It writes nothing. The "leftover" holder carries the real
//! app-data PATH as text on its command line and touches nothing under it.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_port_gate_check: skipped — a Windows live check (docs/PLAN-windows-port.md §6)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use rexenv_lib::core::ports::{self, Proto};
    use rexenv_lib::platform::traits::Platform;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, UdpSocket};
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::time::Duration;

    const MARKER: &str = "rexenv-port-gate-fixture-7f3a";

    /// A holder process, killed and reaped on drop.
    struct Holder(Child);
    impl Drop for Holder {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    pub fn main() -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("--hold") {
            return hold(&args[2..]);
        }
        let plat = rexenv_lib::platform::current();
        let mut check = Check::new("windows_port_gate_check");

        for (proto, addr) in [
            (Proto::Tcp, "0.0.0.0"),
            (Proto::Tcp, "[::]"),
            (Proto::Tcp, "127.0.0.1"),
            (Proto::Udp, "0.0.0.0"),
            (Proto::Udp, "127.0.0.1"),
        ] {
            held_port(&*plat, &mut check, proto, addr);
        }
        leftover(&*plat, &mut check);
        system_holders(&*plat, &mut check);
        excluded_range(&*plat, &mut check);

        let me = std::process::id();
        let stem = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let named = plat.supervisor().pids_named(&stem);
        check.is(
            "pids_named finds this process by its bare name",
            named.contains(&me),
            &format!("{stem} → {named:?}, me {me}"),
        );
        check.verdict()
    }

    /// Child mode: bind `proto` on `addr:port`, say `ready`, then wait to be killed.
    fn hold(args: &[String]) -> ExitCode {
        let [proto, addr, port, _marker] = args else {
            eprintln!("usage: --hold tcp|udp <addr> <port> <marker>");
            return ExitCode::FAILURE;
        };
        let at: SocketAddr = format!("{addr}:{port}").parse().expect("address");
        let _socket: Box<dyn std::any::Any> = match proto.as_str() {
            "tcp" => Box::new(TcpListener::bind(at).expect("tcp bind")),
            _ => Box::new(UdpSocket::bind(at).expect("udp bind")),
        };
        println!("ready");
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(300));
        ExitCode::SUCCESS
    }

    fn spawn_holder(proto: Proto, addr: &str, port: u16, marker: &str) -> Holder {
        let exe = std::env::current_exe().expect("current exe");
        let mut child = Command::new(exe)
            .args(["--hold", proto.as_str(), addr, &port.to_string(), marker])
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn holder");
        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("stdout")).read_line(&mut line).expect("ready line");
        assert_eq!(line.trim(), "ready", "the holder did not bind {addr}:{port}");
        Holder(child)
    }

    fn unused_port(proto: Proto) -> u16 {
        let local = (Ipv4Addr::LOCALHOST, 0);
        match proto {
            Proto::Tcp => TcpListener::bind(local).unwrap().local_addr().unwrap().port(),
            Proto::Udp => UdpSocket::bind(local).unwrap().local_addr().unwrap().port(),
        }
    }

    fn held_port(plat: &dyn Platform, check: &mut Check, proto: Proto, addr: &str) {
        let case = format!("{} {addr}", proto.as_str());
        let port = unused_port(proto);
        let holder = spawn_holder(proto, addr, port, MARKER);
        let pid = holder.0.id();
        let sup = plat.supervisor();

        // What the old gate would have said — printed, not asserted: the §6 matrix is
        // the measurement, this line only shows it again on the machine at hand.
        let local = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let bind_alone = match proto {
            Proto::Tcp => TcpListener::bind(local).is_ok(),
            Proto::Udp => UdpSocket::bind(local).is_ok(),
        };
        println!("  · {case}:{port} — a trial bind alone says free: {bind_alone}");

        check.is(&format!("{case}: is_free says busy"), !ports::is_free(plat, port, proto), "said free");
        let holders = sup.port_holders(port, matches!(proto, Proto::Udp)).unwrap_or_default();
        check.is(&format!("{case}: the tables list the holder"), holders.contains(&pid), &format!("{holders:?}, holder {pid}"));

        let err = ports::ensure_free(plat, port, proto, "fixture").unwrap_err().to_string();
        let last = err.lines().last().unwrap_or_default();
        check.is(
            &format!("{case}: the conflict names the holder's pid and image"),
            err.contains(&format!("pid {pid}")) && err.to_lowercase().contains("windows_port_gate_check.exe"),
            &err,
        );
        check.is(
            &format!("{case}: the last line stops it with Stop-Process"),
            last == format!("$ Stop-Process -Id {pid}"),
            &err,
        );

        let command = sup.pid_command(pid).unwrap_or_default();
        check.is(&format!("{case}: pid_command carries the marker"), command.contains(MARKER), &command);
        let exe = sup.pid_exe(pid).map(|p| p.display().to_string().to_lowercase());
        let me = std::env::current_exe().ok().map(|p| p.display().to_string().to_lowercase());
        check.is(&format!("{case}: pid_exe is this executable"), exe.is_some() && exe == me, &format!("{exe:?} vs {me:?}"));
        check.is(&format!("{case}: pid_alive while it runs"), sup.pid_alive(pid), "not alive");

        if matches!(proto, Proto::Tcp) {
            check.is(
                &format!("{case}: owned_listeners finds it by marker, any case"),
                sup.owned_listeners(port, MARKER) == vec![pid]
                    && sup.owned_listeners(port, &MARKER.to_uppercase()) == vec![pid],
                &format!("{:?}", sup.owned_listeners(port, MARKER)),
            );
            check.is(
                &format!("{case}: owned_master is the holder"),
                sup.owned_master(port, MARKER) == Some(pid),
                &format!("{:?}", sup.owned_master(port, MARKER)),
            );
            check.is(
                &format!("{case}: a marker nobody carries owns nothing"),
                sup.owned_listeners(port, "not-a-marker-anyone-has").is_empty(),
                "matched",
            );
        }
        check.is(
            &format!("{case}: owned_pids finds it by marker"),
            sup.owned_pids(MARKER).contains(&pid),
            &format!("{:?}", sup.owned_pids(MARKER)),
        );

        drop(holder);
        std::thread::sleep(Duration::from_millis(200));
        check.is(&format!("{case}: pid_alive is false once it is gone"), !sup.pid_alive(pid), "still alive");
        check.is(
            &format!("{case}: the port reads free again"),
            ports::wait_free(plat, port, proto, 20, Duration::from_millis(100)),
            "still busy",
        );
    }

    /// A holder carrying the real app-data path on its command line, in another case, is
    /// our leftover: the message says so and suggests Stop-Process, never a stranger's help.
    fn leftover(plat: &dyn Platform, check: &mut Check) {
        let Ok(data) = plat.paths().app_data_dir() else {
            check.is("leftover: app data resolves", false, "no app data dir");
            return;
        };
        let marker = data.display().to_string().to_uppercase();
        let port = unused_port(Proto::Tcp);
        let holder = spawn_holder(Proto::Tcp, "0.0.0.0", port, &marker);
        let pid = holder.0.id();
        let err = ports::ensure_free(plat, port, Proto::Tcp, "fixture").unwrap_err().to_string();
        check.is(
            "leftover: an upper-cased app-data path is still ours",
            err.contains(&format!("leftover rexenv process (pid {pid})"))
                && err.lines().last() == Some(format!("$ Stop-Process -Id {pid}").as_str()),
            &err,
        );
    }

    /// The RPC endpoint mapper (svchost, port 135) and SMB (System, port 445) listen on a
    /// stock Windows install; each is named for what it is when present.
    fn system_holders(plat: &dyn Platform, check: &mut Check) {
        let sup = plat.supervisor();
        if sup.port_holders(135, false).is_some_and(|h| !h.is_empty()) {
            let help = sup.port_conflict_help(135, false);
            let holder = help.holder.unwrap_or_default();
            check.is(
                "tcp 135: named as a Windows service in svchost",
                holder.starts_with("the Windows service") && holder.contains("svchost.exe"),
                &holder,
            );
            println!("  · tcp 135 holder: {holder}");
        } else {
            println!("  · tcp 135: no listener on this machine — skipped");
        }
        if sup.port_holders(445, false).is_some_and(|h| h.contains(&4)) {
            let help = sup.port_conflict_help(445, false);
            let holder = help.holder.unwrap_or_default();
            check.is(
                "tcp 445: named as the System process, with no command pretending to free it",
                holder.contains("System process, pid 4") && help.free_command.is_none(),
                &holder,
            );
        } else {
            println!("  · tcp 445: not held by pid 4 on this machine — skipped");
        }
    }

    /// A port inside an excluded range has no holder in any table, so the gate's answer
    /// there is the trial bind's: busy exactly when the bind is refused. The first run
    /// on the Dell (13 Sep 2026) asserted "busy" and failed — its administered range
    /// 50000–50059 refuses no bind. Where a range does refuse, the conflict names it.
    /// Read-only: only a range the machine already has is used.
    fn excluded_range(plat: &dyn Platform, check: &mut Check) {
        let out = Command::new("netsh")
            .args(["interface", "ipv4", "show", "excludedportrange", "protocol=tcp"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        print!("  · netsh excluded ranges (tcp):");
        for l in out.lines().filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())) {
            print!(" [{}]", l.trim());
        }
        println!();
        let first = out.lines().find_map(|l| {
            let mut w = l.split_whitespace();
            let start: u16 = w.next()?.parse().ok()?;
            let _end: u16 = w.next()?.parse().ok()?;
            Some(start)
        });
        let Some(port) = first else {
            println!("  · no excluded TCP range on this machine — skipped");
            return;
        };
        let sup = plat.supervisor();
        if sup.port_holders(port, false).is_some_and(|h| !h.is_empty()) {
            println!("  · excluded port {port} also has a listener — skipped");
            return;
        }
        let bind_ok = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).is_ok();
        println!("  · excluded {port}: a 127.0.0.1 bind there succeeds: {bind_ok}");
        let free = ports::is_free(plat, port, Proto::Tcp);
        check.is(
            &format!("excluded {port}: with no holder, is_free agrees with the bind"),
            free == bind_ok,
            &format!("is_free {free}, bind {bind_ok}"),
        );
        if !bind_ok {
            let help = sup.port_conflict_help(port, false);
            let holder = help.holder.unwrap_or_default();
            check.is(&format!("excluded {port}: a refused port is named as an excluded range"), holder.contains("excluded"), &holder);
        }
    }
}
