# In-app binary updates — a signed version manifest (proposed)

**Status: PROPOSED, not started.** Written 8 Aug 2026 against `3bac4bc`.

Goal: PHP 8.3.32 ships upstream; a rexenv user gets it from **Settings → PHP → Update**,
not from waiting on a rexenv release. Same for MySQL/MariaDB/PostgreSQL/Caddy once the
mechanism exists. Herd does this for PHP and Node; today we cannot, and the reason is
not UI.

## 1. Why this isn't just a button

PHP comes from static-php.dev's bulk builds:

```
https://dl.static-php.dev/static-php-cli/bulk/php-<version>-{cli,fpm}-macos-<arch>.tar.gz
```

**static-php publishes no checksums** (`core/binaries.rs:198`). So rexenv downloads each
artifact at pin time, hashes it, and hardcodes the digest —
`PHP_8_3_31_CLI_MAC_ARM64_SHA256` and its eleven siblings, resolved by
`php_sha256(kind, version, arch)` (`binaries.rs:478`), which returns `None` for any
version not in that table. `manifest()` (`binaries.rs:545`) then `unwrap()`s it, so an
unpinned version is unresolvable by construction.

That is the non-negotiable from `CLAUDE.md` working as designed:

> Native static binaries, downloaded on demand through `BinaryProvider` (pinned
> versions, checksum-locked).

An in-app update fetches a version whose digest was never compiled in. Three ways out,
and only one keeps the invariant:

| | What it trusts | Verdict |
|---|---|---|
| **A. Signed manifest** | An ed25519 signature made by a rexenv release key, verified against a public key compiled into the app | **Take this.** |
| B. Bare TLS ("it came from dl.static-php.dev over HTTPS") | Any CA in the trust store, plus the upstream host | Reject — deletes the guard that catches a swapped body, and there is nothing upstream to verify against anyway |
| C. Nothing new; keep pins in code | — | The honest fallback (§9) if we don't want the release-side work |

Herd can offer its button because it builds and hosts its own PHP and serves a manifest
it signed. Option A is the same shape: **the trust anchor moves from a `const` in the
binary to a signed document whose public key is a `const` in the binary.** It never
moves to TLS.

## 2. Trust model

- One **ed25519 release keypair.** Private key lives with whoever cuts releases (offline
  / in a hardware token — never in CI env vars, never in the repo).
- The **public key is compiled in** (`core/updates.rs`, `RELEASE_PUBKEY`). Rotating it
  requires an app release, which is the point: a stolen manifest host cannot hand out a
  new key.
- The **manifest is signed as a whole** (detached signature over the exact bytes), so a
  single entry cannot be swapped, and old entries cannot be silently dropped.
- The manifest carries **`generated_at` + a monotonically increasing `serial`**. The app
  stores the highest serial it has accepted and **refuses a lower one** — otherwise a
  host that keeps serving an old signed manifest can hold a user on a version with a
  known CVE forever (rollback attack; the signature alone does not stop it).
- **Every artifact is still SHA-256 verified after download, exactly as today.** The
  manifest only supplies the expected digest; `binaries.rs`'s existing stream-hash +
  compare (`binaries.rs:1662-1690`) is untouched. Nothing new executes a binary that
  didn't match.
- **Compiled-in pins remain the floor.** No network, unreachable host, bad signature,
  stale serial → rexenv runs exactly what it runs today. The manifest can only ever ADD
  versions and MOVE a minor forward; it can never make the app run less-verified code.

### What this does not defend against

The upstream artifact itself. If static-php.dev ships a compromised 8.3.32, we hash it
faithfully, sign it faithfully, and distribute it. Our signature attests **"this is the
byte string rexenv's maintainer saw at pin time,"** never "this build is safe."
That is exactly today's posture — the pin script is what a human runs — so the update
path must not *claim* more. It goes in the ledger as a stated, accepted limit (§8).

## 3. The manifest

Hosted as a **GitHub release asset** on the rexenv repo (`manifest/latest`) — no new
infrastructure, no `dl.rexenv.dev` needed for this. Two files:

```
manifest.json        the document
manifest.json.sig    detached ed25519 signature over manifest.json's exact bytes
```

```jsonc
{
  "serial": 7,                       // monotonic; app refuses a lower one than it has seen
  "generatedAt": "2026-08-08T10:00:00Z",
  "minAppVersion": "0.4.0",          // entries below assume this app's extract/prepare logic
  "artifacts": [
    {
      "name": "php",                 // matches manifest()'s `name` argument
      "kind": "cli",                 // cli | fpm | (engines: single) — the existing spec split
      "version": "8.3.32",
      "os": "macos",
      "arch": "arm64",
      "url": "https://dl.static-php.dev/static-php-cli/bulk/php-8.3.32-cli-macos-aarch64.tar.gz",
      "sha256": "…",
      "archive": "targz",
      "member": "php"
    }
  ]
}
```

