//! W3's "Done when", on a real Windows machine (ledger #600, docs/PLAN-windows-port.md
//! W3): MySQL and Mailpit start through `core`, OUTLIVE the process that started them —
//! and the job it ran in — and the next process ADOPTS them and stops them cleanly.
//!
//! Two phases, two processes, two SSH sessions. An SSH session on Windows runs inside a
//! kill-on-close job (measured on the Dell, 13 Sep 2026), so the end of phase 1's session
//! is a harsher "the app quit" than a plain exit: anything not broken away from that job
//! dies with it.
//!
//! ```text
//! ssh dell@<host> .\windows_supervision_check.exe phase1   # start both, exit WITHOUT stopping
//! ssh dell@<host> .\windows_supervision_check.exe phase2   # alive? adopt, stop cleanly, clean up
//! ```
//!
//! **Phase 1's ssh must RETURN by itself.** Its first two runs did not: the services had
//! inherited handles the check process was born with from sshd, and held the session open
//! until phase 2 stopped them (`platform/windows/handles.rs`). A phase 1 that hangs after
//! printing PASS is that defect back, whatever its checks say.
//!
//! Fixture-owned: a sandboxed platform rooted at `%TEMP%\rexenv-supervision-check` (removed by
//! phase 2), rexenv's fixed ports on a machine with no rexenv stack (`require_stack_stopped`
//! first), the real binary cache (the documented `sandbox` exception). Classified `demo`:
//! it takes a phase argument and needs a Windows host; on macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_supervision_check: skipped — a two-phase Windows live check (docs/PLAN-windows-port.md W3)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check};
    use rexenv_lib::core::ports::{self, Proto};
    use rexenv_lib::core::service_manager::ServiceManager;
    use rexenv_lib::core::{binaries, database, mail};
    use std::io::Read;
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::time::Duration;

    fn root() -> PathBuf {
        std::env::temp_dir().join("rexenv-supervision-check")
    }

    pub async fn main() -> ExitCode {
        match std::env::args().nth(1).as_deref() {
            Some("phase1") => phase1().await,
            Some("phase2") => phase2(),
            other => {
                eprintln!("usage: windows_supervision_check phase1|phase2 (got {other:?})");
                ExitCode::FAILURE
            }
        }
    }

    /// Start MySQL and Mailpit the way the app does, then leave WITHOUT stopping them.
    async fn phase1() -> ExitCode {
        common::require_stack_stopped();
        let mut check = Check::new("windows_supervision_check phase1");
        let root = root();
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());

        let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION)
            .await
            .expect("resolve mysql on Windows");
        let mailpit = binaries::resolve(&*plat, "mailpit", binaries::MAILPIT_VERSION)
            .await
            .expect("resolve mailpit on Windows");
        println!("  · mysql {} · mailpit {}", basedir.display(), mailpit.display());

        let datadir = database::data_dir(&*plat).unwrap();
        let socket = database::socket_path(&*plat).unwrap();
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        database::initialize(&*plat, &basedir, &datadir).expect("mysqld --initialize-insecure");

        let mysqld = database::start(&*plat, &basedir, &datadir, database::MYSQL_PORT, &socket)
            .expect("start mysqld");
        let mailpitd = mail::start(&*plat, &mailpit).expect("start mailpit");
        let (mysql_pid, mailpit_pid) = (mysqld.id(), mailpitd.id());
        common::await_listening(database::MYSQL_PORT, "mysqld", Some(&plat.paths().log_dir().unwrap().join("mysql-error.log")));
        common::await_listening(mail::MAILPIT_HTTP_PORT, "mailpit", None);

        let sup = plat.supervisor();
        let under_basedir: Vec<u32> = sup
            .pids_named("mysqld")
            .into_iter()
            .filter(|&pid| sup.pid_exe(pid).is_some_and(|exe| exe.starts_with(&basedir)))
            .collect();
        check.is(
            "mysqld is ONE process (--no-monitor): the pid rexenv holds is the listener",
            under_basedir == vec![mysql_pid]
                && sup.port_holders(database::MYSQL_PORT, false).unwrap_or_default() == vec![mysql_pid],
            &format!("mysqld processes {under_basedir:?}, spawned {mysql_pid}, listeners {:?}", sup.port_holders(database::MYSQL_PORT, false)),
        );
        let cmd = sup.pid_command(mysql_pid).unwrap_or_default();
        check.is("mysqld's command line carries --no-monitor", cmd.contains("--no-monitor"), &cmd);

        std::fs::write(root.join("pids.txt"), format!("{mysql_pid} {mailpit_pid}")).unwrap();
        println!("  · started mysqld {mysql_pid} and mailpit {mailpit_pid}; leaving them running");
        // The app quitting: the `Child` handles are dropped, which on Windows neither kills
        // nor waits, and this session's job closes after the process exits.
        drop((mysqld, mailpitd));
        check.verdict()
    }

    /// A new process in a new session: the services must still be there, be adopted as
    /// ours, and stop cleanly.
    fn phase2() -> ExitCode {
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_supervision_check phase2");
        let root = root();
        let Ok(pids) = std::fs::read_to_string(root.join("pids.txt")) else {
            eprintln!("no phase-1 record under {} — run phase1 first", root.display());
            return ExitCode::FAILURE;
        };
        let mut it = pids.split_whitespace().filter_map(|p| p.parse::<u32>().ok());
        let (mysql_pid, mailpit_pid) = (it.next().unwrap(), it.next().unwrap());
        let plat = common::sandbox_platform_at(root.clone());
        let sup = plat.supervisor();
        let marker = plat.paths().app_data_dir().unwrap().display().to_string();

        check.is("mysqld outlived phase 1 and its session", sup.pid_alive(mysql_pid), "gone");
        check.is("mailpit outlived phase 1 and its session", sup.pid_alive(mailpit_pid), "gone");
        let greeting = mysql_greeting(database::MYSQL_PORT);
        check.is(
            "MySQL still answers with its own handshake",
            greeting.contains(binaries::MYSQL_VERSION),
            &format!("{greeting:?}"),
        );

        let mut mgr = ServiceManager::default();
        let adopted = mgr.adopt_startup(&*plat, &[], true);
        check.is("adopt_startup adopts both", adopted >= 2, &format!("adopted {adopted}"));
        check.is(
            "the adopted MySQL is the pid phase 1 spawned",
            sup.owned_master(database::MYSQL_PORT, &marker) == Some(mysql_pid),
            &format!("{:?}", sup.owned_master(database::MYSQL_PORT, &marker)),
        );
        check.is(
            "the adopted Mailpit is the pid phase 1 spawned",
            sup.owned_master(mail::MAILPIT_SMTP_PORT, &marker) == Some(mailpit_pid),
            &format!("{:?}", sup.owned_master(mail::MAILPIT_SMTP_PORT, &marker)),
        );

        let log = plat.paths().log_dir().unwrap().join("mysql-error.log");
        let started = std::time::Instant::now();
        let stopped = database::stop(&*plat, mysql_pid);
        let took = started.elapsed();
        let tail = std::fs::read_to_string(&log).unwrap_or_default();
        check.is(&format!("database::stop returned Ok ({} ms)", took.as_millis()), stopped.is_ok(), &format!("{stopped:?}"));
        check.is(
            "MySQL shut down CLEANLY through its event, not TerminateProcess",
            tail.contains("Normal shutdown") && tail.contains("Shutdown complete"),
            &tail.lines().rev().take(4).collect::<Vec<_>>().join(" | "),
        );
        check.is(
            "the MySQL port is free afterwards",
            ports::wait_free(&*plat, database::MYSQL_PORT, Proto::Tcp, 30, Duration::from_millis(100)),
            "still held",
        );

        let stopped = mail::stop(&*plat, mailpit_pid);
        check.is("mail::stop returned Ok", stopped.is_ok(), &format!("{stopped:?}"));
        check.is("mailpit is gone", !sup.pid_alive(mailpit_pid), "alive");
        check.is(
            "the Mailpit ports are free afterwards",
            ports::wait_free(&*plat, mail::MAILPIT_HTTP_PORT, Proto::Tcp, 30, Duration::from_millis(100))
                && ports::wait_free(&*plat, mail::MAILPIT_SMTP_PORT, Proto::Tcp, 30, Duration::from_millis(100)),
            "still held",
        );
        check.is("stopping a pid that is already gone is Ok", sup.stop(mysql_pid).is_ok(), "errored");

        match std::fs::remove_dir_all(&root) {
            Ok(()) => println!("  · removed {}", root.display()),
            Err(e) => println!("  · could not remove {}: {e}", root.display()),
        }
        check.verdict()
    }

    /// The first bytes MySQL sends on connect: protocol 10, then the server version.
    fn mysql_greeting(port: u16) -> String {
        let Ok(mut s) = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_secs(2)) else {
            return "no connection".into();
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = [0u8; 128];
        let n = s.read(&mut buf).unwrap_or(0);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}
