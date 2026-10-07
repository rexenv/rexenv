# Shipped — October 2026

⚠️ **Historical.** Completed rows moved out of `docs/TODO.md` by `scripts/todo-reconcile.py`, with their ✓ evidence as written at the time. May contradict current code — the live description is `docs/ARCHITECTURE.md`.

## Reconcile of 8 Oct 2026

### From “Now — actionable code/test work”

- [x] **The update card gives one download two sizes** ✓ 1 Oct 2026 — the consent sentence and the
  not-enough-space refusal print through `size_label` (`core/app_update.rs`), `fmtBytes`' rule (base
  1024): 31_000_000 bytes is "29.6 MB" in all three places; `the_update_card_gives_one_download_one_size`
  pins both sides (planted), ledger #766. Found 30 Sep 2026 recording the tutorial videos: the header reads "rexenv 0.8.11 · 29.9 MB" (`fmtBytes`, base 1024 —
  `src/components/settings/AppUpdateCard.tsx:159` via `shell/DownloadPanel.tsx`) while the consent
  sentence under it says "Downloads rexenv 0.8.11 (31 MB)" (`consent_sentence`, `/ 1_000_000` —
  `src-tauri/src/core/app_update.rs:1034`). Same card, same file; it reads like two different
  downloads. Common code, so all three OSes show it. *Done when:* both numbers come from one rule
  (the consent sentence is Rust's, so the header uses the same unit or Rust sends the formatted
  size), and a test pins that the two agree for a size where base 1000 and 1024 round differently.

- [x] **A running site with an empty log is told to start itself** ✓ 1 Oct 2026 — `RecentLogs`
  takes the page's `isServing`: serving reads "No recent activity yet — new lines appear here as they
  are logged.", the start hint only when the site is not serving; both seen rendered against the
  video demo backend (Agency Blog serving; Shop Staging after Stop site). Found 30 Sep 2026 recording
  the tutorial videos: Site detail → Overview → "Recent logs" prints "No recent activity — start
  the site to see logs here." whenever `recent.length === 0` (`src/routes/SiteDetail.tsx:1647`),
  never asking whether the site is running — so a site serving right now, whose log is merely
  empty, is told to start. *Done when:* the empty state follows the site's state (running: an
  empty-log sentence; stopped: the start hint), on all three OSes (frontend, common).

- [x] **Tutorial videos: the full series, the launch intro and the feature tour** ✓ 1 Oct 2026 —
  `scripts/video/`: 22 narrated tutorials (`videos.json` + `record-all.mjs` + `make-index.mjs`, one
  `scenes/<name>.ts` each mirroring its Rust flow), "Introducing rexenv" in landscape and 1080×1920
  (`record-intro.mjs [portrait]`, ~65 s) and "The rexenv tour" (`record-tour.mjs`, 2:52, every
  feature); recorded in Chromium against the scripted backend, owner-reviewed, MP4s made.

