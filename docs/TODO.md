# TODO — the single active-work file

Everything open lives here, and ONLY open work lives here. Shipped rows move to the
month's evidence log (`docs/archive/SHIPPED-2026-07.md`, `-08.md`, `-09.md`) with
`scripts/todo-reconcile.py` — the mechanical half; the judgement half is the
`reconcile-todo` skill. Tick an item in the commit that does the work, with a one-line
✓ evidence note. `scripts/todo-reconcile.py --count` prints the open/ticked tally.

**Reconciled 5 Sep 2026, second pass** (HEAD `369c471`, after v0.4.0 + 205 commits, the
0.5.0 release cut). Two ticked blocks moved (mcp.log; MCP D2 settled) and the emptied
*Decisions pending (owner)* section went with them — every MCP decision is now closed.
Fourth reconcile; each has found the same shapes, so they are the checklist:

- **Ticked rows pile up** — 57 on 21 Aug, 93 on 5 Sep (173 ticked boxes hiding 46 open).
  Now scripted, so the move costs nothing and the rule can hold.
- **Open only on paper** — work landed in a commit that never touched this file (six rows
  on 21 Aug; on 5 Sep the second-instance guard #441 sat under a struck-through title with
  its box still open, and the per-site-lifecycle parent stayed open after every child closed).
- **A row describing a guard that does not exist** — the dangerous kind (`wp_dns_check`,
  21 Aug; closed 23 Aug as ledger #381). Read the code the row names before trusting it.
- **Shipped narrative inside an open row** — the MCP decisions row ran 530 lines with one
  open decision in it. A row is open for ONE reason; say it in the first line.

## Now — actionable code/test work

- [ ] **In-app self-update — a dmg user has no update path at all**
  — 6 Sep 2026, planned in `docs/PLAN-self-update.md`; supersedes the Phase 4+ row
  "Packaging polish: Tauri updater" (`docs/archive/TASKS-RELEASE.md` §6.1), which
  proposed the wrong mechanism. `brew upgrade --cask rexenv` is the only updater
  today, so everyone who installed from the dmg re-downloads and drags a new copy
  over — and that path silently leaves the KeepAlive DNS agent running the OLD
  binary, which this work also fixes. Thirteen tasks, T0–T12 in the plan.
  **T0 is a MEASUREMENT and comes first**: whether macOS App Management lets an
  ad-hoc-signed bundle rename itself in `/Applications` is documented nowhere, and
  the swap's error handling (and in outcome O4, whether an in-app install ships at
  all) is a function of the answer.
  - [x] T0 — the swap probe on a real Mac ✓ 6 Sep 2026 — **O1** on macOS 26.6.2 (25G83):
    `renamex_np(RENAME_SWAP)` on an ad-hoc bundle in `/Applications` works, so does the
    rename pair, the swapped copy relaunches with no quarantine and no dialog, and the
    quarantine leg translocates exactly as R6 assumes. Recorded in
    `docs/PLAN-self-update.md` §T0 with the raw results; §6/§7/#517 rewritten to match.
    Found on the way: App Management does not protect an ad-hoc bundle **at all** —
    in-place writes into the launched bundle succeeded too, which is a note for
    `docs/SIGNING.md`'s case rather than a change to the design.
  - [x] T1 — `core/app_update.rs` trust core, the shared verify seam, `macho::archs`
    ✓ 6 Sep 2026 — ledger #518–#522; 15 L0 tests incl.
    `both_manifest_modules_verify_through_one_seam` (one flipped byte, both documents
    must refuse) and `archs_reads_a_fat_header_and_a_thin_one_and_refuses_everything_else`;
    six settings keys ruled (the signed trio Denied); `updates.rs`'s 17 tests untouched.
  - [ ] T2 — transport, the launch ride, the 6 h poller, check commands, minimal card
  - [ ] T3 — the 12th platform trait `AppBundle` (facts, pre-flights, stage, swap, sweep)
  - [ ] T4 — apply end to end: refusals, hub download, swap, helper, exit through the gate
  - [ ] T5 — the DNS agent states its build; a stale one is kickstarted at launch
  - [ ] T6 — the full About card, the one-source consent sentence, copy guard, L2 probe
  - [ ] T7 — the tray item and the app-menu "Check for Updates…"
  - [ ] T8 — `rex status` / MCP read field, with no new dispatch arm
  - [ ] T9 — release flow: the tar.gz asset, the version guard, §A0, and the WRONG docs
  - [ ] T10 — the runtimes publisher + the tap's `auto_updates true` (other repos)
  - [ ] T11 — the first real in-app update on a real Mac (0.6.0 → 0.6.1)
  - [ ] T12 — archive the plan as a design record

