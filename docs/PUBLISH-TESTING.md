# PUBLISH-TESTING.md — live/GUI checks before publishing

Everything below needs a **real launch, a GUI action, or root/launchd** — i.e. things
that can't be verified by `cargo test` / `tsc` / `brew style` and must be run on a real
Mac by hand. Each item lists the **exact command**, **expected result**, **why it
matters**, and whether it's **🚧 publish-blocking** or **🟢 nice-to-have**.

**Already verified (no action needed):**
- All code fixes are unit-tested and green — `cd src-tauri && cargo test --lib` → **336
  passed / 0 failed**, `cargo build --examples` clean, `tsc --noEmit` clean.
- The tap **static** side: the built app is validly ad-hoc signed (`codesign --verify`
  passes), universal `x86_64 arm64`, and `homebrew-rexenv/Casks/rexenv.rb` passes
  `brew style`.
- **C) B31 CSP packaged smoke test — ✅ DONE** (packaged build, every flow worked, no
  `Refused to … violates CSP` lines). Not re-listed here.

---

## A) 🚧 RE-RUN NEEDED on the fresh dmg — Apple-Silicon ad-hoc launch test — THE gate for the tap being real

**STATUS:** §A passed twice before (2026-07-20 `d48bc8ba…`, 2026-07-21 `8d201724…`), but a **fresh build
`rexenv_0.1.0_universal.dmg` sha256 `0e57f11c…` (2026-07-22)** now supersedes those — it adds the entire
deferred pass (21 fixes: B25 timeout family, the cert pass B6/B13, B7/B15/B28/B29, B12/B16/B26/B30, and
the B20 recorded-port allocator + migration). Ad-hoc signing is unchanged, so the launch behavior should
hold, but **re-confirm §A on `0e57f11c…` before announcing the tap** (all 21 fixes post-date the last
pass). Steps below (§A-orig) — use the `0e57f11c…` dmg. On pass, the tap approach is re-validated for the
shipping artifact. (Canonical cask sha256 still recomputed from the uploaded Release asset — see §D.)

_(Prior passes: 2026-07-20 `d48bc8ba…`, 2026-07-21 `8d201724…` — both superseded by the `0e57f11c…` rebuild.)_

## A2) ✅ PASSED (2026-07-21) — first-run PHP download resume on a real flaky link

**RESULT:** on the reporter's new Mac (the link that originally failed with "error decoding response
body … gave up after 3 attempts"), the first-run PHP download (`php-8.3.31-fpm-macos-aarch64`, 34 MB
from `dl.static-php.dev` → DO Spaces fra1) **completed cleanly** against this build — the HTTP Range
resume (commit `4c6bb63`) recovers from mid-body drops instead of restarting from byte 0. This is the
real-world proof the unit/integration tests (happy-resume + corrupt-fails-closed + 200-restart) stand
in for; first-run site creation is unblocked.

---

## A-orig) 🚧 Apple-Silicon ad-hoc launch test — THE gate for the tap being real

**Why:** the static checks prove the app is validly ad-hoc signed with an arm64 slice, so
in theory it runs on Apple Silicon once de-quarantined. This confirms it **empirically** —
the one thing I could not verify (it requires launching the app, which starts the stack).

Uses the existing local build; **no rebuild needed**.

```sh
cd /Users/wpdev/PhpstormProjects/rexenv
DMG="src-tauri/target/universal-apple-darwin/release/bundle/dmg/rexenv_0.1.0_universal.dmg"

# 1. Simulate a real "downloaded from the internet" quarantine on the dmg:
xattr -w com.apple.quarantine "0083;00000000;manual;$(uuidgen)" "$DMG"

# 2. Install from it:
hdiutil attach "$DMG"                       # note the "/Volumes/…" name it prints
cp -R "/Volumes/rexenv/rexenv.app" /Applications/
hdiutil detach "/Volumes/rexenv"

# 3. Confirm it's quarantined and BLOCKED:
xattr -p com.apple.quarantine /Applications/rexenv.app   # prints a value ⇒ quarantined
open /Applications/rexenv.app                            # EXPECT: Gatekeeper blocks it
                                                         # ("damaged" / "unidentified developer")

# 4. De-quarantine (the tap-user step) and launch:
sudo xattr -rd com.apple.quarantine /Applications/rexenv.app
open /Applications/rexenv.app                            # EXPECT: it launches
```

