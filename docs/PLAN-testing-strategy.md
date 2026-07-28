# PLAN — the testing strategy: proofs at the right level, by design

Written 28 Jul 2026. Companion ledger: `docs/CLAIM-LEDGER.md` (the full 194-claim
inventory this plan's §2 summarizes — and the project's test metric).

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

Four layers exist de facto. Named, with what each CANNOT prove stated as loudly as what
it can:

### L0 — Logic (`cargo test --lib`, 535 tests)

- **Proves:** our pure functions, state machines, validators/refusals, config
  *generation* (regression-pinning bytes WE emit), SQLite migrations (rusqlite is real
  in-process), witness types ("this state is unrepresentable"), parser behavior against
  captured fixtures.
- **Cannot prove:** ANY fact about an external tool, server, network, or render. A flag
  list can't prove mysqldump accepts the flags. A plist string can't prove launchd loads
  it. A config substring can't prove nginx 404s a dotfile. An output fixture can't prove
  the tool still prints that line.
- **Cost:** seconds. **Runs:** every `verify.sh`. **Bug class:** logic regressions,
  drift in our own generation/parsing, unrepresentable-state violations.

### L1 — Tool (`src-tauri/examples/*.rs`, 105 live checks)

- **Proves:** what real binaries accept and do — real mysqld/mariadbd handshakes, real
  nginx reloads, real php parsing our generated files, real wp-cli installs, real
  process lifecycle (adopt/orphan/reap), real filesystem layouts. **This layer caught
  the most bugs this month.** Discipline: `common::sandbox` + `common::Reaped` +
  fixture ports (`examples/common/mod.rs` — read its invariant first).
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
  harness routes with mocked IPC, zero backend.
- **Cannot prove:** backend truth (IPC is mocked BY DESIGN), real cookies/schemes
  (`rexdb://` doesn't exist in Playwright), aesthetics.
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
- Frontend honest-UI surface: StatusPill non-running states, StartStopToggle,
  StatusFooter, Tunnels tri-state, SiteProvisionCard — zero programmatic coverage; the
  WKWebView pill-metrics fix was verified once by hand and never committed as a check.
- `uireview.js` cannot fail (assertion-free screenshots, overflow non-fatal, not in
  `run-all.js`).

## 2. The claim inventory

Full inventory: **`docs/CLAIM-LEDGER.md`** — 194 claims from a sweep of every
invariant-language comment in `src-tauri/src`, each with file:line, the claim, and a
verdict. Tally at compile time: **117 proven · 32 half-proven · 36 provable-unproven ·
9 inherently unprovable.** The 🔨 rows plus the unproven halves of ◐ rows ARE the
testing backlog, worked highest-risk first:

1. CF-header set as tunnel discriminator (wp_tunnel + wp_login) — L1
2. "a tunnel can only expose its one site" — asserted in three modules, negative never
   probed — L1
3. dotfile guard proven only as a config substring — L1 (live 404)
4. login-autostart "never download / never prompt" — untested at any level — L0+L1
5. wp_login PHP-injection safety inherited, not tested at the injection point — L0
6. DNS answer-anything justified by an unasserted loopback bind — L0
7. `webview_dialogs.rs` wry/WebKit claims — L2

Ledger discipline: a new "must never / always / is safe because" comment adds a row in
the same commit. A proof-commit flips the verdict and names the proof. That keeps the
metric honest and the ledger from rotting into the archive problem.

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

Current state (full audit in session evidence): 105 examples; only 15 use
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
   404 (all three templates); fpm candidate-vs-live `-t` isolation; tunnel second-Host
   negative probe; manifest HEAD+digest sweep (network tier).

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
| Sleep/wake + reboot edge recovery; DNS agent handoff (#46) | ADD to SMOKE-TEST (task list) |
| Datadir corruption recovery (B22/B23) | PUBLISH-TESTING §E |
| Firefox out-of-the-box trust (#154) | ADD one-line check to SMOKE-TEST |
| Visual design / theme correctness | SMOKE-TEST settings + uireview screenshot sweep (human eyeballs the shots) |
| Radicle layout reality (#95) | flagged in code; first real project confirms |
| Phase-A-never-resolves dtrace spot-check (#19) | ADD short procedure to PUBLISH-TESTING |

## 6. The gate

`verify.sh` stays THE pre-commit bar, unchanged in discipline (own cwd, exit codes
load-bearing, green only from its own line). It grows tiers by composition, not by
bloating the fast path:

- **`scripts/verify.sh`** (fast, pre-commit, minutes): lib tests + examples build +
  clippy -D warnings + tsc. Unchanged.
- **`scripts/verify-full.sh`** (before release / after touching a layer's subject,
  ~10–20 min): runs verify.sh, then the L1 `sandbox` tier, then wk-checks `run-all`
  (spawns its own vite on 5199, kills it after). Own `verify-full: all green` line.
- **`network` / `stack` / `system` L1 tiers:** run deliberately when the change
  touches their subject (the runner prints per-tier one-liners for the commit note).
- **L3:** SMOKE-TEST per release; PUBLISH-TESTING per its per-item triggers.

A gate nobody can afford to run stops being one: the fast bar stays fast, and nothing
above it is required per-commit. The metric the gate serves is the ledger tally, which
only proof-commits move.

## Task list

Small, one commit each, ✓-ticked in `docs/TODO.md` with evidence. Ordered so every
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
CF-header discriminator probes (#2/#33), manifest HEAD+digest sweep, Bedrock live
provision, fpm candidate isolation (#104), sandbox adoption cohorts (§4.4),
`webview_dialogs` L2 coverage (#166), import-graph lint (#163), rusqlite-outside-state
guard (#167).
