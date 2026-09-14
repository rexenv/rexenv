//! W4's "Done when" on a real Windows machine (plan §5): a one-click WordPress site loads
//! through nginx and its mail lands in Mailpit.
//!
//! ```text
//! scripts/probes/windows-example.sh dell@<host> windows_wp_site_check
//! ```
//!
//! The core path `commands::sites::create_site` takes for a WordPress site, as
//! `examples/wp_create_serve.rs` walks it on macOS, minus the edge (W5) and DNS (W6): MySQL
//! initialised and started, Mailpit started, the php-cgi group for 8.3 with the mail catch on,
//! `sites::provision`, `wordpress::install_for_site` through the site's own PHP and the pinned
//! WP-CLI (core as a zip, wp-config, the database through the bundled client, `core install`,
//! the password over stdin), `sites::rebuild_configs` and the shared nginx. Then, as the edge
//! would send them (`Host` + `X-Forwarded-Proto: https`): the homepage with the site's title,
//! the login form, a lost-password POST whose mail must reach Mailpit through the group's SMTP
//! keys — and a `wp eval` `wp_mail()` through WP-CLI, whose mail must reach Mailpit too.
//!
//! Fixture-owned: a sandboxed platform and database under `%TEMP%\rexenv wp site check` (a
//! SPACE in it, as a Windows user name can have), removed at the end; rexenv's fixed MySQL,
//! Mailpit, pool and nginx ports on a machine with no stack (`require_stack_stopped`); every
//! service held by `OwnedService` or the pool manager. The binary cache is the documented
//! exception. Needs the network (WordPress, the binaries on first use). `demo` tier:
//! Windows-only; on macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_wp_site_check: skipped — a Windows live check (plan W4)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check, OwnedService};
    use rexenv_lib::core::db::DbEngine;
    use rexenv_lib::core::php::{fpm_port, PhpFpmPools};
    use rexenv_lib::core::ports::{self, Proto};
    use rexenv_lib::core::services::{self, NGINX_HTTP_PORT};
    use rexenv_lib::core::wordpress::{self, InstallOptions};
    use rexenv_lib::core::{binaries, database, mail, sites, ssl};
    use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process::ExitCode;
    use std::time::{Duration, Instant};

    const DOMAIN: &str = "wpsite.rex";
    const TITLE: &str = "Windows WP Site Check";
    const MINOR: &str = "8.3";

    /// Record a step; on failure, the value is gone and the caller bails.
    fn step<T, E: std::fmt::Debug>(check: &mut Check, label: &str, r: Result<T, E>) -> Option<T> {
        match r {
            Ok(v) => {
                check.is(label, true, "");
                Some(v)
            }
            Err(e) => {
                check.is(label, false, &format!("{e:?}"));
                None
            }
        }
    }

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_wp_site_check");
        let root = std::env::temp_dir().join("rexenv wp site check");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        let log_dir = plat.paths().log_dir().unwrap();
        let verdict = run(&mut check, &*plat, &log_dir).await;
        if let Err(e) = std::fs::remove_dir_all(&root) {
            println!("  · sandbox not fully removed: {e}");
        }
        let _ = verdict;
        check.verdict()
    }

    async fn run(check: &mut Check, plat: &dyn rexenv_lib::platform::traits::Platform, log_dir: &std::path::Path) -> Option<()> {
        let conn = common::sandbox_db(plat);
        let ca = step(check, "the local CA is created", ssl::load_or_create(plat.paths(), plat.permissions()))?;

        let t = Instant::now();
        let mysql_base = step(check, "MySQL resolves", binaries::resolve_dir(plat, "mysql", binaries::MYSQL_VERSION).await)?;
        let mailpit = step(check, "Mailpit resolves", binaries::resolve(plat, "mailpit", binaries::MAILPIT_VERSION).await)?;
        let php = step(check, "the site's PHP resolves as php.exe", binaries::resolve_program(plat, "php", binaries::PHP_VERSION).await)?;
        let wp = step(check, "WP-CLI resolves", binaries::resolve_file(plat, "wp-cli", binaries::WP_CLI_VERSION).await)?;
        let nginx = step(check, "nginx resolves", binaries::resolve_program(plat, "nginx", binaries::NGINX_VERSION).await)?;
        let (db_client, _) = step(check, "the bundled MySQL client resolves", DbEngine::Mysql.sql_client_bins(plat, binaries::MYSQL_VERSION).await)?;
        println!("  · binaries in {:.0} s", t.elapsed().as_secs_f64());

        // ── The services create_site brings up. ──
        let datadir = database::data_dir(plat).unwrap();
        let socket = database::socket_path(plat).unwrap();
        let _ = std::fs::create_dir_all(socket.parent().unwrap());
        step(check, "MySQL initialises", database::initialize(plat, &mysql_base, &datadir))?;
        let mut mysqld = OwnedService::new(
            step(check, "MySQL starts", database::start(plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket))?,
            "mysqld",
        );
        common::await_listening(database::MYSQL_PORT, "mysqld", Some(&log_dir.join("mysql-error.log")));
        let mut mailpitd = OwnedService::new(step(check, "Mailpit starts", mail::start(plat, &mailpit))?, "mailpit");
        common::await_listening(mail::MAILPIT_HTTP_PORT, "mailpit", None);

        let mut pools = PhpFpmPools::default();
        pools.set_mail_catch(mail::catch_for(Some(&mailpit), true));
        step(check, "the php-cgi group for 8.3 starts with the mail catch on", pools.ensure(plat, MINOR).await)?;
        common::await_listening(fpm_port(MINOR).unwrap(), "php-cgi group", Some(&rexenv_lib::core::php_cgi::output_log(log_dir, MINOR)));

        // ── The site. ──
        let site = step(
            check,
            "sites::provision creates the site (docroot, certificate, row)",
            sites::provision(
                &conn,
                plat,
                &ca,
                NewSite {
                    name: TITLE.into(),
                    domain: DOMAIN.into(),
                    site_type: SiteType::Wordpress,
                    php_version: MINOR.into(),
                    web_server: WebServer::Nginx,
                    path: String::new(),
                    db_engine: SiteDbEngine::Mysql,
                    git_url: String::new(),
                    git_ref: None,
                    git_migrate: true,
                    git_build_assets: false,
                    starter_db: false,
                },
            ),
        )?;
        let docroot = std::path::PathBuf::from(&site.path);
        let t = Instant::now();
        let installed = wordpress::install_for_site(
            &php,
            &wp,
            &docroot,
            &site.domain,
            &site.name,
            &site.db_name,
            &format!("127.0.0.1:{}", database::MYSQL_PORT),
            &db_client,
            &InstallOptions {
                admin_user: "owner".into(),
                admin_password: "rexenv-pw".into(),
                admin_email: format!("owner@{DOMAIN}"),
                ..Default::default()
            },
        );
        println!("  · install_for_site took {:.0} s", t.elapsed().as_secs_f64());
        step(check, "install_for_site: core zip, wp-config, database, core install, password", installed)?;
        let verified = wordpress::wp_cli_checked(&php, &wp, &["core", "verify-checksums", &format!("--path={}", docroot.display())], None);
        check.is("wp core verify-checksums passes on Windows", verified.is_ok(), &format!("{verified:?}"));

        let cfg = step(check, "sites::rebuild_configs writes the nginx config", sites::rebuild_configs(&conn, plat, &ca, NGINX_HTTP_PORT, 8080, 8443))?;
        let mut ngx = OwnedService::new(
            step(check, "the shared nginx starts", services::start_nginx(plat, &nginx, &cfg.nginx_conf, &cfg.nginx_prefix))?,
            "nginx",
        );
        common::await_listening(NGINX_HTTP_PORT, "nginx", Some(&log_dir.join("nginx-error.log")));

        // ── Through nginx, as the edge sends it. ──
        let home = http("GET", "/", "");
        println!("  · GET / -> {}", status_line(&home));
        check.is("the homepage loads with the site's title", status_line(&home).contains(" 200") && home.contains(TITLE), &head(&home));
        let login = http("GET", "/wp-login.php", "");
        check.is(
            "the login form loads",
            status_line(&login).contains(" 200") && login.contains("name=\"log\"") && login.contains("name=\"pwd\""),
            &head(&login),
        );

        // ── Mail: a page request (the group's SMTP keys) and WP-CLI (its own flags). ──
        let reset = http("POST", "/wp-login.php?action=lostpassword", "user_login=owner&redirect_to=&wp-submit=Get+New+Password");
        println!("  · lost-password POST -> {}", status_line(&reset));
        check.is(
            "a page request's mail (password reset) lands in Mailpit",
            wait_for_mail("Password Reset"),
            &format!("{} · mailpit: {}", head(&reset), head(&mailpit_messages())),
        );
        // WP-CLI routes mail by PHP's SMTP keys here, never the sendmail shim — a shim is a cmd.exe
        // line that a Mailpit path with a space breaks while mail() answers true (#407). Read from
        // INSIDE a real WP-CLI run, so it is what PHP was given, not what an argv builder returns.
        // (The shim would deliver from this Mailpit path too — it has no space — so the delivery
        // below cannot tell the branches apart; this can.)
        let ini = wordpress::wp_cli(
            &php,
            &wp,
            &["eval", "echo ini_get('SMTP'), '|', ini_get('smtp_port'), '|', ini_get('sendmail_path');", &format!("--path={}", docroot.display())],
            None,
        );
        let ini = ini.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(|e| e.to_string());
        check.is(
            "inside WP-CLI, PHP's SMTP keys aim at Mailpit and no sendmail shim is set",
            ini == format!("127.0.0.1|{}|", mail::MAILPIT_SMTP_PORT),
            &ini,
        );
        let eval = wordpress::wp_cli(
            &php,
            &wp,
            &["eval", "var_dump(wp_mail('owner@wpsite.rex', 'rexenv wp-cli mail check', 'sent by wp eval'));", &format!("--path={}", docroot.display())],
            None,
        );
        let said = eval.as_ref().map(|o| format!("exit {:?} · {} · {}", o.status.code(), String::from_utf8_lossy(&o.stdout).trim(), String::from_utf8_lossy(&o.stderr).trim())).unwrap_or_else(|e| e.to_string());
        println!("  · wp eval wp_mail: {said}");
        check.is("WP-CLI's mail (wp eval wp_mail) lands in Mailpit", wait_for_mail("rexenv wp-cli mail check"), &said);

        // ── Teardown, checked. ──
        ngx.stop();
        pools.stop_all(plat);
        mailpitd.stop();
        mysqld.stop();
        std::thread::sleep(Duration::from_millis(1000));
        for (port, what) in [
            (NGINX_HTTP_PORT, "nginx"),
            (fpm_port(MINOR).unwrap(), "the php-cgi group"),
            (mail::MAILPIT_HTTP_PORT, "Mailpit"),
            (database::MYSQL_PORT, "MySQL"),
        ] {
            check.is(&format!("{what}'s port is free after the stop"), ports::wait_free(plat, port, Proto::Tcp, 50, Duration::from_millis(100)), "held");
        }
        Some(())
    }

    fn http(method: &str, path: &str, body: &str) -> String {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", NGINX_HTTP_PORT)) else { return "no connection".into() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
        let extra = if method == "POST" {
            format!("Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n", body.len())
        } else {
            String::new()
        };
        let _ = write!(
            s,
            "{method} {path} HTTP/1.1\r\nHost: {DOMAIN}\r\nX-Forwarded-Proto: https\r\n{extra}Connection: close\r\n\r\n{body}"
        );
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn status_line(response: &str) -> String {
        response.lines().next().unwrap_or("").to_string()
    }

    fn head(response: &str) -> String {
        response.chars().take(600).collect()
    }

    fn mailpit_messages() -> String {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", mail::MAILPIT_HTTP_PORT)) else { return "no connection".into() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = write!(s, "GET /api/v1/messages HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn wait_for_mail(subject: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if mailpit_messages().contains(subject) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        false
    }
}
