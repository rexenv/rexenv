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
    // v13 — asset provenance origin: 'cloned' (added via the Git flow),
    // 'adopted' (manually-cloned checkout the user chose to manage), or
    // 'linked' (symlink into wp-content — deletes must UNLINK, never recurse
    // into the target). Every pre-v13 row came from the clone flow — exact.
    "ALTER TABLE site_git_assets ADD COLUMN source TEXT NOT NULL DEFAULT 'cloned';",
    // v14 — recorded per-site override backend port (B20 §4). Schema-only here
    // (Phase A): a PLAIN NULLABLE INTEGER, NO UNIQUE constraint — uniqueness is
    // enforced by the allocator at write time, and pre-existing collisions are
    // RESOLVED by the Rust backfill (`core::sites::backfill_override_ports`, Phase
    // B, run once at startup), never rejected, so this can't brick on existing
    // data (the B21 lesson). Existing rows are NULL until the backfill records
    // each override site's CURRENT derived port (non-colliding sites unchanged);
    // consumers fall back to the derived port for the between-phases window.
    "ALTER TABLE sites ADD COLUMN override_port INTEGER;",
    // v15 — per-manager installed-dependency fingerprints for the zero-exec
    // "Check deps" feature. PLAIN NULLABLE TEXT, no constraints (the B21
    // lesson: nothing existing rows could violate). Every pre-v15 asset is
    // NULL = "present (unverified)" — NEVER "stale" — so existing users
    // upgrade to a calm panel, not a wall of reinstall alarms. Written only
    // after an install step succeeds through rexenv; values are algorithm-
    // prefixed ("fnv1a:1:<hex>") so a future scheme change reads as foreign
    // → unverified, not false-stale.
    "ALTER TABLE site_git_assets ADD COLUMN composer_installed_fp TEXT;
     ALTER TABLE site_git_assets ADD COLUMN node_installed_fp TEXT;",
    // v16 — provisioning-completeness flag for the streamed site-create job.
    // DEFAULT 1: every EXISTING row reads "provisioned" (the v15 lesson —
    // upgrades must never spray alarms over sites that were fine yesterday).
    // The provision job flips a new row to 0 right after insert and back to 1
    // only when the job settles ok; a 0 row renders the honest "setup
    // incomplete" badge with Retry/Delete instead of masquerading as healthy.
    "ALTER TABLE sites ADD COLUMN provisioned INTEGER NOT NULL DEFAULT 1;",
    // v17 — does rexenv OWN this site's docroot, i.e. may teardown delete it?
    // PLAIN NULLABLE INTEGER, no default, no constraint (the B21 lesson —
    // nothing existing rows could violate).
    //
    // A DEFAULT can't express this, unlike v16's: it would have to GUESS for
    // rows whose docroot was MOVED outside the sites folder — which
    // `move_site_docroot` allows for any destination, and whose confirm dialog
    // PROMISES such a folder is "kept — not deleted". Defaulting those to
    // deletable would destroy files today's code preserves. So the answer is
    // recorded per row by the Rust backfill
    // (`core::sites::backfill_docroot_managed`, run once at startup), which
    // evaluates TODAY's lexical sites-dir test exactly once and freezes it —
    // zero behavior change for every existing row (the v14 override-port
    // pattern). NULL = not yet backfilled: consumers fall back to the legacy
    // lexical test for that window only.
    //
    // This replaces inferring ownership from the path at DELETE time, which
    // read the MUTABLE `sites_dir` setting (stored unvalidated): re-pointing
    // the Sites folder at ~/code silently made an unrelated project's docroot
    // look deletable.
    "ALTER TABLE sites ADD COLUMN docroot_managed INTEGER;",
    // v18 — resolver files we BORROWED from another tool (Valet/Herd).
    //
    // Ownership of `/etc/resolver/<tld>` is content equality, so the moment we
    // take a foreign file over it becomes indistinguishable from one we
    // created — and teardown would then delete it, leaving the user with
    // neither their config nor ours. This row is what teardown consults to
    // RESTORE instead of remove, so it must outlive the takeover; a settings
    // string wouldn't carry the backup path and timestamp, and blueprints
    // already set the precedent that structured state gets a table.
    //
    // `tld` is the PRIMARY KEY, and the backup file is named after it rather
    // than a timestamp, so at most one backup per TLD exists BY CONSTRUCTION —
    // a timestamped name is exactly what would make an orphaned backup
    // representable. The row owns its file: both are written together and
    // deleted together, including when drift drops the row as moot.
    "CREATE TABLE resolver_takeovers (
        tld         TEXT PRIMARY KEY,
        original    TEXT NOT NULL,
        backup_path TEXT NOT NULL,
        taken_at    TEXT NOT NULL DEFAULT (datetime('now'))
    );",
    // v19 — did rexenv CREATE this site's database? (Stage 2, database import.)
    //
    // The third instance of "recorded, not derived" (db_name v6, override_port
    // v14, docroot_managed v17), and for the same reason: importing a Valet/Herd
    // database may restore into a name that ALREADY EXISTED on our engine — a
    // database the user made themselves, or one an earlier import left. Deleting
    // the site must never drop that. Ownership is a fact known only at the moment
    // we look, so it is written down then rather than re-guessed at delete time.
    //
    // NULL     = legacy: created by rexenv's own provisioning (`configure`
    //            phase). Today's teardown behaviour, unchanged — a nullable
    //            column with no DEFAULT touches no existing row.
    // 1        = this import created it. May be dropped.
    // 0        = the name PRE-EXISTED and we restored into it after typed
    //            confirmation. NEVER dropped, by any path.
    //
    // No DEFAULT for the v17 reason: a default would have to invent an answer
    // for rows whose truth we don't know, and the wrong invention here destroys
    // a database the user created.
    "ALTER TABLE sites ADD COLUMN db_created INTEGER;",
    // v20 — the settled outcome of a site's database import (Stage 2 step 9).
    //
    // ONE serialized fact per site: the summary sentence, the Sites badge and
    // the SiteDetail panel all render from this row, so they can never
    // disagree — the failure mode being designed out is a badge and a summary
    // each right by its own arithmetic.
    //
    // `state` is a CLOSED set with one value today: 'imported' — the copy
    // exists on rexenv's engine and the site still reads the OLD database.
    // The 'connected' value does not exist until Stage 3 introduces the fact
    // that proves it, so no UI can render "connected" by inference.
    "CREATE TABLE db_imports (
        site_id       TEXT PRIMARY KEY,
        state         TEXT NOT NULL,
        db_name       TEXT NOT NULL,
        table_count   INTEGER NOT NULL,
        size_bytes    INTEGER NOT NULL,
        source_label  TEXT NOT NULL,
        mirrored_user TEXT,
        imported_at   TEXT NOT NULL DEFAULT (datetime('now'))
    );",
    // v21 — Stage 3, the connection rewrite's two facts.
    //
    // `db_imports.verified` — HOW a 'connected' state was proven: 'signin'
    // (the rewritten settings sign in to the rexenv copy) or 'signin+http'
    // (plus the supplementary HTTP probe). Nullable, no DEFAULT: every
    // existing row is 'imported', for which verification does not exist —
    // NULL is the truth, not a guess. 'connected' joins the closed set here,
    // but its ONLY writer is `store::set_db_import_connected`, which demands
    // a witness type (`ConnectedVerified`) with no production constructor
    // until the rewrite job's verification path exists — the value comes
    // from something we proved, never from "the write succeeded" (plan §6).
    //
    // `config_rewrites` — the whole-file backup taken before the ONE write
    // rexenv ever makes inside a user's project. (site_id, file) is the
    // PRIMARY KEY and rows are INSERTed, never upserted: FIRST BACKUP WINS
    // by construction. A second rewrite overwriting the backup with
    // already-rewritten content would silently turn revert into a lie — the
    // PK makes that unrepresentable rather than merely avoided. The row and
    // its backup file are created together and removed together (the v18
    // resolver-takeover pattern; the sweep reports, never auto-deletes).
    "ALTER TABLE db_imports ADD COLUMN verified TEXT;
     CREATE TABLE config_rewrites (
        site_id     TEXT NOT NULL,
        file        TEXT NOT NULL,
        backup_path TEXT NOT NULL,
        written_at  TEXT NOT NULL DEFAULT (datetime('now')),
        PRIMARY KEY (site_id, file)
     );",
    // v22 — Stage 3 step 5: what the rewrite actually WROTE (sha256 hex of
    // the file content the job renamed into place, refreshed on every
    // successful write). This is what makes "the file changed since the
    // rewrite" a NAMED revert state instead of a vague always-on warning:
    // digest matches the file → clean restore; differs → the conservative
    // FileEdited branch with an explicit their-edits-will-be-lost choice.
    //
    // Nullable, no DEFAULT (the v17/v19/v21 bar). NULL reads as
    // "can't prove the file is unchanged" and lands in the conservative
    // branch — NEVER as "matches". The row is inserted with NULL and the
    // digest is recorded only AFTER the rename succeeds, so every crash
    // window fails toward the safe branch.
    "ALTER TABLE config_rewrites ADD COLUMN written_digest TEXT;",
    // v23 — tunnel lifecycle (ruling 28 Jul 2026: tunnels DIE WITH THE APP).
    // One row per spawned cloudflared, written at spawn — BEFORE the URL poll,
    // so a start the app quits out of mid-poll is still on record — and
    // deleted on clean stop, failed start, and app exit. A row surviving to
    // the next launch therefore means a crash: the startup sweep
    // (core::tunnels::sweep_startup) kills the pid only after positive argv
    // identification and removes the mu-plugin at `docroot` either way.
    // `docroot` is recorded, not looked up at sweep time — a site deleted or
    // renamed between crash and relaunch must still get its file removed.
    "CREATE TABLE tunnels (
        domain     TEXT PRIMARY KEY,
        pid        INTEGER NOT NULL,
        docroot    TEXT NOT NULL,
        started_at TEXT NOT NULL DEFAULT (datetime('now'))
    );",
    // v24 — the WP content dir RELATIVE to the docroot ('wp-content' for stock
    // WordPress; 'app' for Bedrock, 'content' for Radicle — Stage 0 linked
    // layouts where docroot/wp-content does not exist and writing there both
    // pollutes the user's repo and silently does nothing). Recorded ONCE at
    // creation (and by the startup backfill for existing rows) from the same
    // fs markers detection uses; every writer reads the record, never
    // re-derives — historical junk (a bug-written web/wp-content) must not
    // poison the answer at use time. Nullable, no DEFAULT (the v17/v19/v21
    // bar): NULL reads as the WP default 'wp-content' via
    // `Site::content_dir_rel`, never as a guess recorded to disk.
    "ALTER TABLE sites ADD COLUMN content_dir TEXT;",
    // v25 — did REXENV create this site's `mu-plugins/` dir (step 6)? Set to 1
    // the first time a writer (tunnel or login mu-plugin) has to create it;
    // site delete/rename may then remove the dir when it's empty again. NULL =
    // not ours / unknown — never inferred from emptiness: a user's own empty
    // mu-plugins dir is not ours to delete.
    "ALTER TABLE sites ADD COLUMN mu_dir_created INTEGER;",
    // v26 — the MCP agent activity feed (accountability record: what an AI agent
    // did through the MCP server). A TYPED shape, deliberately: `target_site` is
    // the only argument recorded (the site a call named), NOT a free-form arg
    // summary — a later tool's args can't smuggle content into the feed because
    // there is no column to hold it (a per-tool value like a query gets its own
    // typed column when that tool lands). `detail` is rexenv's own bounded reason
    // for a non-ok outcome, never agent-supplied content. Bounded by a row cap +
    // user-clearable (see `mcp_server::feed`).
    "CREATE TABLE agent_actions (\
        id INTEGER PRIMARY KEY AUTOINCREMENT, \
        at TEXT NOT NULL, \
        client TEXT NOT NULL, \
        tool TEXT NOT NULL, \
        target_site TEXT, \
        outcome TEXT NOT NULL, \
        detail TEXT);",
    // v27 — MCP M2a: who a site BELONGS to, and when a disposable one dies.
    //
    // `origin` is the tier boundary itself (PLAN §3.2/§4.1): an agent may only
    // mutate or delete a site recorded as its own. Recorded at insert, NEVER
    // derived — not from the `.scratch.rex` name (a user who hand-creates one
    // owns a normal site the reaper must never touch), not from the path.
    //
    // NOT NULL DEFAULT 'user' is a default that is CORRECT, not merely
    // convenient, and that is why it differs from v17/v19/v24's deliberate
    // nullability. Those columns were nullable because the fact was UNKNOWN for
    // existing rows and a guess written to disk would outlive its excuse. Here
    // the fact is known with certainty: no code path could have written 'agent'
    // before this migration exists, so every pre-v27 row IS the user's. The
    // default records that fact rather than guessing at it — and it also happens
    // to be the conservative direction (a 'user' row is never reaped).
    //
    // `agent_client` is the MCP client's SELF-REPORTED name from `initialize` —
    // the one agent-controlled value on this table. Display-only, length-capped
    // at write, and nothing branches on it (`Site::reap_due` ignores it, pinned
    // by a test): the feed's `client` column discipline, mirrored.
    //
    // `expires_at` is nullable and NULL means NEVER — the shape a user site and
    // a KEPT scratch site share, so the reaper's predicate treats them
    // identically by construction rather than by remembering to. Keep clears it;
    // nothing recomputes an expiry from `created_at`.
    "ALTER TABLE sites ADD COLUMN origin TEXT NOT NULL DEFAULT 'user';\
     ALTER TABLE sites ADD COLUMN agent_client TEXT;\
     ALTER TABLE sites ADD COLUMN expires_at TEXT;",
    // v28 — WHO did the thing this feed row records (MCP M2a).
    //
    // The table's implicit claim is "an AI agent did this", and it was true
    // while the session loop was the only writer. M2a's reaper breaks that: a
    // scratch site expiring and being deleted is the most consequential event in
    // the lifecycle, so it must not be invisible — and it is rexenv's own doing,
    // so recording it unlabelled would make every row's attribution a lie by
    // juxtaposition. One TYPED column keeps both claims true (a deliberate typed
    // addition, never a free-form blob — the v26 discipline).
    //
    // NOT NULL DEFAULT 'agent' is a default that is CORRECT: `feed::record` is
    // called from exactly one place (the MCP session loop), so every pre-v28 row
    // IS an agent's tool call. Same shape as v27's `origin`, same reasoning.
    //
    // The READ fails the other way, though, and deliberately: an unrecognised
    // actor reads as REXENV, not as the agent (`FeedActor::parse_db`).
    // Attributing our own action to an agent is the damaging error in an
    // accountability record — and it is also the right forward-compatible read,
    // since a future actor value ('user', say) is by definition not the agent.
    "ALTER TABLE agent_actions ADD COLUMN actor TEXT NOT NULL DEFAULT 'agent';",
    // v29 — a plugin/theme an agent CLONED into a scratch site (MCP M2a, S1).
    //
    // A new table, so unlike v27/v28 there is no existing-row question: it is
    // born empty by construction. What IS true of existing rows elsewhere: the
    // scratch sites created before this migration have zero package rows, and
    // that reads as "nothing was added to this site" — a real answer, not an
    // unknown, because the tool that adds one did not exist when they were made.
    //
    // `kind` is DERIVED from the source's own header before the clone, never a
    // parameter an agent asserts. `source_path` is RECORDED at add time and
    // never re-derived, so a sync can only ever re-read where the clone came
    // from. `fingerprint` is a stat-only summary of the source tree (max mtime,
    // file count, total bytes) — it detects a change reliably; its absence is a
    // strong hint, not a proof, which is why the copy says "no changes detected
    // since" rather than "unchanged".
    "CREATE TABLE scratch_packages (\
        site_id     TEXT NOT NULL, \
        slug        TEXT NOT NULL, \
        kind        TEXT NOT NULL, \
        source_path TEXT NOT NULL, \
        synced_at   TEXT NOT NULL, \
        fingerprint TEXT NOT NULL, \
        PRIMARY KEY (site_id, slug));",
    // v30 — what a tool call was ABOUT, where the tool's name and its target
    // don't say (MCP M2a follow-up).
    //
    // Only ONE tool needs this, and the reason is a property, not a preference:
    // `wp_run` is the only tool whose action leaves no other record. A create, a
    // delete, an add and a sync all change state the user can go and look at;
    // `wp_run`'s effects are inside the site, and its history exists nowhere but
    // here. Forty rows reading `wp_run · x.scratch.rex · ok` record that a
    // runner ran, not what it did.
    //
    // NULLABLE, and the null is MEANINGFUL: it says "this tool's name and target
    // already describe it", which is true of seven of the eight. It does not
    // mean a summary was wanted and missed.
    //
    // The typed-shape rule (#202) is intact, and this is the part worth reading
    // before touching it. The column is NOT a free-form argument dump: each tool
    // declares its own `summarise` (the `sweep_args` shape), so the feed layer
    // never parses agent JSON generically, and every value is charset-clamped at
    // the WRITE to `[a-z][a-z0-9-]{0,19}` per token. That clamp is the security
    // property, not a tidiness rule — the feed is where a user goes to find out
    // what an agent did, so text an agent chose, rendered there, could otherwise
    // impersonate a client name, rexenv's own `rexenv · automatic` rows, or the
    // `·` separators between them. An audit surface that can be made to lie is
    // worse than one that shows less.
    "ALTER TABLE agent_actions ADD COLUMN args_summary TEXT;",
    // v31 — tables the source could not read, left out of the copy.
    //
    // `table_count` alone makes an incomplete copy indistinguishable from a
    // complete one: 173 tables reads as a whole database unless something says
    // 12 more existed and did not come. The import log says it, but logs are
    // pruned and this record is what the summary, badge and detail panel render
    // from — the missing data outlives the log that announced it.
    //
    // NOT NULL DEFAULT '' rather than nullable: every pre-v31 row was written by
    // a build that ABORTED on an unreadable table, so a copy that settled `ok`
    // back then provably skipped nothing. Empty is the true answer for those
    // rows, not an unknown — this is the rare backfill where the old behaviour
    // makes the default a fact rather than a guess.
    "ALTER TABLE db_imports ADD COLUMN skipped_tables TEXT NOT NULL DEFAULT '';",
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
///
/// Each step's DDL **and** its `user_version` bump commit in ONE transaction, so
/// a crash or error mid-step rolls back cleanly and the step re-runs safely on
/// the next open. Several migrations are non-idempotent (`CREATE TABLE` /
/// `ALTER TABLE ADD COLUMN`): a half-applied step — DDL on disk but the version
/// not bumped — would otherwise re-run into "table already exists" / "duplicate
/// column" and brick the database. SQLite has transactional DDL and
/// `PRAGMA user_version` participates in the enclosing transaction, so the step
/// is all-or-nothing.
fn migrate(conn: &Connection) -> Result<()> {
    migrate_with(conn, MIGRATIONS)
}

