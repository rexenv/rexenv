# rexenv Sync — the WordPress plugin half of live ↔ local sync

**Not a plugin yet.** This is the start of the companion plugin that
`docs/PLAN-wp-live-sync.md` describes: it goes on a LIVE WordPress site and answers signed
HTTPS requests from rexenv (`docs/rexsync-protocol.md`). So far it holds only the request
signature, which is the part both sides must agree on byte for byte.

| File | What |
|---|---|
| `includes/class-rexenv-sync-signature.php` | the canonical string, the HMAC, and the §3 check in its order (key → clock → signature → nonce) |
| `tests/vectors.json` | the SHARED vectors — also read by `src-tauri/src/core/live_sync/sign.rs`'s tests |
| `tests/signature-test.php` | plain PHP, no PHPUnit: `php companion/rexenv-sync/tests/signature-test.php` (7.4+) |

License: GPL-2.0-or-later, like every WordPress plugin (the plan keeps the wordpress.org
door open, §11 Q1).
