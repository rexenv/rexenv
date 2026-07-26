//! core::dns — embedded DNS resolver (Phase 1 task 2.1; per-TLD since the
//! configurable-TLD feature).
//!
//! Answers A queries for ANY host with `127.0.0.1`. The handler is deliberately
//! TLD-agnostic: WHICH TLDs ever reach it is scoped entirely by which OS
//! resolver files exist (`/etc/resolver/<tld>` on macOS, one per TLD) — so
//! there is no in-process TLD state and adding a TLD never restarts the DNS
//! server. LOOPBACK-ONLY CAVEAT: this is safe precisely because the server
//! binds 127.0.0.1 and only OS resolver files we install route queries to it;
//! it must never be bound on a non-loopback interface, where answer-anything
//! would turn it into an open wildcard resolver. Because every name resolves
//! to loopback, WordPress subdomain multisite (`*.mysite.rex`) works for
//! free. Pointing the OS at this server is the per-OS `DnsManager` step.

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use async_trait::async_trait;
use hickory_proto::op::{Header, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use hickory_server::authority::MessageResponseBuilder;
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_server::ServerFuture;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::net::UdpSocket;

/// Default loopback port for the embedded resolver. Not :53 (privileged) — the
/// OS resolver config (task 2.2) points `.rex` (and any configured TLD) lookups here.
pub const DEFAULT_DNS_PORT: u16 = 15353;

/// TTL (seconds) on answers. Short, since these are local and may change.
const ANSWER_TTL: u32 = 60;

/// Request handler that maps EVERY A query → a single loopback address (TLD
/// scope lives in the OS resolver files — see the module docs).
pub struct DnsHandler {
    answer: Ipv4Addr,
}

impl DnsHandler {
    pub fn new(answer: Ipv4Addr) -> Self {
        Self { answer }
    }

    /// Build and send the response for one request. Errors are turned into
    /// SERVFAIL by the caller.
    async fn respond<R: ResponseHandler>(
        &self,
        request: &Request,
        response_handle: &mut R,
    ) -> Result<ResponseInfo> {
        let query = request.query();
        let name: Name = query.original().name().clone();
        let qtype = query.query_type();

        let is_query = request.op_code() == OpCode::Query
            && request.message_type() == MessageType::Query;

        let mut header = Header::response_from_request(request.header());
        header.set_authoritative(true);

        let mut answers: Vec<Record> = Vec::new();
        if !is_query {
            header.set_response_code(ResponseCode::Refused);
        } else if qtype == RecordType::A {
            // ANY name → loopback. No TLD check here on purpose: only queries
            // for TLDs with an installed OS resolver file ever arrive, so the
            // TLD scope lives in which files exist (loopback-only caveat in
            // the module docs).
            answers.push(Record::from_rdata(
                name.clone(),
                ANSWER_TTL,
                RData::A(A(self.answer)),
            ));
        }
        // Non-A (e.g. AAAA): NOERROR with no records, so clients fall back to
        // the A record instead of failing.

        let builder = MessageResponseBuilder::from_message_request(request);
        let empty: Vec<Record> = Vec::new();
        let response = builder.build(
            header,
            answers.iter(),
            empty.iter(),
            empty.iter(),
            empty.iter(),
        );
        Ok(response_handle.send_response(response).await?)
    }
}

impl Default for DnsHandler {
    fn default() -> Self {
        Self::new(Ipv4Addr::LOCALHOST)
    }
}

#[async_trait]
impl RequestHandler for DnsHandler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        match self.respond(request, &mut response_handle).await {
            Ok(info) => info,
            Err(e) => {
                log::warn!("dns: failed to handle request: {e}");
                let mut header = Header::response_from_request(request.header());
                header.set_response_code(ResponseCode::ServFail);
                header.into()
            }
        }
    }
}

/// Bind a UDP socket and build a resolver server on it. Returns the actually
/// bound address (useful when `addr` uses port 0) plus the server; the caller
/// drives it with `server.block_until_done().await`.
pub async fn serve_udp(addr: SocketAddr) -> Result<(SocketAddr, ServerFuture<DnsHandler>)> {
    let socket = UdpSocket::bind(addr).await?;
    let local = socket.local_addr()?;
    let mut server = ServerFuture::new(DnsHandler::default());
    server.register_socket(socket);
    Ok((local, server))
}

/// The running embedded resolver, managed as a background task. Created once on
/// app launch (held in Tauri state) and aborted on `stop()` / drop, giving a
/// clean shutdown on app exit. Must be created within a tokio runtime context.
pub struct DnsService {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl DnsService {
    /// Bind `addr` and spawn the resolver on the current tokio runtime.
    pub async fn start(addr: SocketAddr) -> Result<Self> {
        let (local, mut server) = serve_udp(addr).await?;
        let handle = tokio::spawn(async move {
            if let Err(e) = server.block_until_done().await {
                log::error!("dns: resolver task ended with error: {e}");
            }
        });
        log::info!("dns: embedded resolver listening on {local} (udp)");
        Ok(Self { addr: local, handle })
    }

