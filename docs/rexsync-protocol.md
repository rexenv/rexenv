# rexsync1 — the wire protocol between rexenv and the rexenv Sync plugin

**Status:** DRAFT 10 Oct 2026. This is L1 of `docs/PLAN-wp-live-sync.md`, written before the
owner's answers to that plan's §11 Q2–Q7. None of those answers change the wire: they decide
names, defaults and scope. Nothing implements it yet. A change to anything in §2–§4 before the
first release is free; after it, it is a `rexsync2`.

The plan says WHY: rexenv always calls out, the plugin is dumb, URL rewriting happens only in
rexenv, and push goes to shadow tables, then a swap, with a backup. This doc says exactly WHAT
crosses the wire, precisely enough that the Rust client (`core/live_sync/`) and the PHP plugin
(`companion/rexenv-sync/`) can be written separately and agree. §7's vectors are the proof they
agree. Both test suites read the same vectors file.

---

## 1. Actors and trust

- **The plugin** runs on the LIVE site, under its PHP and its database user. It holds one
  pairing: `key_id` + a 32-byte `secret`, in its own option row (`rexsync_pairing`, autoload
  off).
- **rexenv** holds the same `key_id` + `secret` in the OS secret store (plan §3, `SecretStore`),
  keyed `rexsync:<site_url>`.
- **Every request is rexenv → plugin, over HTTPS.** The plugin never opens a connection.
  The plugin authenticates rexenv by the signature (§3). rexenv authenticates the plugin by
  TLS (certificate validation stays ON — no "accept self-signed" switch in v1) and by the
  `site_url` the key carries matching the `manifest`'s.

## 2. The pairing key

What the plugin shows once, and the user pastes into rexenv:

```
rexsync1:<base64url(JSON)>          JSON = {"u": site_url, "k": key_id, "s": base64url(secret)}
```

- `base64url` is RFC 4648 §5, unpadded, everywhere in this protocol.
- `u`: the site's `home_url()` with scheme, no trailing slash. **Must be `https://`.** rexenv
  refuses an `http://` key at paste time, with a sentence naming why (a database dump in clear
  text).
- `k`: `k_` + 8 lowercase hex characters, chosen by the plugin. It names WHICH pairing a
  request uses, so Regenerate makes old requests fail by name ("key k_0123abcd was replaced"),
  not by a bare signature mismatch.
- `s`: 32 random bytes (`random_bytes(32)`).
- JSON with no spaces. A key that does not parse, a missing field, an extra field or a wrong
  prefix is refused as a whole.

The plugin keeps one pairing at a time. **Disconnect** deletes it; **Regenerate** replaces it.
Either takes effect on the next request.

## 3. Request signing

Every request carries four headers:

| Header | Value |
|---|---|
| `X-Rexsync-Key` | the `key_id` |
| `X-Rexsync-Ts` | Unix seconds, decimal |
| `X-Rexsync-Nonce` | 16 random bytes, base64url (22 chars) |
| `X-Rexsync-Sig` | base64url(HMAC-SHA256(secret, canonical)) |

The **canonical string** is these seven lines, joined by `\n`, with no trailing newline:

```
rexsync1
<METHOD>                      upper case
<route>                       the REST ROUTE, e.g. /rexenv-sync/v1/manifest — never the URL path
<query>                       key=value pairs sorted by key (then value), joined by &, keys and
                              values percent-encoded RFC 3986 unreserved-only; `rest_route`
                              is never part of it; "" when there is none
<ts>                          the X-Rexsync-Ts value
<nonce>                       the X-Rexsync-Nonce value
<hex(sha256(body))>           of the raw request body; of "" for a GET
```

The plugin's check, **in this order and before it reads any argument** (plan invariant #4):
1. All four headers are present, and `key_id` names the current pairing. Otherwise `401
   unknown_key`.
2. Recompute the signature and compare it with `hash_equals`. Otherwise `401 bad_signature`.
3. `|now − ts| ≤ 300`. Otherwise `401 clock_skew`, with the plugin's `now` in the body so rexenv
   can say "this machine's clock is N minutes off". AFTER the signature (changed 10 Oct 2026,
   the security review): the time, and the fact that the key id is current, are for a caller
   who holds the secret — not for anyone with a guessed key id and three headers.
4. The nonce has not been seen within 600 s. It is stored as a transient keyed by
   `sha256(key_id|nonce)`. Otherwise `401 replayed`.
5. Only then is the route's own handler called.

Each route's `permission_callback` IS this check. No route uses `__return_true`, and a route
added without it fails the plugin's own test that enumerates the routes.

