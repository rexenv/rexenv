//! Win32 behind `WindowsSupervisor`: the identity and port-gate reads (ledger #599) —
//! the socket tables, a process's image, command line, parent and creation time, the
//! services it hosts, `netsh`'s excluded port ranges — `Stoppable`, the process a stop
//! holds by handle, and the inherit-flag sweep before a service spawn (ledger #600).
//!
//! The rules applied to what these return live in `port_table.rs` and `stop_policy.rs`,
//! which have no Win32 in them and run their tests on every host. Everything here is
//! compile-checked from the Mac and proven only by a run on Windows
//! (`examples/windows_port_gate_check.rs`, `examples/windows_supervision_check.rs`).

use super::port_table::{self, Table};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::time::Duration;
use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA,
    FILETIME, HANDLE, INVALID_HANDLE_VALUE, NO_ERROR, STATUS_BUFFER_OVERFLOW,
    STATUS_BUFFER_TOO_SMALL, STATUS_INFO_LENGTH_MISMATCH, UNICODE_STRING, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, TCP_TABLE_CLASS, TCP_TABLE_OWNER_PID_ALL, TCP_TABLE_OWNER_PID_LISTENER,
    UDP_TABLE_OWNER_PID,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, ENUM_SERVICE_STATUS_PROCESSW,
    SC_ENUM_PROCESS_INFO, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_WIN32,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenEventW, OpenProcess, QueryFullProcessImageNameW, SetEvent,
    TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW, EVENT_MODIFY_STATE,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

/// A kernel handle closed on drop.
struct Owned(HANDLE);

impl Owned {
    /// `pid` opened for `access`, or `None` when it is gone or this user may not open it.
    fn process(pid: u32, access: u32) -> Option<Self> {
        if pid == 0 {
            return None;
        }
        // SAFETY: a plain call; the null failure value is checked before any use.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        (!handle.is_null()).then_some(Owned(handle))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful open and is closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

/// One of the owner-pid tables, as bytes for `port_table::owners_of_port`, or `None`
/// when the call fails for a reason other than a buffer that was too small.
fn read_table(family: u16, table: Table) -> Option<Vec<u8>> {
    read_table_class(family, table, TCP_TABLE_OWNER_PID_LISTENER)
}

/// [`read_table`] with the TCP table class spelled out — `TCP_TABLE_OWNER_PID_ALL` for the
/// connections as well as the listeners (UDP ignores it).
fn read_table_class(family: u16, table: Table, tcp_class: TCP_TABLE_CLASS) -> Option<Vec<u8>> {
    let mut size = 0u32;
    // The table can grow between the size query and the read; a few retries with
    // slack cover a busy machine without looping forever on a broken one.
    for _ in 0..6 {
        let mut buf = vec![0u8; size as usize];
        let ptr = if buf.is_empty() { std::ptr::null_mut() } else { buf.as_mut_ptr().cast() };
        // SAFETY: `size` is `buf`'s length in bytes (0 with a null pointer, which asks for
        // the size); the rows are parsed as bytes, so the u8 buffer's alignment is fine.
        let rc = unsafe {
            match table {
                Table::Tcp4 | Table::Tcp6 => GetExtendedTcpTable(
                    ptr,
                    &mut size,
                    0,
                    u32::from(family),
                    tcp_class,
                    0,
                ),
                Table::Udp4 | Table::Udp6 => {
                    GetExtendedUdpTable(ptr, &mut size, 0, u32::from(family), UDP_TABLE_OWNER_PID, 0)
                }
            }
        };
        match rc {
            NO_ERROR if buf.len() >= 4 => return Some(buf),
            NO_ERROR | ERROR_INSUFFICIENT_BUFFER => size = size.saturating_add(size / 4 + 256),
            other => {
                log::warn!("rexenv: reading the {table:?} socket table failed (error {other})");
                return None;
            }
        }
    }
    log::warn!("rexenv: the {table:?} socket table kept growing past every retry");
    None
}

/// Every pid holding `port` on any local address — both address families. `None` only
/// when neither family's table could be read; one family failing still answers from
/// the other rather than dropping back to the trial bind alone.
pub(crate) fn port_holders(port: u16, udp: bool) -> Option<Vec<u32>> {
    let tables = if udp {
        [(AF_INET, Table::Udp4), (AF_INET6, Table::Udp6)]
    } else {
        [(AF_INET, Table::Tcp4), (AF_INET6, Table::Tcp6)]
    };
    let mut read_any = false;
    let mut pids = Vec::new();
    for (family, table) in tables {
        if let Some(buf) = read_table(family, table) {
            read_any = true;
            pids.extend(port_table::owners_of_port(&buf, table, port));
        }
    }
    pids.sort_unstable();
    pids.dedup();
    read_any.then_some(pids)
}

/// ESTABLISHED TCP connections whose LOCAL end is on `port`, both address families (the
/// busy-workers signal, plan §3 D1(b)). `None` only when neither family's table could be read.
pub(crate) fn established_on(port: u16) -> Option<usize> {
    let mut read_any = false;
    let mut count = 0;
    for (family, table) in [(AF_INET, Table::Tcp4), (AF_INET6, Table::Tcp6)] {
        if let Some(buf) = read_table_class(family, table, TCP_TABLE_OWNER_PID_ALL) {
            read_any = true;
            count += port_table::established_on_port(&buf, table, port);
        }
    }
    read_any.then_some(count)
}

/// The full image path of `pid` — the file the kernel mapped, whatever the process
/// calls itself.
pub(crate) fn image_path(pid: u32) -> Option<PathBuf> {
    let process = Owned::process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut buf = vec![0u16; 32_768];
    let mut len = buf.len() as u32;
    // SAFETY: `len` is the buffer's capacity in characters; on success it is the count written.
    let ok = unsafe { QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
    (ok != 0).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buf[..len as usize])))
}

