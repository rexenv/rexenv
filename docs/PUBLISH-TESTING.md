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

## Publish-blocking summary

| # | Check | Status |
|---|---|---|
| A | Apple-Silicon ad-hoc launch (de-quarantine → launches) — **re-run on the fresh `0e57f11c…` dmg** | 🚧 **do before announcing the tap** |
| B | Uninstall removes the root :443 daemon | 🚧 do when convenient (tears down your edge) |
| C | B31 CSP packaged smoke test | ✅ done |
| D | Full tap install dry-run (after Release + tap push) | 🚧 do once the dmg is released |
| E | Clean-Mac QA + example live-checks + deferred-pass wiring (B28/B29/B7/B20) + (deferred) signing | 🟢 nice-to-have |
