# Streamed site provisioning: honest step-wise progress for New Site

## Context

Site creation is today a single opaque awaited `create_site` IPC: the NewSiteDialog
button flips to "Installing…" and nothing else moves — through resolver install,
binary prefetch (up to ~600MB cold), DB spawn, a ~25MB `wp core download` (captured,
**no timeout cap** — a latent B25-class gap), `wp core install`, blueprint apply, and
edge reload. The CLI likewise prints one line, blocks, prints one line. This plan
gives provisioning the same honest job treatment as the install card (bar + phase
label + log + Cancel), reusing the wp-install machinery — no third path.

Decided with user: failed/cancelled provision → **keep + "setup incomplete" badge +
Retry/Delete** (schema v16 `provisioned` flag, existing rows default 1); stack-stopped
create → **ok at 100 + honest summary note**. After this feature: ship.

## A. The real provisioning sequence (traced, not invented)

Entry: `commands/sites.rs:186 create_site` (CLI `site.create` arm calls the same fn,
`cli_server.rs:383`). Ordered steps → job phases:

| Job phase (weight) | Real steps | Ours / subprocess | Duration |
|---|---|---|---|
| **prepare** (3) | TLD validate; `dns::ensure_resolver` (privileged, no-op if installed); `sites::provision` = docroot mkdir + rcgen leaf cert + **site row insert** (`core/sites.rs:748-774`) | Ours (resolver = one privileged cmd, first TLD use only) | instant |
| **fetch binaries** (27) | `downloads::prefetch("Create site", plan)` — php-fpm/CLI, engine (mysql ~600MB), wp-cli, FrankenPHP/httpd as needed (`commands/sites.rs:219-237`) | Ours (reqwest, Hub-tracked) | network-bound cold; no-op warm |
| **start database** (5) | `spawn_db` under services lock + `await_ready` (`:243-248`) | Subprocess spawn + our probe | seconds |
| **download WordPress** (35) | `wp core download [--locale]` — ~25MB (`core/wordpress.rs:2020-2027`) | Subprocess (wp-cli) | network-bound, minutes |
| **configure** (5) | `wp config create` + `CREATE DATABASE` via bundled client + `is-installed` (`wordpress.rs:2030-2064`) | Subprocess, instant ones | instant |
| **install WordPress** (10) | `wp core install` (`wordpress.rs:2071-2078`) | Subprocess | seconds |
| **apply blueprint** (5) | `blueprints::apply_wordpress` + optional multisite convert (`commands/sites.rs:273-283`) | Subprocess (wp-cli) | network-bound if plugins |
| **start serving** (10) | `ensure_php_pool` + `mgr.reload` (config gen + nginx/caddy reload) + `await_ready` (`:286-297`) — only if stack running | Ours + reload subprocesses | seconds |

Non-WP sites (php/laravel): phases prepare / fetch binaries / start serving only.
**Phase list is built per-job at start; weights renormalize over applicable phases**
— a slice that can't fill (no blueprint, non-WP) is never reserved (same rule as the
install card's activate slice). WP-only phases sum table = 100 as shown.

Advantage over the install card, stated in code comments: phases are OUR step
boundaries — deterministic, zero output parsing. wp-cli's verbatim lines
(`Downloading WordPress 6.x…` / `Success: WordPress downloaded.`) are sub-detail
display only, never triggers.

## B. Binary-download sub-progress (real bytes, first-run case)

- Provisioning **already waits** on the download manager (`prefetch(...).await`,
  then cache-hit `resolve`s). The Hub (`core/downloads.rs`) already has REAL byte
  progress per item — `ItemSnapshot { id, label, phase, downloadedBytes, totalBytes,
  bytesPerSec }` — emitted app-wide as the coalesced `download-progress` snapshot
  event (`lib.rs:340-349`), already rendered by DownloadPanel/StatusFooter.
- **Composition (no new widget):** job state carries the plan's uncached
  `downloadIds`. The provision card, during the fetch phase, filters the existing
  `useDownloads()` snapshot to those ids and renders the existing `Row`-style line
  under the phase label: `Downloading PHP 8.3 — 12.4 / 34 MB · 2.1 MB/s` (existing
  `Track`, `pctOf`, byte formatting from `DownloadPanel.tsx`). This is genuine byte
  progress — allowed under the honesty rule and documented as such.
