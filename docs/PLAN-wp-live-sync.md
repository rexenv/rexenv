# WordPress live ↔ local sync — clone a live site, pull and push

**Status:** PLANNED 9 Oct 2026 — not started. Owner request ("local staging system"); this
plan was written by the agent while the owner was away, so every decision in §2 is a
recommendation waiting for the owner's yes (the open questions are §11). **Owner answered
9 Oct 2026:** the plugin ships as an in-app zip only for v1, and a push always needs a
human click (never MCP). See §11. Planned against
`00bc4dd1` (v0.8.13). Task list: §10, mirrored as one row in `docs/TODO.md`.

> **The one-sentence design:** a small **companion plugin** on the live site answers
> signed HTTPS requests from rexenv, and **rexenv does all the thinking** — it pulls a
> manifest, downloads the database and the files that changed, rewrites URLs locally with
> the wp-cli it already ships, and for a push prepares the live-shaped copy LOCALLY and
> hands the plugin finished pieces to swap in, with a backup it can roll back to.

---

## 0. The flow, in the owner's words, made concrete

1. User installs **rexenv Sync** on the live site (`example.com`), clicks *Connect to
   rexenv* in its admin page → it shows a **connection key** (one string: site URL + key id
   + secret).
2. In rexenv: **New site → From a live site** → paste the key → rexenv checks the
   connection, shows what it found (WP 6.6, PHP 8.2, 1.4 GB uploads, 312 MB DB, 3
   plugins with no wordpress.org source…) → creates `example.rex`, downloads everything,
   rewrites `https://example.com` → `https://example.rex`, and the site opens locally.
3. Later, **Pull** (live → local): rexenv brings what changed on live — DB, and only the
   files whose size/mtime/hash differ.
4. **Push** (local → live): rexenv shows what will change on live, warns if live changed
   since the last sync, takes a backup ON live, swaps the new DB/files in, and keeps a
   one-click **Roll back**.

## 1. What already exists (reuse, do not rebuild)

| Need | Already in the tree | Notes |
|---|---|---|
| URL rewrite in a WP DB, serialization-safe | `core/wordpress.rs::rehome_urls_on_copy` (`:2854`), `url_rehome_pairs` (`:2750`), `network_rehome_pairs` (`:2797`) | wp-cli `search-replace --all-tables` + the proof that `siteurl` moved; JSON-escaped `http:\/\/` pairs included |
| Restore a dump into rexenv's engine with provenance | `core/dbrestore.rs` (`record_provenance`, `prepare_target`, `feed`, `verify_complete`) | Retry drops and re-imports, never resumes |
| Honest streamed progress from real bytes | `commands/db_import.rs` phases + `db-import://state/{id}` events | never goes backwards, ≤99 until settled |
| A job card with phases, cancel, retry, per-job log | `commands/site_provision.rs` + `SiteProvisionCard.tsx` | `ProvisionJobs::busy_for` — one job per site |
| WordPress site provisioning (PHP, DB, vhost, cert) | `core/sites.rs::provision_with` | a "from live" site is a WordPress site with a different source for content, like git-site-clone was for Laravel |
| Mail never leaves the machine | `rexenv-mail.php` mu-plugin (`wp_mail_catch.rs`) | a pulled shop cannot email real customers from local |
| Agent consent | `core/agent_access.rs` dial (Read / Changes / Full) | push is a new Destroy-scope op |
| HTTPS client | `reqwest` (already a dependency) | |

**What does NOT exist:** any client that talks to a remote WordPress site; any
companion plugin rexenv distributes; **any secret store** (rexenv has never kept a
credential — `DbConnection` is not even `Serialize`); any site snapshot/backup.

## 2. The decisions (recommendations — owner confirms, §11)

### 2.1 Direction: rexenv always calls out; live never calls in

Every request is **local → live over HTTPS**. Live never connects to the developer's
machine: no tunnel, no open port, no inbound path to rexenv's control sockets (which are
0600 local sockets by non-negotiable). A laptop behind NAT, on hotel Wi-Fi, asleep half
the day — all work, because the laptop is always the client. "Pull" and "push" are both
initiated by rexenv. (Rejected: live pushing changes to rexenv through a tunnel — it
would make the private control plane a public endpoint.)

