# Stage 3 — the opt-in connection rewrite

**Status:** APPROVED 27 Jul 2026 — all five decisions settled (§9). Stages 0–2 shipped;
the research is `PLAN-valet-herd-migration.md` §8 (Q7, decided as option (c)), the
interim state this replaces is `PLAN-valet-herd-db-import.md` §9.

**Scope:** after a database import, change the site's own connection config — per-site,
opt-in, backed up, diff shown first — so the site actually reads the rexenv copy. This
is the ONE place in the whole migration where we write inside the user's project, and
everything below is shaped by that.

The standing rules still bind: the rewrite touches exactly the file it names, backup
before write, diff before consent, consent unchecked by default, tell-only as the
permanent floor. The SOURCE environment remains read-only forever — this stage writes
into the user's project (with consent), never into Valet/Herd/DBngin.

---

## 1. The root case, analysed — D2's deferred decision

Their config connects as `root` with a password; ours is passwordless root, and Stage 2
correctly refused to mirror onto root (passwording our root breaks `client_base_args`
and with it every DB operation rexenv performs — settled, not revisited).

### What each option actually shows in the diff

**3-key version** (host, user → `root`, password → empty). The diff the user reviews:

```diff
-define( 'DB_HOST', '127.0.0.1' );
+define( 'DB_HOST', '127.0.0.1:13306' );
-define( 'DB_USER', 'root' );
+define( 'DB_USER', 'root' );        (unchanged in the root case — shown for the .env shape)
-define( 'DB_PASSWORD', 'hunter2' );
+define( 'DB_PASSWORD', '' );
```

**Their password, plaintext, in a review UI.** On screen during review, in any
screenshot of it, potentially in a screen-share while asking a colleague "is this
right?". The person reviewing may not be the person who owns the secret. This is not a
storage problem — it's a *display* problem, and it's inherent to the option: you cannot
show a password line changing without showing the password.

**2-key version** (host, user → a dedicated account holding THEIR password):

```diff
-define( 'DB_HOST', '127.0.0.1' );
+define( 'DB_HOST', '127.0.0.1:13306' );
-define( 'DB_USER', 'root' );
+define( 'DB_USER', 'rex_ea_test' );
```

The password line is never touched, so the diff cannot contain a secret. Stronger than
"doesn't": **the rewrite-plan type has no password key in its vocabulary** (§3), the
same shape as `DbConnectionInfo` — the writer can't emit a password change, so no diff
it produces can ever show one, whatever future code does.

### Is the dedicated user genuinely no worse? The honest ledger

- **It's another account.** Loopback-scoped (`localhost` + `127.0.0.1`, the Stage 2
  two-host list — `'%'` structurally impossible), granted on ONE database, holding a
  password the user already chose for local dev. Surface added: an account that can
  read one local database from the local machine. Acceptable, and smaller than it
  sounds.
