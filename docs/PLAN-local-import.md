# Import sites from Local (WP Engine / Flywheel) — the third migration source

**Status:** BUILT 11 Sep 2026 (T0–T7, `c2e1cc9` → `7eea758` + the T7 docs commit), awaiting
the owner: the live pass on a STARTED Local site (`docs/PUBLISH-TESTING.md` §N — rexenv
never starts Local's servers, so no agent can run it) and the three questions in §9.
Moves to `docs/archive/` once §N passes. Planned against `6aee703`.

Valet and Herd import shipped in four stages (`docs/archive/PLAN-valet-herd-*.md`,
`PLAN-linked-sites.md`). Local is the other big WordPress environment people leave,
so it gets the same journey: scan → link the folder → copy the database → connect →
(revert). **Every standing rule of the Valet/Herd work binds here unchanged** — the
source environment is strictly read-only (we never write, start or stop anything of
Local's), project folders are linked in place and never copied, moved or deleted, a
database import is a COPY (a non-locking read of theirs), and the connection rewrite
stays per-site, opt-in, backed up and diff-first. This plan records only what is
DIFFERENT about Local, and why.

## 1. What Local actually puts on disk

Verified read-only on the dev machine (Local 6.3.1, two sites — one single-site on
PHP 7.3.5, one subdomain multisite on 8.1.29, both halted) plus Local's own
templates shipped inside each site's `conf/`.

- **Registry: `~/Library/Application Support/Local/sites.json`** — a JSON object keyed
  by site id (`"0Yh5N8r16": {…}`). Per site: `name`, `path` (tilde form,
  `~/Local Sites/ea`), `domain` (`ea.local`), `multiSite` (`""` · `ms-subdomain` ·
  `ms-subdir`), `multiSiteDomains`, `mysql {database, user, password}`, and
  `services {php, mysql, nginx|apache, mailpit}` each with `version` and `ports`
  (`services.mysql.ports.MYSQL[0]` = that site's own mysqld port, e.g. 10003).
  `site-statuses.json` says `running`/`halted` per id — **a label only** (the DBngin
  lesson: plists label, listeners decide).
- **Layout per site:** `<path>/app/public` is the WordPress docroot (always — Local is
  WordPress-only); `<path>/app/sql/local.sql` is a dump Local wrote at some past
  export/import (**not current** — the dev machine's is from Jul 2024, never used);
  `<path>/conf/` holds Handlebars templates for nginx/apache/php/mysql; `<path>/logs/`.
- **Database: one mysqld PER SITE**, running only while that site is started in Local.
  Template `conf/mysql/my.cnf.hbs`: `port = {{port}}`, `bind-address`, `socket =
  {{socket}}`, `skip-name-resolve`, `default_authentication_plugin =
  mysql_native_password`, and a `[client]` group of `user=root / password=root` over the
  socket. Datadir and socket live under `~/Library/Application Support/Local/run/<id>/mysql/`.
  **Every site's database is named `local`** and every site signs in as `root`/`root`.
- **wp-config.php says `DB_HOST = 'localhost'`** — i.e. the unix socket, which Local's
  generated php.ini points at that site's mysqld. Read literally, the config names
  `localhost:3306`, which is NOT where the data is.
- **DNS: `/etc/hosts` lines** (`127.0.0.1 ea.local #Local Site`, plus `www.`), no
  resolver file. Domains default to `.local`.
- **HTTP by default** — `siteurl`/`home` are `http://ea.local`.

## 2. The four ways Local differs from Valet/Herd — and the design answer

### 2a. `.local` cannot be a rexenv TLD → the site is RE-HOMED

`core::tld` blocks `local` outright (Bonjour/mDNS; shadowing it breaks printers and
AirDrop). So a Local site keeps its FOLDER but not its hostname: `ea.local` imports as
`ea.<default_tld>` (`ea.rex` out of the box). A Local site whose domain is on an
allowed TLD (`shop.test`) keeps its name, exactly as a Valet row does. The scan shows
both names (`ea.local → ea.rex`) so the change is never a surprise, and a re-homed name
that some rexenv site already answers on is **needs attention**, never a silent suffix.

### 2b. The database is found through Local's registry, not the config's literal host

The config's `localhost` means "Local's socket for this site". Probing
`127.0.0.1:3306` would find nothing — or worse, somebody's Homebrew MySQL, which may
well hold a database called `local`. So the database import asks Local's registry:
**when a site's docroot is a Local site's `app/public` AND its config host is
`localhost`**, the source is that site's mysqld — reached over **its socket for the
pre-auth probe, the preflight and the dump** (it is how WordPress itself signs in
there). *Corrected 11 Sep 2026 after the first real import failed:* this line used to
send the PROBE over TCP and only the sign-in over the socket, on the guess that
`skip-name-resolve` would only bite at login. Measured on Local 10.1.2 / MySQL 8.4.0,
it bites before that — a TCP connect from 127.0.0.1 gets ERR 1130 ("Host '127.0.0.1'
is not allowed to connect") in place of the handshake, so the probe could never learn
the server's version and the job refused it as unidentifiable. Over the socket the
same server greets as `8.4.0` and the bundled 8.4.6 client signs in. The TCP port is
now only the fallback when no socket file exists. Both are RE-READ from the registry
on every run, like the credentials are re-read from wp-config: nothing about their
setup is cached. An explicit `host:port` in wp-config always wins — a user who pointed
a Local site at DBngin meant it.

A stopped site is the per-site status it always was, with Local's name on it: *"`ea`'s
database runs only while the site is started in Local — start it there, then retry.
rexenv never starts or stops Local's servers."* `app/sql/local.sql` is **never** used as
a fallback: it is a stale snapshot, and importing it would be a silent time-travel of
their data.

### 2c. The copy's URLs are re-homed — in rexenv's copy only

A Local database says `http://ea.local`; served by rexenv at `https://ea.rex`, WordPress
redirects every request back to a name rexenv does not answer. The import therefore
ends with a URL pass **on rexenv's copy** (never theirs — theirs is only ever read):
`http://<old>` and `https://<old>` → `https://<new>` (plus the JSON-escaped `http:\/\/`
forms), then the bare `<old>` → `<new>` when the hostname changed — the same passes
Change domain runs, through WP-CLI's serialization-aware `search-replace
--all-tables`.

**How WP-CLI reaches the copy before the site is connected.** The site's wp-config
still names Local's database (socket + `root`/`root`), and rexenv's root has no
password, so the plain `wp search-replace` cannot sign in to the copy. WP-CLI loads a
`--require` file before WordPress and before wp-config; PHP keeps the FIRST
`define()` of a constant (a redefinition is a Notice on 7.x, a Warning on 8.x — never
fatal; checked with the bundled 7.4.33). So the pass runs with a rexenv-written,
0600, deleted-after require file defining `DB_HOST=127.0.0.1:<our port>`,
`DB_USER=root`, `DB_PASSWORD=''`, `DB_NAME=<the copy>` — **no secret in it at all**,
nothing written into the project, `--skip-plugins --skip-themes` so their code does not
run beyond WordPress core bootstrapping. A failed pass fails the job (Retry re-imports
from scratch — the established Stage 2 recovery), because a copy that silently still
points at `ea.local` is a broken site that looks imported.

Applies only to Local-sourced imports (the registry match of 2b). Valet/Herd imports
keep their hostname and are untouched by this pass.

### 2d. Local sites mostly need a PHP choice → the Import screen gets the picker

Local pins full patch versions (`7.3.5`); rexenv ships 7.4 and 8.0–8.5. A 7.3 site is
**needs attention** — and the Import screen has never had a way to answer that (the
wire always carried `php: {}`), so on the dev machine NOTHING would be importable. The
row gets a version picker listing what rexenv ships; picking one makes the row
selectable and travels in `ImportRequest.php`. This also closes the same silent gap
for Valet/Herd rows pinned to an unshipped minor. Never a silent substitute: the
picker starts empty and the row stays unticked until a version is chosen.

## 3. What is NOT imported in this cut (honest rows, not silence)

- **Multisite** (`multiSite != ""`) → **unsupported**, with the reason: re-homing a
  network changes `DOMAIN_CURRENT_SITE` in wp-config (a write into their project the
  rewrite contract cannot express) and every subsite's row in `wp_blogs`; rexenv's
  own Change domain refuses multisite for the same reason. §9 Q2.
- **Missing folder / no `app/public/wp-config.php`** → unsupported, naming the path.
- **Local's certificates, router, Mailpit, Xdebug toggle, Live Links** — never
  imported; rexenv issues its own certs and runs its own mail catcher.
- **Apache sites** are served by rexenv's nginx like every other WordPress import (the
  WordPress rules are the same); the row says Local used Apache.

## 4. The pieces, as built

- `core/localwp.rs` — PURE, read-only discovery: parse `sites.json` leniently, expand
  `~`, map each site to a `valet::DiscoveredSite` (new `SourceKind::Local`) with the
  re-homed domain and `renamed_from`, plus `db_source_for(home, docroot)` (2b) and
  `rehome_domain` (2a). Unit-tested against fixture trees shaped like the real one.
- `commands/valet_import.rs` — the scan merges Local rows; `enrich` gains
  already-imported-by-FOLDER (a re-homed domain can't be matched by name) and the
  `php_choice` marker; the run is unchanged (same sequential, continue-on-failure,
  cancel-between-sites loop).
- `commands/db_import.rs` — the Local source override (2b) and the URL pass (2c) as a
  sixth job phase, `skipped` for every non-Local import.
- `core/dbdump.rs` — `DefaultsFile` can say `socket=` instead of `host/port`.
- UI — Import screen names Local, shows `old → new`, the PHP picker; Sites/Settings
  nudges say "Valet, Herd or Local".
- MCP `valet_import` — description + scan fields (`renamedFrom`). Tool name unchanged
  (renaming a shipped tool breaks every agent config that learned it).

## 5. Task list (one commit each)

- **T0** `docs(plan)` — this file + the TODO row.
- **T1** `feat(core)` — `core/localwp.rs` discovery + `SourceKind::Local` + fixture
  tests; MAP/README.
- **T2** `feat(import)` — scan integration (merge, folder-based already-imported,
  `renamedFrom`), TS types, Import/Sites/Settings copy, MCP description.
- **T3** `feat(import)` — the PHP picker for rows pinned to a version rexenv doesn't ship.
- **T4** `feat(db-import)` — the Local database source (registry port + socket),
  socket-capable defaults file, the Local-named "start it in Local" status.
- **T5** `feat(db-import)` — the URL re-home pass on rexenv's copy (`--require`
  override), sixth phase.
- **T6** `test(examples)` — TWO examples, split by tier (found while building T5:
  installing a real WordPress is `wp core download`, i.e. network):
  `local_scan_check` (sandbox — the real `~/Local Sites` and Local's app data,
  read-only fingerprint, #570's L1) and `local_import_check` (network — a sandbox
  mysqld plays Local's server: sign-in over its SOCKET through the new defaults file
  (#572's half that needs no started Local), and the URL pass against a real
  WordPress whose wp-config names a dead socket and `root`/`root`, #573's L1).
- **T7** `docs` — ARCHITECTURE §8, ledger rows, PUBLISH-TESTING §N (the live pass that
  needs a Local site STARTED — a button only the owner presses), TODO tick.

## 6. Invariants this adds (each gets a ledger row in the commit that writes it)

1. `core::localwp` never writes, and never reads a file inside a project beyond an
   existence probe (same rule as `core::valet`) — `local_scan_check` fingerprints it.
2. A `.local` hostname never becomes a rexenv domain (policy already refuses it; the
   scan re-homes BEFORE `enrich`, so no row ever carries one).
3. The Local source override applies only when the docroot IS a registered Local site
   AND the config host is `localhost`; an explicit host always wins.
4. The URL pass writes only rexenv's copy: its require file names rexenv's engine and
   the copy's name, and the job refuses to run it unless the target is the database
   this job just restored.

## 7. Verification plan, and the honest gap

L0: registry parsing (tilde, missing keys, junk JSON, the multisite and
missing-folder rows), re-homing, `db_source_for` matching, defaults-file socket form,
require-file contents (no password key, our port), phase list. L1: `local_scan_check`
against the real install (read-only fingerprint over `~/Local Sites` and Local's app
data). **The gap:** the dump from a live Local mysqld and the URL pass end to end need
a Local site STARTED in Local — rexenv never starts their servers, so that is
`PUBLISH-TESTING.md` §N, run by the owner.

## 8. Explicitly out of scope

Local's Live Links, Blueprints and Connect (host sync), importing `local.sql`
snapshots, multisite re-homing, and any "clean up Local" affordance.

## 9. Questions for the owner (the build proceeds on the stated interim)

**Ruled 12 Sep 2026:** Q1 **declined, then reversed the same day** — after the owner's
second Local import (`tr.local`, restored as `local_tr_local_rex`) showed the refusal in
practice: the site could not load at all until someone edited `DB_NAME` by hand.
`RewriteKey::Name` now carries the rename (ledger #574). Q2 **parked** (a `docs/TODO.md` Parked row). Q3 **approved and
needed now**: the owner's `tr.local` could not be imported at all because `tr.test`
already existed (their default TLD is `.test`), and the row offered no way to pick
another name. The original questions follow as asked.

- **Q1 — the database name.** Every Local site's database is `local`, so the second
  Local import collides and is restored as `local_<domain>`. The Stage 3 rewrite made
  collision-renames **tell-only permanently** (28 Jul 2026), on the evidence of zero
  collisions across a dozen real Valet sites — for Local, collisions are the norm, so
  every Local site after the first gets the copy-paste block instead of the one-click
  connect. Options: (a) keep the ruling (interim — built this way); (b) allow a
  `DB_NAME` key in `RewritePlan` (a name, never a secret — the diff stays secret-free);
  (c) name Local copies `<site>` from the start (still a rename → same as b).
  **Recommendation: (b).**
- **Q2 — multisite.** Unsupported in this cut (§3). Worth building re-homing for
  networks (wp-config `DOMAIN_CURRENT_SITE` edit + `wp_blogs`), or does a
  keep-the-name path for multisite on an allowed TLD suffice?
- **Q3 — re-home target.** Interim: `<name>.<default_tld>`. Should the scan offer a
  per-row domain edit instead?
