//! Pull a live site INTO a local WordPress site (L6 of `docs/PLAN-wp-live-sync.md`,
//! §2.4). The core of the job; the job card, the New-site flow and the stored key
//! are later tasks.
//!
//! The order is the safety argument:
//! 1. every table is exported (`.partial` + sha256 per chunk) and imported into a
//!    STAGING database, `<db>_pull` — the local site is not touched;
//! 2. the staging copy's URLs are moved from the live host to the local domain
//!    (`rehome_urls_on_copy`, which re-reads `siteurl` to prove it);
//! 3. only then ONE `RENAME TABLE` swaps: the local site's tables move to
//!    `<db>_prepull` (kept — the previous local state, one step back), the staging
//!    tables move in. A failure before this leaves the local site exactly as it was
//!    (plan invariant #6); the staging database is dropped;
//! 4. files land in a staging folder inside `wp-content` and are moved into place
//!    one by one. Nothing local is deleted (the sync base that would say what was
//!    deleted on live is S3).
//!
//! MySQL/MariaDB only: a live WordPress is MySQL, and a local PostgreSQL site has
//! nothing to receive it.

use super::client::Client;
use crate::core::db::{DbEngine, SqlClient};
use crate::core::wordpress;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};

/// Paths a pull never asks for (plan §5 — rexenv's list, in ONE place): caches,
/// backup plugins' archives, logs, the plugin's own quarantine.
pub const DEFAULT_EXCLUDES: [&str; 9] = [
    "cache/*",
    "*/cache/*",
    "updraft/*",
    "ai1wm-backups/*",
    "backups-dup-lite/*",
    "uploads/backwpup*",
    "*.log",
    "uploads/rexsync-*",
    "upgrade/*",
];

/// The local site a pull writes into.
pub struct LocalSite<'a> {
    pub platform: &'a dyn Platform,
    pub engine: DbEngine,
    pub client: &'a SqlClient,
    /// The engine's dump binary (`sql_client_bins`' second half) — the push dumps with it.
    pub dump: PathBuf,
    pub port: u16,
    pub db_name: &'a str,
    pub domain: &'a str,
    /// The SERVED root (`wp-config.php`, `wp-content/`).
    pub docroot: &'a Path,
    pub php: &'a Path,
    pub wp_phar: &'a Path,
    /// Private scratch space in app data (dumps, the wp-cli override).
    pub scratch: &'a Path,
}

/// What a pull did.
#[derive(Debug, Clone, Default)]
pub struct PullReport {
    pub tables: usize,
    pub rows: u64,
    pub files: usize,
    pub refused_files: Vec<String>,
    pub replacements: u64,
    /// The database now holding the local site as it was before this pull.
    pub backup_db: String,
    /// What the live site looked like — the next push's base (§2.6).
    pub base: super::base::SyncBase,
}

/// The host of `https://host[:port][/path]` — what the copied database says.
pub fn host_of(url: &str) -> String {
    url.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or("").to_string()
}