**Expected:** blocked while quarantined → **launches after `xattr -rd`**.
(If step 1's synthetic xattr doesn't trip Gatekeeper on your macOS, the definitive test is
to `curl -LO` the dmg from any URL — a genuine download applies the quarantine for sure;
that's also the exact tap-user path once it's on GitHub Releases, i.e. section D.)

**If it launches:** the tap is real — announce it. **If it's rejected even after `xattr
-rd`:** stop and tell me; the ad-hoc approach won't work and we reconsider (signing /
Intel-only / other). I'd be surprised given the valid signature, but this is the gate.

---

## B) 🚧 B2 — uninstall removes the root :443 daemon (launchd live check)

**Why:** confirms the B2 fix actually unloads the root edge LaunchDaemon and frees :443. The
command *composition* is unit-tested (`edge_daemon_uninstall_boots_out_and_removes_plist_wrapper_and_binary`);
only the real launchd unload needs a live run. It tears down your running edge, so do it when convenient.

```sh
# Precondition: the edge daemon is installed/running (you've started the stack at least once).
launchctl print system/dev.rexenv.rexenv.edge >/dev/null 2>&1 && echo "edge daemon present"
lsof -nP -iTCP:443 -sTCP:LISTEN                    # note: caddy on :443

# In the app: Settings → "Remove system changes"  (one admin prompt), then:
launchctl print system/dev.rexenv.rexenv.edge      # EXPECT: "Could not find service" (gone)
lsof -nP -iTCP:443 -sTCP:LISTEN                    # EXPECT: nothing rexenv/caddy
ls -la /Library/LaunchDaemons/dev.rexenv.rexenv.edge.plist   # EXPECT: No such file
```

**Expected:** service gone, plist removed, nothing on :443, and `/etc/resolver/*` for your
TLDs removed, CA trust dropped (login keychain). One auth prompt total (edge + resolvers are
combined into a single privileged shell by the fix).

---

## D) 🚧 Full custom-tap install dry-run — once the dmg is on GitHub Releases

**Why:** the real end-to-end a user experiences. Only doable after you (1) push the
`homebrew-rexenv/` contents to `github.com/rudlinkon/homebrew-rexenv`, and (2) upload the dmg
to a Release tagged `v0.1.0` on `github.com/rudlinkon/rexenv`, then **recompute the sha256
from the uploaded asset** and bump the cask if it differs from the provisional one.

```sh
# One-time online cask audit (may ask you to add `verified: "github.com/rudlinkon/rexenv/"`
# to the url stanza — trivial to add):
brew audit --cask --new rudlinkon/rexenv/rexenv    # after the tap is pushed

# The user path:
brew tap rudlinkon/rexenv
brew install --cask rexenv                         # EXPECT: downloads, installs, postflight de-quarantines
open -a rexenv                                     # EXPECT: launches (no Gatekeeper block)
which rex && rex --version                         # EXPECT: rex on PATH, prints version
brew uninstall --zap --cask rexenv                 # EXPECT: clean removal of user-level state
```

**Expected:** installs, **launches without a Gatekeeper block** (postflight did the
de-quarantine), `rex` is on PATH, `--zap` cleans user-level state. Reminder: run the app's
**"Remove system changes"** before `brew uninstall` to clear the privileged bits (§B).

---

## F) 🚧 Resolver TAKEOVER + RESTORE — clean-VM only (fixture-tested, never live-run)

**Why this is here:** the dev Mac has no `/etc/resolver/test` (only `rex`, plus an
unrelated `sb`), so the foreign-file paths of the Valet/Herd resolver takeover have never
run against a real root-owned foreign file. Creating one on the dev machine to test was
deliberately refused. The logic is unit-tested against fixture directories
(`core::dns::owner_of`) and the ABSENT-file path is live-verified; these two paths are
fixture-only. Plan: `docs/PLAN-valet-herd-import.md` §4.

Needs a VM (or a spare macOS account/machine) with **Valet or Herd installed and started
at least once**, so `/etc/resolver/<tld>` genuinely exists and is theirs.

1. Confirm the starting state: `cat /etc/resolver/test` shows THEIR content
   (`nameserver 127.0.0.1`, no `port` line), and `ls -l` shows it root-owned.
2. Open rexenv → the Valet/Herd import screen. Expect the resolver card to report the file
   as **not ours**, showing their content beside ours.
3. Try to create a `.test` site WITHOUT consenting. Expect an honest refusal — and verify
   `/etc/resolver/test` is byte-identical afterwards (`shasum` before/after). **This is
   the regression guard for the silent-overwrite bug.**
4. Tick the consent box, take it over. Verify: the file now holds our signature; a backup
   exists at `<app-data>/resolver-backups/test` with mode `600`; exactly one row in
   `resolver_takeovers`.
5. Import a site on `.test`, confirm it resolves and serves.
6. **Hand it back** from the resolver card. Verify: `/etc/resolver/test` is byte-identical
   to step 1's `shasum`; the backup file is GONE; the record is GONE; the confirm warned
   that rexenv `.test` sites stop resolving.
7. Take it over again, then this time run **Settings → Remove system changes**. Verify the
   same restore happened as part of the single privileged prompt.
8. **Drift:** take it over, then run `valet install` (or relaunch Herd) so they reclaim the
   file. Restart rexenv → expect the startup drift notice naming the TLD, and `rex doctor`
   reporting it too. Then remove system changes and verify we left THEIR file alone and
   dropped our record + backup.
9. **Backup-missing path:** take it over, delete `<app-data>/resolver-backups/test` by
   hand, then remove system changes. Expect our file removed and an honest message saying
   the backup was gone and to run `valet install`.
10. Throughout: `/etc/resolver/rex` must be untouched, and no file we did not create may
    ever be removed.

## H) ⚠️ READING THE EVIDENCE — a 200 on an imported host proves nothing

Burned us once during a real diagnosis, so it is written down rather than
re-learned. When a reload FAILS, nginx keeps serving its previously loaded
config. A request for a host that isn't in that config does not 404 — nginx
falls back to the **default server**, the first `server` block for the listen
address, and answers 200 from a DIFFERENT site's docroot.

So `curl -H "Host: newsite.test" http://127.0.0.1:18088/` returning 200 is NOT
evidence the site is served. Check the BODY:

```sh
curl -s -H "Host: newsite.test"  http://127.0.0.1:18088/ | head -c 80
curl -s -H "Host: someothersite" http://127.0.0.1:18088/ | head -c 80
```

Byte-identical bodies mean you are looking at the fallback, not your site. The
same applies to the UI: a green row is only meaningful once the site's own
content comes back.

## G) 🚧 `/import` screen — packaged-app GUI pass

The most complex screen we've built (per-row honest statuses, selection, the
resolver consent panel) and it has only ever been type-checked. Everything below
is reproducible against the dev Mac's real Valet + Herd install unless marked
CLEAN-VM.

**What a full import would CHANGE on this machine — read before clicking:**
importing a `.test` site installs `/etc/resolver/test` (one admin prompt). That
is a plain create, NOT a takeover — the file is absent today, so rexenv owns it
outright and "Remove system changes" deletes it. It also creates real site rows
pointing at real project folders and issues certs. Deleting those sites later
leaves the folders untouched (Stage 0 guarantee). **Import ONE site, not all 19.**

1. **Populated list.** Sites → the "N sites found in Valet or Herd" banner
   appears (19 importable today). Open it. Expect **32 rows, 19 ready**, header
   count matching, and two source cards (Herd `.test`, Valet `.test`) with
   Herd's "parked folder ~/Herd has no sites in it" note.
2. **Dangling symlinks** — `back.test`, `bl.test`, `ealite.test`, `eatest.test`,
   `front.test`, `learn-valet.test`, `learn-wp.test`, `storeware-reviews.test`,
   `valet-wp-learn.test`, `wpdcd.test`. Each must read "can't import" AND name
   its missing target (e.g. `/Users/wpdev/bl`). Checkbox disabled.
3. **Leftover configs** — `abc.test`, `eatest.dev`, `wp-dev.test.test`: "leftover
   config with no site folder". Note `eatest.dev` proves a conf on a TLD the
   config never mentions still appears.
4. **Dedupe** — 11 rows carry "also in Valet" (Herd's copy won). No domain
   appears twice.
5. **PHP pins** — `strata.test` 8.5, `srdi.test`/`tr.test`/`typingbcc.test` 8.4,
   `pma.test` 8.2, `ea`/`eapro`/`adminer` 8.3 (both marker formats). Rows with no
   marker show the default.
6. **Docroot resolution** — a Laravel/Bedrock row must show `(serving public/)`
   or `(serving web/)`, not the project root.
7. **Selection** — select-all ticks only the 19 ready ones; the indeterminate
   state shows on a partial selection; disabled rows can't be ticked; the count
   in the bar matches; Rescan preserves nothing stale.
8. **Import one site.** Pick a small static/PHP one (`shop.test`, `snpz.test`,
   `rp.test`). Expect the admin prompt ONCE, up front, before any site is
   created. Watch the row flip to `imported`. Then: it appears in Sites with the
   **external** badge, `https://<domain>` loads THEIR files, and deleting it
   leaves the folder on disk.
9. **Continue-on-failure** — hard to force naturally; if you want it, rename a
   project folder between the scan and the import so one row fails, and confirm
   the rest still import and the summary names the failure.
10. **Cancel** — with several selected, cancel mid-run: the current site
    finishes, the rest report `skipped`, and no half-created site appears.
11. **Settings → DNS & SSL** — the "N sites can be imported" row appears and
    navigates to `/import`.
12. **Banner dismissal** — Dismiss on Sites, reload the app, it stays dismissed.

**CLEAN-VM only** (cannot be exercised here):
- The **empty state** (no Valet or Herd at all) — the "No Valet or Herd sites
  found" card. Most first-run users see this, so it matters as much as the
  populated list. Do NOT fake it by moving the dev Mac's Valet/Herd folders.
- The **resolver consent panel** — needs a foreign `/etc/resolver/test`; both
  TLDs read "absent" here, so the panel never renders. Covered by §F.
- **Hand-back row** in Settings — only appears for a BORROWED TLD, so it needs
  §F's takeover first.

## E) Other live/GUI items from the review & publish

- **🟢 Clean-Mac release QA** — `docs/SMOKE-TEST.md` on a fresh Mac / user account. The
  broadest confidence check; especially worth it before sharing with your QA friend.
- **🟢 `examples/*` live re-verification of the fixes whose full behavior needs real
  binaries** (the "live-check" scope notes in `docs/CODEBASE-REVIEW.md`). All are
  unit-tested for logic; these examples exercise them end-to-end against real services:
  - B22/B23 datadir cleanup — force a DB init failure (e.g. a deliberately-broken bootstrap)
    and confirm the next start re-inits cleanly rather than starting on a corrupt datadir.
  - B4 submodule clean-clone — `examples/repo_clone_check` against a **submodule** repo:
    it should clone with empty submodule dirs and exit 0 (not error).
  - B24 wp-cli `--` — a plugin/theme operation still works (the argv reorder didn't break
    real wp-cli parsing).
  - B20 override-port reap-guard — the runtime refusal path (needs two override backends).
- **🟢 Deferred-pass items verified only by inspection (all unit-tested; these exercise the
  wiring end-to-end in a real running stack — POST-PUBLISH nice-to-have, NOT gates):**
  - **B28** (`b1c8dfe`) adopt binary-wiring — with an adopted FrankenPHP or Apache override
    backend running, make an env-var or PHP-settings change that forces a backend restart.
    *Expected:* the backend restarts promptly — no resolve/download stall under the services
    lock (the recorded `frankenphp_bin`/`httpd_dir` is used).
  - **B29** (`030a545`) adopted-service reap — with an adopted DB engine running: (a) induce a
    single transient probe miss → *the service is NOT reaped* (still shown running, no
    restart-failed); (b) genuinely stop the adopted DB → *reaped after ~2 watchdog ticks
    (~20s)* and respawned cleanly. Confirms `owned_master` (marker), not bare `alive()`, is the
    probe.
  - **B7** (`dc77f67`) probe group-kill — `examples/repo_clone_check` (or a `repo add`) against
    a **slow / black-holed** git remote (e.g. a firewalled host, or add a 31s+ hang).
    *Expected:* the probe times out promptly at the cap and returns an error — and `pgrep ssh`
    shows **no orphaned ssh** left behind (the group kill took the grandchild).
  - **B20** (`3477760`) override-port backfill — on a real install that ALREADY has ≥1 FrankenPHP
    and/or Apache site created BEFORE this build, launch once. *Expected:* each existing override
    site keeps its **exact current backend port** (compare `lsof -iTCP -sTCP:LISTEN` on 8200–8399
    before/after, and the site still serves) — the migration preserves non-colliding ports.
- **🟢 B32 signing/notarization** — N/A for the ad-hoc tap path you've chosen. If you ever
  want a Gatekeeper-clean, no-`xattr`-needed distribution, `docs/SIGNING.md` has the exact
  steps (one config change + notarization env vars).

---

## I) ✅ Database import (Stage 2) — live DBngin source check + packaged GUI pass

**PASSED 27 Jul 2026 — all 12 steps, human-verified on the packaged app.** The two
load-bearing steps behaved: with DBngin stopped, the UI contradicted the plist's
`Status = started` and refused naming host, port and the fix (step 4); and the drift
demonstrated itself — an edit made through the site did not appear in rexenv's copy
(step 7). DBngin's MySQL 8.0.27 handshake also confirmed the last open pre-auth
identification case (plan §2.1).

The sandbox proves the machinery (`db_dump_check`, `db_restore_check`); this pass proves
it against a REAL source and the packaged UI. **The user's side of the bargain (D6): rexenv
never starts or stops their database server, so step 2 is yours.**

**Preconditions — in this order:**

1. Rebuild + reinstall; confirm the commit via Settings → About or `rex version`.
2. **Start DBngin's MySQL 8.0.27 yourself** (DBngin.app → start the engine). rexenv will
   refuse honestly if 3306 is silent — that refusal is itself checkable (step 4).
3. Have `ea.test` imported as a site (Stage 1) or import it now.

**The pass:**

4. WITH DBNGIN STILL STOPPED first: SiteDetail → ea.test → Database → **Import database**.
   Expect the honest per-site refusal naming host, port and what to do ("start it in
   DBngin, then re-scan") — NOT a timeout, NOT a generic error. The plist claims
   `started`; the UI must not believe it.
5. Start DBngin's MySQL. Import again. Expect phases check → copy → start → restore →
   finish, progress monotonic, ≤99 until the settle.
6. **The interim state (§9 — the state users actually live in).** After success:
   - the summary reads "Imported — not yet connected", names `ea`, the table count, size
     and source, and says the site STILL reads the old database and the two DRIFT;
   - the Sites page shows the "DB imported · not connected" badge on ea.test — same
     wording as the summary implies, because both render one `DbImportRecord`;
   - `DB_USER` in ea's wp-config is root, so the copy must state the 3-key change
     (host + user + empty password), not just DB_HOST.
7. Verify the copy is real: SiteDetail → Database (Adminer) → the `ea` database exists
   on rexenv's MySQL with the expected tables. The SITE meanwhile still serves from the
   old database (edit a post title via the site, confirm it does NOT appear in rexenv's
   copy — that's the drift, demonstrated).
8. **Their side untouched:** DBngin still runs, `ea` on 3306 intact (`shasum` of their
   datadir is overkill — check table counts via any client, or just that the site still
   works when pointed at it).
9. Cancel path: start an import, cancel during the copy. Expect "cancelled", no artifact
   left (Settings shows no leftover), site row unchanged, DBngin untouched.
10. Failure keep: stop DBngin's MySQL MID-copy (this is the one legitimate way to kill a
    dump). Expect a frozen failed state naming the kept dump file; Settings →
    "Leftover database dumps" lists it with a working Delete.
11. Batch: `/import` → tick "also copy databases" → import a small WP site. Expect the
    per-row "DB copied" chip, and a site with no database config to read "DB skipped"
    with the reason — never a failure.
12. Concurrency guard: while a database import runs, Retry/provision for the same site
    must refuse with "a database import is running for …", and vice versa.

---

## Publish-blocking summary

| # | Check | Status |
|---|---|---|
| A | Apple-Silicon ad-hoc launch (de-quarantine → launches) — **re-run on the fresh `0e57f11c…` dmg** | 🚧 **do before announcing the tap** |
| B | Uninstall removes the root :443 daemon | 🚧 do when convenient (tears down your edge) |
| C | B31 CSP packaged smoke test | ✅ done |
| D | Full tap install dry-run (after Release + tap push) | 🚧 do once the dmg is released |
| E | Clean-Mac QA + example live-checks + deferred-pass wiring (B28/B29/B7/B20) + (deferred) signing | 🟢 nice-to-have |
| I | Database import: live DBngin source + packaged GUI pass (user starts DBngin) | ✅ passed 27 Jul 2026 |
