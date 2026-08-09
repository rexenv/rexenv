# DESIGN — the visual system and the decisions behind it

The live design reference (extracted 28 Jul 2026 from the historical
`docs/archive/DESIGN_BRIEF.md` + `docs/archive/DESIGN-GAPS.md`, refreshed to
as-shipped). Token values live in `src/styles/tokens.css` + the Tailwind theme —
never hardcode hex. Screen comps (`design/*.dc.html`, one per screen; historical,
aspirational) were removed from the tree pre-publication (2026-08-08) — recover them
from git history if needed, and see "intentional divergences" below before "fixing"
the shipped UI toward one.

## Design DNA

- **Concept:** "rex" = king — the app is the developer's **command room** for their
  local kingdom: a calm, fast control panel where they run every server/site/
  database and see at a glance what's running.
- **The boldness is spent in ONE place — the live status system**: status pills,
  start/stop toggles, and the global resource meter, crafted to feel alive.
  Everything else stays quiet and disciplined. Plus a restrained violet crown mark.
- **Palette (dark-first):** bg `#0D0E12` · surface-1 `#15171D` · surface-2
  `#1C1F27` · border `#262A33` · text `#E7E9EE` · muted `#8A90A0` · brand royal
  violet `#7C5CFF` — chosen because it reads regal AND doesn't collide with the
  status colours: running `#3FB950` · stopped/idle `#6E7681` · error `#F85149` ·
  warning `#D29922`. Violet is used sparingly: primary actions, active nav, focus
  rings, logo. Light theme is a first-class variant of the same tokens.
- **Three deliberate type roles:** display = **Space Grotesk** (hero/onboarding
  ONLY, restrained) · UI/body = **SF Pro Text** (native feel; Inter fallback) ·
  mono = **JetBrains Mono** for ALL technical values (domains, paths, versions,
  ports, commands, logs). If a value could be copy-pasted into a terminal, it's mono.
- **Avoid the AI-design clichés** the direction was defined against: cream + serif
  + terracotta; near-black + a single acid accent; broadsheet hairline columns.

## Honest-UI rules (design-level, enforced in code)

- Status derives from reality (ownership AND liveness) — no fake per-site toggles.
- Progress moves only on real completions; 100% only when settled; failure/cancel
  FREEZE the bar in place, never roll it back.
- Refusals name the consequence ("a tunnel would publish X"), never "busy".
- Unknowable state says so ("can't determine") instead of guessing.
- **A batch reports the job it is actually running** — the step line is that job's own
  phase label verbatim, the count moves only on terminal rows (a FAILED row still
  counts), and a long child job (a multi-GB dump) keeps the bar alive on its own
  phases. Minutes of silence is a bug, not a quiet success.
- **A long tool call shows the TOOL's own steps, not a spinner.** A plugin, theme or
  core update is one wp-cli call that can run for tens of seconds (WooCommerce,
  Elementor, a full core release), so it streams WP-CLI's upgrader phases and the bar advances on items wp-cli announced as
  settled plus the current item's STEP position — never on elapsed time, and never as
  a byte percentage, which wp-cli does not report. Items it hasn't reached read
  "Queued" instead of a bar at zero, and a failed item is banked but labelled
  "Failed". The version an update installs is shown BEFORE the click (`v10.8.1 →
  10.9.0`) on plugins and themes alike, because "update" alone made the user run it
  to find out.
- **A bar ends when the WORK ends, and a stale badge may not outlive it.** Two ways
  this broke on first real use, both worth remembering: a progress callback that
  returns its refetch promise keeps the run "pending" through a slow re-check, so the
  bar sits at 100% doing nothing; and invalidating a slow list REFETCHES rather than
  erases, so its cached "update available" row is merged back over work already done
  and the badge reappears for a few seconds. Drop what you know is now false instead
  of waiting for the slow truth to catch up.
- **"Update to X" over something already at X is a lie the UI must not be able to
  tell.** Dropping the stale row was not enough: the slow check that was already
  RUNNING when the update started landed afterwards and put the badge back, so the
  button stayed for as long as the next check took — minutes on a site full of
  premium plugins. The durable half is the rule, not the plumbing: a row's update
  claim is checked against the version on disk (`verdict`), so no source — stale
  cache, in-flight check, a plugin's own updater caching for hours — can render one.
- **A floor may sit on the spinner, never on the work.** Operations too fast to see
  (the `/import` rescan) get a ~550ms minimum spin so the click reads as an action —
  paired with the real timestamp of what's on screen (`scanned 12s ago`), so the proof
  it ran is a fact, not the animation.
- **An irreversible action is gated by typing its own domain — and the domain is
  copyable.** Site delete, site reset and db-import overwrite all use ONE gate
  (`ui/type-to-confirm.tsx`, `ConfirmDialog confirmPhrase=`): the phrase carries a
  copy button, the input takes the focus the default button used to (so Enter fires
  nothing), and every destructive button in that dialog stays disabled until the
  typed value matches. Delete used to be a plain OK — one stray Enter from losing a
  site — while reset made you retype a domain you couldn't select. The copy button is
  half the rule: a gate people can only satisfy by retyping from memory is a gate
  people learn to skim. Match is trimmed, because a pasted value drags whitespace.
- **Chrome may lead with a RECORDED fact, never with a guess.** Where a slow probe
  decides what to render (`wp-info` boots WP-CLI three times), render from what we
  already recorded (`site.type`) and let the live answer correct it — the fix for
  pop-in is an earlier true source, not a placeholder.
- WKWebView is the shipping engine: verify layout/metrics in the WebKit harness
  (`scripts/wk-checks/`), not Chrome — pill widths, %-height chains and dialog
  behaviour all differ there.

## Intentional divergences from the comps — do NOT "fix" toward them

The comps predate the real architecture. Correct as shipped:

- **Web-server picker offers Nginx / FrankenPHP / Apache** (comps show
  Apache / OpenLiteSpeed as the pair): Apache since shipped; OLS is blocked
  upstream — no macOS binary exists (`docs/TODO.md` Blocked).
- **DB engines: MySQL / MariaDB per site + PostgreSQL / Redis** (comps predate
  MariaDB/Redis shipping).
- **Databases screen stays ENGINE-focused** — one row per engine with status +
  CPU/RAM + start/stop + Browse. The comp's per-schema re-model (schema rows,
  per-row export/import/drop) was **deliberately not adopted** (decision recorded
  in the design burndown §6): it needs a schema-enumeration backend and a larger
  UI for little gain, and the engine view matches the Services grouping.
- **Light theme is selectable and applied** though the Settings comp gated it "soon".
- **Terminal tab, Blueprints card, Uninstall card, blueprint selector in New Site,
  dynamic PHP version list** — real features the comps never showed. Keep them.
- When a designed control's datum doesn't exist yet, build the chrome and wire
  what exists; leave a clear TODO rather than faking the number.
