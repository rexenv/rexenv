//! Manual check for the Nginx binary provider (task 6.1).
//! Run: `cargo run --example nginx_fetch` — downloads + verifies + signs the
//! static nginx under app-data, then runs `nginx -v` from the cached path.

use rexenv_lib::core::binaries;
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    match binaries::resolve(&*plat, "nginx", binaries::pins().nginx).await {
        Ok(path) => {
            println!("nginx cached at: {}", path.display());
            // `nginx -v` prints to stderr and exits without starting.
            let out = std::process::Command::new(&path)
                .arg("-v")
                .output()
                .expect("run nginx");
            print!("{}", String::from_utf8_lossy(&out.stderr));
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        Err(e) => eprintln!("resolve failed: {e}"),
    }
}