- [ ] **~16 flag-taking `rex` commands still ignore what they do not recognise**
  (3 Sep 2026, ledger #463/#466). Done: `site create`, `wp search-replace`,
  `site delete`, `db reset`, `db import`.
  **Pick the rest by what an ignored flag DOES, not by how destructive the verb
  sounds** — that was the first ordering here and it was wrong. On `db reset` and
  `site delete` a typo fails SAFE: a misspelt `--yes` leaves the prompt standing.
  The danger is a flag that SUPPRESSES a question (`--yes`, `--dry-run`) or
  CHANGES what gets written or built. `wp search-replace` had both at once.
  Two patterns to copy, each bought by a failure:
  the accepted set is a FUNCTION or a table the test reads through a DIFFERENT
  syntactic form than the one the code declares it in — a scan over the same
  lines deletes its own evidence, and that plant came back green; and check the
  POSITIONALS too, since `search-replace old --dry-run new` wrote the flag
  itself into the database.

- [ ] **A public tunnel for `mstest.rex` was running that this session never started**
  (observed 27 Aug 2026, ~07:20). Two `rex tunnel list` calls ~15 min apart: the first said
  "no public tunnels running" after an explicit stop, the second showed `mstest.rex` on a
  URL nothing in this session had printed. Stopped it; `tunnels` table is empty and no
  cloudflared survives. No repro — kept open because a public share appearing unbidden is
  the one class of bug that must not be smoothed over.
  - [x] **The logging half — FIXED 30 Aug 2026, ledger #430.** Every share now writes an
    identifying INFO line to `rexenv.log` when it starts (site · public URL · pid · origin)
    and a closing line on every way it ends (user stop, in-flight kill, quit, crash).
    Plant-proven twice over: content (`a_share_line_names_the_site_the_url_and_the_pid`)
    and call sites (`every_start_and_every_stop_writes_a_line`; dropping the start, stop or
    quit line each fails it). Live leg — reading a real share's line back out of a real
    `rexenv.log` — is 🔨 and rides the tunnel example.
  - **The row's own premise was HALF WRONG, found while fixing it.** It said "`rexenv.log`
    has no tunnel line since 21 Aug"; it does — 27 Aug 21:26 local, `tunnels: killing the
    orphaned tunnel for mstest.rex (pid 78716) — a prior session crashed while sharing`.
    And `logs/tunnel-mstest.rex.log` holds the whole run: quick tunnel requested
    `2026-08-27T14:12:18Z`, URL `https://reflections-jets-ethernet-sewing.trycloudflare.com`,
    traffic to `/sub1/…` — i.e. the multisite-through-a-tunnel session's OWN share,
    surviving a crash. So the trail existed, in the two places you look last: a WARN at
    cleanup time, and a per-domain file you must already suspect. What was missing is the
    line at START, which is the one a reader finds without knowing anything — hence the fix
    above rather than the "no trail at all" the row asserted.
  - [ ] The diagnosis itself stays open: nothing yet explains a share running that no
    session started. With the start line in place, a recurrence is answerable from
    `rexenv.log` alone — which is what makes waiting for one reasonable instead of guessing.

- [ ] **The pinned wp-cli phar (2.12.0) is not PHP 8.5-clean.** #317 moves its
  deprecation off stdout; it does not make it go away, and a user running `wp` in
  rexenv's terminal (deliberately unpinned, #228) still sees it on every command. Worth
  re-checking when wp-cli ships a release that fixes `react/promise` — the pin bump is
  the real fix, this is the containment. **Re-checked 15 Aug 2026:** 2.12.0 is still
  the latest release (upstream issue wp-cli/wp-cli#6271 tracks this exact deprecation);
  `react/promise` 3.3.0 carries the fix and wp-cli's source already depends on it
  transitively via composer ^2.9.5, so the NEXT wp-cli release should clear it —
  nothing to bump yet.
  **Re-checked 24 Aug 2026:** still nothing. `wp-cli/wp-cli` latest release is v2.12.0,
  published 2025-05-07 — over fifteen months old, so this is not a release that is about
  to land. Keep the containment; re-check when a release appears, not on a schedule.
  **Re-checked 30 Aug 2026 — and this time MEASURED rather than read off a constraint.**
  Latest release is still v2.12.0 (2025-05-07), so there is still nothing to bump. What is
  new is that the fix is confirmed to EXIST, run against the real bundled PHP 8.5.8:
  - The pinned phar still raises it, and the containment still contains it. Bare:
    `Deprecated: Case statements followed by a semicolon (;) … react/promise/src/functions.php
    on line 369` lands on **stdout**, in front of `WP-CLI 2.12.0`. With rexenv's
    `-d display_errors=stderr` (#317): stdout is `WP-CLI 2.12.0` alone, the deprecation on
    stderr. That is the ledger #317 claim re-observed on 8.5.8, not re-asserted.
  - **wp-cli master is CLEAN.** The nightly phar (`3.0.0-alpha-6a3afd3`) under the same PHP
    prints its version with no deprecation at all; its vendored `react/promise` has zero
    `case …;` occurrences where the pinned phar's has one. Upstream issue wp-cli/wp-cli#6271
    is **closed (13 Mar 2026)**. So "the next release should clear it" is now a measurement
    of master, not an inference from `composer ^2.9.5` — the earlier re-checks read the
    constraint and believed it, which is the shape this project has been burned by
    (a guarantee read off a dependency's flag names).
  - **Not pinning the nightly**, and that is the point of the pin: it is a moving,
    checksum-less target, and 3.0.0-alpha is a major-version alpha. The bump waits for a
    release. **What must NOT happen when it lands** is deleting the `-d display_errors=stderr`
    flag because the deprecation went away — see the ledger #317 note.
  *(Not doing: forcing `display_errors=stderr` into the rexenv terminal too. That terminal
  is deliberately the user's own environment (#228) and the flag would have to arrive as an
  injected env var, which is a bigger promise broken than a deprecation line shown.)*
- [ ] **Private-window flags for Arc, ChatGPT Atlas, Orion.** Left `None` in the
  `BROWSERS` table because no one has run the flag on a real install, and a fork
  that swallows the flag it inherited opens an ordinary window under a control
  that said private. One-line each once tested; the rows simply show no private
  icon until then.
  **Checked 24 Aug 2026: none of the three is installed on this machine** (Chrome,
  Firefox, Brave and Safari are). So this is not "nobody got round to it" — it cannot be
  tested here at all, and the honest `None` stands until someone with one of those
  browsers runs the flag.
- [ ] **Windows/Linux: `detect_browsers`/`open_in_browser` are the default empty
  stubs** (Phase 4, same shape as `detect_editors`). Until they are filled, those
  platforms open every link in the OS handler and show no chevron — honest, but
  the Settings row will read "No browser detected". The private-window arm is
  part of that stub: `supports_private` is false everywhere, so those platforms
  show no private target rather than a dead one.

- [ ] **Eliminate the bug class: bundled PHP with curl's THREADED resolver** (the real
  fix for #251
  — **and as of 31 Aug 2026 the exposure is MEASURED rather than described, which is new**.
  Under the bundled 8.3/8.5: `gethostbyname("abc.rex")` → `127.0.0.1` and PHP streams fetch
  the page, while `curl_init("https://abc.rex/")` fails outright — *"Could not resolve host:
  abc.rex"*. Under 7.4 (ours, no c-ares) the same call is HTTP 200. So the row's
  "raw `curl_init()` and non-WordPress apps" is exactly right and now demonstrable in three
  lines of PHP. **Also measured: c-ares DOES read `/etc/hosts`** — `kubernetes.docker.internal`
  and `multi.local` both resolve through it under 8.3, only `.rex` fails, because ours is a
  wildcard resolver file and `/etc/hosts` has no wildcards. That prices a second option that
  was never on the list: a rexenv-managed `/etc/hosts` block would fix raw curl for KNOWN
  hostnames, at the cost of a root write per site lifecycle event, and it still could not
  cover subdomain multisite or any host a user invents — which is precisely why the resolver
  file was chosen over `/etc/hosts` in the first place. **The ruling stands; the ladder is now
  priced at every rung.**
  **Made LEGIBLE meanwhile** (ledger #435): `rex doctor` prints the limitation as a NOTE —
  which builds have it, how many sites sit on them, what is covered (WordPress, via the
  mu-plugin) and what is not, plus the workarounds a developer can use today (the WP HTTP
  API, or `CURLOPT_RESOLVE`). Deliberately not a ✗ or a ⚠ and deliberately not counted in the
  exit code: every normal install has this, and a doctor that goes red for everyone teaches
  people to ignore it. The alternative it replaces is an unexplained DNS error inside
  somebody's plugin with nothing on the machine willing to say why.
  **Live 31 Aug 2026**: `· PHP curl  PHP 8.3, 8.4 bundle curl with c-ares … (17 sites on
  them)`, finding count unchanged at 1. The first run printed nothing because the `rex` on
  PATH is the packaged 0.4.0 CLI, not the one built from this tree — doctor's own ⚠ CLI line
  had already said so. Worth remembering when a new doctor line "doesn't appear": check which
  `rex` answered before checking the code — the mu-plugin covers the WordPress HTTP API, not raw `curl_init()` in
  a plugin, and not non-WordPress PHP apps rexenv hosts). Needs a self-built
  static-php (`--enable-threaded-resolver` instead of `--enable-ares`) for 7 minors ×
  cli/fpm × 2 arches. **The hosting path is no longer blocked** — as of 14 Aug 2026
  `rexenv/runtimes` gates, signs and publishes this shape of artifact
  (`docs/archive/PLAN-php-74-support.md`).
  ⚠ **COSTED 25 Aug 2026, and the line above used to over-state it — "builds …
  exactly this shape" is true of 7.4's shape and misleading about 8.x.** Measured:
  `scripts/build-php74.sh` is 452 lines of which ~25 are 7.4-SPECIFIC reasoning
  (K&R definitions C23 removed, a source that is not php.net's because 7.4.33
  fails on OpenSSL 3.6, an extension set that drops swoole/event because modern
  releases dropped 7.4) — its complexity IS 7.4, so it is not a builder you point
  at 8.1–8.5. And `scripts/publish-manifest.sh` is built the OTHER way round:
  `[ "$minor" = "7.4" ] && continue`, because 8.x are DISCOVERED from
  static-php.dev. Self-building 8.x means a new build script, inverting the
  publish logic, and — the part missing from this row entirely — **24 artifacts
  per PATCH release, forever**, since rexenv ships in-app PHP patch updates
  (`docs/archive/PLAN-binary-updates.md`), so every upstream 8.3.32 → 8.3.33 becomes a
  self-build. **RULED 15 Aug 2026: not now**, and this measurement supports the
  ruling rather than weakening it. Rebuilding seven minors self-hosted is a maintenance burden carried
  forever, for a dependency nobody has complained about — a commitment, not a
  fix. The option stays recorded here with the runtimes-repo note so it is
  known when there is a reason.
  **Corrected 21 Aug 2026 — this row used to claim a guard that does not exist.** It
  said "`wp_dns_check` FAILS LOUDLY the day a build stops using c-ares — that is the
  signal this item is done". It does not fail: on a threaded-resolver build the example
  takes its `field("ares") == "-"` branch (`examples/wp_dns_check.rs:171-178`), prints
  `NOTE: this php's libcurl uses the THREADED resolver — the c-ares bug class is gone on
  this build`, asserts both requests return 200, and exits 0 green. So the signal is a
  NOTE in a log nobody reads on a run that passed, not a red gate — which is the
  difference between a control and a hope. Either make the branch loud (a check that
  fails once the whole pinned set is threaded, so the day it flips is a build failure
  that says "this row is done") or keep the note and stop calling it a guard. The row
  says it plainly meanwhile.
  - [x] **Decide which: a real gate, or an honest note. Not both.** ✓ 23 Aug 2026 —
    **a real gate**, ledger #381, plant-proven three ways and RUN live (all green).
    Not the gate the row imagined, because measuring first changed the question.
    **7.4 already uses the threaded resolver.** It is the one build rexenv makes itself,
    against its own curl 8.21.0, and it never received static-php.dev's `--enable-cares`.
    Measured 23 Aug by running all seven cached builds — which nothing had ever done,
    since `wp_dns_check` measures whichever PHP its fixture happens to run. So this row's
    own costing above ("7 minors × cli/fpm × 2 arches") counted a minor that never had the
    bug, and the expensive ruled-out fix has already been demonstrated, accidentally, on
    the only build we control. That is evidence about the option's cost; the ruling stands.
    So the gate cannot be "fail when the build stops using c-ares" — that fails on good
    news today. `core::wp_dns::resolver_for` records the measurement per minor, L0 refuses
    to let a pinned minor go unrecorded (and refuses a table that is uniformly `Ares`,
    which is the assumption this disproves), and `wp_dns_check` fails on any DISAGREEMENT
    between the record and the build in front of it — news in both directions.
- [ ] **Option, not a commitment: a self-built nginx (deployment target 12)
  would drop the app floor from 15 to 14** (MySQL's floor). Same
  `rexenv/runtimes` path that built PHP 7.4; recorded like the c-ares ruling -
  known, waiting for a reason (e.g. macOS-14 users actually asking).
- [ ] **PHP 7.4 — the five residuals of a shipped feature** (`docs/archive/PLAN-php-74-support.md`;
  the stage log is in `docs/archive/SHIPPED-2026-08.md`). Kept as open rows because they
  were living inside a ticked block, which is where open work goes to be forgotten.
  **Four of the five are now closed** (30 Aug 2026); what is left is the ONE with a date
  on it rather than a fix — GitHub's x86_64 runners ending August 2027.
  - [x] **`rexenv/runtimes`' release notes for `php-7.4.33-6` described `-4`** ✓ 30 Aug 2026
    — `runtimes` `c86129d` + `ba58c8e`, and the PUBLISHED build-6 page re-edited to match
    (the tag is immutable for ASSETS; the body is not, and the assets were untouched).
    Both stale lines were measured before they were rewritten, not just reworded:
    - **The floor.** Both shipped slices carry `LC_BUILD_VERSION minos 12.0` (`vtool
      -show-build`; arm64 from the local cache, x86_64 downloaded from the release), so
      the prose's `11.0` was a build behind. **The "asserted per artifact" half was TRUE,
      and this session briefly published that it was not** — the first pass grepped
      `.github/workflows/php-74.yml`, which is where the guard is NOT:
      `scripts/build-php74.sh` gate 4 has always failed the build unless every artifact's
      minos equals `MACOSX_DEPLOYMENT_TARGET` (set to 12.0). The gates live in the script
      the workflow calls — read that next time a claim about this build is checked.
    - **The extension set.** "Narrower … widening is in progress" is false as of build 6:
      57 modules against a static 8.3.32's 62, the difference **exactly** the five
      documented absences (`Zend OPcache`, `random`, `opentelemetry`, `protobuf`,
      `swoole`), with nothing else missing. The "60 modules" this row used to quote was a
      raw `php -m | wc -l` — section headers and blank lines included, which is the number
      a pipe gives you and not the number of extensions.
    - The notes now also state PCRE JIT is compiled out, which the artifacts have done
      since build 1 and the page had never mentioned.

  - [x] **PCRE JIT is compiled OUT of 7.4 — an ACCEPTED POSTURE, not an open task**
    ✓ 30 Aug 2026. 7.4 bundles PCRE2 10.35 (May 2020), too old for Apple Silicon JIT, so
    Composer died on `Allocation of JIT memory failed`; `--without-pcre-jit` removes the
    capability, and regex throughput on 7.4 is lower than on the 8.x rows because of it.
    Undoing it means building 7.4 against a NEWER EXTERNAL PCRE2 — a dependency change to
    an EOL runtime nobody runs for speed. The box was an unticked description of a
    decision already taken, which is how a settled trade-off reads as owed work.
    Recorded where both audiences look: `docs/PORTS.md`'s PHP row, and now the release
    page itself.

  - [x] **The upstream source commit is recorded nowhere in rexenv** ✓ 23 Aug 2026 —
    `PHP_7_4_33_SOURCE_COMMIT`, ledger #377, plant-proven three ways.
    **The row's premise was wrong and that is the useful part.** The commit was NOT
    recorded nowhere: `THIRD-PARTY-NOTICES.md` has carried it since 7.4 shipped. It was
    absent from the file a resolve reads and present in the file a licence auditor reads,
    so the risk PLAN §11 describes was mitigated for the wrong reader. *Recorded* and
    *recorded where the reader is* are different claims, and a row that conflates them
    reads as a bigger gap than it is — which is how it survived a reconcile whose whole
    job was checking premises.
    The second copy is deliberate and knowingly the one-fact-in-two-places family
    (`docs/TESTING.md` §3.1): the two files serve different readers, so they are held
    equal by test rather than deduplicated. `docs/PORTS.md` names where the fact lives
    instead of repeating the hash — a third copy would be a third thing to drift.
  - [ ] **The x86_64 half is on a clock: GitHub's x86_64 runners end August 2027.** PLAN
    §4.5/§11 says to land the cross-compile path before then, and notes that a cross-built
    artifact can never run a native smoke test. Nothing in this file mentioned 2027 until
    the 21 Aug reconcile.
  - [x] **Xdebug is silently unavailable on 7.4, and unlike 8.0 nothing blocks it**
    ✓ 23 Aug 2026 — ledger #378, plant-proven four ways.
    The row asked which it is for 7.4. It was already decided and MEASURED — `docs/PORTS.md`
    has said since 14 Aug that our own 7.4 build exports ~22,400 symbols and not
    `_OnUpdateBool`, the same wall 8.0 hits. So 7.4 is a genuine absence, and the shipped
    message happened to be true of it. **The defect was never 7.4; it was that the type
    could not tell the two apart**, so the sentence was right by luck and would go wrong
    for the first minor that ships before its bottle does — telling that user their PHP is
    broken when the gap is rexenv's, and offering "switch to 8.1 or newer" to someone on
    something newer than the table.
    `XdebugStatus` splits `Available` / `CannotLoadExtensions { measured }` / `NotPinned`,
    and the durable half is that **`NotPinned` is unreachable for a version in
    `PHP_VERSIONS`** — adding a minor fails the build until somebody decides, at the one
    moment the answer is known.
    **A second copy was found on the way:** `SiteDetail`'s `XdebugCard` hardcoded the same
    conflated sentence. That card had already had this exact lesson — it used to disable on
    a literal `minor === "8.0"`, which was moved into core as `xdebug_supported` — and the
    sentence beside the boolean was left behind. The reason now rides the registry row like
    the boolean does.
### Open work that was living inside ticked rows

- [ ] **Radicle-hosted repos are unverified** — same code path as the Bedrock clone that
  was verified and found broken, no live project to hand.
- [ ] **Why that `rex` instance went deaf was never diagnosed** — the evidence died with
  the pid. Reproduce before blaming App Translocation.

## Ledger-driven proof backlog

The test metric is `docs/CLAIM-LEDGER.md`. **Do not copy the tally here** — this line
carried 29 Jul's numbers (123/38/37/5 of 203) until 13 Aug, when the ledger's own
generated line read 213/43/40/5 of 301: a second copy of a number that is generated
in one place drifts, and a stale one reads as progress that did not happen. Run
`scripts/ledger-tally.sh`. The backlog = every 🔨
row + the noted half of every ◐ row, worked by the ledger's blast-radius tiers, top
first:

- [ ] **`wp_plugins_check` failed its deactivate assertion once and has not reproduced —
  the product-bug flag raised 14 Aug 2026 is RETRACTED, mechanism refuted.** The suspicion
  was that `plugin_verb` returns WP-CLI's stdout as a String and so reports success from
  output rather than status. Measured instead of assumed, and it is wrong twice over:
  `wp_run`/`wp_run_timed` both test `status.success()` and turn a non-zero exit into an
  `Error`, and the instrumented run showed deactivate doing exactly what it claims —
  stdout `"Plugin 'hello-dolly' deactivated. Success: Deactivated 1 of 1 plugins."`, with
  the very next raw `wp plugin list` reporting `hello-dolly,inactive`.
  Nor is the shape elsewhere: `item_verb` is shared by plugin activate/deactivate/update
  and theme update/delete and every one goes through the checked path; the unchecked
  `wp_cli` is used only for boolean probes (`core is-installed`, `plugin is-active`,
  `config get MULTISITE`, `maintenance-mode is-active`, `language core is-installed`,
  `verify-checksums`) where a non-zero exit IS the answer. **There is no "we ignore exit
  status across the wp surface" problem to scope.**
  What remains is one unexplained failure. It happened in the isolated re-run immediately
  after the corpse-mysqld bulk run; it has passed 3× since the leftover docroots were
  removed. The tempting story — leftover plugin state — does NOT fit: in the bulk run this
  example died at `install_for_site` with errno 2, before any plugin work. **So the cause
  is unknown, and this is recorded as an unexplained assertion failure rather than a
  flake, because calling it a flake is a story too.** Worth catching if it recurs.
  One genuine measurement kept: `wp plugin deactivate` on an ALREADY-inactive plugin exits
  0 with `Success: Plugin already deactivated.` on stdout and `Warning: Plugin 'x' isn't
  active.` on stderr — so exit-zero-with-a-warning is real on this surface, it just isn't
  what bit here.
  **24 Aug 2026 — three more clean runs, back to back, stack stopped.** All three exited 0
  with the deactivate assertion passing (`bulk-deactivate → Inactive`, then delete). That is
  six consecutive passes since the leftover docroots were removed, and it moves nothing:
  the row is about ONE unexplained failure, and clean runs cannot explain it — they only
  narrow how often it happens. The capture instrumentation is what would settle it, and it
  has still never fired.
  **15 Aug 2026 — a recurrence now captures itself.** The original sighting produced no
  evidence because the assert printed only "not deactivated" and the panic then leaked
  mysqld into the next run. The example now dumps deactivate's own stdout, the parsed
  list, and a raw `wp plugin list` re-read (the raw read separates "parsed list stale"
  from "really still active") before panicking, and mysqld is Drop-owned
  (`common::OwnedService`) so the panic cannot manufacture the corpse-mysqld condition
  the first sighting was tangled with. Nothing new was ruled in or out — still filed
  as unexplained.
  ✓ **26 Aug 2026 — the capture is PLANT-PROVEN, which it had never been.** It exists to
  catch a rare unexplained failure and had never fired, so whether it WORKED was itself
  unknown — and that is not a hypothetical worry: the original 14 Aug sighting produced no
  evidence precisely because the instrumentation was not there yet. Forced by inverting the
  assertion so it fires on a SUCCESSFUL deactivate, it printed all three things it
  promises — deactivate's own stdout (`Success: Deactivated 1 of 1 plugins.`), the parsed
  list, and the raw `wp plugin list` re-read with its exit code, stdout and stderr — before
  panicking.
  **The same run proved the second half**: the panic unwound and reaped everything (no
  stack port left answering, no service process, no docroot in the user's real Sites
  folder), so a recurrence can no longer manufacture the corpse-mysqld condition the
  original sighting was tangled with.
  **Still unexplained, and unchanged by this.** A working instrument is not an explanation;
  it only means the next occurrence will leave evidence. The row stays open for that.
- [ ] **Bedrock live provision — the committed example (#35), deliberately not built.**
  Split out of the compound row below on 30 Aug 2026, because that row's every other item
  is struck through and its box could never close while this sat inside it. The PREMISE is
  proven live (24 Aug 2026: a real Bedrock WordPress, two planted mu-plugins, only the
  recorded content dir's one loaded; ledger #35 carries the method). What is open is a
  COMMITTED example, and the reason it is open is a cost, not an oversight: it would
  download core, create a database and install WordPress on every network-tier run.

## Release gates (human, scripted — see the docs named)

**This section is HALF the set.** The rows below are `docs/PUBLISH-TESTING.md`'s
outstanding gates; the other half is `docs/SMOKE-TEST.md`, run end-to-end on a clean
Mac from the distributed dmg, and it grew steps that are visible from nowhere here
(cold-path 7.4 licences, the PHP update button + its revert, the Adminer update, the
PHP ini revert, the "exists" row and the serving-vs-pinned line, the blank-PHP starter
database, and the MCP HOLDs). **A release-day reader works both files, in this order:
SMOKE-TEST on the built dmg, then PUBLISH-TESTING §A0/§A before publishing.** The rule
this note exists for: silence in a gate list reads as "this is the set", and a gate
nobody can see from the list is indistinguishable from a gate nobody ran.

- [ ] **PUBLISH-TESTING §B** — uninstall removes the root :443 daemon (live launchd).
- [ ] **PUBLISH-TESTING §D** — `--zap` ONLY; everything else has now run four times.
  **Re-scoped 21 Aug 2026**: the row below pins the v0.1.0 cask hash, but the cask has
  bumped cleanly through 0.1.1, 0.2.0, 0.3.0 and 0.4.0 since, so the install half is not "half
  done from August 12" — it is the routine path and `--zap` is the single step that has
  never run anywhere. Original text, still accurate about what was proven:
  full tap install dry-run. **Half done 12 Aug 2026**:
  v0.1.0 is published on `rexenv/homebrew-tap` (private repos 404 `brew`'s anonymous
  fetch, so the artefact ships from the tap — `docs/RELEASING.md`, interim section),
  the cask is bumped to the shipped `b29f21f7…`, the asset fetches anonymously (200),
  and `brew fetch --cask rexenv` verifies ✔︎. **Install half ✅ ran the same day**:
  installs, postflight de-quarantines (`No such xattr`), `rex` links to
  `/opt/homebrew/bin/rex`, app launches, `rex --version`/`status` answer, DNS agent
  plist repointed at `/Applications`. **Left: `--zap` only**, and NOT on this Mac —
  it trashes 17 GB of live app data behind real `.rex` sites. Clean Mac only
  (`docs/SMOKE-TEST.md`).
  Two teeth grown from the first real run: the cask hash bump now compares sha256
  as well as version (a placeholder hash under an unchanged version silently
  skipped), and `brew trust rexenv/tap` is a required user-facing install step.
- [x] **Cask: `postflight` is deprecated in favour of `postflight_steps`** ✓ 5 Sep 2026 —
  `rexenv/homebrew-tap` `2a5d489` (also drops the deprecated `verified:`): the same xattr call as a declarative `run` step with
  `{{appdir}}` as the install-time token; `brew style` 0 offenses (was 1), the cask loads
  and `brew info --json=v2` serialises the step. **Proven through the real runner
  6 Sep 2026** by `brew reinstall --cask --debug rexenv` on this Mac: brew propagated
  quarantine from the cached dmg onto the staged app (`xattr -w com.apple.quarantine
  0381;6a9c1b6e;;…`), copied the xattrs to `/Applications/rexenv.app`, then ran
  `Installing artifact of class Cask::Artifact::PostflightSteps` — and the installed
  app came out with **zero** `com.apple.quarantine` attributes (only
  `com.apple.provenance`), `codesign --verify --deep --strict` still passing. The
  plain (non-debug) run prints none of that, which is why the debug log is the
  evidence: an install that silently skipped the step looks identical.
- [ ] **Flip the release host back when `rexenv/rexenv` goes public** — two things
  in ONE commit, or the tap's guard fails the bump: the cask's `url` and `SOURCE_REPO`
  in `update-cask.yml` (both in `rexenv/homebrew-tap`). It was three until 5 Sep 2026;
  the cask's `verified:` was the third, dropped when brew 6.0.22 deprecated it. Then CI's
  `release.yml` resumes owning the build, and `docs/RELEASING.md`'s interim section
  is deleted rather than left as a second, wrong set of instructions.
- [ ] **The self-update swap probe (T0) and the first real in-app update (T11)** —
  `docs/PLAN-self-update.md` §6.5 and §13. Both need a human at a real Mac: T0
  measures whether an ad-hoc bundle may rename itself under App Management (nothing
  documents it, and the plan branches on the answer), T11 is the 0.6.0 → 0.6.1
  update run on this Mac and on a clean account. Neither can be run by any tier.
- [ ] **PUBLISH-TESTING §K** — the whole migration as ONE journey (rebuild first).
- [ ] **PUBLISH-TESTING §F** — resolver takeover/hand-back/drift: clean-VM only.
- [ ] **PUBLISH-TESTING §G** — `/import` screen packaged GUI pass (only ever
  type-checked; Stage 1's status line now says so). **Grew step 9 on 8 Aug**: the
  batch progress card (`valet-import://progress`) — its arithmetic is L0-proven, but
  that `detail` really is the child job's own label and that the bar FREEZES rather
  than rolls back on a mid-batch failure are 🔨 L2 (ledger #243) and only this pass
  covers them today.
- [ ] **Release 5.4 — clean-Mac smoke test** (`docs/SMOKE-TEST.md`): first pass
  10 Jul 2026 green except multisite-convert (UI didn't exist yet — since built);
  re-verify converted-multisite + onboarding fixes + the TLD v1 Done-when list
  (`docs/archive/TLD-FEATURE-REPORT.md`) on the next cold run.
- [ ] **Tunnel probe session** (one sitting, real network,
  `scripts/tunnel-measure.sh`): kill -9 death-path timings, wifi-blip recovery,
  the banner→authoritative-DNS gap; plus Bedrock "Log in as" landing in wp-admin
  live, and the first real Radicle-layout link (flagged UNVERIFIED in code, #95).
- [ ] **Intel spot-run**: x86_64 bottle digests + MySQL 8.0.44 x86_64 were hashed
  from real downloads but never RUN (PORTS.md caveat) — run-verify on the next
  Intel machine.
- [ ] ⚠ **The macOS floor is a claim about BOTH slices, and most of it has still never
  been measured** (narrowed 30 Aug 2026 — it used to say "half", and one row of the table
  is now genuinely both-slice). PORTS.md's `minos` table is measured from this machine's
  binary cache, which only ever downloads the host arch, so every number in it is an
  **arm64** number **except PostgreSQL**, and `minimumSystemVersion: 15.0` is asserted for
  x86_64 on the assumption that upstream builds both slices to the same deployment target.
  Nothing checks that.
  **The one measured exception is also the evidence that the assumption is worth checking.**
  The 30 Aug PostgreSQL re-pin fetched the x86_64 tarballs and swept every Mach-O in them:
  the new pins are 15.0 on both slices, and the OLD 18.4.0 was 26.0 on both. The two slices
  did move together, twice — which is a data point FOR the assumption and not a substitute
  for checking it, since the failure mode is precisely a pin where they do not. It also
  proves step 1 below is cheap: that was one `curl` and one `vtool` per artifact, done from
  this arm64 Mac. **This is the arm64-DMG mistake's shape**: a
  universal artifact whose two halves differ, working perfectly on the machine
  that made it and wrong for everyone on the other chip — except the failure
  here is worse than a thin binary, because it is invisible until an Intel user
  on macOS 15 finds their web server will not start. (The 15 Aug re-measure also
  showed the table can simply be WRONG where nothing depends on it: PHP 8.0.30
  is 14.0 and had been recorded as 12.0 since the table was written.)
  **What it would take, cheapest first:**
  1. [x] **No Intel Mac needed for the measurement** ✓ 30 Aug 2026 — `minos` is metadata,
     so the x86_64 artifact is fetched and read here. Landed as its own example rather than
     a column on `manifest_sweep_check`: the sweep enumerates ~70 targets including the
     multi-hundred-MB trees, and this needs the small default-stack set with an assertion
     attached, so bolting it on would have made one check answer two questions and fail for
     either reason.
  2. [x] **Assert the DERIVED rule** ✓ 30 Aug 2026 — `macos_floor_check`, ledger #433:
     `max(minos)` per arch must equal `tauri.conf.json`'s `minimumSystemVersion`, in both
     directions. **It failed on its first run, on a real defect** — see the 🔴 nginx row
     above. The row asked for a check that fires on a pin bump that raises a floor; the
     first thing it found was a floor that had already been raised, on the slice nobody
     could see.
  3. [ ] Only now does an actual Intel machine matter, and for the OTHER half — the
     run-verify, which metadata cannot stand in for. It has a sharper question to answer
     than before: whether a `minos 26.0` nginx actually refuses to load on an Intel Mac
     running macOS 15, which is the same unsettled dyld-enforcement question PostgreSQL
     raised, now aimed at a binary that is not optional.
- [ ] **In-app verifies owed** (CLI passthroughs whose service-touching half the
  example harness guard-blocks; fold into the next deep test): `php
  install/uninstall`, `php settings set`, `db versions --set`, `site
  server/domain/move`, `mail clear`, `tunnel start`, `wp core update/switch`,
  `site create --starter-db` (**RUN 3 Sep 2026** — the seed is real: table
  `starter_items`, a `db.php`, the page renders "connected" and one row) —
  plus the packaged-GUI walks still noted inside their shipped entries: MariaDB
  site from the dialog, Apache site in-app, DB version switch from the Databases
  row, Settings CLI-install card, ref-picker on a real many-branch repo, wp.org
  chips + streamed installs, New Site streamed provisioning card, **Upload zip
  through the real native file dialog** (SMOKE §WordPress Manager — L0 proves
  the gate, L1 the install, L2 the card; nothing can drive the picker).
- [ ] **PUBLISH-TESTING §E / §L** — 🟢 nice-to-haves (B22/B23 datadir recovery,
  B4 submodule clone, B24 wp-cli `--`, B20/B28/B29/B7 runtime wiring; §L Phase-A
  tcpdump one-off, re-run per reqwest bump).

## Parked (deliberate — needs explicit go; don't pick up silently)

- [ ] **The live pool swap is still L3.** `php_update_check` proves the chain up
  to "a pool on the new patch answers on a FIXTURE port". Stopping the running
  master on the PRODUCTION port and reverting when it does not come back needs
  the real `ServiceManager` and a deliberately broken tree — `docs/SMOKE-TEST.md`.
- [ ] **SMOKE §M1/§M2a/§M2b — the MCP human gates, PARTLY RUN 25 Aug 2026.** Run over
  the raw socket by a hand-rolled client (protocol + server exercised; `rex mcp` pipe
  proven once by the owner through Claude Code). **Still unrun:** step 1 (a launch that
  has NEVER been enabled), 9 (Keep), 11 ⚠HOLD (no admin prompt — observed, not checked),
  13's eyes-only consent copy, 16 and 17 (§M3, eyes-only), and one arm of 21 (deleting a
  REAL granted site). Step 8's model-facing half ("the refusal survives a model that
  wants to help") has never had a model in front of it. Evidence for what DID run:
  `docs/archive/SHIPPED-2026-09.md`.

- [ ] **Install WordPress into an empty LINKED folder** — out of Stage 0 by
  design (`docs/archive/PLAN-linked-sites.md` decision 2): linking is adopt-only. If
  built: a deliberate site-page action, offered only when the linked folder is
  EMPTY, its own disclosure. Trap first: `configure` is only half idempotent
  (skips `wp config create` when wp-config.php exists but calls
  `create_database` unconditionally), and `phase_defs` blanket-skips WP phases
  on `docroot_managed == Some(false)` — needs an explicit opt-in flag.
- [ ] **Valet compatibility tails** (recorded in the migration research, §2).
  - [x] Laravel's `/storage/*` URI mapping ✓ 2 Sep 2026, ledger #448 — emitted for a
    Laravel site whose `storage/app/public` exists, with php AND dotfiles refused
    INSIDE the block (the `^~` prefix that makes the mapping work also beats the
    vhost's own guards).
  - [x] The `default` catch-all-site key ✓ 2 Sep 2026, ledger #448 — REPORTED by the
    scan, not imported: rexenv has no catch-all, and a behaviour that silently stops
    after a migration is the shape nobody can connect back to the move.
  - [x] A Valet-named port-conflict attribution branch ✓ 2 Sep 2026, ledger #449 —
    positive ID via the include Valet appends to the Homebrew nginx.conf (never "Valet
    is installed"), and the offered fix is `valet stop` rather than
    `brew services stop nginx`, which leaves their Valet half-stopped.
  - [ ] Serving one site under two domains. **Foundation landed 2 Sep 2026** (ledger
    #450): schema v42 `site_domains` (aliases only — the primary stays on the site
    row), `core::sites::{all_domains, validate_alias, add_alias, remove_alias}` with
    the cross-table refusal SQL cannot express. **Serving half + CLI landed 2 Sep 2026**
    (ledger #451): one nginx server block per site whatever it answers on, extra
    addresses on the site's own Caddy block, one certificate covering every name (the
    cache reissues when the name SET changes), and `rex site domains <domain>
    [--add N | --remove N]`. **Import mapping landed 2 Sep 2026** (ledger #452):
    a link farm folds into one site with extra domains, on the served folder, with
    every fold announced. **UI landed 2 Sep 2026** (ledger #453): a Domains card on
    the site's Settings tab, rendering the backend's list after every mutation, with an
    L2 probe. **Still owed**: the live leg — a real link farm imported with both names
    answering, and an added domain actually served over HTTPS.
- [ ] **`rex` design-first set — ONE item left: raw `wp` passthrough** (a security
  ruling about what the CLI may execute, deliberately not gap-filled). The other four —
  single-site restart (#444), web-tier restart (#445), `wp_user_delete` (#446), progress
  streaming (#447) — shipped 2 Sep 2026; live legs ride the in-app verify list in
  `docs/CLI-ROADMAP.md`. Original row text: `docs/archive/SHIPPED-2026-09.md`.
## Menu-bar app

Shipped 31 Aug – 1 Sep 2026 (Phases A–D, ledger #436–#441; log in
`docs/archive/SHIPPED-2026-09.md`, design record `docs/archive/PLAN-menubar-tray.md`).
Nothing open. The second-instance row this section last carried closed with #441
(`hand_off_to_running_instance`: the CLI socket is the lock; a second launch activates
the first and exits) — its box stayed `[ ]` under a struck-through title, ticked 5 Sep 2026.

## Blocked on external work

- [ ] **Xdebug on PHP 8.0** — the Nov 2024 static-php 8.0.30 build exports zero
  Zend symbols (`nm -gU` = 0; dlopen fails `_OnUpdateBool`), upstream still
  serves that exact build (re-verified 16 Jul 2026). 8.1–8.5 solved via the
  bottle path. Fix needs an upstream rebuild or the self-build recipe
  (`docs/xdebug-debug-build.md`). PHP 8.0 is EOL — acceptable to leave excluded.
- [ ] **SMAppService privileged helper** (true single-prompt setup) — needs a
  signed + notarized bundle; packaging-era, after Developer ID signing.
- [ ] **Developer ID signing + notarization** — needs a paid Apple account;
  runbook ready in `docs/SIGNING.md`.
- [ ] **OpenLiteSpeed override server** — no macOS artifact exists anywhere
  (upstream ships Linux tarballs only; no homebrew-core formula; the one
  community tap is frozen at EOL 1.4.51 and fails the trust model). A maintainer
  self-build is plausible but needs the same hosting infra as the Xdebug build.
  Code is ready and honest: `ensure_server_available` refuses OLS in CORE at
  create AND switch, and `OverrideKind` means enabling it later is one new arm.
  On Linux (Phase 4) this is cheap — official tarballs exist.

## Phase 4+ (next era)

- [ ] Windows platform impls — fill the `todo!()` stubs in
  `platform/windows/mod.rs` (trait-by-trait; no restructuring required).
- [ ] Linux platform impls — same, `platform/linux/mod.rs`.
- [ ] Public distribution (the open-sourcing half of the old "packaging polish" row).
  **The updater half moved out of Phase 4+ on 6 Sep 2026** — it is the "In-app
  self-update" row under *Now*, planned in `docs/PLAN-self-update.md`, and it does
  NOT use the Tauri updater the archived §6.1 checklist proposed: that plugin's
  macOS install deletes its own backup and can run a root `rm -rf` outside
  `PrivilegeManager`, and its relaunch bypasses this app's ONE quit gate.

## Known baselines (not bugs)

- WP builds shipping `wp-includes/php-ai-client/**` show those files as "foreign"
  in checksum verify until wordpress.org's manifest covers them. Honest tool
  output — intentionally not filtered.
- On networks that negative-cache DNS, a fresh tunnel URL can be dead on THIS
  machine while live from a second device — the router race, not a bug
  (`docs/SMOKE-TEST.md` tunnels section).