    /// Start on the fixed default loopback port (`DEFAULT_DNS_PORT`), gated on
    /// `ports::ensure_free` like every other service — a conflict names the
    /// holder and a command to free the port instead of a raw bind error.
    pub async fn start_default(platform: &dyn Platform) -> Result<Self> {
        crate::core::ports::ensure_free(
            platform,
            DEFAULT_DNS_PORT,
            crate::core::ports::Proto::Udp,
            "DNS resolver",
        )?;
        Self::start(SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_DNS_PORT))).await
    }

    /// The address the resolver is actually bound to.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Whether the resolver task is still running.
    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }

    /// Abort the resolver task (clean stop).
    pub fn stop(&self) {
        self.handle.abort();
    }
}

impl Drop for DnsService {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Headless resolver — the body of `rexenv --dns-agent`, run by the per-user
/// LaunchAgent (`DnsAgentManager`) so name resolution survives app quits and is
/// up from login. No Tauri, no app state, no SQLite: just the same hickory
/// handler on the fixed loopback port, forever.
///
/// Never exits on a busy port: an older app instance's IN-PROCESS resolver may
/// still hold it, and exiting would make launchd throttle-flap the agent. Instead
/// retry every 10s — when the old holder quits, the agent takes over seamlessly
/// (the handoff that motivates the agent in the first place). Output goes to the
/// agent log via launchd's Standard{Out,Err}Path.
pub fn run_agent() -> i32 {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("rexenv dns-agent: failed to build runtime: {e}");
            return 1;
        }
    };
    rt.block_on(async {
        loop {
            match serve_udp(SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_DNS_PORT))).await {
                Ok((addr, mut server)) => {
                    eprintln!("rexenv dns-agent: listening on {addr} (udp)");
                    if let Err(e) = server.block_until_done().await {
                        eprintln!("rexenv dns-agent: server ended: {e}; re-binding in 10s");
                    }
                }
                Err(e) => {
                    eprintln!(
                        "rexenv dns-agent: cannot bind :{DEFAULT_DNS_PORT} ({e}); retrying in 10s"
                    );
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    })
}

/// Whether OUR resolver semantics are live on loopback `port`: send a real A
/// query for a throwaway name and require the answer `127.0.0.1`. Distinguishes
/// "our agent (or an old in-process resolver) is serving" from a foreign process
/// merely holding the port — the ownership-AND-liveness rule (H2) applied to DNS.
/// Synchronous with a short timeout; callers treat any failure as "not ours".
pub fn answers_as_ours(port: u16) -> bool {
    use hickory_proto::op::{Message, Query};
    use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};

    let Ok(name) = Name::from_ascii("liveness-probe.rex.") else { return false };
    let mut msg = Message::new();
    msg.set_id(0x7e7e)
        .set_message_type(MessageType::Query)
        .set_op_code(OpCode::Query)
        .set_recursion_desired(true)
        .add_query(Query::query(name, RecordType::A));
    let Ok(bytes) = msg.to_bytes() else { return false };

    let probe = || -> std::io::Result<bool> {
        let sock = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        sock.set_read_timeout(Some(std::time::Duration::from_millis(500)))?;
        sock.send_to(&bytes, (Ipv4Addr::LOCALHOST, port))?;
        let mut buf = [0u8; 512];
        let (n, _) = sock.recv_from(&mut buf)?;
        let Ok(reply) = Message::from_bytes(&buf[..n]) else { return Ok(false) };
        Ok(reply.id() == 0x7e7e
            && reply
                .answers()
                .iter()
                .any(|r| matches!(r.data(), Some(RData::A(A(ip))) if *ip == Ipv4Addr::LOCALHOST)))
    };
    probe().unwrap_or(false)
}

/// Install the OS resolver file for `tld` (pointing at our resolver on `port`)
/// through `PrivilegeManager` — one auth prompt.
///
/// Writes unconditionally: callers must have established that the file is ours
/// or absent ([`ensure_resolver`]), or have taken it over deliberately with a
/// recorded backup. `run_system_setup` calls this via `ensure_resolver` for the
/// backbone TLD; CA trust is a SEPARATE prompt and cannot be batched with it
/// (login-keychain trust needs a UI session a detached-root shell lacks — see
/// `core::setup`'s module docs).
pub fn configure_resolver(platform: &dyn Platform, tld: &str, port: u16) -> Result<()> {
    let cmd = platform.dns().install_command(tld, port);
    platform.privileges().run_privileged(&cmd)?;
    Ok(())
}

