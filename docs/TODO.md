# TODO — the single active-work file

Everything open lives here, and ONLY open work lives here — the completed evidence
log this file used to carry is `docs/archive/SHIPPED-2026-07.md`, and everything
finished in August 2026 is now `docs/archive/SHIPPED-2026-08.md` (both historical).
When you finish an item, tick it here with a one-line ✓ evidence note; when a section
is fully shipped, move it to the archive log.

**Reconciled against the code 21 Aug 2026** (HEAD `48e5046`, the first reconcile after
v0.3.0 shipped), by reading every row against the tree rather than trusting it. What it
found is worth more than the tidy file it produced, because it names how this file goes
wrong:

- **57 finished blocks had accumulated here** and are now in `SHIPPED-2026-08.md`. The
  file's own rule ("ONLY open work lives here") had quietly stopped being true, and the
  cost is not tidiness — it is that 50 open rows were hiding among 57 shipped ones.
- **Six rows were open only on paper.** The work had landed in a commit that never
  touched this file: the readiness-gate sweep (`2f564bb`), the "exists" line's
  explanation (`be6a14d`), resolver-drift surfacing, the tunnel-replay leg (`77ba587`),
  the verdict receipt (whose own body had said **DONE** for eight days while the box
  stayed `[ ]`), and PHP 7.4. Every one of them was a commit that did the work and left
  the tick for later; nobody came back.
- **One row described a guard that did not exist** — `wp_dns_check` was said to "FAIL
  LOUDLY the day a build stops using c-ares", and it did not: it printed a NOTE and
  passed green. That is the dangerous kind of staleness, because someone would have
  relied on it. *(Closed 23 Aug 2026, ledger #381 — and the guard it finally got is not
  the one the row described, because measuring first showed 7.4 already has the threaded
  resolver, so "fail when c-ares is gone" would fail on good news.)*
- **And the reconcile itself found new work**: a post-release audit of the readiness-gate
  sweep confirmed 22 defects *inside the fix*, including gates that cannot run and
  examples that report green having asserted nothing.

So the rule this file needs is the one CLAUDE.md already states and this file kept
paying for anyway: **the tick belongs in the commit that does the work.**

## Now — actionable code/test work

- [x] **`frankenphp_edge_serve`: the edge it starts has never been PROVEN to answer —
  DIAGNOSED AND CLOSED 21 Aug 2026, and the cause was not the edge.** Four leaked
  FrankenPHP backends from earlier runs of this same example were found alive (started
  22:38–22:44 the previous evening), **every one of them LISTENING on 127.0.0.1:8200** —
  the recorded override port — serving docroots inside sandbox roots that had already
  been deleted. Two facts made that invisible, and they are the reusable part:
  - **Caddy binds with `SO_REUSEPORT`**, so a new run's backend binds :8200 *successfully
    beside* the squatters and the kernel splits incoming connections across all of them.
    A run could be answered by any of five processes, four of them serving nothing. That
    is the whole of "it served 200/200 twice and then never answered": the green runs were
    borrowed, and so were the red ones.
  - **`ports::is_listening` is a CONNECT probe**, so `await_listening(8200)` returned true
    instantly, satisfied by a corpse. A readiness gate cannot tell your service from the
    remains of your last one.
  ✓ **Fixed by refusing, not by diagnosing**: `common::require_ports_free` now runs as the
  first statement (edge, HTTP edge, shared nginx, pool) and again for the RECORDED
  override port once it is known — before anything is spawned — so a leaked previous run
  stops the next one loudly instead of answering for it.
  ✓ **Proven by running it**: with the four corpses reaped, `live-checks.sh service` came
  back **all green**, 22 examples, this one included — `ng.test` 200 (PHP 8.3.31) and
  `fp.test` 200 (PHP 8.5.8) through the edge it started — and **zero processes and zero
  held ports survived the run**, which is the `OwnedService` half of the sweep-defect work
  proven at runtime rather than by reading.
  The three earlier defects (derived-vs-recorded port, accept-vs-answer, raw `Child`s) are
  in `docs/archive/SHIPPED-2026-08.md`.
- [x] **The readiness-gate sweep's OWN defects — 22 confirmed, all seven families fixed
  21 Aug 2026.** The sweep itself is done (`be3c88d` + `2f564bb`, archived); this row is
  what an adversarial audit of it found afterwards, one verifier per finding, 44 further
  claims refuted. Every sub-item below is closed, one commit per family.
  **Ran, and what that does and does not prove.** `scripts/live-checks.sh sandbox` came
  back **`live-checks(sandbox): all green`** on 21 Aug 2026 with all seven families in —
  56 examples, including the ones the fixes touched that live in that tier. So the changes
  compile, the new `ExitCode` contract does not fire on a healthy machine, and the
  `OwnedService`/fixture-dir guards tear down cleanly.
  **The tiers where most of these files LIVE were not run**: service, network and stack
  need the stack stopped and a human to start them. For those files the fixes are the
  shape being right, not the run being green — and the honest way to close that gap is
  one `verify-full.sh` pass plus the service tier by hand, not another reading of the
  diff. They fall
  into five families, and the families are the useful part — every one of them is a way a
  gate can be PRESENT and not GATE:
  - [x] **Placement — a gate after an early exit is not a gate.** ✓ 21 Aug 2026, both
    instances. `mail_route_check`'s two gates had landed INSIDE the pre-existing
    `for _ in 0..40` edge loop, one statement past its `break`: on the ordinary path (edge
    already listening on the first poll) they never executed at all, and on the unlucky
    path they ran once per 250ms and would have failed naming nginx for an edge that had
    not come up. They are below the loop now, with the reason in the code so the next
    editor does not put them back. `wp_install_serve`'s MySQL poll had the sibling shape —
    15s, fall through, print `mysql running=false`, carry on into `install_for_site`, so a
    dead engine surfaced as wp-cli's error rather than MySQL's — and is now
    `common::await_ready("mysqld (accepting queries)", …)`; `await_ready` rather than
    `await_listening` because `mysql_running` is a protocol check, not a port listen.
  - [x] **Silent pass — `start_all` fails and the example exits 0.** ✓ 21 Aug 2026, all
    13 files. `if let Err(e) = … { eprintln!(…); return; }` inside `async fn main() -> ()`
    returns SUCCESS, and `live-checks.sh` takes its verdict from the exit status alone —
    so the tier printed `all green` for runs that asserted nothing and never reached their
    gates. Every one now has `async fn main() -> std::process::ExitCode` with `FAILURE` on
    the precondition path: `adminer_deeplink_check`, `adminer_serve_check`,
    `blueprint_check`, `log_tail_check`, `mail_route_check`, `monitor_coverage_demo`,
    `multisite_check`, `multisite_wildcard_check`, `network_check`, `server_switch_serve`,
    `service_manager_demo`, `tunnel_check`, `wp_login_check`. **`FAILURE`, not
    `process::exit(1)`** — the two examples that already exited non-zero were skipping
    every destructor to do it, which turns a failed run into a leaked service. The rule is
    written down once, in `examples/common/mod.rs` ("The verdict contract: exit 0 means
    PROVEN"), and each site carries a two-line pointer to it, because this idiom is what
    an editor reaches for by habit.
  - [x] **Leak — a PANICKING gate in front of raw `Child`s.** ✓ 21 Aug 2026. Rust does
    not kill children on drop, so every gate the sweep added between a spawn and its
    teardown was a new leak path holding a SHARED production port. All six are
    `common::OwnedService` now, with the happy-path teardown calling the guard's own
    idempotent `stop()` instead of a hand-written duplicate: `create_site_serve`,
    `delete_site_serve`, `frankenphp_serve`, `php_per_site_serve`, `php_switch_serve`,
    `wp_install_serve` (mysqld included — four services in that one). `OwnedService` and
    not `Reaped`, deliberately: `Reaped`'s sweep is keyed on the program NAME, which on a
    production port would kill the user's own nginx. `monitor_coverage_demo`'s instance
    was the `process::exit(1)` variant and went with the verdict-contract fix.
    `frankenphp_edge_serve` had already had this fix in `48e5046`, where it measured 1
    leaked caddy per panicking run → 0.
  - [x] **Accept-vs-answer — the gate proves the socket ACCEPTS.** ✓ 21 Aug 2026,
    promoted to `common::await_answering` (+ `common::https_status`) rather than copied a
    fourth time, and `frankenphp_edge_serve` — where the local version was written — now
    calls the shared one. Wired into the three that needed it: `server_switch_serve` (the
    sweep had deleted the 1200ms sleep that was the only thing covering Caddy's load
    window, one statement before the first fetch), `delete_site_serve` (its BEFORE-delete
    baseline is what the whole check rests on, and was taken ~1ms after the accept gate),
    and `wp_create_serve` (whose first HTTPS request `.unwrap()`s). The helper waits for
    ANY status and the assertions still demand 200, so it is a readiness gate and not a
    retry loop that hides a bug.
  - [x] **Wrong subject — the gate is satisfied by someone else's server.** ✓ 21 Aug
    2026. `wp_install_serve` now refuses first, like its sibling `wp_create_serve`:
    `common::require_ports_free` on all five production ports, because `sandbox` makes the
    PATHS throwaway and says nothing about ports — beside a live stack its three gates
    were all satisfiable by the USER'S services, which is green while measuring their
    machine. `service_manager_demo` gates the two services BEHIND the edge that its final
    request traverses. `frankenphp_serve` reads :2019 before the spawn and asserts the
    TRANSITION (`!admin_before && is_listening(2019)`) rather than the absolute, so a
    developer's own Caddy no longer fails a claim that is about what FrankenPHP opened.
  **Two fixture-ownership defects came out of the same audit and are NOT gate bugs** —
  they are the invariant in `examples/common/mod.rs` being broken in the plain sense:
  - [x] `adminer_deeplink_check` opened a bare `db::open` fixture DB with no pin, so
    `sites::provision` resolved the docroot from the HOME directory and it then
    `remove_dir_all`'d a path inside the user's REAL `~/rexenv/Sites` — the incident that
    cost this project a Sites folder once already. ✓ 21 Aug 2026: `common::
    pin_fixture_sites_dir` (a Drop-guarded `/private/tmp` dir pinned into the setting) and
    **all seven examples that provision-then-delete now use it** —
    `adminer_deeplink_check`, `blueprint_check`, `cli_wp_install_check`, `multisite_check`,
    `multisite_wildcard_check`, `network_check`, `wp_install_stream_check`. The pin exists
    separately from `pin_sites_dir` because these run on the REAL `Paths` deliberately (the
    real Adminer docroot, the real certs) and cannot use the sandbox door.
  - [x] `adminer_serve_check` wrote `dbprobe.php` into the REAL Adminer docroot and
    removed it with a bare statement at the end of `main` that six `assert!`s could unwind
    past. Every non-dotfile `.php` there is directly executable through the console's own
    vhost, so a failed run left a root-connectivity endpoint outside every control the
    wrapper installs. ✓ 21 Aug 2026: `common::FixtureFile`, a write-and-own guard whose
    Drop removes it on the panic path too. **NOT renamed to a dotfile** — the probe is
    fetched through nginx on purpose, and `NGINX_DOTFILE_DENY` would 404 the thing the
    check exists to measure; the ownership is the fix, not the name.
  **What this row is really about.** The sweep was written to end a bug class and was
  verified by `verify.sh` + the sandbox tier, both of which only compile these files or
  run a seventh of them. The class it was closing (spawn-then-use) is genuinely closed;
  what it could not catch is that a gate is a RUNTIME claim and the gate on tiers nobody
  runs is a comment that compiles. Fix them family by family, and prefer the fix that
  makes the shape impossible (an `ExitCode` contract, an `OwnedService` type) to the fix
  that adds another line someone must remember.

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
  *(Not doing: forcing `display_errors=stderr` into the rexenv terminal too. That terminal
  is deliberately the user's own environment (#228) and the flag would have to arrive as an
  injected env var, which is a bigger promise broken than a deprecation line shown.)*
- [ ] **Private-window flags for Arc, ChatGPT Atlas, Orion.** Left `None` in the
  `BROWSERS` table because no one has run the flag on a real install, and a fork
  that swallows the flag it inherited opens an ordinary window under a control
  that said private. One-line each once tested; the rows simply show no private
  icon until then.
- [ ] **Windows/Linux: `detect_browsers`/`open_in_browser` are the default empty
  stubs** (Phase 4, same shape as `detect_editors`). Until they are filled, those
  platforms open every link in the OS handler and show no chevron — honest, but
  the Settings row will read "No browser detected". The private-window arm is
  part of that stub: `supports_private` is false everywhere, so those platforms
  show no private target rather than a dead one.

- [ ] **Eliminate the bug class: bundled PHP with curl's THREADED resolver** (the real
  fix for #251 — the mu-plugin covers the WordPress HTTP API, not raw `curl_init()` in
  a plugin, and not non-WordPress PHP apps rexenv hosts). Needs a self-built
  static-php (`--enable-threaded-resolver` instead of `--enable-ares`) for 7 minors ×
  cli/fpm × 2 arches. **That path is no longer blocked** — it was "the same
  self-hosted-artifact path the Xdebug debug build is blocked on", and as of
  14 Aug 2026 `rexenv/runtimes` builds, gates, signs and publishes exactly this
  shape of artifact (`docs/PLAN-php-74-support.md`). **RULED 15 Aug 2026: not
  now.** Rebuilding seven minors self-hosted is a maintenance burden carried
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
- [x] **Debug-log truth on Bedrock** ✓ 24 Aug 2026 — ledger #391, plant-proven both ways.
  The row said "parse `config/application.php` env defines". Parsing PHP was never needed:
  that file reads `env('WP_DEBUG')`, so the VALUE is in `.env`, which `core::dotenv` already
  parses. The whole change is knowing where to look.
  **The refusal half did not move, and it is the load-bearing one.** Only EXPLICIT keys
  count — a missing `WP_DEBUG` stays `indeterminate` rather than becoming "off", which is
  the lie the field exists to prevent. A value hardcoded in `config/application.php` instead
  of `.env` also stays indeterminate; that is the remaining gap and it is stated, not hidden.
  The project root needs BOTH markers (`.env` beside `config/application.php`) because a
  bare `.env` says nothing, and only the docroot and its parent are searched — further up is
  the Sites folder and somebody else's `.env`.
  **The §3.3 matrix caught the change by failing on it**, named the row, and forced a
  modelling fix: `config_readable` and `debug_determinable` had been one field, on the
  assumption wp-config.php is the only place defines live. Three Bedrock-shaped rows now —
  with an env answer, without one, and a stray `.env` with no marker.
  **One guard was proving nothing until a plant said so**: dropping the
  `config/application.php` requirement passed every test until `stray-env` was added.
  ✓ **Live-verified 24 Aug 2026** on a real linked Bedrock tree with the app rebuilt from
  the fix: rexenv detected `content_dir=app`, and all four branches answered correctly —
  both keys present, no `WP_DEBUG`, `WP_DEBUG=false` (determinate OFF, not unknown), a
  custom `WP_DEBUG_LOG` path, and the marker moved away (indeterminate again). Fixture and
  site deleted after. The live leg exercised `core::logs` with the real row's path and
  recorded content dir, not the UI — `wp_debug_log_status` has no CLI arm.
