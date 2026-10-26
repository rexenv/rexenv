//! Pull a live site INTO a local WordPress site (L6 of `docs/PLAN-wp-live-sync.md`,
//! §2.4). The core of the job; the job card, the New-site flow and the stored key
//! are `commands::live_sync`.
//!
//! The order is the safety argument:
//! 1. every table that moved on live is exported (`.partial` + sha256 per chunk)
//!    and imported into a STAGING database, `<db>_pull`; every table that did NOT
//!    move since the base is copied into the staging database from the LOCAL
//!    copy, server-side (`INSERT … SELECT`) — so the staging copy is whole and
//!    WordPress can boot against it, and the wire carries only what changed. The
//!    local site is not touched;
//! 2. the staging copy's URLs are moved from the live host to the local domain
//!    (`rehome_urls_on_copy`, which re-reads `siteurl` to prove it);
//! 3. only then ONE `RENAME TABLE` swaps: the local site's tables move to
//!    `<db>_prepull` (kept — the previous local state, one step back), the staging
//!    tables move in. A failure before this leaves the local site exactly as it was
//!    (plan invariant #6); the staging database is dropped;
//! 4. files whose `size:mtime` moved on live (every file when there is no base, or
//!    on a FULL pull) land in a staging folder inside `wp-content` and are moved
//!    into place one by one. A file the base saw on live that live no longer has
//!    is removed here ONLY if it was not touched here since the last sync (its
//!    mtime is not after the base) — a local edit is never deleted by a pull.
//!
//! "Incremental" (14 Oct 2026): a table unchanged on live keeps whatever it holds
//! here — a local edit to a table live did not touch survives a pull. A table
//! live DID change is replaced whole (live wins on a pull; the previous local
//! tables are in `_prepull`). A FULL pull (`PullOptions::full`) replaces everything,
//! as every pull did before.
//!
//! "Resume" (15 Oct 2026, `progress.rs`): a pull that died mid-way leaves its staging
//! database, its incoming folder and a progress record behind; the next pull (not a
//! full one) reuses every staged table whose live stamp has not moved, treats a table
//! an earlier run already swapped in as kept, and skips an incoming file the listing
//! still describes — nothing already verified onto this disk crosses the wire twice.
//! "Anonymise" (`anonymise.rs`): with the site's switch on, the staging copy is
//! scrubbed before the swap. "Uploads on demand" (`uploads_on_demand.rs`): the pull
//! ends by putting the mu-plugin in place, or taking it out, as the site's switch says.
//!
//! A NETWORK (S5, 16 Oct 2026): the manifest says `multisite` + `subdomain` (plugin
//! 0.1.5). The network constants go into the local wp-config BEFORE the URL move — with
//! `DOMAIN_CURRENT_SITE` as the LIVE host, so wp-cli can list the copy's blogs — then the
//! move runs network-aware (`rehome_urls_on_copy(network = true)`: each sub-site under
//! the live host first, the network last), and the constant is set to the local domain
//! after. The report carries the mode so the command records it on the site row
//! (`sites::adopt_multisite`) and the web tier gets the network rewrite rules. A pull
//! of a single site into a copy that WAS a network takes the constants out again.
//!
//! MySQL/MariaDB only: a live WordPress is MySQL, and a local PostgreSQL site has
//! nothing to receive it.

use super::base::SyncBase;
use super::client::Client;
use super::{anonymise, progress, uploads_on_demand};
use crate::core::db::{DbEngine, SqlClient};
use crate::core::wordpress;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::collections::BTreeMap;
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
    /// The edge's HTTPS port (`core::edge_ports`) — the URL move refuses anything but 443 (T2).
    pub edge_https_port: u16,
}

