//! phpMyAdmin, the second database console (`docs/PLAN-db-console-open-and-phpmyadmin.md`,
//! T7): the staging path end to end, in a SANDBOX app-data root, against the real pinned
//! phpMyAdmin tree (downloaded into the shared binary cache — the one deliberate real
//! write, see `common`) and the real pinned PHP.
//!
//! 1. Before staging: `rebuild_configs_for` emits NO `phpmyadmin.rexenv.rex` block and no
//!    edge route (ledger #867).
//! 2. `phpmyadmin::ensure` stages: the marker, `config.inc.php` with the two loopback
//!    servers, `index.php`, the Twig temp dir, the owner-only secret (#864).
//! 3. After staging: the block AND the route exist, with Adminer's cap (#867).
//! 4. A PLANTED tree (a parse error in `vendor/autoload.php`) fails the probe and the
//!    docroot stays exactly as staged — marker, config, index all unchanged (#866).
//! 5. A planted config that names a remote server fails the probe (#864 — the LOADED
//!    config, not the text).
//! 6. The REAL stack leg, when the running stack has phpMyAdmin staged (`rex db console
//!    phpmyadmin`): a GET through the console proxy (`dbconsole_proxy::forward`, the
//!    in-app frame's path) answers 200 with phpMyAdmin's logged-in title, no
//!    `X-Frame-Options`, and a CSP naming the app's origins (#869). Skipped, loudly, when
//!    the stack is down or phpMyAdmin is not staged there — never a silent pass.
//!
//! Run: `cargo run --example phpmyadmin_check`

use rexenv_lib::core::dbconsole_proxy::{forward, Console};
use rexenv_lib::core::{binaries, phpmyadmin, services, sites, ssl};
use std::path::Path;

