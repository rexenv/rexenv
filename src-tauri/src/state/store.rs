//! Repository layer over SQLite — the only place that knows the app database's
//! `sites` table shape. `core/` reaches the app SQLite database only through
//! these functions; it never hand-writes SQL against the app schema.
//!
//! Withdrawn 29 Jul (was "core never writes SQL itself"): core DOES execute SQL
//! — it reads `information_schema` and issues `CREATE DATABASE`/`GRANT` against
//! the developer's MySQL/Postgres in `core::dbmirror`. The honest, narrow
//! guarantee is only that the app's OWN SQLite schema is state/'s alone.
//!
//! Narrowed AGAIN 15 Aug 2026, by the guard that was supposed to prove it
//! (ledger #167): `mcp_server/feed.rs` has hand-written SQL against
//! `agent_actions` since M2a — "state/'s alone" was already false when the
//! guard arrived, the fourth claim this year whose surface had quietly grown.
//! The honest shape is PER-TABLE ownership: every app table's SQL lives in the
//! ONE module that owns it — state/ for everything except `agent_actions`,
//! which is feed.rs's whole subject. The guard asserts exactly that list, and
//! that feed.rs touches no table but its own.

use crate::error::{Error, Result};
use crate::state::models::{
    Blueprint, BlueprintSpec, GitAsset, MultisiteMode, PhpVersion, ServiceStatus, Site,
    ScratchPackage, SiteDbEngine, SiteOrigin, SiteType, WebServer,
};
use rusqlite::{params, Connection, Row};

/// Columns selected for a full `Site`, in struct order. Shared so every query
/// reads the same shape.
const SITE_COLUMNS: &str = "id, name, domain, type, status, php_version, web_server, ssl, path, \
     created_at, multisite, db_name, db_engine, xdebug, override_port, provisioned, \
     docroot_managed, db_created, content_dir, mu_dir_created, origin, agent_client, expires_at, \
     docroot_subdir, git_url, git_ref, git_migrate, git_build_assets, starter_db";

/// Bound on the AGENT-controlled `agent_client` (v27). It arrives from MCP
/// `initialize`'s `clientInfo.name`, bounded only by the session's 4 MB line
/// cap, so it is capped where it is WRITTEN — the same discipline (and the same
/// bound) as the activity feed's agent-controlled fields.
const AGENT_CLIENT_MAX: usize = 200;

/// Truncate an agent-supplied client name to [`AGENT_CLIENT_MAX`], on a char
/// boundary (the value is arbitrary UTF-8 off the wire).
fn cap_agent_client(v: Option<&str>) -> Option<String> {
    v.map(|s| match s.char_indices().nth(AGENT_CLIENT_MAX) {
        Some((cut, _)) => s[..cut].to_string(),
        None => s.to_string(),
    })
}

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
        // Nullable by design (v24): NULL = pre-backfill; reads as the WP
        // default `wp-content` via `Site::content_dir_rel`.
        content_dir: row.get(18)?,
        // Nullable by design (v25): NULL = the mu-plugins dir is not ours.
        mu_dir_created: row.get::<_, Option<i64>>(19)?.map(|v| v != 0),
        // v27. NOT NULL in the schema, and read leniently anyway: anything but
        // "agent" is the user's site (`SiteOrigin::parse_db`) — one bad cell must
        // not fail the sites list, and it must never fail toward "reapable".
        origin: SiteOrigin::parse_db(&row.get::<_, String>(20)?),
        // v27: agent-asserted, display-only (capped at write).
        agent_client: row.get(21)?,
        // v27: NULL = never expires (a user site, or a Kept scratch site).
        expires_at: row.get(22)?,
        // v32: "" = serve `path` itself, which is every pre-v32 row and every
        // site whose entry point IS its root. Read via `Site::served_root()`.
        docroot_subdir: row.get(23)?,
        // v33: NULL = the code did not come from a repo. Exact for pre-v33
        // rows — cloning into a docroot did not exist before the column did.
        git_url: row.get(24)?,
        git_ref: row.get(25)?,
        // v34: NULL = ON — exact, since every Laravel site made before this
        // column migrated unconditionally. Read via `Site::runs_migrations`.
        git_migrate: row.get::<_, Option<i64>>(26)?.map(|v| v != 0),
        // v35: NULL = NO — exact, since nothing ran a package manager during
        // provisioning before this column. Read via `Site::builds_assets`.
        git_build_assets: row.get::<_, Option<i64>>(27)?.map(|v| v != 0),
        // v41: NULL = NO — exact, since a Blank-PHP site got a `phpinfo()` page
        // and no database before this column. Read via `Site::has_starter_db`.
        starter_db: row.get::<_, Option<i64>>(28)?.map(|v| v != 0),
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
            (id, name, domain, type, status, php_version, web_server, ssl, path, created_at, multisite, db_name, db_engine, xdebug, override_port, provisioned, docroot_managed, db_created, content_dir, mu_dir_created, origin, agent_client, expires_at, docroot_subdir, git_url, git_ref, git_migrate, git_build_assets, starter_db)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29)",
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
            site.content_dir,
            site.mu_dir_created.map(|c| c as i64),
            site.origin.as_db(),
            // Capped HERE, at the write — the one agent-controlled value on the
            // row never reaches disk unbounded, whatever built the `Site`.
            cap_agent_client(site.agent_client.as_deref()),
            site.expires_at,
            site.docroot_subdir,
            site.git_url,
            site.git_ref,
            site.git_migrate.map(|m| m as i64),
            site.git_build_assets.map(|b| b as i64),
            site.starter_db.map(|b| b as i64),
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

/// One recorded live tunnel (v23). Rows exist from spawn to clean stop/exit;
/// a row found at launch is a crash survivor for the sweep to settle.
#[derive(Debug, Clone)]
pub struct TunnelRecord {
    pub domain: String,
    pub pid: u32,
    /// The site's docroot AT SPAWN TIME — the sweep removes the tunnel
    /// mu-plugin here without a site lookup (the site may be gone by then).
    pub docroot: String,
}

fn row_to_tunnel(row: &Row) -> rusqlite::Result<TunnelRecord> {
    Ok(TunnelRecord { domain: row.get(0)?, pid: row.get(1)?, docroot: row.get(2)? })
}

/// Atomically CLAIM the tunnel slot for a domain (step 4 in-flight guard):
/// inserts only when no row exists and returns whether THIS caller won. The
/// row is the shared state between concurrent starts, stop, the exit hook,
/// and the launch sweep — a second start for the same domain loses here,
/// before it resolves a binary or spawns anything. `pid` is the caller's
/// sentinel until the child exists (`set_tunnel_pid`).
pub fn try_claim_tunnel(conn: &Connection, domain: &str, pid: u32, docroot: &str) -> Result<bool> {
    let inserted = conn.execute(
        "INSERT INTO tunnels (domain, pid, docroot) VALUES (?1, ?2, ?3)
         ON CONFLICT(domain) DO NOTHING",
        params![domain, pid, docroot],
    )?;
    Ok(inserted > 0)
}

/// Record the spawned child's real pid on an existing claim. `false` = the
/// claim is GONE — a stop or site delete revoked it while the start was in
/// flight, and the caller must treat the start as cancelled (kill its child),
/// never proceed untracked.
pub fn set_tunnel_pid(conn: &Connection, domain: &str, pid: u32) -> Result<bool> {
    let updated =
        conn.execute("UPDATE tunnels SET pid = ?2 WHERE domain = ?1", params![domain, pid])?;
    Ok(updated > 0)
}

/// Re-point a tunnel record's docroot after a committed docroot move (audit
/// A2): the row's recorded path is what settle/exit/sweep remove the
/// mu-plugin at — left stale, the file our writer put there is orphaned at
/// the NEW location on every unclean end.
pub fn set_tunnel_docroot(conn: &Connection, domain: &str, docroot: &str) -> Result<bool> {
    Ok(conn
        .execute("UPDATE tunnels SET docroot = ?2 WHERE domain = ?1", params![domain, docroot])?
        > 0)
}

/// The recorded tunnel for a domain, if any.
pub fn get_tunnel(conn: &Connection, domain: &str) -> Result<Option<TunnelRecord>> {
    let mut stmt =
        conn.prepare("SELECT domain, pid, docroot FROM tunnels WHERE domain = ?1")?;
    let mut rows = stmt.query_map([domain], row_to_tunnel)?;
    match rows.next() {
        Some(v) => Ok(Some(v?)),
        None => Ok(None),
    }
}