- [x] **WP Manager cron list: arguments display** ✓ 23 Aug 2026 — ledger #384.
  The row guessed "likely the event-args column". **There was no such column, and no
  args anywhere**: `cron_event_list` asked wp-cli for `hook,next_run_gmt,
  next_run_relative,recurrence` and stopped, so the field was never fetched, never in the
  DTO, never rendered.
  **Why that is a defect and not a missing nicety:** WP-CLI addresses cron events by HOOK
  — there is no per-instance id, which `cron_run_hook`'s own comment already said — so a
  hook scheduled more than once renders as N identical rows whose Run buttons all do the
  same thing. Args are the only thing telling them apart. Measured against a real site on
  this machine (23 Aug): `action_scheduler_run_queue` carries `["WP Cron"]`, and Action
  Scheduler ships with WooCommerce, so this is common rather than exotic;
  `publish_future_post` carries a post id per scheduled post.
  Shipped: the field is fetched and formatted in core (args are arbitrary JSON — numbers,
  nested arrays, objects — so a client-side stringify would disagree the first time a
  plugin scheduled a non-string), an Arguments column, args included in the filter, and
  the Run tooltip on a duplicated hook now says every instance runs.
  The mock returned `[]`, so the cron panel rendered BLANK in the dev harness and no L2
  check could ever have seen it; it now carries the duplicate-hook case.
  - [ ] **Still QA's to confirm.** This is a diagnosis of the most defensible defect in
    that panel, not the repro — the original complaint was never captured. If it was
    something else, say so and this reopens.
  - [ ] **No L2 leg:** the WordPress manager's cron tab has no dev-route arm, so nothing
    asserts the column RENDERS. Adding one means a new `DevGitPanel` arm plus a check —
    worth it next time this panel is touched, not on a placeholder row.
- [ ] ⚠ **PostgreSQL's pinned builds carry `minos 26.0` — presumed dead below
  macOS 26, and the presumption cannot be tested from this machine** (found
  15 Aug 2026 during the floor sweep; MEASURED as far as this host allows the
  same day). What the measurement showed: dyld on macOS 26 enforces minos for
  NEITHER main executables NOR dylibs — postgres patched to `minos 99.0`
  (binary and libssl, re-signed) runs clean — while deterministic dyld crashes
  of minos-15 binaries on macOS 14 are documented in the wild. So enforcement
  is a property of the OLDER host's dyld, and only a macOS 14/15 VM can settle
  whether postgres actually fails there (PUBLISH-TESTING clean-VM shape).
  **The failure surface IS traced, labelled as prediction:** if dyld kills the
  child at spawn, the user gets "PostgreSQL did not start within Ns — see
  postgres-stdout.log" (`await_ready` names the log; `spawn_logged` captures
  BOTH streams, so dyld's real reason lands in that log) — a timeout pointing
  at a log that holds the truth, not a silent no-op, but the top-level line
  says nothing about macOS versions. The VM check has TWO questions, not one
  (ruled 15 Aug 2026): whether it fails, AND whether the message leads anywhere —
  a user reading "didn't start in Ns" looks at Postgres, not at their macOS
  version, so if the refusal is real the top-level line needs the version fact,
  not just the log pointer. If the VM confirms the refusal: re-pin to
  lower-target theseus-rs releases (or another source), and consider a
  version-aware tell on the Databases screen.
  - [x] **The message half is DONE** ✓ 23 Aug 2026 — ledger #383. This was ruled as the
    second of the VM's two questions, and it did not need the VM: whether the top-level
    line leads anywhere is answerable here, and the answer was no. A start failure now
    reads *"PostgreSQL did not start within 15s — this build needs macOS 26.0, and this
    Mac runs 15.4, which can stop it loading at all — see postgres-stdout.log"* (observed
    by planting an older host, since this machine is on 26 and gets no note).
    The floor is READ from the Mach-O rather than recorded, because the recorded version
    has already been wrong once — `docs/PORTS.md`'s table said all PHP minors were 12.0
    while 8.0.30 was 14.0, from the day it was written. `core::macho` is cross-checked
    against `otool -l` on the real cache (14 binaries, 23 Aug).
    **It is diagnosis, never a gate, and that must stay.** Refusing to spawn on `minos`
    would block builds that may run perfectly, on a prediction this tree cannot test —
    dyld on macOS 26 enforces it for nothing. Running only after a failure makes a wrong
    prediction free: the sentence simply never appears.
  - [ ] **Still open, and still needs the VM:** whether dyld actually refuses these builds
    below macOS 26, and therefore whether to re-pin to lower-target releases. Nothing
    above changes that — it makes the failure legible if it is real, not less likely.
