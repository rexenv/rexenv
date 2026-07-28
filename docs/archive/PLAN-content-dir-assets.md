# PLAN — 5b: add-from-Git assets follow the recorded content dir

Ruled 28 Jul 2026 (tunnel-review step 5 follow-up): thread the v24 recorded
content dir (`Site::content_dir_rel`) through every direct-fs content path the
add-from-Git feature constructs, batching `theme_screenshot`, plus the cheap
honest-indeterminate fix for the Bedrock debug-log panel. The standing rule
this enforces: **wherever we construct a content path ourselves instead of
asking WP-CLI, we are making an assumption Stage 0 invalidated.**

## Inventory (all direct-fs content paths; WP-CLI flows are exempt by design)

| Site | Layer | Bedrock behavior today |
|---|---|---|
| `core/repo.rs asset_dest` | THE funnel — all 12 command callers build clone/link/scan/watch/delete targets through it | clones INTO the repo at a dead path; every dependent flow inherits the lie |
| `commands/wordpress.rs:141,203` (found in this pass — missed by the first audit) | the unlink-only delete guard's OWN path build | **guard silently defeated**: stats `web/wp-content/...` (wrong), sees no symlink, routes a LINKED asset to wp-cli — which resolves the REAL path and would walk through the user's symlink. The exact destruction the guard exists to prevent. |
| `core/wordpress.rs theme_screenshot` | cosmetic read | silently no screenshots |
| `core/logs.rs wp_debug_log_status` | read-only status | "off/empty" while errors go elsewhere — a wrong answer, not a missing one |

## The two ruled questions

**1. Existing provenance rows on a Bedrock site.** `site_git_assets` rows
store `(kind, dir_name)` — NAMES, never paths. Nothing recorded points at the
old wrong location; resolution happens live through `asset_dest` at every
use. After the fix, a pre-fix clone stranded at `web/wp-content/<kind>s/<x>`
makes its row resolve to the (empty) correct dir, and every flow that touches
it fails LOUDLY with the existing "not a git checkout (no .git)" error — no
silent path, no data risk, and no migration of rows is needed or possible
(there is nothing to rewrite). Decision: **no auto-move** — we never relocate
directories inside a user's repo unprompted, even ones we created; the
stranded checkout stays where it is, visible in the user's own git status.
Surfacing seam (follow-up, not this pass): the assets listing can cheaply
mark a row whose dir is missing at the recorded rel but present under legacy
`wp-content/` as "stranded at <path>" — two stats per row. Recorded here so
it's a deliberate later choice; the population that can hit it is
approximately this dev machine (Stage 0 is two days old, app pre-release).

**2. Unlink-delete guard behavior across the change.** On stock layouts the
rel is `wp-content` — byte-identical paths, zero behavior change. On Bedrock
the guard is BROKEN today (above); the fix restores it: the symlink partition
finally stats the directory wp-cli will actually act on. That clears the
migration bar as a provable improvement — the guard's filesystem-truth
doctrine is unchanged, it just finally looks at the true filesystem.

## Changes

1. `asset_dest(docroot, content_rel, kind, dir_name)`; all 12
   `commands/repo.rs` callers pass `site.content_dir_rel()` (every one already
   holds the Site). Error copy that says "under wp-content" states the actual
   rel. The unmanaged-scan caller derives from `asset_dest` already — inherits.
2. `commands/wordpress.rs` delete guards (141/203) build from
   `site.content_dir_rel()`.
3. `theme_screenshot(docroot, content_rel, slug)` threaded via `theme_list`.
4. `wp_debug_log_status(docroot, content_rel)`: non-stock rel ⇒ new
   `indeterminate: true` (Bedrock's defines live in `config/application.php`,
   which the wp-config reader never parses) — the panel says "can't determine"
   instead of a confident wrong "off". Full fix rides the future wp-config
   reader work (TODO).
5. Radicle `public/content` string flagged in code as
   unverified-against-a-real-project.

## Out of scope

Full Bedrock config parsing (`config/application.php` env defines); the
stranded-row surfacing seam above; any auto-relocation of pre-fix clones.

## Done when

`verify.sh` green; repo tests updated for the new signature (incl. a Bedrock
rel case for `asset_dest` and both delete-guard partitions); no remaining
production `join("wp-content")` outside WP-CLI-backed flows (grep-clean).