- **Naming: per-SITE, not per-database — this is the one real trap found in the
  analysis.** If two sites share one database and both get a dedicated user named after
  the *database*, the second site's `ALTER USER` (the idempotent converge) would reset
  the password to ITS config's value and silently break the first site's sign-in. Named
  after the *site* (`rex_<domain-slug>`, truncated + hashed to MySQL's 32-char cap),
  the accounts can't fight: one site, one account, one password to converge.
- **Re-import:** `dbmirror::mirror` is already idempotent (IF NOT EXISTS + ALTER +
  GRANT); re-running converges the password to the config's current value. No change.
- **Site delete:** the account is dropped iff we created it — recorded, not derived:
  the `db_imports.mirrored_user` column already exists and is the record. Same
  provenance discipline as `db_created`. (Non-root mirrored users from Stage 2 get the
  same cleanup, which Stage 2 left dangling — a small fix this stage carries.)
- **Second credential to keep straight:** it lives in exactly one place — their config
  file, where their old credential already lived. rexenv stores the *username* (in
  `db_imports`), never the password (unchanged rule). Nothing new to keep straight on
  our side.
- **The backup still contains their password either way.** The whole-file backup (§4)
  necessarily holds the original config vertbatim, password included — 0600, our side,
  needed for revert. The dedicated user removes the secret from the *diff*, not from
  the *backup*; saying otherwise would be overclaiming.

**Recommendation: the dedicated user (2-key), per your lean** — the display argument is
decisive and nothing in the ledger outweighs it. The per-site naming detail is the one
thing that would have bitten later.

(For completeness: non-root configs were already mirrored in Stage 2, so their rewrite
is **host/port only** — the best case, one or two keys, no user change at all.)

---

## 2. The shape — (c) with (b) as the floor, plus the socket win

- **(c) per-site opt-in**: a "Connect to the rexenv copy" action on the site's Database
  tab (beside the Stage 2 interim panel it replaces). Shows the exact diff, backup
  noted, checkbox **unchecked by default**, applies only what the diff showed.
- **(b) tell-only stays the permanent floor**: refused consent, needs-attention shapes
  (duplicate keys, computed defines, multi-line values), and anything the editor
  refuses (§3) keep today's copy-paste block. A refusal downgrades to (b), never to a
  guess.
- **The `mysqli.default_socket` free win — an ADDITION, not a replacement**, with the
  two confirmations asked for stated up front:
  1. **It cannot change behaviour for existing rexenv sites.** Our own provisioning
     writes `DB_HOST=127.0.0.1:13306` (`core/wordpress.rs` — `host:port`, TCP);
     `localhost` (the socket path) appears in no config we generate. Sites that would
     be affected are exactly the sites for which the setting is the fix: imported
     `DB_HOST=localhost` sites, which today fail (our static PHP loads no php.ini, so
     the compiled default socket path points at nothing). Strictly additive.
  2. **A shared pool can point at ONE engine's socket — stated, not discovered.** The
     pools are per-PHP-version, shared across sites; `php_admin_value` is pool-level.
     We point it at **MySQL's** socket (the default engine). A `DB_HOST=localhost`
     site on MariaDB does not benefit and the docs/UI say so ("use 127.0.0.1:13307");
     it is not silently half-supported.

  Small and self-contained → its own step with its own live check (a localhost
  wp-config against a pool with the setting, served and connecting).

---

## 3. What we write, exactly — and what refuses

**The `RewritePlan` type is the guarantee.** It has fields for `host`, `port`, and
`user` — **there is no password field**, so no code path can stage, apply, or diff a
password change. The root case is handled by creating the dedicated account to match
the config, never by editing the config's password to match an account.

- **WordPress** → **our own value-span editor, NOT `wp config set` — settled 28 Jul
  2026** (superseding the wp-cli choice above, approved on review of step 3): wp-cli
  would make the preview a reconstruction while the written bytes carry wp-cli's own
  formatting — exactly the approved-vs-written gap the diff contract exists to close.
  The parser already refuses every shape wp-cli would have hedged for (non-literals,
  conditional duplicates, heredocs), so what remains is literal defines, where
  replacing the span inside the quotes is byte-exact and previewable. Bonus: one
  fewer subprocess, one fewer place to reason about argv and secrets. Refusal path
  unchanged: exotic shapes downgrade to tell-only with the reason.
- **Laravel/`.env`** → our conservative key-level editor: byte-preserves every other
  line (including line endings), rewrites only the value of an existing literal key,
  appends `DB_PORT` if absent. **Refuses** — to tell-only, with the reason — on:
  duplicate keys, multi-line/unterminated quotes, `${VAR}` interpolation on a target
  key, and a key present in the file more than once even with equal values (an edit
  would have to choose which). Same `Unreadable` vocabulary as Stage 2; one parser
  (`core::phpconf`) still.
- **Bedrock and friends**: the `.env` path (already how Stage 2 read them).
- Anything else → tell-only.

Write discipline: read → build plan → show diff (computed from the plan, not from a
dry-run file) → on consent: **backup first (§4), then write via temp-file + atomic
rename in the same directory**, then verify (§6). Their file is never half-written.

---

## 4. Backup and revert

- **Location:** `<app-data>/config-backups/<site-id>/<filename>` (their tree is never
  littered). 0600 — it contains their password (§1).
- **First backup wins.** The backup's meaning is "the file as it was before rexenv
  ever touched it". A second rewrite (e.g. after a revert-and-reconsider, or a port
  change) must NOT overwrite it with the already-rewritten content — that would
  destroy the only true original. By construction: the backup is written only when
  none exists for that site+file.
