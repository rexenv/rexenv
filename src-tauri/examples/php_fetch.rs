//! Manual check for the PHP static binary provider (task 5.1).
//! Run: `cargo run --example php_fetch` — downloads + verifies + extracts + signs
//! the static PHP CLI under app-data, then runs `php -v` from the cached path.

use rexenv_lib::core::binaries;
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    match binaries::resolve(&*plat, "php", binaries::pins().php).await {
        Ok(path) => {
            println!("php cached at: {}", path.display());
            let out = std::process::Command::new(&path)
                .arg("-v")
                .output()
                .expect("run php");
            print!("{}", String::from_utf8_lossy(&out.stdout));
            if !out.status.success() {
                eprintln!("stderr: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
        Err(e) => eprintln!("resolve failed: {e}"),
    }
}
