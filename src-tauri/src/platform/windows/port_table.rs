//! Pure reads for the Windows port gate (ledger #599): the socket tables
//! `GetExtendedTcpTable` / `GetExtendedUdpTable` fill, `netsh`'s excluded port ranges,
//! and the words a conflict uses to name a holder.
//!
//! No Win32 calls live here, so the file is also compiled into the macOS test build
//! (`platform/mod.rs`) and every rule below runs in `verify.sh`, not only on a Windows
//! machine. `process.rs` does the calls and hands the bytes and strings to this file.
//!
//! # Why the port gate reads tables at all
//!
//! Measured on the Dell 13 Sep 2026 (plan §6, 288 binds under both tokens): a trial
//! bind on `127.0.0.1` with default options SUCCEEDS while another process holds the
//! same port on `0.0.0.0` or `[::]`, and once rexenv binds, the localhost traffic goes
//! to rexenv. A trial bind therefore reports "free" for exactly the holder a developer
//! is most likely to have — their own MySQL on all interfaces — and then steals its
//! clients. The tables list every local address, so a holder there is a holder.

/// One of the four owner-pid tables. Every field in every row is a `u32` or a byte
/// array, so rows sit at 4-byte offsets right after the `dwNumEntries` count, with no
/// padding (`MIB_*ROW_OWNER_PID` in `iphlpapi.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Table {
    /// `MIB_TCPROW_OWNER_PID`, requested as `TCP_TABLE_OWNER_PID_LISTENER`, so every
    /// row is a LISTEN socket — never a client connection or a TIME_WAIT leftover.
    Tcp4,
    /// `MIB_TCP6ROW_OWNER_PID`, same class.
    Tcp6,
    /// `MIB_UDPROW_OWNER_PID` — UDP has no states; a row is a bound endpoint.
    Udp4,
    /// `MIB_UDP6ROW_OWNER_PID`.
    Udp6,
}

impl Table {
    /// (row length, offset of `dwLocalPort`, offset of `dwOwningPid`) in bytes.
    fn layout(self) -> (usize, usize, usize) {
        match self {
            // dwState, dwLocalAddr, dwLocalPort, dwRemoteAddr, dwRemotePort, dwOwningPid
            Table::Tcp4 => (24, 8, 20),
            // ucLocalAddr[16], dwLocalScopeId, dwLocalPort, ucRemoteAddr[16],
            // dwRemoteScopeId, dwRemotePort, dwState, dwOwningPid
            Table::Tcp6 => (56, 20, 52),
            // dwLocalAddr, dwLocalPort, dwOwningPid
            Table::Udp4 => (12, 4, 8),
            // ucLocalAddr[16], dwLocalScopeId, dwLocalPort, dwOwningPid
            Table::Udp6 => (28, 20, 24),
        }
    }
}

/// The pids owning a row whose LOCAL port is `port`, on any local address, in table
/// order (duplicates kept — the caller dedups across tables).
///
/// `dwLocalPort` carries the port in network byte order in its low 16 bits, so the
/// first two bytes of the field ARE the port, big-endian. A count larger than the
/// buffer holds is clamped to the rows actually present rather than trusted.
pub(crate) fn owners_of_port(buf: &[u8], table: Table, port: u16) -> Vec<u32> {
    let (row, port_at, pid_at) = table.layout();
    let Some(count) = buf.get(0..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) else {
        return Vec::new();
    };
    let present = (buf.len() - 4) / row;
    let rows = (count as usize).min(present);
    (0..rows)
        .filter_map(|i| {
            let r = &buf[4 + i * row..4 + (i + 1) * row];
            let local = u16::from_be_bytes([r[port_at], r[port_at + 1]]);
            (local == port).then(|| {
                u32::from_le_bytes([r[pid_at], r[pid_at + 1], r[pid_at + 2], r[pid_at + 3]])
            })
        })
        .collect()
}

/// A range from `netsh interface ipv4 show excludedportrange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExcludedRange {
    pub start: u16,
    pub end: u16,
    /// Marked `*`: an administrator added it (`netsh ... add excludedportrange`), where
    /// the unmarked ones are taken at run time — by WinNAT for Hyper-V, WSL or Docker.
    pub administered: bool,
}

