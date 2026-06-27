//! Manual check for the FrankenPHP binary provider (Phase 2 task 2.1).
//! Run: `cargo run --example frankenphp_fetch` — downloads + verifies + signs
//! the static FrankenPHP binary under app-data (reusing the Phase-1 BinaryProvider
//! + prepare_binary path), then runs `frankenphp version` from the cached path.

use rexenv_lib::core::binaries;
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    match binaries::resolve(&*plat, "frankenphp", binaries::FRANKENPHP_VERSION).await {
        Ok(path) => {
            println!("frankenphp cached at: {}", path.display());
            let out = std::process::Command::new(&path)
                .arg("version")
                .output()
                .expect("run frankenphp version");
            print!("{}", String::from_utf8_lossy(&out.stdout));
            if !out.status.success() {
                eprintln!("stderr: {}", String::from_utf8_lossy(&out.stderr));
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("resolve failed: {e}");
            std::process::exit(1);
        }
    }
}
