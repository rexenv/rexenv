# Stage 2 — import a Valet/Herd site's database

**Status:** PLAN — awaiting decisions (§10). Stages 0 and 1 shipped
(`PLAN-linked-sites.md`, `PLAN-valet-herd-import.md`); the research this rests on is
`PLAN-valet-herd-migration.md` §5–§7.

**Scope:** copy a site's database out of the environment they already have and into
ours, and mirror the credentials so their own connection details keep working. The
connection-config rewrite is **Stage 3 and explicitly out of scope here** — §9 states
exactly what the user is left holding until it exists.

**The source environment stays STRICTLY READ-ONLY.** We dump (a read). We never drop,
alter, clean up, or start/stop their server, and never touch their config, certs or
project files. Two sub-rules earn their own lines because the code can violate them
without looking like it does:

- `mysqldump --single-transaction --skip-lock-tables` — the default dump takes read
  locks on their live database. Non-locking is not a nicety; a locking dump is a write
  to their server's behaviour.
- Their server may be **stopped**. That is an honest per-site status, never a reason to
  start it. See §2.

---

## 1. The two halves

Each half is verifiable on its own, which is the point of the split.

**Half A — "dump to artifact"** (their side only). Discover their engines, map the site
to a database, judge compatibility, preflight the connection, dump to one file under our
app-data. Nothing of ours is created; nothing of theirs is written. Half A is done when
a `.sql` artifact exists with a manifest describing what it is.

**Half B — "restore into ours"** (+ credential mirroring). Collision check, provenance
record, `CREATE DATABASE`, restore, mirror the site's DB user, verify, report. Half B
takes an artifact + manifest as its input, so it is testable from a fixture file with no
source environment present at all.

That seam is deliberate: every failure mode in B can be reproduced from a checked-in
`.sql` file, and A can be exercised against a real source without ever creating anything.

---

## 2. Discovery and mapping

### 2.1 Live listeners are the authority

`PLAN-valet-herd-migration.md` §5 recorded the trap: DBngin's `DBEngines.plist` says
`Status = "started"` for an engine that is **stopped**. So the plist (and Herd's service
config) is used for **labels only** — engine family, version string, datadir, port — and
never for "is it running".

Candidate ports come from the union of four sources, best-effort, any of which may be
missing or malformed:

1. **the site's own config** — `DB_HOST`/`DB_PORT` is the single most reliable pointer,
   because it is what the site actually uses;
2. DBngin `~/Library/Application Support/com.tinyapp.DBngin/Data/DBEngines.plist`;
3. Herd's `config/services`;
4. well-known defaults (3306, 3307, 5432, 8889).

**No `brew services`.** It needs `brew` on `PATH`, and a Finder-launched app has the bare
launchd `PATH` with no Homebrew — the same lesson that forced our own bundled clients
(`core/database.rs::create_database`). Probing `/opt/homebrew/bin/brew` directly would
work but costs seconds per scan and adds nothing: a brew-run MySQL is listening on a port
we already probe. A brew-managed server therefore shows up as an unlabelled engine on its
port, which is honest and sufficient.

Each candidate is then verified live, and identified **without authenticating**:

> **The MySQL/MariaDB handshake carries the server version before login.** The server
> speaks first — packet header, protocol byte `10`, then a NUL-terminated human-readable
> version (`8.0.27`, `11.4.12-MariaDB`). Reading it needs a TCP connect and one read: no
> credentials, no client binary, and therefore no exposure to the client-pairing trap
> (§3) at the identification step.

*Verification status: reasoned from the protocol, NOT yet byte-verified against DBngin
8.0.27. First task in Half A is to prove it live; if it doesn't hold, fall back to the
paired interactive client and `SELECT VERSION()`.*

Postgres has no pre-auth greeting; identify it with `psql -c "SHOW server_version"` plus
`PGCONNECT_TIMEOUT`.

### 2.2 A stopped source engine

Per-site status, verbatim shape:

> **database not reachable** — `ea` is on MySQL at 127.0.0.1:3306, which isn't running.
> Start it in DBngin, then re-scan.

We name the app when we can identify it (DBngin/Herd/unknown), and we do not offer a
"start it for me" button. Rescan is cheap and explicit.

### 2.3 Per-site mapping — from the SITE's config, never guessed