/// `pid`'s command line as its creator passed it, through
/// `ProcessCommandLineInformation` (Windows 8.1+), which needs only
/// `PROCESS_QUERY_LIMITED_INFORMATION` — no read of the other process's memory.
pub(crate) fn command_line(pid: u32) -> Option<String> {
    let process = Owned::process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    // u64 words, not bytes: the answer starts with a UNICODE_STRING, which holds a
    // pointer and so needs 8-byte alignment.
    let mut words: Vec<u64> = vec![0; 256];
    for _ in 0..4 {
        let bytes = (words.len() * 8) as u32;
        let mut needed = 0u32;
        // SAFETY: `bytes` is the buffer's length; the call writes at most that much.
        let status = unsafe {
            NtQueryInformationProcess(
                process.0,
                ProcessCommandLineInformation,
                words.as_mut_ptr().cast(),
                bytes,
                &mut needed,
            )
        };
        if matches!(status, STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_OVERFLOW | STATUS_BUFFER_TOO_SMALL) {
            words = vec![0; (needed as usize).div_ceil(8).max(words.len() * 2)];
            continue;
        }
        if status < 0 {
            return None;
        }
        // SAFETY: success means the buffer begins with a UNICODE_STRING; its Buffer is
        // checked to lie inside this allocation before a slice is made from it.
        let text = unsafe { &*(words.as_ptr() as *const UNICODE_STRING) };
        let chars = usize::from(text.Length) / 2;
        let (start, end) = (words.as_ptr() as usize, words.as_ptr() as usize + words.len() * 8);
        let at = text.Buffer as usize;
        if text.Buffer.is_null() || chars == 0 || at < start || at + chars * 2 > end {
            return None;
        }
        // SAFETY: bounds checked just above.
        let wide = unsafe { std::slice::from_raw_parts(text.Buffer, chars) };
        return Some(String::from_utf16_lossy(wide));
    }
    None
}

