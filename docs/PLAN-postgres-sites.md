# PostgreSQL as a site database — Laravel and Blank PHP, never WordPress

**Status: step (a) SHIPPED 9 Sep 2026 (ledger #543, #544; L1 `postgres_site_db_check`
green against real PostgreSQL 18.6.0). Steps (b)-(d) open.** Planned against `40a30aa`.

Today a site's database engine is `mysql | mariadb` and nothing else. PostgreSQL
ships, starts, has a version picker and an Adminer button — as a *standalone*
engine on the Databases page. No site can be backed by it: `DbEngine::from_site`
(`core/db.rs:229`) matches two variants, and `sql_client_bins` answers a third with
`"PostgreSQL does not host site databases"`.

That is right for WordPress — core speaks MySQL and nothing else — and wrong for the
two site types rexenv creates that have no such constraint: **Laravel** (`pgsql` is a
first-class Laravel driver; `core/dbimport.rs:504` already *reads* `DB_CONNECTION=pgsql`
out of an imported project's `.env`, so rexenv can import a Postgres Laravel site it
cannot create) and **Blank PHP** (a PDO handle; the driver is a DSN prefix).

Goal: on the New-site dialog, a Laravel or Blank PHP site can pick **PostgreSQL** in the
Database field and get the same finished thing MySQL gives it — a created database,
credentials written into the file the framework reads, the engine downloaded and running,
Adminer opening on it, size on the Sites page, export/import/drop on the Database tab.

---

## 1. What "not supported" is actually made of

Nine places, not one. Each was read, not guessed:

| Layer | Where | What it assumes |
|---|---|---|
| Model | `state/models.rs:64` `str_enum!(SiteDbEngine)` | two variants, doc says "Both speak the MySQL protocol" |
| Dispatch | `core/db.rs:229` `from_site`, `:240` `sql_client_bins`, `:263` `cached_sql_client`, `:277` `datadir_initialized` | site engines are MySQL/MariaDB; the last two answer `None`/`false` for everything else |
| Ops | `core/database.rs` — `create_database`, `drop_database`, `import_from_file`, `export_to_downloads`, `db_sizes`, `mysql_exec`, `client_base_args` | one wire protocol, one flag set, backtick identifiers, `information_schema` |
| Provision | `commands/site_provision.rs:1321,1513,1762` | calls `database::create_database` by name |
| Laravel | `core/laravel.rs` + `core/confedit.rs` `.env` rewrite | `DB_CONNECTION=mysql`, port 13306, user `root` |
| Starter | `core/starter.rs:107` `seed`, `seed_sql` | `USE \`db\`;`, `AUTO_INCREMENT`, `ENGINE=InnoDB`, backticks |
| Lifecycle | `core/downloads.rs:291`, `core/service_manager.rs:669` | "does any site use MariaDB?" gates the download and the start; Postgres is `required() == false`, so Start-all never starts it |
| Status | `commands/sites.rs:170` | `[Mysql, Mariadb]` is the literal list of engines queried for sizes |
| UI | `src/types/index.ts:118`, `NewSiteDialog.tsx:922` | union of two; two hardcoded `<option>`s, no site-type gate |

The 400-page version of this is: **the site-DB layer is not engine-agnostic, it is
MySQL with a MariaDB alias.** MariaDB was free — same protocol, same flags, same SQL.
PostgreSQL is the first engine that costs something, and the cost is exactly the list
above.

## 2. Shape: engine-dispatched site-DB ops, not a second code path

`DbEngine` already dispatches `start`, `data_dir`, `series_of`, `server_binary`. The
site-DB ops are the one family that stayed free functions in `core::database` and got
called by name from eight places. They become **methods on `DbEngine`**, delegating to
`core::database` (MySQL/MariaDB, unchanged bytes) or `core::postgres` (new):

```
engine.create_database(&client, name)      engine.drop_database(&client, name)
engine.import_from_file(&client, name, f)  engine.export_to_downloads(&dump, domain, name)
engine.db_sizes(&client)
```

The port stays a PARAMETER. Folding it in as `self.port()` was this plan's first
version and was wrong: the live examples run fixture servers on private ports
(13396-13399) precisely so a check can never touch the owner's real databases, and
`self.port()` would have quietly pointed every one of them at the production MySQL.
What the sweep did find is `examples/site_provision_check.rs:290` passing
`database::MYSQL_PORT` literally for whatever engine the site had — a bug waiting for
the first MariaDB site, now `DbEngine::Mysql`'s own constant at a MySQL-only fixture.

