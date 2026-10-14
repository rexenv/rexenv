# rexenv Sync — the WordPress plugin half of live ↔ local sync

This is the companion plugin that `docs/PLAN-wp-live-sync.md` describes: it goes on a LIVE
WordPress site and answers signed HTTPS requests from rexenv (`docs/rexsync-protocol.md`), which
ships it as a zip ("Download plugin"). Built (10 Oct 2026, 0.1.3 on 14 Oct 2026):
- the Tools → rexenv Sync page: three cards — Connection (connected or not, the key id, last
  contact, last pull / push / roll back by rexenv, the kept backup and its size on disk; Regenerate
  key and Disconnect ask first), How it works, This site (address, versions, tables, free disk,
  the REST address); the key shown once with Copy key;
- the signature check on every route; `rexsync_last` records what rexenv last did here;
- `/manifest` (the options table's stamp is a content stamp over the non-volatile rows —
  `VOLATILE_OPTIONS`, mirrored in rexenv's `core/live_sync/stamps.rs`), `/files/list`,
  `/files/read` (the binary frame) and `/db/export`.

Push: `/push/begin` (conflicts against rexenv's base), `/push/file` and `/push/db` into a quarantine and shadow tables, `/push/swap` (one `RENAME TABLE`, backup made in the same request), `/push/rollback`, `/push/abort`.

| File | What |
|---|---|
| `includes/class-rexenv-sync-signature.php` | the canonical string, the HMAC, and the §3 check in its order (key → clock → signature → nonce) |
| `tests/vectors.json` | the SHARED vectors — also read by `src-tauri/src/core/live_sync/sign.rs`'s tests |
| `tests/signature-test.php` | plain PHP, no PHPUnit: `php companion/rexenv-sync/tests/signature-test.php` (7.4+) |
| `rexenv-sync.php`, `includes/class-rexenv-sync-{pairing,rest,reader,pusher,admin}.php` | the plugin |
| `tests/integration-test.php` | inside WordPress (`wp eval-file`), through WordPress's own REST dispatcher; `src-tauri/examples/live_sync_plugin_check.rs` runs it on a fixture site |

License: GPL-2.0-or-later, like every WordPress plugin (the plan keeps the wordpress.org
door open, §11 Q1).
