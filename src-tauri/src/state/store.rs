//! Repository layer over SQLite — the only place that knows the `sites` table
//! shape. `core/` calls these functions; it never writes SQL itself.

use crate::error::{Error, Result};
use crate::state::models::{
    Blueprint, BlueprintSpec, MultisiteMode, PhpVersion, ServiceStatus, Site, SiteType, WebServer,
};
use rusqlite::{params, Connection, Row};

/// Columns selected for a full `Site`, in struct order. Shared so every query
/// reads the same shape.
const SITE_COLUMNS: &str =
    "id, name, domain, type, status, php_version, web_server, ssl, path, created_at, multisite, db_name";

/// Map a row (selecting `SITE_COLUMNS`) into a `Site`.
fn row_to_site(row: &Row) -> rusqlite::Result<Site> {
    let site_type: String = row.get(3)?;
    let status: String = row.get(4)?;
    let web_server: String = row.get(6)?;
    let multisite: String = row.get(10)?;
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
            (id, name, domain, type, status, php_version, web_server, ssl, path, created_at, multisite, db_name)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
        ],
    )?;
    Ok(())
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