**The client carries its engine.** `SqlClient` (ledger #329) is today a `PathBuf`
newtype meaning "a MySQL-protocol interactive client". A `psql` at that path would be
accepted by `mysql_exec` and produce a wrong, confusing failure. So `SqlClient` gains an
`engine: DbEngine` field, set by its two constructors, and the ops dispatch on **the
client's** engine, not the caller's argument — handing `psql` to a MySQL op is then not
a runtime mystery but an immediate, named error, and the mismatch has one place to be
caught instead of eight. #329's lesson (a type sees every caller; a grep sees its
pattern) extended one field further.

## 3. What PostgreSQL needs that MySQL did not

Measured against the bundled tree (`bin/psql`, `bin/pg_dump` both present in
`postgres-*/bin`), not assumed:

- **No `CREATE DATABASE IF NOT EXISTS`.** PG has no such form. Create is a two-step:
  `SELECT 1 FROM pg_database WHERE datname = $1`, then create. (Swallowing SQLSTATE
  42P04 would also work and is worse — it hides every *other* create failure.)
- **`DROP DATABASE IF EXISTS "x" WITH (FORCE)`** — PG 13+, and all three pinned majors
  (16/17/18) have it. Without FORCE a single open Adminer tab makes site deletion fail.
- **`psql` exits 0 on SQL errors** unless `-v ON_ERROR_STOP=1`. An import that silently
  "succeeds" while every statement failed is the worst bug in this whole feature; the
  flag is mandatory on every `psql` invocation, and gets its own test.
- **Connect bound is an env var, not a flag** (`PGCONNECT_TIMEOUT=10`), which sidesteps
  the B25/`--connect-timeout` split that forced `export_to_downloads` to keep an
  unbounded connect for the MySQL dump tools: libpq honours the env in `psql` *and*
  `pg_dump`, so PG gets the bound MySQL export could not have.
- **Identifiers quote with `"`**, not backticks. `validate_db_name` (alnum + `_`) is
  already the strict backstop and needs no change.
- **Sizes**: `SELECT datname, pg_database_size(datname) FROM pg_database WHERE NOT
  datistemplate` with `-tA -F'\t'` — same `(name, bytes)` shape as the
  `information_schema` query, so `commands/sites.rs` learns a third engine, not a
  second shape.
- **`db.php` / `.env` credentials differ**: user `postgres` (not `root`), port 15432,
  driver `pgsql`.

## 4. Steps — one commit each, verified between

**(a) Engine-aware site-DB layer + the PostgreSQL ops — SHIPPED 9 Sep 2026.** `SqlClient` carries its
engine; the five ops become `DbEngine` methods; `core::postgres` gains
`pg_dump_bin`, `psql_base_args`, `psql_exec`, `create_database`, `drop_database`,
`import_from_file`, `export_to_downloads`, `db_sizes`; `sql_client_bins`,
`cached_sql_client`, `datadir_initialized` and `from_site` answer for Postgres; all
call sites (8 in `src/`, ~10 in `examples/`) move to the methods. `SiteDbEngine`
gains `Postgres` — the model is a wire/DB value and adding it here does NOT expose it
in the UI (step d does), but it is what lets the ops be written and tested against a
real site row. **No behaviour change for existing sites** — that is the bar for (a).
L1: `examples/postgres_site_db_check.rs` (service tier) — create → seed a table →
size → export → drop, against the real server and the real `psql`/`pg_dump`.

**(b) Laravel.** `DB_CONNECTION=pgsql`, port, user in the generated and rewritten
`.env`; `core/laravel.rs` migrations against PG; `confedit` rewrite paths.

**(c) Blank PHP starter.** PG dialect for `seed_sql` (`SERIAL`/`GENERATED`, no
`ENGINE=`, no `USE`), `db.php` DSN, and the `index.php` prose that names the user.

**(d) UI + lifecycle gating.** Site-type-gated option in `NewSiteDialog`, TS union,
backend refusal for WordPress × Postgres (CLI/MCP reach the same payload — the UI is
not the guard), Postgres added to the download and start-all gates when a site uses it,
per-site Adminer link, sizes list.

## 5. The invariant this feature adds

**WordPress never gets PostgreSQL, and the refusal lives in core, not in the dialog.**
WP core's `wpdb` is mysqli/PDO-MySQL only; a Postgres-backed WP site is a broken site,
not a limited one. `sites::prepare` (the one path all three of UI, `rex` CLI and the MCP
tool funnel through) refuses the pair by name. **The row landed with step (a), not (d)** (#543):
adding the enum variant made the pair REACHABLE from `rex site create --db postgres`
and the MCP `create_site` tool in the same commit that made it representable, and
"unreachable today because no screen offers it" is exactly how the last two cross-site
exposures started. The guard sits in `sites::create_recording_ownership`, the one
function every site insert passes through.
