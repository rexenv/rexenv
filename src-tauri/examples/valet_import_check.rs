//! Live check for the IMPORT chain (Stage 1). Run:
//! `cargo run --example valet_import_check`
//!
//! Drives scan → enrich → link → serve → delete against a FIXTURE Valet tree,
//! on `.rex` so the machine's real `.test` setup is never involved and no
//! resolver file is touched (`.rex` is already ours from onboarding).
//!
//! Proves, in order:
//!   1. the scan classifies a fixture Valet site and reads its isolated PHP;
//!   2. the enrichment resolves the docroot a framework actually serves;
//!   3. importing LINKS that folder — the stored path is theirs, nothing is
//!      created inside it, and `docroot_managed` records that it isn't ours;
//!   4. the site really serves THEIR file;
//!   5. deleting it removes rexenv's own state and leaves their folder intact;
//!   6. the fixture Valet tree is byte-identical throughout.
//!
//! Uses a throwaway database and a fixture nginx port, so the running stack and
//! the real site list are untouched.

use rexenv_lib::core::{binaries, services, sites, ssl, valet};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, WebServer};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod common;
use common::Reaped;

const NGINX_PORT: u16 = 18099;
const DOMAIN: &str = "importcheck.rex";
/// An extra name the site answers on (v42) — the fixture tree links the SAME
/// folder under this name too (a link farm), so the scan must surface it as a
/// second row on one folder, and the imported site must answer on it.
const ALIAS: &str = "importcheck-www.rex";
const MARKER: &str = "IMPORTED-FROM-THEIR-OWN-FOLDER";

fn fingerprint(dir: &Path) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(md) = std::fs::symlink_metadata(&p) else { continue };
            let rel = p.strip_prefix(dir).unwrap_or(&p).display().to_string();
            if md.is_dir() {
                stack.push(p);
                out.insert(format!("{rel}/"), 0);
            } else {
                out.insert(rel, md.len());
            }
        }
    }
    out
}