### 2.2 The companion plugin is dumb; rexenv is smart

The plugin does only what must happen on the server: list, read, receive into a quarantine
area, swap, back up, roll back. **No search-replace in PHP**: URL rewriting always runs in
rexenv with the bundled wp-cli, on a local copy, where it is already proven. That keeps one
implementation of the hardest step, and keeps the plugin small enough to review (and to
pass wordpress.org review — §11 Q1).

Plugin constraints (shared hosting is the target): PHP 7.4+, no `exec`/`shell_exec`
(often disabled), no `mysqldump` binary, `max_execution_time` 30 s, memory 128 MB. So every
endpoint does **bounded work per request** (≈ 8 MB or ≈ 15 s, whichever first) and returns
a cursor; rexenv loops. A dropped request is retried from the cursor, never from zero.

### 2.3 Pairing and auth

- *Connect to rexenv* in the plugin generates a 256-bit secret + key id, stores them in
  its own option row, and shows the key once: `rexsync1:<base64url(site_url|key_id|secret)>`.
- Every request is signed: `HMAC-SHA256(secret, method + path + timestamp + nonce +
  sha256(body))` in headers; the plugin refuses a timestamp outside ±5 min and a nonce it
  has seen (kept 10 min in a transient). Replay-proof without OAuth or app passwords.
- **HTTPS only.** An `http://` live URL is refused at pairing with a sentence saying why
  (a database dump in clear text). No override in v1.
- Disconnect / Regenerate in the plugin admin kills the old key immediately.
- rexenv stores the secret in the OS secret store — **new platform trait** `SecretStore`
  (§3). Never in SQLite, never in a log, never in an IPC payload to the webview after the
  paste.
- The plugin's own option rows (the secret, the nonce cache) are **excluded from every DB
  export** and **preserved across every push** — otherwise a pull would copy the live
  secret into the local DB, and a push would overwrite live's pairing with local's.

### 2.4 Pull (live → local)

1. `GET /manifest` — WP/PHP/MySQL versions, `$table_prefix`, multisite flag, active
   plugins/theme, every table with row count + size + a cheap checksum
   (`CHECKSUM TABLE` where the host allows it, else `max(id)` + row count + `UPDATE_TIME`),
   and a file manifest of `wp-content/` (path, size, mtime) streamed by cursor.
2. rexenv diffs against the **sync base** (§2.6): which tables and files changed.
3. **DB**: `GET /db/export?table=…&cursor=…` — the plugin writes SQL (`INSERT` batches via
   `$wpdb`, `SHOW CREATE TABLE`) in bounded chunks; rexenv appends to a `.partial` file,
   renamed only when the last chunk's sha256 matches the plugin's running hash. Restored
   into a fresh database through `dbrestore`, then `rehome_urls_on_copy` live URL → local
   URL, then the local DB is swapped to that new one (the site's old DB is kept until the
   pull is verified — a failed pull leaves the local site as it was).
4. **Files**: `POST /files/read` with a list of paths → a framed stream (JSON header +
   bytes per file). Only changed files. Written to a staging folder beside the docroot,
   moved into place after all arrive. Deleted-on-live files are deleted locally only inside
   `wp-content/` and only if they were in the previous base (never a local-only file).
5. Scope defaults: whole DB; `wp-content/{themes,plugins,mu-plugins,uploads}`. WordPress
   core is **not** transferred — rexenv installs the same core version locally (cheaper,
   and core is checksum-verifiable). `wp-config.php` is never transferred (it holds live
   credentials); rexenv writes its own.
6. Excluded by default: cache dirs (`wp-content/cache`, `*/cache/`), backup plugins'
   archives (`updraft`, `ai1wm-backups`, `backups-dup-lite`…), `*.log`, the plugin's
   quarantine dir. The list lives in rexenv (one place), sent with each request.

### 2.5 Push (local → live)

1. **Preflight on live**: fresh manifest; compare to the sync base → if live changed since
   the last sync, show WHICH tables/files and stop until the user picks per item (§2.6).
