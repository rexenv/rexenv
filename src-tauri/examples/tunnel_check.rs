//! Phase-3 §9.1 check: cloudflared per-site quick tunnel (NETWORK REQUIRED).
//! Brings up the stack + a site, opens a quick tunnel whose origin is the shared
//! nginx port with the site Host, parses the public `trycloudflare.com` URL, then
//! fetches it from the public internet and asserts it serves the LOCAL site.
//! Finally stops the tunnel.
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free, internet up):
//!   `cargo run --example tunnel_check`

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{binaries, services, sites, ssl, tunnels};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "share.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-9_1.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Share".into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
        },
    )
    .unwrap();

    let mut mgr = ServiceManager::with_ports(Ports { http: 8080, https: 8443, nginx: services::NGINX_HTTP_PORT });
    let all = sites::list(&conn).unwrap();
    let php_minors = rexenv_lib::core::php::installed_minors(&conn).unwrap();
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &php_minors).await {
        eprintln!("start_all failed: {e}");
        return;
    }

    // Resolve cloudflared + start the quick tunnel.
    let bin = binaries::resolve(&*plat, "cloudflared", binaries::CLOUDFLARED_VERSION).await.unwrap();
    println!("✓ cloudflared resolved: {}", bin.display());
    let mut child = tunnels::start(&*plat, &bin, domain, services::NGINX_HTTP_PORT).unwrap();

    // Parse the public URL from cloudflared's log.
    let mut url = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Some(u) = tunnels::read_url(&*plat, domain) {
            url = Some(u);
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let url = url.expect("no trycloudflare URL parsed");
    println!("✓ public URL: {url}");
    assert!(url.ends_with(".trycloudflare.com"));

    // Origin sanity check: the local nginx must serve the site with its Host.
    {
        let origin = reqwest::Client::new();
        let r = origin
            .get(format!("http://127.0.0.1:{}/", services::NGINX_HTTP_PORT))
            .header("Host", domain)
            .send()
            .await;
        match r {
            Ok(resp) => {
                let code = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                println!("origin nginx :{} Host={domain} → {code} (phpinfo={})", services::NGINX_HTTP_PORT, body.contains("phpinfo()"));
            }
            Err(e) => println!("origin nginx request error: {e}"),
        }
    }

    // Fetch the public URL from OUTSIDE (real Cloudflare edge → our local origin).
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().unwrap();
    let mut served = false;
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut attempt = 0;
    while Instant::now() < deadline {
        attempt += 1;
        match client.get(&url).send().await {
            Ok(resp) => {
                let code = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                let hit = body.contains("phpinfo()") || body.contains("PHP Version");
                println!("attempt {attempt}: {code} ({} bytes, phpinfo={hit})", body.len());
                if (200..300).contains(&code) && hit {
                    served = true;
                    break;
                }
            }
            Err(e) => println!("attempt {attempt}: error {e}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert!(served, "public URL did not serve the local site");
    println!("✓ public URL serves the LOCAL site (phpinfo) from outside");

    // Stop the tunnel; it should stop resolving shortly after.
    tunnels::stop(&*plat, child.id()).unwrap();
    let _ = child.wait();
    println!("✓ tunnel stopped");

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — per-site quick tunnel shares the local site publicly, scoped to its Host.");
}
