//! Live check (LINUX ONLY): Redis, MariaDB and Apache httpd from rexenv/runtimes' Linux builds,
//! through the app's own resolve and start paths, on a SANDBOXED app-data root.
//! Run: `cargo run --example linux_trees_check` — on Linux (the VM, or the linux-check image).
//!
//! D-L8 closed on 28 Oct 2026 by pinning OUR builds (`redis-linux-1`, `mariadb-linux-1`,
//! `httpd-linux-1`); rexenv/runtimes gated each one as a running server in a clean 22.04 and
//! 24.04. This proves the OTHER half — what the app does with them:
//!   1. `binaries::resolve_tree` resolves each through `resolve_dir` (Linux's shape), verifying
//!      the pinned digest, extracting with `TarGzTree { strip: 0 }` so the layout lands at the
//!      cache dir's root, and staging the licence archive beside it.
//!   2. The layout every reader expects: `redis::redis_server_bin`, `mariadb::mariadbd_bin` +
//!      the three bootstrap SQL files, `apache::httpd_bin` + the ten modules + the mime map.
//!   3. `DbEngine::Redis.start` (the shared server's real arguments) answers PING and SET/GET.
//!   4. `DbEngine::Mariadb.start` — its own `initialize` (the `--bootstrap` over stdin) and
//!      start — then CREATE/INSERT/SELECT through `sql_client_bins`' client, and a dump.
//!   5. httpd with the conf `apache::write_config` renders for Linux: `-t`, then a static file,
//!      an `.htaccess` rewrite, a dotfile 404, and `.php` to a throwaway php-fpm pool.
//!
//! Uses the engines' FIXED ports (16379, MariaDB's) like `redis_bundle_check`: stop the stack
//! first (service tier). Every process it starts is `Reaped`.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::services::RewriteMode;
use rexenv_lib::core::{apache, binaries, mariadb, redis};
use std::process::Command;

mod common;
use common::Reaped;

const DOMAIN: &str = "linuxtrees.rex";
const HTTPD_PORT: u16 = apache::APACHE_BASE_PORT + 98;
const FPM_PORT: u16 = 9798;

