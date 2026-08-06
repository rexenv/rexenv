//! Live check: shared nginx → php-fpm over FastCGI, self-probed. Run:
//! `cargo run --example nginx_php_serve`
//!
//! SANDBOXED (was the exact incident-3 shape: real config dir, real
//! `nginx.pid`, a docroot written into the real app-data sites tree, and the
//! production ports 18088/9783 — an example run could clear the running
//! stack's pid file and break its next reload). Now: `common::sandbox` paths,
//! fixture ports, sandbox docroot, `Reaped` guards, and the probes the doc
//! header used to ask a human to run with curl.

use rexenv_lib::core::{adminer, binaries, services};
use std::fs;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

mod common;
use common::Reaped;

const HTTP_PORT: u16 = 18131; // fixture — never services::NGINX_HTTP_PORT
const FPM_PORT: u16 = 9791; // fixture — never services::PHP_FPM_PORT

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, sandbox) = common::sandbox("nginx_php_serve");
    let mut checks = common::Check::new("nginx_php_serve");
    let domain = "test6.test";

    let fpm_bin = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION)
        .await
        .expect("resolve php-fpm");
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION)
        .await
        .expect("resolve nginx");

    // Docroot with a PHP file (+ a static file) — inside the sandbox.
    let docroot = sandbox.root().join("sites").join(domain).join("public");
    fs::create_dir_all(&docroot).unwrap();
    fs::write(
        docroot.join("index.php"),
        "<?php echo 'rexenv-php-ok '.PHP_VERSION.\"\\n\";\n\
         echo 'upload='.ini_get('upload_max_filesize').' post='.ini_get('post_max_size').\"\\n\"; ?>\n",
    )
    .unwrap();
    fs::write(docroot.join("hi.txt"), "rexenv-static-ok\n").unwrap();

    // PHP-FPM pool.
    let fpm_conf = services::write_fpm_config(&*plat, "8.3", FPM_PORT, None, &[]).unwrap();
    let fpm = services::start_fpm(&*plat, &fpm_bin, &fpm_conf).expect("start php-fpm");
    let mut fpm = Reaped::new(fpm, FPM_PORT, "php-fpm");

    // Shared nginx.
    let site = services::NginxSite {
        domain: domain.into(),
        docroot: docroot.clone(),
        php_fpm_port: FPM_PORT,
        rewrite: services::RewriteMode::Single,
        body_limit: None,
        // Exercise the Adminer vhost's per-request ini override through a REAL
        // `nginx -t` + php-fpm: the `\n` escape must survive nginx's parser and
        // reach PHP as two ini lines (a literal newline fails to parse).
        php_value: Some(adminer::IMPORT_PHP_VALUE.to_string()),
        env: Vec::new(),
    };
    let (conf, prefix) = services::write_nginx_config(&*plat, HTTP_PORT, vec![site]).unwrap();
    services::test_nginx_config(&*plat, &nginx_bin, &conf, &prefix).expect("nginx -t");
    checks.is("nginx -t accepts the generated config", true, "");
    let nginx = services::start_nginx(&*plat, &nginx_bin, &conf, &prefix).expect("start nginx");
    let mut nginx = Reaped::new(nginx, HTTP_PORT, "nginx");

    thread::sleep(Duration::from_millis(800));
    checks.is("php-fpm listening", services::fpm_running(FPM_PORT), "port closed");
    checks.is("nginx listening", services::nginx_running(HTTP_PORT), "port closed");

    // The probes the header used to delegate to a human's curl.
    let php = common::http_get(HTTP_PORT, domain, "/");
    checks.is("PHP served via FastCGI", php.contains("rexenv-php-ok"), &php);
    // The pool was written with NO settings (PHP's own 2M/8M defaults), so
    // 2048M in the response can only have come from the PHP_VALUE param — proof
    // the `\n` escape reached php-fpm as two ini lines, not one garbled key.
    checks.is(
        "PHP_VALUE raises upload_max_filesize + post_max_size for the vhost",
        php.contains("upload=2048M post=2048M"),
        &php,
    );
    let stat = common::http_get(HTTP_PORT, domain, "/hi.txt");
    checks.is("static file served", stat.contains("rexenv-static-ok"), &stat);

    nginx.reap();
    fpm.reap();
    checks.is("nginx stopped", !services::nginx_running(HTTP_PORT), "still listening");
    checks.is("php-fpm stopped", !services::fpm_running(FPM_PORT), "still listening");
    checks.verdict()
}
