//! Live check: one PHP-FPM pool — config written, validated by the real
//! `php-fpm -t`, master started, port probed, reaped. Run:
//! `cargo run --example php_fpm_serve`
//!
//! SANDBOXED (previously wrote the pool config into the REAL config dir and
//! bound the production `PHP_FPM_PORT`, with a bare end-of-main stop that a
//! panic skipped): `common::sandbox` paths, a fixture port, a `Reaped` guard.

use rexenv_lib::core::{binaries, services};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

mod common;
use common::Reaped;

const FPM_PORT: u16 = 9792; // fixture — never services::PHP_FPM_PORT

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _sandbox) = common::sandbox("php_fpm_serve");
    let mut checks = common::Check::new("php_fpm_serve");

    let fpm = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION)
        .await
        .expect("resolve php-fpm");
    let conf =
        services::write_fpm_config(&*plat, "8.3", FPM_PORT, None, &[]).expect("write fpm config");
    println!("FPM_CONFIG={}", conf.display());

    services::test_fpm_config(&*plat, &fpm, &conf).expect("php-fpm -t");
    checks.is("php-fpm -t accepts the generated config", true, "");

    let child = services::start_fpm(&*plat, &fpm, &conf).expect("start php-fpm");
    println!("FPM_PID={}", child.id());
    let mut master = Reaped::new(child, FPM_PORT, "php-fpm");
    thread::sleep(Duration::from_millis(800));
    checks.is("master listening on the fixture port", services::fpm_running(FPM_PORT), "closed");

    master.reap();
    thread::sleep(Duration::from_millis(300));
    checks.is("stopped and port freed", !services::fpm_running(FPM_PORT), "still listening");
    checks.verdict()
}