**Why the route and not the URL path** (changed 10 Oct 2026, while the plugin was being
written, before anything shipped). A site behind a proxy, a site with a custom REST prefix
(`rest_url_prefix`) and a site with pretty permalinks off (`?rest_route=/rexenv-sync/v1/…`) each
spell the URL differently. The ROUTE is the one thing WordPress hands the plugin the same way
every time (`WP_REST_Request::get_route()`), and the one thing rexenv knows without knowing the
deployment. The first draft signed the URL path, which would have broken on exactly those sites.

## 4. Endpoints (`/wp-json/rexenv-sync/v1/…`)

Every response is JSON unless stated otherwise. A failure is `{"code": "...", "message": "..."}`
with a 4xx/5xx status. The `message` is a sentence a person can act on. **Every endpoint
does bounded work per call:** at most ≈ 8 MB of output or ≈ 15 s, whichever comes first. A
long job is a CURSOR loop. A cursor is an opaque string the plugin returns and rexenv sends
back unchanged. `null` means done.

### 4.1 Read (pull)

| Route | In | Out |
|---|---|---|
| `GET /manifest` | — | `{protocol: "rexsync1", plugin: "1.0.0", site_url, wp, php, mysql, prefix, multisite, subdomain, charset, tables: [{name, rows, bytes, checksum}], free_bytes: n\|null}`. `subdomain` (0.1.5): a network's install type; rexenv writes the local copy's network constants from it |
| `GET /files/list?cursor=` | `root` = `wp-content` (v1: the only root) | `{files: [{path, size, mtime}], cursor}`. `path` is relative to `root` and always `/`-separated, in SEGMENT-wise lexicographic order (a sorted depth-first walk), so the cursor is just the last path returned. Excluded paths are never listed (§5) |
| `POST /files/read` | `{paths: [..]}` (≤ 200) | **framed binary** (§4.3) |
| `GET /db/export?table=&cursor=` | one table | `{table, sql, sha256, rows, cursor}`. The first chunk starts with the session header (`SQL_MODE='NO_AUTO_VALUE_ON_ZERO'`, `SET NAMES`, `FOREIGN_KEY_CHECKS=0` — WordPress's zero-date defaults need it on a strict MySQL), then `DROP TABLE IF EXISTS` + `CREATE TABLE`; later chunks are `INSERT` batches. `sha256` is of this chunk's `sql` |

- `checksum` is a change STAMP, `"rows:<n>:len:<data_length>:upd:<UPDATE_TIME>"`, never
  `CHECKSUM TABLE`: that reads the whole table on InnoDB, on every `/manifest`, and a big
  site's manifest would pass `max_execution_time`. rexenv only compares stamps of the same
  site for EQUALITY. Views are not listed. `Rows` is InnoDB's ESTIMATE, and a `RENAME TABLE`
  resets it (one row reads 0 until the next stats pass): a swap or rollback therefore ends with
  `ANALYZE TABLE` on what it renamed, so the stamp rexenv reads straight after as its new base
  does not move by itself minutes later (0.1.2, #857 — `wpcs_terms` stopped a push on
  live-sync.rex.bd that no one had touched).
- **Every options-like table's stamp is a CONTENT stamp** — the options table (0.1.3, 14 Oct
  2026), and on a network each sub-site's `<prefix><id>_options` and the network's `sitemeta`
  (0.1.5, 16 Oct 2026; `Rexenv_Sync_Reader::options_like`, mirrored by rexenv's
  `stamps::options_like`): every request writes transients into all of them, so a status
  stamp never stands still and a network could never read "up to date". The options table:
  `opt:<count>:<sum of CRC32(option_name|option_value|autoload)>` over the rows that do not
  move on their own — transients, `cron`, `rewrite_rules`, `recently_activated`, the plugin's
  `rexsync_*` rows, Action Scheduler locks (`Rexenv_Sync_Reader::VOLATILE_OPTIONS`; rexenv's
  copy for the LOCAL side is `core/live_sync/stamps.rs::VOLATILE_OPTIONS`, and a lib test
  holds the two lists equal, #859). With `SHOW TABLE STATUS` fields every page view on live
  moved `wp_options`, so "up to date" could never be said about it and every pull re-exported
  it. The other tables keep the status stamp below.
- The nonce store is the plugin's own table `<prefix>rexsync_nonces` (never listed, exported
  or pushed): a row per signed request in `wp_options` would move that table's stamp on
  every call. The stamp is `ai:<Auto_increment>:upd:<Update_time>:rows:<n>:len:<bytes>`,
  read with `information_schema_stats_expiry = 0` on MySQL 8. `Auto_increment` NULL is
  written as 1 (10 Oct 2026): an empty table nobody has opened reports NULL until the first
  SELECT — the pull's own export — makes it 1, so unfolded, every never-used table read as
  changed on live right after a pull. **A MySQL restart on the host** (0.1.6, 16 Oct 2026)
  makes `Update_time` NULL for every table and re-samples the `Rows` estimate: 21 of 38 tables
  read "changed on live" minutes after a pull with nothing written (the cPanel host). The plugin
  remembers each table's last non-NULL `Update_time` with the `Rows` read beside it
  (`rexsync_stamps`, its own row) and hands them back when a row comes up NULL; a real write
  after the restart sets a fresh time and moves the stamp as before.
