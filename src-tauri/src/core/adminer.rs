//! core::adminer — serve the bundled Adminer DB browser as an internal vhost (§5.2).
//!
//! Adminer is a single `adminer.php` (§5.1). We serve it through the shared stack
//! on a fixed internal host (`adminer.rexenv.test`) rooted at an isolated docroot,
//! behind the edge (TLS, local CA). It is deliberately NOT a `Site` in the DB —
//! so it can never be selected as a public tunnel origin (§9): tunnels are scoped
//! to a single site's Host, and this internal vhost isn't one.

use crate::core::binaries;
use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::PathBuf;

/// Internal host Adminer is served on (resolved by the embedded `*.test` DNS).
/// NEVER a public tunnel origin (§9) — it isn't a site.
pub const ADMINER_HOST: &str = "adminer.rexenv.test";

/// Web docroot for Adminer, isolated from the binary cache.
pub fn docroot(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("adminer"))
}

/// Ensure the Adminer docroot exists with the bundled file as `index.php`.
/// Downloads + caches `adminer.php` (§5.1) on first use, then copies it in.
/// Idempotent: re-copies only if the docroot copy is missing or a different size.
pub async fn ensure(platform: &dyn Platform) -> Result<PathBuf> {
    let dir = docroot(platform)?;
    std::fs::create_dir_all(&dir)?;
    let index = dir.join("index.php");
    let src = binaries::resolve_file(platform, "adminer", binaries::ADMINER_VERSION).await?;
    let stale = std::fs::metadata(&index).ok().map(|m| m.len())
        != std::fs::metadata(&src).ok().map(|m| m.len());
    if stale {
        std::fs::copy(&src, &index)?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_an_internal_subdomain() {
        assert!(ADMINER_HOST.ends_with(".rexenv.test"));
        // Not a wildcard / not a user site domain.
        assert_eq!(ADMINER_HOST, "adminer.rexenv.test");
    }
}