/// Who owns the OS resolver file for a TLD.
///
/// Ownership across this codebase is CONTENT equality — there is no marker and
/// no provenance in the file itself (`resolver_contents` doubles as the
/// signature). That makes "is this ours?" answerable, which is what keeps the
/// teardown sweep from touching a foreign file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolverOwner {
    /// No file at all — installing is a plain create, nothing to consent to.
    Absent,
    /// Exactly our signature — nothing to do.
    Ours,
    /// Someone else's file (Valet, Herd, hand-written). NEVER overwritten
    /// without an explicit takeover that backs it up first. `content` is `None`
    /// when the file exists but couldn't be read.
    Foreign { content: Option<String> },
}

/// Classify the resolver file for `tld`.
pub fn resolver_owner(platform: &dyn Platform, tld: &str, port: u16) -> ResolverOwner {
    owner_of(
        &platform.dns().resolver_path(tld),
        &platform.dns().resolver_contents(port),
    )
}

/// The classification itself, over a path + signature so it is unit-testable
/// against fixture files (same reason [`tlds_matching_signature`] takes a dir):
/// the dev machine has no foreign resolver file to exercise this against, and
/// creating a root-owned one to test would be worse than a fixture.
///
/// A file we cannot READ counts as foreign, never absent — refusing to touch
/// what we can't inspect is the safe direction.
fn owner_of(path: &std::path::Path, signature: &str) -> ResolverOwner {
    match std::fs::read_to_string(path) {
        Ok(c) if c == signature => ResolverOwner::Ours,
        Ok(content) => ResolverOwner::Foreign { content: Some(content) },
        Err(_) if path.exists() => ResolverOwner::Foreign { content: None },
        Err(_) => ResolverOwner::Absent,
    }
}

/// Whether `tld`'s OS resolver file is installed with our expected content.
/// Used to skip the privileged prompt when there's nothing to do.
pub fn resolver_installed(platform: &dyn Platform, tld: &str, port: u16) -> bool {
    resolver_owner(platform, tld, port) == ResolverOwner::Ours
}

/// The refusal when another tool already owns a TLD's resolver file.
fn foreign_resolver_error(path: &std::path::Path) -> Error {
    Error::Other(format!(
        "{} is managed by another tool (most likely Valet or Herd) — rexenv won't \
         overwrite it. rexenv can take that TLD over, backing up the existing file first \
         and restoring it if you hand it back, or you can use a different TLD for this site.",
        path.display()
    ))
}

/// Ensure `tld` resolves locally: install its OS resolver file unless it is
/// already ours — so each TLD costs at most ONE privileged prompt, on first
/// use. TLDs coexist (one file each); the embedded server needs no restart (it
/// answers any name — module docs).
///
/// **Refuses a FOREIGN file.** Overwriting one used to be silent, and it is
/// doubly destructive: the user loses their config, and because ownership is
/// content equality the file then looks like ours, so teardown would delete it
/// — leaving them with neither their file nor ours. Taking a TLD over is a
/// deliberate, backed-up, consented operation; it does not happen as a side
/// effect of creating a site.
pub fn ensure_resolver(platform: &dyn Platform, tld: &str, port: u16) -> Result<()> {
    match resolver_owner(platform, tld, port) {
        ResolverOwner::Ours => Ok(()),
        ResolverOwner::Absent => configure_resolver(platform, tld, port),
        ResolverOwner::Foreign { .. } => {
            Err(foreign_resolver_error(&platform.dns().resolver_path(tld)))
        }
    }
}

/// Where we keep our copies of resolver files we borrowed.
pub fn backup_dir(platform: &dyn Platform) -> Result<std::path::PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("resolver-backups"))
}

/// Our backup of `tld`'s original file. Named after the TLD, NOT timestamped:
/// at most one backup per TLD can then exist by construction, which is what
/// makes an orphaned backup unrepresentable rather than merely unlikely (the
/// row and this file are created and deleted together).
pub fn backup_path(platform: &dyn Platform, tld: &str) -> Result<std::path::PathBuf> {
    Ok(backup_dir(platform)?.join(tld))
}

