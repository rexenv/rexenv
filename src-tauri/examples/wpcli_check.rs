//! Manual check for WP-CLI via the bundled PHP (task 9.1).
//! `cargo run --example wpcli_check` — resolves PHP + wp-cli.phar and runs
//! `php wp-cli.phar --info`.

use rexenv_lib::core::{binaries, wordpress};
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let php = binaries::resolve(&*plat, "php", binaries::pins().php)
        .await
        .expect("resolve php");
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli)
        .await
        .expect("resolve wp-cli");
    println!("php:    {}", php.display());
    println!("wp-cli: {}", wp.display());

    let out = wordpress::wp_cli(&php, &wp, &["--info"], None).expect("run wp --info");
    print!("{}", String::from_utf8_lossy(&out.stdout));
    if !out.status.success() {
        eprintln!("stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    }
}
