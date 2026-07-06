//! core::dns — embedded DNS resolver (Phase 1 task 2.1).
//!
//! Answers A queries for any `*.test` host with `127.0.0.1`. Because every name
//! under `.test` resolves to loopback, WordPress subdomain multisite
//! (`*.mysite.test`) works for free. This is platform-agnostic; pointing the OS
//! resolver at this server is the per-OS `DnsManager` step (task 2.2).

use crate::error::Result;
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
/// OS resolver config (task 2.2) points `.test` lookups here.
pub const DEFAULT_DNS_PORT: u16 = 15353;

/// The local development TLD this resolver is authoritative for.
pub const LOCAL_TLD: &str = "test";

/// TTL (seconds) on answers. Short, since these are local and may change.
const ANSWER_TTL: u32 = 60;

/// Request handler that maps `*.test` → a single loopback address.
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
        let under_tld = name_under_tld(&name, LOCAL_TLD);

        let mut header = Header::response_from_request(request.header());
        header.set_authoritative(true);

        let mut answers: Vec<Record> = Vec::new();
        if !is_query {
            header.set_response_code(ResponseCode::Refused);
        } else if !under_tld {
            // We are not authoritative for anything outside `.test`.
            header.set_response_code(ResponseCode::NXDomain);
        } else if qtype == RecordType::A {
            answers.push(Record::from_rdata(
                name.clone(),
                ANSWER_TTL,
                RData::A(A(self.answer)),
            ));
        }
        // Under `.test` but non-A (e.g. AAAA): NOERROR with no records, so
        // clients fall back to the A record instead of failing.

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

/// True if `name`'s last label equals `tld` (case-insensitive). Matches the TLD
/// itself and every subdomain of it (`foo.test`, `a.b.mysite.test`).
fn name_under_tld(name: &Name, tld: &str) -> bool {
    name.iter()
        .next_back()
        .is_some_and(|label| label.eq_ignore_ascii_case(tld.as_bytes()))
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

/// Install the `.test` OS resolver file (pointing at our resolver on `port`)
/// through `PrivilegeManager` — one auth prompt. Standalone helper; the batched
/// system-setup step (3.4) instead concatenates this with the CA-trust command
/// to share a single prompt.
pub fn configure_resolver(platform: &dyn Platform, port: u16) -> Result<()> {
    let cmd = platform.dns().install_command(port);
    platform.privileges().run_privileged(&cmd)?;
    Ok(())
}

/// Remove the `.test` OS resolver file through `PrivilegeManager`.
pub fn remove_resolver(platform: &dyn Platform) -> Result<()> {
    let cmd = platform.dns().uninstall_command();
    platform.privileges().run_privileged(&cmd)?;
    Ok(())
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

    #[tokio::test]
    async fn non_test_domain_is_nxdomain() {
        let (addr, handle) = start().await;
        let reply = query(addr, "example.com.", RecordType::A).await;
        assert_eq!(reply.response_code(), ResponseCode::NXDomain);
        assert!(first_a(&reply).is_none());
        handle.abort();
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
}