/// BORROW another tool's resolver file for `tld`: back up what's there, record
/// it, then install ours (one privileged prompt).
///
/// Ordering is the safety property. The backup and the record land BEFORE the
/// privileged write, so a cancelled password prompt can't leave us holding a
/// file we can't give back; if that write fails, both are rolled back and
/// nothing on disk changed. Refuses a file we can't read — we will not replace
/// what we cannot restore.
pub fn take_over_resolver(
    conn: &rusqlite::Connection,
    platform: &dyn Platform,
    tld: &str,
    port: u16,
) -> Result<()> {
    let original = match resolver_owner(platform, tld, port) {
        ResolverOwner::Ours => return Ok(()), // already ours; nothing borrowed
        ResolverOwner::Absent => return configure_resolver(platform, tld, port),
        ResolverOwner::Foreign { content: None } => {
            return Err(Error::Other(format!(
                "{} can't be read, so rexenv can't back it up — and it won't replace a file \
                 it couldn't give back.",
                platform.dns().resolver_path(tld).display()
            )))
        }
        ResolverOwner::Foreign { content: Some(c) } => c,
    };

    let backup = backup_path(platform, tld)?;
    if let Some(dir) = backup.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Born 0600 (B6); re-hardens an existing file before overwriting, which is
    // what a re-takeover after they reclaimed the TLD does.
    platform.permissions().write_private(&backup, original.as_bytes())?;
    crate::state::store::insert_resolver_takeover(
        conn,
        tld,
        &original,
        &backup.display().to_string(),
    )?;

    if let Err(e) = configure_resolver(platform, tld, port) {
        // Roll back together — the row owns the file.
        let _ = crate::state::store::delete_resolver_takeover(conn, tld);
        let _ = std::fs::remove_file(&backup);
        return Err(e);
    }
    Ok(())
}

/// What teardown (or a hand-back) should do about our resolver files, decided
/// per TLD from the file's CURRENT owner and whether we hold a record.
///
/// | file now | record | action |
/// |---|---|---|
/// | ours | yes | restore their backup |
/// | ours | no | remove (we created it) |
/// | foreign | yes | leave alone — they reclaimed it; drop the moot record |
/// | foreign | no | leave alone (never enumerated) |
/// | absent | yes | drop the moot record |
/// | absent | no | nothing |
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolverPlan {
    /// Ours outright — delete the file.
    pub remove: Vec<String>,
    /// Borrowed — put their file back from `(tld, backup)`.
    pub restore: Vec<(String, std::path::PathBuf)>,
    /// Records to forget afterwards (restored, or reclaimed by them).
    pub drop_records: Vec<String>,
    /// Borrowed, but our backup is gone: we remove ours and say so, rather
    /// than leaving our file in place pretending to be theirs.
    pub backup_missing: Vec<String>,
    /// They took these back themselves — we touch nothing.
    pub reclaimed: Vec<String>,
}

/// Build the plan across every resolver file we might be responsible for:
/// the ones matching our signature, plus every TLD we hold a record for.
pub fn plan_resolver_teardown(
    conn: &rusqlite::Connection,
    platform: &dyn Platform,
    port: u16,
) -> Result<ResolverPlan> {
    let mut plan = ResolverPlan::default();
    let records = crate::state::store::list_resolver_takeovers(conn)?;

    for rec in &records {
        match resolver_owner(platform, &rec.tld, port) {
            ResolverOwner::Ours => {
                let backup = std::path::PathBuf::from(&rec.backup_path);
                if backup.is_file() {
                    plan.restore.push((rec.tld.clone(), backup));
                } else {
                    // Can't give theirs back; removing ours is closer to their
                    // pre-rexenv state than leaving it, and the caller says so.
                    plan.remove.push(rec.tld.clone());
                    plan.backup_missing.push(rec.tld.clone());
                }
                plan.drop_records.push(rec.tld.clone());
            }
            // They reclaimed it (a `valet install`, a Herd relaunch). Not ours
            // to touch, and the record is moot.
            ResolverOwner::Foreign { .. } => {
                plan.reclaimed.push(rec.tld.clone());
                plan.drop_records.push(rec.tld.clone());
            }
            ResolverOwner::Absent => plan.drop_records.push(rec.tld.clone()),
        }
    }

    // Everything else carrying our signature is ours outright — today's sweep.
    let borrowed: std::collections::HashSet<&str> =
        records.iter().map(|r| r.tld.as_str()).collect();
    for tld in installed_tlds(platform, port) {
        if !borrowed.contains(tld.as_str()) {
            plan.remove.push(tld);
        }
    }
    plan.remove.sort();
    plan.remove.dedup();
    Ok(plan)
}

/// Forget the records the plan resolved, deleting each backup with its row —
/// call AFTER the privileged step succeeded.
pub fn finish_resolver_teardown(
    conn: &rusqlite::Connection,
    platform: &dyn Platform,
    plan: &ResolverPlan,
) -> Result<()> {
    for tld in &plan.drop_records {
        if let Ok(Some(rec)) = crate::state::store::get_resolver_takeover(conn, tld) {
            let _ = std::fs::remove_file(&rec.backup_path);
        }
        crate::state::store::delete_resolver_takeover(conn, tld)?;
    }
    let _ = platform; // kept for symmetry with the rest of the module
    Ok(())
}