- **Overall-bar folding:** during the fetch phase the worker ticks (500ms) reading
  the Hub snapshot and folds `Σ downloadedBytes / Σ totalBytes` of the job's items
  into the fetch slice (27 × fraction). Any item with `totalBytes: None` → fall back
  to completed-items/items for the fold (per-item bars stay byte-accurate).
  Monotonic guard on top, as ever.
- **Resume visibility — already correct, don't break it:** on a 206 resume the Hub
  item is seeded with the rehashed on-disk byte count before the first progress emit
  (`binaries.rs:1583-1594`), so bars CONTINUE from the prior offset; `item_started`
  (which zeroes) fires once per resolve, not per internal Range retry. The card must
  render snapshot values verbatim and never zero anything client-side. (Panel Retry
  after 5-attempt give-up genuinely restarts from 0 — partial is deleted; that's
  honest.)

## C. Weighting decision

**Weighted slices (table above), coarse and fixed** — not equal, not time-estimated:
- Equal slices lie in feel (8 phases → race to ~75% in seconds, park for minutes on
  the two network phases). The two network phases get 62% combined because that's
  where wall-time lives.
- Weights are constants; the bar still only moves on (a) real phase completions,
  (b) real bytes within the fetch slice. Nothing is timed or estimated.
- The `wp core download` slice (35) has no byte signal (wp-cli, non-TTY) → fills on
  completion; while it runs, the phase label + verbatim wp-cli lines + the silence
  ticker carry the information — the bar never sits alone with no other signal (D).

## D. Honest-waiting + never-100

- Same ticker as the install card: `waiting on <phase> · no output for Ns` for any
  running phase silent ≥10s (core download's mid-transfer silence, `core install`'s
  api.wordpress.org wait). During fetch, bytes tick constantly — ticker idle.
- Phase label = our phase name; sub-detail = last verbatim subprocess line or byte
  line. LogPane shows the per-job log (phase transition markers + verbatim lines).
- **100 only when the job settles ok** — after `await_ready` on the serve phase
  (site actually answering), capped ≤99 before. Stack stopped: serve phase reports
  "skipped (stack stopped)", job settles ok at 100 with summary
  `created — stack is stopped, site serves on next stack start` (decided).

## E. Failure / cancel

Current truth (verified): site row inserts BEFORE install (`core/sites.rs:774`); no
rollback anywhere in `create_site` (all `?`); half-site stays in the list unlabeled;
duplicate-domain guard (`core/sites.rs:749`) blocks re-create; `install_wordpress`
is idempotent by design ("Each step is skipped if already done", `wordpress.rs:2014`).