- `/db/export`'s cursor is `"k:<json>"` (keyset: a single-column primary key, `WHERE pk >
  last`) or `"o:<n>"` (offset: composite or no key). A cell that is not valid UTF-8 is
  written `0x<hex>`, because the chunk travels inside JSON, which would rewrite the bytes
  and break the chunk's `sha256`. A `%` is written as `%`: `$wpdb->_real_escape()` alone
  writes it as wpdb's per-request `{<64 hex>}` placeholder, which only `$wpdb->query()`
  turns back — and the chunk is imported by the mysql client. Plugin 0.1.0 shipped that,
  so every pulled permalink structure broke and a push carried it to live (#856); the
  client refuses a `manifest` whose `plugin` is older than 0.1.1 (`MIN_PLUGIN`).
- The plugin's own option rows (never exported, restored around a swap): the pairing,
  `rexsync_backup` (the ONE kept backup, read by rollback), `rexsync_push_<id>` (a push's
  record, from `begin` until its swap or abort — dropped at the end of the swap since 10 Oct
  2026; it used to stay for ever, #845), and `rexsync_last` (0.1.3: what rexenv last did here
  — `seen` on any signed request, at most once a minute; `pull` on a table's first export
  chunk; `push` on a swap; `rollback` — the admin page's "last contact / last pull / last
  push" lines; excluded from the options stamp with the other `rexsync_*` rows).
- `/db/export` never emits the plugin's own option rows (§5), by `WHERE option_name NOT IN
  (...)` on the options table and `sitemeta` alike.

### 4.2 Write (push) — built 10 Oct 2026 (plan S2)

| Route | In | Out |
|---|---|---|
| `POST /push/begin` | `{tables: [..], files: [..], deletes: [..], base: <manifest checksums rexenv last saw>, override: [..], resume: <push_id>\|null}` | `{push_id, conflicts: [..], resumed, db_done: [..], received: {path: bytes}}`. **Conflicts** are tables/files whose live checksum or size+mtime differ from `base`. A non-empty list stops here unless the request named each conflict in `override`. **`resume`** (0.1.4, 15 Oct 2026): the id of an OPEN push to continue — the same conflict check, then that push's id back with `resumed: true`, the tables it holds WHOLE (`db_done`) and the bytes it holds of each file (`received`); a push that is not open, or that never named a table or file now asked for, answers `404 no_push` (a widened push starts over). Without `resume`, every other open push is aborted first: **one open push at a time** |
| `POST /push/file?push_id=&path=&offset=` | raw bytes | `{received}`. Written into the quarantine dir `wp-content/uploads/rexsync-<push_id>/` (`push_id` = 32 hex) |
| `POST /push/db?push_id=&table=&chunk=&chunks=` | `{sql, sha256}` | `{ok}`. Executed into `<prefix>rxnew_<table>` ONLY. The SQL is parsed for that one table name, and any other statement is refused. `chunk` of `chunks` (1-based, 0.1.4): the table counts as whole — what `swap` requires and a resume trusts — only once the LAST chunk ran; a first chunk marks it not whole again (a table sent again starts from its `DROP TABLE`). A client that sends no counts (rexenv ≤ 14 Oct 2026) marks it whole on every chunk, as 0.1.3 did |
| `POST /push/swap?push_id=` | — | `{swapped: [..], backup_id}`. One `RENAME TABLE` for the live → `rxbak_` → live rotation; files moved with the old ones kept under the backup; the plugin's option rows re-written; caches flushed; maintenance mode on for the duration only |
| `POST /push/rollback?backup_id=` | — | `{restored: [..]}` |
| `POST /push/abort?push_id=` | — | `{removed: true}`. Quarantine dir and `rxnew_*` tables gone |
| ~~`GET /push/status?push_id=`~~ | — | Never built: resume is `begin` with `resume` (above, 15 Oct 2026) — the one call answers the open push's state AND re-runs the conflict check, so nothing can be continued past a change on live |

**`/push/swap` never runs without a backup made in the same request** (plan invariant #1). If
the backup step fails, nothing is swapped.

### 4.3 The file frame (`POST /files/read`)

`Content-Type: application/octet-stream`. The body is a sequence of records:

```
u32 big-endian   header length H
H bytes          JSON {"path": "...", "size": n, "mtime": n, "sha256": "<hex>"}
size bytes       the file
```

The plugin ends the stream with a zero-length header (`00 00 00 00`). A file that cannot be
read gets a record with `size: 0` and `"error": "..."`, not a missing record. rexenv refuses
a stream that ends without the terminator, because a cut-off transfer looks exactly like a
short one. It refuses a `path` that is absolute, contains `..` or `\`, or is not one it asked
for (plan invariant #5). It also checks every `sha256` before a byte reaches the destination.

## 5. Exclusions

Two lists. rexenv owns one and the plugin owns the other, and neither can widen the other:

- **The plugin's own, always:** its option rows (`rexsync_pairing`, the nonce transients), its
  quarantine and backup dirs, and `wp-config.php` (it is outside `wp-content` anyway; named here
  so a future root cannot add it).
- **rexenv's, sent with each request as `exclude: [glob...]`:** caches, backup plugins'
  archives (`updraft/`, `ai1wm-backups/`, `backups-dup-lite/`) and `*.log`. The list lives in
  ONE place in rexenv (`core/live_sync/exclude.rs`).

## 6. Errors the client must word

| `code` | Status | rexenv says |
|---|---|---|
| `unknown_key` | 401 | the site has a different pairing now — paste the new key |
| `clock_skew` | 401 | this machine's clock is N minutes off the site's |
| `bad_signature` | 401 | the pairing does not match — paste the key again |
| `replayed` | 401 | (retry once with a fresh nonce; a second one is a bug report) |
| `waf_blocked` | — | a 403/406 **without** a rexsync body: "a firewall in front of the site (Wordfence, Cloudflare, ModSecurity) refused the request" |
| `too_large` | 413 | the host's upload limit — the client halves its chunk and retries |
| `conflict` | 409 | the conflict list (plan §2.6) |
| `quota` | 507 | the host's disk is full — `free_bytes` from the manifest names how far |

## 7. Test vectors

`companion/rexenv-sync/tests/vectors.json` holds these vectors (it is in the tree since 10 Oct 2026). The Rust tests
and the PHPUnit suite BOTH read it, so a canonicalisation bug fails on both sides. Computed
10 Oct 2026 with Python's `hmac`, from the same definitions as §2–§3:

```
secret (bytes 0x00..0x1f)    AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8
key_id                       k_0123abcd
nonce                        AAECAwQFBgcICQoLDA0ODw
ts                           1760054400

