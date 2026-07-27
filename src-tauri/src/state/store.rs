//! Repository layer over SQLite — the only place that knows the `sites` table
//! shape. `core/` calls these functions; it never writes SQL itself.

use crate::error::{Error, Result};
use crate::state::models::{
    Blueprint, BlueprintSpec, GitAsset, MultisiteMode, PhpVersion, ServiceStatus, Site,
    SiteDbEngine, SiteType, WebServer,
};
use rusqlite::{params, Connection, Row};

/// Columns selected for a full `Site`, in struct order. Shared so every query
/// reads the same shape.
const SITE_COLUMNS: &str = "id, name, domain, type, status, php_version, web_server, ssl, path, \
     created_at, multisite, db_name, db_engine, xdebug, override_port, provisioned, \
     docroot_managed, db_created";

/// Map a row (selecting `SITE_COLUMNS`) into a `Site`.
fn row_to_site(row: &Row) -> rusqlite::Result<Site> {
    let site_type: String = row.get(3)?;
    let status: String = row.get(4)?;
    let web_server: String = row.get(6)?;
    let multisite: String = row.get(10)?;
    let db_engine: String = row.get(12)?;
    Ok(Site {
        id: row.get(0)?,
        name: row.get(1)?,
        domain: row.get(2)?,
        site_type: SiteType::parse_db(&site_type).map_err(to_sqlite_err)?,
        status: ServiceStatus::parse_db(&status).map_err(to_sqlite_err)?,
        php_version: row.get(5)?,
        web_server: WebServer::parse_db(&web_server).map_err(to_sqlite_err)?,
        ssl: row.get::<_, i64>(7)? != 0,
        path: row.get(8)?,
        created_at: row.get(9)?,
        multisite: MultisiteMode::parse_db(&multisite).map_err(to_sqlite_err)?,
        db_name: row.get(11)?,
        db_engine: SiteDbEngine::parse_db(&db_engine).map_err(to_sqlite_err)?,
        xdebug: row.get::<_, i64>(13)? != 0,
        // Nullable: the recorded per-site override backend port (B20 §4), NULL
        // for nginx and for pre-backfill rows.
        override_port: row.get::<_, Option<i64>>(14)?.map(|p| p as u16),
        provisioned: row.get::<_, i64>(15)? != 0,
        // Nullable by design (v17): NULL = a pre-v17 row the startup backfill
        // hasn't recorded yet, NOT "unowned" — the legacy lexical test covers
        // that window so the answer can never silently flip to deletable.
        docroot_managed: row.get::<_, Option<i64>>(16)?.map(|v| v != 0),
        // Nullable by design (v19): NULL = created by our own provisioning
        // (legacy — droppable), Some(false) = the name pre-existed and is never
        // dropped. See `Site::db_created`.
        db_created: row.get::<_, Option<i64>>(17)?.map(|v| v != 0),
    })
}

/// Bridge our enum-parse error into rusqlite's error channel so it surfaces
/// from `query_row`/`query_map` cleanly.
fn to_sqlite_err(e: crate::error::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
}

/// Insert a fully-formed site row.
pub fn insert_site(conn: &Connection, site: &Site) -> Result<()> {
    conn.execute(
        "INSERT INTO sites
            (id, name, domain, type, status, php_version, web_server, ssl, path, created_at, multisite, db_name, db_engine, xdebug, override_port, provisioned, docroot_managed, db_created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            site.id,
            site.name,
            site.domain,
            site.site_type.as_db(),
            site.status.as_db(),
            site.php_version,
            site.web_server.as_db(),
            site.ssl as i64,
            site.path,
            site.created_at,
            site.multisite.as_db(),
            site.db_name,
            site.db_engine.as_db(),
            site.xdebug as i64,
            site.override_port.map(|p| p as i64),
            site.provisioned as i64,
            site.docroot_managed.map(|m| m as i64),
            site.db_created.map(|c| c as i64),
        ],
    )?;
    Ok(())
}

