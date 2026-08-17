# TESTING — proofs at the right level, by design

The standing testing doctrine (written 28 Jul 2026 as a plan; its T1–T13 all
shipped the same day — evidence in `docs/archive/SHIPPED-2026-07.md` — so this now
reads as the reference for how this repo tests). Companion ledger:
`docs/CLAIM-LEDGER.md` (the full claim inventory §2 summarizes — and the project's
test metric; its mechanical tally is the current number, not any count quoted here).

## 0. Why this plan exists

The bugs that cost the most this month were not catchable by unit tests, and more unit
tests would not have caught them:

| Bug | What it needed |
|---|---|
| wp-config password truncated at `)` | real PHP parse semantics |
| mysqldump rejecting `--connect-timeout` in every group | the real binary |
| MariaDB clients unable to auth to MySQL 8 | a real server handshake |
| `h-full` collapsing against an auto-height wrapper | a real WebKit render |
| nginx's pid file emptied by an example | the real process on the real prefix |
| unlink-delete guard defeated on Bedrock | the real layout on disk |
| tunnel DNS negative-caching | a real network with a real resolver |

Every one was found by a human looking, or by an example running against something real.

So the goal is NOT "unit test everything." It is: **every claim we make has a
programmatic proof at the right level, and the levels are a deliberate design.** The
metric is claims proven (the ledger), never lines executed. And a test that asserts the
wrong thing gets deleted, not kept for the count — we already found one whose assertion
was itself wrong (§1.2 below).

## 1. The layer model