/// Migration engine over an explicit list — factored out so tests can drive a
/// deliberately-failing step. See [`migrate`] for the atomicity contract.
fn migrate_with(conn: &Connection, migrations: &[&str]) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (i, stmt) in migrations.iter().enumerate() {
        let version = (i + 1) as i64;
        if version > current {
            // Shared-ref transaction (the connection is behind `&`, not `&mut`) —
            // same pattern as `state::store`. Dropping without `commit` ROLLBACKs,
            // so any `?` below undoes this step entirely.
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(stmt)?;
            // user_version takes a literal, not a bound parameter; this write is
            // transactional and commits atomically with the DDL above.
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::SiteOrigin;

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
    fn v15_existing_assets_read_null_fps_and_round_trip_after_install() {
        // Bring schema to v14, insert an asset the way it existed BEFORE the
        // fp columns, then migrate the rest — the pre-existing row must read
        // (None, None) = "present (unverified)", never a false "stale".
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..14].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES ('s1','A','a.rex','wordpress','8.3','/tmp/a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO site_git_assets (site_id, kind, dir_name, url)
             VALUES ('s1','plugin','my-plugin','https://x')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        use crate::state::store;
        let fps = store::get_git_asset_fps(&conn, "s1", "plugin", "my-plugin").unwrap();
        assert_eq!(fps, (None, None));

        // Install success writes one family without touching the other.
        store::set_git_asset_fp(&conn, "s1", "plugin", "my-plugin", "composer", "fnv1a:1:aa").unwrap();
        let fps = store::get_git_asset_fps(&conn, "s1", "plugin", "my-plugin").unwrap();
        assert_eq!(fps, (Some("fnv1a:1:aa".into()), None));
        store::set_git_asset_fp(&conn, "s1", "plugin", "my-plugin", "node", "fnv1a:1:bb").unwrap();
        let fps = store::get_git_asset_fps(&conn, "s1", "plugin", "my-plugin").unwrap();
        assert_eq!(fps, (Some("fnv1a:1:aa".into()), Some("fnv1a:1:bb".into())));

        // No provenance row → silent no-op write, (None, None) read.
        store::set_git_asset_fp(&conn, "s1", "plugin", "ghost", "node", "fnv1a:1:cc").unwrap();
        assert_eq!(store::get_git_asset_fps(&conn, "s1", "plugin", "ghost").unwrap(), (None, None));

        // Re-add (INSERT OR REPLACE) resets fps — fresh checkout = unverified.
        store::upsert_git_asset(&conn, "s1", "plugin", "my-plugin", "https://x", None, "cloned")
            .unwrap();
        assert_eq!(
            store::get_git_asset_fps(&conn, "s1", "plugin", "my-plugin").unwrap(),
            (None, None)
        );
    }

    #[test]
    fn v16_existing_sites_read_provisioned_and_flag_round_trips() {
        // Bring schema to v15, insert a site the way it existed BEFORE the
        // provisioned column, then migrate — the pre-existing row must read
        // provisioned = true (an upgrade must never badge sites that were
        // fine yesterday as "setup incomplete").
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..15].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES ('s1','A','a.rex','wordpress','8.3','/tmp/a')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        use crate::state::store;
        let site = store::get_site(&conn, "s1").unwrap().unwrap();
        assert!(site.provisioned, "pre-v16 row must migrate as provisioned");

        // The job lifecycle: 0 after insert, 1 at settle-ok.
        assert!(store::set_site_provisioned(&conn, "s1", false).unwrap());
        assert!(!store::get_site(&conn, "s1").unwrap().unwrap().provisioned);
        assert!(store::set_site_provisioned(&conn, "s1", true).unwrap());
        assert!(store::get_site(&conn, "s1").unwrap().unwrap().provisioned);
        // Unknown id → no-op, reported.
        assert!(!store::set_site_provisioned(&conn, "ghost", true).unwrap());
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
    fn v17_leaves_existing_rows_unrecorded_for_the_legacy_fallback() {
        // Bring the schema to v16, insert two sites the pre-v17 way — one under
        // a sites folder, one moved outside it — then migrate. BOTH must read
        // NULL: v17 deliberately has no DEFAULT, because any default would have
        // to guess for the moved-out row, and guessing "deletable" would destroy
        // files today's code preserves. The startup backfill records the answer;
        // until it runs the legacy lexical test is the fallback, i.e. exactly
        // today's behavior.
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..16].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES ('s1','In','in.rex','wordpress','8.3','/Users/x/rexenv/Sites/in.rex')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES ('s2','Out','out.rex','wordpress','8.3','/Users/x/code/out')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        use crate::state::store;
        assert_eq!(store::get_site(&conn, "s1").unwrap().unwrap().docroot_managed, None);
        assert_eq!(store::get_site(&conn, "s2").unwrap().unwrap().docroot_managed, None);

        // The setter round-trips and reports an unknown id, like its siblings.
        assert!(store::set_site_docroot_managed(&conn, "s2", false).unwrap());
        assert_eq!(
            store::get_site(&conn, "s2").unwrap().unwrap().docroot_managed,
            Some(false)
        );
        assert!(!store::set_site_docroot_managed(&conn, "ghost", true).unwrap());
    }

    #[test]
    fn v19_db_created_is_null_for_existing_rows_and_never_climbs_back_to_ours() {
        // Same shape as v17: an existing row must migrate to NULL, because NULL
        // is what preserves today's teardown behaviour. A DEFAULT of either
        // value would invent an answer, and inventing "ours" drops a database
        // the user created.
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..18].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES ('s1','Old','old.rex','wordpress','8.3','/Users/x/rexenv/Sites/old.rex')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        use crate::state::store;
        assert_eq!(store::get_site(&conn, "s1").unwrap().unwrap().db_created, None);

        // NULL → Some(true): an import created it.
        assert!(store::set_site_db_created(&conn, "s1", true).unwrap());
        assert_eq!(store::get_site(&conn, "s1").unwrap().unwrap().db_created, Some(true));

        // Some(true) → Some(false): downgrade toward safety is allowed.
        assert!(store::set_site_db_created(&conn, "s1", false).unwrap());
        assert_eq!(store::get_site(&conn, "s1").unwrap().unwrap().db_created, Some(false));

        // Some(false) → Some(true) is REFUSED in SQL, not by convention: a
        // second import into the same pre-existing name is still not ours to
        // drop, and a caller that forgets must not be able to make it ours.
        assert!(!store::set_site_db_created(&conn, "s1", true).unwrap());
        assert_eq!(store::get_site(&conn, "s1").unwrap().unwrap().db_created, Some(false));

        assert!(!store::set_site_db_created(&conn, "ghost", true).unwrap());
        assert!(!store::set_site_db_name(&conn, "ghost", "whatever").unwrap());
    }

    #[test]
    fn v21_existing_imports_read_imported_and_connected_needs_the_witness() {
        // Build the schema at v20, insert a db_imports row the way Stage 2
        // wrote it (no verified column), migrate — the row must read as it
        // behaved yesterday: state Imported, no verification.
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..20].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES ('s1','Blog','myblog.test','wordpress','8.3','/Users/x/code/myblog')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO db_imports (site_id, state, db_name, table_count, size_bytes, source_label)
             VALUES ('s1','imported','ea',48,25165824,'MySQL 8.0.27 at 127.0.0.1:3306')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        use crate::state::store::{self, ConnectedVerified, DbImportState};
        let rec = store::get_db_import(&conn, "s1").unwrap().unwrap();
        assert_eq!(rec.state, DbImportState::Imported);

        // The ONE connected writer, with the test-only witness (production
        // minting doesn't exist until the rewrite job's verification does).
        store::set_db_import_connected(&conn, "s1", ConnectedVerified::test_signin()).unwrap();
        let rec = store::get_db_import(&conn, "s1").unwrap().unwrap();
        assert_eq!(rec.state, DbImportState::Connected(ConnectedVerified::test_signin()));

        // The HTTP probe upgrades the verification; the record carries HOW.
        store::set_db_import_connected(&conn, "s1", ConnectedVerified::test_signin_http()).unwrap();
        let rec = store::get_db_import(&conn, "s1").unwrap().unwrap();
        assert_eq!(
            rec.state,
            DbImportState::Connected(ConnectedVerified::test_signin_http())
        );

        // A re-import resets to imported and CLEARS the verification — the
        // fresh copy has not been re-verified.
        let back = store::upsert_db_import(
            &conn,
            &store::NewDbImport {
                site_id: "s1".into(),
                db_name: "ea".into(),
                table_count: 50,
                size_bytes: 1,
                source_label: "MySQL 8.0.27 at 127.0.0.1:3306".into(),
                mirrored_user: None,
                skipped_tables: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(back.state, DbImportState::Imported);
        let verified: Option<String> = conn
            .query_row("SELECT verified FROM db_imports WHERE site_id='s1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(verified, None);

        // connected without an import is not a state.
        assert!(store::set_db_import_connected(&conn, "ghost", ConnectedVerified::test_signin())
            .is_err());
    }

    #[test]
    fn v21_db_import_record_serializes_the_exact_ts_contract() {
        // Pins the wire shape the TS union types: an imported record has NO
        // verified key; a connected one carries exactly "signin"/"signin+http".
        // A new field or renamed variant is a conscious decision, not drift.
        let conn = memory_db();
        use crate::state::store::{self, ConnectedVerified};
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES ('s1','Blog','myblog.test','wordpress','8.3','/x')",
            [],
        )
        .unwrap();
        let rec = store::upsert_db_import(
            &conn,
            &store::NewDbImport {
                site_id: "s1".into(),
                db_name: "ea".into(),
                table_count: 1,
                size_bytes: 2,
                source_label: "src".into(),
                mirrored_user: Some("wp".into()),
                skipped_tables: Vec::new(),
            },
        )
        .unwrap();
        let v = serde_json::to_value(&rec).unwrap();
        assert_eq!(v["state"], "imported");
        assert!(v.get("verified").is_none(), "imported must not carry a verified key");

        store::set_db_import_connected(&conn, "s1", ConnectedVerified::test_signin_http()).unwrap();
        let rec = store::get_db_import(&conn, "s1").unwrap().unwrap();
        let v = serde_json::to_value(&rec).unwrap();
        assert_eq!(v["state"], "connected");
        assert_eq!(v["verified"], "signin+http");
        assert_eq!(
            v.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["dbName", "importedAt", "mirroredUser", "siteId", "sizeBytes", "skippedTables", "sourceLabel", "state", "tableCount", "verified"]
                .iter()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn v31_backfills_every_existing_import_to_skipped_nothing() {
        // The upgrade question for a NOT NULL DEFAULT: what do pre-v31 rows say?
        // "Nothing was skipped" — and here that is a FACT, not a guess: the build
        // that wrote those rows aborted the whole dump on the first unreadable
        // table, so a row that settled `imported` could not have skipped one.
        let conn = memory_db();
        use crate::state::store;
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path)
             VALUES ('old','Blog','old.test','wordpress','8.3','/x')",
            [],
        )
        .unwrap();
        // Write the row the pre-v31 way: every column EXCEPT skipped_tables.
        conn.execute(
            "INSERT INTO db_imports (site_id, state, db_name, table_count, size_bytes, source_label, mirrored_user, verified)
             VALUES ('old','imported','old',12,34,'src',NULL,NULL)",
            [],
        )
        .unwrap();
        let rec = store::get_db_import(&conn, "old").unwrap().unwrap();
        assert!(rec.skipped_tables.is_empty(), "a pre-v31 row must read as a complete copy");
        assert_eq!(rec.table_count, 12);
    }

    #[test]
    fn v21_config_rewrites_first_backup_wins_by_primary_key() {
        let conn = memory_db();
        use crate::state::store;

        store::insert_config_rewrite(&conn, "s1", "/p/wp-config.php", "/appdata/b1").unwrap();
        let row = store::get_config_rewrite(&conn, "s1", "/p/wp-config.php").unwrap().unwrap();
        assert_eq!(row.backup_path, "/appdata/b1");
        assert!(!row.written_at.is_empty());

        // FIRST BACKUP WINS is the PK, not a convention: a second insert for
        // the same site+file errors and the original backup path survives.
        assert!(store::insert_config_rewrite(&conn, "s1", "/p/wp-config.php", "/appdata/b2")
            .is_err());
        let row = store::get_config_rewrite(&conn, "s1", "/p/wp-config.php").unwrap().unwrap();
        assert_eq!(row.backup_path, "/appdata/b1");

        // A different file for the same site is its own record.
        store::insert_config_rewrite(&conn, "s1", "/p/.env", "/appdata/b3").unwrap();
        assert_eq!(store::config_rewrites_for_site(&conn, "s1").unwrap().len(), 2);
        assert_eq!(store::list_config_rewrites(&conn).unwrap().len(), 2);

        store::delete_config_rewrite(&conn, "s1", "/p/.env").unwrap();
        assert!(store::get_config_rewrite(&conn, "s1", "/p/.env").unwrap().is_none());
        assert_eq!(store::config_rewrites_for_site(&conn, "s1").unwrap().len(), 1);
    }

    #[test]
    fn v22_existing_rewrites_read_unknown_digest_and_land_conservative() {
        // Build at v21, insert a rewrite row the way it existed before the
        // digest column, migrate: the row must read written_digest = None —
        // which the revert classifier treats as "can't prove unchanged"
        // (the FileEdited/force branch), NEVER as a match.
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..21].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute(
            "INSERT INTO config_rewrites (site_id, file, backup_path)
             VALUES ('s1','/p/wp-config.php','/appdata/b1')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        use crate::core::confrewrite::{classify_revert, FileEditedReason, RevertCheck};
        use crate::state::store;
        let row = store::get_config_rewrite(&conn, "s1", "/p/wp-config.php").unwrap().unwrap();
        assert_eq!(row.written_digest, None);
        assert_eq!(
            classify_revert(Some("what we wrote"), Some("the original"), row.written_digest.as_deref()),
            RevertCheck::FileEdited { reason: FileEditedReason::UnknownDigest }
        );

        // The digest is recorded only after a successful rename; the update
        // round-trips.
        store::set_config_rewrite_digest(&conn, "s1", "/p/wp-config.php", "abc123").unwrap();
        let row = store::get_config_rewrite(&conn, "s1", "/p/wp-config.php").unwrap().unwrap();
        assert_eq!(row.written_digest.as_deref(), Some("abc123"));
    }

    #[test]
    fn v23_tunnel_records_roundtrip_replace_and_clear() {
        // Upgrade path: a v22 database gains the tunnels table.
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..22].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        migrate(&conn).unwrap();

        use crate::state::store;
        // The claim is atomic and exclusive: exactly one concurrent start can
        // hold a domain's row (the step-4 in-flight guard).
        assert!(store::try_claim_tunnel(&conn, "a.rex", u32::MAX, "/sites/a").unwrap());
        assert!(!store::try_claim_tunnel(&conn, "a.rex", u32::MAX, "/sites/a").unwrap());
        assert!(store::try_claim_tunnel(&conn, "b.rex", 222, "/sites/b").unwrap());

        // The spawned child's real pid lands on the existing claim; a claim
        // that was revoked mid-start reads false, never a silent no-op.
        assert!(store::set_tunnel_pid(&conn, "a.rex", 333).unwrap());
        assert!(!store::set_tunnel_pid(&conn, "revoked.rex", 1).unwrap());
        let rows = store::list_tunnels(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].domain.as_str(), rows[0].pid, rows[0].docroot.as_str()),
                   ("a.rex", 333, "/sites/a"));

        // A committed docroot move re-points the record (audit A2).
        assert!(store::set_tunnel_docroot(&conn, "a.rex", "/moved/a").unwrap());
        assert!(!store::set_tunnel_docroot(&conn, "ghost.rex", "/x").unwrap());
        assert_eq!(store::get_tunnel(&conn, "a.rex").unwrap().unwrap().docroot, "/moved/a");

        // Releasing the claim reopens the slot (stop-then-start).
        assert!(store::delete_tunnel(&conn, "a.rex").unwrap());
        assert!(!store::delete_tunnel(&conn, "a.rex").unwrap()); // already gone
        assert!(store::try_claim_tunnel(&conn, "a.rex", 444, "/sites/a").unwrap());
        store::clear_tunnels(&conn).unwrap();
        assert!(store::list_tunnels(&conn).unwrap().is_empty());
    }

    #[test]
    fn v27_every_existing_site_migrates_to_the_users_own_and_never_expires() {
        // The upgrade path IS the assertion for v27 (the v17/v19/v24 shape:
        // build at the prior version, insert the OLD way, migrate, read back).
        //
        // Unlike those columns, v27's `origin` is NOT NULL DEFAULT 'user' — and
        // that is a default which is CORRECT rather than convenient. v17/v19/v24
        // were nullable because the fact was genuinely unknown for existing rows
        // and a guess on disk outlives its excuse. Here nothing could have
        // written 'agent' before this migration existed, so every pre-v27 row IS
        // the user's; the default records a known fact. It is also the
        // conservative direction — this test proves an untouched site cannot
        // come out of the upgrade looking reapable.
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..26].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        // A row inserted the pre-v27 way, in production shape (a real UUID id,
        // an absolute app-data docroot) — a friendlier fixture would hide a
        // column-order slip in `SITE_COLUMNS`.
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path, docroot_managed)
             VALUES ('7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30','Shop','shop.rex','wordpress','8.3',
                     '/Users/x/Library/Application Support/dev.rexenv.rexenv/Sites/shop.rex', 1)",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        use crate::state::store;
        let site = store::get_site(&conn, "7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30").unwrap().unwrap();
        assert_eq!(site.origin, SiteOrigin::User, "an existing site is the USER's");
        assert!(!site.is_scratch());
        assert_eq!(site.expires_at, None, "no expiry — NULL means never");
        assert_eq!(site.agent_client, None);
        // The point of all three: the reaper cannot touch it, at any clock.
        assert!(!site.reap_due("2099-01-01 00:00:00"));
    }

    #[test]
    fn v28_every_existing_feed_row_migrates_to_the_agent_that_wrote_it() {
        // Same upgrade discipline as v27, and the same known-vs-unknown test:
        // `feed::record` is called from exactly ONE place (the MCP session
        // loop), so every pre-v28 row IS an agent's tool call. `DEFAULT 'agent'`
        // records that fact rather than guessing — and unlike v27's, this
        // default is NOT the conservative direction, which is precisely why the
        // READ fails the other way (see the sibling test in `mcp_server::feed`).
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..27].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute(
            "INSERT INTO agent_actions (at, client, tool, target_site, outcome, detail) \
             VALUES (datetime('now'), 'Claude Code', 'tail_log', \
                     '7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30', 'ok', NULL)",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        use crate::mcp_server::feed::{self, FeedActor};
        let rows = feed::recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].actor, FeedActor::Agent, "an existing feed row is the AGENT's call");
        assert_eq!(rows[0].client, "Claude Code");
        // And it still drives the status line, exactly as it did before v28.
        assert!(feed::recent_head(&conn, 15).unwrap().is_some());
    }

    #[test]
    fn v30_leaves_every_existing_row_saying_nothing_rather_than_guessing() {
        // The upgrade question for a nullable column: what do pre-v30 rows say?
        // NULL, and that reads as "this tool's name and target already describe
        // it" — which is TRUE of every row written before v30, because the only
        // tool the column exists for is the one whose calls it will now describe
        // going forward. A backfilled guess would have been a fabricated record
        // in an accountability table.
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..MIGRATIONS.len() - 1].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        conn.execute(
            "INSERT INTO agent_actions (at, actor, client, tool, target_site, outcome, detail) \
             VALUES (datetime('now'), 'agent', 'Claude Code', 'wp_run', 's1', 'ok', NULL)",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();

        let rows = crate::mcp_server::feed::recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].args_summary, None,
            "a pre-v30 row must say nothing, not a guessed summary"
        );
        assert_eq!(rows[0].tool, "wp_run", "and everything else about it is untouched");
    }

    #[test]
    fn v29_is_born_empty_and_an_existing_scratch_site_has_no_packages() {
        // A CREATE TABLE, so there is no existing-row migration question — but
        // the adjacent fact IS worth proving: scratch sites created before v29
        // (scratch_create_site shipped first) come out with zero package rows,
        // and that is a real answer — "nothing was added" — not an unknown. The
        // tool that adds one did not exist when they were made.
        use crate::state::models::SiteOrigin;
        let conn = Connection::open_in_memory().unwrap();
        for (i, stmt) in MIGRATIONS[..28].iter().enumerate() {
            conn.execute_batch(stmt).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        let mut scratch = crate::state::models::test_site(
            "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24",
            "probe.scratch.rex",
            SiteOrigin::Agent,
        );
        scratch.expires_at = Some("2099-01-01 00:00:00".into());
        crate::state::store::insert_site(&conn, &scratch).unwrap();
        migrate(&conn).unwrap();

        let packages = crate::state::store::scratch_packages(&conn, &scratch.id).unwrap();
        assert!(packages.is_empty(), "an existing scratch site has no packages — and that is an answer");
        // The site itself is untouched by the migration.
        let after = crate::state::store::get_site(&conn, &scratch.id).unwrap().unwrap();
        assert!(after.is_scratch() && after.expires_at.is_some());
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
    fn a_failing_step_rolls_back_atomically_and_reruns_clean_not_bricked() {
        // Models a crash BETWEEN a migration's DDL and its user_version bump: a
        // step that errors partway must leave user_version at the PRIOR value
        // with NONE of its DDL applied, so the next open re-runs the step from
        // scratch instead of hitting "table already exists" on a half-applied
        // schema (the brick this fix prevents). A mid-step failure takes the
        // exact same atomic-rollback path a crash would (an uncommitted txn is
        // rolled back).
        let conn = Connection::open_in_memory().unwrap();
        let has = |t: &str| -> bool {
            conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [t],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
                > 0
        };

        // Step 1 is valid; step 2 creates a table THEN fails by re-creating the
        // step-1 table — the very "already exists" class that would brick.
        let broken: &[&str] = &[
            "CREATE TABLE a (x INTEGER);",
            "CREATE TABLE b (y INTEGER); CREATE TABLE a (dup INTEGER);",
        ];
        assert!(migrate_with(&conn, broken).is_err(), "the broken step must surface an error");

        // Step 1 committed; step 2 rolled back ENTIRELY (no table b) and the
        // version stayed at 1 — the DB is consistent, not half-migrated.
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(version, 1, "only the fully-applied step bumped the version");
        assert!(has("a"), "step 1's table survived (its own committed txn)");
        assert!(!has("b"), "step 2's partial DDL rolled back with the failed step");

        // Recovery: re-running with a FIXED step 2 re-applies it from scratch
        // (table b never persisted) and reaches version 2 — no manual surgery,
        // no brick.
        let fixed: &[&str] = &["CREATE TABLE a (x INTEGER);", "CREATE TABLE b (y INTEGER);"];
        migrate_with(&conn, fixed).unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(version, 2, "the corrected step re-ran cleanly — DB not bricked");
        assert!(has("b"), "table b exists after the successful re-run");
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
