//! Seed the real app DB with a couple of sites and print what `list_sites`
//! returns (task 7.2). After running this, the desktop app's Sites screen shows
//! these real sites. Idempotent (skips provisioning if the domain exists).

use rexenv_lib::core::{sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::models::{NewSite, ServiceStatus, SiteType, WebServer};
use rexenv_lib::state::{db, store};

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let conn = db::open_for_platform(plat.paths()).expect("open db");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");

    let seeds = [
        ("Demo One", "demo-one.test", SiteType::Php, true),
        ("Demo Two", "demo-two.test", SiteType::Wordpress, false),
    ];
    for (name, domain, ty, run) in seeds {
        let site = if store::domain_exists(&conn, domain).unwrap() {
            sites::list(&conn)
                .unwrap()
                .into_iter()
                .find(|s| s.domain == domain)
                .unwrap()
        } else {
            sites::provision(
                &conn,
                &*plat,
                &ca,
                NewSite {
                    name: name.into(),
                    domain: domain.into(),
                    site_type: ty,
                    php_version: "8.3".into(),
                    web_server: WebServer::Nginx,
                    path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
                },
            )
            .unwrap()
        };
        let status = if run {
            ServiceStatus::Running
        } else {
            ServiceStatus::Stopped
        };
        sites::set_status(&conn, &site.id, status).unwrap();
    }

    println!("=== list_sites() returns ===");
    for s in sites::list(&conn).unwrap() {
        println!(
            "{:<10} {:<16} php{} {:<10} {:?}",
            s.name, s.domain, s.php_version, format!("{:?}", s.site_type), s.status
        );
    }
}