fn q(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn exec(local: &LocalSite, sql: &str, what: &str) -> Result<()> {
    crate::core::database::mysql_exec(local.client, local.port, sql, what)
}

/// Pull everything the live site has into `local`. `on_line` gets one line per step.
/// What a pull leaves out (§11 Q3).
#[derive(Debug, Clone, Default)]
pub struct PullOptions {
    /// Extra `exclude` globs beside [`DEFAULT_EXCLUDES`].
    pub excludes: Vec<String>,
    /// Skip files under `uploads/` older than this Unix time — a big media
    /// library's history stays on live.
    pub uploads_since: Option<i64>,
}

pub async fn pull_into(
    client: &Client,
    local: &LocalSite<'_>,
    options: &PullOptions,
    on_line: &mut (dyn FnMut(&str) + Send),
) -> Result<PullReport> {
    if !local.engine.is_mysql_family() {
        return Err(Error::Other("a live WordPress database can only be pulled into a MySQL or MariaDB site".into()));
    }
    let m = client.manifest().await?;
    if m.multisite {
        return Err(Error::Other("pulling a multisite network is not supported yet (plan S5)".into()));
    }
    on_line(&format!("live: {} — WordPress {}, {} tables", m.site_url, m.wp, m.tables.len()));
    let stage = format!("{}_pull", local.db_name);
    let backup = format!("{}_prepull", local.db_name);
    crate::core::database::validate_db_name(&stage)?;
    crate::core::database::validate_db_name(&backup)?;
    std::fs::create_dir_all(local.scratch)?;

    // ── 1. tables → staging ─────────────────────────────────────────────
    local.engine.drop_database(local.client, local.port, &stage)?;
    local.engine.create_database(local.client, local.port, &stage)?;
    let mut report = PullReport { backup_db: backup.clone(), ..Default::default() };
    report.base.tables = m.tables.iter().map(|t| (t.name.clone(), t.checksum.clone())).collect();
    report.base.at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let staged: Result<()> = async {
        for t in &m.tables {
            let file = local.scratch.join(format!("pull-{}.sql", t.name));
            let rows = client.export_table(&t.name, &file).await?;
            // Data from a machine rexenv does not control: under a user that can
            // reach the staging database and nothing else (#837).
            let imported = local.engine.import_from_file_scoped(local.client, local.port, &stage, &file);
            let _ = std::fs::remove_file(&file);
            imported?;
            report.rows += rows;
            report.tables += 1;
            on_line(&format!("  {} — {rows} rows", t.name));
        }
        let have = local.engine.list_tables(local.client, local.port, &stage)?;
        let missing: Vec<&str> = m.tables.iter().map(|t| t.name.as_str()).filter(|n| !have.iter().any(|h| h == n)).collect();
        if !missing.is_empty() {
            return Err(Error::Other(format!("the staging copy is missing {}", missing.join(", "))));
        }
        Ok(())
    }
    .await;
    if let Err(e) = staged {
        let _ = local.engine.drop_database(local.client, local.port, &stage);
        return Err(e);
    }

    // ── 2. the copy's URLs → the local domain (with the live table prefix) ──
    let prefix_now = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "get", "table_prefix"])?;
    let restore_prefix = |why: &str| -> Error {
        if prefix_now != m.prefix {
            let _ = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "set", "table_prefix", &prefix_now, "--type=variable"]);
        }
        let _ = local.engine.drop_database(local.client, local.port, &stage);
        Error::Other(why.to_string())
    };
    if prefix_now != m.prefix {
        on_line(&format!("wp-config: table_prefix {prefix_now} → {} (the live site's)", m.prefix));
        if let Err(e) = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "set", "table_prefix", &m.prefix, "--type=variable"]) {
            return Err(restore_prefix(&format!("setting the table prefix failed: {e}")));
        }
    }
    let from = host_of(&m.site_url);
    match wordpress::rehome_urls_on_copy(local.platform, local.php, local.wp_phar, local.docroot, local.scratch, local.port, &stage, &from, local.domain, false) {
        Ok(n) => {
            report.replacements = n;
            on_line(&format!("URLs: https://{from} → https://{} ({n} replacements)", local.domain));
        }
        Err(e) => return Err(restore_prefix(&format!("moving the URLs failed: {e}"))),
    }

    // ── 3. the swap — ONE statement ─────────────────────────────────────
    local.engine.drop_database(local.client, local.port, &backup)?;
    local.engine.create_database(local.client, local.port, &backup)?;
    let current = local.engine.list_tables(local.client, local.port, local.db_name)?;
    let incoming = local.engine.list_tables(local.client, local.port, &stage)?;
    let mut pairs: Vec<String> = current.iter().map(|t| format!("{}.{} TO {}.{}", q(local.db_name), q(t), q(&backup), q(t))).collect();
    pairs.extend(incoming.iter().map(|t| format!("{}.{} TO {}.{}", q(&stage), q(t), q(local.db_name), q(t))));
    if let Err(e) = exec(local, &format!("RENAME TABLE {}", pairs.join(", ")), "swapping the pulled tables in") {
        return Err(restore_prefix(&format!("{e}")));
    }
    let _ = local.engine.drop_database(local.client, local.port, &stage);
    on_line(&format!("database swapped in; the previous local tables are in `{backup}`"));

    // ── 4. files ────────────────────────────────────────────────────────
    let mut excludes: Vec<&str> = DEFAULT_EXCLUDES.to_vec();
    excludes.extend(options.excludes.iter().map(String::as_str));
    let mut files = client.list_files(&excludes, options.uploads_since).await?;
    report.base.files = files.iter().map(|f| (f.path.clone(), format!("{}:{}", f.size, f.mtime))).collect();
    // Ceilings (#837): a file the frame parser would refuse anyway is left on
    // live and reported, not fetched; the whole list must fit the disk with
    // room to spare, measured BEFORE the first byte lands.
    let too_big: Vec<String> = files.iter().filter(|f| f.size > super::client::MAX_FILE_BYTES).map(|f| f.path.clone()).collect();
    if !too_big.is_empty() {
        on_line(&format!("{} file(s) over {} GiB left on live: {}", too_big.len(), super::client::MAX_FILE_BYTES >> 30, too_big.join(", ")));
        files.retain(|f| f.size <= super::client::MAX_FILE_BYTES);
    }
    let total: u64 = files.iter().map(|f| f.size).sum();
    let content = local.docroot.join("wp-content");
    std::fs::create_dir_all(&content)?;
    if let Some(free) = crate::core::dbdump::free_space(&content) {
        const HEADROOM: u64 = 512 << 20;
        if total.saturating_add(HEADROOM) > free {
            return Err(Error::Other(format!(
                "the live site's files are {} MB and this disk has {} MB free — make room (or pull with recent uploads only) first.",
                total >> 20,
                free >> 20
            )));
        }
    }
    if let Some(since) = options.uploads_since {
        on_line(&format!("uploads older than {since} (Unix time) left on live"));
    }
    let incoming_dir: PathBuf = content.join(format!(".rexsync-incoming-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let fetched = client.read_files(&paths, &incoming_dir, &excludes).await;
    let refused = match fetched {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&incoming_dir); // ours: a fresh random name
            return Err(e);
        }
    };
    report.refused_files = refused.iter().map(|r| r.path.clone()).chain(too_big).collect();
    for rel in &paths {
        if report.refused_files.contains(rel) {
            continue;
        }
        let mut from = incoming_dir.clone();
        let mut to = content.clone();
        for seg in rel.split('/') {
            from.push(seg);
            to.push(seg);
        }
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::rename(&from, &to)?;
        report.files += 1;
    }
    let _ = std::fs::remove_dir_all(&incoming_dir);
    on_line(&format!("files: {} in place, {} refused by the site", report.files, report.refused_files.len()));

    // The plugin came along with the files; a local copy answering as a sync
    // endpoint means nothing (plan §2.8).
    let _ = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["plugin", "deactivate", "rexenv-sync", "--skip-plugins", "--skip-themes"]);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_host_is_read_off_the_site_url() {
        assert_eq!(host_of("https://example.com"), "example.com");
        assert_eq!(host_of("https://example.com/blog"), "example.com");
        assert_eq!(host_of("https://shop.example.com:8443/"), "shop.example.com:8443");
    }
}