Notes that are load-bearing, not decoration:

- **`url` is upstream's**, not a rexenv mirror. We are publishing a *checksum*, not
  re-hosting binaries. (A mirror is a later, separate decision — it costs bandwidth and
  makes us the availability bottleneck.)
- **`minAppVersion`** exists because `prepare_binary` differs per platform and per
  release (de-quarantine → relink → codesign order). A manifest entry that needs
  handling an older app lacks must not be offered to it.
- **Both arches ship together**, matching `php_sha256`'s existing invariant ("a `Some`
  for one arch implies a `Some` for the other").
- A minor's entries are **additive**: 8.3.31 stays in the manifest after 8.3.32 lands, so
  a rollback is a normal install of an already-described version (§6).

## 4. Where it plugs into the code

One choke point per fact, both already single functions:

| Fact | Today | After |
|---|---|---|
| Which patch a minor runs | `php::patch_for_minor(minor)` → `binaries::PHP_VERSIONS` (`core/php.rs:62`) | the SELECTED patch for that minor from app state, defaulting to the compiled-in one |
| The digest for a version | `binaries::php_sha256(kind, version, arch)` (`binaries.rs:478`) | compiled-in table first, then the verified manifest cache |
| The URL for a version | `binaries::manifest()`'s `format!` (`binaries.rs:564`) | unchanged for known-shape versions; manifest `url` wins when present |

New module `core/updates.rs` (platform-agnostic — fetch, verify, cache, query; the
`reqwest` call goes through the same path the download hub already uses):

```rust
pub struct VersionCatalog { /* verified entries, merged over the compiled-in pins */ }
pub fn cached(conn: &Connection) -> VersionCatalog;                 // no network
pub async fn refresh(conn: &Connection) -> Result<VersionCatalog>;  // fetch + verify + persist
pub fn newest_for_minor(cat: &VersionCatalog, minor: &str) -> String;
```

Storage: the verified manifest JSON + its serial in **SQLite settings** (`update_manifest`,
`update_manifest_serial`), and the user's per-minor choice in the existing PHP version
rows. **No new schema for the manifest itself** — it is a cache, and a corrupt cache must
degrade to the compiled-in pins, not to an error.