New behavior (decided — keep + badge + Retry/Delete):
- **v16 migration:** `sites.provisioned INTEGER NOT NULL DEFAULT 1` — existing rows
  read provisioned (same existing-user bar as v15's "unverified not stale"); create
  inserts 0, flipped to 1 when the job settles ok. Migration test asserts existing
  rows default 1.
- Sites list: `provisioned == 0` rows get an honest "setup incomplete" badge +
  Retry + Delete. Retry = new `site_provision_retry(site_id)` command starting the
  same job against the existing row (skips prepare's provision(), re-enters the
  idempotent install/blueprint/serve phases). Delete = existing `delete_site`.
- **Failure:** bar freezes at the failing phase (Track `stopped` variant), phase
  named in the card, per-job log kept, list refresh regardless. No auto-cleanup.
- **Cancel — offered, with honest boundaries:** CancelToken checked at every phase
  boundary (our code); pgid-kill ONLY during wp-cli phases (core download/install,
  blueprint — same safety class as the install card: WP-core places via atomic
  rename; worst residue = partial files in the docroot that the idempotent retry
  re-does) and during fetch (Hub partials are kept + Range-resumed on retry — cancel
  is literally free). NEVER kills spawn_db / pool / reload (shared services — token
  checked before/after only). Result state = cancelled + `provisioned=0` → same
  Retry/Delete affordance. Card copy states the residue honestly.
- **Timeout gap fixed:** streamed `wp core download` / `core install` get the B25
  outer wall-clock (existing `download_timeout` machinery; wp-cli's inner 300s bound
  still fires first) — today's captured calls have NO cap.

## F. Reuse map (one implementation, same look)

Backend (new `commands/site_provision.rs`, mirroring `wp_install.rs` exactly):
- `ProvisionJobs` registry (Mutex map + seq), `SiteProvisionState` (camelCase:
  id, domain, siteId, phases[{key,label,status}], phaseCursor, pct, status
  running|ok|failed|cancelled|timed_out, summary, error, logKey, downloadIds),
  events `site-provision://state/<id>` + `output/<id>`, per-job log
  `site-provision-<domain>-<id8>.log` (prune 4), one job per domain, `seq`-based
  `site_provision_active`, `site_provision_cancel`.
- Worker = today's `create_site` body refactored into a phase-driven async task
  (blocking wp-cli parts via `spawn_blocking`), emitting phase transitions; wp-cli
  steps switch `wp_cli_checked` → `repo::run_step_streamed` (idle None + outer
  timer, the proven pattern). `create_site` command stays as thin start+await for
  compat (dialog/CLI migrate to the job).
- Pure fn `ProvisionProgress` in `core/` (weights, applicable-phase renormalize,
  byte-fold, monotonic, ≤99 cap) — unit-tested like `InstallProgress`.

Frontend:
- `SiteProvisionCard` mirroring `WpInstallCard` (Track + phase label + sub-detail +
  LogPane + Cancel + ticker + frozen `stopped` settles) + the fetch-phase download
  row from existing `useDownloads` + `pctOf`/`Track`.
- NewSiteDialog: submit → `siteProvisionJob(...)`; card replaces the opaque
  disabled-button wait; dialog dismissible mid-run (job continues). Sites route
  adopts running/failed jobs via `siteProvisionActive` (remount-proof, same as the
  install card) + `provisioned == 0` badge with Retry/Delete.
- ipc wrappers + types copied from the wpInstall set; DevGitPanel harness mocks.

CLI (`site.create` arm): start job + poll settle; **observable output unchanged**
(same "creating…" line, same single success line, same exit codes); failure reply
gains failing-phase + log tail. Side-by-side output comparison before/after, Stage-3
discipline.

## Stages

**Stage 1 — backend job + streamed core download + v16** (the bulk)
- Refactor, registry, events, phases, ProvisionProgress + weights, outer timeouts,
  cancel boundaries, v16 migration + provisioned flip, retry command.
- Tests: ProvisionProgress pure (weights/renormalize/byte-fold/monotonic/≤99/frozen),
  migration existing-rows, phase-applicability. Live example (mirrors
  wp_install_stream_check): full create streams phases in order + pct monotonic →
  ok 100 + provisioned=1; kill mid-core-download → failed + provisioned=0 → RETRY
  completes; cancel mid-download → cancelled + frozen pct. Full suite + examples.

**Stage 2 — frontend card + badge**
- Card, dialog wiring, Sites adopt, incomplete badge + Retry/Delete, mocks.
- WebKit matrix: running (fetch-phase byte row, core-download ticker), ok, failed
  frozen + badge, cancelled, dialog-close-and-readopt.

**Stage 3 — CLI rides the job** (observable behavior unchanged, output diff shown)

**Stage 4 — ship**
- Fresh universal DMG (this obsoletes the `~/rexenv-release-staging` backup), user's
  §A smoke → Release upload → canonical sha from uploaded asset → cask bump.
  Version/tag call (ship everything as v0.1.0 vs 0.1.0+0.1.1 split) is the user's at
  ship time.

Each stage: verify, commit, stop for user verify. Packaged verify = CLI-driven real
create (throwaway domain) + user's GUI pass, same protocol as the install-bar round.

## Key files

- `src-tauri/src/commands/sites.rs:186` (create_site body → worker),
  new `src-tauri/src/commands/site_provision.rs`, `src-tauri/src/core/wordpress.rs`
  (`install_wordpress` streamed variant), `src-tauri/src/core/downloads.rs` (Hub
  read for byte-fold — read-only), `src-tauri/src/state/db.rs` (v16),
  `src-tauri/src/cli_server.rs:330`, `src-tauri/src/lib.rs` (manage + register)
- `src/components/sites/NewSiteDialog.tsx`, `src/routes/Sites.tsx`, new
  `src/components/sites/SiteProvisionCard.tsx`, `src/components/wordpress/repoJobUi.tsx`
  (LogPane reuse), `src/components/shell/DownloadPanel.tsx` (Track/pctOf reuse),
  `src/lib/ipc/index.ts`, `src/types/index.ts`, `src/routes/DevGitPanel.tsx`
