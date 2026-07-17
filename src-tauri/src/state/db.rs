//! SQLite connection + schema migrations (Phase 1 task 1.1).
//!
//! The database file lives under the platform `Paths::app_data_dir()` so each OS
//! uses its native convention. Migrations are applied with the `user_version`
//! pragma: each step bumps the version, so opening an existing db only runs the
//! steps it hasn't seen.

use crate::error::Result;
use crate::platform::traits::Paths;
use rusqlite::Connection;
use std::path::Path;

/// File name of the app-state database inside `app_data_dir`.
pub const DB_FILE: &str = "rexenv.db";

/// Ordered schema migrations. Index + 1 is the resulting `user_version`.
const MIGRATIONS: &[&str] = &[
    // v1 — initial schema
    "CREATE TABLE sites (
        id           TEXT PRIMARY KEY,
        name         TEXT NOT NULL,
        domain       TEXT NOT NULL UNIQUE,
        type         TEXT NOT NULL,
        status       TEXT NOT NULL DEFAULT 'stopped',
        php_version  TEXT NOT NULL,
        web_server   TEXT NOT NULL DEFAULT 'nginx',
        ssl          INTEGER NOT NULL DEFAULT 1,
        path         TEXT NOT NULL,
        created_at   TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );",
    // v2 — PHP version registry (Phase 2 §1.2): one row per minor series, the
    // pinned patch build, that version's deterministic php-fpm port, whether it's
    // enabled (a pool is started for it), and whether it's the default for new sites.
    "CREATE TABLE php_versions (
        minor      TEXT PRIMARY KEY,
        patch      TEXT NOT NULL,
        fpm_port   INTEGER NOT NULL,
        installed  INTEGER NOT NULL DEFAULT 0,
        is_default INTEGER NOT NULL DEFAULT 0
    );",
    // v3 — WordPress multisite mode (Phase 3 §10.1): `none` (single site),
    // `subdomain`, or `subdirectory`. Maps to the nginx RewriteMode (Phase 1 §6.2).
    "ALTER TABLE sites ADD COLUMN multisite TEXT NOT NULL DEFAULT 'none';",
    // v4 — Site blueprints (Phase 3 §11.3): reusable site presets. `spec` is a JSON
    // `BlueprintSpec` (site type, PHP/server, multisite mode, plugins/themes to
    // install + activate, WP_DEBUG, locale) applied on one-click create. Seeded with
    // two ready-made examples; users add their own.
    "CREATE TABLE blueprints (
        id         TEXT PRIMARY KEY,
        name       TEXT NOT NULL,
        spec       TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    INSERT INTO blueprints (id, name, spec) VALUES
      ('seed-woocommerce', 'WordPress + WooCommerce',
       '{\"siteType\":\"wordpress\",\"phpVersion\":\"8.3\",\"webServer\":\"nginx\",\"multisite\":\"none\",\"plugins\":[{\"slug\":\"woocommerce\",\"activate\":true}],\"themes\":[],\"wpDebug\":false,\"language\":\"\"}'),
      ('seed-multisite', 'WordPress Multisite (subdirectory)',
       '{\"siteType\":\"wordpress\",\"phpVersion\":\"8.3\",\"webServer\":\"nginx\",\"multisite\":\"subdirectory\",\"plugins\":[],\"themes\":[],\"wpDebug\":true,\"language\":\"\"}');",
    // v5 — per-version PHP ini settings (memory_limit etc.), written as
    // `php_value[key]` lines into that minor's php-fpm pool config. One row per
    // (minor, key); absence = PHP's compiled default (our static builds load no
    // php.ini). Keys are whitelisted in `core::php::SETTINGS` — never free-form.
    "CREATE TABLE php_settings (
        minor  TEXT NOT NULL,
        key    TEXT NOT NULL,
        value  TEXT NOT NULL,
        PRIMARY KEY (minor, key)
    );",
    // v6 — stored database name (change-domain prerequisite). Previously the DB
    // name was re-derived from the domain on every operation
    // (`wordpress::db_name_for`), which makes the domain immutable: changing it
    // would silently point every later reset/import/export/drop at a database
    // that doesn't exist. Now the name is derived ONCE (at creation / here for
    // existing sites) and read from the row ever after. The backfill mirrors
    // `db_name_for` exactly: `validate_domain` allows only [a-z0-9.-], so
    // replacing '.' and '-' with '_' covers every non-alphanumeric character.
    "ALTER TABLE sites ADD COLUMN db_name TEXT NOT NULL DEFAULT '';
     UPDATE sites SET db_name = 'wp_' || replace(replace(domain, '.', '_'), '-', '_');",
    // v7 — per-site environment variables (Phase 3 §1.6). Injected per-request as
    // `fastcgi_param` lines in the site's nginx server block (FrankenPHP overrides
    // get config `env` lines + real process env at spawn) — the shared per-version
    // php-fpm pools are untouched. Visible via getenv(), $_SERVER and $_ENV on
    // both servers. Names and values are validated/escaped by `core::site_env`
    // before any config emission. Cascade rides the connection-level
    // `foreign_keys=ON` pragma.
    "CREATE TABLE site_env (
        site_id TEXT NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
        name    TEXT NOT NULL,
        value   TEXT NOT NULL,
        PRIMARY KEY (site_id, name)
    );",
    // v8 — configurable default TLD (v1: default-for-new-sites only). Seed the
    // setting explicitly so the stored default is visible/queryable; the code
    // still falls back to 'test' when the row is absent. OR IGNORE keeps any
    // value a user already stored.
    "INSERT OR IGNORE INTO settings (key, value) VALUES ('default_tld', 'test');",
    // v9 — `.rex` becomes the backbone/default TLD (`.test` is no longer
    // auto-installed; it stays an ordinary choosable TLD). Flip the v8 seed —
    // and any db that already ran v8 — from 'test' to 'rex'. Pre-release, no
    // existing users: an explicit 'test' choice can't exist yet, so the
    // unconditional flip is safe; a custom value ('banana') is untouched. The
    // code fallback moves with `tld::BACKBONE_TLD`.
    "UPDATE settings SET value = 'rex' WHERE key = 'default_tld' AND value = 'test';",
    // v10 — per-site SQL engine (MySQL default, MariaDB optional at create).
    // Existing sites all live in MySQL's datadir, so the backfill is exact.
    "ALTER TABLE sites ADD COLUMN db_engine TEXT NOT NULL DEFAULT 'mysql';",
    // v11 — per-site Xdebug toggle (§8.2): routes the site's .php to the
    // minor's DEBUG pool. Off for every existing site.
    "ALTER TABLE sites ADD COLUMN xdebug INTEGER NOT NULL DEFAULT 0;",
    // v12 — add-from-Git provenance: where a wp-content plugin/theme dir came
    // from (list badge; the seam for future update-pull/watch). One row per
    // (site, kind, dir) — re-adding the same dir replaces the row.
    "CREATE TABLE site_git_assets (
        site_id TEXT NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
        kind TEXT NOT NULL,
        dir_name TEXT NOT NULL,
        url TEXT NOT NULL,
        git_ref TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        PRIMARY KEY (site_id, kind, dir_name)
    );",
];

