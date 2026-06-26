//! Throwaway manual check for the embedded resolver (task 2.1).
//! Run: `cargo run --example dns_serve` then `dig @127.0.0.1 -p 15353 foo.test`.

use rexenv_lib::core::dns;

#[tokio::main]
async fn main() {
    let addr = format!("127.0.0.1:{}", dns::DEFAULT_DNS_PORT)
        .parse()
        .unwrap();
    let (local, mut server) = dns::serve_udp(addr).await.expect("bind resolver");
    eprintln!("rexenv embedded resolver listening on {local} (udp)");
    server.block_until_done().await.expect("server run");
}
