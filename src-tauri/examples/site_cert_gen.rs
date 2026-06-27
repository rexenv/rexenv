//! Manual check for per-site cert issuance (task 3.2).
//! Run: `cargo run --example site_cert_gen [domain]` (default: mysite.test).
//! Issues a cert signed by the local CA with SANs domain + *.domain. Inspect:
//!   openssl x509 -in <cert> -noout -text | grep -A1 'Subject Alternative Name'
//!   openssl verify -CAfile <ca> <cert>

use rexenv_lib::core::ssl;
use rexenv_lib::platform;

fn main() {
    let domain = std::env::args().nth(1).unwrap_or_else(|| "mysite.test".into());
    let plat = platform::current();
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let cert = ssl::ensure_site_cert(plat.paths(), plat.permissions(), &ca, &domain)
        .expect("issue site cert");
    println!("CA cert:   {}", ca.cert_path.display());
    println!("site cert: {}", cert.cert_path.display());
    println!("site key:  {}", cert.key_path.display());
}