/// The excluded range containing `port`, if any.
///
/// A reservation is not a socket, so a port inside one has NO holder in any table. The
/// conflict help asks `netsh` only after the gate already refused a port no table
/// lists, to say where that port sits. It is a lead, not a proven cause: on the Dell
/// (13 Sep 2026) an ADMINISTERED range, 50000–50059, refused nothing — `127.0.0.1`,
/// `0.0.0.0` and `[::]` all listened and answered, TCP and UDP bound — because an
/// exclusion keeps Windows from HANDING OUT those ports, not a program from asking for
/// one. WinNAT's run-time ranges are widely reported to refuse binds; not measured here.
///
/// `netsh`'s headers and footnote are localized; its rows are not. So a row is any
/// line whose first two tokens are port numbers, with an optional `*` third token —
/// the English and German outputs parse the same.
pub(crate) fn excluded_range_containing(netsh: &str, port: u16) -> Option<ExcludedRange> {
    netsh.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let start: u16 = words.next()?.parse().ok()?;
        let end: u16 = words.next()?.parse().ok()?;
        let administered = match words.next() {
            None => false,
            Some("*") => true,
            Some(_) => return None,
        };
        (start <= port && port <= end).then_some(ExcludedRange { start, end, administered })
    })
}

/// Whether `command` carries rexenv's app-data `marker`, ignoring case.
///
/// Windows paths are case-insensitive, and a command line keeps whatever case its
/// creator typed, so an exact `contains` would disown our own leftover the day a path
/// reaches it spelled differently — the "our own process named as a stranger" error
/// `ensure_free` exists to avoid. An empty marker matches nothing, never everything.
pub(crate) fn command_carries_marker(command: &str, marker: &str) -> bool {
    !marker.is_empty() && command.to_lowercase().contains(&marker.to_lowercase())
}

/// `(pid, parent)` pairs for `traits::select_master`, with a parent link kept only
/// when the parent is known to have been created no later than the child.
///
/// Windows keeps a dead parent's pid in the child's record and reuses pids, so a
/// recorded parent may be an unrelated process born afterwards — which could even be
/// another member of the set, making the real master look like a worker (plan §3
/// D1(a), rule 4). Input: `(pid, parent pid, pid's creation time, parent's creation
/// time)`, times as FILETIME ticks; an unknown time drops the link.
pub(crate) fn parent_links(members: &[(u32, u32, Option<u64>, Option<u64>)]) -> Vec<(u32, u32)> {
    members
        .iter()
        .map(|&(pid, parent, born, parent_born)| match (born, parent_born) {
            (Some(child), Some(p)) if p <= child => (pid, parent),
            _ => (pid, 0),
        })
        .collect()
}

/// From each start pid, climb through parents for which `marked` holds, and return the
/// topmost marked pid of each chain (deduplicated, in input order). `parent_of` answers
/// only GENUINE parents (the caller applies the pid-reuse rule); a chain is cut at 16
/// steps so a corrupt table cannot loop.
pub(crate) fn climb_marked(
    starts: &[u32],
    parent_of: impl Fn(u32) -> Option<u32>,
    marked: impl Fn(u32) -> bool,
) -> Vec<u32> {
    let mut tops = Vec::new();
    for &start in starts {
        let mut top = start;
        for _ in 0..16 {
            match parent_of(top) {
                Some(parent) if parent != top && marked(parent) => top = parent,
                _ => break,
            }
        }
        if !tops.contains(&top) {
            tops.push(top);
        }
    }
    tops
}

/// A holder the conflict message can name: what it is, the app behind it when one is
/// identifiable, and a PowerShell line that stops it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Holder {
    pub holder: String,
    pub app: Option<String>,
    pub free_command: Option<String>,
}

/// The system process: the kernel's own sockets, which on a web port means HTTP.sys
/// serving a URL some other program registered (IIS, WinRM, a `netsh http` reservation).
pub(crate) const SYSTEM_PID: u32 = 4;

/// Name the process holding a port. `image` is its full image path when this user may
/// read it; `services` the Windows services it hosts (a `svchost.exe` is anonymous
/// without them — D2 asks for the service by name).
pub(crate) fn describe_holder(pid: u32, image: Option<&str>, services: &[String]) -> Holder {
    if pid == SYSTEM_PID {
        return Holder {
            holder: "Windows itself (the System process, pid 4) — on a web port that is \
                     HTTP.sys serving a URL another program registered; \
                     `netsh http show servicestate` lists who"
                .into(),
            app: None,
            // Nothing a user can stop by pid, and the registrant is only found by
            // reading that list, so no command pretends to free it.
            free_command: None,
        };
    }
    let file = image.map(|p| p.rsplit(['\\', '/']).next().unwrap_or(p));
    if let Some(service) = services.first() {
        let names = services.join(", ");
        let host = file.unwrap_or("a service host");
        let holder = format!("the Windows service {names} ({host}, pid {pid})");
        // A service name reaches a command the user pastes into an elevated shell, so
        // only a plain token is quoted into it; anything else gets no command at all.
        let free_command = safe_service_name(service).then(|| {
            format!("Start-Process powershell -Verb RunAs -ArgumentList 'Stop-Service -Name {service}'")
        });
        return Holder { holder, app: None, free_command };
    }
    let holder = match (file, image) {
        (Some(file), Some(path)) => format!("{file} (pid {pid}, {path})"),
        _ => format!(
            "pid {pid} (a process this user cannot inspect — another account's, or an \
             elevated one)"
        ),
    };
    Holder { holder, app: None, free_command: Some(format!("Stop-Process -Id {pid}")) }
}

