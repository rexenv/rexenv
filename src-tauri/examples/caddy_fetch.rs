//! Manual check for the Caddy binary provider (task 4.1).
//! Run: `cargo run --example caddy_fetch` — downloads + verifies + extracts +
//! signs Caddy under app-data, then runs `caddy version` from the cached path.

use rexenv_lib::core::binaries;
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    match binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await {
        Ok(path) => {
            println!("caddy cached at: {}", path.display());
            let out = std::process::Command::new(&path)
                .arg("version")
                .output()
                .expect("run caddy");
            print!("caddy version => {}", String::from_utf8_lossy(&out.stdout));
            if !out.status.success() {
                eprintln!("stderr: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
        Err(e) => eprintln!("resolve failed: {e}"),
    }
}
