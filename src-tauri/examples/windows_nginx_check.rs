//! W4 step 2 on a real Windows machine (ledger #602): the shared nginx in front of a php-cgi
//! group, through `core` — config, start, reload, ownership and stop.
//!
//! ```text
//! ssh dell@<host> .\windows_nginx_check.exe
//! ```
//!
//! What it proves there: `services::write_nginx_config` writes a config nginx accepts although
//! every app-data path on Windows is backslashed and the sandbox root has a SPACE (a backslash
//! in a quoted nginx string is an escape — measured); `start_nginx` serves a static file and a
//! PHP request through the group; `reload_nginx` reloads; `owned_master` is nginx's MASTER,
//! although the socket table names its worker; `stop` ends master AND worker through nginx's
//! quit event — a bare terminate leaves the worker serving (measured) — and the port is free.
//! It also records, without asserting, whether a per-vhost `PHP_VALUE` reaches a php-cgi child.
//!
//! Fixture-owned: a sandboxed platform under `%TEMP%\rexenv nginx check` (removed at the end),
//! rexenv's fixed nginx and pool ports on a machine with no stack (`require_stack_stopped`), the
//! real binary cache. `demo` tier: Windows-only; on macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_nginx_check: skipped — a Windows live check (ledger #602)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check};
    use rexenv_lib::core::php::{fpm_port, PhpFpmPools};
    use rexenv_lib::core::ports::{self, Proto};
    use rexenv_lib::core::services::{self, NginxSite, ReloadOutcome, RewriteMode, NGINX_HTTP_PORT};
    use rexenv_lib::core::binaries;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process::ExitCode;
    use std::time::Duration;

    const HOST: &str = "probe.rex";

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_nginx_check");
        // A SPACE in the root, as `Application Support` has on macOS and a Windows user name can.
        let root = std::env::temp_dir().join("rexenv nginx check");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        let sup = plat.supervisor();
        let marker = plat.paths().app_data_dir().unwrap().display().to_string();

        let docroot = root.join("site root");
        std::fs::create_dir_all(&docroot).unwrap();
        std::fs::write(docroot.join("hello.html"), "static-ok").unwrap();
        std::fs::write(
            docroot.join("probe.php"),
            "<?php echo 'php-ok pid=' . getmypid() . ' memory_limit=' . ini_get('memory_limit');",
        )
        .unwrap();

        let minor = "8.3";
        let pool_port = fpm_port(minor).unwrap();
        let mut pools = PhpFpmPools::default();
        let pool = pools.ensure(&*plat, minor).await;
        check.is("the php-cgi group starts", pool.is_ok(), &format!("{pool:?}"));
        if pool.is_err() {
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        }
        common::await_listening(pool_port, "php-cgi group", None);

        let site = NginxSite {
            domain: HOST.into(),
            docroot: docroot.clone(),
            php_fpm_port: pool_port,
            rewrite: RewriteMode::Single,
            body_limit: None,
            read_timeout: None,
            // Recorded, not asserted: php-fpm applies PHP_VALUE per request; php-cgi may not.
            php_value: Some("memory_limit=222M".into()),
            aliases: Vec::new(),
            storage_root: None,
            env: Vec::new(),
        };
        let (conf, prefix) = services::write_nginx_config(&*plat, NGINX_HTTP_PORT, vec![site.clone()], Vec::new()).expect("write config");
        let text = std::fs::read_to_string(&conf).unwrap_or_default();
        check.is(
            "no backslash reaches a quoted string in the written config",
            !text.lines().any(|l| l.contains('"') && l.contains('\\')),
            &text.lines().filter(|l| l.contains('\\')).collect::<Vec<_>>().join(" | "),
        );

        let nginx = binaries::resolve_program(&*plat, "nginx", binaries::pins().nginx).await;
        check.is("resolve_program finds nginx.exe inside the Windows tree", nginx.as_ref().is_ok_and(|p| p.ends_with("nginx.exe")), &format!("{nginx:?}"));
        let Ok(nginx) = nginx else {
            pools.stop_all(&*plat);
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        };
        let tested = services::test_nginx_config(&*plat, &nginx, &conf, &prefix);
        check.is("nginx -t accepts the config (space + app-data paths)", tested.is_ok(), &format!("{tested:?}"));

        let mut child = match services::start_nginx(&*plat, &nginx, &conf, &prefix) {
            Ok(c) => c,
            Err(e) => {
                check.is("start_nginx", false, &e.to_string());
                pools.stop_all(&*plat);
                let _ = std::fs::remove_dir_all(&root);
                return check.verdict();
            }
        };
        common::await_listening(NGINX_HTTP_PORT, "nginx", Some(&plat.paths().log_dir().unwrap().join("nginx-error.log")));
        let master = child.id();

        let static_body = http_get("/hello.html");
        check.is("a static file is served", static_body.ends_with("static-ok"), &static_body);
        let php_body = http_get("/probe.php");
        check.is("a PHP request is served through the group", php_body.contains("php-ok pid="), &php_body);
        println!("  · per-vhost PHP_VALUE memory_limit=222M reached php-cgi: {}", php_body.contains("memory_limit=222M"));

        let holders = sup.port_holders(NGINX_HTTP_PORT, false).unwrap_or_default();
        println!("  · nginx master {master}, listener(s) {holders:?}");
        check.is(
            "owned_master climbs from the worker listener to nginx's master",
            sup.owned_master(NGINX_HTTP_PORT, &marker) == Some(master),
            &format!("owned_master {:?}, master {master}, holders {holders:?}", sup.owned_master(NGINX_HTTP_PORT, &marker)),
        );

        let reloaded = services::reload_nginx(&*plat, &nginx, &conf, &prefix, NGINX_HTTP_PORT);
        check.is("reload_nginx reloads", matches!(reloaded, Ok(ReloadOutcome::Reloaded)), &format!("{reloaded:?}"));
        std::thread::sleep(Duration::from_millis(800));
        check.is("still serving after the reload", http_get("/hello.html").ends_with("static-ok"), "no answer");
        check.is("signal_reload (the event) is delivered to the master", sup.signal_reload(master), "not delivered");
        std::thread::sleep(Duration::from_millis(800));

        let stopped = sup.stop(master);
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(500));
        let left: Vec<u32> = sup.owned_pids(&marker).into_iter().filter(|&p| sup.pid_exe(p).is_some_and(|e| e.ends_with("nginx.exe"))).collect();
        check.is("stop returns Ok", stopped.is_ok(), &format!("{stopped:?}"));
        check.is("master AND worker are gone (the quit event, not a bare terminate)", left.is_empty(), &format!("left {left:?}"));
        check.is("the nginx port is free", ports::wait_free(&*plat, NGINX_HTTP_PORT, Proto::Tcp, 30, Duration::from_millis(100)), "held");

        pools.stop_all(&*plat);
        let _ = std::fs::remove_dir_all(&root);
        check.verdict()
    }

    fn http_get(path: &str) -> String {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", NGINX_HTTP_PORT)) else { return "no connection".into() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = write!(s, "GET {path} HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\n\r\n");
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }
}
