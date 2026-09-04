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
//! All THREE templates get the same probes against their real backends
//! (15 Aug 2026 — the Apache and FrankenPHP legs closed the #103 backlog):
//! nginx+fpm, httpd+the SAME fpm pool (mod_proxy_fcgi), and FrankenPHP's
//! embedded PHP. Each backend gets its own control leg first — "404 on the
//! dotfile" proves nothing about a server that 404s everything.

use rexenv_lib::core::{apache, binaries, frankenphp, services};
use std::fs;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

mod common;
use common::Reaped;

const HTTP_PORT: u16 = 18132; // fixture — claimed in common/mod.rs
const FPM_PORT: u16 = 9793; // fixture
const FRANKEN_PORT: u16 = 9794; // fixture
const APACHE_PORT: u16 = 9795; // fixture

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
        storage_root: None,
        aliases: Vec::new(),
    };
    let (conf, prefix) = services::write_nginx_config(
        &*plat,
        HTTP_PORT,
        vec![site],
        Vec::new(),
    )
    .unwrap();
    let nginx = services::start_nginx(&*plat, &nginx_bin, &conf, &prefix).expect("start nginx");
    let mut nginx = Reaped::new(nginx, HTTP_PORT, "nginx");
    // Both, and neither is the flat 800ms sleep that used to stand here. The
    // sleep was a timing assumption, and on 20 Aug 2026 it lost: the two PHP
    // CONTROLS came back 502/nginx and 503/httpd while all twelve dotfile
    // assertions passed — the pool's fingerprint, read as the server's. The
    // controls exist so a `.php` returning 404 cannot pass vacuously, so a
    // control that fails for the WRONG reason is the check disqualifying
    // itself with a misleading reason attached.
    common::await_listening(FPM_PORT, "php-fpm", Some(&fpm_conf.with_file_name("php-fpm.log")));
    common::await_listening(HTTP_PORT, "nginx", None);

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

    // ── the same five probes against a REAL Apache (same fpm pool) ─────────
    // The claim is per-TEMPLATE: apache.rs's deny is a mod_rewrite [R=404]
    // that must precede the WP routing, and only httpd can say whether that
    // ordering actually denies before mod_proxy_fcgi hands .php to the pool.
    let basedir = binaries::resolve_bundle(&*plat, "httpd", binaries::HTTPD_VERSION)
        .await
        .expect("httpd bundle (cached)");
    let apache_conf = apache::write_config(
        &*plat,
        &basedir,
        domain,
        &docroot,
        APACHE_PORT,
        FPM_PORT,
        services::RewriteMode::Single,
        &[],
    )
    .expect("apache conf");
    let mut httpd = Reaped::new(
        apache::start(&*plat, &basedir, domain, &apache_conf).expect("start apache"),
        APACHE_PORT,
        "httpd",
    );
    for _ in 0..40 {
        if apache::running(APACHE_PORT) {
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    let control = common::http_get(APACHE_PORT, domain, "/index.php");
    checks.is("apache control: PHP executes", control.contains("rexenv-control-ok"), &control);
    probe_backend(&mut checks, "apache", APACHE_PORT, domain);
    httpd.reap();
    fpm.reap();

    // ── and against a REAL FrankenPHP (its embedded PHP, no pool) ──────────
    let franken_bin = binaries::resolve(&*plat, "frankenphp", binaries::FRANKENPHP_VERSION)
        .await
        .expect("frankenphp (cached)");
    let franken_conf = frankenphp::write_config(
        &*plat,
        domain,
        &docroot,
        FRANKEN_PORT,
        services::RewriteMode::Single,
        &[],
    )
    .expect("frankenphp conf");
    let mut franken = Reaped::new(
        frankenphp::start(&*plat, &franken_bin, domain, &franken_conf, &[])
            .expect("start frankenphp"),
        FRANKEN_PORT,
        "frankenphp",
    );
    for _ in 0..40 {
        if frankenphp::running(FRANKEN_PORT) {
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    let control = common::http_get(FRANKEN_PORT, domain, "/index.php");
    checks.is("frankenphp control: PHP executes", control.contains("rexenv-control-ok"), &control);
    probe_backend(&mut checks, "frankenphp", FRANKEN_PORT, domain);
    franken.reap();

    checks.verdict()
}

/// The #103 probe set, identical for every backend: the three dotfile paths
/// 404, the secret never crosses, the hidden .php never executes, and root
/// `/.well-known/` stays exempt. The caller runs the backend's CONTROL leg
/// first — without PHP provably executing, every 404 here is vacuous.
fn probe_backend(checks: &mut common::Check, backend: &str, port: u16, domain: &str) {
    for (path, what) in
        [("/.env", "the .env"), ("/.git/config", "the .git config"), ("/.hidden/x.php", "a dot-dir .php")]
    {
        let resp = common::http_get(port, domain, path);
        checks.is(
            &format!("{backend}: {what} is 404"),
            resp.starts_with("HTTP/1.1 404"),
            resp.lines().next().unwrap_or(""),
        );
    }
    let env_resp = common::http_get(port, domain, "/.env");
    checks.is(
        &format!("{backend}: the secret never crosses the wire"),
        !env_resp.contains(ENV_SECRET),
        "leaked",
    );
    let php_resp = common::http_get(port, domain, "/.hidden/x.php");
    checks.is(
        &format!("{backend}: a dot-dir .php never executes"),
        !php_resp.contains(PHP_MARKER),
        "php executed behind the guard",
    );
    let wk = common::http_get(port, domain, "/.well-known/probe");
    checks.is(&format!("{backend}: root /.well-known/ stays exempt"), wk.contains("well-known-ok"), &wk);
}
