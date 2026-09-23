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
    // Pin `sites_dir` into the sandbox: `sites::provision` reads the SETTING,
    // which falls back to the home directory, so without this the docroots land
    // in the user's real ~/rexenv/Sites (14 Aug 2026 sweep).
    common::pin_sites_dir(&conn, &*plat);
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
            git_build_assets: false,
            starter_db: false,
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
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::pins().nginx).await.unwrap();
    // Drop-guarded: every check below can panic, and a leaked master's workers
    // would keep the port (examples/common).
    let mut nginx = Reaped::new(
        services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap(),
        NGINX_PORT,
        "nginx",
    );
    // Readiness, not a timer — see `common::await_listening` for the incident
    // this pattern produced (a flat sleep loses under CPU contention, and the
    // failure then reads as the SERVER being broken rather than as too-early).
    common::await_listening(NGINX_PORT, "nginx", None);

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

    println!("\n=== re-point after the user moved the folder (#242's serving half) ===");
    // The user relocated their project. The shape + preflight are lib-proven;
    // this is the half only a real nginx can prove: after `set_path` +
    // rebuild + the PRODUCTION reload, the vhost actually serves from the
    // moved folder.
    let moved = std::env::temp_dir().join("rexenv-linked-check-moved");
    let _ = std::fs::remove_dir_all(&moved);
    std::fs::rename(&project, &moved).unwrap();

    // Control: the vhost still points at the OLD path, which no longer
    // exists — the marker must STOP being served, or the reload below could
    // pass on a config nobody rebuilt.
    //
    // The claim is "not served", and it is asserted as that rather than as a
    // STATUS CODE — because the code here belongs to a process this example
    // does not own. The vhost is `try_files $uri $uri/ /index.php` over
    // `fastcgi_pass 127.0.0.1:9783`, so a missing file falls through to the
    // shared 8.3 POOL: with the user's stack up that pool answers "no input
    // file" (404), and with it stopped nothing answers (502). This asserted
    // 404, so it passed only while the user's stack happened to be running —
    // a sandbox-tier check resting on the machine's state, and reaching into
    // the real pool to do it. Found 20 Aug 2026 by running the tier with the
    // stack down, which is the configuration the tier is supposed to be
    // INDEPENDENT of.
    let stale = client
        .get(format!("http://127.0.0.1:{NGINX_PORT}/marker.txt"))
        .header("Host", DOMAIN)
        .send()
        .await;
    let (stale_code, stale_body) = match stale {
        Ok(r) => {
            let code = r.status().as_u16();
            (code, r.text().await.unwrap_or_default())
        }
        Err(_) => (0, String::new()),
    };
    // Two halves, because a status alone can lie in both directions: a 200
    // serving the marker is the failure, and so is a 200 serving anything at
    // all from a root that is gone.
    let gone = stale_code != 200 && !stale_body.contains(MARKER);
    println!(
        "  after the move, before re-point: /marker.txt -> {stale_code}, marker present: {} (want not-served)",
        stale_body.contains(MARKER)
    );
    ok &= gone;

    // A marker that exists ONLY post-move, so serving it can only mean the
    // root really is the new folder — not a cache, not the old tree.
    const MOVED_MARKER: &str = "SERVED-FROM-THE-MOVED-FOLDER";
    std::fs::write(moved.join("moved.txt"), MOVED_MARKER).unwrap();

    // The refusals the preflight promises, at the real fixture:
    ok &= sites::check_docroot_relink(&site, &std::env::temp_dir().join("rexenv-no-such-dir"))
        .is_err(); // not a folder on disk
    ok &= sites::check_docroot_relink(&site, std::path::Path::new(&site.path)).is_err(); // same folder

    // The command's core sequence: check → set_path (RECORDS, never writes
    // files) → rebuild → the PRODUCTION reload path.
    sites::check_docroot_relink(&site, &moved).expect("relink preflight");
    let site = sites::set_path(&conn, &*plat, &site.id, &moved)
        .expect("set_path")
        .expect("site exists");
    ok &= site.docroot_managed == Some(false); // still theirs, still never deletable
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, 8081, 8444).unwrap();
    let outcome =
        services::reload_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix, NGINX_PORT)
            .expect("reload nginx");
    println!("  reload outcome: {outcome:?}");
    tokio::time::sleep(Duration::from_millis(500)).await;

    let served_moved = client
        .get(format!("http://127.0.0.1:{NGINX_PORT}/moved.txt"))
        .header("Host", DOMAIN)
        .send()
        .await
        .map(|r| r.text())
        .unwrap()
        .await
        .unwrap_or_default()
        .contains(MOVED_MARKER);
    let old_marker_travelled = client
        .get(format!("http://127.0.0.1:{NGINX_PORT}/marker.txt"))
        .header("Host", DOMAIN)
        .send()
        .await
        .map(|r| r.text())
        .unwrap()
        .await
        .unwrap_or_default()
        .contains(MARKER);
    println!("  the post-move-only marker serves    : {served_moved}");
    println!("  the user's files travelled and serve: {old_marker_travelled}");
    ok &= served_moved && old_marker_travelled;

    // Delete-leg bookkeeping follows the folder: the site now IS `moved`.
    let project = moved;
    let before: Vec<String> = entries(&project);

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