/// Name an excluded range as the reason a port with no holder is still unusable.
pub(crate) fn describe_excluded(range: ExcludedRange, udp: bool) -> Holder {
    let proto = if udp { "udp" } else { "tcp" };
    let (who, free_command) = if range.administered {
        (
            "an administrator reserved it".to_string(),
            format!(
                "Start-Process netsh -Verb RunAs -ArgumentList 'int ipv4 delete excludedportrange \
                 protocol={proto} startport={} numberofports={}'",
                range.start,
                u32::from(range.end) - u32::from(range.start) + 1
            ),
        )
    } else {
        (
            "WinNAT reserved it for Hyper-V, WSL or Docker".to_string(),
            "Start-Process powershell -Verb RunAs -ArgumentList 'net stop winnat; net start winnat'"
                .to_string(),
        )
    };
    Holder {
        holder: format!(
            "a port range Windows excluded ({} {}–{}; {who}) — no process holds it",
            proto.to_uppercase(),
            range.start,
            range.end
        ),
        app: None,
        free_command: Some(free_command),
    }
}

fn safe_service_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table buffer: the count, then each row laid out as `layout` says, with the
    /// port big-endian in the port field and the pid little-endian.
    fn table(t: Table, rows: &[(u16, u32)]) -> Vec<u8> {
        let (row, port_at, pid_at) = t.layout();
        let mut buf = (rows.len() as u32).to_le_bytes().to_vec();
        for &(port, pid) in rows {
            let mut r = vec![0xAAu8; row]; // noise in every other field
            r[port_at..port_at + 4].copy_from_slice(&[(port >> 8) as u8, port as u8, 0, 0]);
            r[pid_at..pid_at + 4].copy_from_slice(&pid.to_le_bytes());
            buf.extend(r);
        }
        buf
    }

    #[test]
    fn every_table_yields_the_owner_of_the_port_and_no_other() {
        for t in [Table::Tcp4, Table::Tcp6, Table::Udp4, Table::Udp6] {
            let buf = table(t, &[(80, 4), (13306, 9120), (18025, 777), (13306, 9121)]);
            assert_eq!(owners_of_port(&buf, t, 13306), vec![9120, 9121], "{t:?}");
            assert_eq!(owners_of_port(&buf, t, 80), vec![4], "{t:?}");
            assert!(owners_of_port(&buf, t, 11025).is_empty(), "{t:?}");
        }
    }

    /// The byte order is the whole trick: 13306 is 0x33FA, and read little-endian it
    /// would be 0xFA33 = 64051. A parser that got it backwards finds a holder on the
    /// wrong port and never on ours.
    #[test]
    fn the_port_field_is_network_order() {
        let buf = table(Table::Tcp4, &[(13306, 42)]);
        assert!(owners_of_port(&buf, Table::Tcp4, 64051).is_empty());
        assert_eq!(owners_of_port(&buf, Table::Tcp4, 13306), vec![42]);
    }

    #[test]
    fn a_count_the_buffer_cannot_hold_is_clamped_and_short_buffers_are_empty() {
        let mut buf = table(Table::Udp4, &[(53, 1), (53, 2)]);
        buf[0..4].copy_from_slice(&1000u32.to_le_bytes());
        assert_eq!(owners_of_port(&buf, Table::Udp4, 53), vec![1, 2]);
        assert!(owners_of_port(&[], Table::Tcp6, 53).is_empty());
        assert!(owners_of_port(&[3, 0], Table::Tcp6, 53).is_empty());
    }

    const NETSH_EN: &str = "
Protocol tcp Port Exclusion Ranges

Start Port    End Port
----------    --------
      5357        5357
     13300       13399
     50000       50059     *

* - Administered port exclusions.
";

    const NETSH_DE: &str = "
Portausschlussbereiche für das Protokoll tcp

Startport   Endport
----------    --------
     13300       13399
     50000       50059     *

