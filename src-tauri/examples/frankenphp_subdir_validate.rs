//! Live check: every generated FrankenPHP config is valid TO THE REAL BINARY —
//! `frankenphp validate --adapter caddyfile` on all three rewrite modes, plus
//! an env-vars variant. Run:
//!
//!   cargo run --example frankenphp_subdir_validate
//!
//! This used to PRINT the config for a human to pipe into `frankenphp
//! validate` by hand — an example that asserted nothing. What the real
//! adapter DOES catch: unknown directives, malformed matchers, structural
//! errors. What it does NOT catch (probed live, 28 Jul 2026: a config
//! referencing the nonexistent `{http.regexp.typo.2}` is "Valid
//! configuration"): the shipped `wpsubph`-for-`wpsubphp` placeholder typo,
//! which resolves EMPTY at runtime only. That class is pinned at L0 instead —
//! `placeholders_reference_declared_matchers_in_every_mode` cross-checks
//! every `{http.regexp.NAME.N}` against the declared `path_regexp` names in
//! the same generated config. A live request through a served sub-site stays
//! the remaining leftover for semantics neither level can see.

use rexenv_lib::core::frankenphp;
use rexenv_lib::core::services::RewriteMode;
use rexenv_lib::core::binaries;
use std::process::Command;

#[tokio::main]
async fn main() {
    let plat = rexenv_lib::platform::current();
    let bin = binaries::resolve(&*plat, "frankenphp", binaries::FRANKENPHP_VERSION)
        .await
        .expect("frankenphp binary (cached)");
    let tmp = std::env::temp_dir().join(format!("rexenv-fp-validate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let docroot = tmp.join("site with space/public");
    std::fs::create_dir_all(&docroot).expect("temp docroot");

    let env_pair = [("APP_ENV".to_string(), "local dev".to_string())];
    // A fixture TABLE, not a type worth naming: the tuple is the row shape of
    // the four cases below and exists nowhere else.
    #[allow(clippy::type_complexity)]
    let cases: [(&str, RewriteMode, &[(String, String)]); 4] = [
        ("single", RewriteMode::Single, &[]),
        ("subdomain-multisite", RewriteMode::SubdomainMultisite, &[]),
        ("subdirectory-multisite", RewriteMode::SubdirectoryMultisite, &[]),
        ("single + site env", RewriteMode::Single, &env_pair),
    ];

    let mut ok = true;
    for (name, mode, env) in cases {
        let cfg = frankenphp::generate_config(&docroot, 8271, mode, env, None);
        let path = tmp.join(format!("{name}.Caddyfile"));
        std::fs::write(&path, &cfg).expect("write config");
        let out = Command::new(&bin)
            .args(["validate", "--adapter", "caddyfile", "--config"])
            .arg(&path)
            .output()
            .expect("run frankenphp validate");
        if out.status.success() {
            println!("  ✓ {name}: valid to the real adapter");
        } else {
            ok = false;
            println!(
                "  ✗ {name}: frankenphp validate rejected the generated config\n{}\n{}",
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
    if ok {
        println!("frankenphp_subdir_validate: PASS");
    } else {
        println!("frankenphp_subdir_validate: FAIL");
        std::process::exit(1);
    }
}
