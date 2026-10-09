# rexenv Sync — the WordPress plugin half of live ↔ local sync

**The read side works; nothing in rexenv calls it yet.** This is the companion plugin
that `docs/PLAN-wp-live-sync.md` describes: it goes on a LIVE WordPress site and answers signed
HTTPS requests from rexenv (`docs/rexsync-protocol.md`). Built (10 Oct 2026):
- the Tools → rexenv Sync page: connect, a key shown once, regenerate, disconnect;
- the signature check on every route;
- `/manifest`, `/files/list`, `/files/read` (the binary frame) and `/db/export`.

Push is a later stage.

| File | What |
|---|---|
| `includes/class-rexenv-sync-signature.php` | the canonical string, the HMAC, and the §3 check in its order (key → clock → signature → nonce) |
| `tests/vectors.json` | the SHARED vectors — also read by `src-tauri/src/core/live_sync/sign.rs`'s tests |
| `tests/signature-test.php` | plain PHP, no PHPUnit: `php companion/rexenv-sync/tests/signature-test.php` (7.4+) |
| `rexenv-sync.php`, `includes/class-rexenv-sync-{pairing,rest,reader,admin}.php` | the plugin |
| `tests/integration-test.php` | inside WordPress (`wp eval-file`), through WordPress's own REST dispatcher; `src-tauri/examples/live_sync_plugin_check.rs` runs it on a fixture site |

License: GPL-2.0-or-later, like every WordPress plugin (the plan keeps the wordpress.org
door open, §11 Q1).
