//! Manual check for the app service manager (task 10.5). The ServiceManager owns
//! the whole stack: it starts MySQL + php-fpm + nginx + Caddy (real :443), serves
//! a provisioned site, reports per-service status + live metrics, then stops.
//! Caddy on :443 prompts once for admin (privileged bind).

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{binaries, monitor::Monitor, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::time::Duration;

mod common;

// High ports so this runs unattended (no privileged :443 admin prompt). The
// privileged :443 path is the same start_privileged proven in task 4.2; pass
// `real443` as an arg to use :443 (will prompt — run in the foreground).
#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Deliberate real-stack control: this utility exists to adopt/stop the
    // shared stack. Without this, core::stack_guard skips adopted services.
    rexenv_lib::core::stack_guard::allow_real_stack_control();
    let plat = platform::current();
    let domain = "mgrdemo.test";
    let real443 = std::env::args().any(|a| a == "real443");
    let https_port: u16 = if real443 { 443 } else { 8443 };
    let ports = Ports {
        http: if real443 { 80 } else { 8080 },
        https: https_port,
        nginx: services::NGINX_HTTP_PORT,
    };

    let conn = {
        let p = std::env::temp_dir().join("rexenv-10_5.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Manager Demo".into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
        },
    )
    .unwrap();

    let mut mgr = ServiceManager::with_ports(ports);
    println!("starting stack (Caddy on :{https_port})…");
    let all = sites::list(&conn).unwrap();
    let php_minors = rexenv_lib::core::php::installed_minors(&conn).unwrap();
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &php_minors, binaries::ADMINER_VERSION).await {
        eprintln!("start_all failed: {e}");
        // `FAILURE`, never a bare `return` — a bare return from `main` exits 0 and the
        // tier records a run that asserted nothing as green (common/mod.rs, the
        // verdict contract).
        return std::process::ExitCode::FAILURE;
    }

    // `start_all` returns without awaiting the EDGE — its ReadyChecks cover the
    // databases, mailpit and the FrankenPHP overrides only — and the HTTPS
    // request further down needs it.
    common::await_listening(https_port, "the caddy edge", None);

    println!("\n=== service status + live metrics ===");
    let mut mon = Monitor::new();
    mon.refresh_processes();
    std::thread::sleep(Duration::from_millis(400));
    mon.refresh_processes();
    for s in mgr.status(&*plat, &[]) {
        let met = s.pid.and_then(|p| mon.tree(p));
        let metric = met
            .map(|m| format!("cpu {:.1}% ram {} MB", m.cpu_percent, m.ram_mb))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<8} running={:<5} pid={:<7} port={:<6} {}",
            s.name,
            s.running,
            s.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            s.port,
            metric
        );
    }

    // Serve check (validated against our CA).
    let url = if real443 {
        format!("https://{domain}/")
    } else {
        format!("https://{domain}:{https_port}/")
    };
    let addr: SocketAddr = format!("127.0.0.1:{https_port}").parse().unwrap();
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(domain, addr)
        .build()
        .unwrap();
    match client.get(&url).send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            println!("\nGET {url} -> HTTP {code} (phpinfo: {})", body.contains("phpinfo()"));
        }
        Err(e) => println!("\nrequest error: {e}"),
    }

    mgr.stop_all(&*plat).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    println!(
        "stopped; :{https_port} free now = {}",
        rexenv_lib::core::ports::is_free(https_port, rexenv_lib::core::ports::Proto::Tcp)
    );
    std::process::ExitCode::SUCCESS
}