/// Open the app database at `path`, creating parent dirs and applying migrations.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Open the database at the platform-resolved app-data location.
pub fn open_for_platform(paths: &dyn Paths) -> Result<Connection> {
    let path = paths.app_data_dir()?.join(DB_FILE);
    open(&path)
}

/// Open an in-memory database with migrations applied. Used by unit tests
/// across modules that need a ready schema without touching the filesystem.
#[cfg(test)]
pub(crate) fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

/// Connection-level pragmas applied on every open.
fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Apply any migrations newer than the current `user_version`.
fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 =
        conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (i, stmt) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i64;
        if version > current {
            conn.execute_batch(stmt)?;
            // user_version takes a literal, not a bound parameter.
            conn.pragma_update(None, "user_version", version)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-memory db with migrations applied (no filesystem needed).
    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn migrations_create_tables_and_set_version() {
        let conn = memory_db();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);

        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type='table' AND name IN ('sites','settings')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 2);
    }

    #[test]
    fn site_insert_read_round_trips() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["s1", "Acme", "acme.test", "wordpress", "8.3", "~/Sites/acme"],
        )
        .unwrap();

        let (name, domain, ssl): (String, String, i64) = conn
            .query_row(
                "SELECT name, domain, ssl FROM sites WHERE id = ?1",
                ["s1"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, "Acme");
        assert_eq!(domain, "acme.test");
        assert_eq!(ssl, 1); // schema default
    }

    #[test]
    fn v6_backfills_db_name_for_existing_sites() {
        let conn = Connection::open_in_memory().unwrap();
        // Bring the schema up to v5 only, then insert sites the way they
        // existed BEFORE db_name was stored.
        for (i, stmt) in MIGRATIONS[..5].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        for (id, domain) in [("s1", "blog.test"), ("s2", "my-shop.test"), ("s3", "a.b-c.test")] {
            conn.execute(
                "INSERT INTO sites (id, name, domain, type, php_version, path)
                 VALUES (?1, ?1, ?2, 'wordpress', '8.3', '/tmp')",
                rusqlite::params![id, domain],
            )
            .unwrap();
        }

        // Applying the remaining migrations must backfill exactly what
        // `wordpress::db_name_for` derives for each domain.
        migrate(&conn).unwrap();
        for (id, expect) in
            [("s1", "wp_blog_test"), ("s2", "wp_my_shop_test"), ("s3", "wp_a_b_c_test")]
        {
            let (name, derived): (String, String) = conn
                .query_row("SELECT db_name, domain FROM sites WHERE id = ?1", [id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .map(|(n, d): (String, String)| (n, crate::core::wordpress::db_name_for(&d)))
                .unwrap();
            assert_eq!(name, expect);
            assert_eq!(name, derived, "SQL backfill must mirror db_name_for");
        }
    }

    #[test]
    fn v8_v9_seed_and_flip_default_tld_to_rex() {
        // Fresh DB: v8 seeds 'test', v9 flips it — final state is 'rex'.
        let conn = memory_db();
        let v: String = conn
            .query_row("SELECT value FROM settings WHERE key='default_tld'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "rex");

        // A db that already ran v8 (stored 'test') gets flipped by v9…
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..8].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        migrate(&conn).unwrap();
        let v: String = conn
            .query_row("SELECT value FROM settings WHERE key='default_tld'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "rex", "v9 must flip the v8 'test' seed");

        // …while a custom value survives both the v8 OR IGNORE and the v9 flip.
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..7].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute("INSERT INTO settings (key, value) VALUES ('default_tld', 'banana')", [])
            .unwrap();
        migrate(&conn).unwrap();
        let v: String = conn
            .query_row("SELECT value FROM settings WHERE key='default_tld'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "banana", "a custom value must survive v8+v9");
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = memory_db();
        // Running again must not error or re-run create statements.
        migrate(&conn).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn open_creates_file_and_persists() {
        let dir = std::env::temp_dir().join("rexenv-test-db");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join(DB_FILE);

        {
            let conn = open(&path).unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('theme', 'dark')",
                [],
            )
            .unwrap();
        }
        assert!(path.exists(), "db file should be created");

        // Reopen: migrations skip, data persists.
        let conn = open(&path).unwrap();
        let value: String = conn
            .query_row("SELECT value FROM settings WHERE key='theme'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(value, "dark");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
