//! Live check for LINKED sites (Stage 0): serving a project that lives outside
//! the rexenv sites folder, in place. Run:
//! `cargo run --example linked_site_check`
//!
//! Proves the promise end to end, with throwaway processes on fixture ports:
//!   1. `provision` with a caller-supplied path LINKS the folder — it records
//!      the path we gave it instead of creating `<sites_dir>/<domain>`, and
//!      writes nothing into it (no `index.php` of ours appears).
//!   2. The site serves THEIR file from where it already lives.
//!   3. The row records `docroot_managed = false`.
//!   4. `check_docroot_move` refuses to relocate it (a cross-volume move copies
//!      then DELETES the source — that would rewrite the user's own layout).
//!   5. **Deleting the site leaves the folder and its contents untouched**,
//!      while the row and the certificate do go away.

use rexenv_lib::core::{binaries, services, sites, ssl};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::time::Duration;

mod common;
use common::Reaped;

/// Fixture port — deliberately NOT the shared stack's 18088, so a running
/// rexenv is neither contended nor swept.
///
/// This check probes the shared NGINX directly rather than going through Caddy
/// and php-fpm: the question is whether the vhost's document root points at the
/// user's folder, which a static request answers exactly. It also keeps the
/// check runnable while the real stack is up — a second Caddy would contend for
/// the edge's admin socket, and the 8.3 pool port belongs to the running stack.
const NGINX_PORT: u16 = 18099;
const DOMAIN: &str = "linked-check.test";
/// Distinctive marker so the response can only have come from THEIR file.
const MARKER: &str = "SERVED-FROM-THE-USERS-OWN-FOLDER";

#[tokio::main]
async fn main() {
    // Sandboxed: every path the app derives (config dir, nginx PREFIX and
    // therefore nginx.pid, run/, certs) lands in a throwaway root, so this
    // example cannot touch the running stack. See examples/common.
    let (plat, _sandbox) = common::sandbox("linked_site_check");
    let mut ok = true;

    // A project living where the user keeps their code — outside the sites dir.
    let project = std::env::temp_dir().join("rexenv-linked-check-project");
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("marker.txt"), MARKER).unwrap();
    std::fs::write(project.join("index.php"), "<?php echo 'theirs';").unwrap();
    std::fs::write(project.join("notes.txt"), "the user's own file\n").unwrap();
    let before: Vec<String> = entries(&project);

    let db_path = std::env::temp_dir().join("rexenv-linked-check.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("db");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");

    println!("=== provision with a caller path (link) ===");
    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Linked".into(),
            domain: DOMAIN.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            // The whole point: a non-empty path means "serve THIS folder".
            path: project.display().to_string(),
            db_engine: SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
        },
    )
    .expect("provision linked");

    let canon = project.canonicalize().unwrap();
    let path_kept = std::path::Path::new(&site.path) == canon;
    let owned = site.docroot_managed;
    println!("  stored path == the folder we passed : {path_kept}");
    println!("  docroot_managed                     : {owned:?} (want Some(false))");
    println!("  folder untouched by provisioning    : {}", entries(&project) == before);
    ok &= path_kept && owned == Some(false) && entries(&project) == before;

    // A folder we don't own is not ours to relocate.
    let move_refused = sites::check_docroot_move(&site, &std::env::temp_dir()).is_err();
    println!("  move refused                        : {move_refused}");
    ok &= move_refused;

    println!("\n=== serve it from where it lives ===");
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, 8081, 8444).unwrap();
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    // Drop-guarded: every check below can panic, and a leaked master's workers
    // would keep the port (examples/common).
    let mut nginx = Reaped::new(
        services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap(),
        NGINX_PORT,
        "nginx",
    );
    tokio::time::sleep(Duration::from_millis(800)).await;

    let client = reqwest::Client::new();
    let body = match client
        .get(format!("http://127.0.0.1:{NGINX_PORT}/marker.txt"))
        .header("Host", DOMAIN)
        .send()
        .await
    {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(e) => format!("ERROR: {e}"),
    };
    let served = body.contains(MARKER);
    println!("  GET /marker.txt via the {DOMAIN} vhost  : {served}");
    ok &= served;

    println!("\n=== delete the site ===");
    let out = sites::teardown(&conn, &*plat, &site.id).unwrap();
    let cert_gone =
        !ssl::site_cert_dir(plat.paths(), DOMAIN).map(|d| d.exists()).unwrap_or(false);
    let after = entries(&project);
    println!("  reported docroot_removed            : {} (want false)", out.docroot_removed);
    println!("  row gone                            : {}", sites::get(&conn, &site.id).unwrap().is_none());
    println!("  certificate gone                    : {cert_gone}");
    println!("  THEIR folder still exists           : {}", project.exists());
    println!("  THEIR files untouched               : {}", after == before);
    ok &= !out.docroot_removed
        && out.existed
        && sites::get(&conn, &site.id).unwrap().is_none()
        && cert_gone
        && project.exists()
        && after == before;

    // Explicit: `exit` below skips destructors (the guards remain the backstop).
    nginx.reap();
    let _ = std::fs::remove_dir_all(&project); // ours: the fixture we created
    let _ = std::fs::remove_file(&db_path);

    if ok {
        println!("\nOK — a linked folder is served in place, refuses to be moved, and survives deleting the site.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

/// Sorted file names in `dir`, to prove nothing was added or removed.
fn entries(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into()).collect())
        .unwrap_or_default();
    v.sort();
    v
}
