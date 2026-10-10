# TODO — the single active-work file

Everything open lives here, and ONLY open work lives here. Shipped rows move to the
month's evidence log (`docs/archive/SHIPPED-2026-07.md`, `-08.md`, `-09.md`, `-10.md`) with
`scripts/todo-reconcile.py` — the mechanical half; the judgement half is the
`reconcile-todo` skill. Tick an item in the commit that does the work, with a one-line
✓ evidence note. `scripts/todo-reconcile.py --count` prints the open/ticked tally.

**Reconciled 9 Oct 2026, after the 0.8.13 publish** (HEAD `c32f79c8`): 28 open rows audited against the
code, PUBLISH-TESTING and SMOKE. Shape 3 (shipped narrative inside an open row) was the big one: the
**Windows launch** umbrella (1065 lines) and **Linux launch** (84) were open while both OSes have shipped
since 0.8.8 — closed, and their six genuinely-open children (two macOS-keyed per-OS answers, the silent
Caddy no-start, Windows PHP curl's CA bundle, the winget PR, the WebView2 bootstrapper + Smart App
Control, Linux's tunnel normal-stop leg) became their own rows. Shape 1 (open only on paper): the
in-app self-update umbrella ("a dmg user has no update path" — false since 0.6.0) closed, its unwatched
legs living in the remaining-human-rows row; Linux L6; the relauncher's record (#768 → ✅, real updates on
all three OSes; SMOKE ×3). Shape 4: the "both slices" floor row folded into the Intel spot-run. False
sentences corrected: the Defender row's "first run is the next release's", SMOKE's Windows open list,
CLI-ROADMAP's `php install` "not live-run". Open after the pass: 30 rows (24 kept for their first-line reason + the 6 split out).

