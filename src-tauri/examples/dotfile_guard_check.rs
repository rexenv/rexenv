//! Live check: the nginx dotfile guard actually 404s over the wire (ledger
//! #103, highest-risk cluster). Run: `cargo run --example dotfile_guard_check`
//!
//! The claim ("`/.hidden/x.php` must 404, never reach fastcgi — a cloned
//! `.git/`/`.env` in a served docroot was readable, and tunnels make docroots
//! PUBLIC") was proven only as a SUBSTRING of the generated config; whether
//! the regex-location order actually denies before the `.php` location is
//! nginx's fact. This spins the sandbox nginx+fpm and asks for real secrets:
//!   /.env             → 404, body never contains the secret
//!   /.git/config      → 404
//!   /.hidden/x.php    → 404, PHP never executed (marker absent)
//!   /.well-known/x    → 200 (the root exemption stays real)
//!   /index.php        → 200 (control: PHP itself works)
//!
//! Apache + FrankenPHP legs: ledger backlog (same probes, their backends).

use rexenv_lib::core::{binaries, services};
use std::fs;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

mod common;
use common::Reaped;

const HTTP_PORT: u16 = 18132; // fixture — claimed in common/mod.rs
const FPM_PORT: u16 = 9793; // fixture

const ENV_SECRET: &str = "REXENV_DOTFILE_SECRET_e5b1";
const PHP_MARKER: &str = "REXENV_HIDDEN_PHP_RAN_e5b1";

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, sandbox) = common::sandbox("dotfile_guard_check");
    let mut checks = common::Check::new("dotfile_guard_check");
    let domain = "dotguard.test";

    let fpm_bin =
        binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION).await.expect("php-fpm");
    let nginx_bin =
        binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.expect("nginx");

    // A docroot shaped like the incident: cloned repo droppings beside PHP.
    let docroot = sandbox.root().join("sites").join(domain).join("public");
    fs::create_dir_all(docroot.join(".git")).unwrap();
    fs::create_dir_all(docroot.join(".hidden")).unwrap();
    fs::create_dir_all(docroot.join(".well-known")).unwrap();
    fs::write(docroot.join("index.php"), "<?php echo 'rexenv-control-ok'; ?>").unwrap();
    fs::write(docroot.join(".env"), format!("APP_KEY={ENV_SECRET}\n")).unwrap();
    fs::write(docroot.join(".git/config"), "[core]\n\trepositoryformatversion = 0\n").unwrap();
    fs::write(docroot.join(".hidden/x.php"), format!("<?php echo '{PHP_MARKER}'; ?>")).unwrap();
    fs::write(docroot.join(".well-known/probe"), "well-known-ok").unwrap();

    let fpm_conf = services::write_fpm_config(&*plat, "8.3", FPM_PORT, None, &[]).unwrap();
    let fpm = services::start_fpm(&*plat, &fpm_bin, &fpm_conf).expect("start php-fpm");
    let mut fpm = Reaped::new(fpm, FPM_PORT, "php-fpm");

    let site = services::NginxSite {
        domain: domain.into(),
        docroot: docroot.clone(),
        php_fpm_port: FPM_PORT,
        rewrite: services::RewriteMode::Single,
        body_limit: None,
        read_timeout: None,
        php_value: None,
        env: Vec::new(),
    };
    let (conf, prefix) = services::write_nginx_config(&*plat, HTTP_PORT, vec![site]).unwrap();
    let nginx = services::start_nginx(&*plat, &nginx_bin, &conf, &prefix).expect("start nginx");
    let mut nginx = Reaped::new(nginx, HTTP_PORT, "nginx");
    thread::sleep(Duration::from_millis(800));

    let control = common::http_get(HTTP_PORT, domain, "/index.php");
    checks.is("control: PHP executes", control.contains("rexenv-control-ok"), &control);

    for (path, what) in
        [("/.env", "the .env"), ("/.git/config", "the .git config"), ("/.hidden/x.php", "a dot-dir .php")]
    {
        let resp = common::http_get(HTTP_PORT, domain, path);
        let denied = resp.starts_with("HTTP/1.1 404");
        checks.is(&format!("{what} is 404"), denied, resp.lines().next().unwrap_or(""));
    }
    let env_resp = common::http_get(HTTP_PORT, domain, "/.env");
    checks.is("the secret never crosses the wire", !env_resp.contains(ENV_SECRET), "leaked");
    let php_resp = common::http_get(HTTP_PORT, domain, "/.hidden/x.php");
    checks.is(
        "a dot-dir .php never reaches fastcgi",
        !php_resp.contains(PHP_MARKER),
        "php executed behind the guard",
    );

    let wk = common::http_get(HTTP_PORT, domain, "/.well-known/probe");
    checks.is("root /.well-known/ stays exempt", wk.contains("well-known-ok"), &wk);

    nginx.reap();
    fpm.reap();
    checks.verdict()
}