**No test counts are written down here.** Three of them drifted in a single day
(14 Aug 2026: L0 757 vs 784 actual, L1 116 vs 121, §4's 105) and each still read as
current. A number nobody can verify at a glance is worse than no number, so the counts
now come from the things that produce them: `cargo test --lib` prints L0's, and
`scripts/live-checks.sh list` prints L1's per tier. The fact that actually matters —
**every example is classified** — is enforced in both directions by that script (an
unlisted example and a tier entry with no example each fail the run), which is why it
can be stated here without a guard of its own.

Four layers exist de facto. Named, with what each CANNOT prove stated as loudly as what
it can:

### L0 — Logic (`cargo test --lib`)

- **Proves:** our pure functions, state machines, validators/refusals, config
  *generation* (regression-pinning bytes WE emit), SQLite migrations (rusqlite is real
  in-process), witness types ("this state is unrepresentable"), parser behavior against
  captured fixtures.
- **Cannot prove:** ANY fact about an external tool, server, network, or render. A flag
  list can't prove mysqldump accepts the flags. A plist string can't prove launchd loads
  it. A config substring can't prove nginx 404s a dotfile. An output fixture can't prove
  the tool still prints that line.
- **A special case worth naming: the source scan.** A handful of L0 tests read the
  TREE rather than a value — `every_wp_cli_argv_in_the_tree_is_pinned` (#228),
  `every_owned_mu_plugin_is_swept_by_the_cleanup_that_claims_them_all`,
  `only_one_place_in_this_module_builds_a_dist_archive_argv` (#230). They exist for
  claims about a WHOLE SURFACE ("every wp-cli spawn is pinned"), where the honest
  guard is not a list of the sites known today: #228's ledger row named four spawn
  sites and there were seven by the time it was worked, two added after the row was
  written. A scan turns "someone remembers to add it to the list" into a build
  failure. Two rules learned by breaking both: **strip comments before scanning** —
  a guard that reads prose reads its own explanation and passes (#235 shipped that
  way; #228's scan did it too and failed at 3 by counting its own doc comments) —
  and **carry a canary** that the detection still finds something, or the whole
  scan passes vacuously the day a path changes.
- **`common::sandbox` covers PATHS and says nothing about PORTS.** Every service- and
  network-tier example inherits that gap, and it fails in the direction that looks like
  success: with the user's stack up, an example that brings up its own services binds —
  or worse, TALKS TO — theirs. Found twice. `mcp_mail_check` would have planted test
  mail in a real inbox and proven the mail filter against the user's own messages;
  `tunnel_exposure_check` would have created its fixture database inside the user's
  RUNNING MySQL, because `install_for_site` just uses the port. Same accident, bigger
  blast radius, and both would have read as the example working. The refusal therefore
  lives in the harness (`common::require_ports_free`), not in each example — a correct
  implementation with a warning beside it has already failed to stop a repeat in this
  repo, so the next example inherits the guard rather than the lesson.
- **A probe needs a known-good case, for the reason an example needs a control leg.**
  Same principle, one level up: without a case whose answer you already know, a check
  cannot tell ITS OWN defect from the subject's, and it reports the app as broken.
  Twice in one session, both caught the same way. `wpverdict.js` matched the Update
  control by button TEXT, but the control is icon-only with its version in the
  `title` — it found nothing and read every row as "no update offered", failing the
  working rows; the known-good `wordpress-seo` row is what exposed it. The onboarding
  probe read `edge=` from the URL hash when the harness puts params in the search, so
  it failed both rendering cases; `onboarding-clear` passing while the others failed
  is what pointed at the probe. In both, the check that would have "found a bug" had
  the bug. So: every probe carries at least one case it should pass, and a probe that
  only ever asserts absence is the shape to distrust — absence is also what a broken
  selector returns.
- **A third party's error taxonomy is measured, never read.** `edge_wire_check`
  exists because `reqwest::Error::is_connect()` looked like the way to tell "nothing
  is listening" from "something answered wrongly", and is not: a TLS handshake against
  a plaintext server reports as a connect error, so the first mapping filed a real
  blocker as an empty port. Three real sockets settled it in one run. The rule this
  generalises: when a decision rests on how someone else's library classifies a
  failure, the classification is a FACT ABOUT THEIR CODE and belongs at L1 — reading
  the docs and believing them is how a posture ends up resting on a third party.
- **A pure function with no test runner is held where it RENDERS.** The repo has no
  JS test runner, and adding one for two assertions costs more than it buys — so
  #250's update-claim rule (`a row may claim an update only when the offer is newer`)
  is proven by `wk-checks/wpverdict.js` over fixture rows in the real component. The
  discipline that makes it a proof and not a screenshot: assert BOTH directions (the
  claim that must not render AND the real update a naive string compare would hide),
  prove the fixture rows are PRESENT first (absence looks identical to a working
  filter), and keep an ordinary row in the list so the two cannot pass on a panel
  where nothing ever offers anything.
- **A copy guard is an L0 that expires at the boundary of what it can see.** A
  must-say list proves the sentence is still in the source; it cannot prove what the
  source PRODUCES. #301 is the worked example: L0 asserts the card branches on whether
  the packages could be named, and only the L2 render (`wppackages-unnamed`) catches
  the branch producing `The 0 packages in ~/.wp-cli/packages — —`. Pair them whenever
  a copy variant exists that no developer's machine will ever render by accident.
- **Cost:** seconds. **Runs:** every `verify.sh`. **Bug class:** logic regressions,
  drift in our own generation/parsing, unrepresentable-state violations,
  whole-surface claims going stale as the surface grows.

### L1 — Tool (`src-tauri/examples/*.rs`, every one tiered in `scripts/live-checks.sh`)

- **Proves:** what real binaries accept and do — real mysqld/mariadbd handshakes, real
  nginx reloads, real php parsing our generated files, real wp-cli installs, real
  process lifecycle (adopt/orphan/reap), real filesystem layouts, **what a bundled
  binary was COMPILED with** (`wp_dns_check`, sandbox tier: the bundled PHP's libcurl
  uses c-ares, so it cannot see `/etc/resolver` — a fact no Rust test can hold, and the
  reason WP-Cron was silently dead on every site until 10 Aug 2026; the check asserts
  the bug still reproduces AND that the mu-plugin fixes it, so it also goes red the day
  a threaded-resolver rebuild makes the whole class obsolete). **This layer caught
  the most bugs this month.** Discipline: `common::sandbox` + `common::Reaped` +
  fixture ports (`examples/common/mod.rs` — read its invariant first).
  **`relink_tree_check` (sandbox tier, 14 Aug 2026) is this layer aimed at the tool
  chain itself.** `prepare_binary_tree`'s bug was never arithmetic — it was WHAT the
  chain was asked to look at: `needs_tree_relink` treated any `@loader_path/` prefix
  as in-tree, so a bottle whose deps read `@loader_path/../../../../opt/<f>/lib/…`
  passed both the rewrite and the VERIFY loop, the provider reported success over 49
  Mach-Os, and dyld then refused the binary. No pure test can catch that, because the
  claim is about `otool`/`install_name_tool`/`codesign` on a real Mach-O. The fixture
  is the example's OWN binary with one `/usr/lib` dep rewritten, so it needs no
  download and no service; leg C is the control (an already-in-tree path must be left
  ALONE, or leg A would pass for the boring reason that the predicate rejects
  everything), and the plant — restoring the old prefix test — fails leg A by name.
  **`php_fpm_serve` takes a VERSION argument** (`cargo run --example php_fpm_serve
  7.4.33`) for the same reason: the pool config generator emits one file per
  minor, so "php-fpm accepts it" is a claim about each minor's own binary. Reading
  `PHP_VERSION` could only ever ask about the newest, which is the version least
  likely to break — 7.4's php-fpm is a different program (ledger #327).
  A live check does not have to be networked to earn its layer: `git_site_clone_check`
  (sandbox tier) builds its own fixture repositories with real `git init`/`commit` and
  clones them locally, so it needs no remote, no credentials and no service — and it
  still caught an ordering bug no unit test could have (provisioning overwrites
  `NewSite.path` with the resolved docroot before the clone/link rule ran, so every
  cloned site read as also-linked and was refused). **Building the fixture with the
  real tool is what makes a hermetic example an L1 rather than an L0 in disguise.**
  **`wp_packages_check` (sandbox tier) is the pattern taken one step further: it plants
  the thing the check is about.** The obvious version — ask for a package command,
  expect it missing — is green on every machine that never installed one, which is most
  of them. So it writes its own canary package, REQUIRES that WP-CLI resolves it
  unpinned, and reports **CONTROL LEG BROKEN, this run proves NOTHING** if it does not,
  rather than letting a command that was absent for its own reasons read as the pin
  working. A control that fails silently converts a check into "nothing happened either
  way" — the vacuous-pass shape, one layer up from where this project usually catches
  it. Its verdict is also scoped in the ledger to what the run showed (one hook style,
  one machine), not to the claim's ambition.
  **`wp_noise_check` (sandbox tier, 14 Aug 2026) is the same pattern aimed at somebody
  else's code.** The claim (#316) is about ORDER inside the real phar — that rexenv's
  `--require` shutdown function runs before a plugin's — which no lib test can reach: L0
  proves the cut and the argv, and would keep proving them the day wp-cli changed when
  required files load. So the fixture reproduces the MECHANISM rather than the plugin: a
  require file that registers a `before_wp_load` command printing JSON and a shutdown
  function that writes to stdout with `fwrite`, which is the exact call Elementor's real
  trace ends in. Leg A then requires that this still produces an UNPARSEABLE answer
  without the marker — if the disease stops reproducing, the check says CONTROL BROKEN
  and stops, because a fixture that no longer bites makes every later leg vacuous.
  Its leg D (#317, the other end of the same report) shows what a control is FOR when
  the environment can answer for you: the claim is that PHP's diagnostics go to stderr,
  and on a machine whose `php.ini` already redirects them every assertion would pass
  with the flag deleted. So D re-runs the production argv with the flag removed **as a
  pair** (`-d` and its value — dropping the value alone would make PHP read the phar
  path as an ini setting) and requires the diagnostic to appear on stdout before
  believing the flagged run. The canary is `trigger_error`, deliberately not the phar's
  real 8.5 deprecation: a check that depends on a bug in someone else's release goes
  green the day they fix it.
  **`wp_login_client_ip_check` (sandbox tier, 14 Aug 2026) adds the third variant: a
  matrix that defends ITSELF, so the check does not depend on anyone re-running the
  plants.** It extracts the shipped client-IP gate from `wp_login::MU_PLUGIN` between
  markers (the `adminer_login_gate_check` arrangement, so it cannot drift from what
  ships) and runs it through real PHP over 13 header shapes — the tunnel ones verbatim
  from the live measurement in CLAIM-LEDGER #307, not invented. The problem with a
  table of cases is that a WRONG implementation can satisfy it: reading the first hop,
  reading nothing, or hard-coding a verdict. Plants catch those on the day you run them
  and never again. So the properties are asserted about the MATRIX, before PHP starts —
  an allow-case and a deny-case must share a leftmost entry (a first-hop read then
  cannot produce the table) and must share a `REMOTE_ADDR` (an XFF-ignoring read
  cannot either), with a floor on both verdict counts. **The plants were still run
  — three of them — but the table no longer rests on that having happened.**
  **`mcp_mail_check` (service tier) adds the other half of that discipline: refusing to
  run when the environment could answer for you.** It brings its own Mailpit, because
  borrowing the user's would mean planting test mail in their real store to prove rexenv
  can tell their mail from a scratch site's. The hazard is that borrowing happens by
  ACCIDENT: with the stack up, `mail::start` cannot bind and exits, `mail::running()`
  then sees the user's catcher on the fixed port, and every leg proceeds against a real
  inbox. So it proves the ports are free, its child is alive, and the store is empty
  before it writes anything — and it never infers from absence, since an empty list is
  also what a correctly filtered empty inbox looks like.
  Its network-tier sibling `git_site_provision_check` is the other half, and the one
  that pays: it drives the REAL `site_provision_job` on a `tauri::test::mock_app`
  against real remotes, and on its first run found **two bugs no unit test could
  reach** — a composer download that died mid-stream reported as "you look offline"
  (24 of 25 packages had just arrived), and `wp core install` pinned to the docroot on
  a Bedrock site whose core Composer puts one level in. `ONLY=bedrock` runs one case;
  each is a real `composer install`.
- **Cannot prove:** a CSS chain resolves, a WKWebView quirk, real-internet DNS
  propagation, anything needing root or a second device (some examples DO take prompts
  — those are L3-adjacent and marked).
- **Cost:** seconds to minutes each; some need network/downloads; some need the stack
  stopped, a few need it running. **Runs:** on demand per feature; `verify.sh` only
  BUILDS them today. §6 adds a tiered runner. **Bug class:** assumed-tool-behavior,
  real-process lifecycle, real-layout path bugs.

### L2 — Render (`scripts/wk-checks/`, Playwright WebKit + mockIPC dev routes)

- **Proves:** layout and copy in the engine family that ships (WKWebView class bugs
  Chrome hides): overflow, collapse, control chrome, dialog flows — via dev-only
  harness routes with mocked IPC, zero backend. Also data-FRESHNESS wiring, by counting
  mocked IPC calls rather than reading pixels (`focusrefresh.js`: a focus event must
  re-read git status and branches, and must NOT fire the lazy network read).
  A rendered IMAGE is proved by decoding, not by presence: `openin.js` reads each app
  icon's `naturalWidth`, because a broken `data:` URI still leaves an `<img>` in the DOM
  that a count-the-elements assertion would happily pass.
- **Cannot prove:** backend truth (IPC is mocked BY DESIGN) — **it proves a card RENDERS,
  never that its command WORKS.** The MCP toggle rendered correctly across 10 harness
  scenarios while `mcp_set_enabled` *aborted the packaged app* on click (the mock returned
  success; the real command was never invoked). A backend crash is invisible here by
  construction — the same shape as "a lib test can't prove mysqldump accepts the flag"
  (§1's table): read ten green harness scenarios as render coverage, never as backend
  coverage. Also can't prove: real cookies/schemes (`rexdb://` doesn't exist in Playwright),
  aesthetics. **And the mock is a fixture that can be WRONG**: `repo_job_state` answered one
  canned job for every id, so the panel's (correct) post-attach snapshot re-read swapped the
  job the click had started for a different one, and `repopanel.js` sat red for months on a
  UI bug that did not exist. A harness that ignores its arguments produces false RED as
  readily as false green — mock by id (fixed 10 Aug 2026).
- **Cost:** ~a minute + a vite dev server. **Runs:** after UI changes; part of the full
  gate (§6). **Bug class:** percentage-height collapse, WebKit metrics overflow,
  states that render wrong or not at all.

### L3 — World (SMOKE-TEST.md, PUBLISH-TESTING.md — scripted manual)

- **Proves:** Gatekeeper/quarantine, privileged prompts, keychain trust, launchd under
  real root, second-device tunnel reach, router DNS caches, sleep/wake, how it looks.
- **Cannot prove:** anything repeatably or cheaply. Which is why everything that can
  move down a layer must (§4), and what stays is SHORT and scripted (§5).
- **Cost:** human hours, sometimes a clean Mac/VM. **Runs:** release gate + per-feature
  items. **Bug class:** the real world disagreeing with all three layers above.

### 1.1 Audit: over-tested at the wrong level (L0 asserting L1 facts)

From the full test sweep (distribution: core/ 469, platform/ 36, state/ 16,
commands/ 9 — commands/ is 1.7% of tests for ~20 files of orchestration):

- **launchd/security/dyld string-pinning** (`platform/macos/mod.rs` — ~15 tests):
  plist XML substrings, `launchctl bootstrap` shell strings, `security add-trusted-cert`
  flag arrays, `@loader_path` semantics. All launchd's/`security`'s/dyld's call. Zero
  live coverage of the daemon path. These stay as template-drift guards but must stop
  COUNTING as proof (ledger #67, #155–156, #162 note the unproven halves).
- **Manifest tests named as if they fetch** (`core/binaries.rs` — 18 tests):
  `every_offered_db_version_is_pinned_and_resolves` — "resolves" means a HashMap lookup.
  No fetch proof at all for cloudflared, composer, non-default DB versions.
  **Closed 15 Aug 2026** (`manifest_sweep_check`, network tier, #335): every pin
  HEAD-probed on both arches and everything under the size cap re-hashed against
  its digest — including the Intel pins that were hashed once and never re-run.
- **The design system's colour contrast was asserted in one comment and checked
  nowhere** (#337, scan landed 16 Aug 2026). `every_text_on_surface_pairing_meets_wcag_aa`
  and `every_rex_colour_class_names_a_token_that_exists` in `core::copy_scan` derive
  both the text set and the surface set from the frontend's own `text-rex-*` /
  `bg-rex-*` usage, so a token joins the check by being used. **They ship as a
  RATCHET first, not as a red bar** — `verify.sh` red would block every commit through
  the pre-commit receipt — and **both ratchets are now gone, deleted with the debt they
  held**. That lifecycle is the point: record, refuse to let it grow, fail when it
  shrinks so the repayment gets written down, delete. Both assertions are unconditional
  today. Two things the scan taught that generalise: it could not originally see a text
  colour set in RAW CSS, and `globals.css` was styling every `::placeholder` at
  2.58-3.35:1 there — the app's most widespread text; and its exemptions were keyed on
  TOKEN NAMES, so a plant that moved an "icon-only" class onto a `<span>` sailed through.
  Icons are excluded structurally now, by reading the element the class sits on. **An
  exemption keyed on a name cannot notice when the thing changes underneath it.**
- **A licence obligation asserted only in prose** (#336, closed 16 Aug 2026). The
  claim "an artifact rexenv BUILT ships its licence texts beside its bytes" is
  discharged by FILES on disk, so a manifest arm returning `Some` proves nothing
  about it. L0 covers the derivation (a self-hosted version with no licence pin
  fails by name) and the staleness rule; **only `php_versions_check` (network
- `php_update_check` (network) — the in-app PHP update chain end to end against the LIVE signed manifest: verify against the compiled-in key, resolve a patch this build was never made with, download it through the existing digest gate (the first time that gate compares against a network-supplied number), run the interpreter, serve FastCGI from it, and check the selection/floor/revert. Does NOT cover the live pool swap on the production port — that is SMOKE-TEST. Ledger #351.
  tier) sees the texts land** — beside `php` AND `php-fpm`, which are separate
  artifacts published by separate resolves. The field-repair path (a cache
  predating the fix) is planted rather than assumed.
- **SQL text under `sql_mode`** (`dbmirror.rs:282`): backslash-escape assertions that
  change meaning under `NO_BACKSLASH_ESCAPES`; the live restore example is the real
  proof.
- **PHP substrings superseded by live checks** (`wp_tunnel.rs:208`, `wp_login.rs:202`):
  the examples run the real plugin under real PHP; the needle lists add nothing and one
  needle (`".test"`) matches anywhere in the file.
- **The incident surface itself was untested at ANY level** (closed by T4,
  `db_dump_flags_check`): `dbdump.rs`'s real dump argv and `database.rs`'s exclusion of
  `--connect-timeout` were documented in prose only — prose the new proof promptly
  FALSIFIED (mariadb-dump warns-and-ignores rather than hard-erroring; ledger #195).

### 1.2 Audit: assertions that are wrong or vacuous (delete/fix, not keep)

- The **>80-char verdict check still exists and is still wrong**: `dbcompat.rs:456`
  `text.len() > 80` — bytes not chars (em-dashes count 3×), and length is not a proxy
  for "teaches anything." Same pattern: `dbcompat.rs:580`, `confverify.rs:211`,
  `phpconf.rs:842`.
- Unfailable: `terminal.rs:201` (`contains("")` always true).
- Split-assert blindness: `macos/mod.rs:2007` (`RunAtLoad` and `<true/>` asserted
  independently — a `<false/>` plist passes).
- Constant tautologies: `mail.rs:358/363/384`, `wordpress.rs:2696`, `binaries.rs:2409`.
- Names promising external facts fixtures can't prove: `…_resolves`,
  `the_live_sample_imports_cleanly…`, `…parse_live_wp_cli_output`.

### 1.3 Audit: untested because no layer fit (until now)

- `commands/` orchestration honesty (ledger #175, #181–182, #186, #188–190).
- `webview_dialogs.rs` — zero tests, three stacked wry/WebKit claims (#166).
  **Closed 15 Aug 2026, and the layer moved during the measuring** — the queued L2
  check would have tested Playwright's own dialog machinery, not wry's (the subject
  is absent from that harness), so the proofs landed where the subject lives: an L0
  against the real ObjC runtime, `webview_dialogs_check` (L1, real `WKWebView`,
  plant-proven), and SMOKE's drop-table step for the render/eye half.
  `docs/PLAN-webview-dialog-proofs.md` records the measurement.
- Frontend honest-UI surface: StatusPill non-running states, StartStopToggle,
  StatusFooter, Tunnels tri-state — the WKWebView pill-metrics fix was verified once by
  hand and never committed as a check. **Both halves of this bullet have since been
  closed and the entry is kept as the record of what the gap cost:** the pill metrics
  are now the `pills` probe, and `SiteProvisionCard` gained the `provisionRow` probe on
  9 Aug 2026 — after a long backend phase label pushed the site's domain out of the
  card in the shipped app (ledger #248). The probe failed the first attempted fix,
  which is the argument for having it. The `deleteGate` probe (9 Aug 2026) joined them
  when site delete gained the type-the-domain gate: it types into every delete variant
  and asserts EVERY destructive button — the connected variant has two — is dead on an
  empty box and on a near-miss, and live on the exact domain. Proven by removing the
  `disabled` binding: both widths failed.
- `uireview.js` cannot fail (assertion-free screenshots, overflow non-fatal, not in
  `run-all.js`). **Fixed:** it asserts now — horizontal overflow is a failure, pageerror
  and console.error are failures, per-scenario probes run — and it is in `run-all.js`.
- **The `php-versions` probe checks the row's TRUTH as well as its layout** (18 Aug
  2026). Every defect in the in-app-update row was found by a person looking at a
  screenshot: chips overlapping, the EOL badge wrapping mid-pill, "8.2.32 exists"
  printed beside "Update to 8.2.32", and the button still offered after the update was
  applied. So the probe now asserts a button appears **iff** a verified manifest offers
  something for an INSTALLED minor, that the "exists" chip disappears once the button
  names the same version, that a row with nothing pending renders quiet (no amber
  `serving`), and that the why-no-button note only appears on a screen where some
  upstream version genuinely has no button. Plus a fixture-coverage assert over five
  states, because before it every mock row was installed and none was post-update —
  each rule would have been a branch nothing exercised.

  **What it cannot ask, stated rather than implied:** whether the patch displayed is
  the post-update one. Nothing in the DOM carries the compiled-in pin, deliberately —
  the row's job is to show what the minor WILL RUN. That comparison is L0
  (`after_an_update_the_row_shows_the_new_patch_and_offers_nothing`); this layer adds
  the half L0 cannot see.

  **Two of these rules were wrong when first written and planting is what said so.**
  Un-suppressing the chip failed by name (good). But asserting "offers a patch ⇒ has a
  button" failed a LEGITIMATE state — a manifest carrying 8.0.31 for a minor the user
  never installed, where the absence of a button is correct — so the rule is now scoped
  to installed rows. A probe that convicts correct behaviour trains people to ignore it.
- **The harness itself stopped answering questions it does not know** (18 Aug 2026).
  `DevUiReview`'s `mockIPC` default arm returned `1` for any unmocked command. The PHP
  section fires `php_update_check` on mount, so `1` was published into the versions
  query cache and `versions.some(…)` threw — the whole view rendered nothing, at the
  exact moment a second reader of that array appeared. The default now accepts Tauri's
  own plumbing quietly and THROWS for an app command, naming it. A fixture that answers
  everything with a friendly value cannot fail; an unmocked command is the harness
  saying it does not know.

## 2. The claim inventory

Full inventory: **`docs/CLAIM-LEDGER.md`** — 194 claims from a sweep of every
invariant-language comment in `src-tauri/src`, each with file:line, the claim, and a
verdict. Tally at compile time: **117 proven · 32 half-proven · 36 provable-unproven ·
9 inherently unprovable.** The 🔨 rows plus the unproven halves of ◐ rows ARE the
testing backlog, worked highest-risk first:

1. CF-header set as tunnel discriminator (wp_tunnel + wp_login) — L1
2. "a tunnel can only expose its one site" — asserted in three modules, negative never
   probed — L1
3. dotfile guard proven only as a config substring — L1 (live 404) *(closed: nginx at T11, Apache+FrankenPHP 15 Aug 2026 — #103 ✅)*
4. login-autostart "never download / never prompt" — untested at any level — L0+L1
5. wp_login PHP-injection safety inherited, not tested at the injection point — L0
6. DNS answer-anything justified by an unasserted loopback bind — L0
7. `webview_dialogs.rs` wry/WebKit claims — L2 *(measured 15 Aug 2026: L2 cannot
   see the subject; closed at L0+L1+SMOKE instead — see #166)*

Ledger discipline: a new "must never / always / is safe because" comment adds a row in
the same commit. A proof-commit flips the verdict and names the proof. That keeps the
metric honest and the ledger from rotting into the archive problem.

**And for any claim whose subject is a third party's behaviour, the FIRST step is a
measurement of which layer can observe the subject at all — before any test is
designed.** Promoted from a note to standing rule 15 Aug 2026, after it saved two
sessions in one week: #2/#33 (the CF-header set was simulated in every guard until a
live run measured it) and #40/#166 (both carried `🔨 L2` for months; the measurement
showed Playwright contains neither subject, and a wk-check would have tested
Playwright's own machinery forever). The dotfile guards went eight months as a config
substring for want of the same question.

## 3. Coverage of the classes we actually hit

Per recurring class: the honest mechanism — lint, test helper, or documented audit.
"Only a periodic audit catches this" is stated where true.

### 3.1 One fact computed in two places / recorded-vs-derived (same family)

- **Test pattern (exists, now named): the POISON TEST.** Plant a derivable state that
  contradicts the record; assert the record wins
  (`content_dir_rel_reads_layout_markers_and_resists_poison` is the template). Every
  recorded fact (`db_name`, `override_port`, `docroot_managed`, `content_dir`,
  `db_created`) must have one; ledger tracks which do.
- **Lint (partial, honest about limits):** a source-shape lib test (the
  `include_str!` pattern already used for identity-drift guards) greps for known
  re-derivation shapes of recorded columns outside their minting sites. Catches known
  shapes only.
- **Residual: design review.** The class is born at write time; the checklist question
  is "is this fact recorded anywhere already?" No lint can see a novel derivation.

### 3.2 One-time check on a mutable fact (lifetime guards)

- **Not lint-able in general.** The two July instances were both cross-site exposure
  under a live share; each got an instance test.
- **Mechanism: documented audit question** at review time: *"does this guard hold for
  the dependent thing's LIFETIME, or is it a snapshot?"* Plus: any new `ensure_*`/
  preflight that reads mutable state gets a test for the state-changed-after case
  (`commands/sites.rs:311`'s guard family, ledger #188).

### 3.3 Path assumption a layout invalidates (Bedrock class)

- **Test helper: a layout fixture matrix.** `layouts()` in test-support + examples
  common: stock, Bedrock (`web/` + `app/`), subdir-docroot (`public/`), each a small
  on-disk skeleton. Any path-building fn gets matrix-parameterized tests; one L1
  example provisions a real Bedrock-shaped site (today only a unit test covers it).
- The class is fully testable once the matrix exists — no audit needed, but adding a
  NEW supported layout must add a matrix row (review checklist).

### 3.4 Assumed tool behavior (the mysqldump class)

- **Rule: every argv/flag/config handed to an external tool has an L1 example that
  feeds it to the REAL tool.** The ledger is the registry — each such claim is a row;
  a new tool-fact needs its example named in the verdict column.
- **Not lint-able.** A grep can't know which strings reach argv. The mechanism is the
  ledger + the §1.1 rule that an L0 string test never counts as proof of acceptance.
- Immediate backlog: dump-flag acceptance per bundled vendor (§4), launchctl daemon
  strings (L3 clean-VM procedure — root), git version-dependent flags (already
  L1-covered by `repo_*` examples), `frankenphp validate` (its example currently
  asserts nothing).

### 3.5 Recorded-vs-derived disagreement

Covered by 3.1 (same family — poison tests + the recorded-fact ledger rows).

### 3.6 Percentage-height chain (the h-full class)

- **Test helper: an L2 heights probe.** wk-checks gains a shared measurement helper:
  key containers assert height ≥ floor, the Adminer iframe ≠ intrinsic-default
  (~150px), bottom within viewport after scroll. The harness already mounts the real
  `DatabaseTab` in a replica region chain — the probe just needs attaching. Other
  full-height surfaces (Terminal, Mail, Databases, SiteLogs) get harness scenarios or
  are named manual.
- **Class-level honesty:** the probe catches collapse on surfaces IN the harness.
  A new full-height surface must be added to the sweep (review checklist).

## 4. The real-dependency layer (L1) — making the best layer cheaper

Audit that MOTIVATED this section, taken before 28 Jul 2026 — a dated snapshot, not
the current state. Since then the runner exists (`scripts/live-checks.sh`, with the
tier table and its two-way completeness check). The rest is kept because it is the
inventory the work was aimed at: 105 examples; only 15 use
`common::sandbox`, 9 use `Reaped`; three still have the exact incident-3 shape
(`nginx_php_serve`, `php_fpm_serve`, `php_pools_serve` — real prefix/config, production
ports); 8 sandboxed examples still bind production ports (18088/9783); the WP-install
block is copy-pasted ~12×; temp-DB filenames collide across two example pairs; verdict
reporting is inconsistent (assert vs exit(1) vs print-only); **there is no runner**.

Workstreams, in order:

1. **Uniform verdict contract.** Every example exits 0 = proven / non-zero = not.
   Print-only "examples" (`frankenphp_subdir_validate`, `dns_serve`, demos) either gain
   assertions or are renamed `*_demo` and excluded from the runner.
2. **A tiered runner** (`scripts/live-checks.sh`), same discipline as verify.sh (own
   cwd, no pipes between check and verdict, one green line):
   - `sandbox` tier — sandboxed, fixture ports, no network, no prompts: safe anytime.
   - `service` tier — spawns real services on fixture ports; stack may be running.
   - `network` tier — needs internet (fetch checks, tunnel, wp.org).
   - `stack` tier — needs the user's running stack (adopt/wire-probe/resource checks).
   - `system` tier — prompts/root/system mutation: run deliberately, never in bulk.
   Tier membership is a table in the script; unlisted examples are a build-time error
   so new examples must declare a tier.
3. **Fixture library** in `examples/common/`: `fixture_db(tag)` (collision-free temp
   SQLite), fixture-port allocator (kills the sandboxed-but-production-port class),
   `wp_fixture()` (the 12× install block), a shared `Check` reporter with the uniform
   exit contract.
4. **Sandbox adoption**, worst first: the three incident-3-shaped examples, then the
   8 production-port sandboxed ones, then the ServiceManager cohort (real Sites-folder
   docroots today). Deliberately-real examples (`seed_and_list`, `system_setup`…) get
   an explicit REAL-SYSTEM banner + `stack_guard` opt-in instead of a sandbox.
5. **New examples the gaps demand** (from the ledger and §3.4): dump-flag acceptance
   matrix per bundled vendor+version; MariaDB-client-vs-MySQL8 negative auth; confedit/
   confrewrite output through real `php -l`; Bedrock live provision; dotfile-guard live
   404 (all three templates — landed 15 Aug 2026); fpm candidate-vs-live `-t`
   isolation (landed 15 Aug 2026); tunnel second-Host negative probe; manifest
   HEAD+digest sweep (network tier — landed 15 Aug 2026, #335).

**What moves down from manual:** the frankenphp validate step (was: human pipes
output), fpm reload isolation, dotfile 404s, Bedrock login flow, dump-tool flags —
all currently "found by a human looking."

## 5. What stays manual — short, and scripted

Everything here needs root, a second device, a clean machine, or an eye. Each maps to a
written procedure; "manual" means scripted-for-a-human, never remembered.

| Item | Procedure |
|---|---|
| Gatekeeper/quarantine first launch from the dmg | PUBLISH-TESTING §A |
| Privileged prompts (foreground, cancel/retry) + root daemon on-disk ownership (ledger #67/#155) | SMOKE-TEST robustness + PUBLISH-TESTING §B |
| Keychain CA trust dialog | SMOKE-TEST first-run |
| Second-device tunnel reach; router DNS negative-cache | SMOKE-TEST tunnels (explicitly not-a-bug note) |
| Resolver takeover/restore on a clean VM | PUBLISH-TESTING §F |
| Sleep/wake + reboot edge recovery; DNS agent handoff (#46) | SMOKE-TEST robustness (added, T13) |
| Datadir corruption recovery (B22/B23) | PUBLISH-TESTING §E |
| Firefox out-of-the-box trust (#154) | SMOKE-TEST robustness (added, T13) |
| Visual design / theme correctness | SMOKE-TEST settings + uireview screenshot sweep (human eyeballs the shots) |
| Radicle layout reality (#95) | flagged in code; first real project confirms |
| Phase-A-never-resolves wire spot-check (#19) | PUBLISH-TESTING §L (added, T13) |

## 6. The gate

`verify.sh` stays THE pre-commit bar, unchanged in discipline (own cwd, exit codes
load-bearing, green only from its own line). It grows tiers by composition, not by
bloating the fast path:

- **`scripts/verify.sh`** (fast, pre-commit, minutes): lib tests + **`cli` crate tests**
  + examples build + clippy -D warnings + tsc.
  **Grew the `cli` step 12 Aug 2026, and the gap is worth remembering:** the bar ran
  `cargo test --lib` in `src-tauri` only, so the `cli` crate — a SHIPPED binary, linked
  onto every user's PATH by the cask — was never entered. It also had no tests to run.
  §D then found `rex --version` hanging forever on an app that accepts and never
  answers (ledger #300). A crate that ships and a crate the gate visits are now the
  same set; keep them that way when a third crate appears.
- **`scripts/verify-full.sh`** (before release / after touching a layer's subject,
  ~10–20 min): runs verify.sh, then the L1 `sandbox` tier, then wk-checks `run-all`
  (spawns its own vite on 5199, kills it after). Own `verify-full: all green` line.
- **`network` / `stack` / `system` L1 tiers:** run deliberately when the change
  touches their subject (the runner prints per-tier one-liners for the commit note).
- **L3:** SMOKE-TEST per release; PUBLISH-TESTING per its per-item triggers.

A gate nobody can afford to run stops being one: the fast bar stays fast, and nothing
above it is required per-commit. The metric the gate serves is the ledger tally, which
only proof-commits move.

## Task list (ALL SHIPPED 28 Jul 2026 — kept for the record; evidence in
`docs/archive/SHIPPED-2026-07.md`)

Small, one commit each, ✓-ticked with evidence. Ordered so every
commit lands standalone value.

- **T1** ✍ this plan + the ledger + TODO entry (docs).
- **T2** Fix/delete wrong assertions (§1.2): the four `len() >` checks → structural
  assertions; `terminal.rs:201`; `macos/mod.rs:2007`; delete the constant tautologies.
- **T3** Rename lying test names (§1.2) — `…_resolves` family, `…_live_…` fixtures;
  reduce the two PHP-needle tests superseded by live examples to their non-redundant
  core.
- **T4** NEW `examples/db_dump_flags_check.rs` — every bundled dump tool × the exact
  argv from `dbdump.rs`/`database.rs`, real binaries, incl. the negative
  (`--connect-timeout` rejected). Closes the incident class. Ledger update.
- **T5** Make `frankenphp_subdir_validate` actually validate (run the real binary,
  assert, exit contract). Ledger row for the placeholder-typo class.
- **T6** `examples/common`: `fixture_db`, fixture-port allocator, `Check` reporter,
  `wp_fixture()`; migrate two examples as proof.
- **T7** De-fang the three incident-3-shaped examples (sandbox + fixture ports).
- **T8** `scripts/live-checks.sh` tiered runner with declared-tier enforcement.
- **T9** wk-checks: heights probe on the dbtab scenarios (§3.6), pageerror listeners +
  fatal overflow in uireview.js, add it to run-all; StatusPill all-states scenario.
- **T10** wp_login injection-point test (#36) + DNS loopback-bind assertion (#44) +
  autostart guard tests (#175, L0 half).
- **T11** Dotfile-guard live 404 example (#103, nginx first).
- **T12** `scripts/verify-full.sh`.
- **T13** SMOKE-TEST/PUBLISH-TESTING additions from §5 + ledger re-tally.

Backlog after T13 (ledger-driven, next sessions): tunnel second-Host negative (#10),
CF-header discriminator probes (#2/#33), ~~manifest HEAD+digest sweep~~ (closed
15 Aug 2026, #335), Bedrock live
provision, ~~fpm candidate isolation (#104)~~ (closed 15 Aug 2026), sandbox adoption cohorts (§4.4),
~~`webview_dialogs` L2 coverage (#166)~~ (closed 15 Aug 2026 at L0+L1+SMOKE — the L2
shape was measured impossible), ~~import-graph lint (#163)~~ (closed 15 Aug 2026), rusqlite-outside-state
guard (#167 — closed 15 Aug 2026; see the ledger row).