* - Verwaltete Portausschlüsse.
";

    #[test]
    fn excluded_ranges_parse_by_shape_in_any_language() {
        for out in [NETSH_EN, NETSH_DE] {
            assert_eq!(
                excluded_range_containing(out, 13306),
                Some(ExcludedRange { start: 13300, end: 13399, administered: false })
            );
            assert_eq!(
                excluded_range_containing(out, 50059),
                Some(ExcludedRange { start: 50000, end: 50059, administered: true })
            );
            assert_eq!(excluded_range_containing(out, 18025), None);
        }
        assert_eq!(excluded_range_containing(NETSH_EN, 5357).map(|r| r.start), Some(5357));
        // A footnote or a stray line with a third word is never a range.
        assert_eq!(excluded_range_containing("100 200 ports", 150), None);
    }

    #[test]
    fn the_marker_matches_whatever_case_the_command_line_kept() {
        let marker = r"C:\Users\DELL\AppData\Local\rexenv\rexenv\data";
        let cmd = r#""c:\users\dell\appdata\local\rexenv\rexenv\data\bin\mysql\bin\mysqld.exe" --port=13306"#;
        assert!(command_carries_marker(cmd, marker));
        assert!(!command_carries_marker(r"C:\Program Files\MySQL\bin\mysqld.exe", marker));
        assert!(!command_carries_marker(cmd, ""), "an empty marker must own nothing");
    }

    #[test]
    fn a_parent_born_after_its_child_is_a_reused_pid_not_a_parent() {
        // 200 records parent 100, but the process now called 100 started later: the
        // real parent died and its pid was reused. 300's parent 200 is genuine.
        let links = parent_links(&[
            (100, 7, Some(10), Some(5)),
            (200, 100, Some(20), Some(30)),
            (300, 200, Some(40), Some(20)),
            (400, 200, None, Some(20)),
        ]);
        assert_eq!(links, vec![(100, 7), (200, 0), (300, 200), (400, 0)]);
    }

    /// nginx: the listener is the worker (8560) and its master (10208) carries the same
    /// command line — the climb reaches the master. php-cgi: the listener is the parent,
    /// whose own parent (a shell, unmarked) stops the climb. A self-parent cannot loop.
    #[test]
    fn a_listener_climbs_to_its_marked_master_and_stops_at_an_unmarked_parent() {
        let parents = |pid: u32| match pid { 8560 => Some(10208), 10208 => Some(11300), 9184 => Some(700), 5 => Some(5), _ => None };
        let marked = |pid: u32| matches!(pid, 8560 | 10208 | 9184 | 5);
        assert_eq!(climb_marked(&[8560], parents, marked), vec![10208]);
        assert_eq!(climb_marked(&[9184], parents, marked), vec![9184]);
        assert_eq!(climb_marked(&[5], parents, marked), vec![5]);
        assert_eq!(climb_marked(&[8560, 10208], parents, marked), vec![10208]);
    }

    #[test]
    fn holders_are_named_by_image_service_or_as_uninspectable() {
        let mysqld = describe_holder(9120, Some(r"C:\Program Files\MySQL\bin\mysqld.exe"), &[]);
        assert_eq!(mysqld.holder, r"mysqld.exe (pid 9120, C:\Program Files\MySQL\bin\mysqld.exe)");
        assert_eq!(mysqld.free_command.as_deref(), Some("Stop-Process -Id 9120"));

        let svc = describe_holder(1300, Some(r"C:\Windows\System32\svchost.exe"), &["SharedAccess".into()]);
        assert_eq!(svc.holder, "the Windows service SharedAccess (svchost.exe, pid 1300)");
        assert!(svc.free_command.unwrap().contains("Stop-Service -Name SharedAccess"));

        let odd = describe_holder(1301, None, &["bad'; rm".into()]);
        assert_eq!(odd.free_command, None, "an unsafe service name never reaches a command");

        let hidden = describe_holder(5000, None, &[]);
        assert!(hidden.holder.contains("cannot inspect"), "{}", hidden.holder);

        let system = describe_holder(SYSTEM_PID, None, &[]);
        assert!(system.holder.contains("HTTP.sys") && system.free_command.is_none());
    }

    #[test]
    fn an_excluded_range_says_who_reserved_it_and_how_to_release_it() {
        let dynamic = describe_excluded(ExcludedRange { start: 13300, end: 13399, administered: false }, false);
        assert!(dynamic.holder.contains("TCP 13300–13399") && dynamic.holder.contains("WinNAT"));
        assert!(dynamic.free_command.unwrap().contains("net stop winnat"));
        let admin = describe_excluded(ExcludedRange { start: 50000, end: 50059, administered: true }, false);
        assert!(admin.free_command.unwrap().contains("startport=50000 numberofports=60"));
    }
}
