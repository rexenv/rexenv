//! Phase-3 §4.2 check (data path): the terminal command path the UI drives.
//! Mirrors `commands::terminal::terminal_open` for a REAL provisioned site —
//! resolve the site's PHP + `wp` wrapper, open a PTY in its docroot — then runs
//! the Done-when commands `wp --info` and `ls`, asserting WP-CLI reports the
//! bundled PHP and `ls` lists the docroot. (The xterm.js rendering on top is
//! standard wiring; the PTY I/O is what this proves.)
//!
//! Run: `cargo run --example terminal_site_check`

use rexenv_lib::core::terminal::{self, PtyConfig, TerminalSession};
use rexenv_lib::core::{binaries, php, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "termsite.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-4_2.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Term Site".into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
        },
    )
    .unwrap();
    let site = sites::get(&conn, &sites::list(&conn).unwrap()[0].id).unwrap().unwrap();

    // Same resolution terminal_open does.
    let minor = php::minor_of(&site.php_version);
    let patch = php::patch_for_minor(&minor).unwrap();
    let php_bin = binaries::resolve(&*plat, "php", patch).await.unwrap();
    let wp_phar = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let wp_dir = terminal::ensure_wp_wrapper(&*plat, &php_bin, &wp_phar).unwrap();
    let php_dir = php_bin.parent().unwrap().to_path_buf();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let session = TerminalSession::open(
        PtyConfig {
            cwd: site.path.clone().into(),
            shell,
            path_prepend: vec![php_dir, wp_dir],
            rows: 30,
            cols: 100,
        },
        move |chunk| {
            let _ = tx.send(chunk);
        },
    )
    .unwrap();

    session.write(b"wp --info\n").unwrap();
    session.write(b"ls\n").unwrap();
    session.write(b"exit\n").unwrap();

    let mut out = String::new();
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(c) => out.push_str(&String::from_utf8_lossy(&c)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if out.contains("WP-CLI version") && out.contains("index.php") {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    println!("docroot: {}", site.path);
    assert!(out.contains("WP-CLI version"), "`wp --info` did not run WP-CLI");
    println!("✓ `wp --info` runs WP-CLI");
    assert!(out.contains(binaries::PHP_VERSION), "wp --info shows wrong PHP (want {})", binaries::PHP_VERSION);
    println!("✓ wp --info reports bundled PHP {}", binaries::PHP_VERSION);
    assert!(out.contains("index.php"), "`ls` did not list the docroot (index.php)");
    println!("✓ `ls` lists the docroot");

    let _ = session.kill();
    println!("\nALL GOOD — terminal command path runs wp --info + ls in the site's docroot.");
}
