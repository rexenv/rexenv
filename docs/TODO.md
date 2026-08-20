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
- **One row described a guard that does not exist** — `wp_dns_check` was said to "FAIL
  LOUDLY the day a build stops using c-ares", and it does not: it prints a NOTE and
  passes green. That is the dangerous kind of staleness, because someone would have
  relied on it.
- **And the reconcile itself found new work**: a post-release audit of the readiness-gate
  sweep confirmed 22 defects *inside the fix*, including gates that cannot run and
  examples that report green having asserted nothing.

So the rule this file needs is the one CLAUDE.md already states and this file kept
paying for anyway: **the tick belongs in the commit that does the work.**

## Now — actionable code/test work

- [ ] **`frankenphp_edge_serve`: the edge it starts has never been PROVEN to answer.**
  Three defects were found and fixed inside it on 20 Aug 2026 (`021fb40`, `48e5046`) and
  are recorded in `docs/archive/SHIPPED-2026-08.md`; the fourth is the one that matters
  and it is still open. On a genuinely clean machine its own edge binds :8443 and then
  never answers HTTPS within 20s — yet it DID serve 200/200 twice earlier in the same
  session, while leaked caddies from previous runs were alive. The strong hypothesis is
  that those green runs were answered by a LEFTOVER edge, and that this example has never
  served through the one it starts.
  **It is a hypothesis, and the next step is to settle WHICH PROCESS ANSWERED** — not to
  read the edge config, which is where the last attempt lost the most time. Run it with
  the stack stopped (it is `service` tier) and, in a second shell during the answer-wait,
  `lsof -nP -iTCP:8443 -sTCP:LISTEN`: record the pid and whether it is the caddy this run
  spawned.
  **Method note worth more than the bug.** The first diagnosis was wrong: FrankenPHP's log
  showed `started 🐘` … 110ms … `SIGTERM`, which read as "something kills the backend",
  and it sent me through `recover_stale_edge`, `ensure_free` and every terminate site for
  nothing. Probing liveness at REQUEST time refuted it in one run (`listening=true` right
  before the curl); the SIGTERM was teardown. A log line near a failure is a coincidence
  until something rules the alternative out, and probing the subject at the moment of the
  symptom beats reading code that might be innocent.

- [ ] **The readiness-gate sweep's OWN defects — 22 confirmed, 21 Aug 2026.** The sweep
  itself is done (`be3c88d` + `2f564bb`, archived); this row is what an adversarial audit
  of it found afterwards, one verifier per finding, 44 further claims refuted. They fall
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
  - [ ] Decide which: a real gate, or an honest note. Not both.
- [ ] **Debug-log truth on Bedrock** (deferred with the wp-config-reader work):
  parse `config/application.php` env defines so WP_DEBUG/WP_DEBUG_LOG read
  truthfully on non-stock layouts; today's honest state is `indeterminate`
  ("can't determine", `core/logs.rs:208-252`).