/// Record whether rexenv created this site's database (v19) — see
/// [`crate::state::models::Site::db_created`].
///
/// Called by the database-import job **before** `CREATE DATABASE`, with
/// `false` for a name that already existed on our engine. Once `false` it must
/// never be raised to `true`: a later import into the same pre-existing name is
/// still not ours to drop, so the flag is monotonic toward safety exactly like
/// `docroot_managed`, and this function enforces that rather than trusting
/// callers.
pub fn set_site_db_created(conn: &Connection, id: &str, created: bool) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET db_created = ?1 WHERE id = ?2 \
         AND (db_created IS NULL OR db_created = 1 OR ?1 = 0)",
        params![created as i64, id],
    )?;
    Ok(affected > 0)
}

/// Point a site row at the database it actually uses (v19). The import restores
/// into THEIR name where it's free, so the derived name assigned at creation is
/// no longer the truth — and every later consumer (teardown, DB size, Adminer)
/// reads this column.
pub fn set_site_db_name(conn: &Connection, id: &str, db_name: &str) -> Result<bool> {
    let affected =
        conn.execute("UPDATE sites SET db_name = ?1 WHERE id = ?2", params![db_name, id])?;
    Ok(affected > 0)
}

/// Record whether rexenv owns a site's docroot (v17) — see
/// [`crate::state::models::Site::docroot_managed`]. After creation this is only
/// ever called with `false` (a move out of the sites folder): the flag is
/// monotonic toward safety, never re-derived from the path.
pub fn set_site_docroot_managed(conn: &Connection, id: &str, managed: bool) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET docroot_managed = ?1 WHERE id = ?2",
        params![managed as i64, id],
    )?;
    Ok(affected > 0)
}

/// Flip a site's provisioning-completeness flag (v16, streamed create job:
/// 0 right after insert, 1 only when the job settles ok).
pub fn set_site_provisioned(conn: &Connection, id: &str, provisioned: bool) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET provisioned = ?1 WHERE id = ?2",
        params![provisioned as i64, id],
    )?;
    Ok(affected > 0)
}

/// Set (or clear, with `None`) a site's recorded override backend port (B20 §4).
pub fn set_site_override_port(conn: &Connection, id: &str, port: Option<u16>) -> Result<bool> {
    let n = conn.execute(
        "UPDATE sites SET override_port = ?1 WHERE id = ?2",
        params![port.map(|p| p as i64), id],
    )?;
    Ok(n > 0)
}

/// All sites, newest first.
pub fn list_sites(conn: &Connection) -> Result<Vec<Site>> {
    let sql = format!("SELECT {SITE_COLUMNS} FROM sites ORDER BY created_at DESC, id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_site)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Fetch one site by id, or `None` if it doesn't exist.
pub fn get_site(conn: &Connection, id: &str) -> Result<Option<Site>> {
    let sql = format!("SELECT {SITE_COLUMNS} FROM sites WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map([id], row_to_site)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Delete a site by id; returns whether a row was removed.
pub fn delete_site(conn: &Connection, id: &str) -> Result<bool> {
    let affected = conn.execute("DELETE FROM sites WHERE id = ?1", [id])?;
    Ok(affected > 0)
}

/// Update a site's status; returns whether a row was updated.
pub fn set_site_status(conn: &Connection, id: &str, status: ServiceStatus) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET status = ?1 WHERE id = ?2",
        params![status.as_db(), id],
    )?;
    Ok(affected > 0)
}

/// Update a site's Xdebug toggle; returns whether a row was updated.
pub fn set_site_xdebug(conn: &Connection, id: &str, enabled: bool) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET xdebug = ?1 WHERE id = ?2",
        params![enabled as i64, id],
    )?;
    Ok(affected > 0)
}

/// Update only a site's display `name` column; returns whether a row was updated.
pub fn set_site_name(conn: &Connection, id: &str, name: &str) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    Ok(affected > 0)
}

/// Update only a site's `php_version` column; returns whether a row was updated.
pub fn set_site_php_version(conn: &Connection, id: &str, version: &str) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET php_version = ?1 WHERE id = ?2",
        params![version, id],
    )?;
    Ok(affected > 0)
}

