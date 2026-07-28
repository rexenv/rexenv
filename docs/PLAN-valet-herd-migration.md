# Migrate an existing Valet / Herd environment into rexenv

**Status: ALL FOUR STAGES SHIPPED — the feature is complete.** Stage 0
(`PLAN-linked-sites.md`), Stage 1 (§9 pass, 26 Jul 2026), Stage 2 (§I pass, 27 Jul
2026), Stage 3 (§J pass, 28 Jul 2026 — `PLAN-valet-herd-rewrite.md`). Approved 26 Jul
2026; research (Q0–Q7) verified as recorded below; all decisions settled (the last,
§11.3, on 27 Jul 2026 as Stage 3 D5). Researched 25–26 Jul 2026 against the code as
of `f302996`, a live Valet 4.12.0 + Herd 1.29.0 install (the "reference install" —
a real, messy two-tool setup), laravel/valet source at tag v4.12.0,
herd.laravel.com docs, and the bundled DB binaries. Line numbers below are anchors
taken at `f302996` — trust the file, verify the line (several `core/sites.rs` cites
have drifted as the file grew). What SHIPPED is described in `docs/ARCHITECTURE.md`
§8; the per-stage build record is `docs/archive/SHIPPED-2026-07.md`. This doc
remains the canonical home of the empirical research: the on-disk layouts (§2),
the four site-side conflicts (§3), the engine-compat findings (§6) and the
dump-tool flag surface (§7).

The two pre-existing bugs in §12 are folded INTO this work, not handled separately —
they sit directly in the migration path.

## Context

Most of our users already run Valet or Herd. Making them recreate 20 sites by hand is
the adoption wall; importing them is the single biggest unlock. And because rexenv is a
COMPLETE stack (bundled MySQL/MariaDB/Postgres), the migration has to bring DATABASES
too — if the user still needs DBngin or Homebrew MySQL running afterwards, we only did
half the job.

Two rules constrain every line of the design:

1. **The source environment is STRICTLY READ-ONLY.** We never modify, move, or delete
   anything belonging to Valet or Herd — not their config, certs, resolver files,
   nginx, and not their databases (a dump is a read; we never drop, alter, clean up, or
   stop their server). The user must be able to go back to Herd at any time. We never
   offer to "clean up" or uninstall their old environment.
2. **We never move, copy-over, or rewrite the user's project files** — except under the
   opt-in connection-config rewrite (§8) if approved: explicit, per-site, backed up,
   diff shown first. Import otherwise creates only rexenv state (site row, our vhost,
   our cert) pointing AT the existing directory.

Migration by COPY is not acceptable (duplicates gigabytes, breaks their git workflow),
so §1 is the feasibility gate for the whole feature.

## 1. Feasibility gate — can we serve files outside the sites dir?

**YES. The serving plane is already 100% path-agnostic; only the create ENTRY POINTS
overwrite the caller's path.** Traced end to end:

| Link | Verdict | Evidence |
|---|---|---|
| Schema | works as-is | `sites.path TEXT NOT NULL`, no constraint, no canonicalization; the path IS the docroot (`state/db.rs:19-30`, `state/models.rs:87`) |
| Create IPC | **the one gap** | `provision()` computes `sites_dir/<domain>`, `create_dir_all`s it, then **overwrites `new.path`** (`core/sites.rs:761-778`). `create()` itself already accepts an arbitrary path (`:221-272`). No link-existing-folder flow exists (backend or UI) |
| Provisioning | tolerates existing | WP phases are individually idempotent — `core_download` skipped if `wp-load.php` exists, `wp config create` skipped if `wp-config.php` exists, `core_install` skipped if installed (`commands/site_provision.rs:655,673,707`). Blank-PHP phase set (prepare/fetch/serve) already IS "just serve this dir" (`:123-136`) |
| nginx vhost | works as-is | `root "{root}"` comes from `PathBuf::from(&s.path)` verbatim (`core/services.rs:411,424`, `core/service_manager.rs:756`); never re-joined from `sites_dir`. All three rewrite templates are path-independent |
| Caddy edge | works as-is | per-site block is `tls` + `reverse_proxy` only — no filesystem root, no `file_server` (`core/proxy.rs:152-200`) |
| Overrides | works as-is | FrankenPHP `root * "{root}"`, Apache `DocumentRoot "{docroot}"` — same row, quoted (`core/frankenphp.rs:153-161`, `core/apache.rs:143-145`) |
| php-fpm pool | works as-is | pool config has ZERO per-site paths, no `open_basedir`, no `chroot`, no user/group; runs as the logged-in user (`core/services.rs:26-81`) → reads `~/code` fine |
| B26 docroot validation | passes | character-level only — rejects `" $ { } \` + control chars, nothing semantic; no sites_dir prefix requirement, no existence check (`core/sites.rs:479-489`). `~/code/myapp/public` passes |
| Permissions | works as-is | nginx / fpm / overrides all unprivileged as the user; the root edge never touches files (proxies TCP only) |
| Cert issuance | works as-is | pure-local rcgen keyed on the hostname, SANs `[domain, *.domain]`, 397-day leaf; no TLD logic, no rate limits (`core/ssl.rs:164-176`) |
| **Delete safety** | **already guarded** | teardown removes the docroot ONLY if it starts with the configured / default / legacy sites dir (`core/sites.rs:636-650`). An external path is never deleted — already surfaced in the move-folder dialog copy (`src/routes/SiteDetail.tsx:573-576`) |
| Frontend | gap | `NewSiteDialog.tsx:156` hardcodes `path: ""`; the TS type documents it (`src/types/index.ts:72`). CLI `site.create` likewise hardcodes `String::new()` (`cli_server.rs:~368`) |

### Minimal change set ("linked sites")

1. `core/sites.rs` `provision()` — honor a non-empty caller path: validate (existing
   `validate_docroot_path` + `is_absolute` + `is_dir`), skip `create_dir_all` and the
   unconditional Blank-PHP `index.php` write, don't overwrite `new.path`.
2. `commands/site_provision.rs` — nothing structural (phases already skip pre-existing
   installs); optionally a "linked, skip provisioning" flag so a WP-type row can skip
   the DB/core phases outright.
3. `cli_server.rs` — optional `path` on `SiteCreateArgs`.
4. `NewSiteDialog.tsx` — "use existing folder" picker replacing the hardcoded `""`.
5. **Delete-guard hardening (do it):** the current guard is a lexical prefix check
   against the CURRENT setting. If a user later points the Sites folder at `~/code`,
   deleting a site whose docroot is `~/code/myapp` passes the guard and `remove_dir_all`
   erases the project. Record a per-row `linked` marker and exclude linked rows from
   docroot removal — structural, not inferred (cf. "prefer structural guarantees").

Nothing changes in config generation, pool generation, cert issuance, or the move /
status / metrics / terminal consumers — they all read `sites.path` verbatim.

**Behavioral note to disclose in the UI:** WP conveniences write INTO the docroot
(tunnel + login mu-plugins, git-asset clones, wp-cli). For a linked directory that
means writing inside the user's project — same as Valet/Herd do, but say so.

## 2. What Valet and Herd actually put on disk

Verified read-only on the reference install and against laravel/valet v4.12.0 source.

### Valet 4.12.0 — `~/.config/valet/`

- `config.json` — live here: `{"tld":"test","loopback":"127.0.0.1","paths":["~/.config/
  valet/Sites"]}`. Parse rule: `tld = config.tld ?? config.domain ?? "test"` (the key
  was `domain` until **v2.1.0**, which also moved the home dir from `~/.valet` and
  flipped the default TLD from `.dev` to `.test`). `valet link` PREPENDS the Sites dir
  into `paths` — expect it in the list.
- `Sites/` — symlink farm; hostname = symlink name + tld. Links beat parked dirs (Valet
  filters colliding parked entries and scans the prepended Sites path first).
- `Nginx/<fqdn>` — exists ONLY for isolated / secured / proxied sites. Plain parked
  sites have no per-site file at all (served by a catch-all).
  - **Isolated PHP version = first line `# ISOLATED_PHP_VERSION=<v>`.** Value format
    varies by writer: `php@8.1`, `8.1`, or bare `81` — all three occur (the reference
    install had two of them side by side). Parse all three. `.valetrc` (v4) / `.valetphprc` (v3) in the
    project root are input hints, not the store — read the marker first.
  - **Proxy sites** are recognized by first line `# valet stub: proxy.valet.conf` (or
    `secure.proxy.valet.conf`) + a `proxy_pass` line, and must be EXCLUDED from site
    import (list them separately as not-importable).
- `Certificates/<fqdn>.{crt,key,csr,conf}` — "secured" detection = the `.crt` exists
  (Valet's own test). CA is `CA/LaravelValetCASelfSigned.pem`. **We never import their
  certs** — we always issue our own.
- DNS: Homebrew dnsmasq + `dnsmasq.d/tld-test.conf` (`address=/.test/127.0.0.1`) and
  `/etc/resolver/test` = `nameserver 127.0.0.1`, written by the Valet CLI itself (its
  wrapper re-execs every command under sudo).
- **Docroot is never recorded on disk** — Valet decides it per request via PHP drivers.
- Layout is stable from v2.1.0 → v4 (dir names, cert naming, config keys). Detect, don't
  hardcode: home dir (`~/.config/valet` else `~/.valet`), tld key, marker format,
  `valet.sock` vs per-version `valetXY.sock` (v3+), `BREW_PREFIX` (`/usr/local` vs
  `/opt/homebrew`).

### Herd 1.29.0 — `~/Library/Application Support/Herd/`

- Embeds a customized Valet, so the shapes match: **`config/valet/config.json` is
  Valet-v4-shaped** (live here: tld/loopback/paths + `share-tool`), same `Sites/`
  symlinks, same `Nginx/<fqdn>` confs with the same `ISOLATED_PHP_VERSION` marker, same
  `Certificates/` naming, same CA filename.
- Differences in the confs: `fastcgi_pass $herd_sock_84` (an nginx variable per PHP
  version, not a literal sock path), `server.php` inside Herd.app, an extra
  `fastcgi_param HERD_HOME`.
- PHP binaries in `bin/` (e.g. `php82`–`php85` on disk). **Prefs lie**: the plist can
  claim versions installed while those binaries are absent and `bin/php` is a broken
  symlink — trust binaries on disk, never `defaults read`.
- Live quirks a parser must survive (all observed on the reference install):
  duplicate parked path differing only by a trailing slash; a parked `~/Herd/` folder
  with no sites in it; an orphan conf + cert with no Sites symlink; a conf on a TLD
  the config never mentions (a `.dev` one); a double-suffix name (`<name>.test.test`);
  **a third of the symlinks dangling**.
- **Herd auto-migrates Valet on first launch** → the same site appears in BOTH trees
  (the Herd tree is a superset of the Valet one). Dedupe by domain; prefer the Herd row.
- Services (nginx, dnsmasq, php-fpm) run via the root helper `de.beyondco.herd.helper`
  and **die when Herd quits** — the opposite of our outlive-the-app model. See §3b.
- Herd Pro (not present on the reference install): MySQL 8.0/8.4/9.4, MariaDB 10.11, Postgres
  14–18, Mongo, Redis/Valkey, etc. under `config/services`, multiple instances, per-
  instance ports, root + empty password by default. Mail = its own catcher on SMTP 2525,
  messages in `HerdCoreData.sqlite`. All import-relevant SITE state is free-tier.
- Alternative read path worth knowing: Herd ships an MCP server with a `get_all_sites`
  tool. We won't depend on it (read files), but it corroborates.

### The reference install (what the scan was hardened against)

A real two-tool setup: ~30 unique site names across both tools (roughly two-thirds
live targets, a third dangling), a dozen WordPress projects with `wp-config.php` at
root, one secured Valet site, no Laravel project with a `.env` present.

## 3. The four site-side conflicts

### a) `:443` — Herd/Valet hold it, our root edge wants it

Existing detection is solid and two-layer, and it covers the Herd case:

- **Pre-start gate** — `ports::ensure_free` connect-probes 127.0.0.1:443 for privileged
  ports (`core/ports.rs:50-61`); called for the edge only when no rexenv KeepAlive daemon
  is installed (`core/service_manager.rs:565-573`).
- **Shadow-bind marker probe** — `proxy::edge_answers_as_ours` (`core/proxy.rs:71-87`):
  HTTPS GET `/__rexenv-probe` pinned to 127.0.0.1:443, positive ID = our
  `X-Rexenv-Edge` response header. Deliberately marker-only (a `Server: Caddy` fallback
  was removed as a false positive against a developer's own Caddy). This exists because
  Herd binds `127.0.0.1:443` specifically, out-specificing our `*:443` bind — **no bind
  error anywhere, and Herd answers every site**.
- Runs at Start-all phase 5 (`commands/services.rs:139-176`), in the ~10s health
  watchdog (`edge-blocked` / `edge-unblocked` events, `service_manager.rs:1893-1932`),
  and in `rex doctor`.
- Message text (holder named dynamically from the executable path, so Herd shows as
  "Herd"): *"the edge is running, but {holder} answers port 443 in front of it — every
  site is unreachable until you quit {app}"* + a copy-paste `$` fix — for an app-managed
  holder that's `osascript -e 'quit app "Herd"'`; for Valet's nginx it resolves to the
  brew-services form (a Valet user never sees the word "Valet" — acceptable, could be
  special-cased later). The Services screen folds `edge_blocked` into Caddy `running:
  false` so a green row can't lie, and every site's `serving` flips false.

**Honest UX: import while Herd runs (the scan is pure file reads); SERVING requires the
user to quit Herd. We never stop it.** Two gaps to fix as part of this feature:

1. Nothing gates site creation on `edge_blocked`, and the provision job reports
   "created — serving at https://…" (`site_provision.rs:811`) while Herd is actually
   answering — truth only arrives at the next watchdog tick. The migration flow must
   probe `edge_answers_as_ours` explicitly and show "imported — serving paused until you
   quit Herd" instead of a false success.
2. Onboarding does no 443 probe at all.

### b) `/etc/resolver/<tld>` — a real bug to fix first

**Today's code silently overwrites a foreign resolver file.** `dns::ensure_resolver`
(`core/dns.rs:277-282`) skips only when the file content is EXACTLY ours (`nameserver
127.0.0.1` + `port 15353`). Valet's file (no port line) doesn't match → it falls through
to `configure_resolver` → privileged `printf > /etc/resolver/test`. No foreign-content
check, no consent, generic admin prompt. The content-signature ownership test protects
only the UNINSTALL sweep (foreign files are never removed — that half is correct, and
B10 additionally requires a valid TLD label before any privileged `rm`).

Importing a `.test` site would hit this on first use. **Fix the write path regardless of
this feature** (same care class as B2/B10 — it's a ROOT op on a file we don't own).

Honest context that shapes the UX, not an excuse:

- Their resolver file without their dnsmasq is a dangling pointer — it routes `.test` to
  127.0.0.1:53 where nothing answers. Quitting Herd (required to free `:443`) kills its
  dnsmasq, so `.test` goes dark machine-wide with their file still in place. **That
  was the reference install's live state**: no `/etc/resolver/test` at all, both
  dnsmasqs stopped, every `.test` site unresolvable.
- Both tools self-heal on their own terms: `valet install` idempotently rewrites resolver
  + dnsmasq (officially the reset path), and Herd's onboarding/helper recreates its
  state. Neither repairs continuously — Valet's `start`/`restart`/`status` never touch
  the resolver.
- Valet's own uninstaller deliberately LEAVES `/etc/resolver/<tld>` when Herd.app exists,
  first-party confirmation the file is shared.

**Design:** never overwrite silently. In the migration flow, offer a consent-gated
takeover — show their file content vs ours, back up their copy on OUR side, explicit
checkbox, and state plainly that Valet/Herd will recreate it on their next
install/onboarding. Refusal path = re-home those sites to `.rex`. Absent file (this
machine) = a plain install, no conflict. **We never delete or "clean up" their file.**

### c) TLD choice — keep `.test` (recommended), `.rex` as the fallback

Keeping `.test` is the least disruptive (bookmarks, `.env` URLs, API endpoints, OAuth
callbacks keep working) and is fully supported today:

- `default_tld` is only the default for NEW sites; each site's TLD lives in its domain
  (`core/sites.rs:677-702`). Importing `foo.test` needs no global change.
- `.test` is in the never-blocked RFC 2606/6761 safe set (`core/tld.rs`).
- Embedded hickory answers ANY name — scope comes solely from which `/etc/resolver/<tld>`
  files exist; adding a TLD needs no DNS restart (`core/dns.rs:34-96`).
- Certs and vhosts are TLD-agnostic; `foo.test` coexists with the `.rex` backbone with no
  special-casing (the only `.rex`-specific paths are backbone plumbing: onboarding's
  backbone file, the internal `adminer.rexenv.rex` vhost, the DNS liveness probe name).

Given (b), the per-option truth the UI must state: keep `.test` + consent → everything
works; keep `.test` + refuse → sites import but stay unresolvable once Herd quits;
re-home to `.rex` → no resolver conflict, user updates external references. Serving a
site under BOTH names would need multi-domain support we don't have (domain is UNIQUE,
one per site) — note as future, don't build it here.

### d) PHP versions

**The premise that we lack 8.1/8.2 is stale — we already pin 8.0.30 / 8.1.34 / 8.2.31 /
8.3.31 / 8.4.23 / 8.5.8** (`core/binaries.rs:20-28`; pools 9780–9785). The dominant Valet/Herd cohort maps exactly. Genuinely absent: **7.4**
(static-php.dev never published it — needs a self-hosted build, the same blocked class as
the Xdebug debug build) and anything older.

- Unavailable version → surface honestly per site ("PHP 7.4 not available — import with
  8.0? the site may break"), explicit choice, **never a silent substitute**.
- **Trap 1:** a site row referencing an unpinned version silently serves on the DEFAULT
  pool today (`pool_port_for`, `core/sites.rs:905-911`) — that's exactly the silent
  substitution we forbid, so the importer must never write an unpinned version.
- **Trap 2:** provision's `ensure_php_pool` does NOT mark the registry installed — an
  import that writes `php_version = "8.1"` while 8.1 is uninstalled works until the next
  restart, then 502s. Import must call `php::set_installed` (and prefetch the binary).
- Valet/Herd per-site isolate → our per-site `php_version`: read the nginx marker (3
  formats), map minor→minor. Adding a future static-php-published version is data-only
  (append to `PHP_VERSIONS` + 4 sha consts).

## 4. Detecting each site's shape

**Our own detection, not their driver logic — confirmed as the right call, and nothing
exists to reuse.** `SiteType` (Wordpress/Laravel/Php) is user-chosen and never detected;
docroot is never probed; `wp_info` exists but runs WP-CLI, i.e. executes the user's PHP —
wrong tool for a scan. (Side finding: the Laravel type card promises an installer the
backend doesn't have — the DB branch is "intentionally empty", `core/sites.rs:770-776`.)

The codebase already states the norm to extend — **detection = pure fs, execution =
explicit user action** (`core/repo.rs:1674-1675`); the Logs-tab wp-config reader is the
precedent (static `define()` parse, checks docroot and one level up,
`core/logs.rs:211-252`).

Ordered pure-fs classifier (mirrors the shipped drivers' outcomes without parsing them):

1. `wp-config.php` **or `wp-config-sample.php`** at root → WordPress, docroot = root
2. Bedrock (`composer.json` requires `roots/bedrock-autoloader`, or `web/app/` +
   `web/wp-config.php` + `config/application.php`) → docroot `web/` — wp-config is NOT at
   the served root, which a naive probe gets wrong. Radicle → `public/`
3. `artisan` + `public/index.php` → Laravel, docroot `public/`
4. Craft → `web/`; Symfony/Statamic → `public/`; Magento → `pub/`; Drupal →
   `docroot|public|web`
5. `public/index.php|index.html` → generic public/
6. root `index.php` → Php; root `index.html` only → Php **plus a flag** (our nginx
   template is PHP-oriented; pure-static needs an `index index.html` check — small gap)
7. else → **needs attention** + manual docroot picker

Where our detection cannot match Valet's truth — flag these sites, don't guess:
`LocalValetDriver.php` in the project root or any `~/.config/valet/Drivers/*ValetDriver.
php` (arbitrary PHP choosing the docroot), and `.valet-env.php` (per-site `$_SERVER`
injection). Also honest gaps: Valet's `default` config key (catch-all site) and its
`/storage/*` URI mapping for Laravel.

## 5. Databases — discovery and per-site mapping

Valet manages no databases at all; users run DBngin, Homebrew, or Docker. Herd Pro
bundles its own.

**Live scan of the reference install:** the only DB servers running were OURS (mysqld
8.4.6 on 13306, mariadbd 12.3.2 on 13307). DBngin was installed with one engine —
a MySQL 8.0.x **on 3306, stopped** — and its plist said `Status = "started"`.
**Never trust DBngin's plist; trust `lsof`.** No brew DB service running, no Herd Pro
services, no Docker. The sampled WordPress site pointed at `DB_HOST=127.0.0.1`
(i.e. TCP to the stopped DBngin), user `root`, password present.

Discovery plan (read-only): enumerate DBngin `Data/DBEngines.plist` (engine, version,
port, datadir), Herd `config/services`, `brew services list`; then verify each against
live listeners and confirm engine identity from the connect handshake (`8.0.27` vs a
`-MariaDB` suffix). **We never start their server.** A stopped source DB is a per-site
status: *"database not reachable — start MySQL in DBngin, then re-scan."*

Per-site mapping lives in the SITE's config, and is reliably readable statically:

- **WordPress** → extend the existing static parser to `DB_NAME`, `DB_USER`,
  `DB_PASSWORD`, `DB_HOST` (host:port form), `$table_prefix`. Handles the WP-standard
  `define( 'K', 'v' );` (the reference install's sample was exactly that). Non-literal/env-driven defines
  (Bedrock) → the `.env` path or needs-attention. **Never execute wp-config.**
- **Laravel** → new conservative `.env` line parser: `DB_CONNECTION`, `DB_HOST`,
  `DB_PORT`, `DB_DATABASE`, `DB_USERNAME`, `DB_PASSWORD`; tolerate quotes, comments,
  `export` prefixes; refuse on duplicate keys or multiline values → needs attention.
- **Anything else** (other frameworks, plain PHP) → honest fallback "needs attention":
  the user either enters connection details manually (then we can enumerate databases
  with their credentials and let them pick) or imports the site without a database.

**Secrets handling, stated plainly.** Reading wp-config/.env means reading secrets.
They live in memory for the duration of the job only; **never persisted** (the site row
stores db name + engine, nothing else); **never logged** (job logs stream child output
only, and no redaction layer exists today — so the rule is don't put secrets where logs
look); **never on argv** — passed to the dump tools exclusively through a
`--defaults-extra-file` created **born 0600** via the B6 helper
`PermissionManager::write_private` (created-with-mode, never write-then-chmod;
`platform/traits.rs:249-253`) and deleted after the job; Postgres via `PGPASSFILE`
(libpq ENFORCES 0600 — a 0644 file is warned about and ignored, verified) plus
`PGCONNECT_TIMEOUT`. Note for completeness: our "never argv" contract is a CLI-surface
rule — WP-CLI subprocesses already receive `--admin_password=` internally today; the
rewrite stage (§8) avoids the issue entirely because it only ever touches DB_HOST.

## 6. Engine and version compatibility

Two findings reshape the pessimistic matrix:

1. **We bundle both MySQL (8.4.6 / 8.0.44) and MariaDB (12.3.2 / 11.4.12)**, so the
   default policy is a SAME-ENGINE target and the scary cross-engine cases mostly never
   fire.
2. **Client pairing is mandatory: our bundled MariaDB clients CANNOT authenticate to
   MySQL 8 at all.** `caching_sha2_password` is a dynamic plugin `.so` our bottle bundle
   deliberately excludes → verified live: `ERROR 1156 (08S01): Plugin
   caching_sha2_password could not be loaded`. Use MySQL-branded tools for MySQL sources,
   MariaDB tools for MariaDB sources. (This also means the bundled mariadb client can
   never talk to our own MySQL — worth knowing beyond this feature.)

Folklore corrected by direct verification against the bundled binaries:

- The mariadb-dump sandbox line is now `/*M!999999\- enable the sandbox mode */` (Aug
  2024+; both our versions emit it — byte-verified) and **MySQL parses `/*M!` as a plain
  comment**. Only dumps from the seven May-2024 MariaDB releases carry the breaking
  `/*!999999` form (`ERROR at line 1: Unknown command '\-'`). Scan line 1; strip only
  that form.
- **MariaDB ≥ 11.4.5 aliases the 0900 collations** (MDEV-35256; our 12.3.2 verified
  round-tripping `utf8mb4_0900_ai_ci`). "Unknown collation" only bites MariaDB targets
  < 11.4.5 — not ours. For old targets remap to `utf8mb4_uca1400_nopad_ai_ci` (NO PAD
  matches 0900 semantics; `utf8mb4_unicode_520_ci` / `general_ci` below 10.10).
- mariadb-dump emits `STORED`, not `PERSISTENT` (verified) — the generated-column scare
  only applies to ancient or handwritten DDL.

| Source → our target | Verdict | Why |
|---|---|---|
| MySQL 5.7 / 8.0 → our MySQL 8.0.44 | OK | closest series; dumping 5.7 needs `--column-statistics=0` (defaults ON in our 8.0.44 and 8.4.6 — verified) |
| MySQL 8.0 → our MySQL 8.4.6 | OK + scan | scan for `mysql_native_password` (disabled by default in 8.4, removed in 9.x) and 5.7-origin `NO_AUTO_CREATE_USER` sql_mode blocks around triggers/routines |
| MySQL 9.x source | warn / refuse | newer than anything we bundle — a downgrade restore |
| MariaDB 10.x / 11.x → our 11.4.12 or 12.3.2 | OK | same engine, our targets ≥ common sources |
| Postgres ≤ 18 → our matching-or-newer series | OK | pg_dump must be ≥ the server major (a 16 pg_dump REFUSES an 18 server — verified); restore with psql ≥ the dump's writer (PG 18 / 17.6+ plain dumps carry `\restrict` meta-commands that older psql rejects — verified) |
| MySQL → MariaDB (cross) | warn, opt-in | definers + feature drift; unnecessary, a same-engine target exists |
| MariaDB → MySQL (cross) | warn / refuse | Aria `PAGE_CHECKSUM=1 TRANSACTIONAL=1` in real dumps → MySQL 1064 (verified); sequences; system versioning |
| Any downgrade (source series > our best) | warn / refuse with override | state "may fail or degrade" honestly |

Universal dump hygiene: `--set-gtid-purged=OFF` (a GTID-enabled source otherwise emits
`SET @@GLOBAL.GTID_PURGED`, which hard-fails a fresh target with ERROR 1839); dump ONE
database (never the `mysql` schema → no user-table or Aria noise); then cheap-grep OUR
artifact before restore (sandbox form, `IDENTIFIED WITH mysql_native_password`,
`NO_AUTO_CREATE_USER`, `DEFINER=`) and surface findings per site. Neither mysqldump nor
mariadb-dump has any `--skip-definer` (verified) — we restore as root, so orphan definers
become use-time warnings; warn, don't block.

## 7. Dump / restore mechanics

Verified empirically against the bundled binaries (not assumed from the clients' flags):

- **`--connect-timeout` is accepted NOWHERE by the dump tools** — argv, `[client]`, and
  `[mysqldump]` defaults-file groups all hard-error `unknown variable` (exit 7; verified
  on mysqldump 8.0.44/8.4.6 and mariadb-dump 11.4.12/12.3.2). The interactive
  `mysql`/`mariadb` clients DO accept it. So: **preflight the connection with the
  interactive client + `--connect-timeout=10`** (exactly today's `client_base_args`
  pattern), then dump with an unbounded transfer — the B25 rule holds: bound the CONNECT
  phase, never a wall-clock cap on the transfer. Dead loopback ports refuse instantly
  (verified 0.02s); the hang class is remote/filtered hosts, which local migration lacks.
  Postgres: `PGCONNECT_TIMEOUT` (verified ~2.0s at value 2) or `connect_timeout` in the
  conninfo.
- **`--defaults-extra-file` must be the FIRST option** — verified fatal otherwise on both
  vendors. `--result-file` / `-r` exists on both dump tools (no shell redirection; app-
  data paths contain spaces). `MYSQL_PWD` is deprecated in 8.4 — don't use it.
- **Progress:** reuse the job/streaming machinery behind the install + provision cards —
  the recipe is codified: registry (`Mutex<HashMap<id, entry>>` + seq) managed in
  `lib.rs`, `repo::CancelToken` (process GROUP kill), `<ns>://state|output/<id>` events
  carrying whole snapshots, per-scope busy refusal (refuse, don't queue), pruned per-job
  logs, an outer timer that records `timed_out` FIRST then cancels, and the honest-
  progress contract (monotonic, real signals only, ≤99 until settle, frozen on
  failure/cancel). The real progress signal for a dump is byte growth of the
  `--result-file` output (the tools' stderr is silent by default) — the same shape as the
  download Hub's real-bytes fraction. Never a fabricated ticker.
- **Naming collisions:** generalize B21 — check the imported name against BOTH
  `store::db_name_exists` (site-owned) and a live `SHOW DATABASES`; on collision use the
  disambiguated form (`unique_db_name`, `core/sites.rs:201-217`), never a SQL UNIQUE
  constraint (a migration on collided data would brick the DB — the B18 class), and
  **never overwrite an existing rexenv database without explicit typed confirmation**
  (the UI convention already exists).
- **Failed restore:** cleanup is safe because it's OUR database in OUR engine — but only
  drop what THIS run created. No DB-provenance ledger exists today; record a job-state
  "created_db" flag BEFORE `CREATE DATABASE` (the `sites.provisioned` set-0-then-1-on-
  settle shape). If the name pre-existed, never drop — leave it and report. On their
  side a failed dump deletes only our partial artifact (existing behavior).
- **Credential mirroring (new, and it's what makes §8 cheap):** after restoring as root,
  `CREATE USER 'their_user'@'localhost' IDENTIFIED BY <their_pass>` + `GRANT ALL ON
  <db>.*`, fed over **stdin SQL, never argv**. Their existing credentials then work
  against our engine — only host/port differ.

Reuse skeleton that already exists: `core/database.rs` (`create_database`,
`drop_database`, `import_from_file` stdin feed, `export_to_downloads` with partial-file
cleanup), `DbEngine::sql_client_bins` / `cached_sql_client`, the fail-fast
`engine.running()` guard, and `delete_site`'s bring-up recipe (`datadir_initialized` →
prefetch BEFORE the services lock → `spawn_db` → `await_ready` → op).

## 8. The decision: connection-config rewrite

After import the site still points at their old DB (127.0.0.1:3306, their credentials)
while ours is on 13306 — so it won't work until its connection config changes. That
collides with "we never rewrite the user's project files."

**Recommendation: option (c) — per-site opt-in, backup, diff shown first — with one
reframe that shrinks the risk: mirror their credentials into our engine (§7), so the
rewrite becomes a ONE-KEY change (`DB_HOST`, plus `DB_PORT` for Laravel). No password
ever appears in an edit, an argv, or a diff.**

What each option actually involves:

- **(a) Auto-rewrite.** WordPress: `wp config set DB_HOST 127.0.0.1:13306` — verified
  available in bundled WP-CLI 2.12; runs on `before_wp_load`, does a string transform,
  does NOT load WordPress or the site's code. Clean for literal defines; useless for
  Bedrock/env configs. Laravel `.env`: a key-level line edit is reliable only if
  conservative (byte-preserve everything else, append `DB_PORT` if absent, refuse on
  duplicates/multiline → needs attention). Two real hazards: `.env` edits are invisible
  under `php artisan config:cache` (tell the user to `config:clear`; we never run their
  artisan), and any in-project write shows up in their git status. **Violates the rule as
  a default — rejected.**
- **(b) Tell-only.** Zero risk and the permanent floor: a per-site copy-paste snippet.
  Keep it for refused consent and needs-attention shapes, but alone it undercuts one-click
  migration.
- **(c) Opt-in + backup + diff.** Unchecked by default; shows the exact 1–2 line diff
  before touching anything; backup stored on OUR side (app-data, one-click revert — keeps
  stray `.bak` files out of their working tree); applied via the wp-cli transformer (WP)
  or the conservative line edit (.env). **Agreed — this is the right shape.**

**Zero-rewrite check (asked rather than assumed): mostly no, with one narrow exception.**
Serving our DB where their config already points would mean binding 3306 — only possible
while their server is stopped, broken the moment they restart it, and against the fixed-
port model. Rejected. BUT: PHP treats `DB_HOST=localhost` as "use the unix socket", and
our static PHP builds load NO php.ini — so `php_admin_value[mysqli.default_socket]` /
`[pdo_mysql.default_socket]` in our generated pools, pointed at our `run/mysql.sock`,
plus mirrored credentials, would make a `localhost` WordPress site work with ZERO file
changes. The seam exists (`generate_fpm_config`, `core/services.rs:26-81`) and our
servers already create sockets (`run/mysql.sock`, `run/mariadb.sock`); the plumbing does
not exist today (`default_socket` is set nowhere). Narrow: only `DB_HOST=localhost` sites
(the sampled config used `127.0.0.1` — TCP, unaffected), one engine's socket per shared
pool, nothing for Laravel's TCP default, and mind macOS's ~104-char socket path limit.
Worth doing as a free win IN ADDITION to (c), not instead of it.

## 9.–12. Feature shape, staging, decisions, folded-in bugs (shipped — see the build record)

These four sections described what to build and in what order. All of it shipped
(Stages 0–3, 26–28 Jul 2026) and is now recorded where current truth lives:
`docs/ARCHITECTURE.md` §8 (the as-built model: docroot ownership, resolver
takeovers, db import, connection rewrite), the four stage plans
(`PLAN-linked-sites.md`, `PLAN-valet-herd-import.md`, `PLAN-valet-herd-db-import.md`,
`PLAN-valet-herd-rewrite.md`), and the per-commit evidence log
(`docs/archive/SHIPPED-2026-07.md`). The two §12 pre-existing bugs (foreign-resolver
silent overwrite; false "serving at …" under a shadow-binding proxy) were fixed
inside Stage 1 and the provisioning work respectively.

## Appendix — verification status

**Verified live on the reference install (read-only):** the Valet + Herd trees and their
real config/conf/cert contents; the ~30-site inventory and its quirks; `/etc/resolver` state;
every listening service and its owning process; DBngin's stale `Status`; the bundled
binaries' `--version`, `--help` flag surfaces, `--connect-timeout` rejection (exit 7),
`--defaults-extra-file` first-arg rule, dead-port timings, `PGCONNECT_TIMEOUT`, the
0644-pgpass warning, the `/*M!` sandbox form, the 0900 collation round-trip, the
MariaDB-client → MySQL 8 auth failure, `STORED` emission, Aria DDL rejection, the pg_dump
version-mismatch error, `\restrict` in PG 18 plain dumps, and `wp config set --help`.

**Read in source:** every rexenv claim carries a `file:line`; laravel/valet at tag
v4.12.0 (plus v2/v3 tags for the churn map).

**Docs / secondary (marked as such above):** Herd internals beyond what's on disk
(closed source), Herd Pro service layout, MySQL/MariaDB/Postgres release notes and MDEV
tickets for the compatibility matrix.

**Inferred at research time, since PROVEN with fixtures:** scan behaviour against
Bedrock/Radicle/Craft-shaped trees (detector fixture matrix in `core/sites.rs` tests)
and Laravel `.env` parsing (`core/dbimport.rs` tests + `examples/valet_import_check`
runs a fixture Valet tree with a Laravel project).