- **Recorded, not derived — v21 `config_rewrites`:** `site_id`+`file` PK, `backup_path`,
  `written_at`. The row and the backup file are created together and removed together
  (the resolver-takeover pattern — one backup per site+file BY NAMING, so an orphan
  set is bounded and enumerable; `sweep` reports, never auto-deletes).
- **Revert = restore the backup file, delete backup + row, clear the `connected` fact**
  (state returns to `imported`, the interim panel returns). One click, from the same
  card.
- **Lifecycle end:** revert consumes it; site delete removes it (with the §9 D3
  question answered first); uninstall removes it and the teardown report says so.

---

## 5. The Laravel config cache — prominent, because the symptom imitates our failure

`php artisan config:cache` makes Laravel read `bootstrap/cache/config.php` and ignore
`.env` entirely. We never run their artisan, so this is copy, not code — but an
applied edit + a cached config looks EXACTLY like "rexenv's rewrite didn't work": diff
applied, file correct, site still on the old database.

So for Laravel-shaped sites the post-rewrite panel leads with it, conditionally on
evidence: if `bootstrap/cache/config.php` **exists** (a read, which we're allowed),
the message is not a footnote but the first line — "this site has a cached config;
the change won't take effect until you run `php artisan config:clear`" — and the
connected-verification (§6) is annotated accordingly. If the cache file doesn't
exist, one quiet sentence suffices.

---

## 6. When "connected" becomes a fact — and what actually proves it

The `db_imports.state` closed set finally gains its second value. The discipline that
kept the badge honest in Stage 2 stays: **`connected` is set only by the rewrite job,
from a verification it ran — never inferred from "the write succeeded".**

What we can prove without executing their code, in increasing strength:

1. **The write landed** — file contains the new values. Proves nothing about the site.
2. **The rewritten settings authenticate** — take host/port/user from the file as
   re-read (+ the password only in memory, Stage 2 rules), open a connection with the
   bundled client, `USE <db>`. Proves the config now describes a working connection to
   the rexenv copy. This is the gate for `state='connected'`.
3. **The running site answers without a database error** — an HTTP probe of the site
   through our own stack (the server executes the site as it normally would; WE
   execute nothing). A WordPress site that can't reach its database serves "Error
   establishing a database connection". Body-sniffing is heuristic (localised strings,
   custom error pages), so this is a **supplementary signal**: recorded when observed,
   never required, never able to un-set `connected`.

**The honest limit, stated in the record rather than papered over:** without executing
their code we cannot prove the *runtime* reads the file we rewrote (config caches,
exotic bootstraps, a second config include). So the serialized fact carries HOW it was
verified: `connected { verified: "signin" | "signin+http" }`, the summary says
"verified: the rewritten settings sign in to the rexenv copy" — a true sentence — and
for Laravel-with-cache the §5 warning rides beside it. The badge flips to
"DB connected" reading the same one row as ever; nothing renders `connected` by
inference, because nothing else can write the value.

---

## 7. The self-source guard, closed for real

After a rewrite, the site's config points at our engine — so every later scan and
re-import must land in the right branch:

- Engine RUNNING: `read_connection` → `127.0.0.1:13306` → probe → handshake matches →
  `server_is_ours` (two facts) → `classify_self_import` → the site's own `db_name`
  (recorded by `finish` in Stage 2) → **`SelfImport::ThisSite`** → "this site already
  reads and writes `ea` on rexenv's own engine — nothing to import". Already built;
  Stage 3 adds the assertion: the rewrite example ends by re-running the classification
  on the rewritten config and asserting the `ThisSite` branch.