/// Update only a site's `multisite` column; returns whether a row was updated.
/// Update ONLY the domain column. `path` and `db_name` are intentionally left
/// untouched — the docroot folder is never renamed and the database name is
/// stable for the site's lifetime (see `core::sites::set_domain`).
pub fn set_site_domain(conn: &Connection, id: &str, domain: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE sites SET domain = ?1 WHERE id = ?2",
        params![domain, id],
    )?;
    Ok(n > 0)
}

/// Update ONLY the docroot path column (after the files verifiably exist at the
/// new location — see `commands::sites::move_site_docroot`).
pub fn set_site_path(conn: &Connection, id: &str, path: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE sites SET path = ?1 WHERE id = ?2",
        params![path, id],
    )?;
    Ok(n > 0)
}

pub fn set_site_multisite(conn: &Connection, id: &str, mode: &str) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET multisite = ?1 WHERE id = ?2",
        params![mode, id],
    )?;
    Ok(affected > 0)
}

/// Update only a site's `web_server` column; returns whether a row was updated.
pub fn set_site_web_server(conn: &Connection, id: &str, server: &str) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET web_server = ?1 WHERE id = ?2",
        params![server, id],
    )?;
    Ok(affected > 0)
}

/// A resolver file we took over from another tool (v18).
#[derive(Debug, Clone)]
pub struct ResolverTakeover {
    pub tld: String,
    /// Their exact file content, as read at takeover time.
    pub original: String,
    /// Our 0600 copy of it under app-data.
    pub backup_path: String,
}

fn row_to_takeover(row: &Row) -> rusqlite::Result<ResolverTakeover> {
    Ok(ResolverTakeover {
        tld: row.get(0)?,
        original: row.get(1)?,
        backup_path: row.get(2)?,
    })
}

/// Record that we borrowed `tld`'s resolver file. Replaces any previous record
/// for that TLD (a re-takeover after they reclaimed it): the newest backup is
/// the right one to restore, since it is what we actually replaced.
pub fn insert_resolver_takeover(
    conn: &Connection,
    tld: &str,
    original: &str,
    backup_path: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO resolver_takeovers (tld, original, backup_path)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(tld) DO UPDATE SET
             original = excluded.original,
             backup_path = excluded.backup_path,
             taken_at = datetime('now')",
        params![tld, original, backup_path],
    )?;
    Ok(())
}

/// The takeover record for `tld`, if we hold one.
pub fn get_resolver_takeover(conn: &Connection, tld: &str) -> Result<Option<ResolverTakeover>> {
    let mut stmt = conn
        .prepare("SELECT tld, original, backup_path FROM resolver_takeovers WHERE tld = ?1")?;
    let mut rows = stmt.query_map([tld], row_to_takeover)?;
    match rows.next() {
        Some(v) => Ok(Some(v?)),
        None => Ok(None),
    }
}

/// Every resolver file we currently hold, TLD-sorted.
pub fn list_resolver_takeovers(conn: &Connection) -> Result<Vec<ResolverTakeover>> {
    let mut stmt = conn.prepare(
        "SELECT tld, original, backup_path FROM resolver_takeovers ORDER BY tld",
    )?;
    let rows = stmt.query_map([], row_to_takeover)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Forget a takeover — after restoring their file, or when they reclaimed it
/// themselves. The caller deletes the backup file in the same operation.
pub fn delete_resolver_takeover(conn: &Connection, tld: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM resolver_takeovers WHERE tld = ?1", params![tld])? > 0)
}

/// Read a setting value by key, or `None` if unset.
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query_map([key], |r| r.get::<_, String>(0))?;
    match rows.next() {
        Some(v) => Ok(Some(v?)),
        None => Ok(None),
    }
}

/// Insert or update a setting.
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// The database's current timestamp string, matching the `created_at` column
/// default (`datetime('now')`). Lets `core/` stamp rows without a time crate
/// and keeps the format identical to DB-generated values.
pub fn db_now(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("SELECT datetime('now')", [], |r| r.get(0))?)
}

