//! Manual check for the PHP-FPM pool (task 5.2).
//! Resolves php-fpm, writes a pool config, validates it, starts the master on a
//! loopback port for ~12s (so you can probe it), reports status, then stops.

use rexenv_lib::core::{binaries, services};
use rexenv_lib::platform;
use std::thread;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let port = services::PHP_FPM_PORT;

    let fpm = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION)
        .await
        .expect("resolve php-fpm");
    let conf = services::write_fpm_config(&*plat, "8.3", port).expect("write fpm config");
    println!("FPM_CONFIG={}", conf.display());

    services::test_fpm_config(&*plat, &fpm, &conf).expect("php-fpm -t");
    println!("config test: OK");

    let mut child = services::start_fpm(&*plat, &fpm, &conf).expect("start php-fpm");
    println!("FPM_PID={}", child.id());
    thread::sleep(Duration::from_millis(800));
    println!("FPM_READY port={port} running={}", services::fpm_running(port));

    thread::sleep(Duration::from_secs(12));

    let _ = services::stop(&*plat, child.id());
    let _ = child.wait();
    thread::sleep(Duration::from_millis(300));
    println!("stopped; running={}", services::fpm_running(port));
}
