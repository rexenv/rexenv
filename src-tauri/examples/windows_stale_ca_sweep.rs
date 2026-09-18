//! Live check for the Windows stale-CA sweep (`platform/windows/cert_store.rs`,
//! ledger #678/#686). Run: `scripts/live-checks.sh system windows_stale_ca_sweep`.
//!
//! **System tier, Windows only.** It adds a THROWAWAY CA — made by the production
//! generator, so its subject reads "rexenv Local CA" exactly like the real one —
//! to the real `CurrentUser\Root` store, then asks the sweep to take it back out.
//! Windows raises its own confirmation for each change; answer **Yes** to both:
//!
//! 1. "Security Warning: You are about to install a certificate…" → **Yes**.
//! 2. The same warning for the REMOVAL → **Yes**.
//!
//! **Why the real CA is passed as `current` rather than a second fixture one.**
//! The sweep deletes every rexenv CA that is not `current`, matched on the exact
//! common name — so a run that named two throwaways as the pair would delete the
//! machine's real rexenv root as a third. Naming the real one as `current` makes
//! this check exercise the rule it claims (keep the current, take the rest) with
//! the only outcome it could get wrong being the one worth finding.
//!
//! It still CAN get it wrong, and that is the point: if the sweep is broken the
//! real CA goes too, and rexenv's sites stop being trusted on this machine until
//! it is re-trusted. This example never guesses about that — it prints the exact
//! state it found and, when the real CA is gone, the file to re-trust it from.
//!
//! Cleanup is fixture-owned: the throwaway leaves by ITS OWN thumbprint, never by
//! name, because the real CA has the same name.

use rexenv_lib::core::ssl;
use rexenv_lib::platform;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Certificates in `CurrentUser\Root` whose subject names rexenv, as thumbprints.
/// Read through PowerShell rather than through the module under test: a check that
/// counts with the code it is checking agrees with itself by construction.
fn rexenv_roots() -> Vec<String> {
    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-ChildItem Cert:\\CurrentUser\\Root | \
             Where-Object { $_.Subject -like '*rexenv Local CA*' } | \
             ForEach-Object { $_.Thumbprint }",
        ])
        .output()
        .expect("powershell");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_uppercase())
        .filter(|l| !l.is_empty())
        .collect()
}

fn thumbprint_of(pem: &Path) -> String {
    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "(New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 \
                 '{}').Thumbprint",
                pem.display()
            ),
        ])
        .output()
        .expect("powershell");
    String::from_utf8_lossy(&out.stdout).trim().to_uppercase()
}

struct Fixture {
    dir: PathBuf,
    thumbprint: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if rexenv_roots().contains(&self.thumbprint) {
            eprintln!(
                "\nLEFT BEHIND: the throwaway CA is still a trusted root. Remove it by \
                 THUMBPRINT (never by name — the real CA shares it):\n  \
                 Remove-Item -Path Cert:\\CurrentUser\\Root\\{} \n  \
                 and then delete {}",
                self.thumbprint,
                self.dir.display()
            );
            return;
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn main() {
    if !cfg!(target_os = "windows") {
        eprintln!("windows_stale_ca_sweep: Windows only — this store does not exist here");
        std::process::exit(1);
    }
    let plat = platform::current();
    let trust = plat.cert_trust();

    // The REAL CA, at the path production uses. Not spelled here: a fixture that
    // writes the path it is checking can be right about a file nobody else reads.
    let real = ssl::ca_dir(plat.paths())
        .expect("ca dir")
        .join(ssl::CA_CERT_FILE);
    assert!(
        real.exists(),
        "no CA at {} — run rexenv once on this machine first",
        real.display()
    );
    let real_thumb = thumbprint_of(&real);
    let before = rexenv_roots();
    assert!(
        before.contains(&real_thumb),
        "the real CA ({real_thumb}) is not a trusted root, so there is no 'current' to keep. \
         Trust it from the app first. Roots seen: {before:?}"
    );
    println!("real CA {real_thumb} is trusted; {} rexenv root(s) before", before.len());

    // The throwaway: the production generator, so the subject matches exactly.
    let dir = std::env::temp_dir().join(format!("rexenv-stale-ca-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let (cert_pem, _key_pem) = ssl::generate_ca().expect("generate");
    let pem = dir.join("throwaway-ca.pem");
    std::fs::write(&pem, &cert_pem).expect("write");
    let fixture = Fixture { dir, thumbprint: thumbprint_of(&pem) };
    assert_ne!(fixture.thumbprint, real_thumb, "the throwaway must be a different certificate");

    println!("\n[1/2] answer YES to the install warning for {}", fixture.thumbprint);
    trust.trust_ca(&pem).expect("trust the throwaway");
    let mid = rexenv_roots();
    assert!(
        mid.contains(&fixture.thumbprint) && mid.contains(&real_thumb),
        "expected both roots present before the sweep; saw {mid:?}"
    );
    println!("both roots present ({})", mid.len());

    println!("\n[2/2] answer YES to the removal warning");
    let swept = trust.untrust_stale(&real).expect("sweep");
    let after = rexenv_roots();

    // Three separate claims, each named, because a single "== [real]" would pass
    // for the wrong reason if the store had been empty or the count were a guess.
    assert!(
        after.contains(&real_thumb),
        "THE SWEEP TOOK THE CURRENT CA. rexenv's sites are no longer trusted on this \
         machine. Re-trust it from {} (the app's Settings → Local CA does the same). \
         Roots now: {after:?}",
        real.display()
    );
    assert!(
        !after.contains(&fixture.thumbprint),
        "the stale CA is still trusted — the sweep reported {swept} but left {}",
        fixture.thumbprint
    );
    assert_eq!(swept, 1, "expected exactly one sweep; roots before {mid:?}, after {after:?}");
    assert_eq!(after.len(), before.len(), "the store must end as it started: {after:?}");

    println!(
        "\nOK — swept {swept}; the current CA survived and the stale one is gone \
         ({} root(s), as before)",
        after.len()
    );
}