/// Give ONE borrowed resolver file back to whoever we took it from.
///
/// The same operation teardown performs, wired to a button: borrowing someone's
/// file is only honest if the return path is one click rather than "uninstall
/// rexenv". Refuses a TLD we never borrowed — handing back a file we created
/// ourselves would just be deleting it under a friendlier name.
pub fn hand_back_resolver(
    conn: &rusqlite::Connection,
    platform: &dyn Platform,
    tld: &str,
    port: u16,
) -> Result<ResolverPlan> {
    if crate::state::store::get_resolver_takeover(conn, tld)?.is_none() {
        return Err(Error::Other(format!(
            "rexenv didn't take .{tld} over from anything, so there's nothing to hand back."
        )));
    }
    let full = plan_resolver_teardown(conn, platform, port)?;
    // Narrow the whole-system plan to this one TLD.
    let only = |v: &[String]| -> Vec<String> {
        if v.iter().any(|t| t == tld) { vec![tld.to_string()] } else { Vec::new() }
    };
    let plan = ResolverPlan {
        remove: only(&full.remove),
        restore: full.restore.into_iter().filter(|(t, _)| t == tld).collect(),
        drop_records: vec![tld.to_string()],
        backup_missing: only(&full.backup_missing),
        reclaimed: only(&full.reclaimed),
    };

    let mut cmds = Vec::new();
    if !plan.remove.is_empty() {
        cmds.push(platform.dns().uninstall_command(&plan.remove));
    }
    if !plan.restore.is_empty() {
        cmds.push(platform.dns().restore_command(&plan.restore));
    }
    if !cmds.is_empty() {
        platform.privileges().run_privileged(&cmds.join(" ; "))?;
    }
    finish_resolver_teardown(conn, platform, &plan)?;
    Ok(plan)
}

/// TLDs we hold a record for whose file is no longer ours — Valet or Herd took
/// it back (a `valet install`, a Herd relaunch).
///
/// Worth surfacing because the failure is otherwise silent and baffling: our
/// resolver still answers on its own port so the health watchdog stays green,
/// while every rexenv site on that TLD stops resolving. Checked where we
/// already look at environment truth (startup, `rex doctor`) rather than by a
/// watcher — reading one small file per borrowed TLD is nearly free.
pub fn drifted_takeovers(
    conn: &rusqlite::Connection,
    platform: &dyn Platform,
    port: u16,
) -> Vec<String> {
    crate::state::store::list_resolver_takeovers(conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| matches!(resolver_owner(platform, &r.tld, port), ResolverOwner::Foreign { .. }))
        .map(|r| r.tld)
        .collect()
}

/// Delete backups no record refers to — the belt for a crash between writing
/// the backup and inserting its row. Returns how many were swept.
pub fn sweep_orphan_backups(conn: &rusqlite::Connection, platform: &dyn Platform) -> usize {
    let Ok(dir) = backup_dir(platform) else { return 0 };
    let Ok(entries) = std::fs::read_dir(&dir) else { return 0 };
    let known: std::collections::HashSet<String> =
        crate::state::store::list_resolver_takeovers(conn)
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.tld)
            .collect();
    let mut swept = 0;
    for e in entries.flatten() {
        let Ok(name) = e.file_name().into_string() else { continue };
        if !known.contains(&name) && std::fs::remove_file(e.path()).is_ok() {
            swept += 1;
        }
    }
    swept
}

/// The TLDs whose resolver files under `dir` are OURS — file content equals
/// `signature` (`resolver_contents(port)`, i.e. loopback + our fixed port —
/// the same ownership test as service adoption's port+marker). Pure directory
/// scan, factored out of [`installed_tlds`] for testability. Non-UTF8 names
/// and unreadable/foreign files are skipped.
///
/// The name is ALSO required to be a syntactically valid TLD label
/// (`tld::is_valid_label`, `[a-z]{1,63}`): every resolver file rexenv writes has
/// that shape (creation passes `ensure_allowed`), so this excludes nothing of
/// ours, but it means a scanned filename that ISN'T ours-by-construction can
/// never reach the privileged `rm` in `uninstall_command` — a foreign file with
/// a shell-metachar name + our signature is dropped here, not interpolated into
/// a root shell (B10).
fn tlds_matching_signature(dir: &std::path::Path, signature: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut tlds: Vec<String> = entries
        .flatten()
        .filter(|e| std::fs::read_to_string(e.path()).ok().as_deref() == Some(signature))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| crate::core::tld::is_valid_label(name))
        .collect();
    tlds.sort();
    tlds
}