/// True if a site already uses `domain` (domains are unique).
pub fn domain_exists(conn: &Connection, domain: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sites WHERE domain = ?1",
        [domain],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

/// True if a site already stores this database name. Backstop for the injective
/// name derivation at create (`core::sites::unique_db_name`) — `db_name` derives
/// from the domain but isn't a UNIQUE column, so create checks it explicitly.
pub fn db_name_exists(conn: &Connection, db_name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sites WHERE db_name = ?1",
        [db_name],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

/// The site that owns `db_name`, if any — the database import's "whose is
/// this?" question (Stage 2). Same authority as [`db_name_exists`], but the
/// import needs WHICH site, to name it in the shared-database refusal.
pub fn site_with_db_name(conn: &Connection, db_name: &str) -> Result<Option<Site>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SITE_COLUMNS} FROM sites WHERE db_name = ?1 LIMIT 1"
    ))?;
    let mut rows = stmt.query_map([db_name], row_to_site)?;
    Ok(rows.next().transpose()?)
}

// ── PHP version registry (Phase 2 §1.2) ───────────────────────────────────────

fn row_to_php_version(row: &Row) -> rusqlite::Result<PhpVersion> {
    Ok(PhpVersion {
        minor: row.get(0)?,
        patch: row.get(1)?,
        fpm_port: row.get::<_, i64>(2)? as u16,
        installed: row.get::<_, i64>(3)? != 0,
        is_default: row.get::<_, i64>(4)? != 0,
    })
}

/// Insert a PHP version, or update its `patch`/`fpm_port`/`is_default` if it
/// already exists. The `installed` flag is **preserved** on update so re-seeding
/// never clobbers a user's enable/disable choice.
pub fn upsert_php_version(conn: &Connection, v: &PhpVersion) -> Result<()> {
    conn.execute(
        "INSERT INTO php_versions (minor, patch, fpm_port, installed, is_default)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(minor) DO UPDATE SET
             patch = excluded.patch,
             fpm_port = excluded.fpm_port,
             is_default = excluded.is_default",
        params![
            v.minor,
            v.patch,
            v.fpm_port as i64,
            v.installed as i64,
            v.is_default as i64,
        ],
    )?;
    Ok(())
}

/// All registered PHP versions, ordered by minor series.
pub fn list_php_versions(conn: &Connection) -> Result<Vec<PhpVersion>> {
    let mut stmt = conn.prepare(
        "SELECT minor, patch, fpm_port, installed, is_default
         FROM php_versions ORDER BY minor",
    )?;
    let rows = stmt.query_map([], row_to_php_version)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Enable/disable a PHP version (whether a pool is started for it). Returns
/// whether a row was updated.
pub fn set_php_installed(conn: &Connection, minor: &str, installed: bool) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE php_versions SET installed = ?1 WHERE minor = ?2",
        params![installed as i64, minor],
    )?;
    Ok(affected > 0)
}

/// Make `minor` the default for new sites: in ONE atomic statement set
/// `is_default` to 1 for `minor` and 0 for every other row (so exactly one default
/// always remains). Returns whether `minor` exists.
pub fn set_default_php_version(conn: &Connection, minor: &str) -> Result<bool> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM php_versions WHERE minor = ?1",
        [minor],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(false);
    }
    // `(minor = ?1)` is 1 for the match, 0 otherwise — flips the default atomically.
    conn.execute("UPDATE php_versions SET is_default = (minor = ?1)", [minor])?;
    Ok(true)
}

// ── Per-version PHP ini settings ────────────────────────────────────────────────