**Reconciled 9 Oct 2026, the full judgement pass** (HEAD `f8447efc`, v0.8.12 + 11 commits): 15 ticked
blocks moved by the script, then 1 more after the ticks (13 from Now, 1 from Blocked, 1 from Phase 4+ — the 8 Oct Windows report's rows and the apt +
website closures). All 34 open rows audited against code, git and CI. **Shape 1 (open only on paper): 4**
— the release-on-Actions and L9 children (v0.8.12's `release.yml` run, 16 assets), the W9 screens child
(`a9d987bf`), Public distribution (repo public since 30 Sep); ticked in place with evidence (the nested
ones stay as their open parents' evidence). **Shape 2 (a row claiming what is no longer true): 6
corrected** — §G's "#243 is 🔨" (✅ L2 since 2 Sep), the floor row's "15.0" and nginx question, the
in-app-verifies list naming two CLI legs that ran 5 Sep, §D's tap-hosting, ledger #95's line; and a
wp-cli re-check paragraph that sat in the private-window row moved to the wp-cli row. Two rows got the
SMOKE legs they had not named (#768, #772). **Shape 3 (bloat): left as is** — the Windows/Linux launch
parents, macOS 13 floor, self-update and four one-off investigations each carry pages for one open
reason; collapsing them is its own pass. Shape 4: none. Orphaned prose: the evidence of two of this
pass's own ticks had landed inside other rows (a row with no blank line after it) — moved under its rows.

**Reconciled 8 Oct 2026, before the 0.8.12 cut, mechanical pass** (HEAD `5c92e4f8`, v0.8.11 + 53
commits): 17 ticked blocks moved by the script into the new `docs/archive/SHIPPED-2026-10.md` (15 from
Now, 1 from Release gates, 1 from Blocked): the update card's one size, the empty-log hint, the tutorial
videos, the Linux update-into-Welcome and WebKitWebProcess rows, `rex status | head`, the macOS reopen,
`/S` over a running app, the Background-Items pile-up, Stop all vs a slow root edge, the WordPress
core-files bug (users' sites now detected + repaired, #793), the keychain dialog, the runtimes Rosetta
builds, the curl threaded resolver, the PHP 7.4 residuals, the release host, OpenLiteSpeed. The four
judgement shapes were NOT re-hunted (shape 3's one known case — the core-files row — closed and moved).

**Reconciled 30 Sep 2026, before the 0.8.11 cut** (HEAD `149535c2`, v0.8.10 + 35 commits): 23
ticked blocks moved by the script (22 from Now, 1 from Menu-bar app) — the week's fixes, each proven
on the VMs: the per-OS update manifests, the patient DNS and edge watchdogs, the durable boot files,
the Linux route/link and certutil rows, the Windows second-account task, the macOS relink without the
CLT and the DNS agent's settled-bundle wait + launcher name, the From-Git PHP check, the stopped-stack
tabs, the "Setting up" row, the tutorial videos. Judgement: shape 1 (open only on paper) hunted over
the bug rows only — each was fixed or proven this week, none left open on paper; the feature rows were
not re-hunted. Shape 2: none found. Shape 3: 1 — the WordPress core-files row carries its shipped
narrative and stays open for the users'-sites ruling, left as is. Shape 4: none. No orphaned prose.

**Reconciled 29 Sep 2026, before the 0.8.10 cut, mechanical pass only** (HEAD `207d6280`,
v0.8.9 + 39 commits): 16 ticked blocks moved by the script (13 from Now, 2 from Release gates,
1 from Phase 4+): the one-command install, the Linux `.rex` scope fix, the unclean-reboot DB
lock files, the one-prompt setup/teardown, the onboarding finish page, the update-card states,
Bedrock's wp-cli path, the release-notes wiring. The four judgement shapes were NOT re-hunted.

**Reconciled 23 Sep 2026, before the 0.8.7 cut** (HEAD `b208949e`, v0.8.6 + 16 commits): 19
ticked blocks moved by the script (17 from Now, 2 from Release gates — the 18 Sep clean-VM
fixes, the Windows rows, the Local/Valet multisite imports, the typed-`name.rex` fix). Shape 2
(a row describing a state that no longer exists): 1 — "a self-built nginx would drop the floor
from 15 to 14" sat open while nginx has been ours at `minos 12.0` since 30 Aug and the floor
moved to 13 on 23 Sep (`docs/PLAN-macos-13-floor.md`); moved to the archive's judgement
section. Shapes 1, 3 and 4 were not re-hunted this pass; the macOS-13 row in Now is the one
long row and is open for its stated reason (the 15-upgrade and the 14 VM).

**Reconciled 12 Sep 2026, targeted pass** (HEAD `ab06f11`): two blocks moved by hand — Import
sites from Local (every child closed; its plan archived with it as `docs/archive/PLAN-local-import.md`, Q2
multisite staying as its Parked row) and Valet compatibility tails (the link-farm leg closed
owner-run). A third ticked row, the admin-password dialog (`c995163`, the same hour), was
left in place for the next mechanical pass. Shape 4 (parent whose children all closed):
the Local parent, 1. The other shapes were not re-hunted.

**Reconciled 11 Sep 2026, mechanical pass only** (HEAD `954d0f7`, v0.6.1 + 46 commits, before
the 0.7.0 cut): seven ticked blocks moved (MCP parity; the first-run stuck downloads and the
three privileged-prompt rows; the two job-log rows). The four judgement shapes below were
NOT re-hunted this time — the last full pass is still the one below.

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

- [ ] **Windows Defender flags the PUBLISHED 0.8.11 `setup.exe` as `Trojan:Win32/Bearfoos.B!ml`** (found 8 Oct
  2026 on the Dell, Windows 10 22H2, signatures 1.459.601.0, putting it back for the 0.8.12 §M): `Start-Process`
  refused "the file contains a virus or potentially unwanted software", the detection SeverityID 5 (severe),
  `!ml` = Defender's machine-learning heuristic — the usual false positive on an unsigned NSIS installer
  (D5: unsigned by ruling). The 0.8.12 `setup.exe` and `.zip` scanned clean the same hour
  (`MpCmdRun -Scan -ScanType 3`), but an ML verdict can land on any build, and a user who meets it reads
  "rexenv is a trojan". Owed: (1) the owner submits both installers to Microsoft as a false positive
  (https://www.microsoft.com/wdsi/filesubmission — outward-facing, his); (2) a release step that scans every
  Windows asset with Defender on the Dell before Publish (`docs/RELEASING.md`). The verdict is NEW in
  signatures between 1.459.576.0 (the Win11 VM: 0.8.11 scans clean) and 1.459.601.0 (the Dell). The
  0.8.12 Windows §M therefore ran on the VM only.
  ✓ **(2) done 9 Oct 2026:** `docs/RELEASING.md` step 3 + the `release` skill's Windows gate — both
  Windows assets scanned with `MpCmdRun` on fresh signatures before Publish, the signature version recorded,
  a detection blocks Publish. First run: 0.8.13, 9 Oct 2026, the Dell, signatures 1.459.636.0 — `setup.exe` and
  `.zip` clean (PUBLISH-TESTING §A 0.8.13). **Open for (1) only** — the owner's
  submission to Microsoft.

- [ ] **macOS 13 floor — three tiers, T0–T6 landed, T7 RUN on a 13.6 VM and its in-place 15.8 upgrade (six real defects found and fixed; 14-VM left), T8 SHIPPED as 0.8.7** (owner ruled 23 Sep 2026):
  macOS 15 stays the STANDARD (every feature, latest pins); the app also RUNS on 13 and 14
  with a per-host pin set (`BinaryTier`, derived from the host every launch) and a
  "needs macOS 14" refusal for PostgreSQL and PHP 8.0 on 13; self-builds
  (`rexenv/runtimes`) flip refusals to pins later. Every `minos` measured both slices,
  the pin set, the design and the task list T0–T8: `docs/PLAN-macos-13-floor.md` §6–§7.
  Ship line is T0–T8; proof is a real 13 + 14 VM (T7), never `minos` alone.
  ✓ **T0** 23 Sep 2026 — `BinaryTier` + `Platform::binary_tier()` (default `Standard`,
  macOS override) + `install_tier` at launch; ledger #707; no consumer reads it yet.
  ✓ **T1** 23 Sep 2026 — `PinSet` + `pins()`; the `*_VERSION` constants are private, so the
  guard is the compiler (135 files migrated); every tier still answers the Standard set;
  ledger #708.
  ✓ **T2** 23 Sep 2026 — Legacy13/14 pin sets with digests (cloudflared 2025.4.0; MySQL
  8.4.3/8.0.40 `macos14`; ventura blobs for Redis 8.2.1, MariaDB 12.0.2/11.4.8, httpd 2.4.65,
  Xdebug 3.4.5 for 8.1–8.4, none loads for 8.5; PostgreSQL 16.4.0 on 14 only); `manifest_sweep_check` sweeps every tier;
  `legacy_pins_check` resolves + runs each here; ledger #709.
  ✓ **T3** 23 Sep 2026 — refusals: `php_minor_needs_macos` / `engine_needs_macos` (derived:
  the lowest tier that offers it), ONE sentence from `words.rs`, listed-disabled on Settings /
  Databases / New Site, the same words from `rex` and MCP through the core gates; ledger #710.
  ✓ **T4** 23 Sep 2026 — `minimumSystemVersion` 13.0; `default_stack(tier)`; `macos_floor_check`
  per tier (each stack ≤ its floor, stated == lowest); the onboarding legacy note
  (`legacy_notice`, derived from the refusals); INSTALL / SMOKE (Legacy section) / RELEASING
  (manifest floor must follow the conf) / PORTS / TESTING; ledger #433 re-shaped.
  ✓ **T5** 23 Sep 2026 — catalog `minMacos`: a legacy host is offered only an entry that
  declares a floor it meets (undeclared = not offered there; Standard takes all); both
  accessors gate; runtimes' publisher emits it from `vtool` (runtimes `05220a5`, pushed);
  **serial 7 published 23 Sep 2026** — `minMacos: "12.0"` on all 20 PHP entries; ledger #711.
  ✓ **T6** 23 Sep 2026 — `legacy_upgrade_check` (network tier): MySQL 8.4.3→8.4.6, 8.0.40→8.0.44,
  MariaDB 12.0.2→12.3.2 (default→default, one `mariadb/data`), 11.4.8→11.4.12 — legacy writes,
  standard reads, same datadir; ALL PASS here; ledger #712.
  ◐ **T7** 23 Sep 2026, clean macOS 13.6 VM over ssh + `rex`: the first build ABORTED at launch
  (`NSApplication.activate` is 14+ — ledger #713, gated by `respondsToSelector:`), and
  `rex db versions --set postgres` said the platform sentence (a second copy of the engine
  gate — folded into `DbEngine::ensure_available_on`, #710). With both fixed: legacy pins
  offered and RUN (mysql-8.4.3, nginx, php-fpm 8.3 + the 3.4.5 Xdebug pool, Caddy, Mailpit),
  a Blank-PHP, a WordPress and a Laravel site were created and served HTTPS 200, both refusals
  say the tier sentence, the app swap adopted the running services. Eyes-on (JXA clicks +
  screenshots) the same day: onboarding legacy note, the branded keychain prompt, muted
  PostgreSQL row, New Site's tier note, the 8.0 chip — all as designed after two TSX fixes
  found there (a PDO note firing beside the tier note; 8.0 appended out of order); a public
  share ran on **cloudflared 2025.4.0** and answered 200 from the host. With CLT installed on
  the VM: Apache 2.4.65 served, MariaDB 12.0.2 ran; **Redis 8.2.1 died on the session's
  `LANG=C.UTF-8`** (a locale macOS 13 lacks) — fixed by spawning with `LC_ALL=C` (#714).
  **The in-place upgrade to 15 ran the same day** (that VM, `startosinstall` to 15.8): MySQL
  8.4.6 / MariaDB 12.3.2 / Redis 8.8.0 came up on the datadirs the legacy pins wrote, markers
  and rows intact, PostgreSQL and PHP 8.0 ordinary rows — and it found the `DnsMode::Down`
  latch (#442 leg 4). **Still open:** the macOS **14** VM. `docs/SMOKE-TEST.md` "macOS 13 and 14".
  ✓ **T8** 23 Sep 2026 — 0.8.7 published (`docs/PUBLISH-TESTING.md` §A 0.8.7): tap release +
  cask `:ventura`, app-manifest serial 13, source `34bd0d7e`. The row stays open for the 14 VM
  and for §6.6's self-build follow-ups, listed below it.
- [ ] **PostgreSQL self-build for macOS 13/14** (`docs/PLAN-macos-13-floor.md` §6.6 rule 3, item 1
  — opened at T8, 23 Sep 2026). Both slices, `MACOSX_DEPLOYMENT_TARGET=13.0`, in `rexenv/runtimes`
  on the nginx recipe; ships as a Legacy13/Legacy14 pin and flips the "Needs macOS 14/15" refusal
  on the Databases page, New Site and `rex db versions`. Items 2 (PHP 8.0 — blocked on
  static-php-cli's x86_64 abort) and 3 (Redis/MariaDB/httpd — the Intel bottle row) stay where
  §6.6 leaves them.
- [ ] **Windows: two per-OS answers still read off macOS builds** — `php::pdo_pgsql_supported` →
  `binaries::php_has_pdo_pgsql` is keyed on the macOS self-hosted tag (so Windows refuses PostgreSQL
  on 7.4/8.0 although `platform/windows/mod.rs` enables `pdo_pgsql`), and `wp_dns::resolver_for(minor)`
  carries macOS measurements with no OS. Xdebug and the update catalogs are already per-OS. Split
  out of the Windows launch row 9 Oct 2026 (plan §3a Q3).
- [ ] **Windows: a Start all that never spawns Caddy and says nothing** — seen after a tree-kill
  (Windows launch row) and again on the Dell right after the 0.8.13 `/S` install (PUBLISH-TESTING §A
  0.8.13): `rex start` returned done, Caddy stayed idle, no log line; a second `rex start` brought it
  up and three exact re-runs did not reproduce it. Not #806 (that edge was live). Next: log the
  reason whenever `prepare_edge` plans no start, then catch it.
- [ ] **Windows: PHP's own curl has no CA bundle** — a plugin's bare `curl_init("https://…")` to a
  public host fails certificate verification (nothing sets `curl.cainfo`/`openssl.cafile`). Needs a
  pinned CA bundle shipped as an artifact. Owner ruled "not now" (Windows launch row).
- [ ] **winget: PR 437674 waits for a moderator** — `scripts/winget-manifest.sh` generated it; the PR
  is OPEN with `Azure-Pipeline-Passed` + `Validation-Completed` (checked 9 Oct 2026). Nothing to do
  from here but watch it merge.
- [ ] **Windows: WebView2's `downloadBootstrapper` on a PC without WebView2** — never run: every test
  machine already has WebView2. Open: that the per-user installer fetches it without an admin
  prompt. Also open from SMOKE § Windows: a run with Smart App Control ON.
- [ ] **Linux: the tunnel guard's normal-stop leg** — SMOKE § Linux's last unticked row; never run, and
  `tunnel_parent_death_check` has no Linux tier in `scripts/live-checks.sh`. Split out of the Linux launch
  row 9 Oct 2026.
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

- [ ] **Private-window flags for Arc, ChatGPT Atlas, Orion.** Left `None` in the
  `BROWSERS` table because no one has run the flag on a real install, and a fork
  that swallows the flag it inherited opens an ordinary window under a control
  that said private. One-line each once tested; the rows simply show no private
  icon until then.
  **Checked 24 Aug 2026: none of the three is installed on this machine** (Chrome,
  Firefox, Brave and Safari are). So this is not "nobody got round to it" — it cannot be
  tested here at all, and the honest `None` stands until someone with one of those
  browsers runs the flag.
### Open work that was living inside ticked rows

- [ ] **Radicle-hosted repos are unverified** — same code path as the Bedrock clone that
  was verified and found broken, no live project to hand.
- [ ] **Why that `rex` instance went deaf was never diagnosed** — the evidence died with
  the pid. Reproduce before blaming App Translocation.

- [ ] **Windows `setup.exe /S` over a running app + agent sometimes exits 2** — 2 of 16 rounds on the
  Win11 ARM VM (0.8.13, 9 Oct 2026), 0 of 5 on the Dell. Safe failure (an abort before any file is
  replaced: the old version stays, the agent comes back), but a scripted silent update reports failure.
  Excluded by measurement: a file-lock race (`rexenv.exe` writable within ~50 ms of the kill). Fits:
  Tauri's own `CheckIfAppIsRunning` (right after our PREINSTALL) finding a `rexenv.exe` that started
  AFTER our kill loop — in the first failure the agent was restarted mid-install — and its kill
  returning an error → `Abort`. Next: run the failing shape under a console (`-NoNewWindow`) until it
  fails, so `CheckIfAppIsRunning`'s red line names the path; then disable the task for the swap (and
  re-enable it in `.onInstFailed` too), or re-run our kill loop after a settle. PUBLISH-TESTING §A 0.8.13.

- [x] **On Windows, an example's `Reaped` PostgreSQL leaves `--forkchild` workers alive** ✓ 10 Oct 2026 — `Reaped::reap` runs `taskkill /PID <pid> /T /F` on Windows BEFORE terminating, while the parent link still names the children; `db_clone_check` then ran twice in a row on the Dell, both all green, the second not refused (found 10 Oct 2026 on
  the Dell running `db_clone_check`): the run passed, but an `io_worker` child of the sandbox postmaster kept
  listening on :13393 and holding the example's stdout. The ssh call hung, and the next run refused the port.
  `taskkill /T` on the postmaster pid after the fact took all but one. `Reaped::from_proc` kills the postmaster
  only; on Windows its children are not in a process group with it. Fix in `examples/common` (kill the tree,
  or ask `pg_ctl stop`), then re-run `starter_seed_check` and `db_clone_check` on the Dell twice in a row.

- [x] **Live sync: `/files/list` walks ALL of `wp-content` on every page** ✓ 10 Oct 2026 — a bounded sorted walk (`Rexenv_Sync_Reader::walk`): skips what sorts before the cursor, stops at the budget; 14 small pages = the one-page list, plant caught (#825) (code review, 10 Oct 2026). The
  walk is sorted so a cursor can be a path, but nothing bounds the walk itself. A site with 100k
  uploads could pass `max_execution_time` before the first page returns. Fix: walk in sorted order
  and STOP at the budget (resume by descending into the cursor's directories), or cache the sorted
  list per pull in a transient. `companion/rexenv-sync/includes/class-rexenv-sync-reader.php`.
- [x] **Worktree git runs block an async worker** ✓ 10 Oct 2026 — `commands::worktree::off_the_runtime` (`block_in_place` on the multi-threaded runtime, a plain call on a current-thread one) wraps the IPC create/remove, the delete path's release, the CLI arm and the MCP op; `worktree_site_check` green through it (code review, 10 Oct 2026): `commands::worktree::start`
  (Shape B's `git worktree add`) and `release_for_delete` (`git worktree remove`) run git synchronously
  inside async commands, the delete path included. A big checkout holds a tokio worker for its whole
  length, with no cancel. Move both into `spawn_blocking`.

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
## Release gates (human, scripted — see the docs named)

**This section is HALF the set.** The rows below are `docs/PUBLISH-TESTING.md`'s
outstanding gates; the other half is `docs/SMOKE-TEST.md` — the clean-Mac main body plus its
Windows and Linux sections — and each of those three now opens with a **"Still open on
<OS>"** list (reconciled 28 Sep 2026), which is the SMOKE half of this section: rows that are
visible from nowhere here (the PHP update button's apply + REVERT, the Adminer revert, the PHP
ini revert, the MCP HOLDs, …). **A release-day reader works both files, in this order:
SMOKE-TEST on the built installers, then PUBLISH-TESTING §A0/§A before publishing.** The rule
this note exists for: silence in a gate list reads as "this is the set", and a gate
nobody can see from the list is indistinguishable from a gate nobody ran.

- [ ] **The in-app update's remaining human rows (macOS)** — was "the swap probe (T0) and the
  first real in-app update (T11)", `docs/archive/PLAN-self-update.md` §6.5 and §13. **Both RAN:**
  T0 measured the swap on the dev Mac; T11 ran 0.6.0 → 0.6.1 there (7 Sep 2026, twice), on the
  clean 15.6.1 VM (18 Sep, → 0.7.2) and on the 15.8 VM (27 Sep, 0.8.7 → 0.8.8). Open for what is
  left, reconciled 28 Sep 2026 — `docs/SMOKE-TEST.md` §"In-app self-update": the card when
  current, the offline leg against #540's fix, the Homebrew consent sentence on a cask-installed
  copy, and which permissions come back per macOS major. None can be run by any tier.
  Also open there: Start all over a running edge adding no "Background Items Added" card, ledger #772.
  ✓ The relauncher's record (#768) RAN 9 Oct 2026: `relaunch.log` written by 0.8.12's and 0.8.13's
  relaunchers on the 15.8 VM (PUBLISH-TESTING §M 0.8.12 / §A 0.8.13).
- [ ] **Release 5.4 — clean-Mac smoke test** (`docs/SMOKE-TEST.md`): cold runs on a clean
  macOS 15.6.1 VM (18 Sep 2026) and a clean 15.8 VM (23 Sep, 0.8.7) since the 10 Jul first
  pass. Open for the file's own **"Still open on macOS"** list (reconciled 28 Sep 2026) —
  including what the 23 Sep pass did not reach: multisite (convert included), tunnels, WordPress
  Manager, Git assets, the MCP parity steps — and the TLD v1 Done-when list
  (`docs/archive/TLD-FEATURE-REPORT.md`), which no recorded run names.
- [ ] **Tunnel probe session** (one sitting, real network,
  `scripts/tunnel-measure.sh`): kill -9 death-path timings, wifi-blip recovery,
  the banner→authoritative-DNS gap; plus Bedrock "Log in as" landing in wp-admin
  live, and the first real Radicle-layout link (flagged UNVERIFIED in code, #95).
- [ ] **Intel spot-run**: x86_64 bottle digests + MySQL 8.0.44 x86_64 were hashed
  from real downloads but never RUN (PORTS.md caveat) — run-verify on the next
  Intel machine. No physical Mac is needed: a GitHub-hosted Intel macOS runner (15.7.9) was
  already used on 29 Sep 2026 (SMOKE-TEST). This row also carries the floor's run half (the folded
  "both slices" row).
- [ ] **In-app verifies owed** (CLI passthroughs whose service-touching half the
  example harness guard-blocks; fold into the next deep test): `php
  install/uninstall`, `php settings set`, `db versions --set`, `site
  server/domain/move`, `mail clear`, `tunnel start`, `wp core update/switch`,
  (**corrected 9 Oct 2026:** `wp core switch/update` and `tunnel start --yes`/`stop` were live-run 5 Sep —
  `docs/CLI-ROADMAP.md`'s live-run record; `php install` ran on the Linux VM 24 Sep and on the Win11 VM
  9 Oct. Still owed: `php uninstall`, `php settings set`, `db versions --set`, `site server/domain/move`,
  `mail clear`, plus CLI-ROADMAP's `tld --remove` real-removal leg and `site restart`'s other outcomes.)
  `site create --starter-db` (**RUN 3 Sep 2026** — the seed is real: table
  `starter_items`, a `db.php`, the page renders "connected" and one row) —
  plus the packaged-GUI walks still noted inside their shipped entries: MariaDB
  site from the dialog, Apache site in-app, DB version switch from the Databases
  row, Settings CLI-install card, ref-picker on a real many-branch repo, wp.org
  chips + streamed installs, New Site streamed provisioning card, **Upload zip
  through the real native file dialog** (SMOKE §WordPress Manager — L0 proves
  the gate, L1 the install, L2 the card; nothing can drive the picker).
- [ ] **Windows: the Sites takeback banner names Valet/Herd and a "resolver file"** — `Sites.tsx`'s
  `ResolverDriftBanner` reads "Valet or Herd took <tld>'s resolver file back". **Measured 16 Sep 2026,
  and deliberately NOT fixed:** `drifted_takeovers` filters `list_resolver_takeovers` — only TLDs
  rexenv took over FROM Valet or Herd — and neither tool is ever detected on Windows (#641), so the
  banner cannot render there. Dead copy, not wrong copy on a screen, and a cosmetic rewrite could not
  be tested on either platform without fabricating a takeover row. Fix it WITH the Valet/Herd import
  port, when that path exists on Windows at all
  (Moved here from "Now" 6 Oct 2026: nothing on Windows can make the banner render, so there is no
  work until the import port exists — it sat at the top of the actionable list as if there were.)
  (9 Oct 2026: the "could not be tested" half is stale — `scripts/wk-checks/uireview.js`'s `resolver-drift`
  scenario renders this banner from a mock on macOS words. The decision stands for the other reason: the
  sentence is owner-approved copy the probe pins, and on Windows/Linux the banner cannot render.)
- [ ] **The live pool swap is still L3.** `php_update_check` proves the chain up
  to "a pool on the new patch answers on a FIXTURE port". Stopping the running
  master on the PRODUCTION port and reverting when it does not come back needs
  the real `ServiceManager` and a deliberately broken tree — `docs/SMOKE-TEST.md`.
- [ ] **SMOKE §M1/§M2a/§M2b — the MCP human gates, PARTLY RUN.** Runs recorded: 25 Aug 2026
  (raw socket, a hand-rolled client), 3–4 Sep (the owner's Mac, Claude Code driving) and 18 Sep
  (clean 15.6.1 VM, a raw JSON-RPC client — steps 1, 16 and 17 ran there). **Still open**
  (reconciled 28 Sep 2026, `docs/SMOKE-TEST.md` §"AI agents"): 9 (Keep), 11 ⚠HOLD (no admin
  prompt — observed, not checked), 13's consent copy since D16, 18's real-site arm (the old
  "one arm of 21"), and every step that exists for a MODEL's half — 8, 14, 44 — which the raw
  clients could not give. Evidence: `docs/archive/SHIPPED-2026-09.md` and the SMOKE rows.

- [ ] **Install WordPress into an empty LINKED folder** — out of Stage 0 by
  design (`docs/archive/PLAN-linked-sites.md` decision 2): linking is adopt-only. If
  built: a deliberate site-page action, offered only when the linked folder is
  EMPTY, its own disclosure. Trap first: `configure` is only half idempotent
  (skips `wp config create` when wp-config.php exists but calls
  `create_database` unconditionally), and `phase_defs` blanket-skips WP phases
  on `docroot_managed == Some(false)` — needs an explicit opt-in flag.
- [ ] **`rex` design-first set — ONE item left: raw `wp` passthrough** (a security
  ruling about what the CLI may execute, deliberately not gap-filled). The other four —
  single-site restart (#444), web-tier restart (#445), `wp_user_delete` (#446), progress
  streaming (#447) — shipped 2 Sep 2026; live legs ride the in-app verify list in
  `docs/CLI-ROADMAP.md`. Original row text: `docs/archive/SHIPPED-2026-09.md`.
## Menu-bar app

Shipped 31 Aug – 1 Sep 2026 (Phases A–D, ledger #436–#441; log in
`docs/archive/SHIPPED-2026-09.md`, design record `docs/archive/PLAN-menubar-tray.md`).
The second-instance row this section last carried closed with #441
(`hand_off_to_running_instance`: the CLI socket is the lock; a second launch activates
the first and exits) — its box stayed `[ ]` under a struck-through title, ticked 5 Sep 2026.

## Blocked on external work

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
  **Re-checked 2 Oct 2026:** still v2.12.0 (`gh api repos/wp-cli/wp-cli/releases/latest`,
  published 2025-05-07) — nothing to bump; the containment stays.
  **Re-checked 6 Oct 2026:** latest release still v2.12.0 (2025-05-07; `gh api …/releases`), so
  still nothing to bump — and moved here from "Now": the work waits on an upstream release, not
  on anything rexenv can do. Re-check when a wp-cli release appears.
- [ ] ⚠ **Homebrew publishes NO Intel macOS bottle for `redis` or `mariadb` any more**
  (found 23 Sep 2026 while walking ghcr for `docs/PLAN-macos-13-floor.md`: `formulae.brew.sh`
  lists only `arm64_*` + Linux for redis 8.10.2 and mariadb 13.0.2; `mariadb@11.4`,
  `openssl@3`, `pcre2`, `httpd`, `apr`, `apr-util` still carry a `sonoma` x86_64 bottle).
  The pinned `sonoma` x86_64 blobs (Redis 8.8.0, MariaDB 12.3.2) still resolve — ghcr is
  content-addressed — so nothing is broken TODAY, but the **next routine bump of either
  formula has no x86_64 bottle to pin**, and `manifest_sweep_check` would only say so after
  the bump. Options, in order of cost: freeze the Intel pin at the last x86_64 tag while
  arm64 moves (a per-arch version, which the descriptor does not model); self-build like
  nginx/PHP (`rexenv/runtimes`, both slices); or drop Redis/MariaDB on Intel with a refusal
  that names Homebrew's decision. Owner's call before the next bump, not after.
- [ ] **Xdebug on PHP 8.0** — the Nov 2024 static-php 8.0.30 build exports zero
  Zend symbols (`nm -gU` = 0; dlopen fails `_OnUpdateBool`), upstream still
  serves that exact build (re-verified 16 Jul 2026). 8.1–8.5 solved via the
  bottle path. Fix needs an upstream rebuild or the self-build recipe
  (`docs/xdebug-debug-build.md`). PHP 8.0 is EOL — acceptable to leave excluded.
- [ ] **SMAppService privileged helper** (true single-prompt setup) — needs a
  signed + notarized bundle; packaging-era, after Developer ID signing.
- [ ] **Developer ID signing + notarization** — needs a paid Apple account;
  runbook ready in `docs/SIGNING.md`.
## Phase 4+ (next era)

- [ ] **Git worktree workflow** — one branch = one running site at `feature-x.shop.rex` (falling back to
  `shop-feature-x.rex`), with its own DB cloned from the parent and removal only through `git worktree
  remove`. It also serves worktrees that agents (Claude Code, Cursor…) made outside rexenv. The WP
  plugin/theme-repo shape comes first (owner, 9 Oct 2026). Plan + tasks W0–W11:
  `docs/PLAN-git-worktrees.md`; W0 answered, §9 answered 10 Oct; W7 (adopt, Shape B) and Re-clone DB built the same day (#830, #831). W1 done (v45 relation + the delete guards, #814/#815), W2 done (domain rule + git plumbing, #816), W3 done (same-server DB copy, #817), W4+W5 done (the plugin worktree child job, #819; L1 ALL PASS), W6 done (delete through git, #820), W8 done (UI, #821; WebKit check green), **W7 waits on a decision** (adopting an outside worktree, see the plan's §9), W9 done (`rex worktree` + the MCP tool, #822), W10 done (the whole-site repo shape, #823), **W11 remaining runs next** (Windows lib tests + `worktree_site_check`; Linux on the VM).
- [ ] **WordPress live ↔ local sync** — a companion plugin on the live site + signed HTTPS from rexenv:
  clone a live site locally, Pull, Push with backup + Roll back, per-table conflict choice. Needs a new
  `SecretStore` platform trait (all three OSes). Plan + tasks L0–L15: `docs/PLAN-wp-live-sync.md`.
  Owner answered 9 Oct 2026: in-app zip only, and a push always needs a human click (never MCP). §11 all answered 10 Oct 2026 (0600 file secret store on every OS). L1 (the protocol spec) DRAFTED 10 Oct 2026: `docs/rexsync-protocol.md`, awaiting review; the signature half of L3/L5 built against shared vectors (#824); the plugin's pairing + read side (L3/L4) proven inside WordPress (#825). the client (L5) pulls from it over HTTP (#826). the pull's core (L6) pulls a whole site into a local one (#827). L2 (one owner-only pairing file, #828), L6's job + the **Live tab** (#829) built 10 Oct: a WordPress site can connect and pull. S2 push built 10 Oct: the plugin's quarantine/shadow/swap/rollback (#832), `push_from` (#833), the job + the picker + the typed host + Roll back (#834). Not yet: L7's New-site shortcut, L12 CLI/MCP, L13 security review, L14 real-host smoke (SMOKE § Live sync written, push rows unrun), S3 (resume, anonymise, uploads on demand, per-item conflict choice).
## Known baselines (not bugs)

- WP builds shipping `wp-includes/php-ai-client/**` show those files as "foreign"
  in checksum verify until wordpress.org's manifest covers them. Honest tool
  output — intentionally not filtered.
- On networks that negative-cache DNS, a fresh tunnel URL can be dead on THIS
  machine while live from a second device — the router race, not a bug
  (`docs/SMOKE-TEST.md` tunnels section).