/// `pid`'s creation time in FILETIME ticks — the pid-reuse check in
/// `port_table::parent_links`.
pub(crate) fn creation_time(pid: u32) -> Option<u64> {
    let process = Owned::process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: four valid out-pointers to FILETIMEs on this stack frame.
    let ok = unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) };
    (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// Whether `pid` is a running process this user may wait on. A process that cannot be
/// opened reads as not alive — the same answer macOS's `kill -0` gives for one this user
/// may not signal; callers only ask about processes rexenv started.
pub(crate) fn alive(pid: u32) -> bool {
    let Some(process) = Owned::process(pid, PROCESS_SYNCHRONIZE) else {
        return false;
    };
    // SAFETY: a zero-timeout wait on a handle opened with SYNCHRONIZE.
    unsafe { WaitForSingleObject(process.0, 0) == WAIT_TIMEOUT }
}

/// One process in a ToolHelp snapshot.
pub(crate) struct Entry {
    pub pid: u32,
    pub parent: u32,
    /// The image FILE NAME only (`mysqld.exe`), as ToolHelp reports it.
    pub exe: String,
}

/// Every process on the machine, with its recorded parent.
pub(crate) fn processes() -> Vec<Entry> {
    // SAFETY: a plain call; INVALID_HANDLE_VALUE is its failure value.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let snap = Owned(snap);
    let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut out = Vec::new();
    // SAFETY: `entry.dwSize` is set as the API requires; the snapshot handle is open.
    let mut ok = unsafe { Process32FirstW(snap.0, &mut entry) };
    while ok != 0 {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        out.push(Entry {
            pid: entry.th32ProcessID,
            parent: entry.th32ParentProcessID,
            exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
        });
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap.0, &mut entry) };
    }
    out
}

/// The root of `pids` — the member whose GENUINE parent is not a member (plan §3 D1(a)
/// rule 4, `traits::select_master`), with parent links a reused pid could fake dropped.
pub(crate) fn root_of(pids: &[u32]) -> Option<u32> {
    if pids.len() <= 1 {
        return pids.first().copied();
    }
    let table = processes();
    let members: Vec<_> = pids
        .iter()
        .map(|&pid| {
            let parent = table.iter().find(|e| e.pid == pid).map_or(0, |e| e.parent);
            let parent_born = if parent == 0 { None } else { creation_time(parent) };
            (pid, parent, creation_time(pid), parent_born)
        })
        .collect();
    crate::platform::traits::select_master(&port_table::parent_links(&members))
}

/// The names of the running Win32 services hosted by `pid` — what makes a
/// `svchost.exe` holder nameable (D2 asks for the service, not the host).
pub(crate) fn services_hosted_by(pid: u32) -> Vec<String> {
    // SAFETY: null machine and database names mean the local, active database.
    let scm = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ENUMERATE_SERVICE) };
    if scm.is_null() {
        return Vec::new();
    }
    struct Scm(windows_sys::Win32::System::Services::SC_HANDLE);
    impl Drop for Scm {
        fn drop(&mut self) {
            // SAFETY: opened above, closed once.
            unsafe { CloseServiceHandle(self.0) };
        }
    }
    let scm = Scm(scm);
    // u64 words: the entries hold string pointers, so the buffer needs 8-byte alignment.
    let mut words: Vec<u64> = vec![0; 8_192];
    let (mut needed, mut count, mut resume) = (0u32, 0u32, 0u32);
    let mut names = Vec::new();
    for _ in 0..64 {
        // SAFETY: the length passed is the buffer's size in bytes; `resume` carries the
        // enumeration across ERROR_MORE_DATA rounds.
        let ok = unsafe {
            EnumServicesStatusExW(
                scm.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_ACTIVE,
                words.as_mut_ptr().cast(),
                (words.len() * 8) as u32,
                &mut needed,
                &mut count,
                &mut resume,
                std::ptr::null(),
            )
        };
        // SAFETY: read immediately after the failing call.
        let more = ok == 0 && unsafe { GetLastError() } == ERROR_MORE_DATA;
        if ok == 0 && !more {
            break;
        }
        // SAFETY: the call filled `count` entries at the start of the buffer.
        let entries = unsafe {
            std::slice::from_raw_parts(words.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize)
        };
        for entry in entries {
            if entry.ServiceStatusProcess.dwProcessId == pid {
                names.push(wide_c_str(entry.lpServiceName));
            }
        }
        if !more {
            break;
        }
        if count == 0 {
            words = vec![0; (needed as usize).div_ceil(8).max(words.len() * 2)];
        }
    }
    names
}

