//! W4 step 1 on a real Windows machine (ledger #601): a PHP minor served by the php-cgi
//! GROUP through `core::php::PhpFpmPools` — the same lifecycle the php-fpm pool uses.
//!
//! ```text
//! ssh dell@<host> .\windows_php_pool_check.exe
//! ```
//!
//! What it proves there: `ensure("8.3")` resolves the official PHP zip, writes rexenv's
//! own ini, passes the preflight and starts ONE parent with 10 children; a FastCGI request
//! answered by a child shows every extension the model names loaded, the SMTP keys and the
//! Laravel mail environment in place, and no other ini read; the parent is the listener and
//! the group's `owned_master`; the preflight refuses an ini whose extension cannot load
//! although php-cgi exits 0; `stop_all` leaves no php-cgi and a free port.
//!
//! Fixture-owned: a sandboxed platform under `%TEMP%\rexenv-php-pool-check` (removed at the
//! end), the pool's fixed port on a machine with no rexenv stack (`require_stack_stopped`),
//! the real binary cache (the documented sandbox exception). `demo` tier: Windows-only; on
//! macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_php_pool_check: skipped — a Windows live check (ledger #601)");
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
    use rexenv_lib::core::{binaries, mail, php_cgi};
    use rexenv_lib::platform::traits::PoolModel;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process::ExitCode;
    use std::time::Duration;

    const MINOR: &str = "8.3";

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_php_pool_check");
        let root = std::env::temp_dir().join("rexenv-php-pool-check");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        let sup = plat.supervisor();
        let PoolModel::CgiGroup(group) = sup.php_pool_model() else {
            check.is("this platform names the php-cgi group model", false, "Fpm");
            return check.verdict();
        };
        let port = fpm_port(MINOR).expect("pool port");
        let www = root.join("www");
        std::fs::create_dir_all(&www).unwrap();
        let probe = www.join("probe.php");
        std::fs::write(
            &probe,
            "<?php echo json_encode(['pid' => getmypid(), 'ext' => get_loaded_extensions(), \
             'zend' => get_loaded_extensions(true), 'smtp' => ini_get('SMTP'), \
             'smtp_port' => ini_get('smtp_port'), 'mail_host' => getenv('MAIL_HOST'), \
             'ini' => php_ini_loaded_file(), 'scanned' => php_ini_scanned_files(), \
             'memory_limit' => ini_get('memory_limit')]);",
        )
        .unwrap();

        let mut pools = PhpFpmPools::default();
        pools.set_mail_catch(mail::catch_for(Some(std::path::Path::new("mailpit-unused")), true));
        let mut settings = std::collections::HashMap::new();
        settings.insert(MINOR.to_string(), vec![("memory_limit".to_string(), "384M".to_string())]);
        pools.set_settings(settings);
        let started = pools.ensure(&*plat, MINOR).await;
        check.is("ensure(8.3) starts the group", started.is_ok(), &format!("{started:?}"));
        if started.is_err() {
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        }
        common::await_listening(port, "php-cgi group", None);
        let parent = pools.status().first().map(|s| s.pid).unwrap_or(0);

        // The group's shape, from the process table.
        let marker = root.display().to_string();
        let members = sup.owned_pids(&format!("php-cgi-{MINOR}.ini"));
        check.is(
            "one parent and 10 children, all carrying the ini path",
            members.len() == 11 && members.contains(&parent),
            &format!("{} members {members:?}, parent {parent}", members.len()),
        );
        check.is(
            "the listener is the parent, and it is the group's owned_master",
            sup.port_holders(port, false).unwrap_or_default() == vec![parent]
                && sup.owned_master(port, &marker) == Some(parent),
            &format!("holders {:?}, master {:?}", sup.port_holders(port, false), sup.owned_master(port, &marker)),
        );

        // A request, answered by a child.
        let body = fastcgi_get(port, &probe.display().to_string());
        let json: serde_json::Value = serde_json::from_str(body.split("\r\n\r\n").last().unwrap_or("")).unwrap_or_default();
        println!("  · answered by pid {} (parent {parent})", json["pid"]);
        check.is("a child answered, not the parent", json["pid"].as_u64().is_some_and(|p| p as u32 != parent && members.contains(&(p as u32))), &body);
        let loaded: Vec<String> = json["ext"].as_array().into_iter().flatten().chain(json["zend"].as_array().into_iter().flatten())
            .filter_map(|v| v.as_str().map(str::to_ascii_lowercase)).collect();
        let missing: Vec<&str> = group.modules(MINOR).into_iter()
            .filter(|e| !loaded.contains(&e.to_string()))
            .chain(group.zend_extensions.iter().copied().filter(|_| !loaded.contains(&"zend opcache".to_string())))
            .collect();
        check.is(&format!("all {} extensions the model names are loaded", group.modules(MINOR).len() + group.zend_extensions.len()), missing.is_empty(), &format!("missing {missing:?}"));
        check.is("the SMTP keys point at Mailpit", json["smtp"] == "127.0.0.1" && json["smtp_port"].as_str().and_then(|s| s.parse::<u16>().ok()) == Some(mail::MAILPIT_SMTP_PORT), &body);
        check.is("the Laravel mail env reached the request", json["mail_host"] == "127.0.0.1", &body);
        check.is("the user's setting is live", json["memory_limit"] == "384M", &body);
        check.is("no php.ini was loaded besides ours (-n -c)", json["ini"].as_str().is_some_and(|p| p.ends_with(&format!("php-cgi-{MINOR}.ini"))) && json["scanned"] == false, &body);

        // The preflight refuses what php-cgi itself exits 0 on.
        let php_dir = binaries::cached_path(&*plat, "php", binaries::pins().php).expect("php cached");
        let bad = root.join("bad.ini");
        std::fs::write(&bad, format!("extension_dir = \"{}\"\nextension = does_not_exist\n", php_dir.join("ext").display())).unwrap();
        let refused = php_cgi::preflight(&*plat, &group, MINOR, &php_dir, &bad).map_err(|e| e.to_string());
        check.is("the preflight refuses an extension that cannot load, quoting PHP", refused.as_ref().is_err_and(|e| e.contains("Unable to load")), &format!("{refused:?}"));

        pools.stop_all(&*plat);
        std::thread::sleep(Duration::from_millis(1500));
        let left = sup.owned_pids(&format!("php-cgi-{MINOR}.ini"));
        check.is("stop_all leaves no php-cgi of the group", left.is_empty(), &format!("{left:?}"));
        check.is("the pool port is free", ports::wait_free(&*plat, port, Proto::Tcp, 30, Duration::from_millis(100)), "held");
        let _ = std::fs::remove_dir_all(&root);
        check.verdict()
    }

    /// A minimal FastCGI GET for `script`: BEGIN_REQUEST, PARAMS, empty STDIN, read to END_REQUEST.
    fn fastcgi_get(port: u16, script: &str) -> String {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return "no connection".into() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
        let record = |kind: u8, body: &[u8]| {
            let mut r = vec![1, kind, 0, 1, (body.len() >> 8) as u8, body.len() as u8, 0, 0];
            r.extend_from_slice(body);
            r
        };
        let mut params = Vec::new();
        for (k, v) in [("SCRIPT_FILENAME", script), ("REQUEST_METHOD", "GET"), ("SCRIPT_NAME", "/probe.php"),
                       ("QUERY_STRING", ""), ("SERVER_PROTOCOL", "HTTP/1.1"), ("GATEWAY_INTERFACE", "CGI/1.1"), ("REDIRECT_STATUS", "200")] {
            for len in [k.len(), v.len()] {
                if len < 128 { params.push(len as u8) } else { params.extend_from_slice(&((len as u32) | 0x8000_0000).to_be_bytes()) }
            }
            params.extend_from_slice(k.as_bytes());
            params.extend_from_slice(v.as_bytes());
        }
        let mut out = record(1, &[0, 1, 0, 0, 0, 0, 0, 0]);
        out.extend(record(4, &params));
        out.extend(record(4, &[]));
        out.extend(record(5, &[]));
        if s.write_all(&out).is_err() { return "write failed".into() }
        let mut stdout = Vec::new();
        let mut head = [0u8; 8];
        while s.read_exact(&mut head).is_ok() {
            let len = ((head[4] as usize) << 8) | head[5] as usize;
            let mut body = vec![0u8; len + head[6] as usize];
            if s.read_exact(&mut body).is_err() { break }
            match head[1] { 6 => stdout.extend_from_slice(&body[..len]), 3 => break, _ => {} }
        }
        String::from_utf8_lossy(&stdout).into_owned()
    }
}
