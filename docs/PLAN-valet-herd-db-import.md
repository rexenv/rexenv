# Stage 2 — import a Valet/Herd site's database

**Status:** SHIPPED — §I passed 27 Jul 2026 (all 12 steps, live DBngin source, packaged app; the honest-refusal and drift steps both behaved). Stages 0 and 1 shipped
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

**VERIFIED live (2026-07-26), both vendors, no authentication:**

```
127.0.0.1:13306  payload=73B seq=0 proto=10  version='8.4.6'           raw=b'\n8.4.6\x00'
127.0.0.1:13307  payload=82B seq=0 proto=10  version='12.3.2-MariaDB'  raw=b'\n12.3.2-MariaDB\x00'
127.0.0.1:3306   ConnectionRefusedError — DBngin stopped, refused instantly
```

So: 3-byte LE payload length, 1-byte sequence, then protocol byte `10` and the
NUL-terminated version. **Family = `-MariaDB` present anywhere in the string**; version =
the rest. The stopped case refuses instantly, confirming §2.2 needs no timeout tuning.

Two caveats carried into the implementation:

- **MariaDB 10.x prefixes `5.5.5-`** (`5.5.5-10.11.2-MariaDB`) — the old replication
  compatibility hack. Our 12.3.2 sends no prefix (verified above); the 10.x form is
  documented but **not verified here**, so the parser strips a leading `5.5.5-` when
  present and a live 10.x sighting should be recorded back into this section
  (none sighted as of 28 Jul 2026 — the parser leg is pinned by tests either way).
- ~~DBngin's MySQL 8.0.27 specifically is still unverified~~ **VERIFIED in the §I pass
  (27 Jul 2026)**: identified pre-auth as MySQL 8.0.27 once started.

**If it does not hold — a proxy in front, TLS required, an unusual configuration — the
answer is to REPORT, not to work around it.** A quiet fallback to an authenticated probe
would reintroduce the client-pairing trap at exactly the step this design removes it
from: to authenticate we must already know the vendor, which is what we were trying to
learn. The honest degraded state is "an engine is listening on 3306; rexenv can't tell
which one" plus a way for the user to say. Whatever we find gets written back into this
section as verified fact.

Postgres has no pre-auth greeting; identify it with `psql -c "SHOW server_version"` plus
`PGCONNECT_TIMEOUT`.

### 2.2 A stopped source engine

Per-site status, verbatim shape:

> **database not reachable** — `myblog` is on MySQL at 127.0.0.1:3306, which isn't running.
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

### 2.4 One vocabulary of states, and three that must not collapse

`DbSiteStatus` is a closed set; the parse side produces some variants, the live preflight
produces the rest, and both use the same words. Three of them look alike from a distance
and are deliberately kept apart, because each has a different cause and a different fix:

| state | what happened | what the user does |
|---|---|---|
| **database not reachable** | nothing is listening at the host:port their config names | start their server; we never do |
| **sign-in refused** | something IS listening and rejected their own credentials | fix the credentials, or point us at the right server |
| **database not found** | connected fine, signed in fine, and the named database isn't there | see below |

**"Database not found" is the one worth designing.** It usually means one of two things:
they dropped the database, or the site points at a different server than the one that is
running. So the message doesn't stop at the negative — having authenticated, we can list
what the server *does* hold, which turns it into a diagnosis:

> The 8.0.27 server at 127.0.0.1:3306 is running and accepted the sign-in, but has no
> database called `myblog`. It does have: myblog_old, wordpress, shop (+3 more). Either the
> database was deleted, or this site points at a different server than the one running
> here.

And when the server is empty, that reads differently again — "no user databases at all"
is nearly always a site pointed at the wrong server, and the copy says so. Collapsing any
of this into "database error" would throw away the only information that tells the user
which of the three problems they have.

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
  then let `--result-file` write into it: the tool truncates in place, so the inode's
  mode survives. **VERIFIED (db_dump_check, 2026-07-27):** the artifact reads mode 600
  after the real mysqldump 8.4.6 wrote into it — no chmod window, no fallback needed.

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
(`effective_db_version`), not per site. So a MySQL 5.7 source may produce "Warn against
your MySQL 8.4.6 — rexenv also ships 8.0.44, which is a closer match."

**That offer must read as disruptive, because it is.** Each version series keeps its own
datadir (never an in-place up/downgrade — the existing `set_db_engine_version` confirm
already says so). Switching 8.4.6 → 8.0.44 therefore means **every database every other
rexenv site uses becomes invisible**: not deleted, but sitting in the 8.4.6 datadir that
is no longer mounted by a running server. Those sites break — WordPress shows its
connection error — until the user either switches back (which restores them exactly, and
then *this* import is the odd one out) or exports and re-imports each one into the new
series.

So the wording is a warning that happens to have a button, not a convenience:

> rexenv also ships MySQL 8.0.44, a closer match for this 5.7 source. **Switching is
> disruptive and usually the wrong choice:** your other N sites' databases live in the
> 8.4.6 datadir and would stop being visible until you switch back. Importing into 8.4.6
> normally works — the warning above is what to watch for.

Never silent, never framed as the recommended fix, and the count of affected sites is
real, not "other sites".

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

**The preflight refuses in cost order, and the order is structural (built, step 5).**
is-this-us → compatibility verdict → live checks (sign-in / database-exists / size) →
disk. Not a convention: `preflight_live` and `dump` require the `Cleared` witness only
`gate` (the no-connection checks) can mint, and `dump` additionally requires the
`Preflight` only `check_disk` produces — a caller physically cannot reach a connection
attempt for a pairing that was refusable from the start, nor skip the disk answer.

**A partial dump is unrepresentable as an artifact (built, step 5).** Three layers, each
sufficient alone: the dump writes to `<domain>.sql.partial` and renames only on exit 0
(a crash leaves a wrong-named file nothing restores); the manifest is written only after
the rename (rename-then-manifest, so a manifest's existence implies a whole artifact —
the reverse order could describe a file that isn't there); and Half B loads exclusively
through `load_manifest`, which refuses a missing manifest and an artifact whose byte
size differs from the recorded one. Cancel/crash/disk-full all land in one of those.

**The manifest** records: domain, database, source host/port/vendor/handshake-version,
the target engine+version the verdict was computed against (Half B re-checks it —
restoring into a different engine would silently skip the gate), exact artifact bytes,
table count, dump tool, unix timestamp, and the hygiene findings (sandbox line to skip,
definer count, `mysql_native_password`, `NO_AUTO_CREATE_USER`). **No credential fields
exist on the type** — the restore re-reads the site's own config live, so it never
needed them — and a test pins the exact serialized key set so a new field is a conscious
decision.

**Cancelling a dump is a read that stopped (verified reasoning, step 5).**
`--single-transaction` opens a consistent-snapshot READ transaction; killing the client
drops the connection, and session teardown rolls the snapshot back and releases metadata
locks. We never use `FLUSH TABLES WITH READ LOCK`, `--master-data`, or
`--lock-all-tables` — the flags that take locks an ordinary session death wouldn't
already release. Cancel = SIGTERM → poll → SIGKILL on the *client*, delete the
`.partial`; their server just sees a connection drop.

**Preflight, then dump.** `--connect-timeout` is accepted **nowhere** by the dump tools
(argv, `[client]`, `[mysqldump]` — all hard-error `unknown variable`, exit 7, verified on
all four bundled dump binaries). So the connection is bounded by a preflight with the
**interactive** client (`--connect-timeout=10`, today's `client_base_args` shape), which
also fetches the size, and the dump itself runs with an **unbounded transfer**. B25's
rule holds: bound the connect, never the transfer. A 20-minute dump of a large database
is a success, not a hang.

**"Is this server us?" — positively identified, never a string match.** Discovery probes
whatever host:port a site's config names, and after Stage 3 rewrites a site that will be
*our* engine. Dumping our own database to restore over itself must be refused — but a
`host == "127.0.0.1" && port == 13306` test is the wrong shape: it breaks on
`localhost`, on a hostname that resolves to loopback, and the moment a port moves.

The answer already exists in this codebase for exactly this question. Ownership is
**positive identification** — our fixed port *plus* an app-data marker on the process
cmdline (`owned_master`, `adopt_startup`) — and `ServiceManager` is the source of truth
for what we own and whether it is live. So:

```
is_ours(port, handshake_version) =
      ServiceManager reports a RUNNING rexenv-owned engine on that port
  AND the handshake version equals that engine's effective version
```

Two independent facts, neither of them a name: we own the listener, and the thing that
answered is the thing we own. Host spelling stops mattering, because `localhost` and
`127.0.0.1` reach the same listener and the question is about the listener, not the
string. (If our engine is stopped, the probe simply finds nothing — "database not
reachable", whose fix is to start rexenv's MySQL. Also honest.)

**And the more interesting neighbour: it's ours, but it's another site's database.**
Once "is this server us?" is answered positively, the database name splits three ways,
and only the first is "nothing to import":

| what we find | state |
|---|---|
| our engine, and the name is THIS site's `db_name` | **already imported** — the site already uses rexenv's database; nothing to do |
| our engine, and the name is ANOTHER site's `db_name` | **shared with `<domain>`** — two sites pointing at one database. Real (staging pairs, a duplicated config), and importing would either clobber that site's data or silently fork it. Name the other site and stop. |
| our engine, and no site claims the name | a database on our engine that rexenv didn't create for a site — the user's own, via Adminer or a hand-run import. Treat as a collision, not a source. |

None of these is a failure; all three are "there is nothing here to copy, and here's what
you're actually looking at". Implemented in step 5 with the `sites` table as the
authority for who owns a name — recorded, not derived, exactly like `db_created`.

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

**The artifact is a copy of their database, and is treated as one.**
`<app-data>/db-imports/<domain>.sql`, born 0600 (§3), one per domain by construction so
the orphan set is bounded and enumerable — the same reasoning as the per-TLD resolver
backups. Deleted when the job settles ok; **kept on failure**, because that is when
someone needs to look at it.

A kept artifact must be **visible and removable in the UI, not a file someone stumbles
on months later**. So: the failed job's summary names the path and says plainly that it
contains a full copy of the database; Settings grows a row alongside the borrowed-resolver
card — "Leftover database dumps: 2 files, 340 MB" with a per-file delete and a delete-all
— and the same enumeration backs a `sweep_orphan_dumps` on startup that reports (never
auto-deletes: it's their data). Uninstall removes them, since we created them, and says
so in the teardown report.

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
`myblog` sample is `DB_USER=root` with a password). Our engines run passwordless
`root@localhost`; that is baked into `client_base_args` and every path that uses it.

> **Hard rule: never create, alter, or set a password on `root`** (or `mysql.sys`,
> `mysql.session`, `mysql.infoschema`, `postgres`).

The reasoning, recorded because the choice alone is not the useful part: every rexenv
database operation authenticates as passwordless root through one shared flag array,
`core/database.rs::client_base_args` — `--user=root` with no password, pinned by a unit
test precisely so it can't drift. Adminer, the per-site DB-size query on the Sites page,
`create_database`/`drop_database`, site teardown and every WP-CLI database call go through
it or its equivalent. Setting a password on our root to mirror theirs would break **all of
them at once**, on every site on the machine, in exchange for one imported site's
convenience. That is not a trade-off to weigh; it is a hard stop.

So for `root`-owned source configs, mirroring is impossible and the honest consequence is
that Stage 3's rewrite for those sites touches three keys (`DB_HOST`, `DB_USER`,
`DB_PASSWORD` → root / empty) rather than one. The interim message (§9) says exactly that.

**Recorded for Stage 3, deliberately not built now:** the alternative is a dedicated
non-root user created with *their* password and granted only this database. The rewrite
then becomes `DB_HOST` + `DB_USER` and **the password line never changes**, so no secret
ever appears in a diff, a backup, or an editor buffer — which is the reason to prefer it
once the rewrite is real and diffs are something the user reads. It is the wrong thing to
build in Stage 2: it invents an account the user never asked for, to serve a rewrite that
doesn't exist yet.

---

## 7. Failure states and the recovery path

The Stage 1 lesson in one line: *"we chose keep + Retry because Retry recovers" is
worthless unless something asserts that Retry genuinely recovers.* Every state below
names what the user does next, and every "retry" claim has a test that proves it.

| Failure | What exists afterwards | What the user does | Proven by |
|---|---|---|---|
| source engine unreachable | nothing — no artifact, no database | start their engine (named), then Retry | `db_dump_check` (refused-gate leg) |
| credentials rejected | nothing | fix the config or enter details manually, Retry | unit (preflight error text) |
| not enough disk | nothing | free space, Retry | unit (preflight refusal) |
| dump failed mid-way | partial artifact **deleted** (existing `export_to_downloads` behaviour) | Retry — re-dumps from scratch | `db_dump_check` (partial-deleted leg) |
| restore failed, we created the DB | `db_created = 1` database, partially populated | Retry — **drops and recreates**, then restores | `db_restore_check` case 3 |
| restore failed, DB pre-existed | their/our pre-existing DB, possibly modified | reported, **never auto-dropped**; user decides | `db_restore_check` case 4 |
| cancelled | as the boundary it stopped at, stated | Retry | cancel test |

`db_restore_check` is an example, not a unit test, because the thing that broke in
Stage 1 broke *below* the level unit tests reach. Case 3 is the one that matters and is
written to fail loudly if the retry path regresses: restore a dump crafted to error
part-way (valid tables, then a bad statement), assert the job failed with the database
present and partial, then run **Retry unaided** and assert the final database has every
expected table **and the expected row counts** — not "the command exited 0". Case 4
asserts a pre-existing database still exists and is untouched by cleanup.

---

## 8. Live verification — exactly what it touches

Two levels, and neither writes to the user's real engines.

**Sandboxed example (`db_dump_check + db_restore_check`), the default.** Uses `common::sandbox()` from
`f4d7a61`, and spins its **own** `mysqld` on a fixture port with a throwaway datadir
inside the sandbox as both source and target. Touches nothing outside the sandbox root
and the shared binary cache. This is what runs pre-commit.

**Live source check, on request only.** Uses the real DBngin MySQL 8.0.27 and the real
`myblog` database as the **source**, because a real dump of a real WordPress database is
worth more than a fixture.

On the user's side it will:

- **read** `~/Library/Application Support/com.tinyapp.DBngin/Data/DBEngines.plist`;
- **TCP connect** to 127.0.0.1:3306 and read the handshake;
- **read** the `myblog` site's `wp-config.php` (this is where its password comes from);
- **run `mysqldump --single-transaction --skip-lock-tables`** against `myblog` — a
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

> **Imported — not yet connected.** `myblog` (48 tables, 24 MB) was copied into rexenv's
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

**The summary and the badge must read ONE fact, not two computations of it.** The failure
mode to design out is the pair drifting — a badge saying "connected" beside a summary
saying "not yet connected", each right by its own arithmetic. So the backend serializes a
single `DbImportState` per site (`state`, `dbName`, `tableCount`, `sizeBytes`,
`sourceLabel`, `artifactPath`); the summary sentence and the badge label are both rendered
**from that one struct** and neither derives anything of its own. `state` is a closed enum
whose "connected" variant **does not exist** until Stage 3 introduces the fact that proves
it — so the UI cannot render "connected" by inference, only by being told.

Copy-paste block per shape (wp-config define, Laravel `.env` keys), and for Laravel a
note that `.env` edits are invisible under `php artisan config:cache` (`config:clear`
fixes it; we never run their artisan). Sites page badge: **"DB imported · not connected"**
— structural, from `db_created IS NOT NULL` plus a Stage-3 "connected" fact that doesn't
exist yet, so the badge cannot lie by inference — and it renders the same `DbImportState`
the summary does, so the two can never disagree.

---

## 10. Decisions — SETTLED (2026-07-26)

**D1 — the imported database's name: keep theirs** (`myblog`) when free; collisions stay the
B21 pattern (`db_name_exists` + live `SHOW DATABASES` → `unique_db_name`'s disambiguated
form). Their config's `DB_NAME` then needs no change, which is what keeps the Stage 3
rewrite short.

**D2 — never touch our root; the interim is a 3-key change.** Reasoning recorded in §6,
not just the choice. The dedicated-non-root-user alternative is recorded there too, as
the **Stage 3 option** — likely preferable then precisely because it keeps secrets out of
the diff, and the wrong thing to build now.

**D3 — Postgres: discovery + honest refusal.** A half-working PG path is worse than none.
The wording must say **"not supported yet"** and never imply their setup is wrong:

> PostgreSQL detected on 127.0.0.1:5432 (`sitedb`). rexenv can't import PostgreSQL
> databases yet — MySQL and MariaDB only for now. Everything else about this site imports
> normally.

**D4 — one implementation, two entry points:** a standalone per-site job (SiteDetail →
"Import database", which the Stage 1 sites need) plus an opt-in "also import databases"
checkbox on the `/import` batch that runs the same job after each site's provision
settles.

**D5 — one artifact per domain**, `<app-data>/db-imports/<domain>.sql`, born 0600,
deleted on success, kept on failure — and **discoverable and removable from the UI**, not
a file found later. Full spec in §5; it holds a complete copy of their database and the
copy says so.

**D6 — the live check: confirmed.** The user starts DBngin's MySQL themselves; the check
**refuses honestly if 3306 is silent** and never attempts to start anything. Written into
`PUBLISH-TESTING.md` §I as an explicit precondition step, not an aside.

---

## 11. Build order

*Half A*

1. v19 migration + `db_created` plumbing (store, teardown guard, backfill semantics).
2. `core::dbimport` — config mapping (WP defines + `.env` parser), unit-tested against
   real-shaped fixtures including the Bedrock and duplicate-key refusals.
3. Engine discovery + handshake identification. **Step 0 of the whole stage: prove the
   pre-auth handshake read live — report if it doesn't hold, never fall back silently
   (§2.1).**
4. `compat()` matrix + per-site verdict, unit-tested exhaustively.
5. Preflight (connect bound, size, disk) + dump to a 0600 artifact + manifest + hygiene
   scan. `db_dump_check` example against a sandbox source.

*Half B*

6. Collision + provenance + `CREATE DATABASE` + restore with real byte progress.
7. Credential mirroring with the reserved-name refusal.
8. Job wiring (registry, cancel, log, phases) + both entry points (D4).
9. Interim-state reporting (§9) — the honest summary, the badge, the copy-paste block.
10. `db_dump_check + db_restore_check` end-to-end in the sandbox + `db_restore_check` cases 1–4.
11. `PUBLISH-TESTING.md` §I — the live DBngin pass and the packaged GUI steps.

Commit per step, `docs/TODO.md` ticked with evidence, `cargo test --lib` +
`cargo build --examples` before each commit.
