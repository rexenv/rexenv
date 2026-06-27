//! Manual check for the MULTI-VERSION PHP binary provider (Phase 2 task 1.1).
//! Run: `cargo run --example php_versions_check`
//!
//! For every version in `binaries::PHP_VERSIONS`, this downloads + verifies +
//! extracts + signs the static-php "bulk" `php` (cli) and `php-fpm` builds under
//! app-data (reusing the Phase-1 BinaryProvider + prepare_binary path), then runs
//! `php -v` from each cached path and confirms `php-fpm -v` runs too. Each version
//! caches independently under `bin_dir/php-<version>/` and `bin_dir/php-fpm-<version>/`.

use rexenv_lib::core::binaries;
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let mut all_ok = true;

    for v in binaries::PHP_VERSIONS {
        println!("=== PHP {v} ===");

        // CLI: resolve + `php -v`.
        match binaries::resolve(&*plat, "php", v).await {
            Ok(path) => {
                println!("  php     cached at: {}", path.display());
                let out = std::process::Command::new(&path)
                    .arg("-v")
                    .output()
                    .expect("run php -v");
                let line = String::from_utf8_lossy(&out.stdout);
                print!("  {}", line.lines().next().unwrap_or("").to_string() + "\n");
                if !out.status.success() {
                    all_ok = false;
                    eprintln!("  php -v FAILED: {}", String::from_utf8_lossy(&out.stderr));
                }
            }
            Err(e) => {
                all_ok = false;
                eprintln!("  php resolve FAILED: {e}");
            }
        }

        // FPM: resolve + `php-fpm -v` (proves the fpm build of the same version).
        match binaries::resolve(&*plat, "php-fpm", v).await {
            Ok(path) => {
                println!("  php-fpm cached at: {}", path.display());
                let out = std::process::Command::new(&path)
                    .arg("-v")
                    .output()
                    .expect("run php-fpm -v");
                print!(
                    "  {}",
                    String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string()
                        + "\n"
                );
                if !out.status.success() {
                    all_ok = false;
                    eprintln!("  php-fpm -v FAILED: {}", String::from_utf8_lossy(&out.stderr));
                }
            }
            Err(e) => {
                all_ok = false;
                eprintln!("  php-fpm resolve FAILED: {e}");
            }
        }
        println!();
    }

    if all_ok {
        println!("OK — all {} PHP versions resolved + ran.", binaries::PHP_VERSIONS.len());
    } else {
        eprintln!("FAILED — see errors above.");
        std::process::exit(1);
    }
}