- [x] **Linux: the 0.8.10 → 0.8.11 in-app update ends in onboarding's Welcome with "resolver
  MISSING" over a route that still works** (30 Sep 2026, 22.04 VM, §M): the relaunched 0.8.11 does
  not classify 0.8.10's route unit (`ExecStop=/sbin/ip link del rexenv0`) as its own, although
  resolved still routes `~rex` through `rexenv0` and a fresh name resolves; one "Set up domains &
  SSL" (a polkit prompt) rewrote the unit to #762's shape and everything read installed. Every
  0.8.10 Linux install pays that extra pass on this update and is greeted like a first run over
  its existing sites. *Done when:* an app whose route works but whose unit is an older rexenv
  shape says so ("route from an older rexenv — re-apply" with the one prompt) or re-applies it
  in the update's own polkit, and never opens the Welcome screen over an install with sites.
  ✓ **Code, 2 Oct 2026 (ledger #769):** a Linux route that still routes is installed whatever rexenv
  wrote its script/unit — `dnsroute::classify` weighs marker + liveness + this build's shape, an older
  shape becomes `DnsManager::route_notice` (Linux only; macOS/Windows `None`): `rex status` prints the
  sentence under the DNS line with `rex tld --repair rex`, Settings shows it with a Re-apply button, and
  the repair verb (`core::dns::repair_resolver`: installed / re-applied / unchanged — CLI, Settings, MCP)
  re-applies it with ONE polkit; `ensure_resolver` leaves a working route alone, so the update itself
  never prompts. No Welcome: `resolverInstalled` stays true. L0 on the VM's verbatim `resolvectl` output
  + 0.8.10's unit shape (plant-proven), TEXT on the command (plant-proven). **Still open — the VM leg:**
  the VM's unit is already the current shape, so the proof is a hand-written 0.8.10 unit there
  (`docs/SMOKE-TEST.md` § Linux): installed + the notice, Re-apply once, no Welcome.
  ✓ **The classification RAN on the VM later the same day** (`linux_route_shape_check`, built in
  the `linux-check` image): 0.8.10's unit by hand → this build `Ours` + the notice while the route
  routed — and the installed 0.8.11 still said "installed", because 0.8.11 compared only the
  SCRIPT; the 30 Sep install had 0.8.10's script AND unit. ✓ **The repair verb ran on the VM, 6 Oct
  2026** (this build's app + `rex`, release-built in the `linux-check` image and swapped into the
  installed deb; the published 0.8.11 deb reinstalled after): 0.8.10's unit by hand → `rex status`
  "resolver installed" + the notice, the app opened on Sites (not Welcome), Settings → DNS & SSL
  showed the sentence with **Re-apply** → ONE polkit (rexenv's own sentence) → unit back to
  `resolvectl revert`, notice gone, `probe.rex` resolving throughout; the same from the CLI
  (`rex tld --repair rex` → "re-applied … it kept working throughout"), and once more → "already
  resolves here — nothing to repair", no prompt.

- [x] **`rex status | head -1` panics "failed printing to stdout: Broken pipe (os error 32)"** (30
  Sep 2026, 22.04 VM): the CLI's `println!` on a closed pipe aborts with a Rust panic on stderr.
  *Done when:* a closed stdout ends the command quietly (SIGPIPE default, or the writes checked)
  on all three OSes. ✓ **2 Oct 2026** — every print in `cli/` goes through `emit`
  (`outln!`/`out!`/`errln!`/`err!`): a `BrokenPipe` ends the command with exit 0 and nothing said,
  any other write error keeps std's panic — ONE rule for the three OSes, no signal disposition
  (ledger #767). Source-guarded (`every_line_rex_prints_goes_through_emit`, plant-proven), the
  platform's errno classified (`a_closed_reader_is_told_apart_from_a_failed_write`), and the cli
  crate's first integration test (`cli/tests/closed_reader.rs`) runs the built `rex -h` on a pipe
  whose reader was dropped before the spawn (plant-proven). Run on all three: dev Mac (`rex status
  | true` 101 → 0, silent); Ubuntu 22.04 arm64 in the `linux-check` image (31 tests green, `rex -h
  | true` → 0); Win11 ARM VM (`closed-reader.ps1`: a planted std-panic build exits 101 "The pipe is
  being closed. (os error 232)", the fixed build 0 with empty stderr).

- [x] **macOS: the in-app update's automatic reopen did not happen once (1 of 2, 0.8.10 → 0.8.11
  on the 15.8 VM, 30 Sep 2026)** — OK on "rexenv 0.8.11 is installed" → the app quit ("reopening
  … once this process exits" logged 15:05:41) → nothing for two minutes: no launch line, no crash
  report, no `--relaunch` process caught; a hand `open` then launched 0.8.11 at once. The second
  run reopened within a second. The relauncher (`platform/macos/relauncher.rs`) waits on the
  parent's kqueue exit with a 120 s cap and `open`s the bundle, stderr to `/dev/null` — so a
  failed `open` or a wrong parent token leaves no trace. *Done when:* the relauncher writes its
  outcome (parent wait, `open`'s exit code, an error) to the app's log dir, and the next update
  on the VM either reopens twice in a row or names why not. The 0.8.10 → 0.8.11 update also
  showed "rexenv quit unexpectedly" once (the DNS agent, #761's race under the 0.8.10 plist —
  expected exactly once per 0.8.10 install, never again after the #764 launcher is written).
  ✓ **The record, 2 Oct 2026 (ledger #768):** every OS's relauncher writes `logs/relaunch.log`
  under rexenv's data folder — what it was told, how the wait ended (`ParentWait`: exited /
  already gone / still alive at the cap / unwatchable, with the seconds) and how the launch
  ended (`open`'s exit code or the spawn error) — to a `--log <dir>` when given (the live checks'
  fixture root), else the app's log dir; L0 format/rotation/parse + a TEXT guard over the three
  relaunchers (plant-proven), `app_relaunch_check` asserts the record (PASS, dev Mac);
  `windows_app_relaunch_check` the same on the Dell (PASS 8/8, 5 Oct 2026).
  **Still open — the VM leg:** the next update on the 15.8 VM reopens, or `relaunch.log` names
  why not (`docs/SMOKE-TEST.md` § In-app self-update).
  ✓ **6 Oct 2026, the 15.8 VM — reopened twice in a row:** this build stamped 0.8.10 (`b7c6b591`)
  → the published 0.8.11 through Settings → About, OK → 0.8.11 answering after **6 s**; the same
  again → **3 s**. And the row's own premise was wrong: on macOS the NEW side's binary runs the
  relauncher — the second run caught it (`--relaunch-after 5411 … /Applications/rexenv.app`) and
  its executable was inode 710141, the swapped-in 0.8.11, not the 0.8.10 build's 709533;
  `current_exe()` is a path, which the swap had already re-pointed. So no `relaunch.log` appeared
  (0.8.11 predates #768), and the 30 Sep failure ran 0.8.11's relauncher too. The record first
  appears on an update TO a build carrying #768 — 0.8.12's.

- [x] **Windows: `setup.exe /S` over a RUNNING rexenv returns 0 and leaves the old `rexenv.exe` in
  place** (found 30 Sep 2026 on the Win11 VM, 0.8.11's third draft over its second): the
  silent install wrote the registry (`DisplayVersion` 0.8.11), `rex.exe` and the task, but the
  running app's image could not be replaced (file in use) and NSIS carried on — `rex --version`
  read `rex 0.8.11 (21f8a1b) · app rexenv 0.8.11 (7d9ec97)`, a half-replaced install with a
  green exit code. The non-silent installer shows its running-app sentence; `/S` has no UI.
  **Sharper, the same afternoon:** the DNS agent is `rexenv.exe` too (`\rexenv\dns-agent`, a
  logon task — always running), and with only the app quit the replacement failed again (exe hash
  unchanged, `app rexenv 0.8.11` behind `rex 0.8.10`); with the app AND the agent task stopped
  the same installer replaced it — so on any machine where rexenv has ever run, `setup.exe /S`
  cannot replace the binary unless it stops the agent first. *Done when:* the silent path either
  closes the app (the in-app updater's swap does this with `RenamePair`) or exits non-zero
  naming the running process; `docs/SMOKE-TEST.md` § Windows carries the row; the tap's
  `install.ps1` (which leaves a running rexenv alone by design) is not the fix.
  ✓ **5 Oct 2026, the Dell (Win10 22H2), ledger #787:** Tauri NSIS `installerHooks`
  (`src-tauri/nsis/hooks.nsh`): `AllowSkipFiles off` (a locked file aborts with a non-zero exit,
  never a silent skip — NSIS's default lets silent mode skip it), and on `/S` the pre-install hook
  ends `\rexenv\dns-agent`, kills every `rexenv.exe` of the user and WAITS until none is left
  (bounded), the post-install hook re-runs the task on the new binary; the app is not relaunched
  (silent means silent). Built there (`pnpm release:win`, §A0-windows green) and run under the
  desktop's Medium token with the app AND the agent running: exit 0 in 7 s, `rexenv.exe` replaced
  (12CF… → 5FA2…, carrying this build's stamp `2026-10-05T04:00:09Z`), the agent back on the new
  binary answering `127.0.0.1:53`; with the whole stack up, every service process survived it (same PIDs, adopted by the relaunched app). L0 guard ties the hooks to `DNS_AGENT_TASK` (plant-proven).

- [x] **Linux: after an in-app update Ubuntu shows "WebKitWebProcess closed unexpectedly"** (29 Sep
  2026, 22.04 VM, 0.8.9 → 0.8.10): the update itself was clean — the new app came back — but at
  09:12:26, the moment the OLD app exited through the update's exit gate, its WebKitWebProcess
  (pid 2656, reparented to `systemd --user`) died on SIGSEGV and apport put up "Problem in
  WebKitWebProcess … closed unexpectedly" over the new window (`/var/crash/_usr_lib_aarch64-linux-
  gnu_webkit2gtk-4.1_WebKitWebProcess.1000.crash`). A user reads it as the update crashing. Find
  out whether an ordinary Quit does the same (the gate may exit without closing the webview) and
  close the window/webview before the process exits. **Measured the same day, and NOT fixed in
  0.8.11 — nothing to aim a fix at yet:** a second 0.8.9 → 0.8.10 in-app update did NOT crash (1 of
  2); tray Quit with the window hidden, and visible, did not; `dpkg -i` under a running app then
  Quit did not; Quit followed at once by a relaunch did not. The update's quit is the ordinary one
  (`app.exit(0)` through the gate), so it is an intermittent WebKitGTK crash at exit, not rexenv's
  swap. The first crash file was deleted before it was read (a mistake). **Next sighting:** keep the
  `.crash`, `apport-retrace -o trace.txt <file>` (or `apport-unpack` + `gdb` on the core) for the
  stack, then decide whether closing the webview before `app.exit` is the fix. **Mitigation
  shipped 30 Sep 2026 (ledger #757) without waiting for the stack:** on Linux `RunEvent::Exit`
  destroys every webview window before the job/tunnel cleanup and the process end, so WebKitGTK's
  web process gets an orderly shutdown instead of losing its UI process mid-flight. TEXT-proven
  (the destroy loop is the arm's first statement, Linux only); `linux-check` compiles it. Still
  open until the VM shows an update (or four quits) with no apport dialog — and if it recurs, the
  `.crash` capture above is still the next step. (30 Sep 2026 on the VM: four SIGTERMs and four
  Ctrl+Q presses did not quit the Linux build — neither is the quit gate; the tray's Quit is — so
  no evidence either way; `/var/crash` stayed empty.)

  ✓ **30 Sep 2026, §M for 0.8.11 (the first update on the #757 build, 22.04 arm64 VM):** 0.8.10 →
  0.8.11 through the app, ONE polkit, the app came back by itself, `/var/crash` empty, no dialog —
  0 of 1 with the mitigation; the row stays open until the next update makes it 0 of 2 or more.
  ✓ **6 Oct 2026, three more on the same VM — 0 of 4 with the mitigation (1 of 2 before it):**
  this tree's release binary stamped 0.8.10 (`7c2e6966`, built in the `linux-check` image) swapped
  into the installed deb, launched from GNOME, Settings → About → Install the published 0.8.11 →
  ONE polkit (the update's own sentence) → "0.8.11 is installed" → OK → 0.8.11 answering within
  2 s, three times. After each: `/var/crash` empty, no `segfault`/`WebKitWebProcess` line in the
  journal since the OK, no apport window on the screen 30 s later. The VM ends on the published
  deb (`dpkg -V` clean). If it ever recurs, the `.crash` capture above is still the next step.

- [x] **macOS: "Background Items Added" notifications pile up — eleven on the 15.8 VM** (seen 2 Oct
  2026 while running T3 on the installed 0.8.11; screenshot in the session): Notification Centre
  held 11+ identical "Background Items Added — 'rexenv' is an item that can run in the background.
  You can manage this in Login Items & Extensions." cards. macOS posts one each time a login item
  or LaunchAgent is (re)registered, so something re-registers rexenv's on every launch or on a
  schedule — the DNS agent's LaunchAgent (`#764`'s launcher plist?), the login item, or the edge
  daemon — instead of once. Find which registration repeats (`log show --predicate 'subsystem ==
  "com.apple.backgroundtaskmanagement"' --last 1d` on the VM names the item and the caller) and
  make it idempotent: register only when the on-disk definition differs. *Done when:* a launch of an
  already-set-up app posts no new card, and the VM's stack of cards stops growing across three
  launches. ✓ **Found and fixed the same night (ledger #772):** `sfltool dumpbtm` on the VM —
  the EDGE LaunchDaemon's item at `Generation: 11`, the login item and the DNS agent at 1: every
  Start all over a loaded edge ran `install_command`'s `bootout` + `bootstrap system`, and each
  `bootstrap` is a new BTM registration = one card. The install shell now takes `kickstart -k`
  when the binary, wrapper and plist on disk are byte-identical and the label is loaded (L0
  plant-proven; RUN as root on the VM twice: pid changed, Generation 11 → 11, site 200). ✓ **And the
  residual closed 3 Oct 2026 (ledger #773):** the daemon's keep-alive is now a SWITCH — `KeepAlive =
  {PathState: <root flag>}`, no `RunAtLoad`; Stop all lowers the flag and kills (a bounded loop), the job
  stays loaded, Start all raises the flag and kickstarts — so a Stop all/Start all pair registers nothing
  and posts no card; a pre-#773 plist still gets `disable` + `bootout` until the first Start all rewrites
  it. RUN on the VM: a throwaway PathState daemon first, then rexenv's edge through the generated shells
  (stop/start ×2, start → immediate stop, Generation 12 throughout) and two reboots (flag up → the edge
  came back by itself; flag down → stayed down, then started). **Owed:** the app-driven Stop all/Start all
  with this build (SMOKE § Robustness). Also seen in the same
  screenshot: the DNS card on that VM reads "in-process — Running
  inside the app", not "agent" (the 0.8.10 → 0.8.11 update's #761 race left the agent down and
  nothing restored it — `rex status` "answering (in-process, udp 15353)"); the row above about the
  reopen covers that install, but the degraded mode surviving a relaunch is worth a look of its own.

- [x] **macOS: Stop all can leave a slow-booted root edge running** (seen 5 Oct 2026, the 15.8 VM,
  installed 0.8.11): after a boot the auto-start logged "nothing is answering port 443" (the daemon was
  still coming up), so the manager's edge handle stayed Stopped; `rex stop` then said "✓ stop done" and
  the launchd edge (pid from boot) kept running — which is how a live check later reloaded it. Stop all
  must stop the supervised edge whenever the daemon is installed and its job is loaded, whatever the
  handle says (the handle is a belief; `launchctl print` is the fact). Reproduce first: reboot with the
  login toggle on, `rex stop` inside the first minute, then `launchctl print system/dev.rexenv.rexenv.edge`.
  ✓ **Fixed the same day (ledger #790):** `stop_services` asks `ServiceManager::edge_daemon_needs_stop` —
  the handle OR (daemon installed AND enabled) — so a running supervised edge is stopped whatever the
  handle believes, and an already-stopped one raises no prompt. L0 plant-proven; the VM reproduction
  above is the live leg still owed.

- [x] **SHIPPED macOS BUG — new WordPress sites are missing core files (measured 14 Sep 2026).** ✓ 8 Oct 2026 — new sites fixed in 0.7.2 (#604); existing sites detected + one-click repair (#793), below. WP-CLI's
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
  - [x] Read-only `wp core verify-checksums` across the owner's rexenv sites — **measured 14 Sep 2026** (17 sites):
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
    ruling. ✓ **8 Oct 2026, owner ruling "auto-detect + 1-click":** a WordPress site's Overview shows
    "N WordPress <v> core files with long names are missing" with **Repair core files** (`core_reinstall`), and
    `rex doctor` prints a `WP core` finding — the docroot checked against wordpress.org's file list for its version
    (legit names sit at exactly 100 bytes, so no offline check can tell). Ledger #793: L0 + three plants, L1
    `wp_core_zip_check` PASS 19/19 incl. the real PharData extract of `latest.tar.gz` → all 25 long core files
    named. SMOKE "A site missing long core files…" ✓ on the macOS 15.8 VM (banner, Repair, doctor ✗→✓,
    offline silent/⚠). Reaches users with the next release.
  - [x] **Default-theme files the tarball cut are not repaired.** ✓ 8 Oct 2026 — owner ruled "missing file gulo-i
    nao": the banner now counts them (only while the theme's folder exists) and Repair fetches each from the release
    tag, MD5-checked, written only where nothing is (#793; L1 + VM smoke). Original row: The no-content zip `core_reinstall` uses never
    touches `wp-content/`, so a 0.4.0–0.7.1 site keeps missing up to 14 long-named twentytwenty* fonts/patterns
    (7.1) — `hridoy.rex` still misses `Platypi-Italic-VariableFont_wght.woff2` and others after its 14 Sep repair.
    Cosmetic (a font falls back) and only while that theme is active; #793 deliberately does not report them.
    Fix needs a ruling: `wp theme install <slug> --force` overwrites a user's edits to a default theme

- [x] **The keychain (CA trust) dialog is rexenv's too** — 12 Sep 2026, owner, after the admin
  dialog got its name: the CA trust dialog still read "security". Measured first: wrapping
  `security` in the rexenv applet does NOT change the title; calling the trust API
  in-process does (ledger #579). ✓ **Closed 3 Oct 2026** — T1 (the in-process trust API), T2 (the
  live cancel/approve legs) and T3 (the packaged app's dialog on the 15.8 VM: "rexenv" + the lock
  and logo badge, Cancel → the app's cancel toast) all ran; #579 is ✅.
  - [x] T1 — `MacosCertTrust` calls `SecCertificateAddToKeychain` +
    `SecTrustSettingsSet/RemoveTrustSettings` itself ✓ 12 Sep 2026 — 4 L0 incl.
    `a_dismissed_dialog_reads_as_a_cancel_never_a_status_code`; example
    `cert_trust_prompt_check` (system tier) written
  - [x] T2 — `cert_trust_prompt_check` live ✓ 12 Sep 2026 — PASS, owner answering: Cancel →
    "Keychain permission was cancelled — …"; approve → trusted; approve untrust →
    untrusted; all on a spawned thread; throwaway CA removed by its SHA-1 (#579)
  - [x] T3 — the title from the REAL app: in a packaged build (the installed
    `/Applications/rexenv.app` predates this), Settings → re-trust the CA and read the
    dialog — "rexenv" + logo. Rides SMOKE first-run; #579 stays ◐ until then. ✓ **2 Oct 2026,
    the 15.8 UTM VM, the installed 0.8.11:** Settings → DNS & SSL → Re-trust (AX-pressed over
    ssh) → the SecurityAgent dialog read "rexenv" with the lock + logo badge, "You are making
    changes to your Certificate Trust Settings.", Update Settings / Cancel; Cancel → the app's
    cancel toast; the CA stayed trusted. #579 → ✅.

- [x] **The rest of `rexenv/runtimes`' macOS x86_64 builds are on the same August 2027 clock**
  (found 6 Oct 2026 while closing PHP 7.4's): `php.yml` (the 8.x catalog), `nginx.yml` and
  `openlitespeed.yml` all build their macOS x86_64 half on `macos-15-intel`, which GitHub retires
  in August 2027 — after that no Intel-Mac binary of anything rexenv pins can be rebuilt. The shape
  that works is PR #16's for 7.4 (`x86_host=rosetta`: `arch -x86_64` on `macos-15`, build tools
  installed natively first, because spc's/the scripts' brew calls are refused under Rosetta).
  *Done when:* each of the three has the input, one trial run green, and its Rosetta artifact
  compared with the Intel-built one (arch, minos, size, what it loads) — then the default flips
  to `rosetta` before the runners go. Reproduce failures on the dev Mac first (Rosetta is there).
  ✓ **6–7 Oct 2026, the trial half done — `rexenv/runtimes` PR #17** (branch `x86-rosetta-rest`), all
  three green with `x86_host=rosetta`, publish off, each compared under Rosetta with its published
  Intel-built twin: **nginx** (run 37474222024) x86_64, minos 12.0, `libSystem` only, regex location
  answered identically; **OpenLiteSpeed** (37474228252) byte-identical size, same libs and file list;
  **PHP 8.3.33** (37495216716) vs published 8.3.32: same six system libs, the same 65 modules, swoole
  6.2.2, identical script output, the PostgreSQL gate passed. Two real fixes found on the way, both
  reproduced on the dev Mac first: nginx's bundled PCRE2 never got `-arch` (`--with-cc-opt` reaches
  only nginx's objects — arm64 under Rosetta, a lost link), now `--with-pcre-opt`; and swoole had
  drifted under spc's `v6.*` to v6.2.3, which does not compile (`sw_usleep` undeclared) on ANY
  arch — 8.2+ now pin 6.2.2, what every published 8.x carries. **Cost noted:** Rosetta PHP 8.x takes
  2 h 14 min of the job's 180 (native 41 min). #17 merged 7 Oct 2026. **The default flip is
  `rexenv/runtimes` PR #18** (`x86_host` defaults to `rosetta` in all four workflows, README says
  so, `intel` kept as a fallback until Aug 2027); checked by run 37563378554 — nginx dispatched with
  no `x86_host` built its x86_64 half on `macos-15` under Rosetta, green. ✓ **#18 merged 7 Oct 2026
  (`6dc5098`)** — every macOS x86_64 artifact now builds under Rosetta by default; nothing depends on
  `macos-15-intel` any more. The first real build after it is the first PUBLISHED Rosetta artifact.

- [x] **Eliminate the bug class: bundled PHP with curl's THREADED resolver** (the real
  fix for #251
  ✓ **FIXED for 8.1–8.5, 7 Oct 2026 — `php-8x-8`.** The 15 Aug ruling ("not now") rested on a cost
  that had since been paid: rexenv builds 8.1–8.5 itself since 10 Sep (`php-8x-3…7`, for `pdo_pgsql`).
  And those builds had the bug TOO — re-measured on all five: `ares=1.34.6`, "Could not resolve
  host". Cause: spc's curl builder does `optionalLib('libcares', '-DENABLE_ARES=ON')` and **swoole**
  lib-depends on `libcares`, so c-ares came in without being asked for; 7.4 has no swoole. Fix in
  `rexenv/runtimes` (#19, #20): `build-php.sh` pre-builds libcurl with only its other optional libs
  (`spc build:libs "curl,openssl,zlib,brotli,nghttp2,zstd"`), which the full build then skips as
  installed; **gate 9** fails a build whose curl links c-ares or loses SSL/LIBZ/ASYNCHDNS/HTTP2/
  BROTLI/ZSTD/HSTS — read as curl.h BITS, because PHP 8.1 has no `CURL_VERSION_ZSTD`/`HSTS`
  constants (the first publish run, 37583046066, failed 8.1.34 on exactly that, with a mask
  identical to 8.3.32's). Published by run 37605359071 (all 10 builds + publish green); rexenv pins
  `php-8x-8` with all 30 digests taken from its `.sha256` files by script; `resolver_for` 8.1–8.5 →
  `Threaded`; runtimes PR #21 moves the update manifest to -8. **Proof:** published -8 8.1.34 and
  8.3.32 — `ares` empty, mask `0x55a9028d`, raw curl to `amin.rex` HTTP 200 from 127.0.0.1 (-5:
  "Could not resolve host"); three artifacts re-hashed against `.sha256`. **Still c-ares: 8.0.30**,
  static-php.dev's build — `rex doctor`'s note now names it alone, and the mu-plugin stays for it.
  **The update manifest followed the same day:** runtimes #21 moved `RELEASE_TAG_FOR` to -8, and
  `publish-manifest.yml` (run 37634634492, approved by the owner) published serial 9 — all 30 8.x
  entries on `php-8x-8`, 8.3.32 cli arm64 `16fb4520…` = rexenv's pin — so an OLDER rexenv taking an
  in-app PHP update gets the c-ares-free builds too. On the way: a dry run's `manifest` artifact
  was the TRACKED (already published) file, not the document the run computed (37631757183 showed
  -5 URLs while its log hashed -8's) — fixed in runtimes PR #22 (`MANIFEST_DRY_OUT`, merged
  `3316b7e`); checked by dry run 37635371313: its artifact is serial 10, generated by that run,
  every 8.x entry on -8 — the computed document, not the published serial 9.
  **Open:** `wp_dns_check` runs the default 8.3, which no longer exercises the plugin's c-ares path
  (ledger #251).
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

- [x] **PHP 7.4 — the five residuals of a shipped feature** (`docs/archive/PLAN-php-74-support.md`;
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
  - [x] **The x86_64 half is on a clock: GitHub's x86_64 runners end August 2027.** PLAN
    §4.5/§11 says to land the cross-compile path before then, and notes that a cross-built
    artifact can never run a native smoke test. Nothing in this file mentioned 2027 until
    the 21 Aug reconcile. ✓ **6 Oct 2026 — not a cross-compile: Rosetta** (`rexenv/runtimes`
    PR #16, branch `x86-under-rosetta`): the workflow's new `x86_host=rosetta` runs the macOS
    x86_64 half on the arm64 `macos-15` runner as an x86_64 process tree (`arch -x86_64`), so
    `build-php74.sh` needs no cross branch and its gates still EXECUTE the artifact (emulated).
    Run 37455554616 (publish off): all four lanes green; against the published Intel-built
    `php-7.4.33-7`, both under Rosetta: Mach-O x86_64, minos 12.0, 80 MB, the same 57 modules
    (identical list), `7.4.33 NTS` cli + fpm, identical output from an intl/imagick/PCRE/DateTime
    script. Three trials failed first on ONE cause, found on the dev Mac rather than by a fourth
    hour of CI: spc's `doctor --auto-fix` installs missing tools with `brew`, Homebrew refuses
    under Rosetta in the ARM prefix, and the script's `|| true` swallowed it — so the tools are
    installed natively first. Default stays `intel` until a published build uses `rosetta`.
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


### From “Release gates (human, scripted — see the docs named)”

- [x] **The release host, now that `rexenv/rexenv` IS public (30 Sep 2026): stay on the tap, or
  move the draft here?** ✓ **Ruled MOVE by the owner the same day** ("ekhon theke rexenv tei
  release gulo dite … jeno oikhan thekei sob download korte pare … in app self update jeno
  thik moto kaj kore"), and moved: `release.yml` drafts here, `release-published.yml` dispatches
  to the tap (`TAP_TOKEN`) with a daily poll as the fallback, the tap's cask `url` + `SOURCE_REPO`
  flipped in one commit with `install.sh`/`install.ps1`, runtimes' `TAP_REPO`, apt's `TAP` and the
  website's sync + links flipped, 0.8.8–0.8.10 mirrored here byte-identical, the descriptor's home
  unchanged (both download prefixes allowed since 0.6.0 — `docs/RELEASING.md`, "The artefacts
  live on `rexenv/rexenv`"). The recommendation before the ruling was to stay; the ruling
  prefers one place to download from. As it stood: if moved, two things in ONE commit, or the
  tap's guard fails the bump: the cask's `url` and `SOURCE_REPO`
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


### From “Blocked on external work”

- [x] **OpenLiteSpeed override server** ✓ 4 Oct 2026 — **P1, P2, P3 shipped** (`docs/archive/PLAN-openlitespeed.md`).
  ✓ `openlitespeed_site_check` green on macOS and the Ubuntu 22.04 VM; ✓ every running-app SMOKE
  row 28/28 on the macOS 15.8 VM and the Ubuntu VM (LSCache purge-on-edit = P3); the runs fixed
  three bugs (ledger #783 `.htaccess` re-read, #784 `-n`, #785 launch mirrors); ✓ SMOKE § Windows
  refusal rows on the Dell (installed NSIS build), which found #786 (the switch downloaded before
  it refused); #785 L0 `load_mirrors_hands_the_manager_the_stored_env`.