#[tokio::main]
async fn main() {
    // Sandboxed: every path the app derives (config dir, nginx PREFIX and
    // therefore nginx.pid, run/, certs) lands in a throwaway root, so this
    // example cannot touch the running stack. See examples/common.
    let (plat, _sandbox) = common::sandbox("valet_import_check");
    let mut ok = true;

    // ── a fixture Valet environment: a linked site whose docroot is public/ ──
    let base = std::env::temp_dir().join("rexenv-import-check");
    let _ = std::fs::remove_dir_all(&base);
    let valet_home = base.join(".config/valet");
    let project = base.join("code/importcheck");
    std::fs::create_dir_all(valet_home.join("Sites")).unwrap();
    std::fs::create_dir_all(valet_home.join("Nginx")).unwrap();
    std::fs::create_dir_all(project.join("public")).unwrap();
    // A Laravel-shaped project: the docroot is public/, NOT the project root.
    std::fs::write(project.join("artisan"), "#!/usr/bin/env php\n").unwrap();
    std::fs::write(project.join("public/index.php"), "<?php echo 'php';").unwrap();
    std::fs::write(project.join("public/marker.txt"), MARKER).unwrap();
    std::fs::write(project.join(".env"), "APP_KEY=secret-not-read\n").unwrap();
    common::symlink(&project, valet_home.join("Sites/importcheck")).unwrap();
    // The link farm: `valet link importcheck-www` run in the same project.
    common::symlink(&project, valet_home.join("Sites/importcheck-www")).unwrap();
    std::fs::write(
        valet_home.join("config.json"),
        r#"{"tld":"rex","loopback":"127.0.0.1","paths":["/nonexistent"]}"#,
    )
    .unwrap();
    // Their isolate pin, in the bare-digit form Valet writes after a secure.
    std::fs::write(
        valet_home.join("Nginx/importcheck.rex"),
        "# ISOLATED_PHP_VERSION=83\nserver { }\n",
    )
    .unwrap();
    let tree_before = fingerprint(&valet_home);
    let project_before = fingerprint(&project);

    println!("=== 1. scan the fixture Valet tree ===");
    let (source, found) = valet::scan_source(valet::SourceKind::Valet, &valet_home);
    let site = found.iter().find(|s| s.domain == DOMAIN).expect("the site is discovered");
    let scanned_ok = source.tld == "rex"
        && matches!(site.status, valet::SiteStatus::Importable)
        && site.php_minor.as_deref() == Some("8.3");
    println!(
        "  domain={} status={:?} php={:?} (bare-digit marker read)",
        site.domain, site.status, site.php_minor
    );
    ok &= scanned_ok;
    // Both farm names reach the scan as rows on ONE folder — the input shape
    // `fold_same_folder` turns into one site with an extra domain (the fold
    // itself is L0: `same_folder_rows_become_one_row_with_extra_domains`).
    let farm_ok = found
        .iter()
        .find(|s| s.domain == ALIAS)
        .is_some_and(|s| s.path.is_some() && s.path == site.path);
    println!("  link farm: {ALIAS} is a second row on the same folder: {farm_ok}");
    ok &= farm_ok;

    println!("\n=== 2. enrich: which folder would we actually serve? ===");
    let root = PathBuf::from(site.path.clone().expect("target exists"));
    let detected = sites::detect_project(&root);
    let serve = root.join(&detected.docroot_rel);
    println!("  detected={} docroot_rel={:?}", detected.label, detected.docroot_rel);
    println!("  would serve {}", serve.display());
    let enriched_ok = detected.label == "Laravel" && detected.docroot_rel == "public";
    ok &= enriched_ok;

    println!("\n=== 3. import (link, never copy) ===");
    let db_path = std::env::temp_dir().join("rexenv-import-check.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("db");
    // Pin `sites_dir` into the sandbox: `sites::provision` reads the SETTING,
    // which falls back to the home directory, so without this the docroots land
    // in the user's real ~/rexenv/Sites (14 Aug 2026 sweep).
    common::pin_sites_dir(&conn, &*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let created = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: site.name.clone(),
            domain: DOMAIN.into(),
            site_type: detected.site_type,
            php_version: site.php_minor.clone().unwrap_or_else(|| "8.3".into()),
            web_server: WebServer::Nginx,
            path: serve.display().to_string(),
            db_engine: SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .expect("provision the linked site");
    let linked_ok = Path::new(&created.path) == serve.canonicalize().unwrap().as_path()
        && created.docroot_managed == Some(false)
        && fingerprint(&project) == project_before;
    println!("  stored path      : {}", created.path);
    println!("  docroot_managed  : {:?} (want Some(false))", created.docroot_managed);
    println!("  their folder      untouched by import: {}", fingerprint(&project) == project_before);
    ok &= linked_ok;

    println!("\n=== 3b. an extra name is served by the MIRROR the manager holds, not the table ===");
    // The stale-mirror shape, reproduced: production regenerates configs from
    // `ServiceManager::site_aliases` (`rebuild_configs_for`), while the DB path
    // (`rebuild_configs`) is what every example calls — so no example could
    // catch an alias that was recorded and never pushed to the manager (the
    // Valet import did exactly that, 3 Sep 2026). Here the alias is recorded,
    // then the configs are built with an EMPTY mirror and must NOT serve it,
    // and with the table's map and must.
    sites::add_alias(&conn, &created.id, ALIAS).expect("record the extra name");
    let stale = sites::rebuild_configs_for(
        &sites::list(&conn).unwrap(),
        &*plat,
        &ca,
        NGINX_PORT,
        8081,
        8444,
        &Default::default(),
        &Default::default(),
        &std::collections::HashMap::new(),
    )
    .unwrap();
    let stale_conf = std::fs::read_to_string(&stale.nginx_conf).unwrap_or_default();
    let stale_serves = stale_conf.contains(ALIAS);
    println!("  empty mirror  → nginx.conf names {ALIAS}: {stale_serves} (want false — the recorded name is DARK)");
    let fresh = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, 8081, 8444).unwrap();
    let fresh_conf = std::fs::read_to_string(&fresh.nginx_conf).unwrap_or_default();
    let fresh_serves = fresh_conf
        .lines()
        .any(|l| l.trim_start().starts_with("server_name") && l.contains(ALIAS) && l.contains(DOMAIN));
    println!("  table's map   → one server_name carries {DOMAIN} and {ALIAS}: {fresh_serves} (want true)");
    ok &= !stale_serves && fresh_serves;

    println!("\n=== 4. serve their file ===");
    let cfg = fresh;
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let mut nginx = Reaped::new(
        services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap(),
        NGINX_PORT,
        "nginx",
    );
    // Readiness, not a timer — see `common::await_listening` for the incident
    // this pattern produced (a flat sleep loses under CPU contention, and the
    // failure then reads as the SERVER being broken rather than as too-early).
    common::await_listening(NGINX_PORT, "nginx", None);
    let body = match reqwest::Client::new()
        .get(format!("http://127.0.0.1:{NGINX_PORT}/marker.txt"))
        .header("Host", DOMAIN)
        .send()
        .await
    {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(e) => format!("ERROR: {e}"),
    };
    let served = body.contains(MARKER);
    println!("  GET /marker.txt through the {DOMAIN} vhost: {served}");
    ok &= served;
    // No GET through ALIAS here, on purpose: with one site, nginx hands ANY Host
    // to its only server block (a planted `plant-not-a-farm-name.rex` got the
    // marker, 12 Sep 2026), so it would pass with the alias unserved. The name
    // is proved by 3b's server_name; that it answers is proved at the edge —
    // SNI, certificate, and refusal once removed — by the live run recorded in
    // TODO.md's "Serving one site under two domains".

    println!("\n=== 5. delete the imported site ===");
    let out = sites::teardown(&conn, &*plat, &created.id).unwrap();
    let kept = project.exists() && fingerprint(&project) == project_before;
    println!("  reported docroot_removed : {} (want false)", out.docroot_removed);
    println!("  their project intact     : {kept}");
    ok &= !out.docroot_removed && kept;

    println!("\n=== 6. their Valet tree untouched throughout ===");
    let tree_same = fingerprint(&valet_home) == tree_before;
    println!("  {} unchanged={tree_same}", valet_home.display());
    ok &= tree_same;

    nginx.reap();
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(ssl::site_cert_dir(plat.paths(), DOMAIN).unwrap());

    if ok {
        println!("\nOK — a scanned Valet site imports as a LINK to its real docroot, serves their file, and survives deletion.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}