/// What a pull did.
#[derive(Debug, Clone, Default)]
pub struct PullReport {
    /// Tables exported from live.
    pub tables: usize,
    /// Tables unchanged on live, kept from the local copy.
    pub tables_kept: usize,
    /// Tables an interrupted pull had already staged or swapped, reused (not exported).
    pub tables_resumed: usize,
    /// Files an interrupted pull had already verified into the incoming folder, reused.
    pub files_resumed: usize,
    /// Tables the anonymise rules touched in the copy.
    pub anonymised: Vec<String>,
    pub rows: u64,
    /// Files fetched from live.
    pub files: usize,
    /// Files unchanged on live, left as they are here.
    pub files_kept: usize,
    /// Files gone from live, removed here (untouched here since the last sync).
    pub files_deleted: Vec<String>,
    /// Files gone from live but edited here since the last sync — kept, named.
    pub files_kept_edited: Vec<String>,
    pub refused_files: Vec<String>,
    pub replacements: u64,
    /// The database now holding the local site as it was before this pull.
    pub backup_db: String,
    /// What both sides look like now — the next pull's and push's base (§2.6).
    pub base: SyncBase,
    /// The live site's network mode, as the local copy is now: `Some("subdomain" |
    /// "subdirectory")` for a network, `None` for a single site.
    pub network: Option<&'static str>,
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

/// What a pull leaves out (§11 Q3), and whether it is a FULL one.
#[derive(Debug, Clone, Default)]
pub struct PullOptions {
    /// Extra `exclude` globs beside [`DEFAULT_EXCLUDES`].
    pub excludes: Vec<String>,
    /// Skip files under `uploads/` older than this Unix time — a big media
    /// library's history stays on live.
    pub uploads_since: Option<i64>,
    /// Replace EVERY table and fetch EVERY file, base or not — "Pull everything again".
    /// A full pull never resumes an interrupted one either.
    pub full: bool,
    /// Scrub personal data in the copy before the swap (`anonymise.rs`).
    pub anonymise: bool,
    /// Put the uploads-on-demand mu-plugin in place (`Some(true)`), take it out
    /// (`Some(false)`), or leave it (`None`) once the pull is done.
    pub uploads_on_demand: Option<bool>,
}

/// `*`-only glob on a `/` path, the way the plugin's `fnmatch` reads the list.
pub fn glob_ish(pattern: &str, path: &str) -> bool {
    fn m(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => m(&p[1..], s) || (!s.is_empty() && m(p, &s[1..])),
            (Some(a), Some(b)) if a == b => m(&p[1..], &s[1..]),
            _ => false,
        }
    }
    m(pattern.as_bytes(), path.as_bytes())
}

/// Is `rel` (wp-content-relative) one a sync leaves alone — rexenv's excludes, the
/// mail mu-plugin, the companion plugin's own folder?
pub fn skipped_locally(rel: &str) -> bool {
    DEFAULT_EXCLUDES.iter().any(|g| glob_ish(g, rel)) || rel.starts_with("mu-plugins/rexenv-") || rel.starts_with("plugins/rexenv-sync")
}