Selected-patch storage does need a row: `php_versions.patch` (nullable; `NULL` = "track
the compiled-in pin"). Nullable rather than backfilled-to-current on purpose — a user who
never touches this gets the app's pin even after they update rexenv, which is the
behaviour they have today.

## 5. Update flow (PHP, per minor)

1. **Refresh** — on app launch (once, best-effort, never blocking) and on demand from the
   Settings row. Failure is silent-but-visible: the row says `couldn't check` with the
   reason on hover, never an error toast.
2. **Offer** — the PHP row shows `8.3.31 · 8.3.32 available`. No auto-update, ever: a
   patch swap restarts a pool that is serving the user's sites.
3. **Update** — reuses the whole existing path:
   - `downloads::plan_for_php` + `prefetch` (hub progress, Range-resume, real bytes),
   - cache lands in `bin_dir/php-8.3.32/` — a **new directory**, so 8.3.31 stays on disk,
   - `prepare_binary` (de-quarantine → relink → codesign LAST, macOS order unchanged),
   - persist `php_versions.patch = "8.3.32"`,
   - `service_manager::restart_php_pool` for that minor (`service_manager.rs:1173`) —
     under the existing locking rule: spawn under the lock, `await_ready` after dropping
     it.
4. **Verify then keep** — the pool must come back ready. If it doesn't, revert the stored
   patch to the previous value, restart on it, and report what happened with the log key.
   **The new tree is not deleted** (it is a valid, verified artifact — deleting it makes
   the retry re-download hundreds of MB).
5. **Sites are untouched.** They pin the minor (`"8.3"`), never the patch. No config
   regeneration, no edge reload.

Failure at any step before the pool restart leaves the site running the old patch,
because the only thing that switches it is the persisted `patch` value.

## 6. Rollback

Same flow with an older version from the manifest. The old tree is usually still cached,
so it is a settings write plus a pool restart. UI: the per-minor row's version dropdown
lists every manifest version for that minor, current one selected — the Databases
engine-version picker's exact shape (`src/routes/Databases.tsx:63-76`), which users have
already met.

Unlike the DB engines, **there is no data directory** behind a PHP patch, so this needs
no "your databases won't be visible" warning. Don't copy that dialog.

## 7. UI

`Settings → PHP versions` (`src/routes/Settings.tsx:396 PhpVersionsSetting`), per row:

```
8.3   ● installed · running   8.3.31   [Update to 8.3.32]   [Make default]
8.4   ○ not installed         8.4.23
```

- **Update** appears only when the manifest offers a newer patch for a minor that is
  installed. It is never shown for a minor the user doesn't have.
- While updating: the existing download-hub byte row + the phase line, same components as
  the provision card. Real bytes, not a spinner.
- After: the row states the patch it is now on, and a one-line `restarted the 8.3 pool`.
- Checked `just now / 2h ago` beside the section header, from the manifest's stored fetch
  time — the same honesty as the Import screen's `scanned 12s ago`: a refresh that finds
  nothing new must still visibly have run.
- **The engines (MySQL/MariaDB/PG) get the same treatment for free** — their picker already
  exists; the manifest just adds patches to the offered set. Ship PHP first.

## 8. Claims this introduces (CLAIM-LEDGER rows, same commit as the code)

| Claim | Layer that can prove it |
|---|---|
| A manifest with a bad/absent signature is never used | L0 — tamper each field, assert `cached()` falls back to the compiled-in pins |
| A manifest with a serial ≤ the stored one is refused | L0 — replay an older signed document |
| No artifact is executed without a SHA-256 match | L0 exists already (`binaries.rs` tests) + L1 for the manifest-supplied digest path |
| The compiled-in pins run when the network is gone | L0 — `refresh()` error → `cached()` still resolves every current version |
| A failed pool restart leaves the minor on its previous patch | L1 — point a minor at a deliberately broken tree, assert revert + honest error |
| ⚠ Our signature attests provenance-at-pin-time, NOT upstream build integrity | 🚫 accepted posture — stated here and in the module doc, not provable by us |

## 9. If we don't do this

The honest small version, worth shipping on its own and compatible with everything above:
the PHP row states the patch it runs and that a newer one exists — checked against the
manifest **read-only, no update button**. That is ~half a day and removes the "am I on
something stale?" question, while leaving the upgrade to a rexenv release.

## 10. Commit sequence

1. `feat(core)` — `core/updates.rs`: manifest struct, ed25519 verify, serial rule,
   settings-backed cache, merge-over-pins. Lib tests for every §8 L0 row. **No UI, no
   network call wired in** — pure, testable, inert.
2. `feat(db)` — `php_versions.patch` (nullable) + store accessors; `patch_for_minor`
   reads the selection, defaults to the pin. Migration test that a pre-migration row
   keeps today's behaviour.
3. `feat(commands)` — `php_update_check` / `php_update_apply` IPC; apply = prefetch →
   prepare → persist → restart pool → verify → revert-on-failure.
4. `feat(ui)` — the Settings row (§7) + the checked-N-ago line.
5. `chore(release)` — NEW `scripts/pin-binaries.sh` (no pin script exists today; pinning is
   done by hand): download → hash → emit manifest entries → sign → attach to the GitHub
   release. It must also be able to emit the compiled-in `const` block, so the two
   sources of truth are generated by one run and cannot drift. Documented in
   `CONTRIBUTING.md` so the step is not tribal knowledge.
6. `docs` — `PORTS.md` gains a "how a version reaches a user" section; this file flips to
   SHIPPED with commit hashes.

Steps 1–2 are useful even if 3–5 are never built: they make the pin table data instead of
code.

## 11. Open questions

- **Key custody.** Who holds the private key, and what is the rotation story if it leaks?
  (Compiled-in pubkey means rotation = app release. Acceptable, but it must be a decision,
  not a discovery.)
- **Cadence.** PHP patches land the first Thursday monthly. Is a manual pin-and-sign run
  per release realistic, or does this need CI with the key in a hardware token?
- **Mirror or not.** Publishing only checksums keeps us out of the bandwidth business but
  leaves availability with static-php.dev, which has rebuilt artifacts in place before
  (`binaries.rs:203`) — that is precisely a checksum mismatch users can't fix today. A
  manifest fixes the *reporting* (we re-hash and re-publish); only a mirror fixes the
  *availability*.
- **Scope of the first cut.** PHP only, or PHP + engines? The engines' picker exists, so
  the marginal cost is small — but each engine's version switch carries the per-series
  datadir warning, and patch-level updates inside a series do not. Don't let one control
  mean two things.
