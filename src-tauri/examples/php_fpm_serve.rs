//! Live check: one PHP-FPM pool — config written, validated by the real
//! `php-fpm -t`, master started, port probed, reaped. Run:
//! `cargo run --example php_fpm_serve [<version>]`
//!
//! The version is an ARGUMENT (default: the pinned default) because the config
//! generator emits one file for every minor, and "php-fpm accepts it" is a claim
//! about each minor's OWN binary — not about the newest one. It matters most at
//! the edges of the range: rexenv's generator was written against 8.x, and
//! **PHP 7.4's php-fpm is a different program** that can reject a directive 8.x
//! takes. Run it for 7.4.33 and the answer stops being an assumption:
//! `cargo run --example php_fpm_serve 7.4.33`
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

    // Default to the pinned default; accept any pinned version as argv[1].
    let version = std::env::args()
        .nth(1)
        .unwrap_or_else(|| binaries::PHP_VERSION.to_string());
    let minor = rexenv_lib::core::php::minor_of(&version);
    assert!(
        rexenv_lib::core::php::patch_for_minor(&minor).is_some(),
        "{version} is not a pinned build — this check must run against a version rexenv ships"
    );
    println!("php-fpm version under test: {version} (minor {minor})\n");

    let fpm = binaries::resolve(&*plat, "php-fpm", &version)
        .await
        .expect("resolve php-fpm");
    let conf = services::write_fpm_config(&*plat, &minor, FPM_PORT, None, &[])
        .expect("write fpm config");
    println!("FPM_CONFIG={}", conf.display());

    services::test_fpm_config(&*plat, &fpm, &conf).expect("php-fpm -t");
    checks.is(
        &format!("php-fpm {minor} -t accepts the config rexenv generated for it"),
        true,
        "",
    );

    let child = services::start_fpm(&*plat, &fpm, &conf).expect("start php-fpm");
    println!("FPM_PID={}", child.id());
    let mut master = Reaped::new(child, FPM_PORT, "php-fpm");
    // Readiness, not a timer — see `common::await_listening` for the incident
    // this pattern produced (a flat sleep loses under CPU contention, and the
    // failure then reads as the SERVER being broken rather than as too-early).
    common::await_listening(FPM_PORT, "php-fpm", None);
    checks.is(
        &format!("php-fpm {minor} master listening on the fixture port"),
        services::fpm_running(FPM_PORT),
        "closed",
    );

    master.reap();
    thread::sleep(Duration::from_millis(300));
    checks.is("stopped and port freed", !services::fpm_running(FPM_PORT), "still listening");
    checks.verdict()
}
