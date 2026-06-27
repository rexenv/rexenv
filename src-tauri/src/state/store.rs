//! Repository layer over SQLite — the only place that knows the `sites` table
//! shape. `core/` calls these functions; it never writes SQL itself.

use crate::error::Result;
use crate::state::models::{ServiceStatus, Site, SiteType, WebServer};
use rusqlite::{params, Connection, Row};

/// Columns selected for a full `Site`, in struct order. Shared so every query
/// reads the same shape.
const SITE_COLUMNS: &str =
    "id, name, domain, type, status, php_version, web_server, ssl, path, created_at";

/// Map a row (selecting `SITE_COLUMNS`) into a `Site`.
fn row_to_site(row: &Row) -> rusqlite::Result<Site> {
    let site_type: String = row.get(3)?;
    let status: String = row.get(4)?;
    let web_server: String = row.get(6)?;
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
            (id, name, domain, type, status, php_version, web_server, ssl, path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