- **WordPress** — extend the existing static define reader
  (`core/logs.rs::define_value`, already used for `WP_DEBUG`) to `DB_NAME`, `DB_USER`,
  `DB_PASSWORD`, `DB_HOST` (`host:port` and `host` forms) and `$table_prefix`. It is a
  text scan; **wp-config.php is never executed**. Lift it into `core::dbimport` (or a
  shared `core::phpconf`) so there is one parser, not two.
- **Laravel/`.env`** — a new conservative line parser: `DB_CONNECTION`, `DB_HOST`,
  `DB_PORT`, `DB_DATABASE`, `DB_USERNAME`, `DB_PASSWORD`. Tolerates quotes, `export `
  prefixes, comments and CRLF. **Refuses** on a duplicate key or a multi-line value →
  needs attention.
- **Non-literal / env-driven defines** (Bedrock: `define('DB_NAME', env('DB_NAME'))`) →
  follow the `.env` path if one exists, otherwise **needs attention**. Never a guess,
  never a default.
- **Anything else** → needs attention, with manual entry as the escape hatch: given
  host/port/user/password the user types, we can list databases and let them pick.

Every "needs attention" row states what was ambiguous, so it reads as a finding rather
than a shrug.

---

## 3. Secrets — the handling, stated

Reading wp-config/.env means reading secrets. The rules, in force for both halves:

- **In memory for the job's duration only.** Never written to SQLite; the site row keeps
  the database *name* and engine, nothing else. On Retry we **re-read their file** — the
  source of truth stays theirs, which is also why nothing needs to be cached.
- **Never on argv.** Credentials reach the MySQL-family tools exclusively through a
  `--defaults-extra-file` **born 0600** via `PermissionManager::write_private`
  (`platform/traits.rs:261` — created with the mode, never write-then-chmod; the B6
  rule), passed **as the first option** (verified fatal otherwise on both vendors), and
  deleted in a `Drop` guard so it goes away on every exit path including panic. Postgres
  uses `PGPASSFILE` (libpq *enforces* 0600 — a 0644 file is ignored with a warning).
  `MYSQL_PWD` is deprecated in 8.4 and is not used.
- **Never where logs look.** The job log streams child stderr and our own phase lines.
  There is **no redaction layer today, and this stage does not add one** — the rule is
  that secrets never enter the stream in the first place. Concretely: argv is never
  logged (nothing in the job logs argv today, and the dump/restore invocations must not
  start); the tools' own stderr cannot contain the password because it never reached
  them via argv or env; and dump *content* is never streamed — it goes to
  `--result-file` on the way out and over stdin on the way in.
- **The artifact is 0600 too.** Pre-create the empty result file with `write_private`,
  then let `--result-file` write into it: `O_TRUNC` on an existing path preserves the
  inode's mode. *Verify in Half A that the tool truncates rather than unlink+recreates;
  if it recreates, chmod immediately after and accept the narrow window, or dump to a
  0600 file via stdout redirection instead.*

**Client pairing is a hard invariant, not a preference.** Our bundled MariaDB clients
**cannot authenticate to MySQL 8 at all** — `caching_sha2_password` is a dynamic plugin
our bottle bundle excludes (`ERROR 1156 (08S01)`, verified). MySQL-branded tools for
MySQL sources; MariaDB tools for MariaDB sources. The dump tool is chosen from the
**source's** identified family, the restore tool from the **target's**.

---

## 4. The compatibility gate

A pure function — no live engine needed, therefore unit-tested exhaustively:

```rust
pub enum Verdict { Ok, Warn(String), Refuse(String) }
pub fn compat(src: Family, src_ver: &Version, dst: Family, dst_ver: &Version) -> Verdict
```

| Source → our target | Verdict |
|---|---|
| MySQL 5.7 / 8.0 → our MySQL 8.0.44 | Ok (dump needs `--column-statistics=0`) |
| MySQL 8.0 → our MySQL 8.4.6 | Ok + artifact scan |
| MySQL 9.x → anything we bundle | Refuse (override) |
| MariaDB 10.x/11.x → our 11.4.12 / 12.3.2 | Ok |
| MySQL ↔ MariaDB, either direction | Refuse by default (override) — unnecessary, we bundle both |
| any source series newer than our target | Refuse (override) |
| Postgres | see §10 D3 |

The verdict is computed per site and **shown before anything runs**, next to the row —
not surfaced as a failure halfway through.