2. **Prepare locally**: copy the local DB to a temp DB, `rehome_urls_on_copy` local URL →
   live URL on the COPY (the local site is never rewritten), dump the chosen tables.
3. **Upload to quarantine**: `POST /push/chunk` into `wp-content/uploads/rexsync-<random
   32 hex>/` with an `index.php` + `.htaccess` deny (nginx hosts ignore `.htaccess`, so the
   random name is the real protection, and the dir is deleted on every exit path).
4. **Import DB into shadow tables** `<prefix>rxnew_*` — never touching live tables yet.
5. **Backup + swap**, one request: maintenance mode on → one `RENAME TABLE live→rxbak_*,
   rxnew_*→live` statement (atomic in MySQL) → files moved into place with the overwritten
   ones moved into the backup dir → plugin option rows restored → caches flushed
   (`wp_cache_flush`, rewrite rules) → maintenance off.
6. **Verify**: rexenv GETs the live home page + `/manifest`; on failure offers Roll back.
7. **Roll back**: `RENAME` back + restore files from the backup dir. One backup kept (the
   last push); the next push replaces it. Disk need on live ≈ 2× the pushed DB — preflight
   checks free space where the host reports it and says so when it cannot.

### 2.6 Conflicts: honest, per table and per file — no magic merge

Two copies of a WordPress DB cannot be merged in general (auto-increment IDs collide,
serialized options, plugin tables with no keys). So rexenv does **not** pretend to merge.
It records a **sync base** after every pull/push (per table checksum, per file
size+mtime+hash) in SQLite, and before every pull or push shows a three-column list:

| Item | Changed on live since base | Changed locally since base |
|---|---|---|

and the user picks, per table / per folder, which side wins. Defaults that protect live:
on push, **tables that only live writes** (`wc_orders*`, `woocommerce_*` sessions,
`comments`, form-plugin entries, `users`/`usermeta`) are **unticked** and carry a line
("orders placed on live since your last pull would be lost"). The list of
"live-owned" table patterns lives in rexenv, in one place.

### 2.7 Creating a local site from live ("New site → From a live site")

A WordPress site with a different source for content (the git-site-clone pattern): the
normal WP provisioning (PHP version = live's major.minor if rexenv ships it, else nearest
with a line saying so; same WP core version; same `$table_prefix`), then the pull phases.
Domain default: live host's first label + `.rex` (`example.com` → `example.rex`), editable.
Multisite: **refused in v1** with the reason (domain mapping, per-blog uploads); Stage 5.

### 2.8 Safety on the local side

- Mail is caught (existing mu-plugin). Cron: `DISABLE_WP_CRON` is set in the local
  `wp-config.php` on EVERY pull (a pulled WooCommerce Subscriptions site would otherwise try
  to renew real subscriptions through real gateways) — built 10 Oct 2026, #839; no toggle.
- The companion plugin pulled into the local copy is **deactivated** locally (a local
  copy answering as a sync endpoint is meaningless and its admin page would confuse).
- Pulled files are content, not code rexenv trusts: nothing in them is executed by rexenv
  itself, only by the site's own PHP like any other site.

### 2.9 Surfaces

- **New Site dialog**: a "From a live site" source (key paste, preview, scope).
- **SiteDetail → a "Live" tab** (only on a connected site): connection status, last
  pull/push, Pull, Push (diff list, then a typed confirmation of the live domain), Roll
  back, Disconnect. History of syncs.
- **CLI**: `rex live <domain> status | pull [--db-only|--files-only] | push --confirm <live-host> | rollback | disconnect`.
- **MCP**: `live` action — `status`/`diff` (Read), `pull` (Full — overwrites the local
  site), `push`/`rollback`: **never exposed** (owner, 9 Oct 2026). A push is the only operation in
  rexenv that can damage something outside this machine, so it stays a human click plus a
  typed domain.

## 3. Three platforms (`docs/PLATFORMS.md` §4)

Common, in `core/` once: the client, signing, manifest diff, pull/push state machines,
the exclusion and live-owned lists, the sync base — all OS-free (`reqwest`, bundled
wp-cli + mysql). The plugin is PHP and runs on the live host — no OS question.

