# TODO — the single active-work file

Everything open lives here, and ONLY open work lives here. Shipped rows move to the
month's evidence log (`docs/archive/SHIPPED-2026-07.md`, `-08.md`, `-09.md`) with
`scripts/todo-reconcile.py` — the mechanical half; the judgement half is the
`reconcile-todo` skill. Tick an item in the commit that does the work, with a one-line
✓ evidence note. `scripts/todo-reconcile.py --count` prints the open/ticked tally.

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

- [x] **Linux deb in-app update: the app did not come back after the swap** ✓ fixed 27 Sep 2026
  (`rules::relauncher_exe`, ledger #729): `current_exe()` is `/proc/self/exe` and read
  `/usr/bin/rexenv (deleted)` after `dpkg -i`, so spawning the relauncher was ENOENT and the
  honest fallback ran. A package install runs the relauncher from the new `/usr/bin/rexenv`.
  Ships in the next release; because the OLD side spawns, 0.8.8 → next still needs a hand
  open, and the relaunch is proven by the release after that (SMOKE's Linux row).
- [ ] **The polkit sentence for an in-app update is the generic one** — "rexenv needs
  administrator permission to change system settings — the .rex DNS route, the HTTPS edge, or
  the local certificate authority" — while the step it authorises is `dpkg -i` of the new
  package (seen 27 Sep 2026, the first deb update). One action id = one message; the update
  wants its own id (`…privileged-update`) and sentence, the way the macOS prompt names what it
  is for.
- [ ] **A refused descriptor reads as "Couldn't reach the update server"** in the About card
  (`AppUpdateCard.tsx` maps any check error to that line; seen 28 Sep 2026 on the Dell where
  the refusal was a replay): the server answered, and the sentence hides that the machine
  will never be offered anything. The check should surface a refusal as its own state with
  its own words.
- [x] **The accepted-descriptor serial is ONE key for every update document, so a machine
  that once read another document refuses this one as a replay** ✓ 28 Sep 2026 — per-document
  keys (`app_update::document_for`/`serial_key`, ledger #519; the first document keeps the
  bare names), L0 `each_document_has_its_own_floor_and_the_first_document_keeps_the_bare_keys`.
  Found on the Dell (`core/app_update.rs` `SERIAL_KEY`), and the same hour on the 22.04 VM:
  its 0.8.7 dev deb held serial 14 — the macOS document — and refused the Linux document's serial 1): its Windows 0.8.5 held `serial 9`, the
  macOS document's 0.8.3 of 19 Sep 17:54:25Z (a Windows build from before the per-OS URL
  read `app-manifest.json`), and every Windows publish since — serials 6, 7, 8 — was
  refused "OLDER than the highest already accepted (9)". The About card then said
  **"Couldn't reach the update server"** (`AppUpdateCard.tsx` maps any check error to
  that line), which is wrong twice: the server answered, and the machine will stay on
  0.8.5 forever. Fix: key the stored serial by document (`…_serial:<document>`), and give a
  refused descriptor its own sentence. Only machines that changed document are affected;
  a fresh install reads one document from its first check. The Dell and the VM were reset
  by deleting the three `app_update_release*` settings rows, and then updated through the
  card. **A stuck install cannot receive the code fix (it refuses every descriptor), so the
  Windows document must also be published past the highest macOS serial an early Windows
  build could have stored (≤ 10 on 19–20 Sep 2026); the Linux documents start at 1 and nothing
  shipped read another one. ✓ Done 28 Sep 2026: the Windows document re-signed to serial 10
  (runs 36342956297 and 36343274888 — a second run approved beside the first read the same
  serial from its trigger-time checkout and conflicted, runtimes PR #14 reads the tip first).**
- [ ] **A Finder "Replace" of the running app can pop macOS's "rexenv quit unexpectedly"** (seen
  three times on the 13.6 VM, 23 Sep 2026, every one during a HAND swap of the bundle over ssh
  — `rm -rf` + copy, or copy + `mv`): the DNS agent's LaunchAgent (KeepAlive) relaunches
  `rexenv --dns-agent` the instant the app is killed, lands in a half-replaced bundle, and dyld
  kills it with `SIGKILL (Code Signature Invalid)` at `_dyld_start` — the report names rexenv, so
  the user reads it as the app crashing. `crash.log` stays empty (nothing of ours ran). The
  in-app updater swaps atomically (ledger's `app_bundle` rows) and never showed it; a user who
  drags a newer dmg over a RUNNING copy in Finder walks the same race. **Seen a fourth time,
  27 Sep 2026, 0.8.8's §A on the 15.8 VM:** the app was NOT running — only the 0.8.7 agent was
  (from the deleted bundle); the new app kickstarted it for the version mismatch, the first
  relaunch died `Launch Constraint Violation`, the second served. So the trigger is any
  relaunch of the agent right after a swap, not the app being replaced while open. Options: `bootout` the
  agent before the swap in the updater's Finder-replace guidance (`docs/INSTALL.md`), or have the
  agent's KeepAlive wait for a settled bundle (a signature check before exec). Not a 0.8.7
  blocker — the shipped path is the updater.
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
- [ ] **`rex site list` / `site info` say `serving` about a site that cannot be reached.** Seen on the
  15.8 VM, 23 Sep 2026: a WordPress create died at "downloading WordPress core" (cURL 28) and the
  site stayed setup-incomplete with NO vhost in the Caddyfile — `curl https://legacy-mwp.rex`
  answered a TLS `internal error` (no certificate for the name) while both `rex` commands printed
  `serving`. The CLI reads `sites_serving` — the manager's belief (edge up && the site's upstream
  up), the Sites-page bool — not `readctx::probe_serving`, which asks the wire and keeps
  setup-incomplete distinct (#200) for the MCP. Same fact, two answers, one of them wrong: the
  CLI should render the MCP's classification, not the belief.
- [ ] **After Settings → Remove system changes, the health log blames an outsider.** Clean-15 smoke,
  23 Sep 2026: `[edge-down] Caddy: edge stopped and its KeepAlive daemon is no longer installed
  (removed outside the app)` — twenty seconds after the app itself removed it. The service
  manager has no notion of a teardown, so the honest wording for "the user did this here" does
  not exist. Same family as #715 (the DNS watchdog had the same blind spot, now fixed with a
  mode); the edge needs its equivalent, or the teardown should tell the manager.
- [ ] **The edge wire probe reads the app's OWN Caddy reload as a foreign proxy.** Three times in
  three minutes on the 15.8 VM (23 Sep 2026, one per `rex site create`), and earlier the same day
  on 13.6: `[edge-blocked] … another local proxy answers port 443 in front of it — no site will
  load until you quit that app` followed ~10s later by `[edge-unblocked]`. Nothing foreign was
  there — the marker-header probe (`proxy::edge_wire`) misses during the reload the app itself
  just asked for, and "another local proxy" is the fallback when no holder is found. Two toasts
  per site create, naming an app that does not exist. Fix: the watchdog should know a reload is
  in flight (or require the miss on two consecutive polls) before calling the wire foreign.

- [ ] **Adminer: the documented revert does not exist.** After Update (5.4.2 → 6.0.2) the Databases
  row reads only `Adminer 6.0.2`; the older tree stays on disk but nothing offers it, so
  `docs/SMOKE-TEST.md`'s "a revert is a second press" cannot be done. Either offer the kept
  versions, or drop the row and the design note.
- [ ] **A stopped site's WordPress and Database tabs say nothing true** (seen on Windows,
  19 Sep 2026; not established as Windows-only). The WordPress tab spins "Loading plugins…"
  indefinitely — wp-cli cannot reach a database that is not running, and the spinner has no end
  state. The Database tab prints the Adminer URL above a BLANK frame, because Adminer is not
  serving. Both are honest-UI failures of the kind `docs/DESIGN.md` forbids, and the same site's
  **Logs** tab is the proof they are fixable rather than inherent: it names the file, says
  "WordPress debug logging is off — nothing is being written", and tells you what to turn on.
  Done when: each tab, on a stopped site, says what is not running and offers the start.

- [ ] **Smaller, same run:** sub-sites created on a subdomain multisite are recorded with `http://`
  URLs (main site is `https://`); `rex site create` names a site after its domain (`s1.rex`) where
  the dialog derives a name; Hello Dolly's row shows the terminal button and lands on an honest
  "Terminal unavailable" panel where SMOKE says such rows show no button; SMOKE's MCP row 43 says
  49 tools, the endpoint lists 50; a translocated first launch (zip/`cp`, not a Finder drag) writes
  the DNS LaunchAgent plist with the `/private/var/folders/…/AppTranslocation/…` path — self-heals
  on the next launch, DNS dead in between.
- [ ] **Homebrew-bottle bundles (redis / mariadb / httpd / xdebug) still need the Xcode
  Command Line Tools on a clean Mac.** Found 18 Sep 2026 by the first clean-VM smoke test:
  `prepare_binary` asked `otool` for every binary's dylib list and the CLT shim failed all
  six first-run components (ledger #676 — fixed for single binaries by reading the load
  commands in `core/macho.rs`). Bundles are the other half: their `@@HOMEBREW_*@@` load
  commands must be REWRITTEN to `@loader_path`, and the rewrite is `install_name_tool`,
  which is CLT. Today that is a quiet `clt_preflight` error naming `xcode-select --install`
  (no dialog). Closing it means an in-place Mach-O rewriter (a new path fits in the old
  command's padded slot when shorter, which `@loader_path/../lib/x` usually is; longer
  needs the `-headerpad` slack Homebrew bottles are built with — measure before building).
  Done when: a clean user on the VM installs Redis with `xcode-select -p` failing.
- [ ] **Windows: the Sites takeback banner names Valet/Herd and a "resolver file"** — `Sites.tsx`'s
  `ResolverDriftBanner` reads "Valet or Herd took <tld>'s resolver file back". **Measured 16 Sep 2026,
  and deliberately NOT fixed:** `drifted_takeovers` filters `list_resolver_takeovers` — only TLDs
  rexenv took over FROM Valet or Herd — and neither tool is ever detected on Windows (#641), so the
  banner cannot render there. Dead copy, not wrong copy on a screen, and a cosmetic rewrite could not
  be tested on either platform without fabricating a takeover row. Fix it WITH the Valet/Herd import
  port, when that path exists on Windows at all
- [ ] **SHIPPED macOS BUG — new WordPress sites are missing core files (measured 14 Sep 2026).** WP-CLI's
  `wp core download` extracts WordPress's `.tar.gz` with PHP's `PharData`, and rexenv's PHP 8.3.32 reads that
  tarball with every member name CUT AT 100 CHARACTERS: bsdtar lists 3,782 members, PharData 3,776; 40 names
  come out truncated (`…/Contracts/WithRequestAuthenticationInterface.php` → `…Interface.`, `…Italic.woff2` →
  `…Italic.wof`) and six vanish where two truncations collide — 20+ `wp-includes/php-ai-client` classes and
  several default-theme fonts. macOS creates the dot-ended names without an error, so the damage is silent. On
  the owner's machine, 5 of the rexenv sites checked (`hridoy.rex`, `mstest.rex`, `msd.rex`, `new.rex`,
  `ealite`) have only the truncated file; others have both (a later update restored them). WordPress's `.zip`
  build extracts every name intact through `ZipArchive` (measured on the Mac and the Dell). **Owner ruled 14 Sep
  2026: `wp core download` gets the zip URL; existing sites are MEASURED read-only first, repair only on a
  further go.** Fix written: `wordpress::core_zip_url` + `core_download_args` (the only place `core download` is
  assembled — a source guard), used by site provisioning, `install_wordpress` and `core_reinstall` (the version's
  no-content zip, since WP-CLI refuses `--skip-content` and `--locale` with a URL); ledger #604
  - [ ] Read-only `wp core verify-checksums` across the owner's rexenv sites — **measured 14 Sep 2026** (17 sites):
    `hridoy.rex`, `msd.rex`, `mstest.rex`, `new.rex` (7.1) and `ealite.test` (7.0.4) each MISS 25 core files and
    carry 21 cut-name leftovers — this bug; `bl.rex`, `tr.rex`, `tr2.rex`, `xyz.rex` pass checksums but still carry
    the 21 cut-name leftovers (a later update restored the real files); `lm.test` (6.7.1, 175 missing) and
    `oc.test` (6.2, 142 missing) predate `php-ai-client` — a different cause, not attributed here; the other 6 are
    clean. ✓ **Repaired 14 Sep 2026 at the owner's go:** the five broken sites reinstalled from their version's
    no-content zip (`core_reinstall`'s exact arguments) — 25 missing → 0, `wp core verify-checksums` Success on each;
    wp-content, config and database untouched; the cut-name leftovers kept by ruling. Still open for USERS' sites
    created by 0.4.0–0.7.1: 0.7.2 (a hotfix cut from v0.7.1, being prepared) carries the fix, and its release note
    says how to repair an existing site Users of 0.4.0–0.7.1
    with sites created by rexenv are affected the same way — a release note or an in-app repair is a separate
    ruling
- [ ] **`frankenphp_mail_catch_check`'s catch-OFF backend sometimes accepts a request and never answers** — twice in
  two days, both inside a release `verify-full`: 13 Sep 2026 it held 0.7.1's gate 50 minutes at 0% CPU (curl had no
  timeout; now `--max-time 60`), and 14 Sep 2026 on the 0.7.2 worktree it failed the control check with an EMPTY body
  (`getenv('MAIL_HOST') is empty — ` with no detail; the neighbouring "does not carry our shim" check passes vacuously on
  an empty body). The same example then passed 3 of 3 alone (15 s, 14 s, 11 s). Not the mail catch — the first backend
  the example starts. Unmeasured: whether FrankenPHP is still loading its worker when `await_listening` sees the port,
  or wedges; a check that reads "empty body" as its own failure, and the backend's log spilled, would say which
