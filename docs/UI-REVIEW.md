# UI-REVIEW — the whole-app look-over (inventory first, then fixes)

**Status: CAPTURING (28 Jul 2026).** Not a bug hunt yet — an inventory. The last
few stages added a lot of surface, almost all of it only type-checked or seen in
a WebKit harness, and the app as a whole hasn't had a general look-over in a
long time — so expect several separate issues (including ones that predate the
migration work), not one root cause. Fixes start only after this list is agreed;
per house rules: one fix at a time, diagnosis + diff + "done when", his verify.

## A. Surface inventory — what exists and how much it has been SEEN

| Surface | Added | Verification level |
|---|---|---|
| `/import` screen (rows, statuses, selection, resolver consent panel, run progress) | Stage 1 | §G packaged pass **NEVER RECORDED AS DONE** — partially exercised during §I/§J preconditions only |
| Sites page: import banner (dismissible) | Stage 1 | §G item; seen incidentally |
| Sites page: `external` badge | Stage 0 | seen in earlier passes |
| Sites page: `DB imported · not connected` badge | Stage 2 | seen in §I (packaged) |
| Sites page: `DB connected` badge (green) | Stage 3 step 2 | seen in §J (packaged) |
| Settings: "N sites can be imported" row | Stage 1 | §G item |
| Settings: leftover database dumps card | Stage 2 | seen in §I step 10 |
| Settings: borrowed-resolver / hand-back card | Stage 1 | **clean-VM only (§F)** — never rendered on this machine (no foreign resolver exists here) |
| SiteDetail: DB import card — job progress, failed/typed-confirm states | Stage 2 | seen in §I |
| SiteDetail: interim "Imported — not yet connected" panel + tell-only snippet | Stage 2 | seen in §I; **copy/layout changed in Stage 3 step 6** (reason line, MariaDB note) — the changed form seen in §J only where refusals occurred |
| SiteDetail: rewrite consent card (diff, checkbox, backup note, creates-user note, cache warning) | Stage 3 | seen in §J |
| SiteDetail: fileChanged / engineStopped / verifyFailed notices | Stage 3 | fileChanged + engineStopped seen in §J; **verifyFailed never rendered live** (no §J step forces it) |
| SiteDetail: connected panel + Revert + "Restore anyway" flow | Stage 3 | seen in §J |
| Sites: connected-site delete dialog (3 buttons, two consequence texts, db_created=0 variant) | Stage 3 | seen in §J step 10; **db_created=0 variant never rendered** (no such site existed) |
| Toasts: rewrite success/info incl. the long revert-then-delete refusal message | Stage 3 | partially seen |

## B. Suspects spotted from the code (no build needed — check these first)

1. **The connected-site delete dialog overflows its box.** `Overlay` is a fixed
   `w-[380px]` card; the footer packs THREE buttons ("Cancel", "Delete without
   reverting", "Revert, then delete") in one `flex justify-end` row — that is
   almost certainly wider than 380px, so buttons wrap or overflow. Likely the
   most visible breakage of the new work. (`dialog.tsx` Overlay,
   `Sites.tsx` connected-delete block.)
2. **The consent card is a wall of stacked small-text blocks** — cache warning,
   heading, intro line, diff, creates-user note, backup note, MariaDB note,
   checkbox, button — all `text-xs` paragraphs in one `space-y-2`. Reads as
   dense/unstructured; probably needs grouping/hierarchy rather than copy
   changes. (`DbImportCard.tsx`.)
3. **Bare `<input type="checkbox">`** in the consent card — not a design-system
   control; native WebKit checkbox will look out of place against the token'd
   UI. (Tokens rule: no raw controls.)
4. **Stale notice stacking in the card**: `applyOutcome` notices persist after
   the preview refetches (e.g. an old `engineStopped` box above a now-ready
   consent card, or `fileChanged` remaining after a successful later apply
   until state flips). Exclusive-state rendering isn't enforced.
5. **Badge crowding on Sites rows**: a row can now carry `external` +
   `DB imported · not connected` (long label) + the provision/status pill +
   type chips — narrow windows likely wrap or truncate untidily.
6. **Long toasts**: the revert-then-delete refusal concatenates two sentences
   plus a backend message into one `toast.info` — may clip or overflow the
   toast layout.
7. **Diff `<pre>` styling is minimal** — no line-number gutter, no background
   distinction between -/+ beyond text color; fine functionally, but worth a
   look against the design tokens (it's the centrepiece of the consent card).
8. **JetBrains Mono rule sweep needed**: several new dialog/notice strings
   embed technical values (ports, db names, file paths) — most use `font-mono`,
   but not audited systematically across the new surfaces.

## C. His observations (to fill — the record we fix from)

_Pending: per-screen list from the packaged app. Structure below when it
arrives: screen → what looks wrong → screenshot if easy → suspected class
(layout / spacing / color-token / copy / stale-state)._

- Sites page: —
- SiteDetail (Database tab): —
- SiteDetail (other tabs): —
- `/import`: —
- Settings: —
- Services / Databases / Mail / Tunnels (pre-migration surfaces): —
- Onboarding: —
- Dialogs & toasts: —

## D. Known adjacent loose ends (not UI, listed so they aren't lost)

- §G (`/import` packaged GUI pass) was never run as written — subsumed
  partially by §I/§J preconditions; either run it or fold its unchecked items
  into this review.
- §F resolver takeover/restore remains clean-VM-only (deliberate).
- §A ad-hoc launch re-run on the fresh dmg + §D tap dry-run remain the publish
  gates (pre-existing).
- Continuous resolver-drift watcher: deferred in Stage 1, never built; the
  cheap startup/doctor check exists.
- New Site dialog's Laravel card still promises an installer the backend
  doesn't implement (pre-existing, unowned) — a UI-adjacent honesty bug worth
  folding into this pass.
- Apache per-site config/log not removed by teardown (pre-existing, unowned).