/// Every file under `content` (wp-content) a sync considers: `(relative path,
/// size, mtime)`, sorted by path. The ONE walk the push, the status and the
/// pull's delete rule share.
pub fn walk_local(content: &Path) -> Result<Vec<(String, u64, i64)>> {
    let mut out = Vec::new();
    let mut stack = vec![content.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(content).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            if skipped_locally(&rel) || rel.starts_with(".rexsync-incoming-") {
                continue;
            }
            let ft = e.file_type()?;
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                let md = e.metadata()?;
                let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                out.push((rel, md.len(), mtime));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The delete rule, pure (ledger #861): of the files the base saw on live that live
/// no longer lists, which may go here — those that exist, are not excluded by a
/// current glob, and were NOT touched here since the base. The rest are named as
/// kept. `local` = `(path, mtime)` of the local files now.
pub fn gone_on_live<'a>(base: &'a SyncBase, live_now: &BTreeMap<&str, String>, local: &BTreeMap<&str, i64>, excludes: &[&str]) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut delete = Vec::new();
    let mut keep = Vec::new();
    for p in base.files.keys() {
        if live_now.contains_key(p.as_str()) || excludes.iter().any(|g| glob_ish(g, p)) {
            continue;
        }
        match local.get(p.as_str()) {
            None => {}
            Some(m) if base.file_changed_here(p, *m) => keep.push(p.as_str()),
            Some(_) => delete.push(p.as_str()),
        }
    }
    (delete, keep)
}

/// Pull the live site into `local`. `prior` = the base from the last sync (None, or
/// `options.full`: everything is replaced). `on_line` gets one line per step.
pub async fn pull_into(
    client: &Client,
    local: &LocalSite<'_>,
    prior: Option<&SyncBase>,
    options: &PullOptions,
    on_line: &mut (dyn FnMut(&str) + Send),
) -> Result<PullReport> {
    if !local.engine.is_mysql_family() {
        return Err(Error::Other("a live WordPress database can only be pulled into a MySQL or MariaDB site".into()));
    }
    let m = client.manifest().await?;
    if m.multisite && super::client::plugin_version(&m.plugin) < Some(super::client::MULTISITE_PLUGIN) {
        return Err(Error::Other(format!(
            "rexenv Sync: {} is a network, and its plugin ({}) is older than 0.1.5, the first that describes one — update it (Download plugin here, Upload Plugin there).",
            m.site_url, m.plugin
        )));
    }
    let prior = if options.full { None } else { prior };
    on_line(&format!("live: {} — WordPress {}, {} tables{}", m.site_url, m.wp, m.tables.len(), if prior.is_some() { " (only what changed since the last sync)" } else { "" }));
    let stage = format!("{}_pull", local.db_name);
    let backup = format!("{}_prepull", local.db_name);
    crate::core::database::validate_db_name(&stage)?;
    crate::core::database::validate_db_name(&backup)?;
    std::fs::create_dir_all(local.scratch)?;

    // ── 1. tables → staging: changed ones from live, the rest from here ────
    let local_now = local.engine.list_tables(local.client, local.port, local.db_name)?;
    // An interrupted pull's leftovers (`progress.rs`): reused unless this is a full pull.
    let progress_file = progress::pull_file(local.scratch, local.db_name);
    let mut progress: progress::PullProgress = if options.full { progress::PullProgress::default() } else { progress::read(&progress_file).unwrap_or_default() };
    let stage_exists = local.engine.database_exists(local.client, local.port, &stage)?;
    let in_stage: Vec<String> = if stage_exists && !options.full { local.engine.list_tables(local.client, local.port, &stage)? } else { Vec::new() };
    // A table an earlier, failed run already swapped in (its files phase died) IS live's
    // version: kept, like one the base says is unchanged.
    let swapped_already = |t: &super::client::RemoteTable| progress.swapped.get(&t.name) == Some(&t.checksum) && local_now.contains(&t.name);
    let (changed, kept): (Vec<&super::client::RemoteTable>, Vec<&super::client::RemoteTable>) = m.tables.iter().partition(|t| {
        !swapped_already(t)
            && match prior {
                Some(b) => b.tables.get(&t.name) != Some(&t.checksum) || !local_now.contains(&t.name),
                None => true,
            }
    });
    let (resumable, to_export): (Vec<&super::client::RemoteTable>, Vec<&super::client::RemoteTable>) =
        changed.iter().partition(|t| progress.staged.get(&t.name) == Some(&t.checksum) && in_stage.contains(&t.name));
    if resumable.is_empty() {
        local.engine.drop_database(local.client, local.port, &stage)?;
        local.engine.create_database(local.client, local.port, &stage)?;
        progress.staged.clear();
    } else {
        on_line(&format!("  {} already staged by the interrupted pull, reused", super::count(resumable.len(), "table", "tables")));
        // What the interrupted run staged but live has since moved is dropped before the
        // fresh export lands (the export's own DROP would do it; this keeps the record true).
        progress.staged.retain(|t, _| resumable.iter().any(|r| r.name == *t));
    }
    let swapped_kept = kept.iter().filter(|t| swapped_already(t)).count();
    let mut report = PullReport { backup_db: backup.clone(), tables_kept: kept.len() - swapped_kept, tables_resumed: resumable.len() + swapped_kept, ..Default::default() };
    report.base.tables = m.tables.iter().map(|t| (t.name.clone(), t.checksum.clone())).collect();
    let staged: Result<()> = async {
        for t in &to_export {
            let file = local.scratch.join(format!("pull-{}.sql", t.name));
            let rows = client.export_table(&t.name, &file).await?;
            // Data from a machine rexenv does not control: under a user that can
            // reach the staging database and nothing else (#837).
            let imported = local.engine.import_from_file_scoped(local.client, local.port, &stage, &file);
            let _ = std::fs::remove_file(&file);
            imported?;
            report.rows += rows;
            report.tables += 1;
            on_line(&format!("  {} — {}", t.name, super::count(rows as usize, "row", "rows")));
            // Recorded as it lands, so a pull that dies on the next table resumes from here.
            progress.staged.insert(t.name.clone(), t.checksum.clone());
            progress::write(&progress_file, &progress)?;
        }
        for t in &kept {
            // Unchanged on live since the base: the local copy IS that table — copied
            // server-side, nothing crosses the wire. The session's SQL mode first: a
            // strict MySQL 8 refuses WordPress's '0000-00-00 00:00:00' defaults even in
            // a `CREATE TABLE … LIKE` ("Invalid default value for 'comment_date'" — the
            // first incremental run, 14 Oct 2026), exactly as the export's header says.
            // A copy a resumed run already holds is replaced: the local table may have
            // moved since.
            exec(
                local,
                &format!("SET SESSION sql_mode='NO_AUTO_VALUE_ON_ZERO'; DROP TABLE IF EXISTS {s}.{t}; CREATE TABLE {s}.{t} LIKE {d}.{t}; INSERT INTO {s}.{t} SELECT * FROM {d}.{t}", s = q(&stage), d = q(local.db_name), t = q(&t.name)),
                &format!("keeping {}", t.name),
            )?;
        }
        if !kept.is_empty() {
            on_line(&format!("  {} unchanged on live, kept from here", super::count(kept.len(), "table", "tables")));
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
        // The staging database and the record stay: the next pull continues from them.
        let _ = progress::write(&progress_file, &progress);
        return Err(Error::Other(format!("{e} — the next pull continues from the tables already copied")));
    }
    // Personal data out of the COPY, before anything is swapped in (`anonymise.rs`).
    if options.anonymise {
        report.anonymised = anonymise::apply(local, &stage, &m.prefix, on_line)?;
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
    // WP-Cron OFF on every pulled copy (plan §2.8, owner 10 Oct 2026 — Q8b): a pulled
    // shop whose subscriptions renew on cron would charge real customers through the
    // real gateways from this machine. Set on EVERY pull, before the swap: a failure
    // here aborts with nothing replaced. Due events still run on demand
    // (`wp cron event run --due-now`, the Cron panel, the MCP `cron_run_due`) — the
    // constant only stops a page view from firing them.
    if let Err(e) = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "set", "DISABLE_WP_CRON", "true", "--raw", "--type=constant"]) {
        return Err(restore_prefix(&format!("turning WP-Cron off in wp-config.php failed — not pulling a live site whose cron could run here: {e}")));
    }
    on_line("wp-config: DISABLE_WP_CRON true (a pulled copy never fires the live site's scheduled jobs on its own)");
    let from = host_of(&m.site_url);
    // A network's constants, with the LIVE host first: the URL move lists the copy's blogs
    // through them (see the module doc). A single site takes them out of a copy that
    // was a network.
    let was_network = crate::core::sites::network_mode_on_disk(local.docroot).is_some();
    // The domain the STAGING copy's blogs table holds: live's when this pull exported it,
    // the local one when it was kept from here (an incremental pull with nothing moved on
    // live — the macOS VM's third network pull, 16 Oct 2026, failed with "Site
    // 'live-sync.rex.bd/' not found" because the constant named live while the kept
    // table said l14vm.rex). `wp site list` resolves the copy through this constant.
    let blogs_table = format!("{}blogs", m.prefix);
    let copy_domain: &str = if changed.iter().any(|t| t.name == blogs_table) { from.as_str() } else { local.domain };
    let prior_domain = if was_network { wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "get", "DOMAIN_CURRENT_SITE", "--type=constant"]).ok().map(|s| s.trim().to_string()) } else { None };
    // Whatever fails from here on, the local wp-config is left as it was: the constant
    // back to its previous value, or the six constants out of a copy that was single.
    let restore_network = |why: &str| -> Error {
        if m.multisite {
            match &prior_domain {
                Some(d) if !d.is_empty() => {
                    let _ = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "set", "DOMAIN_CURRENT_SITE", d, "--type=constant"]);
                }
                _ => {
                    for k in ["MULTISITE", "SUBDOMAIN_INSTALL", "DOMAIN_CURRENT_SITE", "PATH_CURRENT_SITE", "SITE_ID_CURRENT_SITE", "BLOG_ID_CURRENT_SITE"] {
                        let _ = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "delete", k, "--type=constant"]);
                    }
                }
            }
        }
        restore_prefix(why)
    };
    if m.multisite {
        let sets: [(&str, &str); 6] = [
            ("MULTISITE", "true"),
            ("SUBDOMAIN_INSTALL", if m.subdomain { "true" } else { "false" }),
            ("DOMAIN_CURRENT_SITE", copy_domain),
            ("PATH_CURRENT_SITE", "/"),
            ("SITE_ID_CURRENT_SITE", "1"),
            ("BLOG_ID_CURRENT_SITE", "1"),
        ];
        for (k, v) in sets {
            let raw = matches!(k, "MULTISITE" | "SUBDOMAIN_INSTALL" | "SITE_ID_CURRENT_SITE" | "BLOG_ID_CURRENT_SITE");
            let mut args = vec!["config", "set", k, v, "--type=constant"];
            if raw {
                args.push("--raw");
            }
            if let Err(e) = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &args) {
                return Err(restore_network(&format!("writing the network constant {k} failed: {e}")));
            }
        }
        on_line(&format!("wp-config: a {} network — the network constants are in", if m.subdomain { "subdomain" } else { "subdirectory" }));
    } else if was_network {
        for k in ["MULTISITE", "SUBDOMAIN_INSTALL", "DOMAIN_CURRENT_SITE", "PATH_CURRENT_SITE", "SITE_ID_CURRENT_SITE", "BLOG_ID_CURRENT_SITE", "WP_ALLOW_MULTISITE"] {
            let _ = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "delete", k, "--type=constant"]);
        }
        on_line("wp-config: the live site is a single site — the network constants are out");
    }
    match wordpress::rehome_urls_on_copy(local.platform, local.php, local.wp_phar, local.docroot, local.scratch, local.port, &stage, &from, local.domain, m.multisite, local.edge_https_port) {
        Ok(n) => {
            report.replacements = n;
            on_line(&format!("URLs: https://{from} → https://{} ({n} replacements)", local.domain));
        }
        Err(e) => return Err(restore_network(&format!("moving the URLs failed: {e}"))),
    }
    if m.multisite && copy_domain != local.domain {
        if let Err(e) = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["config", "set", "DOMAIN_CURRENT_SITE", local.domain, "--type=constant"]) {
            return Err(restore_network(&format!("moving DOMAIN_CURRENT_SITE to {} failed: {e}", local.domain)));
        }
    }
    report.network = m.multisite.then_some(if m.subdomain { "subdomain" } else { "subdirectory" });

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
    // The tables are live's now, whatever happens to the files below: a pull that dies
    // from here on resumes with them as kept.
    progress.staged.clear();
    progress.swapped = m.tables.iter().map(|t| (t.name.clone(), t.checksum.clone())).collect();
    let _ = progress::write(&progress_file, &progress);

    // ── 4. files ────────────────────────────────────────────────────────
    let mut excludes: Vec<&str> = DEFAULT_EXCLUDES.to_vec();
    excludes.extend(options.excludes.iter().map(String::as_str));
    let listed = client.list_files(&excludes, options.uploads_since).await?;
    report.base.files = listed.iter().map(|f| (f.path.clone(), format!("{}:{}", f.size, f.mtime))).collect();
    let live_now: BTreeMap<&str, String> = listed.iter().map(|f| (f.path.as_str(), format!("{}:{}", f.size, f.mtime))).collect();
    let mut files: Vec<&super::client::RemoteFile> = match prior {
        Some(b) => listed.iter().filter(|f| b.files.get(&f.path) != live_now.get(f.path.as_str())).collect(),
        None => listed.iter().collect(),
    };
    report.files_kept = listed.len() - files.len();
    // Ceilings (#837): a file the frame parser would refuse anyway is left on
    // live and reported, not fetched; the whole list must fit the disk with
    // room to spare, measured BEFORE the first byte lands.
    let too_big: Vec<String> = files.iter().filter(|f| f.size > super::client::MAX_FILE_BYTES).map(|f| f.path.clone()).collect();
    if !too_big.is_empty() {
        on_line(&format!("{} file(s) over {} MB left on live: {}", too_big.len(), super::client::MAX_FILE_BYTES >> 20, too_big.join(", ")));
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
    if prior.is_some() {
        on_line(&format!("files: {} changed on live since the last sync, {} unchanged", files.len(), report.files_kept));
    }
    // One incoming folder per site, kept across an interrupted pull: a file already in it
    // with the listing's size and mtime was verified (sha256) by that run and is reused.
    let incoming_dir: PathBuf = progress::incoming_dir(&content, local.db_name);
    if options.full {
        let _ = std::fs::remove_dir_all(&incoming_dir);
    }
    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let mut sized: Vec<(String, u64)> = Vec::new();
    for f in &files {
        let have = join_incoming(&incoming_dir, &f.path).and_then(|p| std::fs::metadata(p).ok());
        let same = have.map(|md| md.len() == f.size && md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64) == Some(f.mtime)).unwrap_or(false);
        if same {
            report.files_resumed += 1;
        } else {
            sized.push((f.path.clone(), f.size));
        }
    }
    if report.files_resumed > 0 {
        on_line(&format!("  {} already fetched by the interrupted pull, reused", super::count(report.files_resumed, "file", "files")));
    }
    let refused = if sized.is_empty() {
        Vec::new()
    } else {
        match client.read_files(&sized, &incoming_dir, &excludes).await {
            Ok(r) => r,
            // The folder stays (ours, named for this site): the next pull reuses what landed.
            Err(e) => return Err(Error::Other(format!("{e} — the next pull continues from the files already fetched"))),
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
    progress::forget(&progress_file);
    // Gone from live since the base: removed here only when untouched here since
    // (ledger #861). Never with `uploads_since`, which leaves old uploads unlisted —
    // they are not gone, just not asked for.
    if let (Some(b), None) = (prior, options.uploads_since) {
        let local_files = walk_local(&content)?;
        let local_mtimes: BTreeMap<&str, i64> = local_files.iter().map(|(p, _, m)| (p.as_str(), *m)).collect();
        let (delete, keep) = gone_on_live(b, &live_now, &local_mtimes, &excludes);
        for rel in delete {
            let mut p = content.clone();
            for seg in rel.split('/') {
                p.push(seg);
            }
            if std::fs::remove_file(&p).is_ok() {
                report.files_deleted.push(rel.to_string());
            }
        }
        report.files_kept_edited = keep.iter().map(|s| s.to_string()).collect();
        if !report.files_deleted.is_empty() {
            on_line(&format!("  {} gone from live, removed here", super::count(report.files_deleted.len(), "file", "files")));
        }
        if !report.files_kept_edited.is_empty() {
            on_line(&format!("  {} gone from live but edited here since the last sync — kept: {}", super::count(report.files_kept_edited.len(), "file", "files"), report.files_kept_edited.join(", ")));
        }
    }
    on_line(&format!("files: {} in place, {} refused by the site", report.files, report.refused_files.len()));

    // The plugin came along with the files; a local copy answering as a sync
    // endpoint means nothing (plan §2.8). BEFORE the local stamps: deactivating
    // writes `active_plugins`, and the stamp must be of the copy as it is left.
    let mut deactivate = vec!["plugin", "deactivate", "rexenv-sync", "--skip-plugins", "--skip-themes"];
    if m.multisite {
        deactivate.push("--network"); // network-activated on live (the bootstrap, or Network Activate)
    }
    let _ = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &deactivate);
    // On a network the plugin may ALSO be active per site — Activate on the main site's
    // Plugins screen writes that blog's `active_plugins`, which `--network` never touches
    // (ledger #896: sd.rex.bd, 16 Oct 2026, a copy whose main site still ran the plugin).
    // Every sub-site, by its local URL.
    if m.multisite {
        for site in wordpress::network_site_list(local.php, local.wp_phar, local.docroot).unwrap_or_default() {
            let url = format!("--url={}", site.url);
            let _ = wordpress::wp_run(local.php, local.wp_phar, local.docroot, &["plugin", "deactivate", "rexenv-sync", "--skip-plugins", "--skip-themes", &url]);
        }
    }
    // The LOCAL half (ledger #863): what the pull REPLACED is stamped fresh (live's
    // version is what is here now); what it KEPT keeps the prior base's stamp, so a
    // local edit made before the pull is still "ahead" after it. The Dell run
    // (14 Oct 2026) read "up to date" right after an incremental pull with a title
    // edited here that live never had: a fresh snapshot of everything had made the
    // edit the base. No prior / a full pull: everything fresh.
    let fresh_tables = super::stamps::local_stamps(local, &m.prefix)?;
    let exported: std::collections::BTreeSet<&str> = changed.iter().map(|t| t.name.as_str()).collect();
    report.base.local_tables = fresh_tables
        .into_iter()
        .map(|(t, s)| match prior.and_then(|b| b.local_tables.get(&t)) {
            Some(was) if !exported.contains(t.as_str()) => (t, was.clone()),
            _ => (t, s),
        })
        .collect();
    let fetched: std::collections::BTreeSet<&str> = paths.iter().map(String::as_str).collect();
    report.base.local_files = walk_local(&content)?
        .into_iter()
        .map(|(p, _, mtime)| match prior.and_then(|b| b.local_files.get(&p)) {
            Some(was) if !fetched.contains(p.as_str()) && !report.files_deleted.contains(&p) => (p, *was),
            _ => (p, mtime),
        })
        .collect();
    report.base.kind = "pull".into();
    // The sync moment is the END of the pull (#841): every file it wrote is now no
    // newer than this, so the next push sends only what is edited from here on.
    report.base.at = super::base::now();
    // Anonymised here: what this pull scrubbed, plus what an earlier anonymised pull
    // scrubbed and this one KEPT from the local copy (a table live replaced is live's
    // again). A push refuses every table on the list (#879).
    let kept_names: std::collections::BTreeSet<&str> = kept.iter().map(|t| t.name.as_str()).collect();
    report.base.anonymised = report.anonymised.clone();
    for t in prior.map(|b| b.anonymised.as_slice()).unwrap_or(&[]) {
        if kept_names.contains(t.as_str()) && !report.base.anonymised.contains(t) {
            report.base.anonymised.push(t.clone());
        }
    }
    report.base.anonymised.sort();
    // Uploads on demand, as the site's switch says (`uploads_on_demand.rs`).
    match options.uploads_on_demand {
        Some(true) => {
            uploads_on_demand::ensure(local.docroot, "wp-content", &m.site_url)?;
            on_line("uploads on demand: a media file missing here is fetched from live when a page asks for it");
        }
        Some(false) => uploads_on_demand::remove(local.docroot)?,
        None => {}
    }
    Ok(report)
}