**A finding that shapes the UI:** the target engine version is a **global setting**
(`effective_db_version`), not per site. So a MySQL 5.7 source may produce
"Refuse/Warn against your MySQL 8.4.6 — rexenv also ships 8.0.44, which is a closer
match." Switching is a **global** action with its own consequence (each series keeps its
own datadir, so databases created on 8.4.6 are not visible on 8.0.44). We therefore
*offer* it as an explicit user action with that consequence stated, and never switch
silently.

Dump hygiene, carried from the research (all verified against the bundled binaries):
`--set-gtid-purged=OFF`; single-database dumps (never the `mysql` schema);
`--column-statistics=0` for 5.7 sources; scan line 1 for the **breaking** May-2024
MariaDB sandbox form `/*!999999\- ` and strip only that (the current `/*M!999999\- `
form is a plain comment to MySQL and is left alone); grep the artifact for
`mysql_native_password`, `NO_AUTO_CREATE_USER` and `DEFINER=` and report findings per
site as warnings, not blocks (we restore as root; orphan definers become use-time
warnings, and no dump tool has a `--skip-definer`).

---

## 5. Mechanics

**Preflight, then dump.** `--connect-timeout` is accepted **nowhere** by the dump tools
(argv, `[client]`, `[mysqldump]` — all hard-error `unknown variable`, exit 7, verified on
all four bundled dump binaries). So the connection is bounded by a preflight with the
**interactive** client (`--connect-timeout=10`, today's `client_base_args` shape), which
also fetches the size, and the dump itself runs with an **unbounded transfer**. B25's
rule holds: bound the connect, never the transfer. A 20-minute dump of a large database
is a success, not a hang.

**Disk preflight.** The same preflight query returns
`SUM(data_length + index_length)` for the database. Compare against free space on the
app-data volume and refuse honestly *before* dumping — "needs ~2.4 GB, 0.9 GB free" beats
filling the disk and failing at 98%. (A dump is usually smaller than the on-disk size,
so this is conservative; say so in the message.)

**Progress — real signal only.** Reuse the job machinery wholesale (registry + seq,
`repo::CancelToken`, `<ns>://state|output/<id>` snapshots, per-scope busy refusal, pruned
per-job logs, outer timer recording `timed_out` before cancelling, and the honest-progress
contract: monotonic, ≤99 until settle, frozen on failure).

- *dump* → byte growth of the `--result-file` output, polled. Genuine bytes, same shape
  as the download hub's fraction. Where the preflight gave a size, it is a fraction of an
  estimate and the UI says "≈"; where it didn't, it is a byte count with no percentage
  rather than a fabricated one.
- *restore* → we own the pipe, so we count the bytes **we feed** to the client's stdin.
  That is a real measurement of our own writing, and it must not be mistaken for
  completion: the client is still executing SQL after the last byte. The feed phase
  therefore tops out well below 100 and the job settles only on process exit.
- Waiting is a ticker that says what it is waiting for. Never a fabricated bar.

**Collisions.** Generalize B21: check the intended name against **both**
`store::db_name_exists` (site-owned) and a live `SHOW DATABASES` on our engine. On
collision, `unique_db_name`'s disambiguated form. **Never overwrite an existing database
without typed confirmation** — and see §6, where "existing" also decides whether we may
ever drop it.

---

## 6. Provenance — recorded, not derived

The third instance of the pattern (`db_name` v6, `override_port` v14, `docroot_managed`
v17): **migration v19 adds `sites.db_created INTEGER` (nullable, no DEFAULT).**

| value | meaning | teardown |
|---|---|---|
| `NULL` | legacy — the database was created by rexenv's own provisioning | drops it (today's behaviour, unchanged) |
| `1` | this import created it | may drop it |
| `0` | the name **pre-existed**; we restored into it after typed confirmation | **never dropped, by any path** |

No DEFAULT, for the same reason v17 had none: a default would invent an answer for rows
whose truth we don't know.

Ordering, so a crash can't lose the fact:

1. `SHOW DATABASES` → if the name exists, `db_created = 0` **before** anything else, and
   restoring into it requires typed confirmation.
2. If absent: write `db_created = 1` **before** `CREATE DATABASE`. A crash between the
   write and the create leaves a claim on a database that doesn't exist — harmless, since
   cleanup is `DROP DATABASE IF EXISTS`. The reverse order is the one that loses data.
3. The restored name is written to `sites.db_name` (recorded, not derived) once the
   restore settles.

**Failed restore cleans up only what this run created:** drop iff `db_created = 1`. A
pre-existing database is left exactly as the failure left it, and the report says so
rather than pretending it was rolled back.

