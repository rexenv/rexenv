//! Phase-3 §11.1 check: DNS status probe + SSL re-issue + autostart round-trip.
//!
//! Verifies the building blocks behind the Settings "DNS & SSL + autostart" card:
//!   - the autostart LaunchAgent round-trips (enable → is_enabled → disable), and
//!     is fully removed afterward (cleanup runs BEFORE asserts so a failure can't
//!     leave a stray `~/Library/LaunchAgents/dev.rexenv.rexenv.plist`);
//!   - the UDP in-use probe `dns_status` relies on (the resolver binds UDP, so the
//!     TCP listen check doesn't apply);
//!   - regenerating a site cert produces FRESH key material while keeping the
//!     wildcard SAN + local-CA issuer (`openssl` dump).
//!
//! Run: `cargo run --example dns_ssl_autostart_check`

use rexenv_lib::core::ssl;
use rexenv_lib::platform;
use std::net::{Ipv4Addr, UdpSocket};
use std::process::Command;

fn main() {
    let plat = platform::current();

    // 1) Autostart round-trip — clean up before asserting (never leak the agent).
    let auto = plat.autostart();
    auto.enable().expect("enable autostart");
    let enabled = auto.is_enabled().expect("is_enabled");
    auto.disable().expect("disable autostart");
    let disabled_after = auto.is_enabled().expect("is_enabled");
    assert!(enabled, "autostart should be enabled after enable()");
    assert!(!disabled_after, "autostart should be off after disable()");
    let leftover = dirs_home()
        .join("Library/LaunchAgents/dev.rexenv.rexenv.plist")
        .exists();
    assert!(!leftover, "LaunchAgent plist leaked after disable()");
    println!("✓ autostart enable → is_enabled=true → disable → is_enabled=false (no leftover plist)");

    // 2) DNS UDP in-use probe (the technique `dns_status` uses).
    let held = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind probe");
    let port = held.local_addr().unwrap().port();
    let in_use = UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).is_err();
    assert!(in_use, "a second UDP bind to the held port should fail (= 'running')");
    let free = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).is_ok();
    assert!(free, "an unbound UDP port should be free (= 'stopped')");
    println!("✓ DNS UDP probe: held port reads in-use (running), unbound reads free (stopped)");
    println!(
        "  OS resolver path = {}",
        plat.dns().resolver_path(rexenv_lib::core::tld::BACKBONE_TLD).display()
    );

    // 3) Cert regeneration: re-issue → fresh material, same wildcard SAN + CA issuer.
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let dir = std::env::temp_dir().join("rexenv-11_1-cert");
    let _ = std::fs::remove_dir_all(&dir);
    let cert_path = dir.join(ssl::SITE_CERT_FILE);
    let key_path = dir.join(ssl::SITE_KEY_FILE);
    let first = ssl::ensure_site_cert_at(&cert_path, &key_path, &ca, "regen.test").expect("issue");
    // "Regenerate" = delete then re-issue (the idempotent path would otherwise reuse).
    std::fs::remove_dir_all(&dir).unwrap();
    let second = ssl::ensure_site_cert_at(&cert_path, &key_path, &ca, "regen.test").expect("re-issue");
    assert_ne!(first.key_pem, second.key_pem, "regenerated cert must have fresh key material");
    println!("✓ regenerate re-issues fresh key material (key PEM changed)");

    let out = Command::new("openssl")
        .args(["x509", "-noout", "-issuer", "-ext", "subjectAltName", "-in"])
        .arg(&second.cert_path)
        .output();
    match out {
        Ok(o) => {
            let info = String::from_utf8_lossy(&o.stdout);
            println!("--- regen.test leaf ---\n{}", info.trim());
            assert!(info.contains("*.regen.test"), "leaf SAN missing the wildcard");
            assert!(info.to_lowercase().contains("rexenv local ca"), "issuer is not our CA");
            println!("✓ openssl: SAN DNS:*.regen.test + issuer rexenv Local CA");
        }
        Err(_) => println!("(openssl unavailable — skipped SAN/issuer dump)"),
    }
    let _ = std::fs::remove_dir_all(&dir);

    println!("\nALL GOOD — autostart round-trips cleanly, DNS UDP probe works, certs re-issue with wildcard SAN + CA issuer.");
}

fn dirs_home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"))
}
