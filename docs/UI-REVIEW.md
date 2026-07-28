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

## B. Suspects spotted from the code — now checked against screenshots (§C1)

Status: #1 CONFIRMED (§C1.1) · #2 largely FINE (§C1 "renders well") · #3
CONFIRMED (§C1.5) · #4 partially confirmed as the §C1.3 contradiction · #5
CONFIRMED worse than predicted (§C1.2) · #6 stack-overflow variant confirmed
(§C1.8) · #7 fine as-is · #8 two misses found (§C1.8). Original list kept for
the record:

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

## C1. Observed by screenshot (28 Jul 2026 — Playwright WebKit, dark,
## 900px + 1440px, `scripts/wk-checks/uireview.js` → `#/dev/ui-review`)

38 captures across 19 scenarios (shots in `scripts/wk-checks/shots-uireview/`,
gitignored — re-run: vite on 5199 + `node uireview.js`). The three
never-rendered surfaces have now been rendered in the harness. Confirmed, in
severity order:

1. **Connected-delete dialog: the button row overflows the 380px overlay.**
   Three buttons need ~470px; "Cancel" renders OUTSIDE the card's left edge at
   every width (the overlay is fixed-width). The predicted #1, confirmed.
2. **Sites badges break the row.** "DB imported · not connected" wraps into a
   three-line `rounded-full` balloon that OVERLAPS the rows above and below
   and collides with the status pill; "DB connected" wraps to two lines; the
   page overflows horizontally by **241px at 900px** (measured). Three
   sub-causes: the label is long, the pill allows wrapping (needs
   nowrap + a shortening strategy), and the row's badge column gets no room.
3. **State contradiction in the rewritten-but-unverified card** (the
   verifyFailed aftermath, seen in the noop variant): the interim panel
   headline says "**this site still reads and writes the old database**"
   while the consent card right below says the file "**already points at
   127.0.0.1:13306**". Both render from their own truth; together they lie.
   The interim headline needs to react to the file state (or the panel needs
   a third wording for rewritten-unverified).
4. **The backup note renders without its referent in the noop variant**:
   "The diff above can't contain your password" — there is no diff above, and
   "Before writing…" precedes a verify that writes nothing. The note must be
   conditional on a non-empty diff.
5. **The consent checkbox is a bare native `<input>`** — tiny, dim,
   dark-on-dark; visibly not the design system.
6. **The connected panel's "Revert" is a ghost button** — the primary revert
   affordance is near-invisible until hover.
7. **fileChanged notice repeats itself** — the backend message plus the UI
   sub-line both say "nothing was written".
8. Minor: the refusedEdited/backupMissing message boxes embed file paths in
   plain (non-mono) text — whole-string messages defeat the mono rule; the
   warning tint on `bg-status-warning-bg/30` reads almost neutral in dark;
   the auto-focused danger button shows a doubled focus ring; ≥6 toasts
   overflow the viewport top (StrictMode double-push is harness-only, but
   the stack has no cap).

**Renders well** (worth saying, so fixes stay scoped): the consent card's
hierarchy incl. cache warning, creates-user note, backup-limit copy and the
MariaDB note; the diff block styling; the refused/tell-only panel with the
reason + 13307 snippet; the connected panel wording (+http variant); the
revert confirm; the refusedEdited → "Restore anyway" flow; the backupMissing
message with `connected` kept; the ruling-2 delete copy naming `lms`; the
resolver hand-back row (first-ever render: fine).

**Coverage honesty**: eyeballed 12 of 19 scenarios (both widths' behaviour
identical except badges); captured-only: `card-consent-root` (baseline subset
of the cache/mariadb variant), `card-apply-engineStopped`,
`delete-wp-plain`/`delete-linked-nodb` (the long-standing ConfirmDialog),
`delete-preexisting-db`, wide duplicates. Harness-vs-real caveat: mocked IPC,
fixture data — real data lengths (long domains, deep paths) may widen rows
beyond what fixtures show.

## C2. His observations (packaged app, real data — the other half)

_Pending: per-screen list. Dark mode, fairly wide window (noted per item if
size-dependent)._

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
