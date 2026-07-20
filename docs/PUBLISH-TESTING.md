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

## A) ✅ PASSED (2026-07-20) — Apple-Silicon ad-hoc launch test — THE gate for the tap being real

**RESULT:** the universal `rexenv_0.1.0_universal.dmg` (sha256 `d48bc8ba…`, built 2026-07-20 with all
review fixes + cleanup) launches **clean after `xattr -rd com.apple.quarantine`** on Apple Silicon.
The ad-hoc tap approach is validated end-to-end — de-quarantine → launch works. Gate cleared; the tap
is real. (Canonical cask sha256 still to be recomputed from the exact GitHub Release asset — see §D.)

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
- **🟢 B32 signing/notarization** — N/A for the ad-hoc tap path you've chosen. If you ever
  want a Gatekeeper-clean, no-`xattr`-needed distribution, `docs/SIGNING.md` has the exact
  steps (one config change + notarization env vars).

---

## Publish-blocking summary

| # | Check | Status |
|---|---|---|
| A | Apple-Silicon ad-hoc launch (de-quarantine → launches) | 🚧 **do before announcing the tap** |
| B | Uninstall removes the root :443 daemon | 🚧 do when convenient (tears down your edge) |
| C | B31 CSP packaged smoke test | ✅ done |
| D | Full tap install dry-run (after Release + tap push) | 🚧 do once the dmg is released |
| E | Clean-Mac QA + example live-checks + (deferred) signing | 🟢 nice-to-have |