mod common;

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let (plat, _sandbox) = common::sandbox("phpmyadmin");
    let conn = common::sandbox_db(&*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let version = binaries::pins().phpmyadmin;
    let rebuild = |p: &dyn rexenv_lib::platform::traits::Platform| {
        sites::rebuild_configs_for(
            &sites::list(&conn).unwrap(),
            p,
            &ca,
            services::NGINX_HTTP_PORT,
            8080,
            8443,
            &Default::default(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap()
    };

    // (1) Nothing staged → nothing emitted.
    assert!(phpmyadmin::staged_version(&*plat).is_none(), "a fresh sandbox has no phpMyAdmin");
    let before = rebuild(&*plat);
    let nginx_before = std::fs::read_to_string(&before.nginx_conf).unwrap();
    let caddy_before = std::fs::read_to_string(&before.caddyfile).unwrap();
    assert!(nginx_before.contains("adminer.rexenv.rex"), "Adminer's block is always there");
    assert!(!nginx_before.contains(phpmyadmin::PHPMYADMIN_HOST), "no phpMyAdmin block before staging");
    assert!(!caddy_before.contains(phpmyadmin::PHPMYADMIN_HOST), "no phpMyAdmin route before staging");
    println!("✓ before staging: no nginx block and no edge route for {}", phpmyadmin::PHPMYADMIN_HOST);

    // (2) Stage for real: download (cache), copy, config, probe on the pinned PHP, publish.
    let dir = phpmyadmin::ensure(&*plat, version).await.expect("stage phpMyAdmin");
    assert_eq!(phpmyadmin::staged_version(&*plat).as_deref(), Some(version));
    assert!(dir.join("index.php").is_file() && dir.join("vendor/autoload.php").is_file());
    let cfg = std::fs::read_to_string(dir.join(phpmyadmin::CONFIG_FILE)).unwrap();
    assert!(cfg.contains("$cfg['Servers'][1]['host'] = '127.0.0.1';") && cfg.contains("$cfg['Servers'][2]['host'] = '127.0.0.1';"), "{cfg}");
    assert!(cfg.contains("$cfg['AllowThirdPartyFraming'] = false;"));
    assert!(!dir.join(".rexenv-probe.php").exists(), "the probe script is removed after the run");
    assert!(phpmyadmin::temp_dir(&*plat).unwrap().is_dir(), "the Twig temp dir exists");
    let secret = plat.paths().app_data_dir().unwrap().join("phpmyadmin.secret");
    assert!(secret.is_file(), "the blowfish secret is written beside, not inside, the docroot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&secret).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the secret is owner-only, got {mode:o}");
    }
    println!("✓ staged phpMyAdmin {version}: marker, config (two loopback servers, framing denied), owner-only secret");

    // Idempotent: a second ensure re-renders nothing and keeps the tree.
    let again = phpmyadmin::ensure(&*plat, version).await.unwrap();
    assert_eq!(again, dir);
    assert_eq!(std::fs::read_to_string(dir.join(phpmyadmin::CONFIG_FILE)).unwrap(), cfg, "the config is stable across ensures");

    // (3) Staged → block and route, with the shared cap.
    let after = rebuild(&*plat);
    let nginx_after = std::fs::read_to_string(&after.nginx_conf).unwrap();
    let caddy_after = std::fs::read_to_string(&after.caddyfile).unwrap();
    assert!(nginx_after.contains(&format!("server_name {}", phpmyadmin::PHPMYADMIN_HOST)) || nginx_after.contains(phpmyadmin::PHPMYADMIN_HOST), "no phpMyAdmin block after staging:\n{nginx_after}");
    assert!(caddy_after.contains(&format!("https://{}", phpmyadmin::PHPMYADMIN_HOST)), "no edge route after staging:\n{caddy_after}");
    let ini = std::fs::read_to_string(dir.join(".user.ini")).unwrap();
    assert!(ini.contains("upload_max_filesize=") && ini.contains("error_reporting=E_ALL & ~E_DEPRECATED"), "{ini}");
    println!("✓ after staging: the nginx block, the edge route and the .user.ini exist");

    // (4) PLANT: a tree whose autoloader is a parse error never becomes the docroot.
    let php = binaries::resolve_program(&*plat, "php", binaries::pins().php).await.unwrap();
    let src = binaries::resolve_dir(&*plat, "phpmyadmin", version).await.unwrap();
    let planted = plat.paths().app_data_dir().unwrap().join("planted-tree");
    copy_dir(&src, &planted);
    std::fs::write(planted.join("vendor/autoload.php"), "<?php function x(").unwrap();
    let marker_before = std::fs::read_to_string(dir.join(".phpmyadmin-version")).unwrap();
    let index_before = std::fs::read(dir.join("index.php")).unwrap();
    let err = phpmyadmin::ensure_from(&*plat, "9.9.9-planted", &planted, &php).unwrap_err().to_string();
    assert!(err.contains("does not run on the bundled PHP"), "{err}");
    assert!(err.contains("REXENV-PROBE-FAIL"), "the probe's verdict is named: {err}");
    assert_eq!(std::fs::read_to_string(dir.join(".phpmyadmin-version")).unwrap(), marker_before, "the marker moved");
    assert_eq!(std::fs::read(dir.join("index.php")).unwrap(), index_before, "the docroot changed");
    assert_eq!(std::fs::read_to_string(dir.join(phpmyadmin::CONFIG_FILE)).unwrap(), cfg, "the config changed");
    assert!(!dir.parent().unwrap().join("phpmyadmin.new").exists(), "the failed copy was not removed");
    assert_eq!(phpmyadmin::staged_version(&*plat).as_deref(), Some(version));
    println!("✓ PLANT: a tree that fails the probe left the staged docroot untouched ({err})");

    // (5) PLANT: a config naming a remote server is refused by the probe (the LOADED config).
    let planted2 = plat.paths().app_data_dir().unwrap().join("planted-remote");
    copy_dir(&src, &planted2);
    let mut cfg2 = phpmyadmin::config_inc_php("0123456789abcdef0123456789abcdef", &phpmyadmin::temp_dir(&*plat).unwrap());
    cfg2.push_str("\n$cfg['Servers'][3]['host'] = 'db.example.com';\n$cfg['Servers'][3]['auth_type'] = 'config';\n");
    std::fs::write(planted2.join(phpmyadmin::CONFIG_FILE), cfg2).unwrap();
    let err = phpmyadmin::verify_tree(&php, &planted2).unwrap_err().to_string();
    assert!(err.contains("not a loopback config-auth server"), "{err}");
    println!("✓ PLANT: a remote server in the loaded config is refused by the probe");

    // (6) The real stack, if it is up and has phpMyAdmin staged.
    let real = rexenv_lib::platform::current();
    let real_staged = phpmyadmin::staged_version(&*real);
    let edge_up = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], rexenv_lib::core::proxy::DEFAULT_HTTPS_PORT)),
        std::time::Duration::from_secs(1),
    )
    .is_ok();
    match (edge_up, real_staged) {
        (true, Some(v)) => {
            let r = forward(Console::PhpMyAdmin, "GET", "/index.php?server=1", None, Vec::new())
                .await
                .expect("GET through the console proxy");
            let body = String::from_utf8_lossy(&r.body);
            assert_eq!(r.status, 200, "status {} body: {}", r.status, &body[..body.len().min(400)]);
            assert!(body.contains("<title>") && body.contains("phpMyAdmin"), "not phpMyAdmin: {}", &body[..body.len().min(400)]);
            assert!(body.contains("| phpMyAdmin"), "not the logged-in title (auth_type config): {}", &body[..body.len().min(400)]);
            let names: Vec<&str> = r.headers.iter().map(|(n, _)| n.as_str()).collect();
            assert!(!names.contains(&"x-frame-options"), "{names:?}");
            assert!(!names.contains(&"set-cookie"), "{names:?}");
            let csp = r.headers.iter().find(|(n, _)| n == "content-security-policy").map(|(_, v)| v.clone()).unwrap_or_default();
            assert!(csp.contains("frame-ancestors tauri://localhost"), "{csp}");
            println!("✓ REAL STACK: phpMyAdmin {v} answers through the console proxy, logged in, framing rewritten for the app");
        }
        (false, _) => println!("– real-stack leg SKIPPED: no edge on :443 (start rexenv, then `rex db console phpmyadmin`)"),
        (true, None) => println!("– real-stack leg SKIPPED: the running app has not staged phpMyAdmin (`rex db console phpmyadmin`)"),
    }

    println!("\n✓ phpmyadmin_check passed");
    std::process::ExitCode::SUCCESS
}
