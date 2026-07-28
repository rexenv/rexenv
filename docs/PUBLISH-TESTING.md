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

## J) ✅ Connection rewrite (Stage 3) — packaged GUI pass on ea.test

**PASSED 28 Jul 2026 — all 12 steps, human-verified on the packaged app
(`44b6a6d`), no deviations reported. Stage 3 SHIPPED.**

The sandbox proves the machinery end to end (`config_rewrite_check`, 11 live
assertions, passed 28 Jul 2026); this pass proved it against the REAL site and
the packaged UI. Steps kept for re-runs. Preconditions: `ea.test` imported (Stage 1) with its database
imported (Stage 2, state "imported · not connected"), rexenv's MySQL running,
and `shasum ea's wp-config.php` noted BEFORE anything below.

1. **The consent card.** SiteDetail → ea.test → Database. Expect the diff card:
   exactly TWO changed pairs (`DB_HOST` → `127.0.0.1:13306`, `DB_USER` →
   `rex_ea_test`), **no password anywhere on screen**, consent unchecked, and
   the backup note carrying the limit ("the diff can't contain your password —
   the backup is the whole file, so it does").
2. **Engine stopped = honest refusal naming OUR page.** Stop MySQL (Databases
   page), tick consent, Apply. Expect "rexenv's own MySQL … isn't running —
   start it from the Databases page", NOT a DBngin message, NOT a timeout.
   Nothing written (`shasum` unchanged). Restart MySQL.
2b. **fileChanged is a normal state** (must run BEFORE the first apply — the
   consent card only shows while not connected). With the card open, add a
   comment line to wp-config in an editor, then tick + Apply. Expect "changed
   since the diff was shown — nothing was written", neutral styling, the
   refreshed diff already below. **Remove the comment** (so step 8's
   byte-identity check stays meaningful), let the card refresh.
3. **Apply.** Apply with consent. Expect the "verified: the rewritten
   settings sign in to the rexenv copy" toast; the Sites badge flips to
   **DB connected** (green); the connected panel's wording claims the sign-in,
   not "the site now uses this database".
4. **The file.** `diff` old vs new wp-config: exactly the two lines; file mode
   unchanged; backup exists at `<app-data>/config-backups/<site-id>/wp-config.php`
   with mode 600; `config_rewrites` has one row with a digest.
5. **Drift now runs the OTHER way (the point of the whole stage).** Edit a post
   title through the site → it appears in rexenv's copy (Adminer), and does
   NOT appear in the old DBngin database (start DBngin to compare if wanted —
   read-only, their side untouched).
6. **HTTP upgrade.** The record's verified level should read `signin+http`
   (site was serving). If the edge is stopped during a later re-apply, expect
   `signin` only — "couldn't confirm over HTTP" must never block or unset.
7. *(merged into 2b — fileChanged needs the consent card, which only shows
   before the site is connected.)*
8. **Revert.** Revert from the connected panel. Expect: `shasum` equals the
   step-0 value (byte-identical), badge back to "DB imported · not connected",
   the interim panel returns, backup file + row gone.
9. **Edited-since refuses without force.** Apply again, hand-edit wp-config,
   Revert. Expect the named refusal + "Restore anyway" (danger); forcing
   restores the original and says edits since are lost.
10. **D2 delete confirm.** With a connected site (use a SCRATCH site, not ea):
    Delete. Expect three buttons, both outcomes NAMED ("…the site breaks on
    next load"), revert-then-delete as the default; after it, the project's
    config points back at the old database and the site row is gone.
11. **The socket free win (D5).** A linked site whose wp-config says
    `DB_HOST=localhost` (user root/their password, mirrored): with Stage 3's
    pools, the site serves and CONNECTS with zero file changes on MySQL.
    A MariaDB site's panel shows the "use 127.0.0.1:13307" note instead.
12. **Concurrency.** While a database import runs for a site, Apply/Revert
    refuse naming the import; while an apply runs, provision/import refuse.

---

## K) 🚧 The whole migration as ONE journey — scan → serve → copy → connect → revert → delete

Not a re-run of §G/§I/§J: one unbroken arc on one site, exercising the SEAMS
between stages, ending not with "it worked" but with **"everything of theirs
is exactly as it was"** — the reversibility promise checked as a single fact.

**Rebuild first.** The installed app predates the §C1/§C2 UI fixes. Build at
this commit or later (`git log -1 --format=%h` at build time; Settings →
About must show it).

### Safety — read before step K0

- **Writes into a real project:** K10 (two lines of the primary's
  `wp-config.php`, backed up first, reverted at K14/K16 — byte-identity is
  asserted). K9 has YOU add/remove a comment by hand.
- **Persists on OUR side until the deletes:** site rows + certs, the copied
  databases, the dedicated `rex_<slug>` user, `config-backups/` entries.
- **Destructive if misclicked:** the typed-confirm overwrite (K8 — drops
  OUR copy only), "Restore anyway" (not scheduled — skip unless a step says
  otherwise), "Delete without reverting" (NOT used in §K; use only the
  revert-then-delete or plain paths as written).
- **Targets, from your real scan:** primary = `photocontest.test` (db
  `photocontest`), secondary = `typingbcc.test` (db `typingbcc`) — swap
  either for another WP site whose content you'd shrug at losing, but NOT
  `ea.test` (the §I/§J testbed — §K uses it read-only as the
  "already imported" exhibit) and not a site whose DBngin database you
  treasure (nothing here writes to their DBs, but you'll be signing into
  the sites and pressing delete buttons near them).
- **If you stop halfway:** nothing of theirs is harmed at ANY stopping
  point; what's left behind is rexenv state (site rows, copies) you can
  delete later. The one state needing an action: stopped between K10 and
  K14, the primary's wp-config points at rexenv's copy until you Revert
  (the card's button, any time).

### Preconditions

1. Rebuild + reinstall + confirm the commit. Start the rexenv stack.
2. Quit Herd/Valet (`:443` must be ours — Services shows the edge green).
3. **Start DBngin's MySQL yourself** (D6 — rexenv never will).
4. Primary and secondary are NOT in rexenv (delete leftovers from earlier
   passes if present — that deletion is outside §K's scope).

### K0 — the BEFORE capture (the reversibility baseline)

```sh
B=~/rexenv-k-before; mkdir -p "$B"
PROJ=/path/to/photocontest    PROJ2=/path/to/typingbcc     # from the scan rows
DB=photocontest               DB2=typingbcc
shasum "$PROJ/wp-config.php" "$PROJ2/wp-config.php" | tee "$B/wpconfig.sha"
find ~/.config/valet ~/Library/Application\ Support/Herd/config/valet \
  -maxdepth 2 -exec stat -f "%m %N" {} \; | sort | tee "$B/trees.mtime"
ls -l /etc/resolver/ | tee "$B/resolver.txt"
# Content tables only: serving a WP site writes options/transients through
# THEIR server (WordPress's normal life, not a rexenv write) — so wp_options
# is deliberately excluded and every deliberate content edit in §K happens
# only AFTER the site is connected to OUR copy.
mysql -h127.0.0.1 -P3306 -uroot -p -e \
  "CHECKSUM TABLE $DB.wp_posts, $DB.wp_postmeta, $DB.wp_users;
   CHECKSUM TABLE $DB2.wp_posts, $DB2.wp_postmeta, $DB2.wp_users;" | tee "$B/db.checksums"
```
(Their client/creds; any MySQL client on 3306 works. If `mysql` isn't on
PATH, use DBngin's bundled one.)

### The journey

1. **Scan.** `/import`: primary + secondary listed importable;
   `ea.test` shows **already imported** (a prior stage's state, visible and
   disabled — the first seam). Counts reconcile with §G's totals minus any
   sites you've since imported/deleted.
2. **Import the primary** (site only — no DB checkbox). Expect: one admin
   prompt at most (resolver already ours), row flips `imported`, Sites shows
   the row with **external** badge, NO DB badge, Running.
3. **It serves THEIR site.** `https://photocontest.test` shows the real
   site (§H: confirm it's not the fallback — the content must be the
   site's own). It works because it still reads DBngin — that's correct.
4. **Re-run: scan again.** Primary now **already imported** (disabled);
   nothing duplicated; New Site with the same domain refuses honestly.
5. **Interleave: import the secondary** (site only). Both rows healthy —
   per-site state, not "the current site".
6. **Copy the database.** Primary → SiteDetail → Database → Import
   database. Phases run; summary reads **"Imported — not yet connected"**;
   the panel says the site **still reads and writes the old database**
   (true — and note it's the PREVIEW-derived sentence now); Sites badge:
   **external + DB imported**. Adminer (below the card, now full-height —
   §C2 fix) shows the copy.
7. **Seam: the card and badge agree at every point from here on** — any
   disagreement is a finding.
8. **Re-run: copy again.** Import database again: expect the
   **typed-name confirm** (the name now exists on OUR engine); type it;
   converges — same settled state, no duplicates. (This is the legitimate
   re-copy; after K10 the same button must behave DIFFERENTLY — K11.)
9. **Failure paths, deliberately:**
   a. Databases → stop MySQL. Consent card → tick → Apply → honest
      **"rexenv's own MySQL … start it from the Databases page"** — OUR
      page, not DBngin; `shasum` unchanged. Restart MySQL.
   b. With the card open: add a comment line to wp-config in an editor,
      Apply → **fileChanged**, neutral ("nothing was written"), refreshed
      diff below; **remove the comment**; card refreshes clean.
   c. (verifyFailed has no safe manual trigger — see Honest limits.)
10. **Connect.** Diff = exactly two pairs (`DB_HOST`, `DB_USER` →
    `rex_photocontest_test`), no password anywhere; backup note carries the
    password-in-backup limit; tick; **Apply and verify** → "verified: the
    rewritten settings sign in…" toast; badge flips **DB connected**;
    connected panel wording claims the sign-in, not "the site uses it".
11. **Seam: the self-source guard.** Import database AGAIN now → expect the
    honest **ThisSite** refusal ("already reads and writes … on rexenv's own
    engine — nothing to import"-class), NOT a copy. This is also why
    "re-import resets connected → imported" is unreachable from connected —
    the guard wins first; the reset path exists only while still `imported`
    (K8). Both behaviours are correct; note both.
12. **Prove it uses OUR database.** Site → wp-admin → edit a post title.
    The edit appears in rexenv's Adminer copy; their DBngin `photocontest`
    must NOT change (the final checksums assert it — don't check by writing
    anything their side).
13. **Interleave under load:** start the secondary's DB import; while its
    copy phase runs, try a THIRD import (any site) → **"a database import is
    already running (for typingbcc.test) — one at a time"**; and Apply on
    the secondary's card → refusal naming the import. (Small DBs finish
    fast — if the window's too short, observe at least the first refusal.)
    Secondary settles: **Imported — not yet connected**, and the primary's
    connected state is untouched.
14. **Revert, re-apply, revert** (the backup lifecycle twice): Revert →
    toast, badge back to **DB imported**, interim panel returns with the
    (again true) still-reads-old sentence, `shasum "$PROJ/wp-config.php"`
    equals `$B/wpconfig.sha`. A second Revert is UNREACHABLE (the button
    left with the state — that's the convergence). Apply again (fresh
    backup of the same original) → connected again → Revert again →
    byte-identical AGAIN.
15. **Dedicated user lifecycle (D3).** After the final revert the user
    remains (inert):
    `"$HOME/Library/Application Support/dev.rexenv.rexenv"/bin/mysql-*/bin/mysql \
      --no-defaults -h127.0.0.1 -P13306 -uroot -e \
      "SELECT user,host FROM mysql.user WHERE user LIKE 'rex_%'"`
    → `rex_photocontest_test` on localhost + 127.0.0.1 only.
16. **Delete the primary** (state: imported, reverted). Ordinary confirm
    ("… its database, and its certificate"); delete. Then verify: row gone;
    `$PROJ` folder intact; rexenv's copy dropped (Adminer); the K15 query
    now returns NO `rex_photocontest_test` (dropped BY THE RECORD — D3's
    other half). Delete the secondary too (its copy + row go the same way).
17. **The AFTER capture.** Re-run every K0 command into `~/rexenv-k-after`
    and diff:
    ```sh
    A=~/rexenv-k-after; mkdir -p "$A"   # …repeat the K0 commands into $A…
    diff "$B/wpconfig.sha"  "$A/wpconfig.sha"     # identical
    diff "$B/trees.mtime"   "$A/trees.mtime"      # identical — their trees untouched
    diff "$B/resolver.txt"  "$A/resolver.txt"     # identical
    diff "$B/db.checksums"  "$A/db.checksums"     # identical — their data untouched
    ```
    **Four empty diffs are the verdict.** The site edit of K12 lives only in
    a copy that no longer exists; their environment is exactly as found.

### Honest limits — what §K cannot cover on this machine

- The **empty state** (no Valet/Herd) and the **resolver takeover /
  hand-back / drift** paths (§F): no foreign `/etc/resolver/test` exists
  here — clean-VM only, as before.
- **verifyFailed** live: no safe manual trigger (it needs the engine to die
  between write and verify, or credentials to rot mid-flight). Proven in
  the sandbox example (WrongTarget after revert) and rendered in §C1.
- **Laravel/.env end-to-end**: no Laravel project with a `.env` exists here
  — the editor is unit-tested + sandbox-proven only.
- **Non-root credential shapes** (Stage 2 mirror, host-only rewrite): every
  wp-config on this machine connects as root.
- **MariaDB-engine journey + the D5 note in anger**; the **unpinned-PHP
  choice** (all markers here are 8.2–8.5); **Valet proxy rows** (none
  exist).

### Verdict

- §K run on: ____ · app commit: ____ · result: **PASS / FAIL** ____
- Deviations (step → what differed):
  - ____

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
| J | Connection rewrite (Stage 3): packaged GUI pass on ea.test | ✅ passed 28 Jul 2026 |
| K | The whole migration as ONE journey (seams + reversibility) | 🚧 rebuild, then run |