/// A NUL-terminated UTF-16 string the service enumeration pointed at.
fn wide_c_str(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: the pointer comes from a successful enumeration and is NUL-terminated
    // inside the same buffer.
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

/// `netsh interface ipv4 show excludedportrange` for the protocol, run without a
/// console window. `None` when netsh is missing or fails. IPv4 only: every port rexenv
/// gates is bound on `127.0.0.1`.
pub(crate) fn excluded_ranges(udp: bool) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let protocol = if udp { "protocol=udp" } else { "protocol=tcp" };
    let out = std::process::Command::new("netsh")
        .args(["interface", "ipv4", "show", "excludedportrange", protocol])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A process being stopped, held by handle for the whole stop so its pid cannot be
/// reused under us between the request, the wait and the terminate (ledger #600).
pub(crate) struct Stoppable {
    pid: u32,
    process: Option<Owned>,
}

impl Stoppable {
    /// `Ok` with no handle when the process is already gone; `Err` when it exists but
    /// this user may not stop it — never reported as "already gone".
    pub(crate) fn open(pid: u32) -> crate::error::Result<Self> {
        let access = PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
        match Owned::process(pid, access) {
            Some(process) => Ok(Stoppable { pid, process: Some(process) }),
            // SAFETY: read immediately after the failing open.
            None if unsafe { GetLastError() } == ERROR_ACCESS_DENIED => Err(crate::error::Error::Other(
                format!("cannot stop pid {pid}: this user may not end that process (access denied)"),
            )),
            None => Ok(Stoppable { pid, process: None }),
        }
    }
}

impl super::stop_policy::Target for Stoppable {
    fn alive(&mut self) -> bool {
        // SAFETY: a zero-timeout wait on a handle opened with SYNCHRONIZE.
        self.process.as_ref().is_some_and(|p| unsafe { WaitForSingleObject(p.0, 0) } == WAIT_TIMEOUT)
    }

    /// The process's own shutdown event, if it published one — `mysqld`'s or nginx's.
    fn request_clean_exit(&mut self) -> bool {
        for name in super::stop_policy::clean_exit_events(self.pid) {
            if set_named_event(&name) {
                log::info!("rexenv: asked pid {} to shut down through its {name} event", self.pid);
                return true;
            }
        }
        false
    }

    fn wait_exit(&mut self, budget: Duration) -> bool {
        let Some(process) = self.process.as_ref() else {
            return true;
        };
        let millis = u32::try_from(budget.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: a bounded wait on a handle opened with SYNCHRONIZE.
        unsafe { WaitForSingleObject(process.0, millis) == WAIT_OBJECT_0 }
    }

    fn terminate(&mut self) -> bool {
        let Some(process) = self.process.as_ref() else {
            return false;
        };
        log::info!("rexenv: terminating pid {}", self.pid);
        // SAFETY: a handle opened with PROCESS_TERMINATE.
        unsafe { TerminateProcess(process.0, 1) != 0 }
    }
}

/// Clear the inherit flag on every handle THIS process holds, so a service spawned next
/// inherits only the stdio `std` hands it (ledger #600, `handles.rs` for the measurement).
///
/// Runs before each service spawn rather than once at start, so a handle some library
/// opened inheritable later is caught too. `std`'s `Stdio::inherit`/`Stdio::from(File)`
/// duplicate their OWN inheritable copy inside `spawn`, after this, so no `std` child loses
/// its stdio; `std` serialises its spawns, so another thread's child-stdio copies cannot
/// slip in between. Returns how many flags were cleared. If the snapshot cannot be read
/// (it needs Windows 8), the standard handles are still cleared — the old, partial answer.
pub(crate) fn keep_inheritable_handles_out_of_children() -> usize {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    /// `ProcessHandleSnapshotInformation`.
    const PROCESS_HANDLE_SNAPSHOT_INFORMATION: i32 = 51;
    let clear = |handle: HANDLE| -> bool {
        // SAFETY: a handle value this process holds; clearing a flag closes nothing, and a
        // value that was closed in between fails the call harmlessly.
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) != 0 }
    };
    // u64 words: the snapshot holds pointer-sized fields, so give it 8-byte alignment.
    let mut words: Vec<u64> = vec![0; 1024];
    for _ in 0..6 {
        let mut needed = 0u32;
        // SAFETY: the length passed is the buffer's size in bytes.
        let status = unsafe {
            NtQueryInformationProcess(
                GetCurrentProcess(),
                PROCESS_HANDLE_SNAPSHOT_INFORMATION,
                words.as_mut_ptr().cast(),
                (words.len() * 8) as u32,
                &mut needed,
            )
        };
        if matches!(status, STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_OVERFLOW | STATUS_BUFFER_TOO_SMALL) {
            words = vec![0; (needed as usize).div_ceil(8).max(words.len() * 2)];
            continue;
        }
        if status < 0 {
            break;
        }
        // SAFETY: a byte view of the u64 buffer the call filled.
        let bytes = unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), words.len() * 8) };
        let cleared = super::handles::inheritable_handles(bytes)
            .into_iter()
            .filter(|&value| clear(value as HANDLE))
            .count();
        if cleared > 0 {
            log::info!("rexenv: cleared the inherit flag on {cleared} handle(s) before spawning a service");
        }
        return cleared;
    }
    log::warn!("rexenv: could not read this process's handle table; clearing the standard handles only");
    use std::os::windows::io::AsRawHandle;
    [std::io::stdin().as_raw_handle(), std::io::stdout().as_raw_handle(), std::io::stderr().as_raw_handle()]
        .into_iter()
        .map(|h| h as HANDLE)
        .filter(|&h| !h.is_null() && h != INVALID_HANDLE_VALUE)
        .filter(|&h| clear(h))
        .count()
}