- [ ] **WP Manager cron list: arguments display** — placeholder for QA's exact
  complaint (likely the event-args column in the SiteDetail cron tab). Get the
  repro or drop after the next QA round.
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
- [ ] **Option, not a commitment: a self-built nginx (deployment target 12)
  would drop the app floor from 15 to 14** (MySQL's floor). Same
  `rexenv/runtimes` path that built PHP 7.4; recorded like the c-ares ruling -
  known, waiting for a reason (e.g. macOS-14 users actually asking).
- [ ] **PHP 7.4 — the two residuals of a shipped feature** (`docs/PLAN-php-74-support.md`;
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
  - [ ] **The upstream source commit is recorded nowhere in rexenv.** PLAN §11 names the
    risk in its own words — "the backports branch is one volunteer's rebased branch… if it
    stops, the artifact quietly becomes a frozen, known-vulnerable PHP" — and prescribes
    the mitigation: record the exact `shivammathur/php-src-backports` commit in the pin
    comment, the way `core/binaries.rs` already does for FrankenPHP. The pin comment
    (`binaries.rs:715-731`) does not carry it, so nothing in this tree can answer "which
    7.4 is this?" without leaving it.
  - [ ] **The x86_64 half is on a clock: GitHub's x86_64 runners end August 2027.** PLAN
    §4.5/§11 says to land the cross-compile path before then, and notes that a cross-built
    artifact can never run a native smoke test. Nothing in this file mentioned 2027 until
    this reconcile.
  - [ ] **Xdebug is silently unavailable on 7.4, and unlike 8.0 it is not blocked by
    anything.** `xdebug_bottle` has rows for 8.1–8.5 only, so `xdebug_supported("7.4")`
    is false through the SAME `None` that means "8.0 physically cannot dlopen" — the
    exact conflation `binaries.rs:126-133` warns about. Decide which it is for 7.4 (a
    bottle that exists and is not pinned, or a genuine absence) and say so where the user
    reads it.
  - [ ] **The upstream source commit is recorded nowhere in rexenv.** PLAN §11 names the
    risk in its own words — "the backports branch is one volunteer's rebased branch… if it
    stops, the artifact quietly becomes a frozen, known-vulnerable PHP" — and prescribes
    the mitigation: record the exact `shivammathur/php-src-backports` commit in the pin
    comment, the way `core/binaries.rs` already does for FrankenPHP. The pin comment
    (`binaries.rs:715-731`) does not carry it, so nothing in this tree can answer "which
    7.4 is this?" without leaving the repo.
  - [ ] **The x86_64 half is on a clock: GitHub's x86_64 runners end August 2027.** PLAN
    §4.5/§11 says to land the cross-compile path before then, and notes that a cross-built
    artifact can never run a native smoke test. Nothing in this file mentioned 2027 until
    the 21 Aug reconcile.
  - [ ] **Xdebug is silently unavailable on 7.4, and unlike 8.0 nothing blocks it.**
    `xdebug_bottle` (`core/binaries.rs:426-436`) has rows for 8.1–8.5 only, so
    `xdebug_supported("7.4")` is false through the SAME `None` that means "8.0 physically
    cannot dlopen" — the exact conflation `binaries.rs:126-133` warns about. Decide which
    it is for 7.4 (a bottle that exists and is simply unpinned, or a genuine absence) and
    say so where the user reads it.
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
- [ ] **Code comments point at TODO rows that are not in TODO.** `core/sites.rs:88-90`
  says the FrankenPHP same-major PHP skew "is tracked in `docs/TODO.md`" and it is not
  (it was ANSWERED on 15 Aug by the disabled-picker annotation, ledger #333 — so the
  comment is not just a dangling pointer, it describes an open gap that is closed). Nine
  more sites cite a "Deferred services" row that moved to `docs/archive/SHIPPED-2026-07.md`
  (`core/apache.rs:1`, `core/mariadb.rs:1`, `core/redis.rs:1`, `core/binaries.rs:1205`,
  and the examples beside them). Following any of them lands a reader in a file that does
  not mention the thing.
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
- [ ] **`verify.sh` lints one target of four.** `cargo clippy --lib -- -D warnings`
  covers the library and nothing else: not `src-tauri/src/main.rs`, not the 134 files in
  `examples/`, not `#[cfg(test)]` code, and not the `cli` crate — which the same script
  deliberately started testing on 12 Aug with the reasoning "a gate that skips a shipped
  crate is not a gate". The examples are where this session found 22 defects.
  **And the frontend has no linter at all**: no eslint/biome/oxlint in `package.json`, no
  config, `verify.sh` runs `tsc --noEmit` only — while `src/` carries 17
  `// eslint-disable-next-line` comments suppressing rules nothing runs. Decide: adopt a
  linter, or delete the comments that pretend one exists.
- [ ] **Two pinned facts are missing from the docs that exist to hold them.**
  `PHP_DEBUG_XDEBUG_VERSION = 3.4.5` (`core/binaries.rs:39`) is the only pinned version
  with no row in `docs/PORTS.md`, where all sixteen siblings appear. And
  `PHP_DEBUG_BASE_URL` still names `dl.rexenv.dev` (`:42`, `:951`, pinned by a test at
  `:3137`) after the B33 ruling moved the host — `docs/xdebug-debug-build.md:84-86` says
  that host "is not used", which is a doc asserting a state the code contradicts. Nothing
  breaks today only because the artefacts are unresolvable; it will be discovered at
  upload time.
- [ ] **Ledger hygiene — three of them, all in the file that polices staleness.**
  (a) `docs/CLAIM-LEDGER.md:561`'s hand-curated tail says "plus 5 🚫 premises living
  inside ◐/✅ rows" and names five; there are about eleven. It is the one clause
  deliberately outside `scripts/ledger-tally.sh`, which is exactly why it drifted.
  (b) The Tier-1/Tier-2 blast-radius tables — the work-ordering index this file's proof
  backlog says to work "top first" — list rows that are now fully ✅.
  (c) `scripts/ledger-tally.sh:8-15` calls the ledger "a 500-line file" twice; it is 881
  lines. The script that exists to stop stale numbers carries two.
- [ ] **Two UNCALLED-allowlist entries have stopped being temporary.** `core/copy_scan.rs`
  exempts `createSite` ("superseded by the job-based provision flow; the wrapper predates
  it") and `wpThemeEnableNetwork`/`wpThemeDisableNetwork` ("multisite theme
  network-enable has no UI yet"). The allowlist's own comment says it "is allowed to
  SHRINK, never to grow silently" — so the dead wrapper should go, and the missing UI is
  a feature whose only record is a const array inside a test.
- [ ] **`REXENV_LARAVEL_DOTENV` is read by a test and set by nothing.**
  `core/laravel.rs:357` returns early when the var is absent, and a repo-wide search finds
  exactly two mentions: that line, and `docs/CLAIM-LEDGER.md:459`, which credits the test
  as the thing that stops the hand-copied `.env` fixture going stale "in silence". The
  guard against silence has never run.
- [ ] **Three small CLI gaps, all with the backend already built.** `cli_server.rs:981`
  answers `mail.mark_read` and no `rex` verb sends it — the one unreachable arm of the
  whole dispatch table. `docs/CLI-ROADMAP.md` lists four 🟢 wins whose IPC exists
  (`site retry` matters most: `rex site create`'s failure message names a recovery the CLI
  cannot perform) and a 🟡 protocol-version handshake, which is the standing answer to a
  hazard this repo has already hit — a stale `rex` against a newer app produced the
  "unknown command … newer than the running app" confusion. None of the five is recorded
  here.
- [ ] **`docs/TESTING.md` §3.3's layout-fixture matrix was designed and never built.**
  `layouts()` appears in no source file. It is the named mechanism for the Bedrock bug
  class — a path assumption a layout invalidates — which has produced the unlink-delete
  guard defeat, the `wp core install --path` bug (#299) and the content-dir rule (v24).
  §3.3 says "the class is fully testable once the matrix exists"; until it does, that
  sentence is a plan, not coverage, and it reads as coverage.
- [ ] **One stdout surface stays uncovered and only ARCHITECTURE says so.**
  `docs/ARCHITECTURE.md:773-775` records that #316's marker cut and #317's
  `display_errors=stderr` close the tail and the head of the wp-cli noise problem, and
  that "what stays uncovered is a plugin that `echo`es mid-command" — the third door into
  the same symptom (every WordPress screen dead on the affected site), held open with no
  row anywhere.

### Promoted out of ticked rows (21 Aug 2026)

Open work that was living inside `[x]` blocks. It is here because the archive is not a
place to keep unfinished things.

- [ ] **The WCAG token sweep has no L2 render check.** The app's look changed in ~40
  files (135 consumers moved to `text-muted`, two tokens deleted, one accent darkened)
  and nothing but an eye has confirmed the result. Worth a pass on the packaged app, or a
  wk-check that samples a dense screen.
- [ ] **An override site has never served through a REAL tunnel end to end.**
  `tunnels::origin_port` resolves the recorded override port and the L0 proof is
  plant-proven; SMOKE §Public sharing gained the step, and a network-tier leg would need
  a FrankenPHP fixture on `tunnel_exposure_check`.
- [ ] **No wk-check asserts the FrankenPHP PHP picker is disabled.** The SiteDetail
  Environment card shows the served version and a disabled select; the L2 gap was stated
  when it shipped (ledger #333) and is still open.
- [ ] **`validate_linked_docroot` does a per-call `list(conn)`** (`core/sites.rs`) — fine
  at current scale, hoist if imports grow.
- [ ] **Plugin-update progress: the TIMING half** (ledger #249) — cancel-then-settle beats
  an in-flight check; the wiring half landed, this did not.
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
  - [ ] **The seven that DELETE are pinned (21 Aug 2026); ~17 that only WRITE are not.**
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
  - [ ] Then make the unpinned path impossible rather than reviewed: `sites::provision`
    could refuse when `sites_dir` still resolves to the home-derived default while the
    platform is a sandbox — a check the fixture cannot forget to write.

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
  `download_progress_check`'s bin-cache delete off the REAL shared cache
  (surface-coverage finding 29 Jul: the sandbox invariant is structural for only
  ~20 of 109 examples, and this one deletes a real content-addressed entry),
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
- [ ] **`docs/PUBLISH-TESTING.md` is stale in the three places a release-day reader
  uses.** (a) §A's heading and the publish-blocking summary still present 0.2.0 as the
  newest release; (b) the summary table names itself the publish-blocking summary and
  omits §F and §G, both marked 🚧 in the body — the guard-covers-claimed-surface family
  again, and this one clears a release without ever showing two blocking gates; (c) §D's
  trigger still reads "once the dmg is on GitHub Releases", an event that happened four
  releases ago, so a reader skips it as not-yet-applicable. `docs/RELEASING.md:167` has
  the matching drift — it calls the interim flow "the flow in effect today (2026-08-12)"
  when it is now the only flow that has ever cut a release.
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