1. POST /rexenv-sync/v1/db/export?table=wp_options&a=1
   body {"tables":["wp_options"]}
   body sha256  d1c59913317b23140a0b33494b3e1901215d9d48394df796d43b918642a83131
   canonical    "rexsync1\nPOST\n/rexenv-sync/v1/db/export\na=1&table=wp_options\n
                 1760054400\nAAECAwQFBgcICQoLDA0ODw\nd1c599…3131"      (query SORTED: a before table)
   sig          1oPUhSd7v7deM03Jj2-_5ORu1fLgdaRXALmKOBSua4Q

2. GET /rexenv-sync/v1/manifest      (no query, empty body)
   sig          pnuq626SMBcdRGwnku6QoUQuj-s9Ev18HY1nklVLNJE

3. The pairing key for https://example.com with the secret above:
   rexsync1:eyJ1IjoiaHR0cHM6Ly9leGFtcGxlLmNvbSIsImsiOiJrXzAxMjNhYmNkIiwicyI6IkFBRUNBd1FGQmdjSUNRb0xEQTBPRHhBUkVoTVVGUllYR0JrYUd4d2RIaDgifQ
```

Vector 1 is the one that matters. It has a query given out of order, so a client that forgets
to sort signs the wrong string.

## 8. What is deliberately not in v1

- **No response signatures.** TLS authenticates the plugin, and per-chunk `sha256` catches a
  truncated or corrupted payload. A response MAC adds a second key schedule that buys nothing
  over TLS.
- **No compression negotiation.** Hosts compress `application/json` themselves. The file frame
  stays raw, because images and zips do not shrink.
- **No multisite routes.** A network needs none (S5, 16 Oct 2026): its tables carry the
  sub-site id after the prefix (`wp_2_posts`), the manifest lists them all, and the blogs
  table is data like any other; what differs is rexenv's side (the constants, the
  network-aware URL move, the content stamps above).
- **No streaming push of the whole database in one request.** Every write is cursor-bounded,
  like every read.
