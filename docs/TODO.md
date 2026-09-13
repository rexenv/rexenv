# TODO — the single active-work file

Everything open lives here, and ONLY open work lives here. Shipped rows move to the
month's evidence log (`docs/archive/SHIPPED-2026-07.md`, `-08.md`, `-09.md`) with
`scripts/todo-reconcile.py` — the mechanical half; the judgement half is the
`reconcile-todo` skill. Tick an item in the commit that does the work, with a one-line
✓ evidence note. `scripts/todo-reconcile.py --count` prints the open/ticked tally.

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

- [ ] **Windows launch** — 12 Sep 2026, owner: macOS is stable, ship a Windows version.
  Not "fill the stubs": Unix-only code outside `platform/`, no php-fpm, no `/etc/resolver`,
  no unix sockets on Windows. Reasoning, measurements and "Done when" per task:
  `docs/PLAN-windows-port.md`. W0–W2 can start now; W3+ wait on the owner's D1–D6.
  - [ ] D5 signing — open for ONE reason: the owner decides Authenticode after an unsigned
    NSIS installer is measured on the Dell (plan §3 D5). Every other ruling is in: D3 named
    pipes + `LocalIpc` (12 Sep 2026); **13 Sep 2026** D1 php-cgi group accepted with the php-src correction (supervision,
    positive ID and worker count written in plan §3 D1(a)/(b) before W3), D2 agent on :53 +
    NRPT accepted and the `hosts` fallback REFUSED (:53 taken → refuse naming the holder),
    D4 accepted and MariaDB NOT in v1 (refused with an honest message; MySQL 8.4/8.0 cover it), D6 accepted (Win 11 x64; Win 10 22H2 best-effort;
    arm64 emulation unsupported)
  - [ ] Measure before building on it (plan §3, Dell + VM): D1 — php-cgi's own parent
    (children spawned, respawned, killed with the parent; the listener's owning pid; job
    breakaway from the app's launch contexts; memory per child; peak concurrency on a
    block-editor load); D2 — who holds loopback :53 and who ANSWERS it, per state (clean,
    hotspot/ICS, Hyper-V, WSL2 NAT + mirrored, Docker Desktop); D5 — unsigned NSIS through
    Edge and Chrome: the clicks (each named), every message verbatim with a screenshot, whether
    "Run anyway" is reachable without "More info", and whether the next build repeats it all;
    the Windows bind matrix (plan §6) before `ensure_free` is written — **bind matrix ✓ 13 Sep 2026
    on the Dell** (288 binds, `scripts/probes/windows-bind-matrix.ps1`): the trial bind reports
    free under a `0.0.0.0`/`[::]` holder and then takes its localhost traffic, so `ensure_free` must
    read the tables; :53 clean state measured (nothing there). Still open: ICS/hotspot on, a WSL
    distribution (none installed), a holder under another account, a non-elevated token, the D5
    installer, php-cgi
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
    - [ ] W11: the Windows reader fetches `manifest-windows.json` / `app-manifest-windows.json`
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
  - [ ] W3 — Paths, ACL permissions, BinaryProvider, ProcessSupervisor (MySQL + Mailpit
    start, outlive the app, adopt on relaunch — on real Windows); `ensure_free` reads the
    TCP/UDP tables, never a trial bind, and a start counts only when OUR server answers (plan §6).
    **Progress 13 Sep 2026 (compile-checked from the Mac, not run on Windows):** `Paths` →
    `%LOCALAPPDATA%\rexenv\rexenv\data` (the app-data namespace constants now live in
    `platform/mod.rs`, shared with macOS); `PermissionManager` → owner-only ACLs (current user only, no SYSTEM — owner ruling 13 Sep 2026) in
    `platform/windows/acl.rs` (SDDL builder L0-tested on every host, ledger #597);
    `set_executable` checks the file exists — Windows has no execute bit; `BinaryProvider` →
    Mark of the Web stripped, every image x64 or x86 before publish (`platform/windows/pe.rs`),
    and directory trees checked too through the new `prepare_binary_dir` (owner ruling 13 Sep
    2026; macOS no-op). Measured across all 16 Windows artifacts: 1,197 images, 3 x86, none
    refused — an x64-only rule would have refused nginx, PHP 7.4 and MySQL 8.4 (ledger #598)
  - [ ] W4 — php-cgi group (preflight + churn breaker, plan §3 D1(a)) + nginx + SMTP mail → a
    WordPress site serves
  - [ ] W5 — Caddy :443 edge + CurrentUser Root CA trust → valid lock in Edge/Chrome/Firefox
  - [ ] W6 — DNS agent + NRPT + UAC + logon task → `*.rex` resolves after reboot, app closed
  - [ ] W7 — ShellRunner, autostart, tray (includes the Windows half of the browser-stub row)
  - [ ] W8 — `rex` CLI + MCP over named pipes; `rex.exe` on PATH
  - [ ] W9 — frontend on WebView2 (Windows paths, Ctrl shortcuts, fonts)
  - [ ] W10 — Redis/Apache/Xdebug/MariaDB (per D4) refused in core with an honest message
  - [ ] W11 — NSIS installer, Authenticode (if D5 rules it in), Windows release job, updater, winget
  - [ ] W12 — launch gates: verify on the Windows runner, SMOKE-TEST + INSTALL Windows
    sections, clean Windows 11 VM pass

- [x] **An older rexenv refuses a database a newer one migrated** ✓ 13 Sep 2026 — built the same
  day at the owner's go: `refuse_newer_schema` in `state::db::open` (before any pragma) and
  `migrate_with`, its own launch-screen arm, 2 L0 plant-proven, ledger #593. Owner, after
  installing the 0.7.1 dmg over data a master dev build had been using. Measured: the only
  production opener (`lib.rs:423` → `state::db::open_for_platform` → `open`, `state/db.rs:702`)
  runs `migrate_with`, which applies the migrations numbered above `user_version` and says
  nothing when `user_version` is ABOVE `MIGRATIONS.len()`; nothing else reads it. So the day
  master adds migration 45, any older build opened on that data — a hand-installed older dmg,
  or this Mac swapping between a dev build and a release — reads and writes tables it does not
  know, silently. Not live today: v0.7.0 and master are both schema v44, and this Mac's
  database reads `user_version` 44.
  - [x] `open` refuses before any write when `user_version` > `MIGRATIONS.len()`, with a
    message naming both numbers and the fix (run the rexenv that migrated the data, or newer),
    shown wherever a failed database open is shown today (measure that path first) ✓ 13 Sep
    2026 — measured first: an open failure becomes the always-managed `InitError`, rendered
    verbatim by `FatalError`; the new `Error::NewerSchema` gets its own `lib.rs` arm, since the
    generic arm's "not writable / disk full" advice is wrong for it
  - [x] L0 test: a database stamped `len + 1` is refused and left byte-identical; plant-proven;
    ledger row ✓ 13 Sep 2026 — two tests (file database; the engine alone); plants: both calls
    removed → both FAIL, only `migrate_with`'s removed → the engine test FAILS; ledger #593
  - [x] Ship it in a release BEFORE the first release that adds migration 45. Builds up to
    0.7.1 stay unguarded forever, so the gap closes only for versions after the guard — say so
    in `docs/RELEASING.md` beside the version bump ✓ 13 Sep 2026 — on master, so every release
    from here carries it, including whichever adds migration 45; RELEASING's bump step now says a
    release that grows `MIGRATIONS` must tell users that going back to ≤0.7.1 is unsupported,
    and INSTALL explains the screen

- [ ] **Third-party notices — a Windows crate table before any Windows release** — 13 Sep 2026,
  owner asked whether the notices count drift was tracked; it was not (the debt lived only
  inside `THIRD-PARTY-NOTICES.md`). The macOS half is done below; open for ONE reason: the
  Windows graph needs its table, and `notices-check.py` a Windows target, before a Windows
  build ships.
  - [x] The rows the shipped app was missing ✓ 13 Sep 2026 — 17 added: `mysql_async`,
    `mysql_common` and 13 they pull in across arm64 + x86_64 (in the app since 24 Aug, so the
    Licenses dialog of 0.4.0–0.7.0 left them out), and the `rex` CLI's `memchr` 2.8.3 + `zmij`
    1.0.23 (the CLI's graph was never inventoried); heading 395 → 412
  - [x] Checked, not remembered ✓ 13 Sep 2026 — `scripts/notices-check.py` in `verify.sh`: Rust
    both directions (arm64 ∪ x86_64, app + CLI) with licences and the heading count, npm both
    directions with its count; plant-proven, ledger #592
  - [ ] Windows: a crate table for the Windows graph (410 crates; 50 beyond the table) and a
    Windows target in the check, before any Windows release — the download sources are
    already listed (plan §3a Q4)

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

- [x] **The admin-password dialog is rexenv's, like Local's** ✓ 12 Sep 2026 — owner: ours
  read "osascript wants to make changes." over a plain lock beside Local's branded prompt.
  Spike measured on macOS 26.6.2 first (name via bundle, badge only once `Assets.car` is
  gone); ARCHITECTURE "The admin dialog is rexenv's".
  - [x] T1 — `run_privileged` asks through a rexenv-named applet with our icon, script
    compiled in, osascript only when no dialog was shown ✓ 12 Sep 2026 — ledger #577;
    `a_built_applet_is_named_rexenv_badged_with_our_icon_and_holds_the_script_inside`
    (plant-proven) + 3 L0; `priv_check` live: "rexenv" + logo, `root` returned
  - [x] T2 — every prompt says what it is for ✓ 12 Sep 2026 — required `PromptReason` on
    `run_privileged`, set by all 8 callers in `dns`/`proxy`/`setup`/`cli`, in the applet
    and the osascript fallback; ledger #578;
    `a_reason_completes_one_sentence_whatever_punctuation_the_caller_brought` + the
    applet/osascript source tests
  - [x] T3 — one live `priv_check`, eyes on the dialog ✓ 12 Sep 2026 — the owner read
    "rexenv wants to run its password-prompt check." under "rexenv" (#578 → ✅); Cancel
    returned "Administrator permission was cancelled — this step needs it. Try again and
    approve the prompt." with no second dialog and no work dir left; the retry, approved,
    returned `root` (#577).
  - [x] T4 — the fallback split proven ✓ 12 Sep 2026 — `settle` + `classify` pulled out of
    the shelling-out code; `only_a_run_that_showed_no_dialog_is_asked_again_through_osascript`
    + `a_launch_that_ran_but_wrote_nothing_is_no_result_never_no_dialog` (both plant-proven)
    over the measured `open -n -W` exits (1 = not launched, 0 = applet killed mid-dialog);
    ledger #577 → ✅. The osascript dialog itself still never ran live (nothing here fails
    to build) — the release SMOKE row's tell covers it. (The "App Background Activity: rexenv" notification seen during
    the live run was NOT the applet: BTM log, 13:34:24, the DNS LaunchAgent re-registered
    pointing at `target/debug/rexenv`, two minutes before the applet launched.)

- [x] **Import Local multisite networks** ✓ 13 Sep 2026 — every child closed; the live
  pass ran owner-run on `multisite.local`. 12 Sep 2026, planned in
  `docs/PLAN-local-multisite.md`. Q2 of the Local import (`docs/archive/PLAN-local-import.md`
  §9), parked the same morning and un-parked by the owner: "Q2 o kore felo multisite".
  Adopt the network (record its mode, never convert), move rexenv's copy's network URLs,
  and let the connect move `DOMAIN_CURRENT_SITE`.
  - [x] T0 — the plan + this row ✓ 12 Sep 2026 — `docs/PLAN-local-multisite.md`
  - [x] T1 — scan + adopt: network rows importable, mode recorded without convert ✓ 12 Sep
    2026 — ledger #580; `adopting_a_network_records_the_mode_and_refuses_none`,
    `a_network_imports_with_its_mode_and_a_mapped_subsite_is_refused`,
    `an_imported_network_is_adopted_never_converted_and_reloaded` (both guards plant-proven).
    Not usable alone: the copy still names `.local` until T2, and serving needs T3
  - [x] T2 — network-aware URL pass on the copy ✓ 12 Sep 2026 — ledger #573 extended;
    `a_network_moves_every_subsite_before_its_own_bare_name`,
    `a_network_is_proved_moved_only_when_every_blog_reads_https_on_the_new_name` and the
    wiring guard (plant-proven ×3). T4 measured its override pin unnecessary — removed;
    the proof's `--url` is what works
  - [x] T3 — `RewriteKey::NetworkDomain` in the connect ✓ 12 Sep 2026 — ledger #581;
    `a_network_moves_only_its_domain_bytes`, `a_networks_domain_reaches_the_plan_before_the_diff`
    (plant-proven ×2); the preview's `movesNetworkDomain` sentence in DbImportCard + MCP JSON
  - [x] T4 — L1 network leg in `local_import_check` ✓ 12 Sep 2026 — leg C PASS (33 checks):
    a real subdomain network with a subsite moves to `https://…multi.rex` in `wp_blogs`,
    `wp_site`, `sitemeta` and both blogs' options (serialized length repaired), wp-config
    byte-identical; the connect plan boots the SUBSITE on its rexenv name with
    `COOKIE_DOMAIN` `.multi.rex`. Plant: removing `--url` from the proof fails it; removing
    the override's `DOMAIN_CURRENT_SITE` pin did NOT — the pin was deleted
  - [x] **fix: a failed import no longer orphans its copy** ✓ 12 Sep 2026 — the owner's
    first `multisite.local` attempt stopped at "a database called `local` already exists …
    no rexenv site owns it": an earlier attempt's copy, left when a post-restore failure
    kept `local` while the row named its derived database, then orphaned by delete. Every
    step from the feed to `finish` now drops the partial copy (ours only); ledger #586,
    `every_failure_after_the_restore_drops_the_partial_copy` (plant-proven). The orphan
    `local` was dropped with the owner's OK. Also found then: Local's router (Site Domains
    mode) held :443/:80, so no rexenv site loaded
  - [x] **A Local copy is named `local_<domain>` from the first import** ✓ 12 Sep 2026 —
    every Local database is `local`, so the bare name only collided; ledger #587,
    `a_local_copy_is_named_after_the_site`; PUBLISH-TESTING N7's diff is now 3 keys
  - [x] **Local's router on :443 is named, with Router Mode → localhost** ✓ 12 Sep 2026 —
    the port-conflict help and the MCP `site_status` verdict (which said "can't identify")
    now name "Local's router" and the setting that frees the port without quitting Local;
    ledger #588, `locals_router_is_named_with_the_way_out_that_keeps_local_running`,
    `a_named_holder_replaces_the_cant_identify_sentence`. L3 owner-run
  - [x] T5 — docs, PUBLISH-TESTING §N network steps, live pass on `multi.local` — steps
    N10–N14 written 12 Sep 2026 (+ the summary row, and N0's stale "multisite can't
    import" corrected) ✓ 13 Sep 2026 — owner-run on `multisite.local`: "ekhon sob thik
    achhe"; the copy's blogs read back over MCP first (all `https://…multisite.rex`)

- [x] **A network's sub-site rows sign in and choose a browser** ✓ 13 Sep 2026 — owner
  report on `multisite.rex`: the Network tab's rows had a Visit icon (default browser
  only) and an "Admin" icon that opened a plain `/wp-admin/`. Each row now has Visit and
  Magic Login split buttons with the header's browser chooser; the login is minted ON the
  sub-site's blog (a main-site token is invisible to a sub-site's request) for the URL the
  network's own `wp site list` reports, host-checked. Ledger #589,
  `a_subsite_login_is_built_only_on_this_sites_host`. L3 owner-run 13 Sep 2026: "thik
  moto kaj korchhe".

- [x] **One icon per hand-off, everywhere** ✓ 13 Sep 2026 — owner, after the sub-site
  rows: their Visit and Magic Login showed a globe and a door while the header showed the
  browser and the WordPress mark ("sob jaigai consistency"). `PreferredBrowserIcon` now
  sits on every open-in-browser control (header, tile, Sites row, sub-site Visit, tunnel
  URL, Open Mailpit, Adminer, Change domain's Open site) and `WordPressIcon` on every
  magic login (header, tile, sub-site, One-click admin login, a user's Log in). DESIGN.md
  rule; ledger #590, `open_in_browser_and_magic_login_wear_one_icon_everywhere`. Checked by
  the owner in the dev app, 13 Sep 2026 ("icon gulo thik achhe").

- [x] **A Valet or Herd multisite network imports silently as a single site** ✓ 13 Sep
  2026 — owner: "Valet/Herd multisite er fix ta koro". The import reads the served
  folder's wp-config text (`MULTISITE`, `SUBDOMAIN_INSTALL`; no PHP runs, at import, not
  scan) and records the network like Local's; the batch reload covers a network found
  there; and Convert on a docroot that already is a network records the file's mode
  instead of converting — which also repairs a network imported before the fix, from
  the WordPress tab's new "Already a multisite network → Record as a network" card.
  Ledger #591, `a_network_is_read_from_wp_config_and_never_converted_again`,
  `an_imported_network_is_adopted_never_converted_and_reloaded`. Original text: found
  12 Sep 2026 researching the Local network import (`docs/PLAN-local-multisite.md` §6).
  Nothing in `core/valet.rs` / `commands/valet_import.rs` detects multisite, so the site
  lands as `multisite = none`: subdirectory subsites 404, subdomain subsites aren't
  served, and the WordPress tab offers "Convert to multisite" on a live network. The
  scan may not open project files, so detection needs another source (the copied
  database, or a post-import `wp_info` read).

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

- [x] **Release 0.7.1 — the notices fix alone, cut from v0.7.0** ✓ 13 Sep 2026 — published, cask
  bumped, app manifest serial 4 offering 0.7.1 (`check-app-manifest: all green`). Owner ruling 13 Sep 2026: never
  rebuild a published version, and do not wait for the next feature release either, because
  the shipped binary carries code whose notice does not travel with it. Flow: `docs/RELEASING.md`,
  "A patch release cut from a published tag". Branch `release/0.7.1` in a worktree: the 17 rows,
  `notices-check.py` wired into `verify.sh`, its ledger row, the four-manifest bump — and two
  release-tooling fixes that ship in no binary: `verify-receipt.sh` hardcoded `.git/`, so the bar
  and the pre-commit hook could not pass in any worktree (the first `verify-full` died on it);
  and 14 example `curl` calls had no `--max-time`, so a fixture FrankenPHP that accepted and
  never answered held the second `verify-full` for 50 minutes (owner: bound them, 13 Sep 2026).
  The same run found a 4-day-old `vite` on :5199 left by an earlier `verify-full` — wk-checks
  would have tested whatever that served; stopped with the owner's go.
  - [x] `verify-full.sh` green on the branch; its four commits (receipt path, curl bounds, the
    notices fix, the bump) ✓ 13 Sep 2026 — `verify-full: all green` (bar, sandbox tier,
    wk-checks ALL PASS) on the third run; `release/0.7.1` = `d324fff` → `956ece1` → `94c9cf7` →
    `b5c3d43`, `check-versions` 0.7.1 in all places
  - [x] `pnpm release:mac`, then §A0 by hand (both slices, the per-slice payload, codesign, one dmg)
    ✓ 13 Sep 2026 — the first build was killed at low memory (18 GB, swap full); the owner re-ran
    it with `CARGO_BUILD_JOBS=4`. `release-assets: all green` (archive sha256 `e21b3525…`), §A0 all
    green: one dmg, `rexenv` + `rex` x86_64 arm64, the payload in both slices, codesign, and the built
    Licenses text carrying `mysql_async | 0.37.0`, `zmij | 1.0.23`, "410 external crates"
  - [x] **Owner:** SMOKE-TEST on that dmg, then PUBLISH-TESTING §A ✓ 13 Sep 2026 — owner reported
    both pass (after installing the same dmg on the dev Mac, where Local import was — correctly —
    absent: it landed after v0.7.0)
  - [x] Draft on `rexenv/homebrew-tap` with the one-line note ✓ 13 Sep 2026 — release id
    387865008, draft, tag `v0.7.1`; four assets `uploaded`, and the API's digests equal the local
    hashes (dmg `89239fd1…a3e3`, archive `e21b3525…c859`); exactly one asset ends
    `_universal.dmg`, the same shape as 0.7.0
  - [x] **Owner** publishes the draft → the cask bumps on that publish → runtimes "Publish app
    update manifest" (dry run first) → `scripts/check-app-manifest.sh` ✓ 13 Sep 2026 — published
    10:34:13Z, Latest; `update-cask` run 34752197550 success, cask `version "0.7.1"` with sha256
    `89239fd1…` = the dmg; manifest dry run 34752249555 ("nothing committed"), real run 34752320411
    → runtimes `b82cdba`, serial 3 → 4, release 0.7.1; `check-app-manifest: all green` once the
    CDN served serial 4 (10:40:39Z)
  - [x] Local tag `v0.7.1` ✓ 13 Sep 2026 — annotated, on `b5c3d43`, not pushed (the pre-push hook
    refuses a `v*` tag while the repo is private)
  - [x] After publish: master records the shipped commit (`git merge -s ours release/0.7.1`),
    then the worktree and branch can go ✓ 13 Sep 2026 — merge `b563ac3` (no tree change;
    `v0.7.1` is in master's history); the worktree's two symlinks were unlinked before
    `git worktree remove`, the main target and wk-checks deps confirmed intact after; branch
    deleted, the tag keeps `b5c3d43`

- [x] **`check-app-manifest.sh` blames a forgotten click for CDN lag** ✓ 13 Sep 2026 — built at the
  owner's go: step 2 reads the committed file through the contents API, verifies its signature, and
  prints "the publish already happened — only the CDN is behind … Do NOT publish again" when the
  commit names the tap's version; the forgotten-click line stays for a real miss. Proven offline by
  `check-app-manifest-test.sh` in `verify.sh` (13 checks, two plants), and against the live
  descriptor (all green). Ledger #594. Found 13 Sep 2026, releasing
  0.7.1: run two minutes after the real publish (runtimes `b82cdba`, serial 4, 10:37:29Z), it read
  `raw.githubusercontent.com`'s cached serial 3 and printed "This is the forgotten-second-click:
  run … 'Publish app update manifest'" — advice that sends someone to publish a second time. The
  committed file was already right (contents API: serial 4, 0.7.1). Read the descriptor through the
  API as well, and when only the CDN is behind, say that and how long raw caches, instead.
  Measured the same day: the CDN served serial 4 at 10:40:39Z, 3 min 10 s after the commit.
  Done when: a check run inside the cache window prints the CDN-lag line, not the forgotten-click one.

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

- [ ] Linux platform impls — `platform/linux/mod.rs`. Windows moved to *Now* as
  "Windows launch" on 12 Sep 2026; its W1/W2 (traits for the Unix-only leaks, the
  `(os, arch)` binary catalog) are most of Linux's groundwork too.
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