- [ ] **Option, not a commitment: a self-built nginx (deployment target 12)
  would drop the app floor from 15 to 14** (MySQL's floor). Same
  `rexenv/runtimes` path that built PHP 7.4; recorded like the c-ares ruling -
  known, waiting for a reason (e.g. macOS-14 users actually asking).
- [ ] **PHP 7.4 — the five residuals of a shipped feature** (`docs/PLAN-php-74-support.md`;
  the stage log is in `docs/archive/SHIPPED-2026-08.md`). Kept as open rows because they
  were living inside a ticked block, which is where open work goes to be forgotten.
  - [ ] **`rexenv/runtimes`' release notes for `php-7.4.33-6` describe `-4`.** Two
    lines are stale on the release page users and auditors read: it says
    `MACOSX_DEPLOYMENT_TARGET=11.0` was "asserted per artifact" (the artifacts
    measure `minos 12.0` — spc's macOS default; PLAN §10b corrected 11.0→12.0 and
    the workflow followed, the prose did not), and it says "the extension set is
    narrower than the 8.x builds … widening is in progress" when `-6` IS the
    parity build (60 modules, five documented absences). Fix in the runtimes repo;
    nothing in rexenv depends on it, but a release page is a claim surface.

  - [ ] **PCRE JIT is compiled OUT of 7.4** — PHP 7.4 bundles PCRE2 10.35 (May
    2020), too old for Apple Silicon JIT, so Composer died on `Allocation of JIT
    memory failed`. `--without-pcre-jit` removes the capability. Regex throughput
    on 7.4 is therefore lower than on the 8.x rows. Worth revisiting ONLY if
    someone builds 7.4 against a newer external PCRE2; not worth it for an EOL
    version nobody runs for speed.
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
### Opened by the 23 Aug 2026 service-tier run (stack UP)

- [ ] **`delete_site_serve` read the USER's nginx and called it its own.** Found by running
  the service tier against a live stack — which is not how that tier is meant to run, and is
  exactly why it was worth watching what each failure DID.
  Eleven of the thirteen failures were loud and correct: production's own `ports::ensure_free`
  refused at bind time and named the holder. Two — `frankenphp_edge_serve` and `mcp_mail_check`
  — refused BEFORE creating anything, with the port, what was answering, and the fix
  (`common::require_ports_free`, added 21 Aug).
  `delete_site_serve` did neither. Its nginx failed to take `:18088` (the user's had it), and
  then **`await_listening(18088)` passed against that nginx** — `ports::is_listening` is a
  CONNECT probe, so somebody else's server satisfies it. The run continued and printed
  `BEFORE delete: del.test -> HTTP 200 / keep.test -> HTTP 200`, which is a fixture claiming
  its precondition holds while reading a server it does not own. It only failed later, on an
  `unwrap` of the nginx reload, because the pid file its own sandbox expected was empty.
  **That ordering is the finding.** The example got as far as printing evidence that looks
  like a passing precondition; the reload happening to fail is what stopped it, not a guard.
  Same family as the SO_REUSEPORT case in `frankenphp_edge_serve` (21 Aug): a readiness gate
  satisfiable by someone else's process is not a readiness gate.
  - [x] **Given `require_ports_free` before it creates anything** ✓ 24 Aug 2026, like the two
    that refused. Verified in the state that produced the bug — with the stack UP it now
    refuses cleanly naming `:18088`, instead of half-running and printing two false 200s.
    The runner refuses the whole tier; this covers the example someone runs BY HAND, which
    the runner cannot.
  - [x] **Decided: the tier RUNNER** ✓ 23 Aug 2026. 19 of the 23 service-tier examples have
    no `require_ports_free`; most are saved by production's `ensure_free` firing when they
    try to BIND, which is luck rather than design — it does nothing for an example that
    reaches a reload or a read path first, as this one did. The runner is the half that
    cannot be forgotten when someone adds the 24th example.
    **The precondition was already written down and not enforced**: `live-checks.sh`
    printed `NOTE: the service tier assumes the rexenv stack is STOPPED` and carried on.
    A note is not a control — the same shape as the c-ares note and the `wp_dns_check`
    note closed earlier today. It refuses now, naming every port that answered.
    No env override, deliberately: an escape hatch here would recreate the note.
    Only rexenv's OWN fixed ports are probed, not `:443` — the edge outlives the app,
    other tools shadow-bind it, and "some Caddy is up" is not "rexenv's stack is running".
    The `stack` tier's opposite note is enforced too: with nothing up, its examples would
    assert against absence and could only pass vacuously.
    Verified live both ways: service refused with the stack up; stack proceeded with the
    stack up; and the stack-down branch fires when pointed at ports nothing answers.
    Per-example `require_ports_free` is still worth adding — the runner protects the tier,
    not a single example run by hand — but it is no longer the only thing standing between
    a live stack and a fixture reading it.
  - [x] **Replace the `unwrap`? NO — closed as won't do** ✓ 24 Aug 2026, and the reason is
    worth more than the change would have been. The obvious replacement leaks: `fpm`,
    `nginx` and `caddy` are live `OwnedService` guards that reap in `Drop`, and
    `std::process::exit` does not run destructors. A panic UNWINDS, so every guard still
    stops its service; a tidy exit would trade a noisy backtrace for three leaked processes
    on fixed ports — the exact leak class `common::Reaped` exists to end.
    **The rule this makes explicit:** `require_ports_free` may exit only because it runs
    BEFORE anything is spawned. After that line, exiting is the unsafe option. Recorded at
    the call site so nobody "improves" it later.

- [x] **The stack tier had no green anyone had seen — it has one now** ✓ 24 Aug 2026.
  `live-checks(stack): all green`, 9 examples, services up and the app quit. **Three of the
  five failures were real defects, and none of them was in the product** — each was a check
  asserting something the code had stopped doing, or never did.
  - [x] `cli_socket_check` / `mcp_socket_check`: **precondition, not a defect.** Their real
    requirement is services UP and the app QUIT, which is coherent — services outlive the app
    by design — while the tier header said only "needs the stack RUNNING". Both pass with the
    app quit. **A tier whose stated precondition is a subset of its examples' cannot be
    satisfied by reading it**, so the header now says so.
  - [x] `edge_adopt_reload_check` ✓ ledger #388 — matched only the per-user cache marker, so
    it could pass only when the edge was the UNPRIVILEGED caddy, i.e. never on a shipped
    install. The privileged edge runs a root-owned *unversioned* copy under `/Library`,
    because a root daemon executing a user-writable binary is a privilege escalation. It
    matches both now.
  - [x] `adminer_proxy_check` ✓ ledger #389 — stale twice over: a hardcoded POST body with no
    CSRF token (Adminer answers that with 403 and a re-rendered form), and then an assertion
    on a 302 that `forward` is designed to follow internally, because WKWebView cannot follow
    a redirect from a custom-scheme handler.
  - [x] `site_resources_check` ✓ ledger #390 — re-derived a database name the model's own doc
    forbids re-deriving, so it demanded `wp_photocontest_test` from a site recording
    `photocontest`. **Fires on any install with an imported site.** It also stopped asserting
    that every WordPress site has a database — a claim about the user's data, not rexenv's
    behaviour — and reports the exceptions by name instead.
  **The transferable finding:** a tier nobody can run to green is a tier whose checks rot
  silently. All three had been wrong for a while; none was noticed, because the tier had a
  precondition nobody could satisfy and so was never run.

### Opened by the 21 Aug 2026 reconcile

These are rows the audit created, not rows it inherited. Grouped because they share one
cause: a commit did the work and the surrounding claim stayed as it was.

- [x] **Six hand-written counts across the docs were stale** ✓ 21 Aug 2026, and fixed as
  a class rather than as six edits. `scripts/doc-counts.sh` computes the schema version,
  the two command counts and the IPC export count from the code and FAILS when a doc
  disagrees; `verify.sh` runs it beside `ledger-tally.sh`, which had already proved this
  shape on exactly this problem. The numbers it now enforces: schema v37 (docs said
  v25/33), `commands/wordpress.rs` 61 (60), `commands/repo.rs` 26 (24), IPC 233 exports
  (217).
  **Two were fixed by DELETING the number instead.** `docs/ARCHITECTURE.md`'s "539 and
  growing" lib tests was wrong by ~350 and cannot be computed cheaply, so it now says
  where to get it; `docs/TESTING.md`'s copy of the ledger tally ("194 claims …
  117/32/36/9") was a second copy of a generated number, drifting exactly as the ledger's
  own summary had before the tally script was written to stop it — it now points at the
  script. A number in the docs is either derived or it is not a number.
- [x] **The MCP server was a shipped subsystem `docs/ARCHITECTURE.md` did not know
  existed** ✓ 21 Aug 2026 — §8.3, written from a citation-backed read of the code rather
  than from the plan (which the code contradicts in eight places, now recorded).
  ARCHITECTURE's §1 inventory, README's feature paragraph and CONTRIBUTING's
  deliberate-decisions list all carry it now.
  **Writing it found three claims in the CODE that were false**, which is the argument
  for writing these sections at all: `mcp_server.rs`'s module doc and ledger #198 both
  said the socket "reuses `cli_server::bind`" — it cannot, that binder returns a tokio
  listener and panics off-runtime, which is how the toggle crashed in the packaged build;
  and two comments said `args_summary` is NULL for "seven of the eight" tools when there
  are eleven. All three corrected in place, each recording what it had said.
- [x] **`CLAUDE.md`'s router carried four labels that were the opposite of the truth** ✓
  21 Aug 2026. "(parked)" for the Valet/Herd migration whose four stages all shipped;
  "(planned)" for PHP 7.4, shipped 15 Aug; "(proposed)" for MCP, whose M1/M2a/M2b shipped;
  "(ruled, not started)" for `wp dist-archive`, finished 5 Aug — plus two rows that named
  a shipped feature as a plan (binary updates, Adminer updates), and no row at all for
  `docs/PLAN-browser-preference.md`. Each label now says what shipped and when. The router
  is the first thing every session reads, so a wrong label costs on every task, not once.
- [x] **Six PLAN headers asserted a state the tree contradicts** ✓ 21 Aug 2026.
  `PLAN-adminer-updates` ("being built" vs shipped the same day it was designed),
  `PLAN-binary-updates` ("PROPOSED, not started" against a live signed manifest, a done
  key ceremony and `core/updates.rs`), `PLAN-git-site-clone` (Bedrock unverified — it was
  verified the day it shipped, and was broken, #294/#299; only Radicle is unverified),
  `PLAN-php-74-support` (extension parity ⏳ vs S1.2's 60 modules),
  `PLAN-valet-herd-db-import` §9 ("Stage 3 does not exist yet" — it shipped 28 Jul), and
  `PLAN-mcp-server` ("building M2a → M2b → M3" with both done). A plan header is what a
  reader checks BEFORE deciding whether to build something, so a stale one invites
  someone to rebuild what is already there — which is why each correction says what it
  had claimed, not just what is true now.
- [x] **Code comments point at TODO rows that are not in TODO** ✓ 23 Aug 2026.
  The row's own inventory was already half-repaired when it was worked: `core/sites.rs`
  and seven of the nine "Deferred services" citations had been corrected in passing, so
  the sweep found four live dangling pointers, not ten — `tunnel_exposure_check.rs:243`
  and `common/mod.rs:414` (both → `SHIPPED-2026-08.md`), `php.rs:1820`'s watchdog/Start-all
  race (→ `SHIPPED-2026-07.md`), and `core/apache.rs:1`, the last bare `TODO "Deferred
  services"`. The other nine `docs/TODO.md` citations in Rust were each checked against
  the file and DO resolve; they are left alone.
  **The durable half is a gate, because a sweep repairs a list and not the mechanism.**
  `scripts/doc-counts.sh` now fails on any `docs/*.md` path the tree cites that does not
  exist (38 paths today), plant-proven three ways. It found a fifth pointer the sweep was
  not looking for: `docs/PLAN-adminer-updates.md` naming that repo's manifest doc as if
  it were ours, when it is a path in the SIBLING `rexenv/runtimes` repo — now qualified
  with its repo, and the scanner grew a leading boundary so a sibling-repo path is not
  read as ours.
  Two traps hit while building it, both already named in this tree:
  - **the scanner counted itself, twice** — writing the sibling-repo example out in
    full made `doc-counts.sh` a citation of a file that does not exist, and then this
    very row did it again while describing the fix. It is not avoidable in general: a
    scan over source text cannot tell prose ABOUT a path from a reference TO one, so
    anything writing about a missing doc must avoid spelling it. Recorded because the
    next person to describe this gate will hit it a third time;
  - **the landmark could never have printed.** Under `set -euo pipefail` a grep that
    matches nothing fails the pipeline, fails the substitution and aborts the script:
    exit 1, no output — the exact state the landmark exists to REPORT. Found by
    planting it, which is the argument for planting the vacuous case and not only the
    defect case.
  **Stated limit** (`docs/TESTING.md` §6): the gate catches the rename/delete class, not
  the commoner one — a ROW moving between files, which is what actually happened here and
  leaves the cited path valid. Closing that needs anchors in the target.
- [x] **`caddy_serve` announced READY before anything was listening** ✓ 21 Aug 2026. It
  called `proxy::start` (which returns at fork), printed `CADDY_READY https=8443
  http=8080` and slept 20s for a human to curl — so the human curled into a socket that
  might not exist and read the refusal as rexenv's edge failing. It now gates on the
  socket AND on an answer before printing, and its caddy is an `OwnedService`. The
  answering gate is honest about what it waits for: the upstream is a deliberately dead
  :9999, so the status that ends the wait is a 502, which still proves the route table
  loaded.
  **`caddy_443` was RE-CHECKED and is fine** — it starts the privileged edge through
  `proxy::start_privileged`, which shells `caddy start`, and that subcommand does not
  return until the server has started. Recorded because the audit flagged it by shape
  (a READY line after a start call) and the shape was not the fact.
- [x] **The sandbox leftover-sweep looked in the directory the roots left** ✓ 21 Aug
  2026. `bd9748b` moved the sandbox root to `/private/tmp` and left the self-healing sweep
  reading `std::env::temp_dir()` — the per-user TMPDIR that no longer holds any root, so
  from that day it swept a directory that could not contain a leftover. It now reads
  `root.parent()`, which cannot drift from where the roots are put.
  **Measured on this machine when the fix landed: 15 orphaned sandbox roots, 1.4 MB**,
  the oldest from the day the root moved. The failure was silent by construction — an
  empty `read_dir` looks exactly like a clean machine, which is the property to distrust
  in any self-healing sweep.
- [x] **`verify.sh` linted one target of four** ✓ 21 Aug 2026 — now
  `cargo clippy --all-targets -- -D warnings` in BOTH crates. `--lib` had covered the
  library and nothing else: not `main.rs`, not the 134 files in `examples/`, not
  `#[cfg(test)]` code, and not `cli` — the same reasoning that added the cli crate's
  tests on 12 Aug ("a gate that skips a shipped crate is not a gate"), and the examples
  are exactly where this week's audit found 22 defects. Clearing the backlog it exposed
  was ~48 fixes; four deliberate cases carry an `#[allow]` **with its reason**, which is
  this repo's rule for allows. One of them was worth more than the lint: `copy_scan`'s
  WCAG guard had a `why` field on every EXEMPT row that nothing ever read, so a failing
  run now prints the pairings it did NOT check and the reason each rests on — an
  exemption that is wrong is invisible until something makes you read it.
- [ ] **The frontend has no linter at all, and 17 `eslint-disable` comments say
  otherwise.** No eslint/biome/oxlint in `package.json`, no config file, and `verify.sh`
  runs `tsc --noEmit` for the frontend — while `src/` carries 16
  `react-hooks/exhaustive-deps` suppressions and one `no-control-regex` across 10 files.
  They are honest intent-markers ("this dep list is deliberate") and completely inert.
  **The decision is the owner's, because it buys a dependency**: adopt
  `eslint-plugin-react-hooks` so those 16 comments start meaning something (and find out
  what else the rule says about a codebase that has never run it), or delete them and
  stop implying a gate. Measured, not guessed, 21 Aug 2026 — the Rust half of this row is
  closed above.
- [x] **`PHP_DEBUG_XDEBUG_VERSION = 3.4.5` had no row in `docs/PORTS.md`** ✓ 21 Aug
  2026 — the only pinned constant in `core/binaries.rs` missing from the file that holds
  the pins, now recorded beside the Xdebug row with why it is deliberately not 3.5.3 (the
  debug build is a recipe, not a shipped artefact).
- [x] **`PHP_DEBUG_BASE_URL` still names `dl.rexenv.dev` after the B33 ruling moved the
  host to GitHub Releases.** ✓ 23 Aug 2026 — ledger #376, plant-proven twice.
  The row's own plan was "the const is the fix, and it belongs to whoever builds first,
  because repointing a URL nobody can fetch is untestable until then." That was wrong in
  a way worth keeping: the untestable part is whether the URL RESOLVES, and nobody was
  asking for that. What was testable today is everything that actually bites — the host,
  and the order the two edits land in.
  - The const is **gone** rather than corrected. php-debug URLs are built from
    `RUNTIMES_RELEASE_BASE`, the same const the 7.4 artifacts use, so a retired host
    cannot be a string that survives a ruling; only the release TAG (`PHP_DEBUG_TAG`)
    is left to fill, which genuinely cannot be known until a release exists.
  - The **two-edit gate** was the real find. The recipe's order is upload → hash → pin,
    so the four digests land first and the tag is the edit nothing fails without — and
    the resolve path read only the digests. `php_debug_spec` now refuses on an empty tag
    before it reads one.
  - The test that should have caught the original defect was a tautology:
    `starts_with(PHP_DEBUG_BASE_URL)` on a URL formatted from that same const. It asserts
    a literal now and bans `dl.rexenv.dev` by name.
  - The new guard takes both halves as PARAMETERS (`php_debug_spec_from`) because both
    consts are empty today: called through `php_debug_spec` it cannot tell the tag gate
    from the digest gate, and deleting the tag check leaves every assertion green.
- [x] **Ledger hygiene — three of them, all in the file that polices staleness** ✓
  21 Aug 2026.
  (a) The hand-curated tail said "plus 5 🚫 premises living inside ◐/✅ rows" and named
  five; there were **twelve** (#15, #40, #43, #52, #149, #154, #254, #294, #309, #343,
  #350, #365). It was the ONE clause deliberately exempted from `ledger-tally.sh` as
  "hand-curated", which is exactly why it was the part that drifted — so the script
  computes and enforces it now. A number inside a generated line that is not itself
  generated is just the next stale number.
  (b) The Tier-1/Tier-2 blast-radius tables — the work-ordering index `docs/TODO.md`
  sends an agent to work "top first" — listed four entries whose rows read ✅ (#37, #49,
  #104/#191). Struck, with each closure's evidence, and the table now says it was checked
  against the rows rather than remembered.
  (c) `scripts/ledger-tally.sh` called the ledger "a 500-line file" twice; it is 890.
  The script written to stop stale numbers carried two of its own.
- [x] **The UNCALLED allowlist shrank by one** ✓ 21 Aug 2026. `createSite` was exempted
  as "superseded by the job-based provision flow; the wrapper predates it" — a temporary
  state that had stopped being temporary, in a list whose own comment says it "is allowed
  to SHRINK, never to grow silently". The wrapper is deleted (with a note in its place
  saying where the flow went); the BACKEND `create_site` command stays, because
  `rex site create` calls it through `cli_server` — what is gone is a frontend door onto
  a flow the UI no longer uses. The guard enforces the pairing in both directions, so the
  wrapper and its exemption had to go in one commit.
- [x] **Multisite theme network-enable: backend + IPC shipped, no UI** ✓ 23 Aug 2026 —
  ledger #382. The Network tab has a "Themes (network)" card; both wrappers are called and
  their `copy_scan` exemption is DELETED rather than reworded.
  **It was not "small and well-defined", and the reason is the interesting part: the
  missing piece was a READ, not a button.** `wp theme list` has no field for
  network-enabled state — theme `status` is only `active`/`parent`/`inactive`, and
  `--status` offers the same three, with no `active-network` the way plugins have. So
  there was nothing to render a toggle's current position from, and a pair of buttons that
  cannot say what they would undo is exactly the control `docs/DESIGN.md` forbids. The set
  lives in the `allowedthemes` network option, read via `wp network meta get 1` (`wp option
  get` has no `--network`).
  Placed in NetworkPanel, not on the Themes tab as this row proposed: network enabling
  exists only for a multisite, and NetworkPanel is the one place that is structurally true
  rather than a runtime `isNetwork` check that renders for a moment on a single site.
  **A guard was proving the wrong thing, found by planting.**
  `every_ipc_wrapper_is_actually_called` matched a whole-identifier occurrence anywhere
  outside the ipc module — so a wrapper that was IMPORTED and never used counted as called,
  which is the exact end-state of deleting the one line that used something. Removing the
  new card's two calls left it green. It strips import statements now.
- [x] **`REXENV_LARAVEL_DOTENV` was read by a test and set by nothing** ✓ 21 Aug 2026 —
  corrected where it counted, which was the LEDGER. Ledger #245 credited that test as the
  thing that stops the hand-copied `.env` fixture going stale "in silence"; nothing sets
  the variable (no example runs `composer create-project` — the clone-based checks read
  the repo's `.env.example`, a different file), so it skipped on every run since it was
  written. A guard against silence that is itself silent is worth less than no guard,
  because the row was counting it. The row now says what actually holds — four fixture
  tests, against a copy that CAN go stale — and the test is documented as a MANUAL leg
  with the command to run it. Its skip is LOUD now (it prints what it wanted and how to
  give it that), and both paths were exercised: unset → the skip line, staged → the
  assertions run and pass.
  - [ ] Optional follow-up, deliberately not done: give it a producer. No example runs
    `composer create-project` because it costs a network install of a whole Laravel
    skeleton per run; the honest home for it is a SMOKE step that stages the file once,
    not a check that pays that on every tier run.
- [ ] **Three small CLI gaps, all with the backend already built.** Partly done, and the
  row's framing was off in a way worth keeping.
  - [x] **`mail.mark_read` — the unreachable arm** ✓ 23 Aug 2026, ledger #385.
    `rex mail mark-read` sends it, and the fix that matters is the GUARD:
    `every_command_this_server_answers_is_reachable_from_the_cli` fails on the next one.
    This was found by a reconcile reading the dispatch table by eye, which is not a
    mechanism — the frontend has had `every_ipc_wrapper_is_actually_called` for exactly
    this defect and the CLI had nothing.
    The same scan found a **second, quieter instance the row did not know about**:
    `mail.list` takes a search term and an unread filter — the UI's own inbox search uses
    them — and the CLI sent `Null`, so both parameters were unreachable. `rex mail list
    [--unread] [query]` uses them.
    ✓ **VERIFIED live 23 Aug 2026**: unread 2 → 0, the unread filter cut 3 rows to 2, a
    query matched 1 of 3. The live run also found the header reporting the MAILBOX above a
    FILTERED list — which reads as a listing bug rather than a filter; it says "1 of 3
    messages shown" now. A filter with no label always looks like that, and only running
    it showed it.
  - [ ] **The remaining "🟢 wins whose IPC exists" are NOT unreachable arms — they are
    unbuilt commands.** `site relink`, `site retry` and `config get|set` have no dispatch
    arm at all, so each is arm + verb + formatting, not one line. `site retry` and
    `config get|set` are the owner's (the latter parked on which keys to allow-list).
    - [x] **`site relink`** ✓ 23 Aug 2026 — `site.relink` dispatch arm + `rex site relink
      <domain> <path>`, covered by the reachability guard (plant-proven: dropping the verb
      fails it by name). The CLI canonicalises the path before sending, so a relative one
      typed at a shell prompt means what the USER's cwd says rather than the app's.
      ✓ **VERIFIED live 23 Aug 2026** against an app rebuilt from `4974e5b`, on a
      throwaway linked site of its own (`/private/tmp`, two docroots, deleted after) so no
      real site was touched. The proof is what is SERVED, not what is recorded: the site
      answered `FIRST-DOCROOT`, the relink moved it, and it then answered `SECOND-DOCROOT`
      — so the config really was regenerated and the edge really did reload. The old
      docroot was byte-identical afterwards, which is the non-destructive claim. A relative
      path resolved against the CLI's cwd (`./first` from `/private/tmp/…`), and a missing
      path was refused before anything was sent.
      An identity relink is refused by core with "the site already points at that folder".
      **The first attempt, against a stale app, paid for itself**: it exposed that the
      version-skew check shipped an hour earlier could not see a stale DEV build (same
      version, different commit) — ledger #386, fixed and live-verified in both directions.
      **And deleting the throwaway found one more** (#387): `site delete` said "database +
      files removed" for a LINKED site whose folder it correctly had not touched. The
      confirm prompt already said the right thing, and `--yes` skips the prompt.
    - [ ] `site retry` — the owner's.
    - [ ] `config get|set` — the owner's, parked on the key allow-list.
  - [x] **Protocol-version handshake** 🟡 ✓ 23 Aug 2026 — **answered differently**, ledger
    #386, plant-proven both ways.
    A protocol integer answers "is the wire contract compatible", which is not the question:
    `rex` and the app ship in the SAME cask at the same version, so a difference is never a
    negotiation — it is a stale build, and naming which side is stale is the fix. And a
    protocol number would have stayed SILENT for the case that keeps happening, because
    adding a command is backward-compatible and would never bump it.
    The hedge was never necessary either: a typo is rejected by `rex`'s own match
    client-side and never reaches the app, so an `unknown command` reply ALWAYS means the
    builds disagree. `rex` now prints both versions and what to do; `rex version` gained the
    same line, since it already showed both numbers and never said they differed.
    ◐ Never exercised against a LIVE mismatch — that needs two builds and a running app.
- [x] **`docs/TESTING.md` §3.3's layout-fixture matrix was designed and never built** ✓
  21 Aug 2026 — `core::layouts`, three real on-disk skeletons (stock, Bedrock,
  subdir-docroot), with `wordpress::core_root` / `core_path_arg` and
  `logs::wp_debug_log_status` parameterized over them. Plant-proven: reverting `core_root`
  to its pre-#294 body fails the matrix and names `bedrock`.
  **Its canary fired on the first run, against the fixture itself** — every row held a
  cleanup guard over the same root and `for l in layouts(..)` consumes the vec, so
  dropping row 1 deleted the tree row 2 was about to read. That surfaced as "core_root
  pointed at the wrong directory", i.e. it would have been read as a bug in the code under
  test. One guard for the whole matrix now, and the incident is in the module doc.
- [x] **One stdout surface stays uncovered and only ARCHITECTURE says so** ✓ 23 Aug 2026
  — ledger #380, plant-proven both ways.
  **Not closed by recovering from it, which would be the wrong fix.** `json_from_wp`
  already refuses to skip to the first brace, because guessing which one starts the
  answer returns plausible wrong data instead of an error — worse than a refusal. That
  stands.
  What was actually wrong is that the failure was ANONYMOUS. All three doors produce the
  same `bad JSON: expected value at line 1 column 1`, which sends the reader at rexenv,
  or at WordPress, or at their database — and all three arrived as the same report, "the
  WordPress tab is dead on this site". So the guess now drives the MESSAGE and never the
  data: if a valid value parses from a later offset that is proof of a prefix, and the
  error reports its length, quotes it (bounded, control characters flattened so a plugin
  cannot inject newlines into a log), names the likely cause and says why rexenv will not
  skip past.
  **The no-invention half of the test was vacuous on its first write** — the garbage
  fixture had no `[`/`{`, so it never entered the branch it was policing and stayed green
  under the plant. Caught by planting, not by reading. The fixture carries a brace now.
  Still uncovered, and now stated in the ledger rather than only in ARCHITECTURE: output
  interleaved INSIDE the value. Nothing parses from any offset there, so it keeps the
  plain parse error — correct, and still undiagnosable.

### The network tier, run 21 Aug 2026 — 14 failures, three causes

First network-tier run in a long while (`live-checks.sh network`, 42 examples, stack
down, internet up). It failed 14, and every one is explained. Kept as a row because the
CAUSES are the point: not one of them was a product defect, and not one of them was
visible before the verdict contract landed this morning.

- [x] **Twelve were one leak, cascading — and there were TWO leakers** ✓ fixed with a
  shared guard, `common::engines_as_found`, after the first re-run proved a one-file fix
  was the wrong shape: 14 failures became 9, `mysqld` was leaked again, and the second
  time it was `site_provision_check`. Both examples drive the app's own provisioning,
  both ADOPT the engine it starts, both printed ALL PASS, both left it running. The guard
  records which engines are up before the run and stops only the ones that run started —
  a TRANSITION, never a state, because stopping an engine the developer already had up is
  the identical defect pointing the other way. Ownership is checked before signalling
  (`owned_master` against our app-data marker), so a developer's own MySQL on the same
  port is never a candidate. Proven: run alone, it prints `stopped the Mysql this run
  started (it was down before)` and leaves :13306 free.
  Original single-instance finding: `git_site_provision_check` drives the
  REAL provision flow, which starts MySQL through the app's own path; the example adopts
  the engine (`adopt_dbs`) and adoption is deliberately not ownership, so no `Drop`
  reaches it. It printed ALL PASS and exited leaving `mysqld` on :13306 against the REAL
  datadir. Every later MySQL-needing example then failed naming the PORT rather than the
  cause — `start_all failed: port 13306 is still held by a leftover rexenv process`.
  **One green run, twelve red ones, and the green one was the culprit.** It now reads
  whether MySQL was up BEFORE and stops only an engine this run started; stopping one the
  developer had running would be the same defect pointing the other way.
  **This is also the first time that cascade could be SEEN.** Before today's verdict-
  contract fix those twelve examples printed `start_all failed:` and exited 0, and the
  tier said `all green`.
  **And the lesson about the fix, not the bug:** patching the one example the evidence
  named left the class open, and the re-run found the second instance in 35 minutes. Two
  instances of one shape is a helper in `common`, not two patches — the same call the
  gate sweep made this morning and the same one this row nearly repeated.
- [x] **One was a fixture that did not look like production** ✓ fixed.
  `repo_run_all_check` died on `no binary manifest for php 8.3.32` — the patch this
  machine's 8.3 runs after an in-app update. `binaries::resolve` consults the compiled-in
  pins first and the VERIFIED CATALOG second, and the app rehydrates that catalog from the
  database at launch (`updates::install_cached`) precisely so an updated patch resolves
  offline. The examples never did, so they only knew 8.3.31. Four examples that open the
  real app database and drive real commands now do what launch does
  (`repo_run_all_check`, `cli_repo_check`, `cli_socket_check`, `mcp_socket_check`); it
  passes ALL PASS. The shape worth remembering: this broke ONLY on a machine that had used
  a shipped feature — the maintainer's — which is the worst place for a check to be wrong.
- [x] **One was a genuine transient, and is now measured as one** ✓ `wporg_icons_check`
  failed on `403 Forbidden` fetching a derived icon from ps.w.org; the same URL returned
  200 seconds later, with and without a User-Agent. It is a CDN throttling a burst the
  check itself created. The assertion retries three times before failing and says how many
  attempts it made, so a dead link still fails and a throttle no longer does. Logged
  against the live-check transients row as the fourth captured instance — this one with a
  named mechanism rather than a shrug.
- [x] **Re-ran the tier twice, and each run found the next layer down** ✓ 21 Aug 2026.
  **14 → 9 → 3**, and the last three were not the cascade at all:
  - [x] **`wp_tools_check` poisoned its own next run.** It `search-replace`s the fixture
    site's rows (`wptools.test` → `changed.test`) and never dropped that database, so run
    N+1 opened a site whose siteurl was ALREADY `https://changed.test`, found nothing to
    replace, and failed on "dry-run found no rows to change" — after which it could never
    pass again without someone dropping the database by hand. Fixed the way its sibling
    already had been: drop the fixture database first (with the name assertion that keeps
    a future domain rename from dropping something a person owns), pin the docroot, and
    own the mysqld. **Proven by running it twice in a row, green both times, nothing left
    on :13306.**
  - [x] **`wp_themes_check` had HALF a fixture.** It dropped its database per run (fixed
    19 Aug for exactly this symptom) and kept its DOCROOT, so a previous run's
    `wp-content` survived into a fresh install and the freshly installed theme came back
    ACTIVE where the assertion demands inactive. Pinned; green twice in a row.
  - [x] **`tunnel_check` was the environment, and now says so.** 30 attempts of
    `error sending request`, and measured while the URL was still live: the system
    resolver returned NOTHING for the host (`curl` reported `dns=0.000000s` — an instant
    negative-cache hit) while `dig @1.1.1.1` answered `104.16.231.132`. The tunnel was up
    the whole time; this machine's network could not see it. That is the router race this
    file already lists under Known baselines — what was missing was a RUN that says so
    instead of asserting "public URL did not serve the local site", which names the wrong
    subject. It now prints the differential and the two-command confirmation, and still
    exits non-zero, because a run that proved nothing is not green.
  **Still owed: one clean end-to-end tier run.** Everything above is fixed and each was
  re-run standalone (twice, where re-runnability was the bug), but no single `network`
  run has been green from start to finish yet.

### Promoted out of ticked rows (21 Aug 2026)

Open work that was living inside `[x]` blocks. It is here because the archive is not a
place to keep unfinished things.

- [x] **The WCAG token sweep has no L2 render check** ✓ 21 Aug 2026 —
  `scripts/wk-checks/contrast.js`, in `run-all`. It measures the contrast a user SEES:
  computed colour over the nearest ancestor that actually paints, alpha composited, with
  AA's size threshold read off the rendered font (4.5:1, or 3:1 only at 24px / 18.66px
  bold). 822 text elements across six routes × both themes. Icons are skipped
  STRUCTURALLY (an element with no text node of its own), which is the lesson from the
  sweep whose name-keyed exemptions a plant walked straight through.
  **It found six failing pairs on its first run, and every one is a pair L0 cannot see** —
  `every_text_on_surface_pairing_meets_wcag_aa` computes text tokens against SURFACE
  tokens, and none of these is text-on-surface:
  - [ ] **`--rex-placeholder` on the Sites list column headers — 2.11:1 light, 2.61:1
    dark.** The worst on screen by a distance, and the likeliest to be a mistake rather
    than a trade: the token is documented as "unbuilt-screen placeholder icon/label" and
    is doing duty as real UI text. The spans carry no colour class of their own, so the
    colour is inherited — invisible to any class-pair scan.
  - [ ] **White on `--rex-brand` — 4.35:1.** The PRIMARY button (New site, Magic Login,
    Add blueprint) and the mail count badge; plus brand-as-text on surface-2 at 4.12:1.
    Fixing it moves the brand colour, which is a design decision and explicitly not a
    check's to make.
  - [ ] **`--rex-accent-blue` 4.37:1 and `--rex-accent-red` 4.48:1 on the letter TILES.**
    Both tokens were darkened on 16 Aug for exactly this reason — but computed against
    surface-3, and a tile is a tinted background nobody computed.
  Recorded as a ratchet rather than fixed, the same lifecycle the 16 Aug sweep used: the
  list may only SHRINK, and a recorded pair that starts passing FAILS the check so the
  repayment cannot go unnoticed. Plant-proven both ways — a new low-contrast pair fails,
  and a recorded pair that no longer renders fails.
- [ ] **An override site has never served through a REAL tunnel end to end.**
  `tunnels::origin_port` resolves the recorded override port and the L0 proof is
  plant-proven; SMOKE §Public sharing gained the step, and a network-tier leg would need
  a FrankenPHP fixture on `tunnel_exposure_check`.
- [x] **No wk-check asserts the FrankenPHP PHP picker is disabled** ✓ 21 Aug 2026 —
  `scripts/wk-checks/phppicker.js`, in `run-all`. Asserts the DISABLED attribute, that the
  option names FrankenPHP's embedded build, that it does NOT show the stored 8.1 (which is
  the promise the row was filed about), and both halves of the sentence that gives the
  user their choice back. **Control included**: an nginx site's picker must still be
  ENABLED, because a page where every select happened to be disabled would satisfy all of
  the above. Plant-proven three ways — remove `disabled`, soften the copy, and the control
  itself.
  **And it found a crash on its way in.** `SiteDetail` called `useQuery` for
  `repo-site-info` BELOW its `if (!site)` early return, so a COLD render of the route ran
  fewer hooks than the render after `sites` resolved and React threw "Rendered more hooks
  than during the previous render" — a blank screen instead of a site page. Navigating
  from the Sites list hides it (the query is cached, `site` is found on the first render);
  a reload or a deep link straight to `/sites/:id` does not. Hoisted above the return with
  `enabled: !!site`. Nothing in `verify.sh` could have caught it: `tsc` type-checks and
  the hooks rule is a RUNTIME contract, which is the argument for L2 in one line.
- [x] **`validate_linked_docroot` does a per-call `list(conn)`** ✓ 23 Aug 2026 —
  **closed as WON'T DO, and the row was the bug.** Hoisting it would have broken the
  Valet import: the apply loop creates sites one at a time through that validation, so
  two scanned projects where one nests inside the other are refused only because the
  second call reads the row the first one wrote. A pre-batch snapshot cannot contain it
  and both would be created — two sites serving one tree, one under the other's domain.
  `enrich` already holds a hoisted `existing` slice two lines from the call, so this was
  a one-line change that looked free.
  **A snapshot is what a hoist IS**, which makes "cache this read" and "turn this
  lifetime guard into a one-time check" the same edit in two vocabularies — and only one
  of them sounds dangerous. Recorded as a second form of the §3.2 class in
  `docs/TESTING.md`, because the disguise is the durable part.
  The per-call read is now documented as load-bearing at the call site and held by
  `each_link_validation_sees_the_site_the_previous_one_created` (ledger #379),
  plant-proven by making the row's own suggestion. The pre-existing overlap test could
  not have caught it: it calls the function once, so it passes identically whether the
  read is fresh or cached.
  The cost was never there either — one SELECT per candidate, beside a `detect_project`
  doing strictly more filesystem I/O in the same function.
- [ ] **Plugin-update progress: the TIMING half** (ledger #249) — **the row's own premise
  was wrong and is corrected here, 24 Aug 2026.** It said "the wiring half landed, this did
  not", which reads as the CODE being missing. It is not: `settleAfterUpdate` has cancelled
  before settling since 9 Aug (`c489b92`), and the same commit added `verdict()`/
  `isNewerVersion`. The row was written on 13 Aug — four days AFTER the fix — and its
  original wording said what it actually meant: *"needs a real site"*. The 21 Aug reconcile
  flattened a proof gap into an implementation gap. (Same shape as the PHP 7.4 "recorded
  nowhere" premise, and the second time a reconcile has done this.)
  **What is genuinely unproven is narrower than the row implied**, because two mechanisms
  cover this and each catches what the other cannot:
  - `verdict()` makes a stale VERSIONED claim unrenderable — a late check offering `3.2.1`
    over a row already at `3.2.1` renders as no update. That half IS tested and
    plant-proven (`wk-checks/wpverdict.js`, ledger #250).
  - `cancelQueries` covers the one claim `verdict()` deliberately lets through: `available`
    with an EMPTY target, which cannot be ordered and so shows a badge with no arrow. A
    late in-flight check of that shape is the only way the badge can still come back.
  - [ ] So the remaining proof is exactly that: a late check landing AFTER a settle with an
    empty `updateVersion` must not restore the badge. Not reachable from `wpverdict.js`
    (it renders fixture rows; this is react-query ordering), so it is a live observation on
    the #249 wiring run, or an L2 case that can script a delayed query resolution.
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
  **15 Aug 2026 — a recurrence now captures itself.** The original sighting produced no
  evidence because the assert printed only "not deactivated" and the panic then leaked
  mysqld into the next run. The example now dumps deactivate's own stdout, the parsed
  list, and a raw `wp plugin list` re-read (the raw read separates "parsed list stale"
  from "really still active") before panicking, and mysqld is Drop-owned
  (`common::OwnedService`) so the panic cannot manufacture the corpse-mysqld condition
  the first sighting was tangled with. Nothing new was ruled in or out — still filed
  as unexplained.
- [ ] **Provisioning examples that do not PIN `sites_dir` write into the user's REAL
  Sites folder — and one of them deletes there.** `sites::provision` reads the `sites_dir`
  SETTING, which falls back to `~/rexenv/Sites`: a path derived from the home directory,
  not from `Paths`, so a sandboxed `Platform` cannot redirect it. The door that closes it
  is `common::sandbox_db` (pin included, cannot be used without it) or
  `common::pin_sites_dir` for examples that open their own database; 12 examples call one
  of them.
  **Re-measured 21 Aug 2026 — the row used to say "snapshot the folder before a bulk run",
  which is a habit, not a control.** The specific live instance found by the audit:
  `adminer_deeplink_check` opens a bare `db::open` on a temp path, never pins, provisions
  `dbsite.test`, and then calls `remove_dir_all` on the resulting docroot — a path inside
  the user's real `~/rexenv/Sites`. That is the same shape as the incident this project
  already paid for (an example `rm -rf`'d `docroot.parent()` and took the whole Sites
  folder). It is tracked as a sub-item of the readiness-gate audit row above; this row
  stays open for the CLASS.
  - [x] **The sweep is DONE — 23 examples pinned** ✓ 21 Aug 2026. Seven that DELETE
    (first pass), two the tier convicted (`wp_tools_check`, `wp_themes_check`), and the
    remaining fourteen that only WRITE. The argument the deferral was missing arrived from
    the tier: an unpinned docroot is not untidy, it SURVIVES into the next run and makes a
    check fail on its own leftovers — which is what both of that day's fixture bugs were.
  - [x] **And the sweep found a trap one command before someone fell into it** ✓
    `seed_and_list` opens the REAL app database (`db::open_for_platform`), so pinning
    `sites_dir` there would have rewritten the USER'S setting and repointed their Sites
    folder — every site they own reading as missing. It is excluded, and the helper now
    REFUSES: `pin_fixture_sites_dir` and `pin_sites_dir` compare `Connection::path()`
    against the real app database and exit with what to open instead. The difference
    between the safe call and the catastrophic one is one earlier line choosing
    `db::open(temp)` over `db::open_for_platform(real)`, which is not a difference review
    reliably sees — so it is a fact the code checks, not a convention.
    *(The list below is STALE — kept as written for the record; all of these were pinned by
    `2e5b737`. See the ticked sub-item above.)*
    The remaining unpinned provisioners — `adminer_serve_check`, `health_watchdog_check`,
    `log_tail_check`, `mail_adopt_settings_check`, `mail_route_check`,
    `monitor_coverage_demo`, `seed_and_list`, `server_switch_serve`,
    `service_manager_demo`, `terminal_site_check`, `tunnel_check`, `wp_info_check`,
    `wp_login_check`, `wp_plugins_check`, `wp_premium_update_check`, `wp_themes_check`,
    `wp_tools_check` — still add directories to the user's real `~/rexenv/Sites` on every
    run. One line each (`common::pin_fixture_sites_dir`), left separate because pinning a
    docroot moves what a running stack serves and each one deserves a look rather than a
    sweep. (`sites_folder_check` is correctly excluded: pointing `sites_dir` somewhere
    else IS its subject.)
  - [x] **Made impossible rather than reviewed** ✓ 24 Aug 2026 — ledger #392.
    `sites::provision`'s CREATE branch refuses when the platform's app-data is outside the
    home directory (a sandbox) while `sites_dir` is still the home-derived default. The
    signal cannot fire on a real install, and the test asserts that rather than assuming it.
    Live-proven by stripping the pin from `git_site_clone_check`: it fails naming both paths
    and the fix. **Two earlier plant attempts passed and were right to** — `linked_site_check`
    and `retry_recovery_check` supply a docroot PATH, never reach the create branch, and never
    touched the user's folder. The guard sits in the one place a docroot is created.
  - [x] **The "remaining unpinned provisioners" list was STALE** ✓ 24 Aug 2026. All 17 named
    here were pinned by `2e5b737` ("pin the last 14 fixture docroots"); only `seed_and_list`
    is unpinned, correctly, because it opens the REAL app database and the helper refuses
    there. Verified empirically as well as by grep: today's sandbox, service and stack tier
    runs added **zero** entries to `~/rexenv/Sites`.
  - [x] **437 MB of historical leftovers removed** ✓ 24 Aug 2026, on the owner's say-so —
    18 orphaned directories in `~/rexenv/Sites`, five of them whole WordPress installs. They
    predated the pinning sweep and cannot regrow now that `provision` refuses (#392).
    Verified unreferenced against EVERY path-shaped column in every table, not just
    `sites.path`, then deleted one exact path at a time behind two guards — a plain
    directory (never a symlink) that is a DIRECT child of the Sites folder — because the
    incident this project already paid for was an `rm -rf` of a derived parent. An inventory
    is at `~/rexenv/orphans-removed-2026-08-24.txt`. The six remaining directories all have
    site rows; all 17 sites still serve.

- [ ] ❓ **Does `tunnels::stop` routinely need SIGTERM?** Observed once, 14 Aug 2026, on
  the first run of `common::adopt_public_tunnel`: cloudflared was still alive 3s after
  `tunnels::stop`, and the guard's escalation stopped it. **One sighting is not a
  finding** — it may be a defect in the stop path or simply a graceful shutdown slower
  than a 3s window, and convicting `tunnels::stop` on a single observation would be the
  attribution-by-elimination move this file keeps refusing. What makes it worth queuing:
  if it IS routine, production's own stop path has the same gap and nothing there
  escalates. Not urgent — the app's exit hook and the crash sweep both cover a survivor —
  so the plan is to watch the guard's output over the next few tunnel runs and open it
  properly only if it recurs.
  **15 Aug 2026 — the watch now has a record to accumulate into.** Checked for
  accumulated evidence first: there was none and there COULD have been none — a
  successful `live-checks.sh` run deletes its log directory, so the guard's stderr only
  persisted if a human was watching (the one kept failure dir from 14 Aug has no
  escalation lines). Two changes in `common::reap_public_tunnel`: every reap now appends
  an outcome line (`stop` / `stop_slow_no_signal` / `stop_no_effect_10s` / `sigterm` /
  `sigkill` / `survived_all`, with elapsed ms) to
  `src-tauri/target/tunnel-stop-evidence.log`, fast path included so the base rate
  accumulates; and the anomalous path watches 3s→10s BEFORE signalling, because a death
  right after SIGTERM is indistinguishable from the earlier stop still finishing — the
  immediate escalation was destroying exactly the evidence this question needs.
  **And the recorder's first run impeached the sighting's own instrument**: it logged
  `tunnel_guard_check`'s sleep stand-in as SURVIVING SIGKILL, which is impossible — the
  guard's `pid_alive` was `kill -0`, which answers "alive" for a macOS ZOMBIE, and the
  tunnel pid is the example's own child that nothing `wait()`s, so every dead cloudflared
  is a zombie until the example exits. `pid_alive` now reads `ps -o state=` and counts
  `Z` as dead (production's `process_running` semantics; it already did this, so
  `tunnels::stop` itself was never fooled). The 14 Aug sighting sits on the broken
  measurement and is DOWNGRADED to suspect — not explained away: its "stopped after
  SIGTERM (verified dead)" step doesn't fit a pure-zombie story, since a zombie ignores
  SIGTERM too. Watch the evidence file; convict on clean measurements only.
  **First REAL-cloudflared datapoints, 15 Aug 2026** (`tunnel_delete_order_check`):
  production's `stop_for_domain` killed a live registered cloudflared INSIDE a 527ms
  delete, and the guard's own `tunnels::stop` killed a deliberately-leaked live one
  in **2ms** (evidence line `pid=39638 outcome=stop elapsed_ms=2`). Two clean
  measurements, both instant — the weight now leans hard toward the 14 Aug sighting
  having been the zombie artifact.

  **Re-read 21 Aug 2026 — the log now has 48 entries and every one says `outcome=stop`,
  and that number answers a DIFFERENT question than the one this row asks.** All 48 come
  from `tunnel_guard_check` (45) and `tunnel_delete_order_check` (3), and both spawn a
  `sleep 300` STAND-IN rather than cloudflared (`tunnel_guard_check.rs:52`). So what the
  base rate proves is that the GUARD stops a process that has no shutdown work to do —
  useful for the guard, silent about `tunnels::stop` against a real cloudflared, which is
  the only subject the row cares about. The instrument is recording the wrong subject, and
  a clean 48/48 reads exactly like the answer while being none of it.
  **What would settle it:** entries from `tunnel_exposure_check` / the network tier, where
  the pid IS cloudflared. Until one of those runs appends a line, this row has no evidence
  at all — which is a better description of its state than "watching".
- [ ] Then: ~~Apache/FrankenPHP dotfile legs (#103)~~ (closed 15 Aug 2026 — all three backends live, plant-proven per template), ~~fpm candidate
  isolation (#104/#191)~~ (closed 15 Aug 2026, `fpm_candidate_check`, plant-proven), ~~manifest HEAD+digest sweep~~ (closed 15 Aug 2026, `manifest_sweep_check` #335 — 88 URLs answer, 78 re-hashed incl. every Intel digest), Bedrock live provision (#35),
  sandbox-adoption cohorts + `wp_fixture()` — incl. scoping
  ~~`download_progress_check`'s bin-cache delete off the REAL shared cache~~
  (closed 24 Aug 2026 — **not by sandboxing it, which would have removed its
  subject**: the shared binary cache is a deliberate exception and a private
  bin dir would mean re-downloading everything per run. The entry is RENAMED
  aside now and restored by `Drop` unless the run replaced it, so a failed
  download — offline, or a registry outage like the 21 Aug GitHub 504 — leaves
  the cache as it found it. Plant-proven: a panic mid-run puts the phar back and
  leaves no stash. The old argument, "safe because every entry is re-fetchable by
  checksum", was true only while the network is),
  ~~`webview_dialogs` L2 (#166)~~ (closed 15 Aug 2026 — the L2 shape was measured
  impossible before building, which was the queued instruction; legs landed at
  L0+L1, eye-half in SMOKE; `docs/PLAN-webview-dialog-proofs.md`),
  ~~import-graph lint (#163)~~ (closed 15 Aug 2026, plant-proven), ~~rusqlite-outside-state
  guard (#167)~~ (closed 15 Aug 2026 as the SQL-string scan the defect-families
  note demanded; its first run found the claim already false — feed.rs owns
  `agent_actions` SQL — and a live violation in `commands/mcp.rs`, both handled).

- [ ] **Live-check transients — a known-unknown, written down so the third one
  isn't a third undocumented data point.** Two unexplained failures on 3 Aug
  2026, both during full-suite runs, both passing standalone immediately after
  and on a clean re-run of the whole tier:
  - one lib test during `verify.sh` (name NOT captured — it passed before it
    could be identified; 4 clean runs after);
  - `apache_site_check` during `verify-full.sh` (passed standalone, then the
    whole sandbox tier passed at exit 0, then a captured full re-run was green).
  What is known: both under CPU contention (a vite dev server and/or WebKit in
  flight), both in checks that bind fixture ports or spawn services, neither
  reproducible in isolation. What is NOT known: whether it is port contention, a
  timing assumption, or something in the runner. **Mitigation already landed** —
  `live-checks.sh` now tees every example to a log and REPLAYS the failing one's
  last 40 lines next to the verdict, keeping the directory on failure, so the
  reason survives to the tail and a re-run no longer destroys the evidence
  (plant-proven). The remaining work is to diagnose the third occurrence when it
  is captured, not to guess now.
  **15 Aug 2026 — a third occurrence WAS captured, with a name and a mechanism.**
  `core::dns::tests::port_bound_true_when_held_false_when_free` failed a full
  `verify.sh` run ("a released UDP port should read as free") and passed standalone —
  and reading it explains itself: it dropped an ephemeral UDP socket and asserted the
  port reads free in a single shot, but nothing stops a parallel test or any process
  on the machine re-binding that exact port in the gap. Fixed by retrying across
  fresh sockets (the subject is `port_bound`'s answer for a known state, not this
  process's ability to reserve a port against the OS). Whether the un-named 3 Aug
  lib-test transient was this same test is NOT claimable — its name was never
  captured — but the shape fits, and this instance is closed.
  **20 Aug 2026 — `apache_site_check`'s transient is EXPLAINED and closed**, on the
  second capture, by the output the mitigation above preserved. The failing line was
  `php-via-fpm=false · fallback-routing=false · css-mime=true · htaccess-302=true`: the
  two legs needing PHP failed and the two that are pure Apache passed, so Apache was
  fine and there was no pool behind it. The example spawned php-fpm and went straight to
  `apache::start` with NO readiness wait for `:9799` — Apache binds in milliseconds and
  satisfies its own loop at once, while a cold php-fpm under contention has not bound
  yet. Fixed by waiting for the pool, and — the half that actually cost the two
  investigations — by FAILING THERE, naming the pool and printing php-fpm's own log
  (which the example was discarding to `/dev/null`), instead of letting a missing
  precondition surface as `php-via-fpm=false`, which reads as "Apache cannot execute
  PHP" and points at the wrong subject. Plant-proven: watching the wrong port fails at
  the precondition with the pool's log attached.
  **The same run found a second, unrelated defect that only a stopped stack reveals.**
  `linked_site_check` asserted its post-move control as `404`, but that status belongs to
  a process it does not own: the vhost is `try_files $uri $uri/ /index.php` over
  `fastcgi_pass 127.0.0.1:9783`, so a missing file falls through to the shared 8.3 POOL —
  the user's real one. With their stack up that pool answers "no input file" (404); with
  it stopped nothing answers (502). A sandbox-tier check that passes only while the
  machine's stack is running is resting on exactly what the tier is supposed to be
  independent of. It now asserts the claim it always meant — the marker is NOT SERVED
  (status ≠ 200 AND the marker absent) — which holds either way.

## Release gates (human, scripted — see the docs named)

- [ ] **0.3.0 SHIPPED on 20 Aug 2026 and NOTHING in this repo records it.** The tap has
  it (`Casks/rexenv.rb` = 0.3.0 / `381952fa…`, bumped by CI at 13:31Z), the GitHub
  release is published with both assets, and `package.json` / `tauri.conf.json` /
  `Cargo.toml` all say 0.3.0 — but there is no release row here, no §A0/§A verdict
  against that dmg, and `docs/PUBLISH-TESTING.md` still heads §A "✅ 0.2.0 — PUBLISHED"
  and certifies `bd019d8d…`. **0.2.0 has a full row precisely because a shipped artefact
  needs its commit, its hash and its §A verdict tied together**; for 0.3.0 nobody can now
  tell whether §A ran on `381952fa…` or was skipped. Two things to close it:
  - [ ] Record the release the way 0.2.0's row does — dmg sha, source commit, and the
    verify-full → §A0 → §A → draft → publish → cask-bump chain that actually happened.
  - [ ] Run §A on the SHIPPED 0.3.0 dmg (quarantined → Gatekeeper → launch), or state in
    the row that it was not run and why. An unrecorded gate is indistinguishable from a
    skipped one six weeks later, which is the whole reason the section exists.
- [ ] **The v0.3.0 TAG does not point at the 0.3.0 release commit.** `v0.3.0` → `bd0648c`
  (20 Aug 18:19); the version bump is `5cb295e` (17 Aug 19:38). Fifteen commits of later
  work — the Adminer-updates family, the release-key/logging fixes, the WP install-card
  work and two thirds of the readiness-gate sweep — are inside a tag whose message
  describes only what shipped as of 17 Aug. Decide which is true (re-tag, or amend the
  tag's message to say what it really contains) and write the rule down in
  `docs/RELEASING.md`, because the next release will do the same thing by default.
- [ ] ⚠ **The shipped cask lets macOS 11–14 install an app that needs macOS 15.**
  `Casks/rexenv.rb` carries `depends_on macos: :big_sur # minimumSystemVersion 11.0`
  while `tauri.conf.json` has said `"minimumSystemVersion": "15.0"` since the floor sweep
  (nginx 1.30.3 and cloudflared 2026.6.1 are both `minos 15.0` — `docs/PORTS.md`). So a
  macOS 11–14 user runs `brew install --cask rexenv` today, gets no refusal, and lands on
  an app whose web server binary cannot start. The floor row further down predicted this
  failure ("invisible until someone finds their web server will not start"); this is the
  install path where it is live. One line in `rexenv/homebrew-tap`, and the comment beside
  it is the reason it drifted — it pins a NUMBER that the app is free to change.
- [x] **`docs/PUBLISH-TESTING.md` was stale in the three places a release-day reader
  uses** ✓ 21 Aug 2026. (a) §A now carries a warning that 0.3.0 shipped and is not
  recorded, and says why that matters — an unrecorded gate is indistinguishable from a
  skipped one; (b) the publish-blocking summary omitted §F and §G, both 🚧 in the body,
  so a release could be cleared from that table without either being seen — the
  guard-covers-claimed-surface shape, in the file that gates shipping. Both added, plus
  §L, plus a note saying the table has to be the WHOLE set; (c) §D's trigger read "do
  once the dmg is released" four releases after that happened, and now reads `--zap`
  ONLY. `docs/RELEASING.md`'s "the flow in effect today (2026-08-12)" now says it is the
  only flow that has ever cut a release.
- [ ] **The release gates are not all in this section.** `docs/SMOKE-TEST.md` grew five
  steps FOR 0.3.0 (cold-path 7.4 licences, the PHP update button + revert, the Adminer
  update, the PHP ini revert, the "exists" row and the serving-vs-pinned line) and carries
  the MCP HOLDs, and none of them is visible from the section a release-day reader works
  from. Either list them here or make this section say plainly that SMOKE-TEST is the
  other half — silence reads as "this is the set".
- [ ] **PUBLISH-TESTING §B** — uninstall removes the root :443 daemon (live launchd).
- [ ] **PUBLISH-TESTING §D** — `--zap` ONLY; everything else has now run four times.
  **Re-scoped 21 Aug 2026**: the row below pins the v0.1.0 cask hash, but the cask has
  bumped cleanly through 0.1.1, 0.2.0 and 0.3.0 since, so the install half is not "half
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
- [ ] **Flip the release host back when `rexenv/rexenv` goes public** — three things
  in ONE commit, or the tap's guard fails the bump: the cask's `url`, its `verified:`,
  and `SOURCE_REPO` in `update-cask.yml` (all in `rexenv/homebrew-tap`). Then CI's
  `release.yml` resumes owning the build, and `docs/RELEASING.md`'s interim section
  is deleted rather than left as a second, wrong set of instructions.
- [ ] **PUBLISH-TESTING §K** — the whole migration as ONE journey (rebuild first).
- [ ] **PUBLISH-TESTING §F** — resolver takeover/hand-back/drift: clean-VM only.
- [ ] **PUBLISH-TESTING §G** — `/import` screen packaged GUI pass (only ever
  type-checked; Stage 1's status line now says so). **Grew step 9 on 8 Aug**: the
  batch progress card (`valet-import://progress`) — its arithmetic is L0-proven, but
  that `detail` really is the child job's own label and that the bar FREEZES rather
  than rolls back on a mid-batch failure are 🔨 L2 (ledger #243) and only this pass
  covers them today.
- [ ] **Plugin-update progress — the WIRING half** (ledger #249, 9 Aug 2026): the
  tracker is L0-proven against WP core's own strings and the pinned phar (plugins,
  themes and core), but that the emit reaches the WordPress tab and the bar really
  advances during a live WooCommerce/Elementor/core download is unproven at any
  layer. Needs one run against a real site (a plugin held one version back), or an
  L2 case rendering the panel with a scripted event stream.
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
- [ ] ⚠ **The macOS floor is a claim about BOTH slices and half of it has never
  been measured.** PORTS.md's `minos` table is measured from this machine's
  binary cache, which only ever downloads the host arch — so every number in it
  is an **arm64** number, and `minimumSystemVersion: 15.0` is asserted for x86_64
  on the assumption that upstream builds both slices to the same deployment
  target. Nothing checks that. **This is the arm64-DMG mistake's shape**: a
  universal artifact whose two halves differ, working perfectly on the machine
  that made it and wrong for everyone on the other chip — except the failure
  here is worse than a thin binary, because it is invisible until an Intel user
  on macOS 15 finds their web server will not start. (The 15 Aug re-measure also
  showed the table can simply be WRONG where nothing depends on it: PHP 8.0.30
  is 14.0 and had been recorded as 12.0 since the table was written.)
  **What it would take, cheapest first:**
  1. **No Intel Mac needed for the measurement** — `minos` is metadata. Fetch the
     x86_64 artifact for every pin and read `otool -l`/`vtool -show` on it. The
     download URLs are already enumerated by `manifest_sweep_check`, which walks
     BOTH arches; a `minos` column is a few lines in an example that already
     fetches these bytes, and the sweep is the natural home because it is the one
     check that already refuses to be arch-blind.
  2. Assert the *derived* rule rather than the numbers: `max(minos)` over the
     default-stack binaries, per arch, must equal `tauri.conf.json`'s
     `minimumSystemVersion`. That fails on a pin bump that raises a floor, which
     is the event the current table can only be updated by hand for.
  3. Only then does an actual Intel machine matter, and for the OTHER half — the
     run-verify above, which metadata cannot stand in for.
- [ ] **In-app verifies owed** (CLI passthroughs whose service-touching half the
  example harness guard-blocks; fold into the next deep test): `php
  install/uninstall`, `php settings set`, `db versions --set`, `site
  server/domain/move`, `mail clear`, `tunnel start`, `wp core update/switch` —
  plus the packaged-GUI walks still noted inside their shipped entries: MariaDB
  site from the dialog, Apache site in-app, DB version switch from the Databases
  row, Settings CLI-install card, ref-picker on a real many-branch repo, wp.org
  chips + streamed installs, New Site streamed provisioning card, **Upload zip
  through the real native file dialog** (SMOKE §WordPress Manager — L0 proves
  the gate, L1 the install, L2 the card; nothing can drive the picker).
- [ ] **PUBLISH-TESTING §E / §L** — 🟢 nice-to-haves (B22/B23 datadir recovery,
  B4 submodule clone, B24 wp-cli `--`, B20/B28/B29/B7 runtime wiring; §L Phase-A
  tcpdump one-off, re-run per reqwest bump).

## Decisions pending (owner)

- [ ] **`rex config get|set`** — parked on which settings keys to allow-list
  (never the whole KV table).
- [ ] Stage-3 leftovers awaiting a ruling only if they resurface: none — the
  collision-rename tell-only and pdo_mysql exclusions are SETTLED (pinned by
  tests; do not reopen).
- [ ] **MCP server — M1 + M2a + M2b are SHIPPED and code-complete (13 Aug 2026); M3 is
  the only milestone left.** Header corrected 21 Aug 2026: it had read "building M2a →
  M2b → M3" for eight days after both were done, and "D2/D5 open" when D5 (no SDK,
  hand-rolled — PLAN §9.5) was settled and D2 (`wp_login_url`, scratch-only) is the one
  open decision, non-blocking. All 8 executing tools are registered
  (`mcp_server/scratch.rs`) beside M1's three (`mcp_server/tools.rs`), and the schema
  work v27–v30 is in `state/db.rs` with the DB now at v37. **What is genuinely left is
  M3 and the human gates** — both below, and the gates were invisible from this file
  until the reconcile.
  (`docs/PLAN-mcp-server.md`): expose an MCP server so a dev's AI agent can drive
  rexenv — disposable WordPress "scratch" sites (new `origin='agent'` column,
  TTL+cap+reaper), real-site DB SELECT-only via a native driver, read-only
  diagnostics. In-process beside `cli_server`, second `0600` socket, `rex mcp`
  pipe. Security is the bulk: the honest reckoning (§3.1) is that running the
  user's plugin code is user-level power — tiers deliver a safe paved road + no
  silent amplifier, NOT containment once scratch exec is in play. D1 (ship
  `wp_run` scratch-only), D3 (real-site mutation unpromised), D4 (mail opt-in
  sub-toggle), D6 (tunnels never a tool), D7 (accept the residual, toggle off +
  ledger 🚫) all SETTLED; **D2/D5 open but non-blocking; the M3 DB surface is
  ruled at M3, not now.** ✓ ahead of the feature: the mirrored-user GRANT
  wildcard-escaping fix landed on its own (ledger #196).
  - ✓ **M1 shipped** 30 Jul 2026 — ledger #198–#203.
  - ✓ **1 Aug 2026 — plan reconciled against shipped M1 + M2 scope ruled**, with
    the honest guarantee paragraph written FIRST (§6.0) so it constrained the
    design instead of describing it: it forced S1 and forbade a "no tool starts
    a service" claim (`scratch_create_site` starts MySQL). **S1** the dev-plugin
    add is a copy-on-write CLONE + explicit sync, not a symlink — `wp_run` makes
    the symlink's write-back guard unenforceable, and argv-screening a raw runner
    is the guard-covers-a-narrower-surface family (4th instance, refused).
    **S2** `wp_plugin`/`wp_theme` dropped (subsumed by `wp_run`). **S3** M2 splits
    into M2a (the scenario) / M2b (`set_php_version` + mail). Also corrected:
    v27/v28 (v26 is spent), mail's "scratch-addressed" filter is unsupportable so
    the tell is one rexenv creates (fail-closed), reaps get a typed
    `agent_actions.actor`, and two M1 leftovers that would have narrowed shipped
    claims by omission (the sweep walks ONE registry; `target_site` is unfillable
    for a create).
  - [x] **M2a — the scratch-site scenario** — all 14 tasks are in the tree (v27–v30, the
    `ScratchSite` witness, the second registry, the reaper). Box flipped 21 Aug 2026: the
    sub-items were ticked one by one and the parent never was. The HUMAN gate is a
    separate row below and is not closed. 14 tasks in `PLAN §7.3`, one commit
    each. ✓ task 2 — **v27** (`origin` / `agent_client` / `expires_at`), ledger
    #204: origin recorded not derived + read conservatively (anything ≠ "agent"
    is the user's), `expires_at` NULL = never so a user site and a Kept scratch
    site are indistinguishable to `Site::reap_due` (the ONE expression of the
    predicate), `agent_client` capped at the write and never branched on.
    Upgrade-path test proven to fail on a `DEFAULT 'agent'`.
    ✓ task 3 — **v28** `agent_actions.actor`, ledger #205 (+#202 amended): the
    feed keeps BOTH true claims ("an agent did this" / "everything consequential
    is visible") by labelling everywhere and filtering in exactly one place —
    `recent_head`, the agent status line, so a reaper sweep can never read as
    "Working — scratch_reap". Default records a known fact; the READ fails the
    other way (unknown actor = rexenv, never the agent). UI: "rexenv · automatic"
    + a reap's unresolvable target reads "(deleted site)", not a bare UUID —
    all three L2 probes proven to fail on the pre-v28 rendering.
    ✓ task 4 — **`ActedTarget`**, ledger #206 (+#202 amended): a create can name
    the site it made, WITHOUT the result becoming a channel for agent content —
    the only setter takes a `&Site` row, and it is an out-parameter so it
    survives `?` (a provisioning failure after the insert still names the
    half-built site). A row names a site iff a row exists. Both load-bearing
    tests proven to fail on a no-op `set`.
    ✓ task 5 — **TTL touch**, ledger #207 (+#199 scope sharpened): using a
    scratch site pushes its expiry out, from the SESSION layer beside the feed
    write so M1's read-only handler boundary is untouched. A touch can only MOVE
    an expiry — never start a clock, never reach a user's site (even one with a
    stale expiry), never resurrect a deleted one; each `WHERE` clause
    mutation-proven load-bearing. Audited what the shape narrows: the plan's
    §3.1c "no writes" and `mcp_socket_check`'s "writes nothing" were already
    loose (the feed writes) and are corrected — read-only scopes the HANDLER,
    not the call.
    ✓ task 6 — **`core::scratch::ScratchSite`**, ledger #208: the origin gate as
    a witness type in core (so CLI and UI inherit it), one door (`claim`, which
    reads the row itself), all three bypasses proven to be compile errors
    (E0423 / E0451 / E0277) with the captured messages in the row; refusals are
    policy statements naming site + rule + way forward, distinct for
    not-found vs not-yours, leaking no path or db name. Documented as NOT a lock
    — destructive writes still re-assert `origin` in their `WHERE`.
    ✓ task 7 — **the two-registry split**, ledger #209: executing tools get their
    own module + context (`ScratchCtx`, whose only door to a site is the witness),
    so #199 is untouched rather than widened; dispatch decides capability by
    WHICH registry a tool came from; `tools/list` and the secret sweep both walk
    the union. Disjointness guard plant-proven, and it teaches (names the
    offender, both module paths, the rule, and the shadowing consequence) —
    `panic!`ed rather than `assert_eq!`ed so the guidance isn't Debug-escaped onto
    one line. Wrote the SOCKET-level statement (different sentence from M1's) and
    guarded the card's "Today those are read-only" copy against the registry by
    `include_str!` — plant-proven to fail the day the first executing tool lands.
    ✓ task 8a — **the agent create PATH**, ledger #210: one `Ownership` value
    decides both what is recorded (`origin`/`agent_client`/`expires_at`, at the
    insert, so a create that fails later still expires) and whether a prompt may
    happen; `ResolverPrompt` is a REQUIRED parameter so a future call site can't
    be silent about its policy; ownership is unreachable from IPC (`create_site`
    has no such field). Never-prompt driven end to end through the real
    `start()` with a recording PrivilegeManager, **with a permanent control test**
    proving the same path DOES escalate for a user create. Cap + domain refusals
    name a way forward.
    ✓ task 8b — **`scratch_create_site`**, ledger #211: the tool runs through the
    SAME provision job as the app and the CLI (runtime generic erased by a
    `SiteCreator` trait object, since a static registry can't be generic);
    stack-stopped is a SUCCESS carrying M1's verdict/resolution vocabulary; a
    half-built create NAMES the site it made (id, owner-split actions, no local
    log path) and records it in the feed; enable-moment copy rewritten to
    describe CAPABILITY, approved 2 Aug, and the #209 guard re-scoped to drift
    (its first trip is spent) with BOTH surviving directions re-proven to fire.
    ⚠ Open for the owner: eyeball the longer paragraph in the PACKAGED webview at
    both widths — L2 covers render at 900/1440, L3 is the WKWebView bug class.
    
    ✓ task 9a — **`scratch_delete_site`**, ledger #212: gated by the WITNESS (not
    the name/suffix), refuses the user's sites with the ownership statement
    rather than a not-found, re-reads `origin` before anything destructive so a
    Keep mid-flight is reported honestly, and runs the app's own full delete
    path. Reap-failure policy decided: ONE row per distinct problem (dedupe on
    outcome+reason), never per launch; successes never deduped. `reap_due`'s
    three clauses each mutation-proven load-bearing.
    ✓ task 10a — **Keep**, ledger #213: ONE atomic UPDATE for one decision
    (origin + expiry), guarded by `AND origin='agent'` so it is idempotent and
    never touches the user's own; keeping at the ceiling frees a slot
    immediately, and the cap refusal now offers it as a third way forward.
    ✓ task 10b — **the promotion choke point**, ledger #214: the RULE ("the user
    deliberately changed this site") replaced §4.3's list — it added the PHP /
    web-server / Xdebug switches and removed share; six commands call one
    function; a source guard fails the build if a command stops calling it or a
    new one never does. ✓ 10b's Keep confirm DIALOG landed with task 13 —
    the approved copy is recorded verbatim in ledger #219, not referenced.
    ◐ task 9b — **the reaper's SWEEP**, ledger #215: skip-don't-stop for shared
    sites (calling the normal delete path would break #29 unattended), a failed
    stop is a skip, per-sweep ceiling of 5 with a loud log, outcomes deduped by
    `record_reap`. Wired to launch + hourly WITH the
    approved summary (names the domains, silent on a quiet launch). Remaining ◐:
    the skip-don't-stop and delete legs need a live tunnel + real site (L1,
    task 14). ◐ task 11 — **v29 `scratch_packages`** landed (new table, born empty;
    proven that a scratch site created before it reads as "no packages added" —
    a real answer, not an unknown). ✓ the MECHANISM (ledger #216):
    header-derived kind refusing both/neither, clone_tree proven byte-identical
    in BOTH directions, stat-only fingerprint with its limit recorded. ✓ the two TOOLS
    (ledger #217): claim → blast-radius → header → clone, sync re-reads only the
    RECORDED source, destination follows the recorded content dir, and the sync
    copy never claims "unchanged". **Task 11 done.** ✓ **The `cp -c` optimisation is RETIRED, on
    measurement** (4 Aug 2026, PLAN §4.4 + the `clone_tree` doc): the premise was
    backwards. `std::fs::copy` already uses `fclonefileat` on macOS, so the
    shipped code IS copy-on-write — a 400 MB file costs 0 bytes of disk — and it
    beats `/bin/cp -c -R` on wall clock (0.61–0.75 s vs 0.81–1.33 s on a
    4802-file tree) because it spawns no process. Doing it would have been
    slower AND added an OS-specific path plus a `todo!()` on Windows/Linux where
    the portable code works. No code written; the reasoning is recorded at the
    function so the next person re-measures instead of re-assuming.
    ◐ task 12 — **`wp_run`**, ledger #218: the target comes from the WITNESS
    (`--path` derived from the claimed site's recorded docroot) and an
    agent-supplied `--path`/`--url`/`--ssh`/`--http`/`@alias` is REFUSED in
    **core** — appending ours last only WINS a race, since a later `--path` wins
    in wp-cli. Recorded in the code and the row in those words: this screens
    TARGETS (closed, small, the tier boundary itself), unlike the VERB screen S1
    rejected — and there is therefore **no subcommand denylist, as a decision**,
    because plugin activation already grants arbitrary user PHP (#197). Witness
    re-asserted after binary resolution (a first-use download is a real
    multi-minute window). A non-zero exit is a RESULT, not a tool error. Output
    runs through `tail_log`'s scrubber — one function, plant-proven on four legs,
    and the planting found that a naive `#[cfg(test)]` stripper would have
    skipped most of this file (the test module sits mid-file), so the scan now
    carries its own coverage canary. Remaining ◐: a real `wp` command against a
    real scratch site is L1, task 14.
    ✓ task 12b — **the scrubber widened, not forked** (#201 amended): `wp cli
    info` and wp's errors print rexenv's own phar / PHP binary / config paths,
    all carrying the OS username — the same leak through a fourth door. The
    known-path set is now DERIVED from the `Paths` trait (`KnownPaths`), so a
    directory added there is scrubbed without a second list hearing about it,
    and `tail_log` + `wp_run` share it by calling one function. Recorded
    judgment: the claim is **"the paths rexenv knows are removed", not "no path
    escapes"** — a plugin can print `/opt/...` or a runtime-built path, and no
    prefix list reaches that. Asserted, not just written (an `/opt` line comes
    through unchanged), and stated in both tools' notes.
    ◐ task 13 — **the Sites UI**, ledger #219: the group reads `origin` and
    nothing else, so a KEPT site and a user's hand-named `mine.scratch.rex`
    both render as ordinary rows — plant-proven at L2 (the domain shortcut
    fails BY NAME). `expiresAt` null yields no label rather than a "never" one,
    so there is no third state to invent. Keep goes through the SAME single
    write as the implied promotion; its **approved copy is now recorded
    verbatim in #219** (it had been referenced-but-not-written, and was
    unavailable to the session that had to build it). A moved package source is
    its own state, never a stale timestamp. Scratch rows yield the metrics
    column — measured (167px overflow at 1440, the column reserves 168).
    Backend added: `keep_site` + `scratch_packages` commands, `store::
    all_scratch_packages` (joined to `sites`, because v29 has no FK).
    Two HARNESS defects found by the planting and fixed here: the new probe was
    never wired (`probeFor` is a name switch; the scenario tuple's third slot is
    ACTIONS), and `runActions` swallowed unknown actions silently — it now
    throws. Remaining ◐: the live page against real scratch rows (task 14 + a
    packaged look). ⚠ Also open: 10b's Keep dialog is now landed, so that
    caveat is spent.
    ⚠ Noticed, not fixed: `scratch_packages` rows are NOT deleted with their
    site (no FK, no delete hook) — harmless orphans, since ids are UUIDs and
    the read joins, but worth a cleanup when task 14 touches deletion.
    ◐ task 14 — **L1 `mcp_scratch_check` + the SMOKE M2a gate**, ledger #220.
    The executing registry over a real socket, sandbox tier. Fixture is
    adversarial where it counts: the USER'S site is named `mine.scratch.rex`,
    so all four tools refusing it proves the gate reads `origin`. The plugin
    source lives OUTSIDE the sandbox root (app-data is a blast-radius refusal
    being tested) behind its own `Drop` guard. **What moved:** #217 ✅ (the
    wired clone path, source byte-compared incl. a binary blob) and #218 ✅
    (four target forms refused + a REAL `wp cli info` child whose output is
    scrubbed — plant-proven both ways: the labels must be present, and removing
    the scrub fails on a raw app-data path). **What did not, and why:** #215's
    delete and skip-don't-stop stay 🔨 (need a provisioned DB + a live tunnel;
    faking either asserts against the fixture, not `delete_site_owned`); #219
    stays ◐ (a socket check cannot move a rendering claim — it moves at L3 in
    the PACKAGED webview, SMOKE 6/9/10); and no wp command that BOOTS
    WordPress is exercised, which leaves wp-cli's loaded behaviour unproven,
    not any rexenv guard. SMOKE §M2a = 6 steps; **8 (the tier boundary in front
    of a human) and 11 (no admin prompt) are HOLDs**, and 8 names the
    work-around a helpful model would reach for as a failure tell.
    **M2a's code is complete; the remaining M2a work is a release-candidate
    SMOKE run and the two 🔨 legs above.**
    ✓ **Orphan `scratch_packages` rows closed** (ledger #221): the delete lives
    in `core::sites::teardown` (not `delete_site_owned`) so the CLI and UI
    inherit it, and the read's join to `sites` STAYS as the second defence —
    they fail differently, and each leg is plant-proven on its own.
    ✓ #203's last leg — **L1 `mcp_control_check`** (sandbox tier): the OFF
    switch, built to prove what SMOKE step 4 is a HOLD for rather than the easy
    version. A session that was ALREADY CONNECTED is dropped (not just new
    connects denied), and the socket FILE is unlinked (not just closed); then
    re-enable rebinds and serves. Both plant-proven against the exact bug shape.
    Fixed while proving it: `serve` unlinked a RE-DERIVED path rather than the
    one its listener bound — latent in the app, but any other caller's shutdown
    would have deleted the APP'S socket.
    **M2a + M1 are code-complete. The release path is settled at nine steps:**
    (1) ✓ guarantee re-read, (2) ✓ #203, (3) packaged eyeball of the
    enable-moment paragraph, (4) `verify-full.sh`, (5) regenerate
    `THIRD-PARTY-NOTICES.md`, (6) fresh DMG, (7) PUBLISH-TESTING §A, (8) the MCP
    gate — SMOKE 1–5 then 6–11 (steps 4, 8, 11 are HOLDs), (9) §G.
    ⚠ Between the build and the tap push: recompute the cask sha256 (the
    committed one is a marked stale placeholder) — §D.
    ✓ **All three closing items done 4 Aug 2026**, none of them blocking v0.1.0:
    the orphan `scratch_packages` rows (#221 — deleted in `teardown`, the read's
    join KEPT as an independent second defence); `cp -c` **retired on
    measurement** (the premise was backwards — `std::fs::copy` already clones on
    APFS and beats shelling out; no code written); and **argv-in-feed landed as
    v30 `args_summary`** (#222), scoped by a property rather than a preference —
    `wp_run` is the only tool whose action leaves no other record, so it is the
    only one that needed it (+ `scratch_add_package`'s folder name). A VERB,
    never a value; per-tool `summarise` so the feed never parses agent JSON;
    clamped at the single writer to ≤2 `[a-z][a-z0-9-]` tokens, which is what
    stops agent text forging a row in the audit list. Rendered dimmed and
    unlabelled — captioning it "command" would overclaim what two tokens say.
    ⚠ **These stale the c7424fa DMG** (they touch `src-tauri/src/` and `src/`).
    Next = M2b.
  - [x] **M2b — CODE-COMPLETE 13 Aug 2026**, box flipped 21 Aug (the row's last line
    already said "M2b is code-complete" and both "open by tier" legs were resolved in the
    same paragraph). Detail below.
  - [x] **M2b** — code-complete 13 Aug 2026, box flipped 21 Aug. ✓ the mechanism extraction (#223 — *reusing a
    command reuses its POLICY*: routing an agent through the app's PHP switch
    would have inherited `promote_if_scratch`, adopting the scratch site and
    freeing a cap slot); ✓ `set_php_version` (#224 — unshipped versions refused
    BY NAME from a DERIVED list, never substituted); ✓ the scratch-mail stamp
    (#225 — one `stamp_for` shared by writer and reader; also closed a gap where
    one unswept mu-plugin blocked the v25 dir cleanup for EVERY site); ✓ the
    sub-toggle (#226 — backfill at the consent moment, which eliminates
    "predates the feature" as a category; both copy halves on the must-say
    list); ✓ `mail_list`/`mail_get` (#227 — ONE predicate filters and gates,
    because Mailpit ids are global and a trusted id would read the user's
    inbox). ✓ task 7 — the L1 legs ride `mcp_scratch_check`: the PHP
    refusal by name (row untouched) + a real 8.2→8.3 switch, with the site
    asserted still the AGENT'S afterwards — #223's cap bypass checked on the
    wired path, not only in a source guard; and the mail three-state
    discrimination, with "the user's site is never stamped" made non-vacuous
    (its fixture has a real docroot, so origin is the only thing skipping it)
    and plant-proven. **M2b is code-complete.**
    ✓ **Both "open by tier" legs resolved 13 Aug 2026 — and one of them was not
    a tier problem at all.**
    - The PHP DOWNLOAD window: **NOT APPLICABLE, record corrected** (ledger #224).
      Writing the example first would have proved the download happens and read
      as proving the guard. `switch_php_version` writes the row BEFORE it
      prefetches, so a Keep during the download lands on the far side of a check
      that already ran and a write that already committed. The comment and the
      row both claimed otherwise — #228's old module doc again: true of downloads
      generally, false of this path. Accepted rather than reordered: the write
      was authorised when it happened, and moving the prefetch on a shipped path
      shared by the UI and CLI buys a window with no real consequence.
    - The mail MATCH over real messages: **PROVEN** — `mcp_mail_check`,
      **`service` tier, not `stack`** as this note said. It brings its OWN
      Mailpit under sandboxed paths, because borrowing the user's running one
      means planting messages in their real store to prove we can tell their mail
      from a scratch site's — a test that contradicts the thing it tests. Own
      Mailpit means the fixed ports, which is what `service` means. Five plants,
      including the two that matter: `ends_with(domain)` returns the near-miss,
      and NOT sending the near-miss fails as FIXTURE BROKEN rather than as a
      passing filter. It also refuses to run beside a live Mailpit — found by
      running it with the stack up, where `mail::start` cannot bind, exits, and
      `mail::running()` sees the USER'S catcher on the fixed port.
  - [ ] **M3 — database access, and it has never had a checkbox of its own.** A fully
    specified stage in `PLAN-mcp-server.md` §1058-1062 with NOTHING in the tree: the
    native-driver query path, agent principals with escaped + expiring grants, `db_query`,
    and the first T1 consent dialog. A grep for `db_query`/`agent_db_grants` across
    `src-tauri/src` finds one comment. It is the largest single piece of unbuilt work
    tracked in this file and it was living as a fragment at the end of another row.
  - [ ] **The MCP human gates have never been recorded as run, and MCP has shipped in
    four releases.** `docs/SMOKE-TEST.md` §M2a/§M2b carry 14 unticked steps and FOUR
    HOLDs — step 4 (disabling really tears the socket down), step 8 (the tier boundary in
    front of a human), step 11 (no admin prompt), step 14 (the mail scope that bounds D4's
    credential-harvest pivot) — under a heading that says "ships only if this passes".
    Either they ran and nobody wrote it down, or they did not; both are answered by
    running them once and recording the verdict, and neither is answered by this row
    staying where only a feature-tracker would find it.
    **M3** — DB, its own session (T1 consent dialog).

## Parked (deliberate — needs explicit go; don't pick up silently)

- [ ] **The live pool swap is still L3.** `php_update_check` proves the chain up
  to "a pool on the new patch answers on a FIXTURE port". Stopping the running
  master on the PRODUCTION port and reverting when it does not come back needs
  the real `ServiceManager` and a deliberately broken tree — `docs/SMOKE-TEST.md`.

- [ ] **`every_gated_setting_key_is_routed_through_its_validating_setter` does
  not cover its surface.** It iterates a hardcoded `[(DEFAULT_TLD_KEY, …),
  (SITES_DIR_KEY, …)]` and asserts only that those two are routed, so a THIRD
  gated key would sail past the generic KV command with no validation and no test
  failing. `commands/settings.rs` asserted the opposite in prose until 18 Aug 2026
  ("fails the build when a third one appears"), which is the dangerous half — a
  doc naming a guard that does not exist. The prose is corrected; the guard is
  not. Same family as ledger #344 (guard-covers-claimed-surface). Fixing it needs
  a declared registry of gated keys next to the setters, since nothing derivable
  distinguishes "has a validating setter" today.

- [ ] **Install WordPress into an empty LINKED folder** — out of Stage 0 by
  design (`docs/PLAN-linked-sites.md` decision 2): linking is adopt-only. If
  built: a deliberate site-page action, offered only when the linked folder is
  EMPTY, its own disclosure. Trap first: `configure` is only half idempotent
  (skips `wp config create` when wp-config.php exists but calls
  `create_database` unconditionally), and `phase_defs` blanket-skips WP phases
  on `docroot_managed == Some(false)` — needs an explicit opt-in flag.
- [ ] **Valet compatibility tails** (recorded in the migration research, §2):
  the `default` catch-all-site key and Laravel's `/storage/*` URI mapping are
  not handled; a Valet-named port-conflict attribution branch doesn't exist
  (Herd's does); serving one site under two domains needs multi-domain support
  (`sites.domain` is UNIQUE).
- [ ] **`rex` design-first set**: single-site restart (manager seam), web-tier
  single-service control (topology invariant), raw `wp` passthrough (security
  decision), `wp_user_delete` (no IPC exists), progress streaming over the CLI
  socket (design once, benefits every long op).
- [ ] **Rowless-orphan launch NOTICE**: the sweep runs before the webview mounts,
  so a plain event would be lost — needs a queued-notices channel; WARN log is
  the honest surface today.
- [ ] **Parent-death watcher helper** (kqueue `NOTE_EXIT`) to close the tunnel
  crash→relaunch gap entirely — revisit only if crash reports show the bounded
  gap mattering.

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
- [ ] Packaging polish: Tauri updater (keypair, endpoint, `latest.json` —
  checklist in `docs/archive/TASKS-RELEASE.md` §6.1), public distribution.

## Known baselines (not bugs)

- WP builds shipping `wp-includes/php-ai-client/**` show those files as "foreign"
  in checksum verify until wordpress.org's manifest covers them. Honest tool
  output — intentionally not filtered.
- On networks that negative-cache DNS, a fresh tunnel URL can be dead on THIS
  machine while live from a second device — the router race, not a bug
  (`docs/SMOKE-TEST.md` tunnels section).
