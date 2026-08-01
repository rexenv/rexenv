//! Repository layer over SQLite — the only place that knows the app database's
//! `sites` table shape. `core/` reaches the app SQLite database only through
//! these functions; it never hand-writes SQL against the app schema.
//!
//! Withdrawn 29 Jul (was "core never writes SQL itself"): core DOES execute SQL
//! — it reads `information_schema` and issues `CREATE DATABASE`/`GRANT` against
//! the developer's MySQL/Postgres in `core::dbmirror`. The honest, narrow
//! guarantee is only that the app's OWN SQLite schema is state/'s alone.

use crate::error::{Error, Result};
use crate::state::models::{
    Blueprint, BlueprintSpec, GitAsset, MultisiteMode, PhpVersion, ServiceStatus, Site,
    SiteDbEngine, SiteOrigin, SiteType, WebServer,
};
use rusqlite::{params, Connection, Row};

/// Columns selected for a full `Site`, in struct order. Shared so every query
/// reads the same shape.
const SITE_COLUMNS: &str = "id, name, domain, type, status, php_version, web_server, ssl, path, \
     created_at, multisite, db_name, db_engine, xdebug, override_port, provisioned, \
     docroot_managed, db_created, content_dir, mu_dir_created, origin, agent_client, expires_at";

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
            (id, name, domain, type, status, php_version, web_server, ssl, path, created_at, multisite, db_name, db_engine, xdebug, override_port, provisioned, docroot_managed, db_created, content_dir, mu_dir_created, origin, agent_client, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
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
}

/// Upsert the settled import outcome for a site (a re-import replaces it)
/// and return the stored row — callers render what was written, not what
/// they meant to write.
pub fn upsert_db_import(conn: &Connection, r: &NewDbImport) -> Result<DbImportRecord> {
    conn.execute(
        "INSERT INTO db_imports (site_id, state, db_name, table_count, size_bytes, source_label, mirrored_user, verified)
         VALUES (?1, 'imported', ?2, ?3, ?4, ?5, ?6, NULL)
         ON CONFLICT(site_id) DO UPDATE SET
           state = 'imported', db_name = ?2, table_count = ?3, size_bytes = ?4,
           source_label = ?5, mirrored_user = ?6, verified = NULL,
           imported_at = datetime('now')",
        params![
            r.site_id,
            r.db_name,
            r.table_count as i64,
            r.size_bytes as i64,
            r.source_label,
            r.mirrored_user,
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
        imported_at: row.get(7)?,
    })
}

pub fn get_db_import(conn: &Connection, site_id: &str) -> Result<Option<DbImportRecord>> {
    let mut stmt = conn.prepare(
        "SELECT site_id, state, db_name, table_count, size_bytes, source_label, mirrored_user, imported_at, verified
         FROM db_imports WHERE site_id = ?1",
    )?;
    let mut rows = stmt.query_map([site_id], row_to_db_import)?;
    Ok(rows.next().transpose()?)
}

pub fn list_db_imports(conn: &Connection) -> Result<Vec<DbImportRecord>> {
    let mut stmt = conn.prepare(
        "SELECT site_id, state, db_name, table_count, size_bytes, source_label, mirrored_user, imported_at, verified
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::db;

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
}