fn run(bin: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(bin).args(args).output().expect("spawn");
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn http(path: &str, header: Option<&str>) -> String {
    let mut args = vec!["-s", "-i", "--max-time", "5"];
    if let Some(h) = header {
        args.extend(["-H", h]);
    }
    let url = format!("http://127.0.0.1:{HTTPD_PORT}{path}");
    let out = Command::new("curl").args(&args).arg(&url).output().expect("curl");
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    if std::env::consts::OS != "linux" {
        eprintln!("linux_trees_check: Linux only — on macOS these are bottle bundles (redis_bundle_check & co.)");
        std::process::exit(2);
    }
    common::require_ports_free(&[
        (DbEngine::Redis.port(), "Redis"),
        (DbEngine::Mariadb.port(), "MariaDB"),
        (HTTPD_PORT, "the check's httpd"),
        (FPM_PORT, "the check's php-fpm"),
    ]);
    let (plat, _sandbox) = common::sandbox("linux_trees_check");
    let mut checks = common::Check::new("linux_trees_check");

    // ── 1–2. Resolve, layout, licences ───────────────────────────────────────────
    let redis_v = DbEngine::Redis.default_version();
    let maria_v = DbEngine::Mariadb.default_version();
    let httpd_v = binaries::httpd_version();
    println!("=== resolve_tree: redis {redis_v}, mariadb {maria_v}, httpd {httpd_v} ===");
    for (name, v) in [("redis", redis_v), ("mariadb", maria_v), ("httpd", httpd_v)] {
        checks.is(&format!("{name} {v} ships on Linux"), binaries::ships_on(name, v, "linux"), "no Linux arm");
        checks.is(&format!("{name} is a tree on Linux"), binaries::shape_of(name) == binaries::Shape::Dir, "shape");
    }
    let redis_dir = binaries::resolve_tree(&*plat, "redis", redis_v).await.expect("resolve redis");
    let maria_dir = binaries::resolve_tree(&*plat, "mariadb", maria_v).await.expect("resolve mariadb");
    let httpd_dir = binaries::resolve_tree(&*plat, "httpd", httpd_v).await.expect("resolve httpd");
    for (name, dir) in [("redis", &redis_dir), ("mariadb", &maria_dir), ("httpd", &httpd_dir)] {
        println!("  {name}: {}", dir.display());
        let lic = dir.join("licenses").read_dir().map(|d| d.count()).unwrap_or(0);
        checks.is(&format!("{name}: licence texts staged beside it ({lic})"), lic >= 5, "licenses/ missing or thin");
        checks.is(&format!("{name}: cached_path agrees it is cached"), binaries::is_cached(&*plat, name, if name == "redis" { redis_v } else if name == "mariadb" { maria_v } else { httpd_v }), "not cached");
    }
    checks.is("redis-server + redis-cli at the root", redis::redis_server_bin(&redis_dir).is_file() && redis::redis_cli_bin(&redis_dir).is_file(), "layout");
    let maria_files = ["share/mysql/mariadb_system_tables.sql", "share/mysql/mariadb_performance_tables.sql",
        "share/mysql/mariadb_system_tables_data.sql", "share/mysql/english/errmsg.sys", "share/mysql/charsets/Index.xml"];
    checks.is("mariadbd + clients + bootstrap SQL + errmsg + charsets",
        mariadb::mariadbd_bin(&maria_dir).is_file() && maria_files.iter().all(|f| maria_dir.join(f).is_file()), "layout");
    let layout = apache::layout();
    checks.is("httpd + the ten modules + the mime map (the bottle layout, so no Layout arm)",
        apache::httpd_bin(&httpd_dir).is_file()
            && httpd_dir.join(layout.mime_types).is_file()
            && httpd_dir.join(layout.modules_dir).read_dir().map(|d| d.count()).unwrap_or(0) == 10,
        "layout");

    // ── 3. Redis through DbEngine ────────────────────────────────────────────────
    println!("\n=== DbEngine::Redis {redis_v} ===");
    let rport = DbEngine::Redis.port();
    let rproc = DbEngine::Redis.start(&*plat, redis_v).await.expect("start redis");
    let mut rserver = Reaped::from_proc(rproc, rport, "redis-server");
    common::await_listening(rport, "redis", None);
    let cli = |args: &[&str]| {
        let mut a = vec!["-h", "127.0.0.1", "-p"];
        let p = rport.to_string();
        a.push(&p);
        a.extend_from_slice(args);
        run(&redis::redis_cli_bin(&redis_dir), &a).1.trim().to_string()
    };
    checks.is("PING → PONG", cli(&["PING"]) == "PONG", "no PONG");
    checks.is("SET/GET round trip", cli(&["SET", "rexenv:linux", "tree-works"]) == "OK" && cli(&["GET", "rexenv:linux"]) == "tree-works", "round trip");
    rserver.reap();

    // ── 4. MariaDB through DbEngine (initialize = --bootstrap, then start) ──────
    println!("\n=== DbEngine::Mariadb {maria_v} ===");
    let mport = DbEngine::Mariadb.port();
    let mproc = DbEngine::Mariadb.start(&*plat, maria_v).await.expect("start mariadb (bootstrap + start)");
    let mut mserver = Reaped::from_proc(mproc, mport, "mariadbd");
    common::await_listening(mport, "mariadb", None);
    let (client, dump) = DbEngine::Mariadb.sql_client_bins(&*plat, maria_v).await.expect("sql client");
    let port_s = mport.to_string();
    let sql = |q: &str| run(client.path(), &["--no-defaults", "-uroot", "-h127.0.0.1", "-P", &port_s, "--batch", "--skip-column-names", "-e", q]);
    let (ok, out) = sql("CREATE DATABASE IF NOT EXISTS lt; CREATE TABLE IF NOT EXISTS lt.t (id INT PRIMARY KEY, v VARCHAR(32)) ENGINE=InnoDB; \
                          REPLACE INTO lt.t VALUES (1, 'tree-works'); SELECT v FROM lt.t WHERE id = 1");
    // A LINE, not the tail: `run` appends stderr, where the 11.4+ client prints its
    // passwordless-login TLS warning (the same on macOS's bottle).
    checks.is("CREATE / INSERT / SELECT through the bundled client", ok && out.lines().any(|l| l.trim() == "tree-works"), &out);
    let (ok, out) = run(&dump, &["--no-defaults", "-uroot", "-h127.0.0.1", "-P", &port_s, "lt"]);
    checks.is("mariadb-dump dumps it", ok && out.contains("CREATE TABLE `t`") && out.contains("tree-works"), &out[..out.len().min(300)]);
    let (_, ver) = sql("SELECT VERSION()");
    let ver = ver.lines().next().unwrap_or("").trim().to_string();
    checks.is(&format!("server says {ver}"), ver.starts_with(maria_v), &ver);
    mserver.reap();

    // ── 5. httpd with the conf rexenv renders for Linux ──────────────────────────
    println!("\n=== httpd {httpd_v} with apache::write_config ===");
    let docroot = std::env::temp_dir().join("rexenv-linux-trees-check");
    let _ = std::fs::remove_dir_all(&docroot);
    std::fs::create_dir_all(&docroot).unwrap();
    std::fs::write(docroot.join("index.html"), "static-ok\n").unwrap();
    std::fs::write(docroot.join("target.html"), "htaccess-ok\n").unwrap();
    std::fs::write(docroot.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(docroot.join(".htaccess"), "RewriteEngine On\nRewriteRule ^from-htaccess$ target.html [L]\n").unwrap();
    std::fs::write(docroot.join("info.php"), "<?php echo 'php-ok|', $_SERVER['HTTPS'] ?? 'off';").unwrap();

    let fpm_bin = binaries::resolve(&*plat, "php-fpm", binaries::pins().php).await.expect("php-fpm");
    let fpm_conf = docroot.join("fpm.ini");
    std::fs::write(&fpm_conf, format!(
        "[global]\ndaemonize = no\nerror_log = {}\n[www]\nlisten = 127.0.0.1:{FPM_PORT}\npm = static\npm.max_children = 2\n",
        docroot.join("fpm.log").display())).unwrap();
    let mut fpm = Reaped::new(
        Command::new(&fpm_bin).args(["-y", &fpm_conf.display().to_string(), "-F"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .spawn().expect("spawn php-fpm"),
        FPM_PORT, "php-fpm");
    common::await_listening(FPM_PORT, "php-fpm", Some(&docroot.join("fpm.log")));

    let conf = apache::write_config(&*plat, &httpd_dir, DOMAIN, &docroot, HTTPD_PORT, FPM_PORT, RewriteMode::Single, &[])
        .expect("write httpd conf");
    let (ok, out) = run(&apache::httpd_bin(&httpd_dir), &["-t", "-f", &conf.display().to_string()]);
    checks.is("httpd -t on the generated conf", ok && out.contains("Syntax OK"), &out);
    let mut httpd = Reaped::new(apache::start(&*plat, &httpd_dir, DOMAIN, &conf).expect("start httpd"), HTTPD_PORT, "httpd");
    common::await_listening(HTTPD_PORT, "httpd", Some(&apache::error_log_path(&*plat, DOMAIN).unwrap()));
    checks.is("static file 200", http("/index.html", None).contains("static-ok"), "static");
    checks.is(".htaccess RewriteRule fires", http("/from-htaccess", None).contains("htaccess-ok"), ".htaccess");
    checks.is("/.env → 404", http("/.env", None).starts_with("HTTP/1.1 404"), "dotfile");
    checks.is(".php through mod_proxy_fcgi, HTTPS=on from X-Forwarded-Proto",
        http("/info.php", Some("X-Forwarded-Proto: https")).contains("php-ok|on"), "php");
    httpd.reap();
    fpm.reap();
    let _ = std::fs::remove_dir_all(&docroot);

    checks.verdict()
}