### Credential mirroring, and the `root` trap

After restoring as root: `CREATE USER 'their_user'@'localhost' IDENTIFIED BY <pass>` +
`GRANT ALL ON <db>.*`, fed over **stdin SQL, never argv**. Their existing credentials then
work against our engine and only the host/port differ — which is what makes Stage 3 a
one-key change.

**Except when their user is `root`, which is the common case on this machine** (the live
`ea` sample is `DB_USER=root` with a password). Our engines run passwordless
`root@localhost`; that is baked into `client_base_args` and every path that uses it.

> **Hard rule: never create, alter, or set a password on `root`** (or `mysql.sys`,
> `mysql.session`, `mysql.infoschema`, `postgres`). Setting a password on our root to
> mirror theirs would break every rexenv database operation on the machine — Adminer, the
> DB-size query, create/drop, WP-CLI. It is not a trade-off; it's a self-inflicted outage.

So for `root`-owned source configs, mirroring is impossible and the honest consequence is
that Stage 3's rewrite for those sites touches three keys (`DB_HOST`, `DB_USER`,
`DB_PASSWORD` → root / empty) rather than one. The interim message (§9) says exactly that.
See §10 D2 for the alternative.

---

## 7. Failure states and the recovery path

The Stage 1 lesson in one line: *"we chose keep + Retry because Retry recovers" is
worthless unless something asserts that Retry genuinely recovers.* Every state below
names what the user does next, and every "retry" claim has a test that proves it.

| Failure | What exists afterwards | What the user does | Proven by |
|---|---|---|---|
| source engine unreachable | nothing — no artifact, no database | start their engine (named), then Retry | `db_import_recovery_check` case 1 |
| credentials rejected | nothing | fix the config or enter details manually, Retry | unit (preflight error text) |
| not enough disk | nothing | free space, Retry | unit (preflight refusal) |
| dump failed mid-way | partial artifact **deleted** (existing `export_to_downloads` behaviour) | Retry — re-dumps from scratch | `db_import_recovery_check` case 2 |
| restore failed, we created the DB | `db_created = 1` database, partially populated | Retry — **drops and recreates**, then restores | `db_import_recovery_check` case 3 |
| restore failed, DB pre-existed | their/our pre-existing DB, possibly modified | reported, **never auto-dropped**; user decides | `db_import_recovery_check` case 4 |
| cancelled | as the boundary it stopped at, stated | Retry | cancel test |

`db_import_recovery_check` is an example, not a unit test, because the thing that broke in
Stage 1 broke *below* the level unit tests reach. Case 3 is the one that matters and is
written to fail loudly if the retry path regresses: restore a dump crafted to error
part-way (valid tables, then a bad statement), assert the job failed with the database
present and partial, then run **Retry unaided** and assert the final database has every
expected table **and the expected row counts** — not "the command exited 0". Case 4
asserts a pre-existing database still exists and is untouched by cleanup.

---

## 8. Live verification — exactly what it touches

Two levels, and neither writes to the user's real engines.

**Sandboxed example (`db_import_check`), the default.** Uses `common::sandbox()` from
`f4d7a61`, and spins its **own** `mysqld` on a fixture port with a throwaway datadir
inside the sandbox as both source and target. Touches nothing outside the sandbox root
and the shared binary cache. This is what runs pre-commit.

**Live source check, on request only.** Uses the real DBngin MySQL 8.0.27 and the real
`ea` database as the **source**, because a real dump of a real WordPress database is
worth more than a fixture.

On the user's side it will:

- **read** `~/Library/Application Support/com.tinyapp.DBngin/Data/DBEngines.plist`;
- **TCP connect** to 127.0.0.1:3306 and read the handshake;
- **read** the `ea` site's `wp-config.php` (this is where its password comes from);
- **run `mysqldump --single-transaction --skip-lock-tables`** against `ea` — a
  non-locking read; their server is never written to, never restarted, never stopped.

Everything it creates — artifact, target datadir, restored database, mirrored user —
lands in the **sandbox**, on a fixture port, never in `~/Library/Application
Support/dev.rexenv.rexenv`.

**It requires the user to start DBngin's MySQL themselves.** We never start their server,
so the check refuses honestly if 3306 isn't listening. That is a button *he* presses, and
the plan says so up front rather than discovering it at run time.

---

## 9. The interim state (Stage 3 does not exist yet)