/// Every recorded tunnel, domain-sorted.
pub fn list_tunnels(conn: &Connection) -> Result<Vec<TunnelRecord>> {
    let mut stmt = conn.prepare("SELECT domain, pid, docroot FROM tunnels ORDER BY domain")?;
    let rows = stmt.query_map([], row_to_tunnel)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Forget a tunnel record — clean stop, failed start, or settled by the sweep.
pub fn delete_tunnel(conn: &Connection, domain: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM tunnels WHERE domain = ?1", params![domain])? > 0)
}

/// Forget every tunnel record (app-exit hook, after killing them all).
pub fn clear_tunnels(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM tunnels", [])?;
    Ok(())
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

/// Push a scratch site's expiry `ttl_hours` further out because an agent just
/// used it (v27) — "idle scratch dies; active scratch lives". Returns whether a
/// row was touched.
///
/// **It can only MOVE an expiry, never establish one**, and every clause of the
/// `WHERE` is there to keep that true:
///
/// - `origin = 'agent'` — a user's own site is never given lifecycle state by an
///   agent naming it. That includes a site the user hand-named
///   `*.scratch.rex`, and a scratch site the user has since KEPT (Keep flips
///   `origin` to `'user'`).
/// - `expires_at IS NOT NULL` — belt for the same fact from the other side. If a
///   row ever reached `origin='agent'` with no expiry, writing one here would
///   CREATE deletion state that did not exist, which is the one direction this
///   must never move in. A touch extends a deadline; it does not start a clock.
/// - `id = ?1` on an UPDATE — a deleted site matches nothing, so a call naming
///   one is a no-op and cannot resurrect a row (an UPDATE cannot insert).
///
/// An already-past expiry IS refreshed: the reaper hasn't collected it yet and
/// the agent is demonstrably still using it, which is exactly what the TTL is
/// asking about.
pub fn touch_site_expiry(conn: &Connection, id: &str, ttl_hours: i64) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET expires_at = datetime('now', ?2) \
         WHERE id = ?1 AND origin = 'agent' AND expires_at IS NOT NULL",
        params![id, format!("+{ttl_hours} hours")],
    )?;
    Ok(affected > 0)
}

/// Record that rexenv created the site's mu-plugins dir (v25). Set-once, only
/// ever to true — ownership is claimed at creation time, never revoked into a
/// guess.
pub fn set_site_mu_dir_created(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.execute("UPDATE sites SET mu_dir_created = 1 WHERE id = ?1", params![id])? > 0)
}

/// Record a site's content dir (v24 backfill; creation writes it inline).
pub fn set_site_content_dir(conn: &Connection, id: &str, rel: &str) -> Result<bool> {
    Ok(conn.execute("UPDATE sites SET content_dir = ?1 WHERE id = ?2", params![rel, id])? > 0)
}

/// Record the folder inside the site path the web server roots at (v32).
///
/// Creation writes this inline from the site TYPE, which is all it can know
/// before a docroot has any contents. A cloned site learns it later, from
/// [`crate::core::sites::detect_project`] reading the checkout that just
/// landed — so a repo whose entry point is not the type's usual subfolder is
/// served correctly instead of being assumed.
///
/// Writing `""` is meaningful (serve the path itself) and therefore allowed;
/// what is NOT allowed is an absolute path or one that climbs out of the
/// project, which would make `Site::served_root` point somewhere the site
/// does not own. Refused here — the one place the value can be changed after
/// creation — rather than at every read.
pub fn set_site_docroot_subdir(conn: &Connection, id: &str, rel: &str) -> Result<bool> {
    let bad = rel.starts_with('/')
        || rel.starts_with('\\')
        || rel.split(['/', '\\']).any(|seg| seg == "..");
    if bad {
        return Err(Error::Other(format!(
            "refusing to serve \"{rel}\" — a site's document root must stay inside its own folder"
        )));
    }
    Ok(conn.execute("UPDATE sites SET docroot_subdir = ?1 WHERE id = ?2", params![rel, id])? > 0)
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

/// Remove a setting, so the next read sees "never chosen" rather than a value
/// meaning it. Absent and empty-string are different states everywhere settings
/// are read, and writing `""` to mean "unset" is how they stop being.
pub fn delete_setting(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

/// The database's current timestamp string, matching the `created_at` column
/// default (`datetime('now')`). Lets `core/` stamp rows without a time crate
/// and keeps the format identical to DB-generated values.
pub fn db_now(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("SELECT datetime('now')", [], |r| r.get(0))?)
}

/// **Keep** a scratch site: it becomes the user's, permanently (v27).
///
/// Two columns, ONE write, on purpose. `origin` and `expires_at` express a
/// single decision — "this is not disposable any more" — and writing them in two
/// statements would be the two-facts-that-must-agree shape this codebase keeps
/// collapsing: a crash between them could leave a site marked the user's with a
/// live clock, or an agent's site with none. A single `UPDATE` is atomic in
/// SQLite, so that interleaving is not representable.
///
/// Even the degenerate failure is inert: `Site::reap_due` tests `origin` FIRST,
/// so an expiry left on a user's row can never collect it. The clear is for the
/// UI's sake (a kept site must not read "expires in 4h"), not for safety.
///
/// This is also the promotion path for any USER-initiated mutation of a scratch
/// site — rename, move, an env edit, sharing it publicly. Touching it makes it
/// yours, so the reaper never deletes a site the user just adopted. Agents
/// cannot call it: it is the counterpart to their tools, not one of them.
///
/// Returns whether a row changed. Idempotent: keeping a site twice is a no-op,
/// and there is no "un-keep" — see the confirm copy.
pub fn keep_site(conn: &Connection, id: &str) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE sites SET origin = 'user', expires_at = NULL WHERE id = ?1 AND origin = 'agent'",
        params![id],
    )?;
    Ok(affected > 0)
}

/// Record (or re-record) a cloned package. The source path is written ONCE at
/// add and never changed by a sync — a sync may only re-read where the clone
/// came from.
pub fn upsert_scratch_package(conn: &Connection, p: &ScratchPackage) -> Result<()> {
    conn.execute(
        "INSERT INTO scratch_packages (site_id, slug, kind, source_path, synced_at, fingerprint) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(site_id, slug) DO UPDATE SET synced_at = excluded.synced_at, \
         fingerprint = excluded.fingerprint",
        params![p.site_id, p.slug, p.kind, p.source_path, p.synced_at, p.fingerprint],
    )?;
    Ok(())
}

/// One recorded package by slug.
pub fn scratch_package(conn: &Connection, site_id: &str, slug: &str) -> Result<Option<ScratchPackage>> {
    Ok(scratch_packages(conn, site_id)?.into_iter().find(|p| p.slug == slug))
}

