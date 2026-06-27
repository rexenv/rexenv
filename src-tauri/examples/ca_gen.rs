//! Manual check for local CA generation (task 3.1).
//! Run: `cargo run --example ca_gen` — generates (or loads) the CA under app-data
//! and prints its paths. Inspect with:
//!   openssl x509 -in <cert> -noout -text | grep -A1 'Basic Constraints'

use rexenv_lib::core::ssl;
use rexenv_lib::platform;

fn main() {
    let plat = platform::current();
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("generate CA");
    println!("CA cert: {}", ca.cert_path.display());
    println!("CA key:  {}", ca.key_path.display());
}