- [ ] **Windows launch** — 12 Sep 2026, owner: macOS is stable, ship a Windows version.
  Not "fill the stubs": Unix-only code outside `platform/`, no php-fpm, no `/etc/resolver`,
  no unix sockets on Windows. Reasoning, measurements and "Done when" per task:
  `docs/PLAN-windows-port.md`. W0–W2 can start now; W3+ wait on the owner's D1–D6.
  - [x] D5 signing ✓ **RULED 19 Sep 2026: unsigned.** The owner: rexenv is open source and earns
    nothing, so it spends nothing — the same answer macOS got, for the same reason. Authenticode is
    out, and the measurement it was waiting for is now a W11 output (what INSTALL must tell a user
    they will see) rather than a decision gate. Every other ruling was already in: D3 named
    pipes + `LocalIpc` (12 Sep 2026); **13 Sep 2026** D1 php-cgi group accepted with the php-src correction (supervision,
    positive ID and worker count written in plan §3 D1(a)/(b) before W3), D2 agent on :53 +
    NRPT accepted and the `hosts` fallback REFUSED (:53 taken → refuse naming the holder),
    D4 accepted and MariaDB NOT in v1 (refused with an honest message; MySQL 8.4/8.0 cover it), D6 accepted (Win 11 x64; Win 10 22H2 best-effort;
    arm64 emulation unsupported)
  - [x] ✓ 19 Sep 2026 — every measurement the port BUILT on is in (D1 supervision W3, D2 :53 + NRPT
    W6, the bind matrix, D5 through Edge on the clean VM: W12 runs 1–2, `docs/SMOKE-TEST.md`). The
    residue named inside is measurement backlog, not a gate: mirrored WSL/`dnsTunneling`, ICS on, a
    holder under another account, Chrome's shelf wording. Measure before building on it (plan §3, Dell + VM): D1 — php-cgi's own parent
    (children spawned, respawned, killed with the parent; the listener's owning pid; job
    breakaway from the app's launch contexts (SSH allows it, #600; a Task Scheduler task REFUSES it —
    ACCESS_DENIED from `proxy::start`, 14 Sep 2026, plan §3 D1; Explorer, Start, Windows Terminal
    unmeasured); memory per child; peak concurrency on a
    block-editor load); D2 — who holds loopback :53 and who ANSWERS it, per state (who ANSWERS, for
    same-account holders: measured 14 Sep 2026, `windows_dns53_probe` — an exclusive `127.0.0.1` agent
    answers over UDP and TCP beside any wildcard holder and is refused only by a `127.0.0.1` holder
    (10048) or an exclusive wildcard (10013), plan §3 D2; the owner ruled hotspot and WSL2 NAT in for the
    real states, Hyper-V and Docker out; WSL 2 NAT measured the same day — once WSL 2 is installed ICS
    (`SharedAccess`, LocalSystem) holds UDP `0.0.0.0:53`, answers the WSL adapter's address, not loopback,
    and an exclusive `127.0.0.1:53` bind still succeeds, and the agent answers beside it; Mobile
    hotspot measured too — no new :53 row, ICS answers `192.168.137.1`, loopback queries time out, the
    agent bind still succeeds; still open: mirrored WSL/`dnsTunneling` on the Windows 11 VM) (clean,
    hotspot/ICS, Hyper-V, WSL2 NAT + mirrored, Docker Desktop); D5 — unsigned NSIS through
    Edge and Chrome: the clicks (each named), every message verbatim with a screenshot, whether
    "Run anyway" is reachable without "More info", and whether the next build repeats it all;
    the Windows bind matrix (plan §6) before `ensure_free` is written — **bind matrix ✓ 13 Sep 2026
    on the Dell** (288 binds, `scripts/probes/windows-bind-matrix.ps1`): the trial bind reports
    free under a `0.0.0.0`/`[::]` holder and then takes its localhost traffic, so `ensure_free` must
    read the tables; :53 clean state measured (nothing there). The desktop user's Medium token
    gives the identical 288 rows (✓ 13 Sep 2026, `scripts/probes/windows-limited-token.sh`). Still
    open: ICS/hotspot on, a WSL distribution (none installed), a holder under another account,
    the D5 installer, php-cgi
  - [x] W3 step 0 — a half-ported build fails out loud (plan §3a Q1, **ruled 13 Sep 2026**): panic hook →
    `crash.log` + a Windows message box; `Error::Unported` stubs instead of `todo!()` (50
    today), scan-enforced under `platform/windows/`, ledger row. **Progress 13 Sep 2026: the stubs
    half is done** — every `Result` stub returns `Error::Unported`, every other one panics through
    `unported!`, `swap` returns `SwapFailure::Other`, `LocalIpc::connect` an `Unsupported` io
    error; `windows_stubs_fail_as_unported_never_todo` scans the module (plant-proven), ledger #595;
    Linux untouched. ✓ 13 Sep 2026, the hook half too: `crash.rs` installed first in `main.rs`
    appends every panic to `<log_dir>/crash.log` (temp dir fallback, rotates at 1 MB) and raises
    `platform::fatal_notice` once per process — a `MessageBoxW` on a Windows release build;
    child-process L0 test + rotation test, both plant-proven, ledger #596. Unproven until a
    Windows run: the message box itself
  - [ ] Update catalogs across OSes — **ruled 13 Sep 2026: a separate signed document per OS**
    (`manifest-<os>.json`, `app-manifest-<os>.json`; unsuffixed = macOS, frozen; naming for
    all three OSes in plan §3b). Measured against every shipped release (0.3.0–0.7.0): an `os`
    field on an `x86_64` row is KEPT by every Intel Mac, so no Windows or Linux entry is ever
    published into the macOS documents. Open until both guards land, and both land before
    the first non-macOS entry is published:
    - [ ] This tree: tests pinning the macOS readers' drop/ignore behaviour, and future macOS
      readers dropping any row whose `os` is not `macos`
    - [ ] `rexenv/runtimes` publisher: refuse an OS marker or a foreign arch in the macOS
      documents, and any per-OS row whose `os` is not the file's
    - [x] ✓ 19 Sep 2026 (`manifest_urls_on`, ledger #690; a pre-`a0d4868f` build reading the macOS
      document is what run 1 of the VM pass saw). W11: the Windows reader fetches `manifest-windows.json` / `app-manifest-windows.json`
      only
  - [ ] Per-OS version answers (plan §3a Q3): PostgreSQL driver, Xdebug, curl resolver, the
    cache marker (the update catalogs are the row above) — each before the Windows feature it gates, each with a test that the
    Windows answer differs where the builds do
  - [ ] Busy-workers signal, macOS too (plan §3 D1(b)): today neither OS tells a user that a
    slow site is a full pool; ESTABLISHED connections on the pool port ≥ workers, sustained →
    the Services row + health log. Done when N+2 parallel `sleep(5)` requests turn it on and off
  - [x] W0 — Windows compile check on the Mac (owner: local `cargo xwin check --all-targets`
    for src-tauri + cli, not Actions — private repo) ✓ 12 Sep 2026 — `scripts/windows-check.sh`,
    exit code is the verdict; first run RED, 29 error sites (19 src-tauri, 10 cli), now the
    inventory in plan §2.1 (+4 the grep missed: `cfg(unix)` modules used ungated,
    `QUIT_MENU_ID`, ungated `objc2` dev-deps, host-cfg sidecar staging); ledger #582/#583
  - [x] W1 — move Unix-only code behind traits; widen ledger #163's scan to `std::os::unix`
    and `Command::new("kill")`, plant-proven. **Progress 12 Sep 2026:** the app lib and
    binary now COMPILE for Windows (objc2 dev-deps gated, About-menu cfg, MCP transport-only
    gate, `LocalIpc` for the edge admin + MySQL socket probes; `kill` in `core/` moved to
    `ProcessSupervisor`; the last `cfg(unix)` in `core/` — the rewrite's owner-only temp and
    the CLI symlink — moved to `write_private` / a new `ShellRunner::symlink_file`). Ledger #163's scan is widened: `std::os::unix`,
    `cfg(unix)`, `cfg(not(unix))` and `kill` in `core/` production now fail it, plant-proven.
    `cli_server` compiles everywhere too (transport-only gate), and every example now
    compiles for Windows (unix-socket and macOS-only checks print a skip line there).
    The whole `src-tauri` crate — lib, binary, tests, every example — now compiles for
    Windows (test-only OS fixtures live in `src/test_support.rs`). ✓ 12 Sep 2026 — `windows-check: all green` for
    both crates, and the check runs inside `verify.sh` (SKIPPED without the toolchain or
    `XWIN_ACCEPT_LICENSE=1`, ledger #584); commits `48613a7` → the W1-done commit (plan §2.1)
  - [x] W2 — `(os, arch)` binary catalog; Windows pins + checksums; sweep covers them.
    **Progress 12 Sep 2026:** `manifest` was already keyed by `os`; zip extraction landed
    (`Archive::Zip` / `ZipTree`, zip-slip + symlink refusal, ledger #585). The Windows arms
    landed too (Caddy, nginx, PHP 7.4–8.5, MySQL, PostgreSQL, Mailpit, cloudflared — x64
    for every `Arch`, table in PORTS.md), with `.exe` naming and per-OS shapes
    (`shape_of_on`). ✓ 12 Sep 2026 — `manifest_sweep_check` PASS with the
    Windows set: 104 targets answering, Windows 10 re-hashed + 6 HEAD-only and named
    (PostgreSQL's three were pinned from the publisher's `.sha256` alone; 13 Sep 2026 our own
    full downloads matched all three — the sweep keeps them HEAD-only, ledger #335)
  - [x] W3 — Paths, ACL permissions, BinaryProvider, ProcessSupervisor (MySQL + Mailpit
    start, outlive the app, adopt on relaunch — on real Windows); `ensure_free` reads the
    TCP/UDP tables before any trial bind — the bind is kept only as a second refusal, for
    whatever refuses a bind without a table row — and a start counts only when OUR server answers (plan §6).
    **Progress 13 Sep 2026 (compile-checked from the Mac, not run on Windows):** `Paths` →
    `%LOCALAPPDATA%\rexenv\rexenv\data` (the app-data namespace constants now live in
    `platform/mod.rs`, shared with macOS); `PermissionManager` → owner-only ACLs (current user only, no SYSTEM — owner ruling 13 Sep 2026) in
    `platform/windows/acl.rs` (SDDL builder L0-tested on every host, ledger #597);
    `set_executable` checks the file exists — Windows has no execute bit; `BinaryProvider` →
    Mark of the Web stripped, every image x64 or x86 before publish (`platform/windows/pe.rs`),
    and directory trees checked too through the new `prepare_binary_dir` (owner ruling 13 Sep
    2026; macOS no-op). Measured across all 16 Windows artifacts: 1,197 images, 3 x86, none
    refused — an x64-only rule would have refused nginx, PHP 7.4 and MySQL 8.4 (ledger #598).
    **Progress 13 Sep 2026, the port gate + process identity — L0 on every host, plant-proven, and
    ✓ run on the Dell (`windows_port_gate_check: PASS`, 64 checks; its first run failed on a wrong
    excluded-range assumption, now corrected):** `ports::is_free` takes the platform and refuses any port
    `ProcessSupervisor::port_holders` lists before it binds; Windows reads both address
    families' owner-pid tables, names the root holder by image / hosted service / HTTP.sys /
    excluded range with PowerShell commands, and implements `pid_exe`, `pid_command`,
    `pid_alive`, `pids_named`, `owned_listeners`, `owned_master` (pid-reuse-safe parent links),
    `owned_pids` — `platform/windows/process.rs` + `port_table.rs`, ledger #599.
    **✓ 14 Sep 2026, spawn + stop — the "Done when" met on the Dell** (`windows_supervision_check`,
    two SSH sessions: MySQL + Mailpit started through `core`, outlived the launching process and
    its kill-on-close job, adopted as the same pids, MySQL stopped cleanly through its event, ports
    free). Owner rulings 13 Sep 2026: `mysqld --no-monitor` (the default monitor's child survived
    the monitor's termination holding the port); stop = the process's own shutdown channel with a
    10 s grace, else `TerminateProcess`. Services spawn broken away from the launcher's job, with
    no console and every inheritable handle of rexenv's cleared first — the first two Dell runs
    kept their SSH session open because the services inherited ~20 handles the launcher was born
    with from sshd. Ledger #600. ✓ 14 Sep 2026, the rest of W3 on the Dell too
    (`scripts/probes/windows-files-check.sh`): owner-only files refused to `NT AUTHORITY\LOCAL
    SERVICE` with one protected rule each (#597 — and a file made under an elevated token is owned
    by Administrators, recorded); Mark of the Web removed, arm64 and non-PE refused, trees checked,
    and rexenv's own 299 downloaded files carry no mark (#598)
    - [x] **macOS has the same hole — measured 13 Sep 2026, found while writing #599; fixed the
      same day at the owner's go** ✓ `MacosSupervisor::port_holders` from `lsof -Fpn`, local ends
      only, `a_wildcard_tcp_holder_is_busy_on_macos_though_the_bind_says_free` +
      `lsof_holders_are_local_ends_on_the_port_and_nothing_else`, both plant-proven (ledger #599).
      What was measured: On macOS 26.6.2, a holder in another process on `0.0.0.0:<p>` or
      `[::]:<p>` (dual-stack), default options or `SO_REUSEADDR`, does NOT stop a
      `127.0.0.1:<p>` bind made the way Rust's `TcpListener::bind` makes it on Unix
      (`SO_REUSEADDR`) — so `ports::is_free` said free for every high TCP port such a holder
      had. And the bind takes the traffic: after it, a client to `127.0.0.1` reached the new
      socket, not the holder (`::1` still reached a dual-stack holder). A `127.0.0.1` holder
      is refused, and every UDP case is refused. Privileged ports (<1024) use a connect probe
      and are unaffected. The shape is Herd's :443 shadow bind, on rexenv's own gate. Parsed from
      `-Fpn` rather than `-t`, because `-iUDP:<p>` also lists a client whose REMOTE end is the port
  - [x] W4 — php-cgi group (preflight + churn breaker, plan §3 D1(a)) + nginx + SMTP mail → a
    WordPress site serves. **Progress 14 Sep 2026:** D1's measurements on the Dell (plan §3 D1);
    owner rulings on the pool's contents (the 26 official extensions, `PoolModel` named by the
    platform with `core` rendering both, rexenv's own ini); step 1 written — `core/php_cgi.rs`
    (ini, env, args, output-reading preflight), `PhpFpmPools` running both models, the settings
    gate for both, Xdebug refused on the group (ledger #601). **✓ 14 Sep 2026, step 2 — the shared
    nginx on Windows** (measured first: backslashes break quoted nginx paths, the listener is the
    worker, a terminated master leaves its worker serving): every nginx.conf path forward-slashed,
    `binaries::resolve_program` for a program that is a single binary on one OS and a tree on another,
    `owned_master` climbing to the marked master, reload and quit through nginx's own events; Dell
    `windows_nginx_check` PASS (ledger #602). **✓ 14 Sep 2026, the churn breaker** — measured first
    (`windows_cgi_churn_probe`: legitimate load recycled ~30 workers per 10 s at 0.9% parent CPU, a
    worker-killing script 0.6% with the rest answering, a worker that cannot spawn 96.8% and ~1 MB of
    log a second while the port kept serving), then owner rulings (parent CPU ≥ 25% of a core, no
    stop for a worker-killing script, `gave-up` with no restart): `PhpFpmPools::trip_spinning` in the
    watchdog, one output log per minor; Dell `windows_cgi_breaker_check` PASS, the spin stopped on the
    first tick with its `unable to spawn` line quoted (ledger #605). **✓ 14 Sep 2026, W4's "Done when"
    MET on the Dell** (`windows_wp_site_check` PASS, 24 checks, ledger #606): MySQL, Mailpit, the 8.3
    group with the catch on, `sites::provision`, `install_for_site` through php.exe and WP-CLI in 103 s
    with `verify-checksums` passing, `rebuild_configs` and nginx serving the homepage and the login
    form, a password-reset mail from a page and a `wp eval wp_mail()` both in Mailpit, every port free
    after the stops. Found on the way: the site-folder check refused every Windows path (it judged `\`
    across the whole string) — now judged per folder name (#303). **✓ 14 Sep 2026, the pool's health
    as a FastCGI answer** (ledger #607), measured first (`pool_get_values_probe`, Mac php-fpm and Dell
    php-cgi): a pool whose workers are all busy does not answer `GET_VALUES` — a frozen one does not
    either — while its requests all complete; the connections held on its port (`established_on`)
    counted 12 on the Dell (queued included) and 4→10 on the Mac (accepted only, php-fpm ramping).
    Owner ruling: no answer is a miss only while the pool is not busy — built as "no answer AND nothing
    held" (`php::pool_serving`), readiness as the answer alone. `pool_health_check` PASS on both: a busy
    pool survived three polls and its 12 requests completed; on the Mac a frozen pool with nothing held
    was reaped on the second poll. **✓ 14 Sep 2026, the busy-workers display**
    (ledger #608, plan §3 D1(b)): the Services row's sub-line "all 10 workers busy — requests are
    queuing" and a health-log line, from `core::pool_busy` — held connections ≥ the pool's workers for
    two samples in a row, sampled in the status poll. `pool_busy_check` PASS on both: the Dell's group
    read 12 of 10 from the first second (busy at 2 s), the Mac's php-fpm grew one worker a second
    (busy at 9.6 s — which is why the check sleeps 15 s, not the plan's 5), both free at 17 s; the
    WebKit check `poolbusy.js` renders the note in its row, and nothing without it. **✓ 14 Sep 2026, Composer through the
    site's PHP** (ledger #609): Windows had no streamed steps (`spawn_streamed` / `stop_group`) and no
    user environment (`login_shell_env`), so no Laravel or Git site could be provisioned there. Owner
    rulings: the environment read fresh from the registry; a step dies with rexenv. Built as a
    kill-on-close Job Object per step, spawned suspended; `windows/login_env.rs` for the merge.
    `windows_streamed_step_check` PASS on the Dell: a registry value written after launch was seen,
    the step saw only its env, cancel and the idle limit ended the step AND its grandchild, a launcher
    exiting took both with it, and `laravel::create_project` installed Laravel through `php.exe` in 112 s
    - [x] ✓ 14 Sep 2026 — fixed: where the pool model is the php-cgi group, `finish_wp_argv` gives
      WP-CLI `-d SMTP=127.0.0.1 -d smtp_port=11025` and NO `sendmail_path` flag (ledger #407). The
      first fix also sent `-d sendmail_path=`, and real WP-CLI mail vanished with `true` on the Dell:
      PHP takes the SMTP path only when that setting is NULL, and an emptied one is `""`
      (ext/standard/mail.c; the probe's case 5 then measured exactly that). Dell after the fix:
      `windows_wp_site_check` PASS — PHP read inside a real WP-CLI run had the keys and no shim, and
      `wp eval wp_mail()` arrived in Mailpit. **WP-CLI's mail is LOST, silently, on Windows when Mailpit's path has a space** — measured
      14 Sep 2026 on the Dell (`windows_cli_mail_probe`, `php.exe` run with the flags a WP-CLI spawn
      gets, a `mail()` per case, Mailpit's API asked for each subject). It rides `-d
      sendmail_path=<mailpit> sendmail …` from `mail::sendmail_path_cli`, escaped for `/bin/sh`:
      (1) no space — delivered (the doubled `\` is read fine); (2) a space — `mail()` returned
      **`true`**, nothing arrived, and cmd.exe said `'C:\…\rexenv-cli-mail-probe\with\' is not
      recognized as an internal or external command` on stderr only; (3) the path double-quoted
      instead — PHP's `-d` parser dropped the quotes AND joined `mailpit.exe` to `sendmail`, same
      cmd.exe error, `true` again; (4) no sendmail at all, `-d SMTP=127.0.0.1 -d smtp_port=11025` —
      delivered, no shell involved. A Windows user name with a space is common, so this is a real
      user's lost mail with a success reported. The fix the measurement points to: where the pool
      model is the php-cgi group, WP-CLI gets PHP's SMTP keys (4) instead of the shim — the way the
      group's own ini already routes a page's mail (#601) — in `wordpress::finish_wp_argv`, the one
      argv builder (#407)
    - [ ] A LINKED or imported docroot is stored as `canonicalize()` returns it (`sites.rs`, the
      existing-folder validation), and on Windows `std::fs::canonicalize` answers the extended
      `\\?\C:\…` form (Rust's documented behaviour — read, not yet measured) — which
      `services::nginx_path` would render as `//?/C:/…`. Measure what nginx does with it and strip
      the verbatim prefix at that one place (the one-click path, `<sites_dir>\<domain>`, never
      canonicalizes and is not affected)
    - [ ] Per-vhost `PHP_VALUE` does NOT reach a php-cgi group — measured 14 Sep 2026 on the Dell
      (`windows_nginx_check`: `memory_limit=222M` sent per vhost, the child kept the pool's value).
      It is php-fpm's per-request ini; php-cgi has none. Measured who sets one: ONLY the Adminer
      vhost (`sites.rs`, `adminer::import_php_value` — the import upload cap), no user site. So on
      Windows Adminer's import cap is the pool's until another carrier exists (a `.user.ini` in
      Adminer's docroot is the likely one) — small, and not a blocker for serving sites
    - [ ] The site terminal (`commands/terminal.rs`) resolves PHP as a single binary, starts
      `$SHELL`/zsh and writes a shell-script `wp` wrapper — none of which exists on Windows. W7's
      terminals, not W4; WP-CLI and Composer themselves now resolve PHP through
      `binaries::resolve_program`
    - [ ] A plugin's bare curl HTTPS call fails on Windows — PHP's curl has no CA bundle there
      (`unable to get local issuer certificate`, measured 14 Sep 2026; `file_get_contents` works via
      the Windows store, and WordPress/WP-CLI/Composer ship their own bundles). Owner ruled: not now.
      The fix when it comes is a pinned CA bundle for `curl.cainfo` — a new artifact, notices row and
      sweep target — or another route
    - [ ] Download planning (`core/downloads.rs`) names `php-fpm` for every PHP it plans; on a
      php-cgi platform it must plan `php` — found writing W4 step 1, not yet changed
  - [x] W5 — Caddy :443 edge + CurrentUser Root CA trust → valid lock in Edge/Chrome/Firefox.
    ✓ 14 Sep 2026 — Dell `windows_browser_lock_check` PASS in the desktop session (Medium token, the owner
    answering both prompts): Edge 153, Chrome 152 and Firefox 105, each headless on a fresh profile, REJECTED
    the edge's `lockcheck.rex` before the CA was trusted, loaded the page and its same-origin beacon after
    `trust_ca`, and rejected it again after `untrust_ca` (ledger #614). Names resolved by the browsers
    themselves — `.rex` DNS is W6.
    **Progress 14 Sep 2026 — D3's Caddy admin measurement, done first:** the rule stands; Caddy on
    Windows serves its admin API on a unix socket. rexenv's `unix//C:\…` was the bug — Caddy splits an
    address at its first slash, named the socket `/C:\…`, refused to start and its CLI could not dial
    it — and `proxy::admin_address` now gives a path that does not start with `/` one slash, the macOS
    string unchanged (ledger #610). Dell `windows_edge_probe` PASS as written: Caddy up with the socket
    file, Rust reaching the admin API over AF_UNIX (`GET /config/` 200), `:443` serving on the local CA's
    certificate, `:80` → 308, `caddy reload` and `caddy stop` through the socket, `:443` released.
    Recorded for the next steps: the socket file carries its folder's inherited ACL, so `|0600` does
    nothing on Windows; and under the desktop user's token (`windows-edge-bind.ps1`, Medium integrity)
    Caddy binds `:443`/`:80` without elevation, but Caddy's default all-interfaces bind — rexenv's
    Caddyfile today — raised Windows Defender Firewall's "Windows Security Alert" on the desktop, while
    `default_bind 127.0.0.1` raised none. **Ruled (owner, 14 Sep 2026): the Windows edge binds 127.0.0.1
    only**; macOS unchanged. **The edge's Windows start path — done the same day (ledger #611):**
    `prepare_edge` asks `PrivilegeManager::port_needs_privilege` (Windows: false) instead of assuming
    443 is privileged, so the edge is an ordinary child with no LaunchDaemon; the Caddyfile carries
    `default_bind 127.0.0.1` from `EdgeSupervisor::default_bind`; `LocalIpc` dials AF_UNIX. Dell
    `windows_edge_start_check` PASS (28 checks): unprivileged plan, `admin_alive` over AF_UNIX,
    `127.0.0.1:443`/`:80` only and the LAN address refusing, TLS on the local CA, adopt + in-place
    reload by a second manager, a `taskkill /F` crash leaving the socket file and a fresh start over
    it, `stop_all` releasing both ports. **Firefox's profiles root — done the same day (ledger #612):**
    `%APPDATA%\Mozilla\Firefox` when it holds a `profiles.ini`. Found building it: the Dell's
    `profiles.ini` is UTF-16LE with a BOM, which `core::firefox::profiles` read as UTF-8 and so found no
    profile — `ini_text` now reads UTF-8 (± BOM) and UTF-16 (LE/BE, BOM). Dell
    `windows_firefox_profiles_check` PASS (12 checks): the real root and its one profile found, read
    only (hashes unchanged); a byte-copy fixture forced once, idempotent; the installed Firefox 105,
    headless, saved `security.enterprise_roots.enabled` true from rexenv's `user.js` and not in a
    control profile. Not looked in: the Microsoft Store build's virtualized folder (none to measure).
    **`CertTrustManager` — code in, half measured (ledger #613, ◐):** trust/untrust in-process on the
    CurrentUser Root store (`cert_store.rs`), `is_trusted` a prompt-free lookup, a No reads as a cancel.
    Dell `windows_cert_trust_check` read-only PASS; its write phase from the SSH session (owner's go:
    measure both ways) returned `0x32` (ERROR_NOT_SUPPORTED) at once — no hang, no silent add, the
    store's rexenv count 0 before and after — now worded as "no desktop to ask on". **The desktop
    session, the same day** (`windows-cert-trust-desktop.ps1` through `windows-limited-token.sh`, Medium
    token, the owner at the Dell answering Yes twice): PASS — `trust_ca` Ok after 6.6 s and `is_trusted`
    true; a re-trust Ok in 0.0 s; `untrust_ca` Ok and `is_trusted` false; exe, task and folder removed.
    **And every answer, the same evening** (`windows-cert-trust-answers.ps1`, the owner answering No,
    Yes, No, Yes): PASS — install No → the cancel, not trusted; install Yes → trusted; delete No → the
    cancel, still trusted; delete Yes → gone. Windows' prompts, read off the screen by the check: "Security
    Warning" naming "rexenv Local CA" and its SHA-1 thumbprint, and "Root Certificate Store" — "Do you
    want to DELETE the following certificate from the Root Store?". **The three browsers — done the same
    evening** (above, ✓)
  - [x] W6 — DNS agent + NRPT + UAC + logon task → `*.rex` resolves after reboot, app closed
    ✓ **Done 15 Sep 2026 (S5, plan §5 W6):** the real `rexenv.exe` on the Dell — launch installed the task
    and ran in Agent mode; Onboarding's setup made the `.rex` rule ours through UAC and trusted the CA;
    the watchdog kickstarted a killed agent in 6 s; a relaunch with the agent held down served :53
    in-process and handed it back in 21 s; Settings' DNS card read "agent"; rebooted with the app closed,
    `.rex` resolved through Windows' own resolver after logon with no app process
    (`scripts/probes/windows-dns-s5.ps1`; ledger #615/#616/#619 residuals updated)
    **Design ruled 14 Sep 2026** (plan §5 W6: R1 a neutral resolver-route trait, R2 rexenv explains then
    UAC elevates `rexenv.exe`, R3 a foreign `.rex` NRPT rule is taken over backup-first as on macOS, R4
    UDP only). **S1 done the same day (ledger #615):** the resolver's port and socket come from the
    platform — Windows `127.0.0.1:53`, exclusive, UDP resets off; macOS unchanged — and
    `DnsService::start_default` binds first and names only the socket that refused it. Dell
    `windows_dns_agent_check` PASS (20 checks) beside ICS's `0.0.0.0:53`: the agent process answers
    `answers_as_ours` and `Resolve-DnsName`, names its build, cannot be shared by a `SO_REUSEADDR` bind
    (10013 — Windows' default, as the plant showed, not the exclusive option), survives twenty vanishing
    clients (with or without the reset ioctl — also planted); `start_default` beside it and beside a planted loopback
    holder is refused naming that holder's pid. **Found on the way:** the first version of that refusal
    blamed ICS and offered `Stop-Service SharedAccess` — and the check passed it. Also:
    `WindowsDnsAgent::is_installed` answers no, so the Windows app no longer dies at the 20 s handoff.
    **S2 done 15 Sep 2026 (ledger #616):** the agent as the logon task `\rexenv\dns-agent`
    (`windows/logon_task.rs`), registered by the user with no elevation; the keep-alive is a time trigger
    repeating every minute (owner's ruling — `RestartOnFailure` was measured NOT to restart a killed
    action); the agent's output goes to `--log <path>` (`platform::send_output_to`); the trait says
    `definition_path`/`definition_contents`. Dell `windows_dns_agent_task_check` PASS in the desktop
    session (17 checks): install → answering in 0.5 s with its bind line in the log; a repeated install
    kept the same process; kickstart gave a new one; killed, it was back in 6 s; uninstall left no task,
    agent, definition or answer. An interactive-token task needs a logged-on user — the first runs died
    with the Dell's session on critical battery. **S0 + S4, first half, 15 Sep 2026 (ledger #617):**
    `DnsManager` is the neutral route trait (R1) — `route_owner`, `our_route_tlds`, `foreign_route_tlds`,
    `route_label`, `route_contents` and the command builders — the core no longer reads resolver files, macOS
    answers from them as before (`platform/resolver_files.rs`), and Windows answers Absent instead of the
    `resolver_path` panic. NRPT measured the same day (plan §5 W6 S4: immediate effect, the registry
    layout, a non-elevated read). **Second half the same day (ledger #618):** `WindowsDns` reads every NRPT
    rule from the registry and `windows/nrpt_rules.rs` decides ownership (comment `rexenv`, one namespace,
    server `127.0.0.1`) and builds the PowerShell — a takeover takes only its own namespace out of a shared
    rule, and the restore reads the backup file and puts it back into the same rule. Dell
    `windows_nrpt_route_check` PASS (21 checks, test TLDs only, the scripts run with the elevated SSH token):
    install → ours and resolving through Windows' own resolver; uninstall → gone; another tool's
    two-namespace rule → foreign; takeover → ours, their rule (same key) keeping the other namespace;
    `uninstall ; restore` → their rule with both again. **S3 done, 15 Sep 2026 (ledger #619):**
    `PrivilegeManager` on Windows — rexenv's own dialog with the reason, then UAC for `rexenv.exe
    --elevated-step`, which accepts only rexenv's ops (owner's ruling: `WindowsDns` hands over
    `nrpt-install test`, never PowerShell). The ops path measured over SSH (`windows_nrpt_route_check`
    PASS, 21 checks, through the step's own body); in the desktop session with the owner answering
    `windows_uac_step_check` PASS (10 checks): non-op refused with no window, install and remove OK+Yes,
    Cancel and OK+No → the cancel wording with nothing changed. **S5 done the same day** (above)
  - [x] W7 — ShellRunner, autostart, tray (includes the Windows half of the browser-stub row) ✓ 15 Sep 2026
    **Design ruled 15 Sep 2026** (plan §5 W7): a named-pipe single-instance lock in W7 that knows only
    `app.open` (W8 grows the same pipe into the CLI); the tray opens the window on a left click and the menu
    on a right click, with the colour app icon; the words a feature shows ("Show in Explorer", "when you
    sign in to Windows") come from the platform. **Measured first, 15 Sep 2026** (`windows_desktop_probe`):
    a first-instance named pipe refuses a second process's create with error 5 and still takes its connect;
    a junction reads as `is_symlink`, `remove_file` on it is refused, `remove_dir` removes only the link;
    `ShellExecuteW` opens a folder and a URL. **S1 done the same day (ledger #620):** the single-instance
    lock is that pipe — with the real app on the Dell, a second copy from SSH exited in 0.14 s and the
    owner's second double-click left one process, the window in front each time. **S2 done the same day
    (ledger #621):** `open` hands the shell only web links, folders and reading files (owner's ruling —
    `ShellExecuteW` runs a `.bat` by association), `reveal` selects in Explorer; Dell
    `windows_shell_open_check` PASS (7 checks): a `.bat` refused and never run. **S3 done the same day
    (ledger #622):** editors, browsers and terminals detected from what installers register, opened as one
    executable with its arguments; Dell `windows_app_open_check` PASS (11 checks) on its fourth run — the
    first three each passed over a real bug in how a child inherited rexenv's handles. **S4, registry half,
    the same day (ledger #623):** the per-user Run value `"<exe>" --hidden`, Task Manager's disable read and
    cleared by `enable`, `refresh` never moving the item to a dev build; Dell `windows_autostart_check` PASS
    (9 checks). **S4's sign-in and S5 done the same day (ledgers #623, #624)**, with the real app: the Settings
    toggle wrote the Run value; after a sign-out and sign-in Explorer started `rexenv.exe --hidden`, no window;
    Mailpit started from it outlived the app's quit (breakaway allowed from Explorer's launch); the tray gave
    the window back on a left click and opened its menu on a right click. **S6 done the same day (ledger
    #625):** "Link folder" makes a junction (no Developer Mode needed), the delete guard sees it, and
    `remove_symlink` takes only the link; Dell `windows_junction_check` PASS (12 checks). **S7 done the same day
    (ledger #626):** the words for the OS's own things come from the platform ("Show in Explorer", "sign in to
    Windows", Git for Windows) — the real app's Settings read the Windows text on the Dell. **All seven steps
    are in, and the done-when is measured** (plan §5 W7). **Found measuring it (ledger #627, fixed 15 Sep 2026):** the first
    WordPress site on the Dell failed "no binary manifest for php-fpm 8.3.32" — the download plans named
    `php-fpm` on every OS; they now ask `PoolModel::catalog_name()`, and the site `w7check` serves HTTPS 200. **And (ledger #628,
    fixed the same day):** "From Git" said git and node were missing — the core read `PATH` split on `:` for
    a file named `git`; the rules are now the platform's (`Path`, `;`, `PATHEXT`). **Measured on the Dell:**
    PhpStorm opened `w7check` (its `.idea` and recent-projects entry); Firefox, Brave and Edge each showed
    the site's window (Chrome in S3); a clone of `WordPress/classic-editor` through "From Git"; a folder linked as a plugin and deleted from the
    WordPress screen left its checkout byte-identical — after the third fix (ledger #629): "Link folder" had
    offered the whole `C:\…` path as the folder name, because the UI split paths on `/`
    - Found, not fixed: `core/terminal.rs` joins its PATH prepend with `:` — the site terminal on Windows
      needs the same rules when it is ported
  - [x] W8 — `rex` CLI + MCP over named pipes; `rex.exe` on PATH ✓ 16 Sep 2026
    **Design ruled 15 Sep 2026** (plan §5 W8): `rex` computes the app's pipe name itself (the `rex` crate
    gains `sha2`); MCP gets its own pipe, alive only while the toggle is on; install copies `rex.exe` into
    `%LOCALAPPDATA%\rexenv\bin` and adds that folder to the user's `Path`, from the Settings button, refreshed
    each launch. **Measured the same day** (`windows_cli_pipe_probe`, both tokens): `rex` can compute the
    running app's pipe name and reach it; a busy pipe answers 231 and `WaitNamedPipeW` lets a client in once
    freed; progress lines stream in order — and a `rex mcp` bridge on a blocking handle DEADLOCKS (a write
    waits for the other thread's pending read), while a tokio client does not, so the `rex` crate takes
    tokio on Windows only (owner, Q5). **S1 done the same day (ledger #630):** `rex` computes the pipe name
    from `%LOCALAPPDATA%`, waits out a busy pipe, and dials it overlapped; on the Dell, from both tokens,
    `rex open` answered, `rex status` returned the W7 pipe's "does not reach rexenv on Windows yet", and a
    name nothing serves gave "rexenv isn't running", exit 2. #54's guard now reads every dependency table.
    **S2 done the same day (ledger #631), L0:** the unix socket and the lock pipe hand every connection to one
    exchange (`cli_server::serve_connection`), so the pipe serves every `rex` command; W7's app.open-only rule
    is gone. Plants 4/4 — the first run of the client-gone plant PASSED (the test's command finished before
    the write that found its client gone), and the test now keeps working past it. **Measured on the Dell the same day** (the app from `83f9ebe`,
    both tokens): `rex status`, `rex site list` (plain and `--json`), `rex version` and `rex open` answered from
    the running app; a name nothing serves gave exit 2. Not run there: a streaming command (both make a site).
    **S3 done the same day (ledger #632), L0:** on Windows the MCP endpoint is its own owner-only pipe, created
    synchronously before the toggle reads on and gone when it goes off; `rex mcp` dials it, says "endpoint is
    off" (not "isn't running") when the app answers but MCP is off — on macOS too — and at end of input waits
    for its answers, since a pipe cannot be half-closed. Plants 10/10. **Measured on the Dell the same day:** off by
    default → "endpoint is off", exit 2; toggled on → `initialize` and `tools/list` (50 tools) answered from both
    tokens and the bridge ended by itself ~120 ms after stdin closed; a session held open was dropped when the
    owner turned the toggle off, and a new `rex mcp` then said the endpoint is off. **S4 done the same day (ledger
    #633), L0:** the sidecar is looked for as `rex.exe` on Windows (`core::cli::sidecar_file_name`) and
    `scripts/build-cli.sh` stages it on a Windows host. **S5 done (ledger #634), 16 Sep 2026:** Settings → Command-line tool →
    Install copies `rex.exe` into `%LOCALAPPDATA%\rexenv\bin` and adds the folder to the user's `Path` in the
    value's own kind, no prompt; a launch keeps an installed copy current (never installing one, never a dev
    build's). On the Dell the owner clicked Install and a Start-menu PowerShell ran `rex status`; the `Path`
    stayed `REG_SZ` with the ten old entries intact. **S6 measured the rest (below).** What S5
    did not run: the launch refresh (above all over a running `rex mcp`), the off-Path sentence, the teardown.
    **Found by S6, fixed 16 Sep 2026 (ledger #635):** on Windows every launch that ADOPTED a surviving stack
    left the service manager's resolved binaries empty (`adopt_startup` joined `caddy-<v>/caddy`, the file is
    `caddy.exe`), so each reload after it failed "services not started" — `rex site create` on the Dell streamed
    its progress and then failed at "starting to serve". **Run there after the fix:** an adopting launch, then
    `rex site create` → "✓ created", exit 0, and a delete. Also measured the same night: Reinstall puts a removed
    `Path` entry back with the other entries intact, and a launch replaces an older installed copy, renaming the
    old one to `rex.exe.old`. **W8 done, 16 Sep 2026:** with the app quit, the installed `rex` said "rexenv isn't
    running", exit 2; relaunched, `rex status` exited 0 and the launch swept `rex.exe.old` — every clause of the
    done-when measured on the Dell (plan §5 W8).
    - Found, not fixed: `rex`'s stall notice ("no reply yet after 10s — the app is either still working or
      wedged") prints even while progress records are arriving — the timer is not reset by progress; seen on the
      Dell's `site create`, every OS
  - [ ] **W9 screens, LOOKED AT on the Dell 19 Sep 2026** — the app built and run there, every screen
    captured and read. The wording W9 shipped is CORRECT where it landed: Local CA card
    ("trusted · Trusted Root store"), the re-trust sentence, "asks for an administrator's approval
    once", the change-domain dialog ("register it with Windows"), Uninstall ("remove every rexenv
    NRPT rule", no /etc/resolver), "Open rexenv at login" ("sign in to Windows … Task Manager's
    Startup tab"), the AI-agents copy ("administrator password"), every path on Settings and
    SiteDetail, and no dead strip under the OS title bar. **Five things it did NOT cover, found by
    looking:**
    1. Settings → General → Theme: "System follows your **macOS** appearance." — a macOS word on a
       Windows screen, and not in W9's list because nobody had read that card.
    2. Settings → About footer: "Built on open source — nginx, PHP, **MariaDB**, PostgreSQL,
       **Redis**, Mailpit, Adminer & cloudflared." Both are refused on Windows by D4/#642
       (`binaries::ships_on`), so the footer names two things this build does not contain.
    3. Settings → Services: "Installed versions each run a **php-fpm** pool". D1 ruled Windows runs
       a php-cgi GROUP; #651 gave the service ROW a platform label and this sentence was outside
       the surface that fix covered.
    4. SiteDetail → Overview: Config path renders `C:\Users\DELL\rexenv\Sites\w7check.rex**/**wp-config.php`
       — mixed separators, so something joins that one with a literal `/` where Project path above
       it uses `join`.
    5. Import: the card says "rexenv doesn't know where Valet, Herd or Local keep their sites on
       Windows yet, so it didn't look" (#641, correct) while the header above it reads
       "0 found · 0 ready to import · **scanned just now**" — the counter reports an empty scan,
       which is the exact claim #641 exists to avoid.
    **All five fixed the same day, and the scan that should have caught them widened.** `pool_kind`
    ("php-fpm pool" / "php-cgi group"), `bundled_tools` and `path_sep` joined `PlatformWords`; the
    Theme card, both pool sentences, the credits line, the wp-config join and the Import header now
    ask for them. The frontend scan forbids the BARE literal `macOS` (it only knew "with macOS", so
    "your macOS appearance" walked past it) and `php-fpm`, with two named exceptions: a CHIP lookup
    KEY, and the "N macOS system file(s) found" line about `.DS_Store`/`__MACOSX` INSIDE a
    WordPress install, which reach a Windows machine with any repo cloned from a Mac.
    **Widening it found three more nobody had seen on a screen:** Onboarding's "macOS will ask for
    permission (resolver + certificate)", Import's "tells macOS where to send .rex lookups", and a
    second "runs its own php-fpm pool" in the PHP version row. And the new credits test — held
    against `DbEngine::available_on`, not against a reading of D4 — found that the footer never
    named **MySQL** on either OS, the one engine every site gets by default.
    **All five RE-READ on the Dell at `a9d987bf` the same night, on screen:** "System follows your
    **Windows** appearance"; the credits line reads "nginx, PHP, **MySQL**, PostgreSQL, Mailpit,
    Adminer & cloudflared" (MariaDB and Redis gone, MySQL there for the first time); "each run a
    **php-cgi group**"; the config path all backslashes; and Import's subtitle "not scanned here
    yet" with Rescan disabled. Fixed and seen, not fixed and assumed.
    **One thing the rebuild taught, worth its own line:** the dev binary IS the DNS agent, which by
    design outlives the app and is put back by its watchdog — so `cargo build` failed with
    `failed to remove file … rexenv.exe: Access is denied (os error 5)` even with the app closed,
    and `Get-Process rexenv | Stop-Process` was not enough because the agent came straight back.
    The kill and the link have to happen in one breath (`taskkill /F /IM rexenv.exe /T` then build).
    **Second pass, 19 Sep 2026 — the screens the first pass had NOT read**: Services, Databases,
    Mail, Tunnels, the site's WordPress / Database / Logs / Terminal tabs, and the New-site dialog.
    Clean, and worth naming because they are what W10 and #651 promised: Services lists
    **PHP-CGI 8.2 / 8.3** and only MySQL + PostgreSQL, the DNS resolver row reads **:53 Running**
    (and it was there after I had killed it, so the watchdog works), the New-site dialog says
    "FrankenPHP and Apache aren't part of rexenv on Windows yet" and "MariaDB isn't part of rexenv
    on Windows yet", and the Logs tab's empty state names the Windows path and says exactly why it
    is empty. **Three things it found:**
    1. **`rex` terminal, `repo`, Adminer-verify and MCP scratch were all DEAD on Windows** — six
       callers asked `binaries::resolve` (the FILE resolver) for `"php"`, which is a ZipTree there,
       so every one failed with "php is a directory distribution — use resolve_dir". The site's
       Terminal tab rendered that sentence — a FUNCTION NAME — to the user. `resolve_program`
       already existed for exactly this and says so in its own doc comment, so the bug was six
       callers reaching past it, not a missing capability. Fixed, plus a scan (ledger #687) that
       fails on the seventh: plant-proven by putting `terminal.rs` back.
       **Fixing it uncovered two more layers underneath, which is the case for looking rather than
       reasoning** (ledger #688): with the resolver right, the tab then said
       `spawn shell: CreateProcessW '"/bin/zsh -l"' … The system cannot find the path specified` —
       the shell came from `$SHELL`, a unix convention, falling back to `/bin/zsh`. And behind
       THAT, the PATH list was joined with `:` (on Windows `C:\php` and everything after it is one
       unusable entry) and the line typed into the fresh shell was POSIX `export` syntax, which
       PowerShell shows as an error at the prompt. `ShellRunner::interactive_shell` now has no
       default — a default is macOS's answer wearing the trait's name — and the PATH halves take
       the os. **Proven live the same night:** the tab opens a PowerShell prompt in the site's
       folder and `php -v` answers **PHP 8.2.32 (cli) … Visual C++ 2019 x64** — the SITE's version,
       not the default 8.3, so the prepended PATH and the per-site PHP are both right.
       **And then `wp --version` found a fourth layer** (ledger #689): the wrapper was an
       extensionless `wp` holding `#!/bin/sh`, so Windows raised "How do you want to open this
       file?" — Internet Explorer among the choices — and returned nothing. It is `wp.cmd` now, and
       `wp --version` answers **WP-CLI 2.12.0** — measured with the stale extensionless `wp` still
       beside it, which PowerShell ignored. **Four layers, one screen, all found by opening the tab
       and typing two commands.** Still owed: a wp-cli command that touches the database (the site
       was stopped), and a shell whose own PowerShell profile reorders PATH.
    2. A **stopped** site's WordPress tab shows "Loading plugins…" for as long as you leave it —
       no error, no "start the site first". Still open (row below).
    3. A **stopped** site's Database tab shows the Adminer URL above a blank white frame — Adminer
       is not running and nothing says so. Still open (row below).
    **Onboarding read too, 19 Sep 2026 — all four screens, without resetting anything.** The gate
    is `!resolverInstalled || !caTrusted`, real system state, so "resetting first-run" would have
    meant untrusting this machine's CA or dropping its NRPT rule. `/onboarding` is its own route and
    the gate only runs at `/`, so the app was pointed at that path instead: a temporary `devUrl` on
    the Dell's checkout plus a static server with an SPA fallback. Nothing on that machine changed,
    and the checkout was reverted (`git status` clean) and rebuilt afterwards.
    All four are clean, and screen 3 is where tonight's fix shows: "rexenv adds a private
    certificate authority to **Windows**" and "**Windows** will ask for permission (resolver +
    certificate)" — it read "macOS will ask" this morning. Screen 2 lists Caddy, Nginx, MySQL 8.4,
    Mailpit, Adminer and PHP 8.2/8.3 with no MariaDB or Redis (D4), and screen 4's port-443 warning
    reads "Another app is answering HTTPS on **this PC**". The button that makes the system changes
    was NOT pressed.
    **How it was looked at, for the next time:** an SSH session is not the interactive desktop —
    `CopyFromScreen` there saves a blank image — so both the app launch and every capture ran
    through `schtasks /run … /IT`, which executes in the logged-on session. Clicks were driven from
    the screenshots and the click script REFUSES unless the foreground window is rexenv's own; it
    refused once, correctly, when a PowerShell window took focus.
  - [x] W9 — frontend on WebView2 (Windows paths, Ctrl shortcuts, fonts) ✓ 19 Sep 2026 — the row's
    own closing condition was a look at the Windows wording on the Dell (Settings' Local CA card,
    Onboarding's first screen, SiteDetail's domain copy, the teardown confirmation). All four were
    read that day, along with every other screen the app has; what the looking found is in the row
    above, fixed and re-read on the same machine. What remains from that pass is tracked as its own
    rows — a stopped site's WordPress and Database tabs, and the Valet/Herd/Local import port.
    **macOS words on the Windows screens** (seen in the real app on the Dell, W6 S5, 15 Sep 2026): Settings
    "trusted · login keychain" and "Local CA re-trusted in your login keychain." (`Settings.tsx:945`, `:856`),
    "Reinstall rexenv's certificate authority in your system keychain." (`:1012`), "asks for your password
    once" (`:1294` — Windows asks rexenv's dialog then UAC); SiteDetail "register it with macOS"
    (`SiteDetail.tsx:1544`); Onboarding "to your Mac" (`Onboarding.tsx:342`); and the app log's "launched at
    login — staying in the menu bar" (`lib.rs` `first_window_decision`). The mechanism exists since W7 S7
    (`platform/words.rs`, ledger #626): the login item's description ("sign in to your Mac", `:1354`), every
    "Show in Finder", the editor fallback's Finder and the tool install hints already come from it — these rows
    join it. **Also Rust's** (read 16 Sep 2026): `core/proxy.rs`'s "is answering HTTPS on this Mac",
    `core/dns.rs`'s "so .{tld} sites open on this Mac", `core/app_update.rs`'s "needs a newer macOS than this
    Mac's" and "this Mac's macOS version could not be read" (seen in the Dell's own log), the five
    "(macOS will also ask for your password)" tool descriptions in `mcp_server/user_sites.rs`, and
    `traits.rs`'s "macOS refused the operation". **And the UI's macOS paths**: Settings' two "/Applications and
    ~/Applications" tooltips and its `~/rexenv/Sites` one, the teardown's `/etc/resolver` sentences, Import's
    `~/.config/valet` and `/etc/resolver/test`, AgentsMcpCard's `~/.cursor/mcp.json`.
    **Design ruled 16 Sep 2026** (plan §5 W9), after measuring on the Dell with the owner: keep Windows' own
    title bar and drop the sidebar's reserved traffic-light row; turn WebView2's browser accelerator keys off
    (Ctrl+R reloads the app today) through `with_webview`; fonts need no work — Inter, JetBrains Mono and
    Space Grotesk are bundled and render there (screenshot).
    **S1 and S2 done 16 Sep 2026 (ledgers #636, #637), L0:** the reserved traffic-light row is behind
    `PlatformWords::window_controls_in_content` (macOS true, Windows false), and the Windows webview is asked
    for `SetAreBrowserAcceleratorKeysEnabled(false)` — `webview2-com` and `windows` became direct Windows
    dependencies at wry's own versions. Plants 6/6. **Measured on the Dell the same day:** Ctrl+R, F5, Ctrl+P, Ctrl+F
    and F12 now do nothing, while Ctrl+A/C/V/Z still work in a field; and the wordmark sits ~14px higher —
    the dead strip under Windows' title bar is gone. **S3a done the same day (ledger #638), L0:** the trust store, the first
    privileged prompt, the OS's name, what a CA is added to, and an agent consent line's note are platform
    words now — Settings' Local CA card, its TLD copy, SiteDetail's domain dialog, Onboarding's first screen
    and the five MCP descriptions. Plants 5/5; the frontend scan widened to "keychain", "your Mac" and "with
    macOS". **S3b done the same day (ledger #639), L0:** `host`, `trayHome` and `appSearch`
    joined the words, and a scan over BOTH Rust crates now forbids a macOS literal in any sentence — three
    named exceptions, each with its reason. Plants 6/6. **S4 done the same day (ledger #640), L0:**
    `routesLabel`, `homePrefix` and `importSearch` joined the words, the teardown sentences stopped promising
    `/etc/resolver`, and the frontend scan now forbids `~/` and `/etc/resolver` outright. Plants 5/5.
    **S5 done the same day:** `docs/DESIGN.md` has a Windows section — the OS's own title bar
    (and why fake traffic lights are the dishonesty the rules forbid), the browser accelerator keys, the OS's
    nouns coming from the platform, and the three bundled type families. **Left to close W9:** the owner's
    look on the Dell at the Windows wording (Settings' Local CA card, Onboarding's first screen, SiteDetail's
    domain copy, the teardown confirmation) — the build will be staged for them. **Ruled by the owner 16 Sep 2026: say so now, port later** — done the same day
    (ledger #641, L0): `importsOtherTools` is false on Windows, the scan skips both discoveries there, and
    the card says rexenv didn't look rather than that nothing is there. Plants 4/4. The port itself stays
    open. **Also ruled:** `app_update`'s "this Mac's macOS version" wording stays macOS-only until the
    Windows updater (W11), which is why the Rust scan names it as an exception
  - [x] W10 — Redis/Apache/Xdebug/MariaDB (per D4) refused in core with an honest message ✓ 16 Sep 2026
    **Core half done 16 Sep 2026 (ledger #642), L0:** one predicate — `binaries::ships_on` — answers
    "is this pinned on this OS", and the four gates ask it: `DbEngine::available_on` (Redis and
    MariaDB out on Windows, which removes them from the Databases page, Services' rows, the port
    list, adoption, the download plan and the MCP context at once), `ensure_server_available_on`
    (Apache and FrankenPHP), `xdebug_status_on` (a fourth verdict, `NotOnThisOs`, with its own
    sentence that offers no way out because there is none), and `spawn_db` (the one start path a
    site's stored engine reaches without passing the filter). Every gate takes the os as a
    parameter, so both answers are measurable from the Mac. Plants 6/6.
    **UI half done the same day (ledger #643), L0:** the picker's options come from
    `sites::offered_web_servers` — the gate that refuses — through one new command, so on Windows Apache
    and FrankenPHP are absent rather than offered-then-refused, and a note says why. Plants 3/3.
    **The CLI half IS measured on Windows now** (16 Sep 2026, on the Dell, against the
    deployed build): `rex db versions` lists only MySQL and PostgreSQL, `rex status` shows no Redis or
    MariaDB row, and `rex site create` refuses both `--db mariadb` and `--server apache` by name. That
    run is also what found the hole #647 fixed — creation had no engine gate at all, so a MariaDB site
    was created on Windows and exited 0. **The UI half is written too** (ledger #648, the same day): the engine picker now
    reads the offered set from core, exactly as the server picker does, so Windows lists no MariaDB and
    says why. **SEEN on Windows 16 Sep 2026** (owner at the keyboard, three screenshots, on the
    build that carries it): Services lists MySQL and PostgreSQL only — no Redis, no MariaDB — and Nginx
    and Caddy only; the New Site dialog offers Nginx alone and MySQL/PostgreSQL/None, each with the note
    naming Windows; Settings → DNS & SSL reads "trusted · Trusted Root store", "Reinstall rexenv's
    certificate authority in your Trusted Root store" and "asks for an administrator's approval once".
    The CLI agrees (`rex db versions`, and both create refusals by name). **Ticked 16 Sep 2026:** the last of the four, Xdebug's, was read
    on the Dell — SiteDetail → Settings → Xdebug says "Xdebug isn't part of rexenv on Windows yet — its
    builds have to match each PHP version's compiler exactly, so they are pinned per version and none is
    pinned here", which is #642's `NotOnThisOs` sentence. The three W9 screens were seen the same day
    (domain change, the teardown confirmation with its card, and Import).
    **Found by looking:** the PHP rows say "PHP-FPM 8.2/8.3" on a machine with no php-fpm — its own row
    above.
  - [ ] W11 — NSIS installer, Windows release job, updater, winget. **UNBLOCKED 19 Sep 2026:
    the owner ruled D5 — unsigned, question closed.** rexenv is open source and earns nothing, so
    it spends nothing; the same answer macOS got, for the same reason. Authenticode is out of the
    row, not deferred in it. **Not affected:** the updater's signed manifest, which is rexenv's own
    Ed25519 key over `latest.json` — free, already how macOS ships, and a different sense of the
    word "signed" than the certificate this declines. **The D5 measurement survives as a W11
    OUTPUT, not a prerequisite:** every Windows user meets SmartScreen, so `docs/INSTALL.md` owes
    them the verbatim wording, the click count per browser, whether "Run anyway" is reachable
    without "More info", and whether a new hash repeats it — measured once the installer exists.
    **First installer BUILT 19 Sep 2026** — `bundle.windows` filled with two keys and nothing else:
    `nsis.installMode: "currentUser"` (D5's per-user, no-admin direction) and
    `webviewInstallMode: downloadBootstrapper` (the smallest; Win11 ships WebView2 and Win10 22H2
    usually has it through Windows Update). `bundle.targets` is left at `"all"` on purpose — the
    Windows release passes `--bundles nsis`, the way `release-mac.sh` already forwards its flags,
    so no MSI is produced: WiX is per-machine and wants admin, which is the opposite of what D5
    ruled. `npx tauri build --bundles nsis` on the Dell: 19 m 19 s release build, then makensis →
    **`rexenv_0.7.0_x64-setup.exe`, 9.7 MB**, `Get-AuthenticodeSignature` = `NotSigned` (as ruled).
    The `rex` sidecar is in it — `build-cli.sh`'s MINGW arm ran as `beforeBuildCommand` and Tauri
    put `rex.exe` beside the app, which is where `core::cli::bundled_rex` looks.
    **First user-facing measurement, 19 Sep 2026** (owner double-clicked it in Explorer, from
    Downloads, carrying a real `Zone.Identifier` — `ZoneId=3` and a GitHub release URL, the exact
    flag a browser writes): Windows showed **"Open File - Security Warning"** — "The publisher
    could not be verified", **Publisher: Unknown Publisher**, and **[Run] [Cancel] with Run on the
    first screen**. No "More info" to find first. That answers D5's item (3); the verbatim text is
    in `docs/INSTALL.md`. Cancelled, not run — the install path is still unmeasured.
    **What that measurement CANNOT say, and nearly said anyway:** SmartScreen's own "Windows
    protected your PC" never appeared, and the reason is not that an unsigned rexenv escapes it —
    the Dell has `HKLM\SOFTWARE\Policies\Microsoft\Windows\System\EnableSmartScreen = 0`.
    Measuring there and writing "no SmartScreen dialog" would have been a comfortable false
    answer, which is the fixture-shaped-like-production trap in its purest form: the MACHINE was
    the fixture. That half moves to the clean Windows 11 VM pass (W12), where the defaults are the
    defaults, and SMOKE-TEST now asks for it by name.
    **Release path built 19 Sep 2026** — `scripts/release-windows.sh` (`pnpm release:win`) and
    `scripts/release-windows-check.sh`, mirroring the macOS pair. The wrapper earns its existence
    for the Windows reason: the DNS agent IS the app's binary, outlives the app and is restored by
    its watchdog, so the link step dies with `Access is denied (os error 5)` naming nothing — it
    kills by image name and then PROVES the file is writable before starting a 20-minute build.
    **Installed and run for the first time, 19 Sep 2026** — the clean Windows 11 VM pass (W12) took
    the D5 outputs: SmartScreen verbatim ("Windows protected your PC", Run anyway behind More info,
    no "Open File" dialog behind it), Edge's shelf wording, the per-user layout and HKCU entry, no
    UAC in the wizard — all in `docs/INSTALL.md`. Two bugs only an installed copy could show, fixed
    the same day: the Finish page's "Run rexenv" confines the app in a job that forbids breakaway,
    so no service could start (#692, the one-hop guard); and `edge_wire`'s 500 ms wait read Windows'
    ~2 s loopback refusal as a foreign proxy (#691). The fixed build is not yet installed — the
    "second build" SmartScreen half and the Finish-page start with the guard are W12's next run.
    It builds `--bundles nsis` only, in the script rather than by narrowing `bundle.targets`,
    which would have changed the macOS build for a Windows reason. The check is §A0's Windows
    half: one installer named for the version, the `rex` sidecar present, the PE machine field
    read off the header rather than the filename, the embedded `Dist_Archive_Command`, and
    `NotSigned` asserted — because `docs/INSTALL.md` promises a specific "Unknown Publisher"
    dialog and that page is a lie the day a certificate appears silently. **Measured on the Dell
    against the real artefact: all green; plants 2/2** (sidecar removed → named; a second
    installer → named). The `release-windows` job attaches to the draft the macOS job creates —
    one release, both platforms — and runs the SAME `verify.sh`, because a Windows job that runs
    less is ledger #684's mistake in a new place. **The job itself has never executed** (the repo
    is private, so neither has the macOS one), which is why the substance is in the scripts.
    `actionlint` is clean.
    **Updater design corrected by measurement, 19 Sep 2026.** The plan assumed "a running `.exe`
    cannot be replaced", so the swap would have to happen after exit, from a relauncher. Measured
    on the Dell: delete and overwrite are refused, but **rename away and move-in both succeed**
    while the old process keeps running from the renamed file — and the renamed file cannot be
    deleted until it exits. That is exactly `AppBundle::swap`'s existing contract ("nothing is
    deleted on any path"), so Windows gets the SAME swap macOS has and the relauncher shrinks to
    what it is on macOS: the thing that restarts, not the thing that swaps.
    **winget answered 19 Sep 2026, from Microsoft's own docs** (read, not submitted — the real
    proof is a merged PR): **no Authenticode requirement exists.** Nothing in the submission
    requirements, the contributing guide or the validation troubleshooting makes signing a
    condition. What the pipeline's "SmartScreen validation" checks is the **URL's reputation**,
    not the binary's signature — `winget-pkgs-submission-test/Troubleshoot.md`: *"SmartScreen
    validation errors indicate that the URL you provided has a bad reputation"* — and a GitHub
    release URL has none of that problem. "Binary validation" beside it is static analysis, hash
    and malware scanning: *"Binary validation errors indicate that the installer failed static
    analysis"*. So an unsigned installer is submittable; the cost stays where D5 already put it,
    on the user meeting SmartScreen on first run, with reputation accruing per hash.
    **Still open:** whether the WebView2 bootstrapper stays admin-free under a currentUser install
    (the Dell already HAS WebView2, so that machine cannot answer it — the clean Windows 11 pass
    can).
    **Updater, the app side, landed 19 Sep 2026 (ledger #690).** `AppBundle` for Windows:
    facts (the install directory is the bundle; kind off `%LOCALAPPDATA%\rexenv` vs Program
    Files vs a cargo `target` dir; writability by a real probe file; owner by SID; the volume's
    read-only flag and free space; no symlink or junction on the way), stage (the guarded zip
    extractor, then the executable's OWN `VERSIONINFO` for version and `ProductName`, the PE
    machine field, the `rex.exe` sidecar, and `uninstall.exe` copied across from the installed
    directory), a rename-pair directory swap with restore, the HKCU uninstall entry's
    `DisplayVersion` rewritten, a sweep that classifies by the version inside, and a relauncher
    that waits on the parent's process handle after checking its creation-time token. The
    trait's vocabulary was made platform-neutral first (`InstallKind::ProgramsPerUser` /
    `ProgramFiles`, neutral docs on `BundleFacts` / `StagedExpect`), chosen over a Windows
    bolt-on because Linux comes next; `core::app_update` grew `staged_expect_on(os)`,
    `Refusal::PerMachineInstall` and per-OS descriptor URLs. **Proven:** L0 on the Mac (rules,
    core), Windows-target clippy. **NOT proven, and the next three things in order:**
    (1) ✓ the release produces `rexenv_<v>_x64.zip` — the install directory's contents, flat,
    without `uninstall.exe` — and `release-windows-check.sh` extracts it and re-checks PE x64,
    the embedded payload, the update key, and the executable's own `ProductVersion`/`ProductName`
    (the words the swap verifies); the workflow attaches it; `check-app-manifest.sh --windows`
    reads the second document. (2) `rexenv/runtimes` does not yet publish
    `app-manifest-windows.json` + `.sig` — the owner's repo and key. **What that publisher must
    do is now written down line by line** (`docs/PLAN-windows-port.md` D5: its own document and
    serial, the `_x64.zip` asset, no macOS floor, the flat-zip shape check, the same key and
    environment) — **merged 19 Sep 2026: https://github.com/rexenv/runtimes/pull/3**
    (`fd4aeebe`; `--windows` mode + a `windows` workflow input; shellcheck/actionlint clean, the
    key tripwire verified on that path, the flat-zip shape branch exercised both ways). **Not yet
    RUN**, and cannot be until a published release carries `rexenv_<v>_x64.zip` — then
    `dry_run: true, windows: true` first, as for macOS; (3) ✓
    `windows_app_bundle_swap_check` (sandbox tier, Windows): **24 checks green on the Dell** —
    stage from a flat zip, verify off VERSIONINFO and the PE header (the version read back
    through PowerShell as an independent oracle), `uninstall.exe` carried across, the directory
    swapped **under a running process** (`RenamePair`), every refusal leaving the installed
    directory byte-identical, the sweep keeping the previous directory until told the app is
    healthy. First run failed one leg for a FIXTURE reason worth writing down: `timeout.exe`
    exits at once when stdin is redirected, so the "still running" waiter is `ping -n 60`.
    **The relaunch is measured too** — `windows_app_relaunch_check`, 6 checks green on the Dell:
    the real app binary in relauncher mode starts nothing while its parent lives, starts the
    bundle once the parent is gone (`WaitForSingleObject` on the process handle), and treats a
    pid whose creation-time token does not match as already gone. **Both owed legs measured 21 Sep
    2026 on the VM** (SMOKE-TEST run 6): the published 0.8.4 → 0.8.5 swapped the real
    `%LOCALAPPDATA%\rexenv`, and with a live share the restart went THROUGH the quit gate ("Stop
    sharing?" → Quit) and reopened once on 0.8.5. Two things it found, both small:
    - [ ] The update card says **"You chose to keep rexenv running, so the new version is waiting"**
      while the quit gate's "Stop sharing?" is still on screen, unanswered — the card read the
      restart call's "not quitting (yet)" as the user's answer. Only "Keep sharing" should put it
      there; while the gate is open it should say nothing new.
    - [ ] **Uninstalling from Apps & Features without the in-app step leaves a live agent.** Measured
      21 Sep 2026 on the VM (SMOKE-TEST run 6): `uninstall.exe` removed everything it owns except
      `rexenv.exe`, because the `\rexenv\dns-agent` task — per-user, and left registered — re-ran the
      agent from it within the minute and held the file. Result: an uninstalled app whose resolver
      still answers `127.0.0.1:53` every minute, forever. INSTALL.md says "in-app step FIRST", but
      Apps & Features is where people uninstall. The task is the user's own, so the uninstaller can
      end and delete it without UAC (a Tauri NSIS `installerHooks` pre-uninstall: `schtasks /End` +
      `/Delete /TN \rexenv\dns-agent /F`, then stop the agent) — NRPT and the CA still need the
      in-app step, and the uninstaller should keep saying so.
    - [ ] `%LOCALAPPDATA%\rexenv\rexenv-0.8.3.bak` (38 MB, 19 Sep) sits beside the app for good: a
      pre-#695 swap left it, and the launch sweep looks only for `.rexenv-update-*`. Sweep the old
      name once, or say why not.
    **winget, 19 Sep 2026:** `scripts/winget-manifest.sh` renders the version, installer and
    locale manifests from the PUBLISHED asset (downloaded and hashed, the API digest cross-checked),
    or from a local installer with `--local` before a release exists. Rendered from the first
    installer on both the Dell and the Mac — identical sha — and **`winget validate` on the Dell:
    "Manifest validation succeeded"**. **SUBMITTED 20 Sep 2026 for 0.8.3 —
    https://github.com/microsoft/winget-pkgs/pull/437674** (fork + branch + the three files through
    the API; winget-pkgs is too big to clone for three YAMLs). One thing had to be fixed first and
    it would have failed their URL validation, not ours: `PackageUrl`, `PublisherSupportUrl` and
    `LicenseUrl` named `rexenv/rexenv`, which is PRIVATE — 404 to everyone. The generator now names
    the public tap and omits `LicenseUrl` (the `License` field still says Apache-2.0), so a rendered
    manifest cannot carry a URL the world cannot open.
    - [ ] When `rexenv/rexenv` goes public, point `PackageUrl`/`PublisherSupportUrl` back at it and
      restore `LicenseUrl` (`scripts/winget-manifest.sh`).
    - [ ] Watch PR 437674 through their automated validation, and answer whatever it asks for.
  - [ ] W12 — launch gates: verify on the Windows runner, SMOKE-TEST + INSTALL Windows
    sections, clean Windows 11 VM pass
    **✓ verify on the Windows RUNNER — green 21 Sep 2026**, `.github/workflows/windows-verify.yml`
    run 35565978803 on `92fe2c2a`: `verify: all green` in 57.0 min (cold cache — the first run
    that succeeded, so the rust-cache is saved from here). It took seven fixes to get there, and
    every one was something that spelled macOS's answer or assumed a developer's machine: two
    tests (`sudo chown`, a `:`-joined PATH), the notices check's macOS graph (`--offline` on a
    Windows host) and its `pnpm` (`pnpm.cmd`), pnpm 11 pinned in CI vs 12.4.2 everywhere else
    (112 vs 127 npm packages), the lockfile that declaration left behind, and `status.py`
    running the WSL launcher as `bash` on the runner. Timings measured so far: runner 52–58 min
    cold (four runs), the Dell 17.3 min warm.
    **verify.sh RAN on Windows for the first time, 21 Sep 2026** (Git Bash, the Dell): 1334
    passed, 2 failed — both TESTS that asserted macOS's answer (`$ sudo chown` where Windows
    says `takeown /R /F`; a `:`-joined PATH where Windows uses `;`). Neither could fail on a
    Mac. Fixed by asking the platform instead of spelling one OS's words; re-run on both.
    **VM pass, run 1 — 19 Sep 2026** (Windows 11 Pro 24H2 26100.4349, ARM under UTM, x64 build
    emulated; `rexenv_0.7.0_x64-setup.exe` at `276fc7cb`): SMOKE's Windows section is ticked row
    by row with what was seen. Passed: SmartScreen path, per-user install, no UAC, onboarding's
    four steps, the resolver pre-dialog + one UAC + Windows' certificate dialog, NRPT rule and
    CurrentUser Root, six binaries pinned, tray menu, window-close keeps the app, Quit leaves the
    agent + 16 service processes, `hosts` untouched, `rex doctor` all green but PATH. Found: #692
    (Finish-page copy cannot start services), #691 (false "Another app is answering HTTPS"), the
    "this Mac's macOS" sentence in a Windows log (neutral now). **Still open from run 1:**
    - [x] run 2 with the guard build, 19 Sep 2026 (`46ccefed`, installed over run 1's copy):
      second-hash SmartScreen repeats verbatim ✓; the Finish-page copy hops (parent = a fresh
      `explorer.exe`) and Start all runs 5/5 ✓; `rex` on the Path ✓; Settings → Services →
      Uninstall (UAC + Windows' "Root Certificate Store" DELETE dialog) leaves NRPT 0 / CA 0 /
      task gone / data kept ✓; `uninstall.exe` removes exe, HKCU entry, both shortcuts ✓. Rows in
      `docs/SMOKE-TEST.md`. Still unrun: the in-app update rows (need a published newer build).
    - [x] ✓ 19 Sep 2026 — `src/main.tsx` suppresses it outside inputs, editable regions and the
      terminal. WebView2's DEFAULT context menu (Back / Refresh / Save as / Print / More tools) shows
      inside the app on Windows — seen run 2 in Settings. macOS's webview shows none; this one
      offers "Save as" and "Print" of the app's own UI. Suppress it (`ICoreWebView2Settings::
      AreDefaultContextMenusEnabled`, or the `contextmenu` event) — an honest-UI item.
    - [x] ✓ 19 Sep 2026 — `remove_symlink_best_effort` removes the folder when empty. `uninstall.exe` leaves an EMPTY `%LOCALAPPDATA%\rexenv\bin` behind (the `rex` copy's
      folder, made by the Settings install, removed by the Settings uninstall — the directory
      itself is never deleted). Remove it when the copy goes.
    - [x] rexenv/rexenv#1 — `ADMINER_VERSION` re-pinned to **6.1.0** ✓ 21 Sep 2026, once the
      owner published runtimes serial 6 (manifest carries 6.1.0). The digest is the signed
      manifest's, checked against the release file itself before it landed. Three test
      fixtures had spelled a version out — the URL assertion and two catalog rows where
      "newer than the pin" was a literal — so the re-pin broke them; all three derive from
      `ADMINER_VERSION` now. ~~`ADMINER_VERSION` is eight releases behind the manifest (5.4.2 vs
      6.0.2 published, 6.1.0 next): re-pin to 6.1.0 + its sha256 AFTER runtimes serial 6 is
      published (rexenv/runtimes#4 unblocks that publish). The issue's other half — the pins
      script explaining the wrong drift direction — is done 19 Sep 2026 (`check-php-pins.sh`
      names both directions).~~
    - [x] **The DNS agent's scheduled task went missing, and the watchdog could only say so.**
      ✓ 20 Sep 2026 — #704, proven on the VM: task deleted by hand + agent killed → the
      watchdog's next tick registered it again from rexenv's own definition, the agent came
      back and `lm.rex` resolved again. **The kickstart now RE-REGISTERS a missing task** from
      rexenv's own copy of the definition, so the watchdog heals it instead of logging at it.
      **The first suspect was measured and cleared:** a reinstall over a RUNNING copy leaves the
      task registered — it kills the AGENT, and the per-minute task brings it back within the
      minute (`.rex` resolved again 60s later on the VM). What actually removed it is still
      unknown; the healing makes it survivable either way — and a machine that loses it now
      repairs itself within a watchdog tick instead of losing `.rex` at the next boot.
      ORIGINAL REPORT ——
      Seen on the VM 20 Sep 2026 in `health.log`: `[restart-failed] DNS: resolver agent kickstart
      failed: could not restart rexenv's DNS agent task (\rexenv\dns-agent): ERROR: The system
      cannot find the file specified.` The task had existed (the agent had been serving); what
      removed it is unknown — an update, an uninstall/reinstall over the top, or a Windows
      cleanup. Two halves: find out what removes it (reinstall over a running copy is the first
      suspect, since that is what this VM had just done), and make the kickstart RE-CREATE a task
      that is gone rather than fail at it — the agent is what makes `.rex` resolve, so a missing
      task is a dead TLD at the next boot.
    - [x] ✓ 21 Sep 2026, seen on the Dell (Win10 22H2, installer `7388104…`, app running): the
      MessageBox reads the new sentence; OK → Setup completed → Finish reopened the app, and the
      DNS agent came back on its own (SMOKE-TEST Windows run-2 block). **Code:** `src-tauri/nsis/English.nsh` (wired
      through `customLanguageFiles`) replaces the three running-app strings — `appRunningOkKill`
      now says rexenv is running as "the app, or the small background resolver that keeps your
      .rex sites answering", that OK stops it, and that the resolver comes back by itself. The
      other 24 strings are Tauri's verbatim. The first build FAILED on a double BOM (Tauri
      prepends its own; the file brought one too) — ASCII and BOM-less now; the Dell's build
      then went §A0 green.
      ORIGINAL — The installer's "rexenv is running! Click OK to kill it" names the DNS agent as if it
      were the app (measured run 2). Correct as far as it goes — the task restarts the agent on
      the new binary — but a sentence that says so would spare the user the guess.
    - [x] ✓ 21 Sep 2026, proven on the VM with the fixed build — a 200 ms window watcher saw NO
      rexenv console through Start all (PostgreSQL via `pg_ctl`), a share, a WordPress create +
      delete, Tunnels and Adminer on PostgreSQL (ledger #705 ✅). Ships in the next release.
      **Console windows from every helper spawn (#705), found 21 Sep 2026 on the VM:** Start
      all left an empty Windows Terminal titled `…\pg_ctl.exe` (hosting PostgreSQL — closing it
      stops the database), Tunnels left one titled `…\php.exe`. Fixed in code: `platform::command`
      (`CREATE_NO_WINDOW` on Windows) for all 22 helper spawns in `core/` + a source-scan guard.
    - [ ] Chrome's download wording (no Chrome on the VM); the certificate dialog's **No** path;
      WebView2's `downloadBootstrapper` on a machine without it (the VM had 153).
    - [x] ✓ 19 Sep 2026, display only — the Services row already carried its platform label (#651);
      now `rex status` reads `PHP-CGI 8.3` on Windows too (`pool_display_name`); the key stays
      `PHP-FPM 8.3` on every OS. `rex status` names the pool `PHP-FPM 8.3` on Windows, where the pool is `php-cgi`
      (`WebTarget::Pool::label`, `ports.rs`'s `PHP-FPM` service name) — the frontend's
      `pool_kind` words never reached the CLI/service labels.
    - [x] ✓ 21 Sep 2026, explained on the Dell — not the edge's fault, two separate curl facts
      (INSTALL.md §Windows "Verify it works"): `SEC_E_ILLEGAL_MESSAGE` is Caddy's TLS alert for
      an SNI it has no certificate for — `adminer.rex` was not a site there, and macOS's curl
      gives `tlsv1 alert internal error` for the same request; a real site through the agent's
      DNS failed instead with `CRYPT_E_NO_REVOCATION_CHECK` (schannel demands a revocation
      check a local-CA leaf cannot offer) and answered 200 with `--ssl-no-revoke`, as did
      `-k --resolve`. rexenv's own probes are rustls and PHP's curl is OpenSSL, so neither
      meets it. ORIGINAL — `curl.exe` (schannel) fails the handshake against the edge with `SEC_E_ILLEGAL_MESSAGE`
      even with `-k --resolve adminer.rex:443:127.0.0.1`, while the app's own probe reads `Ours`
      and `rex doctor` says "answering as rexenv on :443" — unexplained; a browser visit to a
      real site is the row that settles whether it is curl's or the edge's.
    **The example half is done 16 Sep 2026 (ledger #645):** the tier table grew an os column
    (`both`/`macos`/`windows`, enforced like the tier itself), the runner skips what this host
    cannot run and says how many, and a skip is printed beside the verdict rather than folded
    into it. 184 examples: 88 both, 69 macOS-only, 27 Windows-only. Plants 4/4. **Script side surveyed 16 Sep 2026** (read, not run):
    four portable-by-construction fixes landed here — `verify.sh`'s `mktemp` template, `status.py`'s
    utf-8/LF/`bash`-invoked helpers, and a `.gitattributes` with `eol=lf` so a Windows checkout's
    CRLF cannot be read as content by the doc gates. **The host was then MEASURED 17 Sep 2026**
    (Git Bash 5.2, MINGW64) and half the survey's list was guesswork: `xxd` is present, OpenSSL
    3.2.1 signs Ed25519 with `pkeyutl -rawin`, and `jq` is not a dependency (every `--jq` is
    `gh`'s). Really broken, and fixed: `verify-receipt.sh`'s `shasum` (absent; `sha256sum` present
    — now resolved once and loud when neither is, where it used to degrade to an empty
    fingerprint), `check-app-manifest-test.sh`'s `file://` fixtures (native curl resolves a POSIX
    path against the drive root and fetches nothing — silently green), `build.rs`'s macOS-only
    sidecar staging, and `windows-check.sh`'s inert SKIP on the one machine that could check
    natively. **The bulk is not scripts:** no Rust test had ever RUN on a Windows host —
    `windows-check.sh` proves `cargo check` only. **That run happened 17 Sep 2026, and this is its
    result: `1232 passed; 79 failed` of 1311** (plus the `cli` crate's 23, all green). It needed no
    toolchain install — rustup, MSVC Build Tools 2022 and the SDK were already on the Dell — and the
    tree went over as a `git bundle`. Two fixes were needed before a test could even start: the
    sidecar staging (ledger #652) and an application manifest for the test binary, which must come
    from the test invocation because a `build.rs` link-arg breaks the app binary (ledger #653).
    **The 79 are six families, and the code is right in most of them:** symlink fixtures (22, one
    helper: `test_support::symlink`), test paths spelled with `/` where production uses `join`
    (~10), fixtures spawning `yes`/`sleep`/`sh` (~10; `yes` never exits, so two tests hang), tests
    demanding macOS-pinned artifacts (~10, e.g. a `php-fpm` pin on an OS with no php-fpm),
    macOS-only tool scans (~6) and macOS home paths (2). Owner's ruling 17 Sep: work them family by
    family, largest first, one commit each, each measured on the Dell. **Also found, and worse than
    a red test:** `state/store.rs`'s SQL scan compared paths against `"src/state/"` and found ZERO
    hits on Windows — it failed only because it carries a canary. Scans without one (`mcp_server/
    user_sites.rs`, `mcp_server/scratch.rs`) would pass while seeing nothing.
    **Worked down to ZERO the same day, twelve runs on the Dell:** 79 → 51 → 44 → 30 → 22 → 13 →
    12 → 9 → 2 → **0** — `cbfdf1b6`, **1307 passed, 0 failed in 26.48 s**, the first fully green
    Windows suite this port has had. One commit per family, each measured there; ledger #652–#673.
    **Not one was a claim that fails on Windows:** every single failure was a fixture asserting a
    value it spelled itself where the code derives it, or a helper whose Windows arm was missing.
    **The last two took four commits between them and are the ones worth reading:** under `cmd /c`
    every QUOTED path form is refused ("the filename, directory name, or volume label syntax is
    incorrect") while the unquoted form works — and the script still exits 0, because `&` runs
    `echo Success` regardless, so it wore the disguise of a run that succeeded and produced nothing
    (#672). Fixing that exposed a hang behind it: both archive stubs' cancel was a no-op on Windows,
    first via `/bin/kill` and then via `platform::current().supervisor().stop_group`, which returns
    `Ok(())` for a pid with no job object — and a stub-spawned child never has one. Runs ten and
    eleven parked on it and were ended by hand; `taskkill /T /F` is what reaps the detached
    grandchild (#673). **The `cli` crate is measured there too, the same day: 23 passed, 0 failed**
    (`cbfdf1b6`) — and the number is worth writing down rather than repeating, because the Mac runs
    **25**: two tests are gated off Windows, so "23, all green" means all of the ones that run, not
    all of them. The two are named rather than counted, so the next reader can check the arithmetic:
    `soft_request_gives_up_on_a_listener_that_accepts_and_never_answers` and
    `soft_request_returns_the_data_when_the_app_answers`, both `#[cfg(unix)]` — the unix-socket
    transport, which is D3's open item on Windows, so their absence is the port's shape and not an
    oversight. **Left:** verify.sh's own Windows run, the SMOKE-TEST and INSTALL Windows
    sections (D6 rules the floor: Windows 11 x64 supported, 10 22H2 best-effort, arm64 unsupported;
    D5 ruled unsigned 19 Sep 2026, so the SmartScreen wording is a measurement the installer owes,
    not a decision anyone is waiting on),
    and the clean Windows 11 VM pass. Node 24.21.0 + npm/npx 11.19.1 + pnpm 12.4.2 and python3
    3.12.10 are installed on the Dell ✓ 17 Sep 2026.
    **The SMOKE-TEST and INSTALL Windows sections are WRITTEN ✓ 17 Sep 2026 (ledger #674)**,
    both sourced from code, rulings and Dell measurements rather than from the macOS pages
    reworded: INSTALL nests the two OSes symmetrically and SMOKE-TEST names which of its
    sections D4 excludes, which are replaced (install, first run, tray, DNS, uninstall) and
    which carry over. They are UNRUN by a human on Windows — that run is what ticks them.
    Two things are deliberately absent: SmartScreen's verbatim wording (nobody has downloaded an
    unsigned rexenv installer yet, because there is no installer yet — D5 is ruled, so this is now
    a thing to MEASURE rather than a thing to decide) and any claim that the
    update card hides its button — `AppUpdateCard` has no OS gate; `WindowsAppBundle::facts`
    returns `Unported(...)`, so readiness errors instead of refusing, and the checklist asks
    the tester to look at what the card actually renders.
    **Second per-gate run, 19 Sep 2026 at `847f47ec`:** `cargo test --lib` **1313 passed, 0 failed**
    (141 s including the relink) and the `cli` crate **25 passed, 0 failed** — the Mac's 27 minus the
    two `#[cfg(unix)]` socket tests, so the arithmetic still names its own gap. `doc-counts`,
    `check-app-manifest-test` and `status.py --check` green. **`ledger-tally` came back rc=1 with an
    EMPTY log**, and the cause was not the machine but the SHELL: Git Bash's grep in a UTF-8 locale
    does not match a pattern outside the BMP, so the 🔨 and 🚫 counts went to zero, and
    `set -euo pipefail` plus `grep -c`'s exit-1-on-no-match killed the script mid-assignment before
    it could say which. Measured both ways on the same file (rc=1 under `LC_ALL=en_US.UTF-8`, rc=0
    and correct under `LC_ALL=C`), fixed with `LC_ALL=C` and a tolerated zero, ledger #686 — which
    also corrects #681's "ledger-tally rc=0 on the Dell": that run used a non-login shell, which
    never loaded `LANG` from the profile. **Re-run at `61250118` with the fix in: all six gates
    rc=0, the whole run in 110 s** (test-lib 87 s of it, cached). That is the restructured loop
    doing what it was for — the Dell answers the one question the Mac cannot (do these tests PASS
    on Windows) in under two minutes, instead of five hours spent relinking 184 examples to ask it.
    **verify.sh on the Dell, 18 Sep 2026 (ledger #675):** three runs capped inside `cargo build
    --examples` (1 h, 2 h, 5 h — 184 examples each linking the whole lib; the third finished its
    last example 19 s before the cap), then the fourth, all cached, reached clippy in an hour and
    gave the bar's **first real Windows verdict: red, `rc=101`**. Nine lib findings — five
    Windows-dead items, one real MSRV bug (`is_none_or` in `platform/windows/autostart.rs`,
    stable 1.82 vs declared 1.77.2), three that are clippy-1.98-only and not Windows at all — then
    70 more in the examples once the lib was clean. All fixed; `cargo xwin clippy --all-targets
    -- -D warnings` from the Mac now exits 0, which is the first time that check has been runnable
    from here. **The structural finding:** `windows-check.sh` runs `cargo check` with no clippy and
    no `-D warnings`, so the Mac's Windows gate is strictly weaker than the macOS clippy gate and
    could never have caught any of this — closing that is its own row below. **An intermittent, measured
    18 Sep and explained 27 Sep 2026:** with the clippy fixes in, `cargo test --lib` on the Dell failed
    `idle_watchdog_kills_a_silent_step_and_reports_the_stall` twice in ~7 full-suite runs with
    `tail: []` — the `cmd` child closed both pipes before printing a line — while passing 6/6 alone;
    release run 36329787548 (windows-latest) failed it again with `exit=Some(1) … lines=[] tail=[]`.
    Cause: #600's handle sweep ran before EACH spawn, and a sweep on one thread clears the
    child-stdio pipe ends another thread's spawn has just made (`std`'s spawn lock is private);
    that child starts with no stdio. Stressor on the Dell: 0/400 bad alone, 119/400 beside a
    sweeping thread. The sweep now runs once at process start (`sweep_inheritable_handles_before_boot`).
    **Ruled 18 Sep 2026 (owner): the Dell runs ONLY what the Mac cannot.** Three verify.sh
    runs had capped inside `cargo build --examples` (1h, 2h, 5h) and a fourth reached clippy
    only because nothing had changed -- 184 examples each relink the whole lib, ~5h on that
    laptop after ANY lib change, and the Mac's `cargo xwin clippy --all-targets` proves the
    same "they compile for Windows" in 16 s. So the bar is not run whole there. The Dell
    loop is now per gate, each with its own log and verdict line the moment it finishes
    (`scripts/probes/`-style runner, `gates/summary.txt`): `cargo test --lib`, `cli` tests,
    ledger-tally, doc-counts, check-app-manifest-test, status -- **~50 s warm, green on
    `720465ae`: 1311/0, 23/0, every script gate rc=0.** notices-check is NOT a Dell gate:
    it filters the macOS dependency graph and needs darwin-only crates cached offline
    (`base64 0.21.7` was the first miss). **One real portability bug found by it:**
    `status.py`'s `sh()` used `text=True`, which decodes with the locale -- cp1252 on
    Windows -- and `ledger-tally.sh`'s UTF-8 tally marks killed the reader thread; the
    earlier "status.py utf-8 helpers" fix (17 Sep) had missed that one call. Now
    `encoding="utf-8"` there and in notices-check.py. **Left:** the SMOKE-TEST and INSTALL
    Windows sections run by a human on Windows; `windows-check.sh` running clippy (row
    below); the clean Windows 11 VM pass.
  - [x] `windows-check.sh` runs clippy with `-D warnings` for the Windows target instead of a bare
    `cargo check` ✓ 18 Sep 2026 (ledger #684) — the gap #675 measured: nine lib and 70 example reds
    the Mac-side gate passed over, because `check` does not lint and nothing denied warnings. Both
    arms changed (cross and native-host), `clippy` added to the required-toolchain list. Plant-proven:
    dropping the `cfg(target_os = "macos")` from `CHECK_UPDATES_MENU_ID` turns it RED and names
    `src/lib.rs:2314`, while the macOS clippy on that same tree stays rc=0 — the finding is reachable
    only from this gate. Warm cost **26 s**
  - [x] `build.rs` picks its `rex` sidecar marker by HOST cfg ✓ 18 Sep 2026 — the marker now comes
    from `TARGET`, and staging runs only when `TARGET == HOST`: `build-cli.sh` compiles for the machine
    it runs on, so invoking it for a foreign target would stage the WRONG binary under the right name.
    A cross build names the file it lacks and who stages it instead of failing mutely at `tauri_build`.
    Plant-proven (ledger #685): `cargo xwin check` with no placeholder prints
    `no rex sidecar for x86_64-pc-windows-msvc …` and skips `build-cli.sh`

- [ ] **Linux launch** — 24 Sep 2026, owner: "ami chai ekhon rexenv er linux version o build
  korte … at least Ubuntu". Reasoning, measurements, the decisions D-L1–D-L10 (defaults
  assumed overnight, owner rules in the morning) and "Done when" per task:
  `docs/PLAN-linux-port.md`. Windows's W1/W2/W6 did the groundwork, so this IS "fill the stubs".
  - [x] L0 `scripts/linux-check.sh` in `verify.sh` ✓ 24 Sep 2026 — Ubuntu 22.04 container
    (ledger #719); first run RED on 4 dead-code sites, green with L1
  - [x] L1 `platform/linux/` filled ✓ 24 Sep 2026 — no `todo!()` (ledger #716); DNS = resolved
    drop-in (#717, **P1 unmeasured**), edge = systemd system unit (#718), DNS agent = user unit,
    `pkexec`, `/proc`+`ss`, NSS + system CA trust, XDG autostart, `words::LINUX`
  - [x] L2 Linux arms in the binary catalog ✓ 24 Sep 2026 — both archs, every digest from a full
    streamed download (ledger #720); MySQL `.tar.xz` → `Archive::TarXzTree` + Linux-only `xz2`;
    PHP 7.4 has no static Linux build (not in v1); nginx from jirutka's static builds until
    `rexenv/runtimes` publishes a Linux one. `manifest_sweep_check` RAN 25 Sep 2026 — 188 targets, and it CAUGHT
    three wrong first-pass digests (truncated streams), now corrected and re-run green
  - [x] L3 build plumbing ✓ 24 Sep 2026 — `build-cli.sh` Linux arm (host triple from `rustc -vV`;
    ran in the Ubuntu container: `rex-aarch64-unknown-linux-gnu` staged), `bundle.linux` (deb
    depends incl. `libaio1 | libaio1t64`, `libnuma1`), `scripts/release-linux.sh` + `pnpm release:linux`
    (never run — needs an Ubuntu host)
  - [x] L4 the app shell ✓ 24 Sep 2026 — the Linux tray takes the colour icon (ledger #721); the
    tunnel guard, activation and the About menu are macOS-only by design and Linux falls to the
    documented defaults (plan §1.1). The tunnel guard landed 25 Sep 2026 as a pidfd watcher
    (ledger #722 — NOT `PR_SET_PDEATHSIG`, which binds to the spawning thread), and the polkit
    action file gives the deb's prompt rexenv's own sentence (#723)
  - [x] L5 docs ✓ 24 Sep 2026 — SMOKE-TEST Linux section (P1–P5, nothing run), INSTALL, RELEASING
    3b, PORTS Linux table, README tree. `notices-check.py` walks the Linux graph too (25 Sep, #724)
  - [ ] L6 the VM run — **the VM exists (24 Sep 2026: Ubuntu 22.04 arm64 in UTM, cloud image +
    cloud-init seed, `ssh rexenv@192.168.64.7`) and P1 RAN: the first DNS design FAILED (a global
    resolved drop-in routed `example.com` to loopback), the dummy-link replacement passed by hand
    and then 23/23 through `linux_dns_route_check` (ledger #717). First `.deb` + AppImage built
    on the VM (`tauri build`, 15 MB deb, polkit action inside). **The installed deb RAN through
    onboarding, Start all and a WordPress site over HTTPS (curl 200, Chromium no warning) — P1/P2/P3
    through the app, P5 partly; found and fixed: snap Chromium's NSS db (#726), thread-summed RAM
    (#725), `rex`'s Linux socket (#727).** P4 ✓ after a reboot; the rebuilt deb re-proved #725/#727; PHP 7.4's Install button on Linux
    is #728 (fixed). P2's restart/disable legs ✓, the tunnel guard ✓ (owner-approved test tunnel). Owed: Firefox (snap)**
  - [x] L7 **Linux self-update (owner ruled IN, 24 Sep 2026)** ✓ 25 Sep 2026 — `LinuxAppBundle`
    (deb: `dpkg-deb` checks, then `dpkg -i` in the polkit step; AppImage: `--print-version`, then
    `renameat2` exchange), one descriptor per kind+arch (`app-manifest-linux-<deb|appimage>-<arch>.json`),
    relauncher on a pidfd (ledger #729, #730). **`linux_app_swap_check` PASS 16/16 on the VM** beside the
    installed deb (fixture package installed by the platform's own command and removed; Tauri's deb
    lists members WITHOUT `./` — caught by the real 0.8.7 deb, would have refused every update).
    `check-app-manifest.sh --linux`. Publisher: rexenv/runtimes PR #6 (`--linux <kind> <arch>`) — the
    owner merges and RUNS it; until a Linux descriptor is published, no Linux build is offered anything
    (SMOKE-TEST's Linux "In-app update" rows are the About-screen proof still owed)
  - [x] L8 **PHP 7.4 for Linux (owner ruled IN, 24 Sep 2026)** ✓ 25 Sep 2026 — owner published
    `php-7.4.33-7`; the four Linux tarballs + two licence tarballs pinned on that tag
    (`PHP_7_4_33_LINUX_TAG`, macOS stays on `-6`), #728's refusal gone by the same table; L0 tests
    flipped; **L1 `php_versions_check` on the VM: 7.4.33 cli + fpm downloaded from `php-7.4.33-7`, digests
    verified, `PHP 7.4.33 (cli) (built: Sep 24 2026)` runs, `licenses/` beside it** (the example's macOS-only
    spellings — `arm64` for an ELF, "self-distributed" resolved for macOS — were the run's only failures and
    are fixed: `artifact_is_self_distributed` asks the host's OS). **Through the rebuilt deb on the VM:
    `rex php install 7.4` ✓, `PHP-FPM 7.4 running` on 9774, `php74.rex` serves 7.4.33 over HTTPS.** Built in `rexenv/runtimes` like the
    macOS 7.4 (cli + fpm, both archs, licences). **History:**
    rexenv/runtimes PR #7 adds the `ubuntu-24.04` / `ubuntu-24.04-arm` lanes (static musl, ldd/file
    gates). **Dry run 36002401583: all four lanes green** (aarch64 needed `-fPIC -fPIE`; the licence sweep
    needed freetype's `LICENSE.TXT` and a SQLite public-domain note; the static ELFs export no Zend
    symbols, so 7.4 on Linux gets no Xdebug — not in v1 anyway). Then: owner merges PR #7 and publishes
    `php-7.4.33-7`; pin the Linux arms in `binaries.rs` (`php_self_hosted_tag`), drop #728's refusal for 7.4
  - [ ] **Release = GitHub Actions only, all three OSes at once** (owner ruling 27 Sep 2026,
    `docs/PLAN-ci-release.md`) — `release.yml` rewritten (macos-14 / windows-latest / ubuntu-22.04 /
    ubuntu-22.04-arm → one draft on the tap with eight assets, or nothing). **Owed:** the owner's
    `TAP_TOKEN` secret and the arm64 runner decision; the first tag run (it has never executed end to end)
  - [ ] L9 the release leg — `release-linux-check.sh` (§A0-linux) + `release.yml`'s x86_64
    `release-linux` job landed 25 Sep 2026; **VM (aarch64) `pnpm release:linux` exit 0 — §A0-linux all
    green on the rebuilt 0.8.7 deb + AppImage (`--print-version` answers, polkit action, Depends, rex
    sidecar), and that deb installed and runs**; **x86_64 built 25 Sep 2026 on the Dell's WSL2 Ubuntu 26.04**
    (`pnpm release:linux` §A0-linux all green: amd64 deb 14 MB + AppImage 89 MB; the deb installed —
    after `pkexec | policykit-1`, 26.04 has no `policykit-1` — `linux_app_swap_check` PASS,
    `php_versions_check` exit 0 with the x86_64 7.4 from `php-7.4.33-7`). 26.04 is not the 22.04 floor,
    so that deb is a proof, not a release artefact. **The FLOOR build exists (27 Sep 2026): `linux-build.yml`
    on `ubuntu-22.04`, run 36264369017 — §A0-linux all green, `rexenv`/`rex` ask for glibc ≤ 2.34 (asserted
    off the binary, 22.04 has 2.35), `rexenv_0.8.7_amd64.deb` + `.AppImage` as the run artifact.**
    `release.yml`'s Linux job moved to 22.04 too; it still has never run on a tag. **That floor deb RAN
    on a fresh WSL Ubuntu 22.04.5 x86_64 (27 Sep 2026): apt-installed, onboarding, root edge, WordPress on
    MySQL + Laravel on PostgreSQL both 200 over HTTPS — the x86_64 22.04 run-test the row was missing.**
    **27 Sep: the rebuilt amd64 deb ran the whole thing on the Dell — GUI onboarding under WSLg, root edge,
    MySQL (after the `libaio.so.1t64` shim, #731), a WordPress site at `https://acme.rex` → 200; the stale
    system-store CA behind a green "trusted" is #732; PostgreSQL needed the same shim for `libxml2.so.2 → .16`
    and its whole dynamic closure declared in the deb — then a Laravel site on postgres served.** The
    download page is the tap release + website, outside this repo

- [ ] **The keychain (CA trust) dialog is rexenv's too** — 12 Sep 2026, owner, after the admin
  dialog got its name: the CA trust dialog still read "security". Measured first: wrapping
  `security` in the rexenv applet does NOT change the title; calling the trust API
  in-process does (ledger #579).
  - [x] T1 — `MacosCertTrust` calls `SecCertificateAddToKeychain` +
    `SecTrustSettingsSet/RemoveTrustSettings` itself ✓ 12 Sep 2026 — 4 L0 incl.
    `a_dismissed_dialog_reads_as_a_cancel_never_a_status_code`; example
    `cert_trust_prompt_check` (system tier) written
  - [x] T2 — `cert_trust_prompt_check` live ✓ 12 Sep 2026 — PASS, owner answering: Cancel →
    "Keychain permission was cancelled — …"; approve → trusted; approve untrust →
    untrusted; all on a spawned thread; throwaway CA removed by its SHA-1 (#579)
  - [ ] T3 — the title from the REAL app: in a packaged build (the installed
    `/Applications/rexenv.app` predates this), Settings → re-trust the CA and read the
    dialog — "rexenv" + logo. Rides SMOKE first-run; #579 stays ◐ until then

- [ ] **In-app self-update — a dmg user has no update path at all**
  — 6 Sep 2026, planned in `docs/archive/PLAN-self-update.md`; supersedes the Phase 4+ row
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
    `docs/archive/PLAN-self-update.md` §T0 with the raw results; §6/§7/#517 rewritten to match.
    Found on the way: App Management does not protect an ad-hoc bundle **at all** —
    in-place writes into the launched bundle succeeded too, which is a note for
    `docs/SIGNING.md`'s case rather than a change to the design.
  - [x] T1 — `core/app_update.rs` trust core, the shared verify seam, `macho::archs`
    ✓ 6 Sep 2026 — ledger #518–#522; 15 L0 tests incl.
    `both_manifest_modules_verify_through_one_seam` (one flipped byte, both documents
    must refuse) and `archs_reads_a_fat_header_and_a_thin_one_and_refuses_everything_else`;
    six settings keys ruled (the signed trio Denied); `updates.rs`'s 17 tests untouched.
  - [x] T2 — transport, the launch ride, the 6 h poller, check commands, minimal card
    ✓ 6 Sep 2026 — ledger #523/#524; `fetch_signed_pair` shared with the PHP manifest
    (one client, one deadline); `app_update_state`/`app_update_check` + their wrappers
    and the About card; `auto_check_is_read_before_the_request_not_applied_to_the_answer`
    (source scan, plant-proven); `examples/app_update_check.rs` (network) PASSES against
    the real URL and reports the pre-publish state honestly. No install button yet — T6.
  - [x] T3 — the 12th platform trait `AppBundle` (facts, pre-flights, stage, swap, sweep)
    ✓ 6 Sep 2026 — ledger #525–#528; `renamex_np(RENAME_SWAP)` proven on a fixture bundle
    by `examples/app_bundle_swap_check.rs` (sandbox, 20 checks, reports `AtomicSwap`); the
    refusal legs assert the installed bundle is byte-identical afterwards; the thin-bundle
    leg builds its own `lipo -thin` fixture so it always runs. Trait count 11 → 12 in
    CLAUDE.md, ARCHITECTURE, MAP and the README tree; windows/linux `todo!()`.
  - [x] T4 — apply end to end: refusals, hub download, swap, helper, exit through the gate
    ✓ 6 Sep 2026 — ledger #529–#532; `app_update_apply` (GUI only, source-scanned) ends in
    `app.exit(0)` so the quit gate runs, and the relauncher is spawned in `RunEvent::Exit`
    AFTER the gate agreed; `examples/app_relaunch_check.rs` (sandbox) proves the ordering
    against the REAL app binary and is plant-proven. **Deviation from the plan, recorded in
    §7 R16**: an apply does not refuse a busy app — an update IS a quit, and refusing would
    be stricter than Cmd+Q for the same consequence.
  - [x] T5 — the DNS agent states its build; a stale one is kickstarted at launch
    ✓ 6 Sep 2026 — ledger #533; a TXT answer on `_build.rexenv-agent.rex` and
    `agent_is_stale` (silence reads as stale, which is what every pre-T5 agent does);
    fixes the manual drag-replace case too. An L1 was MEASURED as buying nothing and
    deliberately not written (`run_agent` is `serve_udp` in a loop, already L0-driven);
    the launchd re-exec half is a SMOKE leg.
  - [x] T6 — the full About card, the one-source consent sentence, copy guard, L2 probe
    ✓ 6 Sep 2026 — ledger #534; `AppUpdateCard.tsx` with skip/undo, hub-driven progress,
    the Homebrew line and the refusal-as-a-command; `UpdateWatch` toast once per version;
    a Settings nav badge; two L0 copy guards (both plant-proven); six `appupdate-*` L2
    scenarios in `uireview.js` — **not** a separate `appupdate.js`, following the Adminer
    card's precedent, recorded in TESTING §L2.
  - [x] T7 — the tray item and the app-menu "Check for Updates…"
    ✓ 6 Sep 2026 — ledger #535; `TrayAction::UpdateTo` from an in-process snapshot
    (`the_tray_reads_the_offer_from_a_snapshot_never_the_network`, a source scan), the item
    OPENS the card and the tray's must-not list gains the apply; five tray tests extended
    plus `the_update_item_is_present_iff_the_model_offers_a_version`; the app menu gains
    "Check for Updates…" under About, which navigates AND re-checks.
  - [x] T8 — `rex status` / MCP read field, with no new dispatch arm
    ✓ 6 Sep 2026 — ledger #536; the `status` payload and `stack_status` gain `update` from
    the in-process snapshot; `rex update` recorded in CLI-ROADMAP as deliberately NOT built;
    `every_field_the_status_arm_emits_is_rendered_by_rex_status` binds the two ends a crate
    apart (plant-proven). Owed: a `cli_socket_check` run once the app is next restarted —
    a running OLD app answers without the field, which is expected skew, not a bug.
  - [x] T9 — release flow: the tar.gz asset, the version guard, §A0, and the WRONG docs
    ✓ 6 Sep 2026 — ledger #537; `check-versions.sh` (the guard CI has and never ran here —
    plant-proven), `release-assets.sh` (archive + layout asserts + §A0 on the EXTRACTED
    bundle, green against the real 0.5.0 build, layout guard plant-proven),
    `check-app-manifest.sh` (openssl verification proven against a locally-signed fixture);
    `release.yml` mirrors both. RELEASING steps 1/3/4/5 rewritten and 7/8 added,
    PUBLISH-TESTING §A0-b + §M + summary rows, INSTALL §Updating rewritten. ✓ **the first full `pnpm release:mac`
    ran end to end 7 Sep 2026** on the 0.6.0 build: dmg + archive produced, `release-assets:
    all green`, §A0 green by hand on both the built and the extracted bundle.
  - [x] T10 — the runtimes publisher + the tap's `auto_updates true` (other repos) ✓ 7 Sep
    2026: `rexenv/runtimes` branch `app-self-update` (`publish-app-manifest.sh` + its
    reviewer-gated `workflow_dispatch` workflow + `rexenv/runtimes/docs/APP-MANIFEST.md`), `rexenv/homebrew-tap`
    branch `app-self-update` (`auto_updates true`, README rewritten, `brew style` clean).
    Both on BRANCHES; neither `main` touched, the signing workflow never run. Refusals proven
    live: v0.5.0 has no archive and is refused with the reason; a throwaway key is refused
    before any download. Ledger #538 (🚫 posture — the guard is in other repos). Also fixed
    two things found on the way: both publishers now really refuse an unpinned key (`MANIFEST.md`
    §4 had claimed that for months), and the automatic-check setting finally has a GUI toggle
    (it was `rex config`-only while a Rust comment described "the toggle"). **Owed: merging
    both branches — a human gate, and T11 is when the whole chain runs for real.**
  - [ ] T11 — the first real in-app update on a real Mac — **RAN 7 Sep 2026, 0.6.0 → 0.6.1;
    open only for the legs nobody watched**:
    both releases built, published, cask-bumped and descriptor-signed (serials 1 and 2), and
    the update applied from Settings → About. Measured after: version 0.6.1, cdhash changed
    (`e7036221…` → `ce45c7a5…`), codesign valid, no quarantine, still universal,
    `rex 0.6.1 (f748698)` agreeing with About, DNS agent re-execed from the NEW bundle
    (pid 43710 → 47967) with `.rex` still resolving, leftovers swept, and every stack service
    still running. `rex status` prints the offer line (#536 live). Full record:
    `docs/PUBLISH-TESTING.md` §A 0.6.1.
    **Found by running it:** `auto_updates` does NOT stop `brew upgrade --cask rexenv` —
    naming a cask is an explicit request Homebrew honours, and on a stale tap checkout it
    silently put 0.6.0 back over the self-updated 0.6.1. Three docs said otherwise and all
    three were ours; corrected in the cask comment, the tap README (`626d1df`) and
    `docs/RELEASING.md`, and the SMOKE leg now names which command to run.
    **Ran a SECOND time** after the brew downgrade, and the owner reports **no dialog on
    either update**, with the TCC log recording no prompt — which closes the two legs that
    most needed a human (Gatekeeper/App Management on the replaced bundle; which permissions
    come back), plus services-and-DNS-survive-it, leftover cleanup, and the unnamed
    `brew upgrade` leaving rexenv alone. Five SMOKE boxes ticked with evidence.
    **§M found a bug, which is the point of a human gate**: with a tunnel up, Install →
    "Keep sharing" installed the update and then said NOTHING — the card offered Install
    again, as if it had not happened. The plan specified that state and it was never built.
    Fixed 7 Sep 2026, ledger #539 (read from the notice row the swap writes, so it survives
    the window closing; suppresses the offer and clears the tray), L0 + L2, plant-proven.
    Tray item, no-click offer and the startup notice (`rexenv 0.6.1`) all confirmed good.
    The offline leg found a SECOND bug: "Check now" with no network spun past 40 seconds
    and never returned — reqwest's `Client::timeout` did not fire, the same trap `binaries.rs`
    already knew as B34 and this seam did not inherit. Fixed with an enforced 12-second
    deadline over the whole pair, covering the PHP manifest too since they share the seam;
    ledger #540, plant-proven (without it the L0 hangs past 45s).
    **Still owed:** "dark when current" as the CARD renders it, the consent sentence's
    Homebrew line, and a re-run of BOTH fixed legs — public-share and offline — against the
    fixes, which wants the next release.
  - [x] T12 — archive the plan as a design record ✓ 7 Sep 2026: `git mv` to
    `docs/archive/PLAN-self-update.md`, its Status line rewritten to say what IS proven
    (26 L0 in the module, four examples, seven L2 states, both publishers' refusals) and
    what is not (the chain end to end), the archive README row and the CLAUDE.md router
    parenthetical, and all 19 citing files repointed. Archived ahead of T11 deliberately —
    the code is finished, and leaving a plan "in flight" for weeks over a gate that needs a
    release to exist makes `status.py` lie about what is being worked on.

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
- [x] **Windows/Linux: `detect_browsers`/`open_in_browser` are the default empty
  stubs** ✓ both halves filled. Windows W7 S3, 15 Sep 2026 (ledger #622: the registry, Chrome
  flagged default on the Dell, private windows by flag). Linux 24 Sep 2026 (`platform/linux/desktop.rs`,
  L1): found on `PATH` (deb, snap, Toolbox names), the default from `xdg-settings`'s `.desktop` id
  (the snap's `firefox_firefox.desktop` included), private windows by the same flags as macOS —
  **unmeasured on a desktop** (SMOKE-TEST Linux P5).

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

- [x] **PUBLISH-TESTING §B** — uninstall removes the root :443 daemon (live launchd). ✓ 23 Sep
  2026, clean-15 smoke on the UTM VM (macOS 15.8, dmg `cb17756a…`, source `5aab0be0`): after
  Settings → Remove, `/Library/LaunchDaemons/dev.rexenv.rexenv.edge.plist` gone, nothing on
  :443, `/etc/resolver` empty, the CA still in the keychain but `CSSMERR_TP_NOT_TRUSTED`, the
  DNS agent job gone; one admin prompt + the keychain's own trust dialog. Found #715 there.
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
- [ ] **Flip the release host back when `rexenv/rexenv` goes public** — two things
  in ONE commit, or the tap's guard fails the bump: the cask's `url` and `SOURCE_REPO`
  in `update-cask.yml` (both in `rexenv/homebrew-tap`). **Plus a trigger** (11 Sep 2026):
  the bump now fires on the tap's OWN `release: published`, which a release in
  `rexenv/rexenv` never sends — restore a schedule or a `repository_dispatch`, or the cask
  stops moving with everything green (`docs/RELEASING.md`, "Going public later"). It was three until 5 Sep 2026;
  the cask's `verified:` was the third, dropped when brew 6.0.22 deprecated it. Then CI's
  `release.yml` resumes owning the build, and `docs/RELEASING.md`'s interim section
  is deleted rather than left as a second, wrong set of instructions.
  **The self-update descriptor does NOT move with them** (6 Sep 2026): its URL is compiled
  into every shipped build, which is exactly why it is a committed file on
  `rexenv/runtimes` and not a release asset — moving it would strand every copy already
  installed. What moves is one `TAP_REPO` variable in the runtimes publisher, and the
  descriptor's `url` field, which is signed data rather than a constant.
- [ ] **The self-update swap probe (T0) and the first real in-app update (T11)** —
  `docs/archive/PLAN-self-update.md` §6.5 and §13. Both need a human at a real Mac: T0
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
- [ ] ⚠ **The macOS floor is a claim about BOTH slices — the metadata half is now
  measured, the run half is not** (narrowed 23 Sep 2026: `docs/PLAN-macos-13-floor.md`
  swept every default-stack, PHP, FrankenPHP, cloudflared and PostgreSQL artifact on BOTH
  slices; only the bottle bundles' x86_64 blobs and MySQL 8.0.44 x86_64 remain arm64-only.
  The paragraph below is the 30 Aug state, kept for the reasoning). PORTS.md's `minos` table is measured from this machine's
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

- [ ] **Hold the tray menu open while the stack MOVES** (L3, ledger #437) — the 5s tick
  now writes titles/enabled/checkmarks onto the live items and rebuilds only when a row
  appears or disappears, so an open menu should update without closing. Fixed 8 Sep 2026
  after the menu was reported closing itself seconds after being opened; the 1 Sep walk
  missed it because a 15s hold on an IDLE stack is the case where nothing moves. Owed:
  the two `docs/SMOKE-TEST.md` boxes — a ~30s hold during a start (numbers move, menu
  stays open) and **About rexenv** from the tray with the window CLOSED.

## Blocked on external work

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
- [ ] **OpenLiteSpeed override server** — no macOS artifact exists anywhere
  (upstream ships Linux tarballs only; no homebrew-core formula; the one
  community tap is frozen at EOL 1.4.51 and fails the trust model). A maintainer
  self-build is plausible but needs the same hosting infra as the Xdebug build.
  Code is ready and honest: `ensure_server_available_on` refuses OLS in CORE at
  create AND switch, and `OverrideKind` means enabling it later is one new arm.
  On Linux (Phase 4) this is cheap — official tarballs exist.

## Phase 4+ (next era)

- [x] Linux platform impls — `platform/linux/mod.rs` ✓ moved to *Now* as "Linux launch" on
  24 Sep 2026 (`docs/PLAN-linux-port.md`); the stubs are filled (ledger #716). Windows's
  W1/W2 (traits for the Unix-only leaks, the `(os, arch)` binary catalog) were most of
  Linux's groundwork, as this row predicted.
- [ ] Public distribution (the open-sourcing half of the old "packaging polish" row).
  **The updater half moved out of Phase 4+ on 6 Sep 2026** — it is the "In-app
  self-update" row under *Now*, planned in `docs/archive/PLAN-self-update.md`, and it does
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