/// `incoming/<rel>` for a [`super::client::safe_rel`] path, or `None` for anything else.
fn join_incoming(incoming: &Path, rel: &str) -> Option<PathBuf> {
    if !super::client::safe_rel(rel) {
        return None;
    }
    let mut p = incoming.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
    }
    p.starts_with(incoming).then_some(p)
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

    #[test]
    fn the_glob_and_the_local_skip_list() {
        assert!(glob_ish("uploads/rexsync-*", "uploads/rexsync-abc/x") && !glob_ish("*.log", "a.php") && glob_ish("*/cache/*", "plugins/x/cache/y"));
        assert!(skipped_locally("plugins/rexenv-sync/rexenv-sync.php") && skipped_locally("mu-plugins/rexenv-mail.php") && skipped_locally("cache/x"));
        assert!(!skipped_locally("plugins/akismet/akismet.php"));
    }

    /// **A pull deletes here only a file live dropped AND nobody touched here since
    /// the base** (ledger #861): still on live → stays; edited here after the base →
    /// kept and named; excluded by a current glob → never considered; absent here →
    /// nothing to do. Plant: drop the mtime test → the edited file is deleted.
    #[test]
    fn gone_on_live_spares_local_edits() {
        let base = SyncBase {
            at: 1000,
            files: ["a.txt", "b.txt", "c.txt", "d.txt", "cache/e.txt"].into_iter().map(|p| (p.to_string(), "1:1".to_string())).collect(),
            ..Default::default()
        };
        let live_now: BTreeMap<&str, String> = [("a.txt", "1:1".to_string())].into_iter().collect();
        let local: BTreeMap<&str, i64> = [("a.txt", 900), ("b.txt", 900), ("c.txt", 1500), ("cache/e.txt", 900)].into_iter().collect();
        let (delete, keep) = gone_on_live(&base, &live_now, &local, &DEFAULT_EXCLUDES);
        assert_eq!(delete, vec!["b.txt"]);
        assert_eq!(keep, vec!["c.txt"]);
        // With per-file local mtimes (a base since 14 Oct 2026) the recorded mtime decides,
        // not `at`: b.txt recorded at 900 and still 900 → delete; c.txt recorded 900, now 1500 → keep;
        // d.txt recorded 2000 (after `at`) and still 2000 → untouched → delete.
        let mut base2 = base.clone();
        base2.local_files = [("b.txt", 900), ("c.txt", 900), ("d.txt", 2000)].into_iter().map(|(p, m)| (p.to_string(), m)).collect();
        let local2: BTreeMap<&str, i64> = [("b.txt", 900), ("c.txt", 1500), ("d.txt", 2000)].into_iter().collect();
        let (delete, keep) = gone_on_live(&base2, &live_now, &local2, &DEFAULT_EXCLUDES);
        assert_eq!(delete, vec!["b.txt", "d.txt"]);
        assert_eq!(keep, vec!["c.txt"]);
    }

    /// **An incremental pull keeps the prior local stamp of what it did not replace**
    /// (ledger #863) — the pure half: the keep rule over tables and files. Plant: stamp
    /// everything fresh → the kept table's stamp is the fresh one, red.
    #[test]
    fn the_kept_half_keeps_its_prior_stamp() {
        let src = include_str!("pull.rs");
        let body = &src[src.find("let fresh_tables =").expect("the local half")..];
        assert!(body.contains("Some(was) if !exported.contains(t.as_str()) => (t, was.clone())"), "kept tables no longer keep their prior stamp");
        assert!(body.contains("Some(was) if !fetched.contains(p.as_str()) && !report.files_deleted.contains(&p) => (p, *was)"), "kept files no longer keep their prior mtime");
    }
}