**New trait `SecretStore`** in `platform/traits.rs` (`put`, `get`, `delete` by a
rexenv-namespaced key), all three filled in the same change:
- **macOS** — login keychain generic password (the keychain is already used for CA trust;
  watch the "keychain dialog" class of bug fixed in 0.8.12 — the item is created with the
  app as a trusted app so reads do not prompt).
- **Windows** — Credential Manager (`CredWriteW`/`CredReadW`, `CRED_TYPE_GENERIC`,
  per-user). Proof on the Dell via the installer build, not a dev `rexenv.exe`.
- **Linux** — Secret Service over D-Bus (GNOME Keyring / KWallet). A machine with no
  Secret Service (headless, minimal WM) → `Error::Unported`-style honest refusal with a
  sentence, OR a 0600 file under app-data — §11 Q4. Never a silent plaintext fallback.

Words (`platform/words.rs`, all three): the name of the store in sentences ("your login
keychain" / "Windows Credential Manager" / "your keyring").

## 4. Plugin layout and distribution

- Source in this repo: `companion/rexenv-sync/` (PHP, GPL-2.0-or-later, PHPCS WordPress
  standard, PHPUnit against a real WP in CI). Endpoints under
  `/wp-json/rexenv-sync/v1/…`, each with a `permission_callback` that verifies the HMAC —
  never `__return_true`.
- rexenv ships the built zip as a resource and offers **Download plugin** in the dialog
  (zip saved to Downloads). ✓ 10 Oct 2026: the plugin is compiled INTO the app
  (`core/live_sync/plugin_zip.rs`, so the zip is always the plugin this build speaks to) and
  the button sits on the Live tab's connect form and the New Site live source (#838). wordpress.org listing is §11 Q1 — needs review time; the
  in-app zip works from day one.
- Plugin version and rexenv protocol version (`rexsync1`) are exchanged in `/manifest`; a
  mismatch gives a sentence and the update path, never a half-run.

## 5. Invariants (each a ledger row in the commit that writes its comment)

1. Live is never written to except through `/push/*` and `/rollback`, and `/push/swap`
   never runs without a backup having been made in the same request.
2. URL search-replace runs only in rexenv, on a COPY; the local site's own DB is never
   rewritten to live URLs.
3. The pairing secret exists only in the OS secret store (rexenv side) and its own option
   row (live side); it is never in SQLite, logs, IPC to the webview after paste, a DB
   export, or a pushed table.
4. Every plugin endpoint verifies the signature + timestamp + nonce before reading its
   arguments.
5. A pull never deletes a local file that was not in the previous base; neither side ever
   touches a path outside `wp-content/` (traversal-checked on both ends).
6. A failed pull leaves the local site exactly as it was (old DB kept until verify).
7. Push / rollback are not reachable through MCP — owner ruling 9 Oct 2026, not a v1 limit; a
   test asserts the MCP tool list has no push/rollback action.

## 6. Out of scope (v1)

Multisite networks (Stage 5); WordPress core files and `wp-config.php` transfer; scheduled
or automatic sync; non-WordPress sites; row-level DB merge; pushing to a host that is not
the paired one ("push to another server" = a migration tool, a different product); sites
behind HTTP basic auth (§11 Q5).

## 7. Risks

- **Shared hosting limits** — the reason for the cursor design; WAFs (Wordfence,
  Cloudflare, ModSecurity) may block large POSTs or SQL-looking bodies → bodies are sent
  as `application/octet-stream` and the error for a 403/406 names the WAF as the likely
  cause.
- **Big uploads** (10+ GB) — file sync is incremental after the first pull; the first one
  shows size + an estimate and can be scoped to "skip uploads older than N months" (§11 Q3).
- **Plugin as attack surface on live** — the reason for HMAC + nonce + HTTPS only + no
  endpoint before pairing + a security review (agent-skills `security-auditor` pass + a
  human) before the first release.
- **Data protection** — a pull copies customer personal data to a laptop. The dialog says
  so in one line; an opt-in "anonymise users/orders on pull" is §11 Q6.

## 8. Testing

- L0: signing/verification (both sides, shared vectors file read by Rust AND PHPUnit),
  manifest diff, conflict table, exclusion matching, path traversal refusals, cursor
  resume.
- Plugin: PHPUnit on a real WP + MySQL in CI; a "shared host" profile with
  `disable_functions=exec,…`, `max_execution_time=30`, `memory_limit=128M`.
- L1 example `live_sync_check` (network tier): a fixture "live" site served by a SECOND
  rexenv-owned WordPress on HTTPS with the plugin installed — pull → edit locally → push →
  live changed → roll back → live restored. Fixture-owned everything (`common::sandbox()`).
- Real-host proof (human, `docs/SMOKE-TEST.md` new section): one shared host (cPanel), one
  managed host (e.g. a Cloudways/Kinsta-class box), one host behind Cloudflare.
- Per OS: the `SecretStore` round-trip and a pull on macOS, Windows (Dell), Linux (UTM VM).

## 9. Effort shape

Stages, each shippable on its own:

1. **S1 Pull-only clone** — plugin read endpoints + pairing + SecretStore + New Site
   "From a live site" + Pull. Already useful: "get me a local copy of production".
2. **S2 Push** — quarantine, shadow tables, swap, backup, rollback, conflict list.
3. **S3 Incremental + selective** — sync base, per-table/per-folder choice, live-owned
   defaults.
4. **S4 Polish** — CLI, MCP read/pull, history, wordpress.org listing.
5. **S5 Multisite.**

## 10. Task list

| # | Task | Done when |
|---|---|---|
| L0 | Owner answers §11; plan updated | §11 answered |
| L1 | Protocol spec: endpoints, signing, cursor, framing, versions; shared test vectors | ✓ DRAFT 10 Oct 2026 — `docs/rexsync-protocol.md`: the pairing key (JSON in base64url, `https://` only), the seven-line canonical string + HMAC-SHA256, the plugin's check order, the read/push routes with cursor bounds, the binary file frame with a terminator, exclusions, error codes, three vectors computed with Python's `hmac` (the vectors FILE lands with L3/L5, which read it). Reviewed by nobody yet |
| L2 | `SecretStore` trait + macOS / Windows / Linux impls + words ×3 | round-trip proven on all three OSes; ledger #3 — ✓ 10 Oct 2026: NOT a trait after all (§11 Q4 + the ad-hoc-signing question): `core/live_sync/secrets.rs`, one owner-only file per site under app data via `write_private`, on every OS; #828 |
| L3 | Plugin skeleton: pairing UI, key, HMAC verify, nonce cache, `/manifest` | PHPUnit incl. replay + skew refusal; ledger #4 — **started 10 Oct 2026**: the signature check (`includes/class-rexenv-sync-signature.php`, its order, replay/skew/tamper refusals) with a plain-PHP test against the shared vectors; ledger #824. Pairing UI, the nonce store on transients and `/manifest` are still to do — **pairing UI, nonce store (transients) and `/manifest` built 10 Oct 2026**; `live_sync_plugin_check` ALL PASS inside a real WordPress (#825). PHPUnit stayed a plain-PHP + `wp eval-file` pair — no Composer dev dependency in the plugin |
| L4 | Plugin read endpoints: DB export by cursor, file manifest, file read stream; exclusion of its own options | PHPUnit on the shared-host profile — **built 10 Oct 2026**: `/files/list` (sorted walk, cursor = last path), `/files/read` (the §4.3 frame, traversal-refusing), `/db/export` (DROP+CREATE first, PK-ordered pages, the pairing rows excluded); proven in WordPress, not yet on a shared-host profile |
| L5 | `core/live_sync/` client: signing, manifest fetch, cursor loop with retry, `.partial` + hash | L0 tests against the vectors — **started 10 Oct 2026**: `core/live_sync/sign.rs` (canonical string, HMAC over `sha2`, `parse_key` with the https refusal) against the same vectors; ledger #824. The HTTP client, cursor loop and `.partial` handling are still to do; **the client built 10 Oct 2026** (`core/live_sync/client.rs`: signed `?rest_route=` requests, §6 sentences, manifest identity check, cursor loops, the frame parser, `.partial` export) — L1 `live_sync_pull_check` ALL PASS over HTTP against the plugin (#826). Retry-from-cursor on a dropped connection is not built |
| L6 | Pull job: DB → `dbrestore` → rehome → swap DB; files → staging → move; progress phases | L1 `live_sync_check` pull leg; ledger #2, #5, #6 — **core built 10 Oct 2026** (`core/live_sync/pull.rs::pull_into`: staging DB, rehome, one `RENAME TABLE` swap keeping `<db>_prepull`, files via a staging folder, the plugin deactivated locally); L1 `live_sync_pull_into_check` ALL PASS (#827). Not yet: the job card + phases, and its command (needs the stored key, L2); **the job + command built 10 Oct 2026** (`commands/live_sync.rs::live_sync_pull`, one job per site, `live-sync://state/<id>`; the pull's lines stream to the card); #829 |
| L7 | New Site "From a live site" (provision + pull), DISABLE_WP_CRON default, plugin deactivated locally | L1 creates a serving site from the fixture live — ✓ 10 Oct 2026: the dialog's fourth source (`LiveSourceFields`), `live_sync_preview` + `live_sync_create_from_live` (pairing stored in the insert's transaction, the pull chained; `wk-checks/newsite-live.js`, plants; #835). The plugin is deactivated locally by the pull (#827). `DISABLE_WP_CRON` on every pull: ✓ 10 Oct 2026 (Q8b, #839). L1 of the chain itself: SMOKE row |
| L8 | Live tab UI: status, Pull, history, Disconnect | Playwright WebKit pass — ✓ 10 Oct 2026 as the **Live** tab (`components/sites/LiveTab.tsx`: connect form with optional HTTP auth, Pull / Pull-recent-uploads-only behind confirms, the job card, Disconnect); `wk-checks/livetab.js` + `uireview.js` `live-*` green; #829. The history list is not built |
| L9 | Plugin push endpoints: quarantine, shadow import, swap+backup, rollback, maintenance | PHPUnit: swap atomic, rollback restores; ledger #1 — ✓ 10 Oct 2026 (`class-rexenv-sync-pusher.php`; legs 8a–8g in WordPress, two plants; #832). `push/status` is not built: an interrupted push is aborted and started again |
| L10 | Push job: preflight, local copy + rehome to live, upload, swap, verify, Roll back button; typed-domain confirm | L1 push + rollback legs — ✓ core 10 Oct 2026 (`core/live_sync/push.rs::push_from`; L1 legs 6–10, plant; #833). ✓ job + UI 10 Oct 2026: `live_sync_push_plan/push/rollback`, the picker, the typed host (checked in Rust), Roll back (`wk-checks/livetab.js` legs 5–7, plants; #834) |
| L11 | Sync base + conflict list + live-owned table defaults (S3) — 10 Oct 2026: two base bugs found by pushing against the pull's OWN base (#841: pulled files stamped "now" made the first push send all of wp-content; empty tables' NULL Auto_increment read as a live change) | L0 diff tests; L1 leg where both sides changed — base recorded by pull/push (`core/live_sync/base.rs`), the plugin's conflict verdict against it, live-owned defaults (`LIVE_OWNED`): ✓ 10 Oct 2026 (#833). The conflict card lists the items and "Push anyway" overrides them all at once (#834); a per-item three-column choice is S3 |
| L12 | CLI `rex live …`, MCP `live` (status/diff/pull only) + dial | `cli` tests; CLI-ROADMAP; MCP parity; ledger #7 — ✓ 10 Oct 2026: `rex live <domain> status \| diff \| pull \| push --confirm <host> \| rollback \| disconnect` (six `live.*` arms), the MCP `live` tool (status/diff Read, pull Destroy; no push/rollback/pair action at any level — L0 `live_reads_under_read_pulls_under_destroy_and_never_pushes`), `live_sync_diff`; #836. Connect is not a CLI command (the key would land in shell history) |
| L13 | Security review of plugin + client (security-auditor pass + owner) | findings closed — agent pass ✓ 10 Oct 2026 (read-only security-auditor over the plugin, `core/live_sync`, `commands/live_sync.rs`, the protocol): 1 critical (export stream imported as root → scoped throwaway user, #837), 1 high (no size/disk ceilings → 1 GiB per file, disk pre-check), 2 medium fixed (`now` on every 401 + skew before signature → signature first, `now` on `clock_skew` only; host charset in `parse_key`), 1 low fixed (conflict list filtered to what was asked), 3 accepted-and-documented (pulled code runs locally — it is the site; pairing file plaintext behind the ACL; probabilistic nonce GC). All in #837. **Owner's half still open:** read #837 and the three accepted postures |
| L14 | Real-host smoke (3 hosts) + per-OS proof; SMOKE / ARCHITECTURE / MAP / TESTING / INSTALL updated | SMOKE section green with host names — host 1 = `https://live-sync.rex.bd` (owner, 10 Oct 2026); `live_sync_real_host_check` (system tier): run 10 Oct 2026 — pull ✓, nothing-changed-after-pull ✓ (#841 holds on a real MySQL), probe push ✓ (post at the live domain, file served), rollback ✓ by the site's own file list; on the fourth run the host reset every connection from this Mac for some minutes, then recovered (cause not established; NOT an IP block — the owner browsed the site; a pull is ~25 requests). The client now names a reset plainly (#842) and retries READS with backoff (#843). Owed: hosts 2 and 3. Per-OS: the fixture L1 (`live_sync_pull_into_check`) ALL PASS on macOS, the Dell's Windows 10 and the Ubuntu 22.04 VM |
| L15 | Multisite (S5) | own plan section, own sign-off |

## 11. Open questions for the owner

1. **Plugin distribution** — ANSWERED 9 Oct 2026: **in-app zip only for v1**. wordpress.org
   is a later decision. The plugin is still written to its guidelines (GPL, no
   phoning home, nonce/capability checks), so that door stays open.
2. **Name** — DECIDED 10 Oct 2026 (owner: "do them all" — the agent's defaults stand): "rexenv Sync" / the "Live" tab.
3. **Huge uploads** — DECIDED 10 Oct 2026: "skip uploads older than N months" ships with the
   pull (an `exclude`-style filter the plugin applies by mtime); uploads-on-demand (a local
   nginx fallback that fetches a missing file from live) is S3 work, after push.
4. **Linux with no Secret Service** — DECIDED 10 Oct 2026 (owner): a 0600 file. And, asked the
   same morning, **macOS too**: rexenv is ad-hoc signed, so every update changes the code
   identity a keychain item is bound to and would prompt after each update; the owner chose
   the 0600 file. So `SecretStore` is ONE implementation on every OS: an owner-only file under
   app data through `PermissionManager::write_private` (0600 on macOS/Linux, the owner-only
   ACL on Windows, #597) — no keychain, no Credential Manager, no Secret Service.
5. **Basic-auth / IP-allowlisted live sites** — DECIDED 10 Oct 2026: HTTP basic auth is an
   optional user:password stored beside the key and sent as `Authorization`; IP allowlists are
   the host's business (the pull says "a firewall refused").
6. **Anonymise on pull** — DECIDED 10 Oct 2026: later (S3), after push — it is its own feature
   (which columns, which tables, per plugin).
7. **Push scope default** — DECIDED 10 Oct 2026: whole DB minus the live-owned tables.
8b. **`DISABLE_WP_CRON` on a synced site** (§2.8) — ANSWERED 10 Oct 2026: **off, on every pull** ("jodi disable korle valo hoi tahole disable kore rakho"). Set before the swap, so a failure aborts with nothing replaced (#839). The risk it guards: a pulled WooCommerce Subscriptions site renewing real subscriptions through real gateways from the local copy. Due events still run on demand (the Cron panel, `wp cron event run --due-now`). No per-site toggle: a person who wants page-view cron removes the line, and the next pull puts it back — said in the log line.
8. **MCP push** — ANSWERED 9 Oct 2026: **a push needs a human click.** Push and Roll back
   are never MCP actions, at any dial level (invariant #7). That holds even after v1; it
   is not a v1 limit.