/// The plugins/themes an agent has cloned into a scratch site (v29), newest
/// sync first. An empty vec is a real answer — "nothing was added" — not a
/// missing one.
pub fn scratch_packages(conn: &Connection, site_id: &str) -> Result<Vec<ScratchPackage>> {
    let mut stmt = conn.prepare(
        "SELECT site_id, slug, kind, source_path, synced_at, fingerprint FROM scratch_packages \
         WHERE site_id = ?1 ORDER BY synced_at DESC",
    )?;
    let rows = stmt
        .query_map(params![site_id], |r| {
            Ok(ScratchPackage {
                site_id: r.get(0)?,
                slug: r.get(1)?,
                kind: r.get(2)?,
                source_path: r.get(3)?,
                synced_at: r.get(4)?,
                fingerprint: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Drop a site's recorded packages (v29).
///
/// v29 declares `site_id` as a plain column with no foreign key, so nothing
/// removes these rows on its own — before this they outlived their site as
/// orphans, exactly the gap `delete_db_import` was added to close for imports.
/// Called from `sites::teardown`, so the CLI and the UI inherit it rather than
/// one command remembering.
pub fn delete_scratch_packages(conn: &Connection, site_id: &str) -> Result<()> {
    conn.execute("DELETE FROM scratch_packages WHERE site_id = ?1", [site_id])?;
    Ok(())
}

/// Every recorded package whose SITE still exists, newest sync first — one read
/// for a whole page rather than one per row (the `db_import_records` shape).
///
/// The join is load-bearing, not tidiness: `scratch_packages` carries no foreign
/// key (v29 declares `site_id` as a plain column), so deleting a site leaves its
/// package rows behind. Reading through the join means a stale row can never
/// surface as a package of a site that is gone — the read is correct whatever
/// the table's leftovers are.
pub fn all_scratch_packages(conn: &Connection) -> Result<Vec<ScratchPackage>> {
    let mut stmt = conn.prepare(
        "SELECT p.site_id, p.slug, p.kind, p.source_path, p.synced_at, p.fingerprint \
         FROM scratch_packages p JOIN sites s ON s.id = p.site_id \
         ORDER BY p.synced_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ScratchPackage {
                site_id: r.get(0)?,
                slug: r.get(1)?,
                kind: r.get(2)?,
                source_path: r.get(3)?,
                synced_at: r.get(4)?,
                fingerprint: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The database's timestamp `hours` from now, in the same format as
/// [`db_now`] — so an expiry written here compares correctly against
/// `datetime('now')` at reap time. SQLite does the arithmetic, in UTC, with the
/// same clock the comparison will use; computing it in Rust would introduce a
/// second clock and a second format for one value.
pub fn db_time_from_now(conn: &Connection, hours: i64) -> Result<String> {
    Ok(conn.query_row("SELECT datetime('now', ?1)", params![format!("+{hours} hours")], |r| {
        r.get(0)
    })?)
}

/// The site whose PRIMARY domain is `domain`, if any. Separate from
/// [`domain_exists`] because the alias validator needs to NAME the site it
/// collides with — "already taken" sends the user looking, "already the domain
/// of Acme" ends the question.
pub fn site_by_domain(conn: &Connection, domain: &str) -> Result<Option<Site>> {
    let sql = format!("SELECT {SITE_COLUMNS} FROM sites WHERE domain = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map([domain], row_to_site)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
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

/// Witness that a connection verification actually RAN (Stage 3 plan §6).
///
/// The field is private and the only production constructor is
/// [`ConnectedVerified::from_verification`], which demands
/// `confverify::Verified` — itself mintable only by a sign-in that
/// succeeded against the rewritten file as re-read from disk. Nothing can
/// write `connected` from "the write succeeded".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedVerified {
    kind: VerifiedKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifiedKind {
    /// The rewritten settings sign in to the rexenv copy (`USE <db>`), the
    /// gate for `connected`.
    Signin,
    /// Sign-in plus the supplementary HTTP probe. The probe can only ever
    /// UPGRADE `signin` — never gate, never un-set (D4).
    SigninHttp,
}

impl ConnectedVerified {
    /// The ONE production mint. It demands `confverify::Verified` — a proof
    /// type whose only non-test constructor is `verify_signin`'s success
    /// path — so the chain is closed end to end: connected fact ⇐ witness ⇐
    /// proof ⇐ an actual sign-in with the rewritten file as re-read.
    pub fn from_verification(v: &crate::core::confverify::Verified) -> Self {
        Self {
            kind: if v.http_confirmed() {
                VerifiedKind::SigninHttp
            } else {
                VerifiedKind::Signin
            },
        }
    }

    /// The serialized form — also the `db_imports.verified` column value.
    pub fn as_str(self) -> &'static str {
        match self.kind {
            VerifiedKind::Signin => "signin",
            VerifiedKind::SigninHttp => "signin+http",
        }
    }

    #[cfg(test)]
    pub(crate) fn test_signin() -> Self {
        Self { kind: VerifiedKind::Signin }
    }

    #[cfg(test)]
    pub(crate) fn test_signin_http() -> Self {
        Self { kind: VerifiedKind::SigninHttp }
    }
}

impl serde::Serialize for ConnectedVerified {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// The `db_imports.state` closed set. `Connected` cannot be built without a
/// [`ConnectedVerified`] witness, so the type itself enforces "connected is
/// something we proved": serialized as `{"state":"imported"}` or
/// `{"state":"connected","verified":"signin"|"signin+http"}` (flattened into
/// [`DbImportRecord`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", content = "verified", rename_all = "lowercase")]
pub enum DbImportState {
    Imported,
    Connected(ConnectedVerified),
}

/// The settled outcome of a site's database import (v20/v21) — the ONE fact
/// the summary, badge and detail panel all render from. This is the READ
/// shape: writes go through [`upsert_db_import`] (which can only land
/// `imported`) or [`set_db_import_connected`] (the sole `connected` writer).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbImportRecord {
    pub site_id: String,
    #[serde(flatten)]
    pub state: DbImportState,
    pub db_name: String,
    pub table_count: u64,
    pub size_bytes: u64,
    /// e.g. "MySQL 8.0.27 at 127.0.0.1:3306".
    pub source_label: String,
    /// `None` = their config connects as a reserved account (root): the
    /// interim change is three keys, and the copy must say so.
    pub mirrored_user: Option<String>,
    /// Tables the SOURCE could not read, deliberately left out of the copy
    /// (v31). Empty on a complete copy. Present on the record — not only in the
    /// import log — because the log is pruned and the missing data is permanent.
    pub skipped_tables: Vec<String>,
    pub imported_at: String,
}

/// What the import job records when it settles ok — the WRITE shape of
/// [`DbImportRecord`]. It has no state field on purpose: an import's only
/// legitimate outcome is `imported`, so the upsert writes that (and clears
/// `verified`) unconditionally. A re-import of a `connected` site therefore
/// honestly resets to `imported` — the fresh copy has not been re-verified.
#[derive(Debug, Clone)]
pub struct NewDbImport {
    pub site_id: String,
    pub db_name: String,
    pub table_count: u64,
    pub size_bytes: u64,
    pub source_label: String,
    pub mirrored_user: Option<String>,
    pub skipped_tables: Vec<String>,
}

/// Upsert the settled import outcome for a site (a re-import replaces it)
/// and return the stored row — callers render what was written, not what
/// they meant to write.
pub fn upsert_db_import(conn: &Connection, r: &NewDbImport) -> Result<DbImportRecord> {
    conn.execute(
        "INSERT INTO db_imports (site_id, state, db_name, table_count, size_bytes, source_label, mirrored_user, skipped_tables, verified)
         VALUES (?1, 'imported', ?2, ?3, ?4, ?5, ?6, ?7, NULL)
         ON CONFLICT(site_id) DO UPDATE SET
           state = 'imported', db_name = ?2, table_count = ?3, size_bytes = ?4,
           source_label = ?5, mirrored_user = ?6, skipped_tables = ?7, verified = NULL,
           imported_at = datetime('now')",
        params![
            r.site_id,
            r.db_name,
            r.table_count as i64,
            r.size_bytes as i64,
            r.source_label,
            r.mirrored_user,
            encode_skipped(&r.skipped_tables),
        ],
    )?;
    get_db_import(conn, &r.site_id)?
        .ok_or_else(|| Error::Other(format!("db_imports row for {} vanished after upsert", r.site_id)))
}

/// The ONE writer of `state='connected'` (Stage 3 plan §6). Callable only
/// with a [`ConnectedVerified`] witness, and refuses when no import record
/// exists — "connected" without an import is not a state.
pub fn set_db_import_connected(
    conn: &Connection,
    site_id: &str,
    verified: ConnectedVerified,
) -> Result<()> {
    let n = conn.execute(
        "UPDATE db_imports SET state = 'connected', verified = ?2 WHERE site_id = ?1",
        params![site_id, verified.as_str()],
    )?;
    if n == 0 {
        return Err(Error::Other(format!("no database import is recorded for site {site_id}")));
    }
    Ok(())
}

/// Revert's half of the closed set: back to `imported`, verification cleared.
/// Writing the FLOOR state needs no witness — only claiming `connected` does.
pub fn clear_db_import_connected(conn: &Connection, site_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE db_imports SET state = 'imported', verified = NULL WHERE site_id = ?1",
        [site_id],
    )?;
    Ok(())
}

/// Record which user the rewrite job mirrors (the dedicated `rex_<slug>` for
/// root configs) — written BEFORE the account is created, the provenance-first
/// order (`db_created`'s shape): a crash between the record and the create
/// leaves a name `DROP USER IF EXISTS` shrugs at; the reverse order leaks an
/// account nothing will ever drop.
pub fn set_db_import_mirrored_user(conn: &Connection, site_id: &str, user: &str) -> Result<()> {
    let n = conn.execute(
        "UPDATE db_imports SET mirrored_user = ?2 WHERE site_id = ?1",
        params![site_id, user],
    )?;
    if n == 0 {
        return Err(Error::Other(format!("no database import is recorded for site {site_id}")));
    }
    Ok(())
}

fn row_to_db_import(row: &Row) -> rusqlite::Result<DbImportRecord> {
    let state_txt: String = row.get(1)?;
    let verified_txt: Option<String> = row.get(8)?;
    // Strict: the writers above can only produce these three shapes, so
    // anything else is corruption and fails loudly rather than rendering a
    // guessed badge.
    let state = match (state_txt.as_str(), verified_txt.as_deref()) {
        ("imported", None) => DbImportState::Imported,
        ("connected", Some("signin")) => {
            DbImportState::Connected(ConnectedVerified { kind: VerifiedKind::Signin })
        }
        ("connected", Some("signin+http")) => {
            DbImportState::Connected(ConnectedVerified { kind: VerifiedKind::SigninHttp })
        }
        (s, v) => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                format!("db_imports state '{s}' / verified {v:?} is outside the closed set").into(),
            ))
        }
    };
    Ok(DbImportRecord {
        site_id: row.get(0)?,
        state,
        db_name: row.get(2)?,
        table_count: row.get::<_, i64>(3)? as u64,
        size_bytes: row.get::<_, i64>(4)? as u64,
        source_label: row.get(5)?,
        mirrored_user: row.get(6)?,
        skipped_tables: decode_skipped(&row.get::<_, String>(9)?),
        imported_at: row.get(7)?,
    })
}

/// Skipped-table names as ONE column: newline-separated, which cannot collide
/// with a table name (MySQL identifiers can hold spaces and commas, but a
/// newline cannot survive `information_schema` → argv → here without being
/// visible, and no dump tool would have accepted it either).
fn encode_skipped(tables: &[String]) -> String {
    tables.join("\n")
}

fn decode_skipped(raw: &str) -> Vec<String> {
    raw.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

pub fn get_db_import(conn: &Connection, site_id: &str) -> Result<Option<DbImportRecord>> {
    let mut stmt = conn.prepare(
        "SELECT site_id, state, db_name, table_count, size_bytes, source_label, mirrored_user, imported_at, verified, skipped_tables
         FROM db_imports WHERE site_id = ?1",
    )?;
    let mut rows = stmt.query_map([site_id], row_to_db_import)?;
    Ok(rows.next().transpose()?)
}

pub fn list_db_imports(conn: &Connection) -> Result<Vec<DbImportRecord>> {
    let mut stmt = conn.prepare(
        "SELECT site_id, state, db_name, table_count, size_bytes, source_label, mirrored_user, imported_at, verified, skipped_tables
         FROM db_imports",
    )?;
    let rows = stmt.query_map([], row_to_db_import)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// Remove a site's import record (site deletion).
pub fn delete_db_import(conn: &Connection, site_id: &str) -> Result<()> {
    conn.execute("DELETE FROM db_imports WHERE site_id = ?1", [site_id])?;
    Ok(())
}

/// A connection-config rewrite's backup record (v21) — the row is the sole
/// owner of its backup file; both are created together and removed together
/// (the v18 resolver-takeover pattern).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRewrite {
    pub site_id: String,
    /// The rewritten file's path inside the user's project, as written.
    pub file: String,
    /// Our 0600 copy of the file AS IT WAS BEFORE rexenv ever touched it.
    pub backup_path: String,
    pub written_at: String,
    /// sha256 hex of the content the rewrite wrote (v22), recorded AFTER the
    /// rename succeeds. `None` = can't prove the file is unchanged — revert
    /// treats it as the conservative edited-since branch, never as a match.
    pub written_digest: Option<String>,
}

/// Record a rewrite's backup. INSERT, never upsert — FIRST BACKUP WINS: a
/// second rewrite of the same file must reuse the existing record (and its
/// backup), because overwriting the backup with already-rewritten content
/// would silently turn revert into a lie. The (site_id, file) PRIMARY KEY
/// is the backstop; callers check [`get_config_rewrite`] first.
pub fn insert_config_rewrite(
    conn: &Connection,
    site_id: &str,
    file: &str,
    backup_path: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO config_rewrites (site_id, file, backup_path) VALUES (?1, ?2, ?3)",
        params![site_id, file, backup_path],
    )?;
    Ok(())
}

fn row_to_config_rewrite(row: &Row) -> rusqlite::Result<ConfigRewrite> {
    Ok(ConfigRewrite {
        site_id: row.get(0)?,
        file: row.get(1)?,
        backup_path: row.get(2)?,
        written_at: row.get(3)?,
        written_digest: row.get(4)?,
    })
}

pub fn get_config_rewrite(
    conn: &Connection,
    site_id: &str,
    file: &str,
) -> Result<Option<ConfigRewrite>> {
    let mut stmt = conn.prepare(
        "SELECT site_id, file, backup_path, written_at, written_digest FROM config_rewrites
         WHERE site_id = ?1 AND file = ?2",
    )?;
    let mut rows = stmt.query_map(params![site_id, file], row_to_config_rewrite)?;
    Ok(rows.next().transpose()?)
}

/// All rewrite records for one site — site delete restores/cleans these.
pub fn config_rewrites_for_site(conn: &Connection, site_id: &str) -> Result<Vec<ConfigRewrite>> {
    let mut stmt = conn.prepare(
        "SELECT site_id, file, backup_path, written_at, written_digest FROM config_rewrites
         WHERE site_id = ?1",
    )?;
    let rows = stmt.query_map([site_id], row_to_config_rewrite)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// Every rewrite record — the orphan sweep enumerates against this
/// (reports, never auto-deletes).
pub fn list_config_rewrites(conn: &Connection) -> Result<Vec<ConfigRewrite>> {
    let mut stmt = conn.prepare(
        "SELECT site_id, file, backup_path, written_at, written_digest FROM config_rewrites",
    )?;
    let rows = stmt.query_map([], row_to_config_rewrite)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// Record what a successful write actually put on disk — called AFTER the
/// rename, so a crash before it leaves NULL, which revert reads as the
/// conservative "can't prove unchanged" branch.
pub fn set_config_rewrite_digest(
    conn: &Connection,
    site_id: &str,
    file: &str,
    digest: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE config_rewrites SET written_digest = ?3 WHERE site_id = ?1 AND file = ?2",
        params![site_id, file, digest],
    )?;
    Ok(())
}

/// Remove a rewrite record (revert, or site delete) — the caller deletes the
/// backup file in the same operation.
pub fn delete_config_rewrite(conn: &Connection, site_id: &str, file: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM config_rewrites WHERE site_id = ?1 AND file = ?2",
        params![site_id, file],
    )?;
    Ok(())
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
        fpm_port: row.get::<_, i64>(1)? as u16,
        installed: row.get::<_, i64>(2)? != 0,
        is_default: row.get::<_, i64>(3)? != 0,
        selected_patch: row.get(4)?,
    })
}

/// Insert a PHP version, or refresh the DERIVED part of an existing row.
///
/// **Only `fpm_port` is updated.** The other three columns are split by who owns
/// the fact, which is the question this statement kept getting wrong:
///
/// - `fpm_port` is computed from the minor ([`core::php::fpm_port`]) and owned by
///   the app, so re-seeding may refresh it.
/// - `installed` is the user's enable/disable choice ([`set_php_installed`]).
/// - `is_default` is the user's "Make default" choice ([`set_default_php_version`]).
///   It used to be in this SET list, sourced from the compiled-in pin, so every
///   launch silently reset the user's chosen default back to the pinned minor —
///   `installed` sat one line away, deliberately excluded for exactly this reason.
///
/// On INSERT all three are seeded, which is right: a fresh row has no user choice
/// to protect — and `selected_patch` is deliberately NOT among them, so a new row
/// follows the pin until the user says otherwise. (`patch` used to be a column
/// here and is now DERIVED — migration v36. `selected_patch` is its opposite: a
/// user fact, so it is stored and never written by the seed.)
pub fn upsert_php_version(conn: &Connection, v: &PhpVersion) -> Result<()> {
    conn.execute(
        // `selected_patch` is ABSENT FROM BOTH ARMS ON PURPOSE — not an omission
        // to tidy up. It is the user's Update choice; `set_php_selected_patch` is
        // its only writer. Adding it here would reintroduce the #339/#340 family
        // (a seed overwriting a user fact) and `the_seed_cannot_write_a_selected_patch_even_when_handed_one`
        // is what stops that, because the struct DOES carry the field and would
        // happily supply it.
        "INSERT INTO php_versions (minor, fpm_port, installed, is_default)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(minor) DO UPDATE SET
             fpm_port = excluded.fpm_port",
        params![
            v.minor,
            v.fpm_port as i64,
            v.installed as i64,
            v.is_default as i64,
        ],
    )?;
    Ok(())
}

/// Record (or clear) the patch a user chose for `minor`. `None` = follow the pin.
///
/// Its own writer, like `set_php_installed` and `set_default_php_version`, and for
/// the same reason: it is a USER fact, so nothing that runs unasked may touch it.
/// The seed writes every other column on this row and must never write this one
/// (`every_upsert_updates_only_columns_the_app_owns`).
pub fn set_php_selected_patch(conn: &Connection, minor: &str, patch: Option<&str>) -> Result<bool> {
    let affected = conn.execute(
        "UPDATE php_versions SET selected_patch = ?1 WHERE minor = ?2",
        params![patch, minor],
    )?;
    Ok(affected > 0)
}

/// All registered PHP versions, ordered by minor series.
pub fn list_php_versions(conn: &Connection) -> Result<Vec<PhpVersion>> {
    let mut stmt = conn.prepare(
        "SELECT minor, fpm_port, installed, is_default, selected_patch
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

// ── Extra domains a site answers on (v42) ─────────────────────────────────────

/// One site's alias domains, sorted (deterministic config emission, like
/// `get_site_env`). The PRIMARY domain is not in here — it lives on the site
/// row, and every caller that wants "every hostname this site answers on"
/// composes the two through `core::sites::all_domains`.
pub fn get_site_aliases(conn: &Connection, site_id: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT domain FROM site_domains WHERE site_id = ?1 ORDER BY domain")?;
    let rows = stmt.query_map([site_id], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every alias grouped by site id — one read for a whole config regeneration,
/// the same shape as `all_site_env`.
pub fn all_site_aliases(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, Vec<String>>> {
    let mut stmt =
        conn.prepare("SELECT site_id, domain FROM site_domains ORDER BY site_id, domain")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out: std::collections::HashMap<String, Vec<String>> = Default::default();
    for row in rows {
        let (site_id, domain) = row?;
        out.entry(site_id).or_default().push(domain);
    }
    Ok(out)
}

/// Is `domain` already an alias of some site? Returns that site's id.
pub fn site_id_for_alias(conn: &Connection, domain: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT site_id FROM site_domains WHERE domain = ?1")?;
    let mut rows = stmt.query([domain])?;
    Ok(match rows.next()? {
        Some(r) => Some(r.get(0)?),
        None => None,
    })
}

/// Record an alias. The caller has already validated it
/// (`core::sites::validate_alias`) — the UNIQUE constraint here is the backstop
/// for a race between two writers, not the check.
pub fn add_site_alias(conn: &Connection, site_id: &str, domain: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO site_domains (site_id, domain) VALUES (?1, ?2)",
        params![site_id, domain],
    )?;
    Ok(())
}

/// Remove one alias; `false` when the site did not answer on it.
pub fn remove_site_alias(conn: &Connection, site_id: &str, domain: &str) -> Result<bool> {
    let n = conn.execute(
        "DELETE FROM site_domains WHERE site_id = ?1 AND domain = ?2",
        params![site_id, domain],
    )?;
    Ok(n > 0)
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
/// One agent's recorded permission to READ one site's database (v38).
///
/// Every field is recorded rather than derived, and `expires_at` is the reason:
/// a grant the user made on a Tuesday must die on the Tuesday they were told
/// about, not seven days after whenever the code next looks at it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDbGrant {
    pub id: String,
    pub site_id: String,
    /// The MCP `clientInfo` name the grant was given TO. A grant to one agent is
    /// not a grant to the next one that connects — the plan re-prompts on a
    /// client change, and that is only possible because this is stored.
    pub client: String,
    /// The database principal created for it (`rex_agent_*`), so revocation can
    /// drop exactly the user the grant created and nothing else.
    pub db_user: String,
    pub granted_at: String,
    pub expires_at: String,
    /// True when auto-allow produced this grant instead of a human clicking
    /// Allow. Recorded, never re-derived: the toggle is session-scoped, so by
    /// the time anyone reads the list it will usually be off, and "was this
    /// approved by a person?" would then answer wrongly for every past row.
    pub auto_granted: bool,
    /// Set when the user revoked it. The row is KEPT: a revoked grant is
    /// evidence about what an agent could see and until when, and the feed is
    /// where a user goes to find that out. Deleting it deletes the answer.
    pub revoked_at: Option<String>,
}

/// Record a grant. `days` is added by the DATABASE, from the database's own
/// clock, so the stored expiry and the stored `granted_at` cannot disagree by a
/// process clock skew or a timezone.
pub fn grant_agent_db(
    conn: &Connection,
    id: &str,
    site_id: &str,
    client: &str,
    db_user: &str,
    days: u32,
    auto_granted: bool,
) -> Result<AgentDbGrant> {
    conn.execute(
        "INSERT INTO agent_db_grants
             (id, site_id, client, db_user, granted_at, expires_at, auto_granted)
         VALUES (?1, ?2, ?3, ?4, datetime('now'), datetime('now', ?5), ?6)",
        rusqlite::params![id, site_id, client, db_user, format!("+{days} days"), auto_granted],
    )?;
    get_agent_db_grant(conn, id)?
        .ok_or_else(|| crate::error::Error::Other("grant vanished after insert".into()))
}

pub fn get_agent_db_grant(conn: &Connection, id: &str) -> Result<Option<AgentDbGrant>> {
    let mut st = conn.prepare(
        "SELECT id, site_id, client, db_user, granted_at, expires_at, revoked_at, auto_granted
         FROM agent_db_grants WHERE id = ?1",
    )?;
    let mut rows = st.query([id])?;
    match rows.next()? {
        Some(r) => Ok(Some(AgentDbGrant {
            id: r.get(0)?,
            site_id: r.get(1)?,
            client: r.get(2)?,
            db_user: r.get(3)?,
            granted_at: r.get(4)?,
            expires_at: r.get(5)?,
            revoked_at: r.get(6)?,
            auto_granted: r.get::<_, i64>(7)? != 0,
        })),
        None => Ok(None),
    }
}

/// The grant that lets `client` read `site_id` RIGHT NOW, if there is one.
///
/// "Now" is the DATABASE's clock, compared in SQL, for the reason the expiry is
/// stored at all: a check that read the row and compared in Rust would be one
/// more place for a clock to disagree with the one that wrote it. Revoked and
/// expired are both simply absent — a caller cannot accidentally treat either as
/// live, because neither is ever returned.
pub fn active_agent_db_grant(
    conn: &Connection,
    site_id: &str,
    client: &str,
) -> Result<Option<AgentDbGrant>> {
    let mut st = conn.prepare(
        "SELECT id FROM agent_db_grants
         WHERE site_id = ?1 AND client = ?2
           AND revoked_at IS NULL
           AND expires_at > datetime('now')
         ORDER BY granted_at DESC LIMIT 1",
    )?;
    let mut rows = st.query(rusqlite::params![site_id, client])?;
    match rows.next()? {
        Some(r) => {
            let id: String = r.get(0)?;
            drop(rows);
            get_agent_db_grant(conn, &id)
        }
        None => Ok(None),
    }
}

/// Every grant for the UI: live, expired and revoked alike, newest first. The
/// screen that lists them is also the screen a user checks after the fact, so it
/// must show what WAS allowed, not only what still is.
pub fn list_agent_db_grants(conn: &Connection) -> Result<Vec<AgentDbGrant>> {
    let mut st = conn.prepare(
        "SELECT id FROM agent_db_grants ORDER BY granted_at DESC",
    )?;
    let ids: Vec<String> =
        st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(g) = get_agent_db_grant(conn, &id)? {
            out.push(g);
        }
    }
    Ok(out)
}

/// Revoke a grant. Idempotent and never un-revokes: a second call leaves the
/// FIRST revocation time standing, because that is when the access actually
/// stopped.
pub fn revoke_agent_db_grant(conn: &Connection, id: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE agent_db_grants SET revoked_at = datetime('now')
         WHERE id = ?1 AND revoked_at IS NULL",
        [id],
    )?;
    Ok(n > 0)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::db;

    /// **A grant dies on the clock it was written with, and neither expiry nor
    /// revocation can be mistaken for live access.**
    ///
    /// This is the ledger the whole M3 consent story rests on: the dialog
    /// promises "this access expires in 7 days", and that promise is only true
    /// if the expiry is a stored fact compared against the same clock that wrote
    /// it. A duration added at read time would make the promise "seven days from
    /// whenever something next looked", which is a different sentence.
    #[test]
    fn a_db_grant_expires_on_its_own_clock_and_revocation_is_not_reversible() {
        let conn = db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, status, php_version, web_server, ssl,
                                path, db_name, db_engine)
             VALUES ('s1','S','s.rex','php','stopped','8.3','nginx',1,'/tmp/s','wp_s','mysql')",
            [],
        )
        .unwrap();

        let live = grant_agent_db(&conn, "g1", "s1", "Claude Code", "rex_agent_s1", 7, false).unwrap();
        assert!(live.expires_at > live.granted_at, "expiry must be after the grant");
        assert!(live.revoked_at.is_none());

        // The lookup is what every caller will use, so it is what must be right.
        let found = active_agent_db_grant(&conn, "s1", "Claude Code").unwrap();
        assert_eq!(found.map(|g| g.id), Some("g1".to_string()));

        // A grant to ONE client is not a grant to the next agent that connects —
        // the re-consent-on-client-change rule is only possible because `client`
        // is part of the identity rather than a label on the row.
        assert!(active_agent_db_grant(&conn, "s1", "Some Other Agent").unwrap().is_none());
        // …nor to another site.
        assert!(active_agent_db_grant(&conn, "s2", "Claude Code").unwrap().is_none());

        // EXPIRED: written in the past, so the row exists and the lookup refuses
        // it. Both halves matter — the UI must still show it, the gate must not.
        grant_agent_db(&conn, "g2", "s1", "Old Agent", "rex_agent_s1_old", 7, false).unwrap();
        conn.execute(
            "UPDATE agent_db_grants SET expires_at = datetime('now','-1 day') WHERE id='g2'",
            [],
        )
        .unwrap();
        assert!(active_agent_db_grant(&conn, "s1", "Old Agent").unwrap().is_none());
        assert!(get_agent_db_grant(&conn, "g2").unwrap().is_some(), "the row is evidence, kept");

        // REVOKED: gone from the gate immediately, kept in the list.
        assert!(revoke_agent_db_grant(&conn, "g1").unwrap());
        assert!(active_agent_db_grant(&conn, "s1", "Claude Code").unwrap().is_none());
        let revoked = get_agent_db_grant(&conn, "g1").unwrap().unwrap();
        let first_time = revoked.revoked_at.clone().expect("revoked_at recorded");

        // Revoking again must NOT move the timestamp: when access actually
        // stopped is the fact, and a second click is not a second stopping.
        assert!(!revoke_agent_db_grant(&conn, "g1").unwrap(), "second revoke is a no-op");
        assert_eq!(
            get_agent_db_grant(&conn, "g1").unwrap().unwrap().revoked_at,
            Some(first_time),
            "a second revoke rewrote when the access stopped"
        );

        // The list shows everything — live, expired and revoked — because it is
        // also the screen a user checks AFTER the fact.
        assert_eq!(list_agent_db_grants(&conn).unwrap().len(), 2);

        // And a deleted site takes its grants with it: leaving them would let a
        // re-created site inherit permission nobody granted it.
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute("DELETE FROM sites WHERE id='s1'", []).unwrap();
        assert!(list_agent_db_grants(&conn).unwrap().is_empty(), "grants outlived their site");
    }

    /// Every column an upsert may write in its `DO UPDATE SET` list, per table.
    ///
    /// **The list is DERIVED columns only** — values the app computes and owns.
    /// A column a USER can set must never appear here, because the conflict arm
    /// runs on writes the user did not ask for. Each entry names its owner so
    /// the next person adding one has to answer the question rather than pattern-
    /// match the line above.
    const DERIVED_UPDATE_COLUMNS: &[(&str, &[&str])] = &[
        // fpm_port: computed from the minor by `php::fpm_port`. `installed` and
        // `is_default` are the USER's and are absent on purpose — that absence is
        // ledger #339/#340/#344, three bugs in this one statement.
        ("php_versions", &["fpm_port"]),
        // All app-computed: the resolver file we replaced and where we stashed it.
        ("resolver_takeovers", &["original", "backup_path", "taken_at"]),
        // A KV whose OWNER is the key, not the column — every write is either a
        // user action through a validating setter or an app cache writing its own
        // key. Guarded separately by `every_gated_setting_key_is_routed_here`.
        ("settings", &["value"]),
        // Recomputed per sync; `kind`/`source_path` are the caller's and are
        // deliberately preserved.
        ("scratch_packages", &["synced_at", "fingerprint"]),
        // Every column is a measurement of the import that just ran.
        (
            "db_imports",
            &[
                "state",
                "db_name",
                "table_count",
                "size_bytes",
                "source_label",
                "mirrored_user",
                "skipped_tables",
                "verified",
                "imported_at",
            ],
        ),
    ];

    /// Upserts whose ONLY writers are explicit user (or agent) actions — "save
    /// this thing", where overwriting the stored values is the entire point.
    ///
    /// A separate category rather than a hole in the one above, because the
    /// question a source scan cannot answer is WHO CALLS IT. Declaring the answer
    /// is the work: if an automatic caller is ever added to one of these, this
    /// line is what makes that a decision instead of an accident.
    const USER_INITIATED_UPSERTS: &[(&str, &str)] = &[(
        "blueprints",
        "`upsert_blueprint` runs only from the blueprint save command — the user is \
         supplying `name`/`spec`, so overwriting them is what they asked for",
    )];

    /// Upsert statements this codebase is declared to have. Bumping it is the
    /// point: see the failure message.
    const DECLARED_UPSERTS: usize = 7;

    /// `INSERT OR REPLACE` statements that deliberately do NOT name every column,
    /// with the reason. REPLACE deletes the row and re-inserts, so any unnamed
    /// column silently returns to its default — there is no SET list to read, and
    /// that is exactly why these need declaring by hand.
    const PARTIAL_REPLACE_EXCEPTIONS: &[(&str, &str)] = &[(
        "site_git_assets",
        "resets composer/node install fingerprints to NULL and re-dates created_at; \
         a fresh checkout is genuinely unverified (store.rs' own note), and NULL reads \
         as unverified rather than as stale",
    )];

    /// **Every `ON CONFLICT … DO UPDATE SET` list contains only DERIVED columns,
    /// and a new upsert anywhere in the tree must be declared.**
    ///
    /// The guard #339, #340 and #344 earned. One statement was wrong three times
    /// — `patch` and `is_default` in the SET list, then `is_default` again in the
    /// INSERT arm — and each fix was reasoned about column-by-column while the
    /// neighbouring arm went unread. The question that catches all three is not
    /// "is this line right" but "for each column, does the USER own this value or
    /// does the app?", asked of the whole statement.
    ///
    /// **The count assertion is the load-bearing half.** A scan that reads only
    /// the file it knows about is the guard-covers-claimed-surface defect this
    /// project has shipped four times — once inside a guard written to end that
    /// family. So this walks the entire crate, not `store.rs`, and fails when the
    /// number of upsert sites changes at all.
    #[test]
    fn every_upsert_updates_only_columns_the_app_owns() {
        use crate::core::copy_scan::production_source;

        // Whoever trips this guard is adding or editing an upsert, so the message
        // teaches the rule rather than reporting a diff. Four guards in this repo
        // have claimed a surface wider than they checked; the fifth should at
        // least tell the next person what it is actually asking.
        const RULE: &str = "\
THE RULE, because you are probably adding or editing one:\n\
• A `DO UPDATE SET` list may name ONLY columns the APP owns — values it computes.\n\
• A column a USER can set must never appear there. The conflict arm runs on writes the \
user did not ask for, so anything in it is something a launch or a background task can \
silently take back — the user sets it, restarts, and it is quietly gone.\n\
• Ask it PER COLUMN, and for the INSERT arm too: a row that does not exist yet takes that \
arm instead. `php_versions` was wrong three times (ledger #339 `patch`, #340 `is_default`, \
#344 `is_default` again in the INSERT arm) and every fix read one arm and not the other.\n\n\
If the app owns it: add the table + its derived columns to DERIVED_UPDATE_COLUMNS, naming \
each column's owner.\n\
If only an explicit user action can reach the statement: add it to USER_INITIATED_UPSERTS \
with the reason — that is the one question a source scan cannot answer for itself.\n\
Either way, bump DECLARED_UPSERTS. The count exists so a statement in a file this guard \
has never heard of cannot pass unread.";

        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&path) else { continue };
                // Production only (a test fixture writing SQL is not a writer),
                // and COMMENTS STRIPPED. The first run of this guard counted
                // three prose mentions of `ON CONFLICT` as statements — one of
                // them written an hour earlier, in the doc comment explaining
                // this very defect. A scanner that reads its own explanation is
                // the trap this repo has now hit four times, and it landed here
                // in the COUNT, which is the half that is supposed to be
                // load-bearing.
                let code: String = production_source(&raw)
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                out.push((path.display().to_string(), code));
            }
        }

        let mut files = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        assert!(!files.is_empty(), "scanned nothing — the walk is broken, not the code");

        let mut upserts = 0usize;
        let mut replaces: Vec<(String, String)> = Vec::new();
        let mut offences: Vec<String> = Vec::new();

        for (file, src) in &files {
            // Normalised to one line so a statement split across lines — the one
            // syntactic form that could hide from a line-wise scan — still parses.
            let flat = src.split_whitespace().collect::<Vec<_>>().join(" ");
            for (i, _) in flat.match_indices("ON CONFLICT") {
                upserts += 1;
                let after = &flat[i..];
                // Bound the search to THIS SQL literal. Without this, a
                // `DO NOTHING` statement finds the NEXT statement's SET list and
                // reports its columns against the wrong table — which is exactly
                // what the first run of this guard did.
                let stmt = &after[..after.find('"').unwrap_or(after.len())];
                let Some(set_at) = stmt.find("DO UPDATE SET") else {
                    continue; // DO NOTHING — writes nothing, so nothing to own
                };
                let table = flat[..i]
                    .rmatch_indices("INSERT INTO ")
                    .next()
                    .map(|(j, _)| flat[j + "INSERT INTO ".len()..].split_whitespace().next().unwrap_or(""))
                    .unwrap_or("")
                    .to_string();
                let body = &stmt[set_at + "DO UPDATE SET".len()..];
                let end = body.len();
                if USER_INITIATED_UPSERTS.iter().any(|(t, _)| *t == table) {
                    continue; // the user supplied these values; see the constant
                }
                let allowed: &[&str] = DERIVED_UPDATE_COLUMNS
                    .iter()
                    .find(|(t, _)| *t == table)
                    .map(|(_, cols)| *cols)
                    .unwrap_or(&[]);
                for assign in body[..end].split(',') {
                    let Some(col) = assign.split('=').next() else { continue };
                    let col = col.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                    if col.is_empty() {
                        continue;
                    }
                    if !allowed.contains(&col) {
                        offences.push(format!(
                            "{file}: `{table}` upsert updates `{col}`, which is not on that \
                             table's DERIVED list"
                        ));
                    }
                }
            }
            for (i, _) in flat.match_indices("INSERT OR REPLACE INTO ") {
                let rest = &flat[i + "INSERT OR REPLACE INTO ".len()..];
                let table = rest.split_whitespace().next().unwrap_or("").to_string();
                replaces.push((file.clone(), table));
            }
        }

        assert!(
            offences.is_empty(),
            "{}\n\n{RULE}",
            offences.join("\n")
        );

        for (file, table) in &replaces {
            assert!(
                PARTIAL_REPLACE_EXCEPTIONS.iter().any(|(t, _)| t == table),
                "{file}: `INSERT OR REPLACE INTO {table}` names a subset of its columns and \
                 every unnamed one silently returns to its default. Name them all, or add \
                 `{table}` to PARTIAL_REPLACE_EXCEPTIONS with the reason it is safe."
            );
        }

        assert_eq!(
            upserts, DECLARED_UPSERTS,
            "\nThis crate now has {upserts} `ON CONFLICT` statements; {DECLARED_UPSERTS} are \
             declared.\n\n{RULE}"
        );
    }

    /// A scratch row in production shape (UUID id, absolute app-data docroot,
    /// a real `datetime('now')`-style expiry).
    fn scratch(agent_client: Option<&str>) -> Site {
        Site {
            id: "b41d7c58-2e0a-49f6-9a13-7d5c8e2f4011".into(),
            name: "probe".into(),
            domain: "probe.scratch.rex".into(),
            site_type: SiteType::Wordpress,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            ssl: true,
            path: "/Users/x/Library/Application Support/dev.rexenv.rexenv/Sites/probe.scratch.rex"
                .into(),
            created_at: "2026-08-01 09:00:00".into(),
            multisite: MultisiteMode::None,
            db_name: "wp_probe_scratch".into(),
            db_engine: SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed: Some(true),
            db_created: None,
            content_dir: None,
            mu_dir_created: None,
            origin: SiteOrigin::Agent,
            agent_client: agent_client.map(str::to_string),
            expires_at: Some("2026-08-02 09:00:00".into()),
            docroot_subdir: String::new(),
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
            starter_db: None,
        }
    }

    #[test]
    fn v27_fields_round_trip_through_the_row_mapping() {
        // Guards the column-order coupling between SITE_COLUMNS, row_to_site's
        // indices and insert_site's params — three lists that must agree and
        // that nothing else would catch until a site read back wrong.
        let conn = db::open_in_memory().unwrap();
        let site = scratch(Some("Claude Code"));
        insert_site(&conn, &site).unwrap();
        let back = get_site(&conn, &site.id).unwrap().unwrap();
        assert_eq!(back.origin, SiteOrigin::Agent);
        assert!(back.is_scratch());
        assert_eq!(back.agent_client.as_deref(), Some("Claude Code"));
        assert_eq!(back.expires_at.as_deref(), Some("2026-08-02 09:00:00"));
        // The neighbours still land where they belong.
        assert_eq!(back.domain, "probe.scratch.rex");
        assert_eq!(back.docroot_managed, Some(true));
        assert_eq!(back.mu_dir_created, None);
    }

    /// v41 is now the LAST column of the three coupled lists (SITE_COLUMNS,
    /// row_to_site's indices, insert_site's params), so the off-by-one that
    /// used to land on `git_build_assets` now lands here. Round-tripped in all
    /// three states because they are three DIFFERENT answers: yes, no, and
    /// "the question does not apply to this site".
    #[test]
    fn v41_starter_db_round_trips_through_the_row_mapping() {
        let conn = db::open_in_memory().unwrap();
        for (id, want) in
            [("s-yes", Some(true)), ("s-no", Some(false)), ("s-na", None)]
        {
            let site = Site {
                id: id.into(),
                domain: format!("{id}.rex"),
                site_type: SiteType::Php,
                starter_db: want,
                ..scratch(None)
            };
            insert_site(&conn, &site).unwrap();
            let back = get_site(&conn, id).unwrap().unwrap();
            assert_eq!(back.starter_db, want, "{id}");
            assert_eq!(back.has_starter_db(), want.unwrap_or(false), "{id}");
            // The neighbour: an index slip past the end of the row would take
            // this column's value from the one before it.
            assert_eq!(back.git_build_assets, None, "{id}");
        }
    }

    /// v33 is the SECOND pair appended to the three coupled lists (SITE_COLUMNS,
    /// row_to_site's indices, insert_site's params). v27 caught the coupling for
    /// the middle of the row; these are the LAST two columns, where an
    /// off-by-one reads past the end and fails loudly — but only if something
    /// actually reads them back.
    #[test]
    fn v33_repo_origin_round_trips_through_the_row_mapping() {
        let conn = db::open_in_memory().unwrap();
        // A site with no repo: both NULL, and NULL is the answer for every
        // site that was not cloned.
        let plain = scratch(None);
        insert_site(&conn, &plain).unwrap();
        let back = get_site(&conn, &plain.id).unwrap().unwrap();
        assert_eq!(back.git_url, None);
        assert_eq!(back.git_ref, None);

        let cloned = Site {
            id: "0b6e2a91-4c73-4f10-9d2e-5a1f8c3b7e42".into(),
            domain: "shop.rex".into(),
            git_url: Some("https://github.com/acme/shop.git".into()),
            git_ref: Some("develop".into()),
            ..scratch(None)
        };
        insert_site(&conn, &cloned).unwrap();
        let back = get_site(&conn, &cloned.id).unwrap().unwrap();
        assert_eq!(back.git_url.as_deref(), Some("https://github.com/acme/shop.git"));
        assert_eq!(back.git_ref.as_deref(), Some("develop"));
        // The neighbours are intact — these are the LAST two columns of three
        // coupled lists, where an index slip would land the URL in
        // `docroot_subdir` and serve the project root.
        assert_eq!(back.docroot_subdir, "");
        assert_eq!(back.expires_at.as_deref(), Some("2026-08-02 09:00:00"));

        // No ref = the remote's default branch. NULL, not the empty string:
        // "they picked nothing" and "they picked ''" must not read alike.
        let default_branch =
            Site { id: "c4d1".into(), domain: "b.rex".into(), git_ref: None, ..cloned };
        insert_site(&conn, &default_branch).unwrap();
        assert_eq!(get_site(&conn, "c4d1").unwrap().unwrap().git_ref, None);
    }

    /// The docroot subdir is the ONE field that decides what the web server can
    /// reach, and v33 makes it writable after creation (a clone learns its
    /// layout from the checkout). Refusing the escapes HERE — the single place
    /// it can change — is what keeps `Site::served_root` inside the project.
    #[test]
    fn a_docroot_subdir_that_escapes_the_project_is_refused_at_the_only_writer() {
        let conn = db::open_in_memory().unwrap();
        let site = scratch(None);
        insert_site(&conn, &site).unwrap();

        for bad in ["/etc", "../../../Users/x/.ssh", "public/../..", "/", "\\Windows"] {
            assert!(
                set_site_docroot_subdir(&conn, &site.id, bad).is_err(),
                "{bad} must not become a document root"
            );
        }
        // Still the value the row was inserted with — a refusal writes nothing.
        assert_eq!(get_site(&conn, &site.id).unwrap().unwrap().docroot_subdir, "");

        set_site_docroot_subdir(&conn, &site.id, "public").unwrap();
        assert_eq!(get_site(&conn, &site.id).unwrap().unwrap().docroot_subdir, "public");
        // A nested entry point is legitimate (Radicle's `public/content` sits
        // under one), and so is clearing back to "serve the path itself".
        set_site_docroot_subdir(&conn, &site.id, "web/public").unwrap();
        assert_eq!(get_site(&conn, &site.id).unwrap().unwrap().docroot_subdir, "web/public");
        set_site_docroot_subdir(&conn, &site.id, "").unwrap();
        assert_eq!(get_site(&conn, &site.id).unwrap().unwrap().docroot_subdir, "");
    }

    #[test]
    fn v31_skipped_tables_round_trip_and_an_empty_list_stays_empty() {
        // Same column-order coupling as v27, one table over — the SELECTs read
        // `skipped_tables` at index 9 while `imported_at` sits at 7, so a
        // mis-numbered index would silently render the timestamp as a table name.
        let conn = db::open_in_memory().unwrap();
        let site = scratch(None);
        insert_site(&conn, &site).unwrap();
        let with_skips = NewDbImport {
            site_id: site.id.clone(),
            db_name: "shop".into(),
            table_count: 173,
            size_bytes: 2_101_867_055,
            source_label: "MySQL 8.0.27 at 127.0.0.1:3306".into(),
            mirrored_user: Some("wp".into()),
            skipped_tables: vec!["wp_wsal_metadata".into(), "wp_post_views".into()],
        };
        let rec = upsert_db_import(&conn, &with_skips).unwrap();
        assert_eq!(rec.skipped_tables, vec!["wp_wsal_metadata", "wp_post_views"]);
        assert_eq!(rec.table_count, 173);
        assert_eq!(rec.imported_at.len(), 19, "imported_at must not have shifted columns");
        // Re-importing cleanly must CLEAR the list, not leave the old one
        // standing — a stale skip list would report missing data that is now
        // present, which is the same lie in the other direction.
        let clean = NewDbImport { skipped_tables: Vec::new(), ..with_skips };
        assert!(upsert_db_import(&conn, &clean).unwrap().skipped_tables.is_empty());
    }

    #[test]
    fn the_agent_asserted_client_name_is_capped_at_the_write() {
        // `agent_client` comes off the wire (`clientInfo.name`) bounded only by
        // the session's 4 MB line cap, so an agent could otherwise drive
        // megabytes per site row into the app database. Capped HERE — at the
        // write — so it holds whatever built the `Site`, not only the MCP path.
        let conn = db::open_in_memory().unwrap();
        let huge = "A".repeat(10_000);
        insert_site(&conn, &scratch(Some(&huge))).unwrap();
        let back = get_site(&conn, "b41d7c58-2e0a-49f6-9a13-7d5c8e2f4011").unwrap().unwrap();
        assert_eq!(back.agent_client.as_ref().unwrap().chars().count(), AGENT_CLIENT_MAX);

        // Multi-byte input truncates on a char boundary rather than panicking
        // (the value is arbitrary UTF-8 an agent chose).
        let conn2 = db::open_in_memory().unwrap();
        let emoji = "🦀".repeat(1_000);
        insert_site(&conn2, &scratch(Some(&emoji))).unwrap();
        let back2 = get_site(&conn2, "b41d7c58-2e0a-49f6-9a13-7d5c8e2f4011").unwrap().unwrap();
        assert_eq!(back2.agent_client.as_ref().unwrap().chars().count(), AGENT_CLIENT_MAX);
    }

    /// Ledger #167 — the module-doc claim, as a scan of SQL-STRING CONTENT.
    ///
    /// Scanning the `rusqlite` IMPORT was the planned shape and was rejected in
    /// the ledger's own note as the surface-coverage trap: a module can receive
    /// a `&Connection` and hand-write SQL without importing anything. So the
    /// needle is the SQL itself — a verb keyword followed by an APP TABLE name
    /// — and the table list comes from the migrated schema at test time, never
    /// a hand-kept list (a new table joins the scan by existing).
    ///
    /// First run found the claim already false: `mcp_server/feed.rs` has
    /// hand-written `agent_actions` SQL since M2a. The guard therefore asserts
    /// the honest per-table ownership: state/ owns every table except
    /// `agent_actions`, which is feed.rs's — and feed.rs may touch no other.
    #[test]
    fn app_schema_sql_lives_only_in_each_tables_owning_module() {
        let conn = db::open_in_memory().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .unwrap();
        let tables: Vec<String> =
            stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|r| r.unwrap()).collect();
        assert!(tables.len() >= 5, "schema walk found {} tables — detection broken", tables.len());

        // SQL-shaped mention of an app table: verb keyword + the table as a
        // whole word. Uppercased text so casing can't dodge the scan.
        let hits = |text: &str| -> Vec<String> {
            let up = text.to_uppercase();
            let mut out = Vec::new();
            for t in &tables {
                let tu = t.to_uppercase();
                for kw in ["FROM ", "INTO ", "UPDATE ", "JOIN ", "TABLE "] {
                    let needle = format!("{kw}{tu}");
                    let mut at = 0;
                    while let Some(i) = up[at..].find(&needle) {
                        let end = at + i + needle.len();
                        let boundary = up[end..]
                            .chars()
                            .next()
                            .map(|c| !c.is_ascii_alphanumeric() && c != '_')
                            .unwrap_or(true);
                        if boundary {
                            out.push(format!("{kw}{t}"));
                        }
                        at = end;
                    }
                }
            }
            out
        };

        // Walk src/ (production lines only — comments and #[cfg(test)] modules
        // stripped, or the scan reads its own explanation and this very test's
        // fixtures).
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
                        let rel = path.strip_prefix(root).unwrap_or(&path).display().to_string();
                        let prod = crate::core::copy_scan::production_source(&text);
                        let stripped: String = prod
                            .lines()
                            .map(|l| {
                                let cut = l
                                    .match_indices("//")
                                    .find(|(i, _)| *i == 0 || !l[..*i].ends_with(':'))
                                    .map(|(i, _)| i);
                                cut.map_or(l, |i| &l[..i])
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        out.push((rel, stripped));
                    }
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut sources = Vec::new();
        walk(&root.join("src"), &mut sources);
        assert!(sources.len() > 50, "source walk found {} files — it stopped working", sources.len());

        let mut state_hits = 0usize;
        let mut feed_hits: Vec<String> = Vec::new();
        let mut violations: Vec<String> = Vec::new();
        for (path, text) in &sources {
            let found = hits(text);
            if found.is_empty() {
                continue;
            }
            if path.starts_with("src/state/") {
                state_hits += found.len();
            } else if path == "src/mcp_server/feed.rs" {
                feed_hits.extend(found);
            } else {
                for f in found {
                    violations.push(format!("{path}: {f}"));
                }
            }
        }

        // Canaries first: a matcher that finds nothing where SQL definitely
        // lives would make the zero below vacuous.
        assert!(state_hits >= 20, "only {state_hits} SQL hits in state/ — the matcher is broken");
        assert!(!feed_hits.is_empty(), "no hits in feed.rs — the matcher is broken");

        // feed.rs's exemption is scoped to ITS table, not a blanket.
        let strays: Vec<&String> =
            feed_hits.iter().filter(|h| !h.to_lowercase().contains("agent_actions")).collect();
        assert!(
            strays.is_empty(),
            "mcp_server/feed.rs touches app tables beyond agent_actions: {strays:?} — its \
             exemption covers only the table it owns; anything else goes through state/"
        );

        assert!(
            violations.is_empty(),
            "hand-written SQL against the app schema outside its owning module:\n  {}\n\
             The app database's SQL lives in state/ (all tables) or mcp_server/feed.rs \
             (agent_actions only). Add a function to the owning module instead — a second \
             writer is how a schema change breaks a caller nobody re-tested (#167).",
            violations.join("\n  ")
        );
    }
}