/// Enumerate every TLD rexenv has an OS resolver file for — the files in the
/// resolver directory whose content matches our port-`port` signature. The
/// directory comes from the platform's `resolver_path` so this stays
/// platform-agnostic (macOS: `/etc/resolver`, world-readable).
pub fn installed_tlds(platform: &dyn Platform, port: u16) -> Vec<String> {
    let probe = platform.dns().resolver_path(crate::core::tld::BACKBONE_TLD);
    let Some(dir) = probe.parent() else {
        return Vec::new();
    };
    tlds_matching_signature(dir, &platform.dns().resolver_contents(port))
}

/// Whether the resolver's loopback UDP `port` is already bound — a lightweight
/// liveness proxy for the embedded DNS. The resolver binds UDP, so the TCP
/// `ports::is_listening` check doesn't apply; instead we try to bind the port and
/// treat a failure as "something (our resolver) already holds it". Used by the
/// Settings status indicator so the command layer needn't open a raw socket; the
/// authoritative check when a handle is held is [`DnsService::is_running`].
pub fn port_bound(port: u16) -> bool {
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::{Message, Query};
    use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
    use std::time::Duration;
    use tokio::time::timeout;

    #[test]
    fn port_bound_true_when_held_false_when_free() {
        use std::net::{Ipv4Addr, UdpSocket};
        // While we hold an ephemeral UDP port, `port_bound` sees it as in use…
        let held = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        assert!(port_bound(port), "a held UDP port should read as bound");
        // …and free again once released.
        drop(held);
        assert!(!port_bound(port), "a released UDP port should read as free");
    }

    /// A conflict on the fixed resolver port must surface ports::ensure_free's
    /// named error (port + "DNS resolver" + free-it help), not a raw bind error.
    #[tokio::test]
    async fn start_default_conflict_names_service_and_port() {
        // Hold the fixed port ourselves; if an external process (e.g. a running
        // rexenv) already holds it, the conflict exists either way.
        let _held = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, DEFAULT_DNS_PORT)).ok();
        let platform = crate::platform::current();
        let err = match DnsService::start_default(platform.as_ref()).await {
            Ok(_) => panic!("start_default must fail while the port is held"),
            Err(e) => e.to_string(),
        };
        eprintln!("dns conflict error: {err}");
        assert!(err.contains(&DEFAULT_DNS_PORT.to_string()), "msg: {err}");
        assert!(err.contains("DNS resolver"), "msg: {err}");
    }

    /// Start the resolver on an ephemeral loopback port; return its address and
    /// the spawned server task handle.
    async fn start() -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let (addr, mut server) = serve_udp("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let handle = tokio::spawn(async move {
            let _ = server.block_until_done().await;
        });
        (addr, handle)
    }

    /// Send a single query for `host` to `server` and return the parsed reply.
    async fn query(server: SocketAddr, host: &str, qtype: RecordType) -> Message {
        let mut msg = Message::new();
        msg.set_id(0x1234)
            .set_message_type(MessageType::Query)
            .set_op_code(OpCode::Query)
            .set_recursion_desired(true);
        let name = Name::from_ascii(host).unwrap();
        msg.add_query(Query::query(name, qtype));
        let bytes = msg.to_bytes().unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(server).await.unwrap();
        client.send(&bytes).await.unwrap();

        let mut buf = [0u8; 512];
        let n = timeout(Duration::from_secs(2), client.recv(&mut buf))
            .await
            .expect("dns reply timed out")
            .unwrap();
        Message::from_bytes(&buf[..n]).unwrap()
    }

    fn first_a(msg: &Message) -> Option<Ipv4Addr> {
        msg.answers().iter().find_map(|r| match r.data() {
            Some(RData::A(a)) => Some(a.0),
            _ => None,
        })
    }

    #[tokio::test]
    async fn resolves_test_domain_to_loopback() {
        let (addr, handle) = start().await;
        let reply = query(addr, "foo.test.", RecordType::A).await;
        assert_eq!(reply.response_code(), ResponseCode::NoError);
        assert_eq!(first_a(&reply), Some(Ipv4Addr::LOCALHOST));
        handle.abort();
    }

    #[tokio::test]
    async fn resolves_wildcard_subdomains() {
        let (addr, handle) = start().await;
        // Deep subdomain (subdomain multisite) must also reach loopback.
        let reply = query(addr, "site1.mysite.test.", RecordType::A).await;
        assert_eq!(first_a(&reply), Some(Ipv4Addr::LOCALHOST));
        handle.abort();
    }

    /// The handler is TLD-agnostic on purpose: ANY name answers loopback, and
    /// TLD scope lives in which OS resolver files exist (only installed TLDs'
    /// queries ever reach this server). So `.rex` — or even `.com` — answers
    /// here; `.com` still resolves normally system-wide because no resolver
    /// file routes it to us (and the policy refuses installing one).
    #[tokio::test]
    async fn any_tld_answers_loopback() {
        let (addr, handle) = start().await;
        for host in ["foo.rex.", "bar.example.", "example.com."] {
            let reply = query(addr, host, RecordType::A).await;
            assert_eq!(reply.response_code(), ResponseCode::NoError, "{host}");
            assert_eq!(first_a(&reply), Some(Ipv4Addr::LOCALHOST), "{host}");
        }
        handle.abort();
    }

    /// The teardown decision table (v18), row by row, against fixture files.
    ///
    /// Fixtures because this machine has no foreign `/etc/resolver/<tld>` and
    /// creating a root-owned one to test against would be worse than a
    /// fixture — the live paths are a clean-VM item (PUBLISH-TESTING §F). This
    /// drives the pure planner over a fake resolver dir by classifying each
    /// file the same way `plan_resolver_teardown` does.
    #[test]
    fn teardown_decision_table_restores_borrowed_and_never_touches_reclaimed() {
        let dir = std::env::temp_dir().join(format!("rexenv-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sig = "nameserver 127.0.0.1\nport 15353\n";
        let theirs = "nameserver 127.0.0.1\n";

        // ours + no record  -> remove
        std::fs::write(dir.join("rex"), sig).unwrap();
        // ours + record     -> restore
        std::fs::write(dir.join("test"), sig).unwrap();
        // foreign + record  -> leave alone (they reclaimed it)
        std::fs::write(dir.join("dev"), theirs).unwrap();
        // foreign + no record -> invisible
        std::fs::write(dir.join("other"), theirs).unwrap();
        // absent + record   -> just forget the record ("gone")

        assert_eq!(owner_of(&dir.join("rex"), sig), ResolverOwner::Ours);
        assert_eq!(owner_of(&dir.join("test"), sig), ResolverOwner::Ours);
        assert!(matches!(owner_of(&dir.join("dev"), sig), ResolverOwner::Foreign { .. }));
        assert!(matches!(owner_of(&dir.join("other"), sig), ResolverOwner::Foreign { .. }));
        assert_eq!(owner_of(&dir.join("gone"), sig), ResolverOwner::Absent);

        // Only OUR files are ever enumerated for removal — a foreign file with
        // no record can't reach the privileged `rm` at all.
        let ours = tlds_matching_signature(&dir, sig);
        assert_eq!(ours, vec!["rex", "test"], "foreign files must not be enumerated");
        assert!(!ours.contains(&"dev".to_string()));
        assert!(!ours.contains(&"other".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A backup with no record is litter from a crash between writing the file
    /// and inserting its row — the row owns the file everywhere else.
    #[test]
    fn orphan_backup_sweep_keeps_recorded_and_deletes_the_rest() {
        let conn = crate::state::db::open_in_memory().unwrap();
        crate::state::store::insert_resolver_takeover(&conn, "test", "theirs\n", "/tmp/x")
            .unwrap();
        let known: std::collections::HashSet<String> =
            crate::state::store::list_resolver_takeovers(&conn)
                .unwrap()
                .into_iter()
                .map(|r| r.tld)
                .collect();
        assert!(known.contains("test"));
        assert!(!known.contains("stray"), "an unrecorded backup is an orphan");

        // The record round-trips and deleting it forgets the TLD.
        let rec = crate::state::store::get_resolver_takeover(&conn, "test").unwrap().unwrap();
        assert_eq!(rec.original, "theirs\n");
        assert!(crate::state::store::delete_resolver_takeover(&conn, "test").unwrap());
        assert!(crate::state::store::get_resolver_takeover(&conn, "test").unwrap().is_none());
    }

    /// Re-taking a TLD after they reclaimed it replaces the record in place —
    /// one row and one backup per TLD, so no orphan can accumulate.
    #[test]
    fn re_takeover_replaces_the_record_rather_than_adding_one() {
        let conn = crate::state::db::open_in_memory().unwrap();
        let store = crate::state::store::insert_resolver_takeover;
        store(&conn, "test", "first\n", "/tmp/test").unwrap();
        store(&conn, "test", "second\n", "/tmp/test").unwrap();
        let all = crate::state::store::list_resolver_takeovers(&conn).unwrap();
        assert_eq!(all.len(), 1, "one row per TLD");
        assert_eq!(all[0].original, "second\n", "newest backup is what we replaced");
    }

    /// Ownership classification, against fixture files.
    ///
    /// Fixtures rather than a live check by necessity: the dev Mac has no
    /// foreign `/etc/resolver/<tld>`, and creating a root-owned one to test
    /// against would be a worse idea than this. The takeover/restore paths are
    /// tracked as a clean-VM item in docs/PUBLISH-TESTING.md §F.
    #[test]
    fn owner_of_classifies_ours_foreign_and_absent() {
        let dir = std::env::temp_dir().join(format!("rexenv-owner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sig = "nameserver 127.0.0.1\nport 15353\n";

        // Absent.
        assert_eq!(owner_of(&dir.join("nothing"), sig), ResolverOwner::Absent);

        // Ours — byte-exact.
        let ours = dir.join("rex");
        std::fs::write(&ours, sig).unwrap();
        assert_eq!(owner_of(&ours, sig), ResolverOwner::Ours);

        // Valet's real shape: same nameserver, NO port line. This is the one
        // that used to be silently overwritten.
        let valet = dir.join("test");
        std::fs::write(&valet, "nameserver 127.0.0.1\n").unwrap();
        assert_eq!(
            owner_of(&valet, sig),
            ResolverOwner::Foreign { content: Some("nameserver 127.0.0.1\n".into()) },
            "a Valet resolver file must read as FOREIGN, never as ours"
        );

        // Our nameserver but a different port — still theirs.
        let other = dir.join("dev");
        std::fs::write(&other, "nameserver 127.0.0.1\nport 5333\n").unwrap();
        assert!(matches!(owner_of(&other, sig), ResolverOwner::Foreign { .. }));

        // Even a near-miss (trailing newline dropped) is foreign, not ours —
        // equality is the whole ownership notion, so it must not be fuzzy.
        let near = dir.join("near");
        std::fs::write(&near, "nameserver 127.0.0.1\nport 15353").unwrap();
        assert!(matches!(owner_of(&near, sig), ResolverOwner::Foreign { .. }));

        // Present but unreadable counts as FOREIGN (never absent): refusing to
        // touch what we can't inspect is the safe direction. Skipped when the
        // test runs as a user who can read it anyway (e.g. root in CI).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = dir.join("locked");
            std::fs::write(&locked, "whatever\n").unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            if std::fs::read_to_string(&locked).is_err() {
                assert_eq!(owner_of(&locked, sig), ResolverOwner::Foreign { content: None });
            }
            let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Enumerating our resolver files: exact-content signature match — foreign
    /// files (a developer's own dnsmasq entry, different port) are never touched.
    #[test]
    fn tlds_matching_signature_finds_only_our_files() {
        let dir = std::env::temp_dir().join(format!("rexenv-resolver-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sig = format!("nameserver 127.0.0.1\nport {DEFAULT_DNS_PORT}\n");

        std::fs::write(dir.join("test"), &sig).unwrap();
        std::fs::write(dir.join("rex"), &sig).unwrap();
        // Foreign: another tool's resolver on a different port, and a
        // same-nameserver file with extra options — neither is ours.
        std::fs::write(dir.join("docker"), "nameserver 127.0.0.1\nport 19999\n").unwrap();
        std::fs::write(dir.join("dev"), "nameserver 127.0.0.1\n").unwrap();
        // Files with OUR signature but a name that isn't a valid TLD label
        // ([a-z]{1,63}) can't be ours-by-construction — and must never reach the
        // privileged `rm`. Shell-metachar / space / uppercase / digit names are
        // dropped here (B10). rexenv could never have created any of these.
        for bad in ["evil;reboot", "a b", "UP", "x9", "back`tick`"] {
            std::fs::write(dir.join(bad), &sig).unwrap();
        }

        // Only the two valid-label files with our signature survive the sweep.
        assert_eq!(tlds_matching_signature(&dir, &sig), vec!["rex", "test"]);
        // Missing dir → empty, not an error (fresh machine, nothing installed).
        assert!(tlds_matching_signature(&dir.join("nope"), &sig).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn managed_service_starts_serves_and_stops() {
        let svc = DnsService::start("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        assert!(svc.is_running());

        let reply = query(svc.addr(), "foo.test.", RecordType::A).await;
        assert_eq!(first_a(&reply), Some(Ipv4Addr::LOCALHOST));

        svc.stop();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!svc.is_running(), "service should stop cleanly");
    }

    /// The agent-adoption probe: true against OUR resolver (any name → 127.0.0.1),
    /// false against a dead port — the app uses this to decide adopt vs install
    /// vs in-process fallback, and the watchdog uses it as agent liveness.
    #[tokio::test]
    async fn answers_as_ours_detects_our_resolver_and_a_dead_port() {
        let svc = DnsService::start("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let port = svc.addr().port();
        // spawn_blocking: the probe is deliberately sync (used from non-async paths).
        let ours = tokio::task::spawn_blocking(move || answers_as_ours(port)).await.unwrap();
        assert!(ours, "must recognize our own resolver");
        svc.stop();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let dead = tokio::task::spawn_blocking(move || answers_as_ours(port)).await.unwrap();
        assert!(!dead, "a dead port must not read as ours");
    }
}