/// The stored ini settings for one PHP minor, ordered by key. Absent keys mean
/// PHP's compiled default (our static builds load no php.ini).
pub fn get_php_settings(conn: &Connection, minor: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT key, value FROM php_settings WHERE minor = ?1 ORDER BY key",
    )?;
    let rows = stmt.query_map([minor], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// All stored ini settings, grouped by minor — loaded once at start so the
/// service manager can write pool configs + nginx body limits without the DB.
pub fn all_php_settings(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, Vec<(String, String)>>> {
    let mut stmt =
        conn.prepare("SELECT minor, key, value FROM php_settings ORDER BY minor, key")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    let mut out: std::collections::HashMap<String, Vec<(String, String)>> = Default::default();
    for row in rows {
        let (minor, key, value) = row?;
        out.entry(minor).or_default().push((key, value));
    }
    Ok(out)
}

/// Replace ALL stored ini settings for `minor` with `pairs` (a key absent from
/// `pairs` reverts to PHP's default). Atomic: delete + insert in one transaction.
pub fn replace_php_settings(
    conn: &Connection,
    minor: &str,
    pairs: &[(String, String)],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM php_settings WHERE minor = ?1", [minor])?;
    for (key, value) in pairs {
        tx.execute(
            "INSERT INTO php_settings (minor, key, value) VALUES (?1, ?2, ?3)",
            params![minor, key, value],
        )?;
    }
    tx.commit()?;
    Ok(())
}

// ── Per-site environment variables (Phase 3 §1.6) ──────────────────────────────

/// One site's env vars, name-sorted (for deterministic config emission).
pub fn get_site_env(conn: &Connection, site_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt =
        conn.prepare("SELECT name, value FROM site_env WHERE site_id = ?1 ORDER BY name")?;
    let rows = stmt.query_map([site_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// All env vars grouped by site id — loaded once at start so the service
/// manager can regenerate configs without the DB (mirrors `all_php_settings`).
pub fn all_site_env(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, Vec<(String, String)>>> {
    let mut stmt =
        conn.prepare("SELECT site_id, name, value FROM site_env ORDER BY site_id, name")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    let mut out: std::collections::HashMap<String, Vec<(String, String)>> = Default::default();
    for row in rows {
        let (site_id, name, value) = row?;
        out.entry(site_id).or_default().push((name, value));
    }
    Ok(out)
}

/// Replace ALL env vars for a site with `pairs` (a name absent from `pairs` is
/// removed). Atomic: delete + insert in one transaction. The caller has already
/// validated every pair (`core::site_env::validate`).
pub fn replace_site_env(
    conn: &Connection,
    site_id: &str,
    pairs: &[(String, String)],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM site_env WHERE site_id = ?1", [site_id])?;
    for (name, value) in pairs {
        tx.execute(
            "INSERT INTO site_env (site_id, name, value) VALUES (?1, ?2, ?3)",
            params![site_id, name, value],
        )?;
    }
    tx.commit()?;
    Ok(())
}

// ── Git-sourced plugin/theme provenance (add-from-Git) ─────────────────────────

/// One site's git-sourced wp-content dirs (list badges; future update/watch).
pub fn get_git_assets(conn: &Connection, site_id: &str) -> Result<Vec<GitAsset>> {
    let mut stmt = conn.prepare(
        "SELECT kind, dir_name, url, git_ref, source FROM site_git_assets \
         WHERE site_id = ?1 ORDER BY kind, dir_name",
    )?;
    let rows = stmt.query_map([site_id], |r| {
        Ok(GitAsset {
            kind: r.get(0)?,
            dir_name: r.get(1)?,
            url: r.get(2)?,
            git_ref: r.get(3)?,
            source: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Record (or replace) where a cloned dir came from — written on clone
/// success. Re-adding the same (site, kind, dir) replaces the row.
#[allow(clippy::too_many_arguments)] // flat mirror of the row
pub fn upsert_git_asset(
    conn: &Connection,
    site_id: &str,
    kind: &str,
    dir_name: &str,
    url: &str,
    git_ref: Option<&str>,
    source: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO site_git_assets (site_id, kind, dir_name, url, git_ref, source) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![site_id, kind, dir_name, url, git_ref, source],
    )?;
    Ok(())
}

/// Drop a git asset's provenance row (after its dir is deleted/unlinked).
pub fn delete_git_asset(
    conn: &Connection,
    site_id: &str,
    kind: &str,
    dir_name: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM site_git_assets WHERE site_id = ?1 AND kind = ?2 AND dir_name = ?3",
        params![site_id, kind, dir_name],
    )?;
    Ok(())
}

/// Update a git asset's recorded ref after a checkout (the row stays truthful).
pub fn set_git_asset_ref(
    conn: &Connection,
    site_id: &str,
    kind: &str,
    dir_name: &str,
    git_ref: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE site_git_assets SET git_ref = ?4 \
         WHERE site_id = ?1 AND kind = ?2 AND dir_name = ?3",
        params![site_id, kind, dir_name, git_ref],
    )?;
    Ok(())
}

/// Stored install fingerprints for one asset — NULL until the matching
/// install step succeeds through rexenv. Reset to NULL by re-add/adopt
/// (upsert_git_asset's INSERT OR REPLACE) — correct: a fresh checkout is
/// unverified. Missing row → (None, None).
pub fn get_git_asset_fps(
    conn: &Connection,
    site_id: &str,
    kind: &str,
    dir_name: &str,
) -> Result<(Option<String>, Option<String>)> {
    let mut stmt = conn.prepare(
        "SELECT composer_installed_fp, node_installed_fp FROM site_git_assets \
         WHERE site_id = ?1 AND kind = ?2 AND dir_name = ?3",
    )?;
    let mut rows = stmt.query_map(params![site_id, kind, dir_name], |r| {
        Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    Ok(rows.next().transpose()?.unwrap_or((None, None)))
}

/// Record a family's install fingerprint after a successful install step.
/// `family` is "composer" or "node" (column chosen here — never interpolated
/// from caller input). 0 rows updated (no provenance row) is fine.
pub fn set_git_asset_fp(
    conn: &Connection,
    site_id: &str,
    kind: &str,
    dir_name: &str,
    family: &str,
    fp: &str,
) -> Result<()> {
    let sql = match family {
        "composer" => {
            "UPDATE site_git_assets SET composer_installed_fp = ?4 \
             WHERE site_id = ?1 AND kind = ?2 AND dir_name = ?3"
        }
        _ => {
            "UPDATE site_git_assets SET node_installed_fp = ?4 \
             WHERE site_id = ?1 AND kind = ?2 AND dir_name = ?3"
        }
    };
    conn.execute(sql, params![site_id, kind, dir_name, fp])?;
    Ok(())
}

// ── Site blueprints (Phase 3 §11.3) ────────────────────────────────────────────

fn row_to_blueprint(row: &Row) -> Result<Blueprint> {
    let id: String = row.get(0)?;
    let name: String = row.get(1)?;
    let spec_json: String = row.get(2)?;
    let spec: BlueprintSpec = serde_json::from_str(&spec_json)
        .map_err(|e| Error::Other(format!("blueprint {id}: bad spec JSON: {e}")))?;
    Ok(Blueprint { id, name, spec })
}

/// All blueprints, newest first.
pub fn list_blueprints(conn: &Connection) -> Result<Vec<Blueprint>> {
    let mut stmt =
        conn.prepare("SELECT id, name, spec FROM blueprints ORDER BY created_at DESC, name")?;
    // query_map's closure must return rusqlite::Result, so it defers JSON parsing
    // (row_to_blueprint_sql wraps our Result); flatten both layers here.
    let mut out = Vec::new();
    for r in stmt.query_map([], row_to_blueprint_sql)? {
        out.push(r??);
    }
    Ok(out)
}

// Adapter so `query_map` (which must return rusqlite::Result) can defer JSON parsing.
fn row_to_blueprint_sql(row: &Row) -> rusqlite::Result<Result<Blueprint>> {
    Ok(row_to_blueprint(row))
}

/// One blueprint by id, or `None`.
pub fn get_blueprint(conn: &Connection, id: &str) -> Result<Option<Blueprint>> {
    let mut stmt = conn.prepare("SELECT id, name, spec FROM blueprints WHERE id = ?1")?;
    let mut rows = stmt.query_map([id], row_to_blueprint_sql)?;
    match rows.next() {
        Some(r) => Ok(Some(r??)),
        None => Ok(None),
    }
}

/// Insert or update a blueprint (upsert by id).
pub fn upsert_blueprint(conn: &Connection, bp: &Blueprint) -> Result<()> {
    let spec_json = serde_json::to_string(&bp.spec)
        .map_err(|e| Error::Other(format!("serialize blueprint spec: {e}")))?;
    conn.execute(
        "INSERT INTO blueprints (id, name, spec) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, spec = excluded.spec",
        params![bp.id, bp.name, spec_json],
    )?;
    Ok(())
}

/// Delete a blueprint by id; returns whether it existed.
pub fn delete_blueprint(conn: &Connection, id: &str) -> Result<bool> {
    let affected = conn.execute("DELETE FROM blueprints WHERE id = ?1", [id])?;
    Ok(affected > 0)
}
