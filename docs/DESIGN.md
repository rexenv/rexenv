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
- **A thing that works is never shown as missing.** A Linux DNS route an older rexenv wrote still
  routed every name, and 0.8.11 said "resolver MISSING" and opened onboarding's Welcome over three
  sites (30 Sep 2026, #769). The shape on disk is now an amber NOTICE beside "installed" with a
  Re-apply — the sentence is the backend's — and "not installed" is reserved for a route that does
  not route.
- **A choice is reported only once it was made.** The update card said "You chose to keep
  rexenv running" from the moment the swap landed — under the restart dialog, over the quit
  gate's unanswered confirm (#771). The sentence now waits for the fact (`restartDeclined`), and
  an undecided state shows the header's plain fact and nothing more.
- Progress moves only on real completions; 100% only when settled; failure/cancel
  FREEZE the bar in place, never roll it back.
- Refusals name the consequence ("a tunnel would publish X"), never "busy".
- **A summary that cannot fail is not a summary.** Onboarding's last step read
  "Everything's installed ✓ Core components" as static copy — and rendered exactly that,
  on the first clean-VM smoke test (18 Sep 2026), over six Install rows that had ALL failed
  (the VM's DNS relay was dead; every download gave up). The sentence under it promised the
  first site would be served instantly, and the site card then had to explain the
  components were missing. The step now derives ONE verdict from the same rows the Install
  step shows (`useCoreComponents`): ready → the old copy; still downloading → "n of m
  ready", create now and it starts when they land; failed → the count, in the error colour,
  with a Retry that re-runs those rows. The rule: any "done" the UI states about a set of
  things is computed from those things, never typed beside them.
- **A word that means "failed" is never shown for "in progress".** The Sites row's `setup
  incomplete` badge is the claim that a provision job died, and it was drawn from
  `provisioned = 0` alone — which is also the state of every site for the whole of its creation,
  so the row behind the New Site dialog said "failed" while the card in front said "installing"
  (30 Sep 2026, #763). The row now asks the backend's job registry and shows `Setting up` for a
  running job, nothing until it has an answer, and the badge only once no job runs for it.
- **A queue the user cannot see is said where they look.** A PHP pool whose every worker holds a
  request queues the next one silently — a slow page, then nginx's 504 — so its Services row says
  "all 10 workers busy — requests are queuing" as a sub-line in the warning text colour
  (`text-status-warning-bright`, the shade Tunnels' warnings use), from a backend value that turns on
  and off only after two agreeing samples, so a burst does not flicker the row (#608). It names the
  state and its consequence, never a cause it cannot see: the health log's hosts are "recently
  served", because nginx logs a request only when it ends.
- **A feature this Mac's macOS cannot run is listed, disabled, with the reason — never
  simply missing.** On macOS 13 there is no PostgreSQL and no PHP 8.0 (`docs/PLAN-macos-13-floor.md`
  §6.3): the Databases page keeps a muted row, the Settings PHP list keeps the row with its
  Install button disabled, New Site's engine note says it — all from ONE sentence Rust builds
  (`needs_newer_os`: "Needs macOS 14 or later — this Mac runs macOS 13.7."), because a page with
  one row fewer than on another Mac reads as a bug unless it says why (ledger #710).
- **A consent prompt's most load-bearing sentence has ONE source, and it is not the
  TSX.** The Agent access dial (MCP parity, D15) renders "An agent can …" under each
  level from strings Rust serves (`AccessLevel::what_it_allows`, in `AgentAccess.levels`),
  so the sentence the user reads is the one that sits beside the rule it describes; a TS
  copy of it is a guard failure. The share prompt shows the agent's own words for what
  it tried ("It asked to: publish it for 5 minutes"), because a person answers a
  concrete question and not an abstract one. **A dial beats a switch plus prompts when
  the prompts are the product:** the first shape (per site × per scope × per client)
  was honest about consent and cost six clicks for one site's ordinary work; the
  dial's "always" is the same standing yes an auto-allow switch was, said plainly. **And a
  level that already describes the risk does not ask again about an instance of it** (D17):
  publishing a site lives in Full beside "runs code of its choosing as you", because a
  second prompt for a thing the chosen level spells out is friction the user reads as
  noise, not as consent.
- **"Never checked", "checked and nothing", "couldn't reach the server" and "the server
  answered but rexenv refused what it published" are four sentences, not one.** The
  self-update card says which, from ONE backend value carrying the timestamp and the answer
  together — so a failed check keeps yesterday's timestamp instead of ageing into a lie. The
  fourth was folded into the third until 28 Sep 2026: a machine refusing every descriptor as a
  replay read "couldn't reach" for a week, which hid that it would never be offered anything.
  Its sentence is Rust's (`refusal_sentence`), rendered as sent. It never says "up to date" (unprovable before a check has ever succeeded, and
  true only of the instant it ran) and never "update available" (which says nothing about
  whether THIS Mac can install it — the refusals exist because sometimes it cannot). A
  refusal renders where the button would have been, with its fix as a copyable command and
  NO button: offering one that could not work is the lie the whole card is shaped to avoid.
  The consent sentence in front of the button is served by Rust and rendered verbatim —
  a sentence describing a rule lives beside the rule, and the DEV harness mocks a fixture
  sentence rather than the real one so the guard it exercises cannot be defeated by its own
  fixture (ledger #534).
- **A tab that reads a stopped service says so and offers the start.** A stopped site's
  WordPress tab spun "Loading plugins…" for as long as wp-cli took to give up on a database
  that was not running, and its Database tab printed the Adminer URL above a blank frame
  (Windows, 19 Sep 2026) — while the same site's Logs tab named the file, said logging was
  off and how to turn it on. Both now poll what they depend on (the site's engine; for
  Adminer, the web tier too) and render `StackNeededPanel` — the service by name, the
  consequence in one clause, Start all as a button — with the real panels unmounted so
  nothing is asked of a service that is down (#752). The rule: a panel whose data comes
  from a service the app itself supervises never spins on that service's absence; it reads
  the supervisor's own status and says what it says.
- **Two stopped-nesses, two sentences.** A site can be down because rexenv's
  services are stopped, or because the user stopped THAT site (v44). The pill is
  the same shape either way — the site is not serving, and that is one honest
  fact — but the user's own choice reads "Stopped by you" and the tooltip names
  the fix, which is different for each ("Start all" in the footer vs. this row's
  own Start). Collapsing them would send someone to start a stack that is already
  running. The same rule reaches the wire: `site_serving` carries `disabled`, the
  agent view carries `stopped`, and the CLI's `stop` output says every time that
  this is not `rex stop`, because the two are one argument apart and the mistake
  is otherwise silent.
- **A link the stack is not serving is not offered.** The row's and the site page's
  "Open in browser" are disabled while nothing answers the site's URL, with the why
  ("Nothing is serving https://x.rex — Start all first"): on the clean VM (18 Sep 2026)
  the first click after creating a site opened Safari on "can't connect", because the
  stack was still stopped. The user's OWN stopped site is the exception — its URL shows
  rexenv's stopped page, which is the honest thing to look at — so the two
  stopped-nesses above decide it, not the pill colour.
- **A per-site Start reports what is TRUE, not what was asked.** Starting a site
  while the services are stopped records the switch and serves nothing; the toast
  is the backend's own sentence about why, never a success. Same rule as the
  status pill: this app does not congratulate itself for a thing the browser is
  about to contradict.
- **A filter tab's count is what clicking it would show.** The Sites page filters
  on two axes (status, and site type), and each control's counts are computed
  against the OTHER's current selection. Counting all sites in both puts
  "Laravel 3" directly above an empty list, and the page then argues with itself
  about how many sites the user has. The list's own empty state offers "Show all
  sites" as a BUTTON — advice to go and clear a filter is not a way out.
- **A default view is chosen once, not re-chosen by a poll.** Sites opens on
  Running when anything is running (else All), decided on the first load that has
  both facts in hand — an unanswered serving poll means "not asked yet", not
  "nothing is running". Re-deciding on the 2s refetch would move the user's tab
  the moment they stopped their last running site, which is now something they do
  from this very list.
- **A chosen-one-of-N control shows its choice the way the New-site type cards do:**
  `border-brand` (+ `shadow-glow-primary`) on the chosen card with a filled
  `CheckCircle2` and a `text-brand-tint` label; a segmented pill (`bg-brand-tint-bg
  text-brand-tint` on a `bg-rex-well` track) for a short row of durations. Brand colours
  are the `brand.*` Tailwind keys — the Agent access dial shipped with `border-rex-brand`
  / `bg-rex-brand-active`, tokens that exist in tokens.css and emit nothing, and the owner
  saw a dial with no chosen level (D16, 4 Sep 2026). The token guard refuses `rex-brand`
  by name now. And **a section whose only content is "nothing yet" does not render when
  the feature is off**: with the MCP endpoint off, the card is the paragraph and the
  toggle, nothing else.
- **The menu-bar menu says when its numbers are old.** The tray never blocks the menu
  bar waiting for the services lock, so a busy lock means the menu shows the PREVIOUS
  snapshot — labelled `· updating…`, never presented as now. Same family as the rule
  above it: a number whose freshness the user cannot see is a claim, and the claim is
  part of the copy. It also caps the Sites submenu and SAYS what it hid ("…and 5 more"),
  because a truncated list that looks complete is how someone concludes a site is gone.
- **An open menu is not redrawn under the cursor.** The tray refreshes every 5s, and
  macOS closes an open menu when its items are replaced — so a moving number (the count,
  the `· updating…` suffix) used to shut the menu in the user's face a few seconds after
  they opened it. The numbers are now written onto the live items instead, and a rebuild
  happens only when a ROW appears or disappears. The honest-UI half: nothing is frozen to
  keep the menu still — it keeps updating, it just stops slamming.
- **"Saved" and "now running" are different sentences, and a toast may only write the
  one the backend measured.** The PHP Update toast said "PHP 8.2 is now on 8.2.32" from
  a bare `Ok(())` — true when a pool was running, a claim about a process that does not
  exist when none was. `php_update_apply` reports `restarted`, and the copy follows it
  ("…will use 8.2.32 — nothing was running to restart"). The general rule: when an
  action's *effect* depends on state the frontend cannot see, the effect is part of the
  return value, not an inference at the call site.
- **No raw Tailwind hue, anywhere — and `-bright` is the TEXT variant.** A palette
  class (`text-amber-400`, `bg-sky-500/15`) has one value, so it cannot follow the
  theme: it is legible in whichever mode it was written against and washed out in the
  other. That is not hypothetical — the plugin list's update badge and its `→ 5.7.2`
  target shipped that way and were reported as unreadable in light mode, while every
  automated check stayed green, because a hue outside the design system has no
  light-theme value for the contrast scan to compute. Use `status-*` for
  running/warning/error, `rex-accent-*` for the hue chips, and add a token rather than
  reaching for the palette; `no_raw_tailwind_hue_reaches_the_ui` fails the build
  otherwise. Within a status family the plain token (`--rex-warning`) is the FILL or the
  dot and the `-bright` one is the text — the light theme darkens the `-bright`
  variants specifically so text clears AA on light surfaces, so text on the plain
  variant reads at 4.1–4.4:1 there. `text-white`/`bg-black/50` stay allowed: the brand
  button's label, the toggle knob and the dialog scrim are deliberately the same colour
  in both themes.
  **A token name states a ROLE, and a role can point opposite ways per theme** (24 Aug
  2026, ledger #395). `--rex-brand-light` is the brand AS TEXT, not a lighter violet: in
  the dark theme that means lighter than the brand, in the light theme DARKER, and it had
  the light-theme value pointing the wrong way at 4.35:1 on white. The same shape cost
  more elsewhere the same day — `--rex-placeholder` is documented for decorative
  unbuilt-screen text and was colouring the Sites list's real column headers at 2.11:1.
  **Four of the six AA failures found that week were the wrong token doing a job it was
  not for, not a wrong colour**, and the fix in each case was to use the right token
  rather than to darken the one in the way, which would have dragged its real consumers
  with it.
  **A background that is not a surface token has nobody computing it.** The two letter-tile
  accents had each been darkened once already, against `surface-3`, and still failed on the
  tile: a tinted background composited from an `-bg` token, which no text/surface pairing
  covers. `wk-checks/contrast.js` reads what a pixel actually is — painted ancestor, alpha,
  the AA size threshold — which is why it sees these and L0 cannot.
  **Opacity dims ornament, never text.** A stopped Services row faded the whole row at
  `opacity-[0.74]`, and group opacity composites the entire subtree over the page — so
  every string in it rendered at an alpha nobody had checked: ports, CPU/RAM labels, the
  Idle pill, the letter tile, all between 3.09:1 and 4.45:1. The arithmetic leaves no
  room to tune it: `--rex-text-muted` sits at 4.91:1 on the pill background, so the
  lowest alpha that still clears AA is **0.94** — a dim no one can see. Stopped now reads
  from a DESATURATED badge (neutral tokens, full strength) plus the Idle pill and the off
  toggle, which says the same thing without touching a single contrast ratio. Same fix on
  the three `opacity-70` counters, which were dimming an already-muted token. The one
  exemption is the standard's own: WCAG 2.1 SC 1.4.3 Incidental covers *inactive user
  interface components*, so a `:disabled` button's faded label is not debt — the probe
  skips `:disabled`/`[aria-disabled]` subtrees and nothing else.
  **A probe that does not parse is a probe that passes.** The ancestor-opacity
  compositing above and a stray backtick inside `contrast.js`'s template literal shipped
  in the same commit; the file became a `SyntaxError`, so the 44 pairs the sharpened
  probe had just started catching went unseen until the next full sweep. `verify.sh` now
  runs `node --check` over every `wk-checks/*.js` — the probes themselves need a browser
  and a dev server, but their SYNTAX is free to gate on every commit.
- **An embedded surface follows the app's theme, not the OS's.** The Adminer console
  renders in its own process off its own stylesheet, so it defaulted to
  `prefers-color-scheme` — and a rexenv set to Light framed a dark console, which reads
  as a broken embed rather than a third-party default. The app writes its RESOLVED
  palette where that surface can read it, and reloads it on change. Resolved, never the
  preference: passing "system" through would leave two processes each asking the OS,
  which is not the same as agreeing.
- **A search box searches what the row SHOWS.** The installed-plugin filter matched
  the slug while every row is labelled with its title, so typing "loopback" against a
  list plainly reading "rexenv loopback DNS" (slug `rexenv-dns`) answered "No plugins
  match" — and a filter searching a string the user cannot see is indistinguishable
  from a broken one. It matches BOTH now: the displayed title and the slug, since the
  slug is what someone pastes from a folder name or a wp.org URL. The rule generalises
  to any list that displays one field and stores another, and `wpsearch.js` holds it
  in the rendered list — plant-proven in both directions, because a fix that swapped
  the fields instead of adding one passes every test written for the new half.
- **A filter may hide a row; it may never hide an EXPOSURE.** Every search box in
  rexenv shrinks a list, and everywhere else that is free: the rows it removes are
  still exactly where they were and nothing about them changed. On Tunnels a hidden
  row is a site the whole internet can reach right now, so the page counts what the
  filter took away and says it in amber — "2 shared sites are hidden by this filter —
  still public until you stop sharing" — including on the no-match screen, which is
  the one a person is most likely to leave the page from. For the same reason the
  header keeps the MACHINE's numbers while a filter is active: the subtitle still
  reads "2 sites shared publicly", and Stop all sharing still stops all of them,
  because both describe the machine and not the view. The L2 probe asserts the count
  and the consequence clause, not just that some warning rendered.
- **One fact per row, and Adminer has one.** The PHP row carries `updatable` AND
  `upstream` because static-php.dev lags php.net, so "exists" and "installable" are
  genuinely different. rexenv downloads Adminer's OWN release asset, so for it they are
  the same thing — the Adminer card therefore has no "exists" chip, and the L2 probe
  fails if one appears. Copying a pattern because it looked good next door is how a
  sentence about honesty becomes a falsehood.
- **The version a thing IS running and the version it WILL run are separate fields, in
  every row that has both.** The Adminer card shows what is staged, and says
  "→ X on next start" ONLY when the two disagree. A console already on the chosen
  version is not a discrepancy, and painting it as one is how a completed update reads
  as pending — the same defect the PHP row's `serving` chip was fixed for. "Nothing
  staged yet" is a third sentence, not a blank.
- **A chip that means "there is no button for this" must vanish when a button appears.**
  The Settings PHP row carries two upstream facts: `updatable` (a signed manifest offers
  it — a button) and `upstream` (php.net lists it; rexenv may have no verified build yet
  — the "· 8.2.32 exists" chip). Once static-php.dev catches up the two are the same
  version, and rendering both put "8.2.32 exists" beside "Update to 8.2.32", which reads
  as two different versions. The chip renders only when `upstream !== updatable`, and the
  paragraph explaining it ("'exists' is not a button") renders only when such a row is on
  screen — it was previously printed on screens that had an Update button on them.
- **An action that finishes somewhere else says so.** Every button on the repo panel
  (Fetch, Pull, Push, Build zip, Check deps, Run: <script>) starts a streamed job whose
  only completion signal was a glyph inside the job card — so with the panel scrolled
  away, or the user over in the browser they clicked Pull to refresh, a finished job
  looked exactly like one that never ran. Each step now toasts its OUTCOME once, naming
  the action and the asset ("Pull — my-plugin finished"); a failure quotes the first
  line of the tool's own error and stops (the rest is the card's and the log's job);
  steps that are still pending, or were skipped because an earlier one failed, stay
  silent rather than reporting work that did not happen. The rule generalises: if the
  visible state does not change on its own when the work lands, the work has to say so —
  which is why plugin and theme Activate/Deactivate/Delete/Install/Update now report
  too: the row they flip is routinely off screen on a 26-plugin list. What they may
  CLAIM is bounded by what is known: an install announces its settled job (with
  `partial` kept as its own outcome, not rounded to success or failure) and says nothing
  when it merely starts; an update states a count only on the path where wp-cli's exit
  code proves every item landed.
- **A panel showing something rexenv does not own re-reads it when the user comes
  back — and offers a way to ask.** Plugin/theme state lives in WordPress and the
  git branch lives in the checkout; both change from wp-admin, a terminal, or the
  Terminal tab, with no event to tell us. Those lists were cached for 30s with
  window-focus refetching OFF, so the app confidently showed the opposite of reality
  until the user left the tab and returned — the fix that "worked" was the one thing
  they had to discover. Now: the NATIVE window's focus event drives the refetch
  (`lib/window-focus.ts` — the webview's own focus/visibility events are unreliable
  inside wry, which is why the flag looked enabled and did nothing), plus a visible
  Refresh control for the case focus cannot cover — a change made while rexenv
  already HAS focus. Cost is bounded on purpose: only the open panel's queries
  refetch, and only local reads join in (the PR-ref `ls-remote` stays lazy).
- **A counter that cannot advance is not shown at all.** The install card's "installing
  item k of N" advances on wp-cli's per-item header — which only the wp.org path
  prints. Adding "Upload zip" as a second source of the SAME job would have parked that
  line at "item 1 of 2" for the whole run: clamped, plausible, and false from the first
  item onward. It is omitted for zip jobs instead, and the bar (which is built to run
  BEHIND on missing lines, never ahead) carries the progress alone. Generalised:
  reusing a card for a second source means re-asking which of its numbers that source
  can still feed — nothing beats a stale something.
- **Consent moves earlier when a flow gets shorter — it does not evaporate.** "Add
  plugin/theme from Git" runs the repo's own code one explicit click at a time, with the
  disclosure above the buttons. Creating a SITE from a repo can't be click-by-click
  without being a worse product, so the same disclosure moved to sit above **Create**,
  naming the three commands that will run (`composer install`, `key:generate`,
  `migrate`) and what they run against. The test for whether a batched flow is still
  honest is not "did we ask" but "was the sentence in front of the button that starts
  it". `composer install` deliberately has no opt-out: `vendor/` is gitignored, so
  offering to skip it would be offering to create a site that 500s — an option that can
  only produce a broken site is not a choice, it is a trap.
- **An offer that takes something away is not the same offer, and must not look
  like one.** Every update button in this app means "strictly better" except one:
  a PHP patch from upstream has no PostgreSQL driver, so updating a minor rexenv
  builds removes it from every site on that version. A version number cannot show
  that, so the row carries a visible **costs PostgreSQL** chip and the sentence —
  which sites, and what stops working — in the button's own title. A tooltip
  nobody hovers is not a tell; the chip is what makes the offer read differently.
  It does not refuse: the machine is the user's, a security patch is a real
  reason, and the honest thing is to let them weigh it rather than decide for
  them. What is not acceptable is a cost discovered afterwards (ledger #552).
- **A field the user can see and cannot use is either a choice or nothing.** The New
  Site dialog's Database field rendered a flat, unclickable "None" for Blank PHP — a
  control that looked like the other two beside it and answered nothing. It is now a
  real choice for that type (MySQL / MariaDB / PostgreSQL / None, MySQL default), and the answer is
  load-bearing: picking an engine gets the site a database, a seeded table and a
  generated `db.php`; picking None skips the ~600 MB engine download. The note under it
  says which of the two the click will do, because "MySQL" alone does not tell a first
  user that a download is about to start. **The stale sentence is part of the change**:
  two other surfaces read "Blank PHP sites have no database", which was true of all of
  them and is now true of half.
- **A control whose only outcome is an error is not rendered.** The New Site dialog
  offers "From Git" for Laravel and Blank PHP and omits it for WordPress, because a
  WordPress checkout without its database is not a site and the backend refuses it. The
  refusal still exists (CLI and MCP reach the same code) — the UI simply does not draw a
  button to walk into it. Same rule that removed the blueprint field from non-WordPress
  types.
- Unknowable state says so ("can't determine") instead of guessing.
- **A capability that goes away is explained where it goes away, and the explanation
  never replaces the tool's own words.** Pinning the wp-cli command set (#228) means a
  user's global packages stop extending rexenv's `wp`. The tell (#301) is a standing
  Settings card AND an explanation APPENDED to `not a registered wp command` at the
  moment it bites — appended, because WP-CLI's own line is the string that finds an
  answer in a search box, and a friendlier message replacing it is a net loss. The card
  also names what is EXCLUDED (the packages) and what still works (rexenv's terminal):
  a scoped change reads as a capability removal without that second half, and it is
  always the half a trim removes as reassurance.
- **One condition with two causes gets two messages, and the fix that is a BUTTON is
  the button.** "Nothing is on :443" and "something else is on :443" are one probe
  result and two different problems. Telling someone to quit an app when nothing is
  listening is worse than saying nothing: it sends them hunting for a program that
  isn't running. So the import caveat says which it is, and when the answer is "rexenv
  isn't serving yet" it carries **Start all** as an action rather than advice to go and
  find it — a fix that lives in this app should never be rendered as a description of
  where to look for it.
- **A count is never rendered from a read that failed.** The same card names packages
  from a `composer.json` it may not be able to parse. Every failure yields a variant
  that claims no count, because "the 0 packages in ~/.wp-cli/packages" is not a
  degraded answer — it is a confident wrong one about the reader's own machine. Guessing
  low is still guessing.
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
- **A control that is working still has to SAY what it is working on — and stay the
  size it was.** Adminer's Update button swapped its whole label for a bare `…` while
  the download ran, so the one moment it had news is the moment it stopped speaking:
  the version being installed vanished, the button shrank to the width of an ellipsis,
  and the row read as an empty box next to the word "Adminer" (reported from a
  screenshot, which is how every defect in this family has arrived). The running label
  keeps the sentence and gains a spinner — `Updating to 6.0.1…` — because a busy
  control's job is to name the work, not to hide it. Sizes matter as much as words
  here: the collapse is what the eye actually notices, and no assertion about TEXT
  would have caught it, which is why the probe measures the button before and after
  the click.
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
  **What the gate covers, scoped 14 Aug 2026 when Reset arrived:** actions that
  destroy a WHOLE artifact rexenv cannot rebuild — a site, a database. The
  Repository panel's Reset (`git reset --hard HEAD`) is irreversible too, and it
  gets the plain danger confirm instead, for two reasons that have to hold
  together: it is a ROUTINE unstick (a composer/npm step rexenv itself ran
  dirtied the tree, and the next checkout is refused), and the recoverable
  alternative is one button to its left — the dialog names it ("Stash instead if
  you might want any of it back"). A gate on the routine path is the gate people
  route around, and the way around this one is a terminal, where there is no
  confirm at all. The confirm earns its keep by saying what it does NOT take:
  untracked files and ignored paths survive, with the counts read live.
- **Chrome may lead with a RECORDED fact, never with a guess.** Where a slow probe
  decides what to render (`wp-info` boots WP-CLI three times), render from what we
  already recorded (`site.type`) and let the live answer correct it — the fix for
  pop-in is an earlier true source, not a placeholder.
- **A log may be FOLDED, but its size stays on screen.** Settings › AI agents
  kept a live feed of every agent call open under the two controls that matter
  (the endpoint toggle and the Agent access dial) and pushed them off a 900px
  window; a log nobody scrolls to is not read more often for being open. It is
  now behind a disclosure whose header carries the count — "Recent activity · 12
  calls", or "· nothing yet". The count is the part that cannot be folded: the
  consent paragraph promises "every call an agent makes is listed below", and a
  fold that hid whether there were any calls at all would quietly stop being
  that promise. Same reason Clear only appears with the rows — a button that
  empties a list you cannot see is a button pressed by accident.
- **With the endpoint OFF the Agent access dial is gone, but its way DOWN is
  not.** Three level cards and a duration row are a decision about a socket that
  is not open. What cannot go is lowerability: a level above Read is the user's
  setting, it survives the socket, and it applies the moment the toggle goes on
  — so the standing sentence under the toggle ("Agent access is set to Full ·
  Always and applies as soon as you turn this on") carries a "Set it back to
  Read" button. Hiding the whole thing would make the setting un-lowerable
  without first turning the endpoint back on, which is the one order of
  operations nobody should have to perform to REDUCE access.
- **A setting's heading carries the ANSWER.** "Agent access" over three cards
  made a reader walk all three to find the lit one; the heading now ends in a
  chip stating the standing level and its duration ("Changes · 7 days"),
  derived from the same status the cards render. And the way DOWN sits with the
  ways up: "Back to Read now" under the duration, because someone looking to
  stop an agent looks for a button that says stop, not for the top of a list.
- **An icon-only control needs three things, or it is a rebus.** The conventional
  glyph (gear = settings, trash = remove, star = default — not an invented one), a
  `title` AND an `aria-label` naming the version it acts on, and a printed LEGEND
  under the list. Settings › Services › PHP versions is the case that set the rule
  (8 Sep 2026): seven rows each carrying four grey text buttons — "Make default",
  "Installed", "Settings", "Remove" — wrapped to two lines apiece, and "Installed"
  was a WORD sitting in the button row that could not be pressed. State moved to a
  dot, verbs moved to icons, and the row fits one line. The legend is not
  redundant with the tooltips: hover text only reaches the reader who already
  suspects what the icon does. Two controls stay WORDED on purpose — Install and
  Update, the only ones that download ~100 MB and restart a pool serving live
  sites, and the Update button names the version it moves to.
- **A control that cannot act is absent, not greyed.** The default PHP version
  shows no star and no trash — something has to serve new sites. One disabled
  trash among seven live ones asks the reader to work out why; a missing one asks
  nothing.
- **A button that hands work to another app wears THAT app's icon, extracted, or
  its own glyph — never an invented one.** "Open in browser" and "Open in editor"
  show the real icon of the app the click will use, read off the installed bundle
  (`AppIcon`, `components/ui/app-icon.tsx`). A hand-drawn brand table was rejected
  twice over: it would hardcode vendor hex against the token rule, and it would go
  stale on every rebrand. When the icon can't be read (artwork only in a compiled
  asset catalog) the component renders the caller's monochrome lucide glyph —
  `null` is a fine answer, a wrong logo is not. The label follows the same rule:
  the tile says "Open in Chrome" only when a browser is actually resolved.
- **One icon per kind of hand-off, on every surface.** EVERY control that opens a
  URL in the browser wears `PreferredBrowserIcon` (`components/ui/open-in.tsx`) —
  the site header and quick tile, the Sites row, a network sub-site's Visit, the
  tunnel's public URL, Open Mailpit, Adminer's Open in browser, Change domain's Open
  site — and EVERY magic login wears `WordPressIcon`: the header and tile, a
  sub-site's Magic Login, One-click admin login, and a user's Log in. Until 13 Sep
  2026 the header showed the browser while the Network tab, Tunnels, Mailpit and
  Adminer showed a generic arrow or globe and three logins a door; the owner asked
  for "sob jaigai consistency". Not covered, on purpose: Settings → About's web links
  keep their arrow-out (it promises "leaves the app", not "visits a site"), and a
  log's Open file opens the default app, not a browser. Guarded by
  `open_in_browser_and_magic_login_wear_one_icon_everywhere`.
- **A chevron appears only when there is something to choose between.**
  `SplitButton`/`QuickTile` drop the chevron entirely when the menu is empty
  (`useBrowserMenu`/`useEditorMenu` return undefined below two apps) — a control
  that opens a one-item menu can't change anything, and the seam it adds reads as
  a promise the machine can't keep. The terminal chevron (`useTerminalMenu`) is
  the exception that states the rule: its menu is not "the same action, elsewhere"
  but a DIFFERENT destination — the plain click opens rexenv's built-in Terminal
  tab, the menu opens the folder in the user's own terminal app — so one entry is
  already a choice, and it shows. The two halves are SIBLING buttons: a nested
  `<button>` is invalid HTML and WKWebView drops the inner click. **Every chevron
  carries a visible SEAM** — the divider is the affordance, not decoration: the
  quick-link tiles shipped without one and read as a single wide button with an
  arrow glued to it, so "what happens if I click there" had no answer (reported
  11 Aug). Bordered variants collapse their facing borders into the seam;
  `primary` draws no border at all and needs its own divider. `openin.js` asserts
  a non-transparent `border-left` on every chevron, whatever the variant.
- **A toggle that promises "everything" states what it cannot reach, on the same
  screen.** The mail catch-all (Settings → Services) says every site's mail goes
  to Mailpit — and three routes escape it: a Laravel app that has run
  `php artisan config:cache` reads its baked config, a plugin mailing through a
  provider's HTTP API never touches PHP's mailer, and a command in the user's own
  terminal is outside rexenv entirely. The card carries those three in plain
  words while the toggle is ON, because the alternative way to learn them is a
  message that reached a real customer. Same rule as the private-window icon
  above, from the other end: there the affordance is WITHDRAWN where the promise
  cannot hold; here it cannot be withdrawn, so the limit is written down. **And
  the OFF direction gets a sentence too** — "your sites now send mail for real"
  — since it is the only control in the app whose off position lets a
  development machine reach a stranger's inbox.
- **A menu row may carry a SECOND target, under the same seam rule.** `MenuItem`'s
  `action` splits an icon off behind a divider — "open it in that browser's
  private window" — as a sibling button that hovers on its own, so which half is
  about to fire is never a guess (same nested-button reason as the chevron). It
  renders ONLY where the action is real: Safari has no private-window command
  line, so its row shows nothing rather than an icon that would open an ordinary,
  recorded window. An affordance whose promise is "this isn't recorded" cannot be
  offered on a best-effort basis — that is the honest-UI rule at its sharpest,
  because the user would see the window they asked for and never learn otherwise.
  Its glyph is the private-browsing IDIOM — hat + spy glasses
  (`common/IncognitoIcon.tsx`), the mark every browser's private window has
  trained people to read — drawn in `currentColor`, never a vendor's own mark or
  hex, since the same icon rides the Firefox and Brave rows too. Lucide has no
  incognito icon (its nearest is a carnival mask, which is not what anyone is
  scanning for). Filled hat, OUTLINED lenses, decided by rendering it at the
  15px it ships at rather than at 120px: two filled discs joined by a bar read
  as a dumbbell there, rings read as glasses at every size.
- **A runtime rexenv offers that receives no security fixes SAYS SO, where it is
  chosen.** rexenv shipped PHP 8.0 from Nov 2023 and 8.1 from Dec 2025 with no
  tell anywhere — `grep -ri 'eol' src/` returned zero — so the picker gave a dead
  version the same face as a supported one. That is the "sentence in front of the
  button" rule failing in the quietest possible direction: nothing is wrong on
  screen, the user simply was not told. The tell rides the registry row
  (`eolSince`, computed in core against **today** from php.net's published end
  dates, so it becomes true on the day it becomes true and no one has to remember
  to flip a flag), and appears in all three places a version is chosen or lived
  with: the Settings row's badge, the create dialog's note, and the site's own
  Environment card — because most sites on a dead runtime got there by import or
  by outliving the version, never by picking it in a dialog. For WordPress it
  also names, in advance, the outdated-PHP notice WordPress will put on the site
  itself; a warning the user meets first from us reads as information, and the
  same warning met first from WordPress reads as a rexenv bug. **A badge on the
  newest dead version alone would be worse than none** — it implies the others
  are fine — which is why 7.4 could not ship until 8.0 and 8.1 were covered too.
- **A one-off detour is not a preference change.** Picking a browser from the
  chevron opens that one link there and leaves the default alone. Magic Login
  carries the same chevron, because it too ends in a browser — its url is minted
  on click (a one-time token can't be parked in a menu that may never open) and
  BOTH paths share one resolver, so the no-auto-login fallback can't differ
  between them; the default
  moves in Settings, where the picker also shows what "System default" currently
  resolves to. A menu that quietly rewrote the setting is how "why does
  everything open in Firefox now" starts.
- WKWebView is the shipping engine: verify layout/metrics in the WebKit harness
  (`scripts/wk-checks/`), not Chrome — pill widths, %-height chains and dialog
  behaviour all differ there.

## Windows — where the same design lands differently (W9, Sep 2026)

One visual system, two hosts. The rules above are unchanged; these are the four places the
SAME design has to be drawn or spoken differently, each measured on the Dell rather than assumed.

- **The window wears the OS's own title bar.** macOS draws its traffic lights OVER the page
  (`titleBarStyle: "Overlay"`), so the sidebar reserves a ~12px row for them; Windows draws a real
  title bar above the page, and reserving there leaves a dead strip under it. The sidebar renders
  that spacer behind `windowControlsInContent` (`platform/words.rs`, ledger #636) — never
  unconditionally. `titleBarStyle`/`hiddenTitle` are macOS-only keys in Tauri; there is no Windows
  equivalent to "fix", and fake traffic lights would be exactly the dishonesty the rules above forbid.
  The drag region stays ours on both.
- **The browser's keys are not the app's keys.** WebView2 ships Ctrl+R, F5, Ctrl+P, Ctrl+F and F12
  enabled, so a desktop app reloads to a blank page on a stray Ctrl+R. rexenv asks the webview for
  `SetAreBrowserAcceleratorKeysEnabled(false)` at startup (ledger #637); Ctrl+A/C/V/Z keep working,
  because those are the text field's, not the browser's. Verified on the Dell, key by key.
- **The OS's own nouns come from the platform, never from a screen.** No component writes "Finder",
  "login keychain", "your Mac", "menu bar", `~/…` or `/etc/resolver`: they are fields of
  `PlatformWords`, served by one IPC call, and two scans — one over `src/**.ts(x)`, one over both Rust
  crates — fail the build if a literal is typed back (ledgers #626, #638, #639, #640). This is a
  design rule, not a plumbing detail: a Windows user told to look in Finder has been shown someone
  else's computer. Where the word is not just a name but a place, it names what the code actually
  does — Windows' "NRPT rule" is what `WindowsDns::route_label` says, and the app-search tooltip lists
  the folders `app_catalog.rs` really reads.
- **The three type families ship with the app.** Inter, JetBrains Mono and Space Grotesk come from
  `@fontsource`, bundled, so the Windows build renders the same roles rather than falling back to
  Segoe UI. (Written down because the first answer here was wrong: the fallback was assumed, then the
  bundle was checked, then the Dell was looked at.)

The costs are recorded where they were paid: `docs/PLAN-windows-port.md` §5 W9, with the Dell runs.

## Intentional divergences from the comps — do NOT "fix" toward them

The comps predate the real architecture. Correct as shipped:

- **Web-server picker offers Nginx / FrankenPHP / Apache** (comps show
  Apache / OpenLiteSpeed as the pair): Apache since shipped; OLS is blocked
  upstream — no macOS binary exists (`docs/TODO.md` Blocked).
- **DB engines: MySQL / MariaDB / PostgreSQL per site + Redis standalone** (comps
  predate MariaDB/Redis shipping; PostgreSQL became a SITE engine 10 Sep 2026).
  **An option a site cannot have is not shown**: PostgreSQL is absent for
  WordPress (`wpdb` speaks MySQL alone) and on a PHP whose build has no working
  `pdo_pgsql` — and the field says WHY in the second case, because a control that
  quietly has fewer options than it did on another site reads as a bug. The first
  case says nothing: telling a WordPress user about a PHP version answers a
  question they cannot ask.
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

### The Domains card (2 Sep 2026)

A site's extra hostnames live on its Settings tab, and the card follows the
honest-UI rule the hard way: **it renders the list the backend returned**, not
the one the user typed. Every mutation replies with the full list, so a name is
on screen only if the server confirmed it — an added domain is served, or it is
not shown. The PRIMARY is listed and marked, and has no Remove: its files,
database and certificate folder are named for it, so removing it is Change
domain, an operation with a different blast radius and its own confirmation.
`scripts/wk-checks/domains.js` holds both halves, and its control leg (the
untouched extra surviving a remove) is what stops "gone" from proving nothing.

### The third accent to fail the letter tile (2 Sep 2026)

`--rex-accent-periwinkle` read **4.42:1** on the site-type avatar in light mode.
Blue and red had each been darkened for exactly this background (#337, #372) —
the tile is lighter than the surface those hues were tuned on — and periwinkle
survived for one reason: **no mock site was Blank-PHP**, so the tile that fails
had never rendered in the harness. It was found the day a fixture gained a
`php`-type site for an unrelated reason.

The lesson is the fixtures one, in its sharpest form yet: a palette is only
checked where something is DRAWN, and a fixture that omits a whole site type
omits a whole colour. Periwinkle is `#54589f` now (4.98:1), and the mock keeps
one site of every type so the next accent cannot hide the same way.