- **Engine STOPPED — the message gap this review found.** `server_is_ours` requires a
  live handshake (correctly — ownership needs both facts), so a rewritten site with our
  engine stopped would today read "database not reachable — start it (DBngin, Herd, or
  however you run it)": the wrong app, on our own port. Fix: when the unreachable
  host:port is loopback on a **configured rexenv engine port**, the message becomes
  "that's rexenv's own MySQL port — start MySQL from the Databases page". A message
  hint only (the port table is static config, fine for wording); the structural
  ownership claim still requires the handshake.

---

## 8. Build order

1. Decisions (§9) → record.
2. v21 `config_rewrites` + `connected` value in `db_imports.state` (+ `verified`
   column) + store fns; badge/summary/TS types read the extended closed set.
3. `core::confedit` — `RewritePlan` (no password field), the `.env` editor
   (byte-preserving, refusal vocabulary), diff builder. Heavy unit tests: CRLF,
   BOM, duplicate keys, `${VAR}`, appended `DB_PORT`, quote styles, the
   password-key-unrepresentable pin.
4. Dedicated-user creation for root configs (per-SITE name, `dbmirror` reuse with the
   reserved-list untouched) + drop-on-site-delete for recorded mirrored users.
5. The rewrite job: backup-first-wins → temp+rename write → sign-in verification →
   `connected` → revert path. Cross-guards with provision/db-import as in Stage 2.
6. UI: the diff/consent card (replaces the interim panel when it settles), revert,
   badge flip, tell-only floor, the §5 cache warning.
7. `mysqli.default_socket`/`pdo_mysql.default_socket` pool addition + live check.
8. Examples: `config_rewrite_check` (fixture project, sandbox stack: rewrite → verify
   → revert → byte-identical original restored → re-scan lands `ThisSite`);
   recovery per the Stage 1 lesson (a failed write must leave their file untouched —
   temp+rename makes a half-written file unrepresentable, asserted).
9. `PUBLISH-TESTING.md` §J — packaged pass on ea.test (root case → dedicated user,
   diff shows 2 keys and no secret), revert restores byte-identical, drift check now
   runs the OTHER way (edit through the site → appears in rexenv's copy).

---

## 9. Decisions — SETTLED (27 Jul 2026)

**D1 — the root case: dedicated per-SITE user** (`rex_<domain-slug>`, 32-char capped)
holding their config's password — the 2-key rewrite. Per-site, NOT per-database,
because per-database naming would let two sites sharing one database break each
other's sign-in via the `ALTER USER` converge (§1) — the one real trap in the
analysis. `RewritePlan` carries no password field, so the writer physically cannot
emit a password change: no secret can appear in any diff, unrepresentable rather than
avoided. The whole-file backup still contains their original password — that limit is
stated plainly in the UI wherever the backup is mentioned, not glossed.

**D2 — site delete vs a rewritten config: offer "revert the connection change, then
delete" as the DEFAULT button, never a silent auto-revert.** Plain delete stays
available. The confirm must NAME both outcomes, not just label the buttons: reverting
first means their config points back at the old database and our copy is dropped;
deleting without reverting means their config points at a database that no longer
exists and the site breaks on next load.

**D3 — revert does NOT un-mirror.** The dedicated user (or Stage 2's mirrored user)
stays on our engine: inert, loopback-scoped, and re-connecting later reuses it. It is
dropped at site delete via the recorded `mirrored_user` — recorded, not derived.
(Stage 2's dangling non-root-user cleanup ships with this.)

**D4 — the HTTP supplementary check: INCLUDED**, with these exact semantics:
supplementary, never required, never able to un-set a signin verification, and a
failed probe reads as "couldn't confirm over HTTP" — never as "not connected".
Passing upgrades `verified` to `signin+http`.

**D5 — the socket win: MySQL's socket on every pool** (the default engine).
Per-pool-majority rejected — mutable derived state deciding runtime behaviour. The
MariaDB-on-localhost limitation surfaces in that site's OWN rewrite panel as the
tell-only instruction ("use 127.0.0.1:13307"), not only in this plan.

**Also agreed:** `connected { verified: "signin" | "signin+http" }` carries its own
limit in the record, and the UI wording matches what was actually proven — "the
rewritten settings sign in to the rexenv copy" is true; "the site is now using this
database" is not something we verified (§6).
