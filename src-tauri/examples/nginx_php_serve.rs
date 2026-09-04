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
/// Same docroot + pool as the served site, on nginx's own fastcgi timeout.
const CONTROL_DOMAIN: &str = "test6-default-timeout.test";

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
        // `?slow` outlives nginx's own 60s fastcgi_read_timeout. sleep() is not
        // CPU time, so PHP's max_execution_time doesn't touch it and php-fpm's
        // 300s request_terminate_timeout is the only guard that would.
        "<?php if (isset($_GET['slow'])) { sleep(66); echo \"rexenv-slow-ok\\n\"; exit; }\n\
         echo 'rexenv-php-ok '.PHP_VERSION.\"\\n\";\n\
         echo 'upload='.ini_get('upload_max_filesize').' post='.ini_get('post_max_size').\"\\n\"; ?>\n",
    )
    .unwrap();
    fs::write(docroot.join("hi.txt"), "rexenv-static-ok\n").unwrap();

    // PHP-FPM pool.
    // max_execution_time 120: PHP's OWN limit must not be what stops the slow
    // probe, or the nginx legs below prove nothing. (Its default 30 does stop it
    // — this build enforces max_execution_time against a plain `sleep()`, so the
    // "CPU time only on unix" folklore does not hold here. Discovered by this
    // example failing, 6 Aug 2026.) php-fpm's request_terminate_timeout stays at
    // its 300s floor, which is what should end a runaway request.
    let fpm_settings = [("max_execution_time".to_string(), "120".to_string())];
    let fpm_conf =
        services::write_fpm_config(&*plat, "8.3", FPM_PORT, None, &fpm_settings).unwrap();
    let fpm = services::start_fpm(&*plat, &fpm_bin, &fpm_conf).expect("start php-fpm");
    let mut fpm = Reaped::new(fpm, FPM_PORT, "php-fpm");

    // Shared nginx.
    let site = services::NginxSite {
        domain: domain.into(),
        docroot: docroot.clone(),
        php_fpm_port: FPM_PORT,
        rewrite: services::RewriteMode::Single,
        body_limit: None,
        read_timeout: Some(adminer::IMPORT_TIMEOUT_SECS),
        // Exercise the Adminer vhost's per-request ini override through a REAL
        // `nginx -t` + php-fpm: the `\n` escape must survive nginx's parser and
        // reach PHP as two ini lines (a literal newline fails to parse).
        php_value: Some(adminer::import_php_value(adminer::MAX_IMPORT_BYTES)),
        env: Vec::new(),
        storage_root: None,
        aliases: Vec::new(),
    };
    // Negative control: the SAME docroot and pool on nginx's own default timeout.
    // Without it, "the slow request survived" proves nothing — it would pass just
    // as well if nginx had no 60s default at all, which is the thing the tuned
    // vhost exists to defeat.
    let control = services::NginxSite {
        domain: CONTROL_DOMAIN.into(),
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
        vec![site, control],
        Vec::new(),
    )
    .unwrap();
    services::test_nginx_config(&*plat, &nginx_bin, &conf, &prefix).expect("nginx -t");
    checks.is("nginx -t accepts the generated config", true, "");
    let nginx = services::start_nginx(&*plat, &nginx_bin, &conf, &prefix).expect("start nginx");
    let mut nginx = Reaped::new(nginx, HTTP_PORT, "nginx");

    // Readiness, not a timer — see `common::await_listening` for the incident
    // this pattern produced (a flat sleep loses under CPU contention, and the
    // failure then reads as the SERVER being broken rather than as too-early).
    common::await_listening(FPM_PORT, "php-fpm", None);
    common::await_listening(HTTP_PORT, "nginx", None);
    checks.is("php-fpm listening", services::fpm_running(FPM_PORT), "port closed");
    checks.is("nginx listening", services::nginx_running(HTTP_PORT), "port closed");

    // The probes the header used to delegate to a human's curl.
    let php = common::http_get(HTTP_PORT, domain, "/");
    checks.is("PHP served via FastCGI", php.contains("rexenv-php-ok"), &php);
    // The pool sets no upload keys at all (PHP's own 2M/8M defaults stand), so
    // the cap in the response can only have come from the PHP_VALUE param —
    // proof the `\n` escape reached php-fpm as two ini lines, not one garbled
    // key. The CONTROL vhost shares this pool and gets no param: it must still
    // read 2M/8M, or the param leaked pool-wide and every site's limits moved.
    let cap = adminer::MAX_IMPORT_BYTES;
    checks.is(
        "PHP_VALUE raises upload_max_filesize + post_max_size for the vhost",
        php.contains(&format!("upload={cap} post={cap}")),
        &php,
    );
    let unset = common::http_get(HTTP_PORT, CONTROL_DOMAIN, "/");
    checks.is(
        "the param does not leak to another vhost on the same pool",
        unset.contains("upload=2M post=8M"),
        &unset,
    );
    let stat = common::http_get(HTTP_PORT, domain, "/hi.txt");
    checks.is("static file served", stat.contains("rexenv-static-ok"), &stat);

    // The 504 leg — the ONE probe here that can't be a substring assertion, and
    // the reason this example costs ~70s: nginx's default only reveals itself
    // after 60 seconds. Both vhosts are hit CONCURRENTLY so it's one wait, not
    // two. Neither line means anything alone: the control must 504 (nginx's
    // default is real and does bite this exact request) for the tuned vhost's
    // 200 to mean the timeout is what stopped it biting.
    let slow_wait = Duration::from_secs(90);
    let control_probe =
        thread::spawn(move || common::http_get_timeout(HTTP_PORT, CONTROL_DOMAIN, "/?slow=1", slow_wait));
    let slow = common::http_get_timeout(HTTP_PORT, domain, "/?slow=1", slow_wait);
    let control_slow = control_probe.join().unwrap_or_default();
    checks.is(
        "a 66s request survives on the tuned vhost",
        slow.contains("rexenv-slow-ok"),
        &slow,
    );
    checks.is(
        "control: the same request 504s on nginx's own default",
        control_slow.contains("504"),
        &control_slow,
    );

    nginx.reap();
    fpm.reap();
    checks.is("nginx stopped", !services::nginx_running(HTTP_PORT), "still listening");
    checks.is("php-fpm stopped", !services::fpm_running(FPM_PORT), "still listening");
    checks.verdict()
}