/// Set the named event `name` if some process published it; `false` when none exists or
/// the set fails. How rexenv talks to a server that listens for its own events rather
/// than for signals (`mysqld`'s shutdown, nginx's quit and reload — `stop_policy.rs`).
pub(crate) fn set_named_event(name: &str) -> bool {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a NUL-terminated name; a null handle (no such event) is checked.
    let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, wide.as_ptr()) };
    if event.is_null() {
        return false;
    }
    let event = Owned(event);
    // SAFETY: an event handle opened with EVENT_MODIFY_STATE.
    unsafe { SetEvent(event.0) != 0 }
}

/// The group a listener set belongs to, climbed to its topmost member: from each pid,
/// up through parents that carry `marker` too — with a parent link counted only when
/// that parent is not younger than the child (pid reuse) — then `traits::select_master`
/// over the result. nginx's listener is its WORKER, whose master holds no socket
/// (measured); php-cgi's listener is already the parent.
pub(crate) fn climb_to_group_root(listeners: &[u32], marker: &str) -> Option<u32> {
    if listeners.is_empty() {
        return None;
    }
    let table = processes();
    let parent_of = |pid: u32| -> Option<u32> {
        let parent = table.iter().find(|e| e.pid == pid).map(|e| e.parent).filter(|&p| p != 0)?;
        let (child_born, parent_born) = (creation_time(pid)?, creation_time(parent)?);
        (parent_born <= child_born).then_some(parent)
    };
    let marked = |pid: u32| command_line(pid).is_some_and(|c| port_table::command_carries_marker(&c, marker));
    let tops = port_table::climb_marked(listeners, parent_of, marked);
    root_of(&tops)
}