After a successful database import and before Stage 3, the site **still connects to their
old server**. The UI must make that the headline, not a footnote, because a green "done"
here would be exactly the Q2a "serving at…" lie in a new place.

The settled job's summary, per site:

> **Imported — not yet connected.** `ea` (48 tables, 24 MB) was copied into rexenv's
> MySQL 8.4.6 at 127.0.0.1:13306. **This site still reads and writes the old database**
> at 127.0.0.1:3306 — the copy will drift from it until you change the site's connection
> settings.
>
> To switch it over, change one line in `wp-config.php`:
> `define( 'DB_HOST', '127.0.0.1:13306' );`
> *(and `DB_USER` → `root` with an empty `DB_PASSWORD`, because your config connects as
> root and rexenv's root has no password — see §6)*
>
> rexenv doesn't edit your project files. A one-click, backed-up, diff-first version of
> this is coming.

Copy-paste block per shape (wp-config define, Laravel `.env` keys), and for Laravel a
note that `.env` edits are invisible under `php artisan config:cache` (`config:clear`
fixes it; we never run their artisan). Sites page badge: **"DB imported · not connected"**
— structural, from `db_created IS NOT NULL` plus a Stage-3 "connected" fact that doesn't
exist yet, so the badge cannot lie by inference.

---

## 10. Decisions needed

**D1 — the imported database's name.** *Recommend:* **keep theirs** (`ea`) when free,
disambiguate only on collision. Their name in their config then needs no change, which is
what keeps the Stage 3 rewrite to one key. The alternative (always our derived
`wp_ea_test`) is tidier in our list but makes every rewrite two keys.

**D2 — `root`-owned source configs.** *Recommend:* never touch our root, and tell the
user their interim change is 3 keys. Alternative: create a dedicated non-root user with
**their** password and grant it the database, so the password line never changes and the
rewrite is `DB_HOST` + `DB_USER` — one extra visible line, no secret in any diff. I lean
to the recommendation for Stage 2 (less machinery, nothing invented) and would revisit
when Stage 3 makes the rewrite real.

**D3 — Postgres.** *Recommend:* discovery + an honest verdict in Stage 2 ("PostgreSQL
source — importing PG databases isn't supported yet"), dump/restore deferred. Valet/Herd
sources are overwhelmingly MySQL-family; the PG path needs its own `pg_dump` ≥ server and
`\restrict` version dance (§6 of the research) and would double this stage.

**D4 — where it's invoked.** *Recommend:* a standalone per-site job (SiteDetail →
"Import database"), because the sites already imported in Stage 1 need it too; plus an
opt-in "also import databases" checkbox on the `/import` batch that runs the same job
after each site's provision settles. One implementation, two entry points.

**D5 — artifact lifecycle.** *Recommend:* one artifact per domain
(`<app-data>/db-imports/<domain>.sql`, 0600), **deleted when the job settles ok**, kept on
failure so it is diagnosable and a Retry can be diagnosed against it. One-per-domain by
construction means an orphan set is bounded and enumerable — the same reasoning as the
per-TLD resolver backup. Relevant to the current disk situation: these are full database
copies.

**D6 — the live check.** Needs the user to start DBngin's MySQL. Confirm before I write
the step into `PUBLISH-TESTING.md`.

---

## 11. Build order

*Half A*

1. v19 migration + `db_created` plumbing (store, teardown guard, backfill semantics).
2. `core::dbimport` — config mapping (WP defines + `.env` parser), unit-tested against
   real-shaped fixtures including the Bedrock and duplicate-key refusals.
3. Engine discovery + handshake identification; **first task: verify the pre-auth
   handshake read against live DBngin 8.0.27.**
4. `compat()` matrix + per-site verdict, unit-tested exhaustively.
5. Preflight (connect bound, size, disk) + dump to a 0600 artifact + manifest + hygiene
   scan. `db_dump_check` example against a sandbox source.

*Half B*

6. Collision + provenance + `CREATE DATABASE` + restore with real byte progress.
7. Credential mirroring with the reserved-name refusal.
8. Job wiring (registry, cancel, log, phases) + both entry points (D4).
9. Interim-state reporting (§9) — the honest summary, the badge, the copy-paste block.
10. `db_import_check` end-to-end in the sandbox + `db_import_recovery_check` cases 1–4.
11. `PUBLISH-TESTING.md` §I — the live DBngin pass and the packaged GUI steps.

Commit per step, `docs/TODO.md` ticked with evidence, `cargo test --lib` +
`cargo build --examples` before each commit.
