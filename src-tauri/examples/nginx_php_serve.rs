//! Manual check for shared nginx → php-fpm (task 6.2).
//! Starts the shared php-fpm pool + nginx, serving a PHP docroot for ~12s. Probe:
//!   curl -H 'Host: test6.test' http://127.0.0.1:8088/        # PHP via FastCGI
//!   curl -H 'Host: test6.test' http://127.0.0.1:8088/hi.txt  # static file

use rexenv_lib::core::{binaries, services};
use rexenv_lib::platform;
use std::fs;
use std::thread;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "test6.test";

    let fpm_bin = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION)
        .await
        .expect("resolve php-fpm");
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION)
        .await
        .expect("resolve nginx");

    // Docroot with a PHP file (+ a static file).
    let docroot = plat
        .paths()
        .app_data_dir()
        .unwrap()
        .join("sites")
        .join(domain)
        .join("public");
    fs::create_dir_all(&docroot).unwrap();
    fs::write(
        docroot.join("index.php"),
        "<?php echo 'rexenv-php-ok '.PHP_VERSION.\"\\n\"; ?>\n",
    )
    .unwrap();
    fs::write(docroot.join("hi.txt"), "rexenv-static-ok\n").unwrap();

    // PHP-FPM pool.
    let fpm_conf = services::write_fpm_config(&*plat, "8.3", services::PHP_FPM_PORT).unwrap();
    let mut fpm = services::start_fpm(&*plat, &fpm_bin, &fpm_conf).expect("start php-fpm");

    // Shared nginx.
    let site = services::NginxSite {
        domain: domain.into(),
        docroot: docroot.clone(),
        php_fpm_port: services::PHP_FPM_PORT,
        rewrite: services::RewriteMode::Single,
    };
    let (conf, prefix) =
        services::write_nginx_config(&*plat, services::NGINX_HTTP_PORT, vec![site]).unwrap();
    services::test_nginx_config(&*plat, &nginx_bin, &conf, &prefix).expect("nginx -t");
    println!("nginx -t: OK");
    let mut nginx = services::start_nginx(&*plat, &nginx_bin, &conf, &prefix).expect("start nginx");

    thread::sleep(Duration::from_millis(800));
    println!(
        "READY domain={domain} http=127.0.0.1:{} fpm_running={} nginx_running={}",
        services::NGINX_HTTP_PORT,
        services::fpm_running(services::PHP_FPM_PORT),
        services::nginx_running(services::NGINX_HTTP_PORT),
    );

    thread::sleep(Duration::from_secs(12));

    let _ = services::stop(&*plat, nginx.id());
    let _ = nginx.wait();
    let _ = services::stop(&*plat, fpm.id());
    let _ = fpm.wait();
    println!("stopped");
}
