# PUBLISH-TESTING.md — live/GUI checks before publishing

Everything below needs a **real launch, a GUI action, or root/launchd** — i.e. things
that can't be verified by `cargo test` / `tsc` / `brew style` and must be run on a real
Mac by hand. Each item lists the **exact command**, **expected result**, **why it
matters**, and whether it's **🚧 publish-blocking** or **🟢 nice-to-have**.

**Already verified (no action needed):**
- All code fixes are unit-tested and green — the gate is `./scripts/verify-full.sh`
  and its own `verify-full: all green` line is the verdict. **No test count is
  written here on purpose**: this bullet used to read "336 passed / 0 failed",
  which was a number generated in one place and copied to another, i.e. the same
  shape `docs/TESTING.md` stopped writing counts down for and the same shape the
  ledger tally is script-enforced against. Run the script; read its output.
- The tap **static** side: the built app is validly ad-hoc signed (`codesign --verify`
  passes), universal `x86_64 arm64`, and the cask — now living in its own repo,
  `github.com/rexenv/homebrew-tap` — passes `brew style --cask rexenv/tap/rexenv`.
- **C) B31 CSP packaged smoke test — ✅ DONE** (packaged build, every flow worked, no
  `Refused to … violates CSP` lines). Not re-listed here.

---

## A0) 🚧 Artefact integrity — check the shipped binary, PER SLICE

**Why this is its own step, and why "per slice" is the whole point.** A universal
binary is two binaries in a trench coat. Anything embedded at compile time is
embedded *per architecture*, so a build that carried it in one slice and not the
other would work perfectly on the machine that built it and fail on every user
with the other chip. That is the arm64-DMG mistake (§A's history) in a subtler
shape: last time the artefact was thin and it was obvious; a half-populated fat
binary is invisible to `lipo -archs`, which only reports that both slices exist.

Run before §A, on the dmg you are about to test. Two minutes.

```sh
cd <your rexenv checkout>
# Read the version from the manifest rather than typing it — this line named
# 0.1.0 through two releases, and a stale path silently checks nothing (the
# `ls` below is what catches it, but only if you notice the count).
V=$(node -p "require('./package.json').version")
APP="src-tauri/target/universal-apple-darwin/release/bundle/macos/rexenv.app"
DMG="src-tauri/target/universal-apple-darwin/release/bundle/dmg/rexenv_${V}_universal.dmg"

# 1. Identity of what you are about to test — record these next to the §A result.
shasum -a 256 "$DMG"
git rev-parse --short HEAD; git status --porcelain | wc -l   # EXPECT: 0 uncommitted
ls src-tauri/target/universal-apple-darwin/release/bundle/dmg/*.dmg | wc -l  # EXPECT: 1

# 2. Both slices present in both binaries.
#    NOTE: `rex` is in Contents/MacOS, NOT Contents/Resources. This line said
#    Resources until 15 Aug 2026 and `lipo` answered "can't open input file" —
#    a check that cannot find its subject proves nothing about it, and this is
#    the copy a HUMAN runs (release.yml has always had the right path, and while
#    the repo is private CI does not run at all, so the doc IS the check).
lipo -archs "$APP/Contents/MacOS/rexenv"     # EXPECT: x86_64 arm64
lipo -archs "$APP/Contents/MacOS/rex"        # EXPECT: x86_64 arm64

# 3. Embedded payloads are in the ARM SLICE ON ITS OWN, not merely somewhere in
#    the fat binary. Today that is the vendored `wp dist-archive` PHP tree
#    (core::wp_packages) — the feature exists precisely so the command is
#    CARRIED rather than resolved from the machine, and a tree present in only
#    one slice would quietly restore the machine dependency for half of users.
lipo -thin arm64 "$APP/Contents/MacOS/rexenv" -output /tmp/rexenv-arm64
strings -a /tmp/rexenv-arm64 | grep -c Dist_Archive_Command   # EXPECT: > 0
lipo -thin x86_64 "$APP/Contents/MacOS/rexenv" -output /tmp/rexenv-x86
strings -a /tmp/rexenv-x86 | grep -c Dist_Archive_Command     # EXPECT: > 0
rm -f /tmp/rexenv-arm64 /tmp/rexenv-x86

# 4. The signature, over the whole bundle including the sidecar.
codesign --verify --deep --strict --verbose=2 "$APP"   # EXPECT: valid on disk
```

**Keep this block equal to `release.yml`'s "§A0 artefact integrity" step.** They
have drifted once already — in the direction where the doc is wrong and CI is
right, which is the dangerous one while the repo is private and CI never runs.

**Expected:** clean tree, one dmg, both slices in both binaries, and a non-zero
count in **each** slice separately. A zero on either side is a HOLD — do not run
§A on that artefact, rebuild it.

**Add a line here whenever something new is compiled INTO the binary.** The check
is only as complete as its list of payloads, and a payload nobody added is the
one that ships in one slice.

### A0-b) The UPDATE archive, and §A0 on what comes back out of it

*Added 6 Sep 2026 with in-app self-update. Scripted — `scripts/release-assets.sh` runs it
as part of `pnpm release:mac`, and `release.yml` re-runs it with `--check`. It is written
down here because the reason is not obvious from the script.*

§A0 above checks the `.app` that `tauri build` produced. **That is not what a self-updating
user receives**: they receive whatever comes back out of `rexenv_<V>_universal.app.tar.gz`.
Checking one artefact and shipping another is the same gap the cask closes by hashing the
DOWNLOADED asset rather than the local build.

So the script asserts, on the EXTRACTED bundle: both binaries universal, `Dist_Archive_Command`
per slice, the update public key per slice (a build whose Intel half cannot verify a
descriptor silently never updates on Intel), `codesign --verify --deep --strict`, and the
Info.plist version equal to the release. Plus the archive's own layout — exactly one
top-level `rexenv.app/` entry and no AppleDouble `._` members, because the in-app extractor
strips exactly one component and both other shapes produce a broken install.

If it fails, do not publish: the dmg would install fine and every in-app update from it
would break.

## A) ✅ 0.6.1 — PUBLISHED, and the release the updater was finally RUN on

`rexenv_0.6.1_universal.dmg`, sha256
`c427e9e6b034bac2be4801b1f81ef4afbd5823e0bd0c7078c7de4b6b7ffb9f46`, 29,143,794 bytes.
Update archive `rexenv_0.6.1_universal.app.tar.gz`, sha256
`39f5044969575c83b9e4ca0cacd3202c2b29551c9ae78cef840dedd4697777fc`, 28,735,832 bytes.
Source `f748698`, clean tree, `verify: all green`. **§A0 ✅ by hand:** one dmg, both
binaries `x86_64 arm64`, `Dist_Archive_Command` ×5 and `RELEASE_PUBKEY` ×2 in EACH slice,
codesign valid, Info.plist 0.6.1. (The About page's changelog URL is not visible to
`strings` on the binary — Tauri brotli-compresses embedded web assets — so it was checked
in `dist/assets/`, which is the artefact that gets embedded.)

**PUBLISHED 7 Sep 2026, 09:35:43Z** (release id 383967598). Cask bumped to 0.6.1 /
`c427e9e6…`. Descriptor published from `rexenv/runtimes`: **serial 1 → 2**, naming 0.6.1,
`check-app-manifest: all green` from this tree.

**A CDN lesson, free:** `check-app-manifest.sh` run immediately after the publish still
read **serial 1 / 0.6.0** — `raw.githubusercontent.com` caches for a few minutes. That is
exactly why the workflow reads its own result back through the **API** rather than raw; had
it checked raw it would fail on every successful run. The same delay also produced a live
demonstration of the forgotten-second-click warning, firing correctly for a state that was
only briefly true.

### §M — the update itself, MEASURED (the T11 gate)

The installed copy was 0.6.0; the owner opened Settings → About and pressed Install.
Before and after, on the same Mac:

| | before | after |
|---|---|---|
| `CFBundleShortVersionString` | 0.6.0 | **0.6.1** |
| cdhash | `e7036221…` | **`ce45c7a5…`** — a genuinely different bundle, not a rewritten one |
| `codesign --verify --deep --strict` | valid | **valid** |
| quarantine xattr | none | **none** |
| `lipo -archs` | — | **`x86_64 arm64`** — universal survived the swap |
| `rex --version` | `rex 0.6.0 (55eae12)` | **`rex 0.6.1 (f748698)`**, agreeing with About |
| DNS agent | pid 43710 | **pid 47967, running from the NEW bundle** |
| `.rex` resolution | 127.0.0.1 | **127.0.0.1** |
| `.rexenv-update-*` leftovers | none | **none** — swept after the healthy launch |

`rex status` afterwards printed `update   rexenv 0.6.1 can be installed from Settings →
About` once a newer build existed again (see below), which is #536's read-everywhere claim
demonstrated on a real machine rather than in a test. **Services outlived all of it** —
MySQL, MariaDB, seven php-fpm pools, Nginx, Caddy and Mailpit were still running with the
same pids afterwards.

**The finding, and it was ours: `auto_updates` does NOT stop `brew upgrade --cask rexenv`.**
Run as part of this gate, the named form reinstalled 0.6.0 over the self-updated 0.6.1 and
reported `Upgraded 1 requested outdated package` — no mention of going backwards. Homebrew
skips auto-updating casks when upgrading *everything*; naming one is an explicit request it
honours. It also installs what the **local tap checkout** says, which on that Mac was a
release behind, so "the window is only minutes" was too comfortable a sentence. Nothing
broke: the app offered 0.6.1 again at its next check, which incidentally proved the recovery
path the tap README promises. Corrected in three places that all carried the same wrong
sentence — the cask comment, the tap README (`626d1df`) and `docs/RELEASING.md` — plus the
SMOKE leg, which now names the command to run and the command not to.

**Run a second time, after the brew downgrade** — 0.6.0 → 0.6.1 again, which is why the
numbers above have a third column in the log: the DNS agent went 43710 → 47967 → 65661, one
re-exec per swap. **The owner reports no dialog of any kind on either update**, and
`log show --predicate 'subsystem == "com.apple.TCC"'` records no prompt for rexenv in the
window — so the two legs that most needed a human closed here: Gatekeeper / App Management
on the replaced bundle, and which permissions come back. Both are recorded in
`docs/SMOKE-TEST.md` with the caveat they deserve: one Mac, grants already settled, re-record
per macOS major.

`rexenv.log` carries the kickstart line four times, including
`the resolver agent is running 0.6.1 f748698 but this build is 0.6.0 55eae12 — kickstarting
it` — the agent NEWER than the app, after the downgrade. T5's check is a mismatch test, not
a "less than" one, and only a downgrade could show that.

`brew upgrade --dry-run` does not list rexenv: the unnamed form really does leave it alone,
which is the half of `auto_updates` that works as documented.

**§M found a real bug, which is what a human gate is for.** With a tunnel up, Install →
"Keep sharing" left the new bundle installed and the process on the old build — and the card
said nothing, offering Install again as though the update had not happened. The plan
specified that state (§8: *"…takes effect when rexenv next opens"*); it was never built,
because the apply assumes the window goes away as part of succeeding, which is true of every
path except the one a person can choose. Fixed the same day: the state is read from the
notice row the swap already writes, so it survives the window closing, suppresses the offer
and clears the tray. Ledger #539, L0 + L2, plant-proven.

**Also observed on this run:** the tray's `Update to …` item behaved (opened About, installed
nothing), the offer arrived with no click, and the startup notice read `rexenv 0.6.1` —
agreeing with About and with `rex --version`.

**Legs of §M still open:** "dark when current" as the CARD renders it, the offline check,
the consent sentence's Homebrew line on a cask-installed copy, and the public-share path —
which RAN but is left open on purpose, since what it proved is that the state it checks did
not exist. Re-run it on the next release against the fix.

## A) ✅ 0.6.0 — PUBLISHED (§A0 ✅ measured · §A ✅ asserted · §M waits for 0.6.1 by construction)

`rexenv_0.6.0_universal.dmg`, sha256
`be81bd17ed93d1afc6fbbe1cb860f8ee27e45beb8d977acf87df3b76461e5c6e`, 29,154,116 bytes.
Update archive `rexenv_0.6.0_universal.app.tar.gz`, sha256
`48cafbf9cb58aa5e54430940c4fb03d4b34e0831c1e6ed2790183952af0ac4ae`, 28,749,924 bytes —
**the first release to carry one**, and what an in-app update actually downloads.
Source `55eae12`, tree clean (0 uncommitted). Built 7 Sep 2026 by `pnpm release:mac`
after `verify.sh` said `verify: all green` on that commit — **the first full run of that
script end to end**, which is the item `docs/TODO.md` had been carrying as owed since T9.
Tag `v0.6.0` cut LOCAL on `55eae12` (annotated; not pushed — the pre-push hook refuses
`v*` while the repo is private, and pushing one would spend 10×-billed macOS minutes
building a dmg nobody can download).

Draft on the tap, release id **383939490**, all four assets state `uploaded`; the API
`digest` of the dmg and of the archive each equal the local sha256 above, so the bytes
survived the wire.

**§A0 ✅ run by hand 7 Sep 2026:** clean tree, exactly one dmg; `rexenv` and `rex` both
`x86_64 arm64`; `Dist_Archive_Command` ×5 in EACH slice; **`RELEASE_PUBKEY` ×2 in each
slice** — new this release, and the one that matters most here, because a build whose
Intel half cannot verify a descriptor would silently never update on Intel while looking
perfect on the Mac that built it; `codesign --verify --deep --strict` valid on disk and
satisfies its Designated Requirement; Info.plist `0.6.0`, `LSMinimumSystemVersion` 15.0.

**§A0-b ✅ (scripted):** `release-assets: all green` — §A0 re-run on the bundle that comes
back OUT of the archive (universal, per-slice payload and key, codesign, version), plus the
archive layout: exactly one top-level `rexenv.app/`, no AppleDouble members.

**0.6.0 PUBLISHED 7 Sep 2026, 08:46:07Z** at `homebrew-tap/releases/tag/v0.6.0` (release
id 383939490). §A is asserted by the owner's publish click, which is the sign-off by rule.
Cask bumped by `update-cask.yml` (run 34102469402, triggered by hand rather than waiting
for the 15-minute poll) to version 0.6.0 / sha256 `be81bd17…`, equal to the local hash.
`auto_updates true` survived the bump, which is the property that stops brew and the app
installing over each other.

**The descriptor — the second click, done.** `rexenv/runtimes` → "Publish app update
manifest", dry run first (run 34103119789) then for real (34103284349, commit `1b8d502`):
**serial 0 → 1**, naming 0.6.0, sha256 `48cafbf9…` re-hashed from a fresh download rather
than taken from the API, signed with `faa52f96…` — the log's own words, *"matches the key
shipped builds pin"*, so the reviewer-gated environment really does hold the key this
binary trusts. The run read the committed document back through the API and compared it
byte-for-byte with what it signed. Verified from this side too:
`check-app-manifest: signature OK — serial 1 names rexenv 0.6.0` /
`the named asset exists and its digest matches` / `all green`.

**The first dry run FAILED, and that is the whole reason it exists.** `tar -tzf … | head -1`
in the publisher: head closes the pipe after one line, GNU tar takes the write error and
exits 2, and `set -o pipefail` killed the job — `tar: stdout: write error`, nothing else.
bsdtar on macOS ignores it, so the line passed every local test; the script is written on
a Mac and runs on ubuntu-latest. Fixed in `ee71943` (list once into a variable, and ask
`jq` for `first(...)` rather than truncating its output, which carried the same hazard).
No document was signed and nothing was published by the failed run.

**§M cannot run on 0.6.0 at all** — an install is only ever offered something NEWER, so
the update itself waits for 0.6.1. That is not a gap in this release; it is what "the
version that introduces a feature cannot use it" means.

## A) ✅ 0.5.0 — PUBLISHED (§A0 ✅ measured · §A ✅ asserted · SMOKE-TEST ✅ asserted, no contemporaneous record)

`rexenv_0.5.0_universal.dmg`, sha256
`86831ecbeadcc3552748d63134782a040239004a571c3684e7cd652021cdee8e`, 29,002,961 bytes.
Source `2c433ae` — measured: `rex --version` off the bundle prints `rex 0.5.0 (2c433ae)`.
Built locally per `docs/RELEASING.md`'s interim flow after `verify-full.sh` said
`verify-full: all green` on that commit (its first two runs were red on the gate's OWN
tiers, fixed in `2c433ae` itself — see that commit). SMOKE-TEST run by the owner on
this dmg, 5 Sep 2026 (asserted, no contemporaneous record — same as 0.4.0). Tag `v0.5.0`
cut LOCAL on `2c433ae` after that (annotated; not pushed, the pre-push hook refuses `v*`
while the repo is private). Draft created on the tap the same day: both assets state
`uploaded`, the dmg's API `digest` equals the local sha256 above — the sidecar says the
same — so the bytes survived the wire.

**§A0 ✅ run by hand 5 Sep 2026:** clean tree (0 uncommitted), exactly one dmg;
`rexenv` and `rex` both `x86_64 arm64`; `Dist_Archive_Command` ×5 in the arm64 slice
and ×5 in the x86_64 slice; `codesign --verify --deep --strict` passes on the `.app`.

**0.5.0 PUBLISHED 5 Sep 2026, 13:22:19Z** at `homebrew-tap/releases/tag/v0.5.0` (release
id 383250086), cask bumped by `update-cask.yml` (`a7f62c6`, 13:29:12Z — the scheduled run
seven minutes after publish) to version 0.5.0 / sha256 `86831ecb…`, equal to the local
hash above. §A is asserted by the owner's publish click, which is the sign-off by rule.

**The cask's quarantine step, measured 6 Sep 2026 — the one gate the dmg cannot show.**
`brew reinstall --cask --debug rexenv` on a dev Mac, against the published 0.5.0 asset:
brew propagated quarantine from the cached dmg onto the staged app, copied the xattrs to
`/Applications/rexenv.app`, then ran `Installing artifact of class
Cask::Artifact::PostflightSteps`; the installed app ended with **zero**
`com.apple.quarantine` attributes and `codesign --verify --deep --strict` still passing.
`spctl -a` still says `rejected` — expected and unchanged: the build is ad-hoc signed,
not notarized, and removing the quarantine attribute is exactly how it launches anyway.
**Run this with `--debug`, or it proves nothing:** the ordinary output prints no step
line at all, so a skipped step and a working one read identically. Services outlived the
app swap as they must — 16 stack processes still up and a real site answered 200 through
the edge afterwards. This closes the tap-side fix (`2a5d489`, `postflight_steps` +
`verified:` dropped) that the 0.5.0 bump's deprecation warnings prompted.

**Sanity after the bump — half done, and why.** `brew audit --cask --online` did NOT run on
this Mac: Homebrew refuses every `audit` with "Your Command Line Tools are too outdated"
(CLT for Xcode 26.3 wanted) before touching the cask, so its exit 1 says nothing about
the cask. What ran instead: `brew fetch --cask rexenv/tap/rexenv` → `✔︎ Cask rexenv
(0.5.0)` — the real download through the cask's `url`, sha256-checked by brew against
the cask, which is the property a user's `brew upgrade` depends on. Re-run the audit
once the CLT is updated. The fetch also surfaced a tap-side deprecation: `postflight`
→ `postflight_steps` (`Casks/rexenv.rb:66`), a warning today, tracked in TODO.

## A) ✅ 0.4.0 — PUBLISHED (§A0 ✅ measured · §A ✅ asserted · SMOKE-TEST ◐ asserted, no contemporaneous record)

**0.4.0 PUBLISHED 27 Aug 2026, 16:37:03Z** at `homebrew-tap/releases/tag/v0.4.0`, cask
bumped by `update-cask.yml` (`66b262d`, 17:43:44Z) to version 0.4.0 / sha256
`a7aecee7…`. Built locally 27 Aug per `docs/RELEASING.md`'s interim flow (`pnpm
release:mac`), because the repo is private and CI never saw this artefact.

`rexenv_0.4.0_universal.dmg`, sha256
`a7aecee7f59738c2803e6a0c7697b82c36ebda5186e60b9fefee85e1bf1e555d`, 26,275,202 bytes.

**Source: `d363e24` — MEASURED, not inferred, and that is new.** The dmg was mounted and
asked: `rex --version` off the mounted volume prints `rex 0.4.0 (d363e24)`, which is the
commit `v0.4.0` (annotated) points at. Every earlier row's commit rested on the tag plus
a build timeline because the artefact could not answer; this is the first release where
the bytes say it themselves (the fix landed 25 Aug in `3e19b83`, and this is it working).
The tag itself stays LOCAL — `scripts/git-hooks/pre-push` refuses a `v*` push while
`rexenv/rexenv` is private, and `git ls-remote --tags origin` is empty for every release
so far, which is the expected state and not a missing step.

**§A0 ✅ run by hand 27 Aug 2026** — the release workflow's own script, copied as
`docs/RELEASING.md` step 4 says. All green: exactly one dmg in the bundle dir; `rexenv`
and `rex` both `x86_64 arm64`; `Dist_Archive_Command` present in EACH thinned slice (×5
in arm64, ×5 in x86_64 — a half-populated fat binary is invisible to `lipo -archs`,
which is why the check thins first); `codesign --verify --deep --strict` passes on the
`.app`.

**§A and `docs/SMOKE-TEST.md` — ASSERTED, and the record is honest about when.** The
sentence this paragraph used to carry ("both run on these bytes before publishing") was
written on 30 Aug (`bd71bfc`), three days after the fact, from memory. What the repo
itself records: `bbf7465` at 16:14:07Z on publish day lists SMOKE-TEST and §A as
"publish-blocking and human-only … outstanding"; the release went public at 16:37:03Z,
twenty-three minutes later; and no commit touched `docs/SMOKE-TEST.md` between 27 and
31 Aug (a full clean-Mac SMOKE pass leaves ticks and findings behind — every earlier one
did). So: the releaser's sign-off is that both gates ran on the draft's dmg (the blank-PHP
starter database, the built-in terminal, multisite through a tunnel, MCP M3 included),
and nothing written at the time backs the SMOKE half. It is recorded as ◐ — asserted,
not evidenced — and NOT re-run: 0.4.0 is on the tap and the next release re-runs the
whole set on its own bytes. **The rule this cost restates**: the receipt is written at
the moment the gate runs, in the same commit as the publish, or it is a memory — the
same lesson as 0.3.0's row, paid a second time in a milder form.

**Four-way hash match, verified 30 Aug 2026** — the check that caught 0.1.0's
placeholder hash, and the one 0.3.0's row had to be reopened for. The published asset
downloads **anonymously** (no token, as `brew` does: HTTP 200, 26,275,202 bytes) and
hashes to `a7aecee7…` — equal to what the cask pins, what the release API reports as the
asset `digest`, and what the `.sha256` sidecar says. So what a user installs is what was
gated. Downloads at that check: 5.

> **What this row cost, and why it is dated three days after the release.** 0.4.0 was
> published on 27 Aug and this file went on saying "BUILT and TAGGED … not published, no
> draft on the tap, no cask bump" until 30 Aug — the SAME failure 0.3.0's row exists to
> record, repeated one release later, with the added twist that the doc was now
> ACTIVELY WRONG rather than merely absent: a reader would have concluded the artefact
> was unpublished while five people had already downloaded it. The rule that failed is
> again "write the row afterwards". The release row belongs in the commit that publishes,
> the way the tick belongs in the commit that does the work.

## A-prev) ✅ 0.3.0 — PUBLISHED, §A0 ✅ (31 Aug 2026), §A CLOSED BY RULING

**0.3.0 PUBLISHED 20 Aug 2026** at `homebrew-tap/releases/tag/v0.3.0`, cask bumped by
`update-cask.yml` (`5c7bdca`, `github-actions[bot]`, 13:31:42Z).

`rexenv_0.3.0_universal.dmg`, sha256
`381952fa338a70c3b9fc09ad17106dd1276db9cad323fe0e150a371f50d14354`, 23,927,123 bytes.

**Four-way hash match, verified 25 Aug 2026** — the check that caught 0.1.0's
placeholder hash. The published asset downloads **anonymously** (no token, as `brew`
does: HTTP 200, 23,927,123 bytes) and hashes to exactly what the cask pins, what the
release API reports as the asset digest, and what the `.sha256` sidecar says. So what a
user installs is what is published, not something built beside it.

**Source: `bd0648c`**, the commit `v0.3.0` points at (20 Aug 12:19:42Z).
`.github/workflows/release.yml` triggers on `tags: ["v*"]` and builds the ref it was
triggered by, and the timing is tight and consistent: tag 12:19Z → the binary's own
embedded `builtAt` **2026-08-20T13:03:53Z** → asset uploaded 13:18:23Z → release
published 13:25:43Z → cask bumped 13:31:42Z.

> **Inference, not measurement, and the difference matters here.** The commit above comes
> from the workflow's trigger plus that timeline — **not** from the artefact. The dmg does
> not self-report it: `build.rs` stamps `REXENV_GIT_COMMIT` and did so at `bd0648c`, yet
> `bd0648c` and `5cb295e` are both absent from the shipped binary, and `rex version` is no
> help because it asks the RUNNING app over the socket and prints ITS commit — point the
> dmg's own `rex` at it and you get whatever is running locally. So a downloaded rexenv
> cannot be asked what built it. Tracked in `docs/TODO.md`; until it is fixed, every
> release row's commit rests on the tag rather than on the bytes.
>
> ✓ **Fixed for the NEXT release, 25 Aug 2026.** `rex --version` now prints the CLI's own
> `REX_GIT_COMMIT` before it asks anything, and labels the app's half as the app's:
> `rex 0.3.0 (a1b2c3d) · app rexenv 0.3.0 (e4f5f6a)`. So an artefact can be asked what
> built it with nothing running — mount the dmg, run its `rex --version`, read the commit
> off the bytes. `rex version` prints both stamps on their own labelled lines too. This
> row stays an inference because 0.3.0 predates the fix; the next one should not.

**§A0 ✅ RUN 31 Aug 2026 — on the SHIPPED bytes, eleven days after publishing.** Not the
local build tree the §A0 block is written against: the published dmg was downloaded
anonymously, hashed (`381952fa…`, matching the cask, the release digest and the sidecar),
mounted, and checked in place. All green:

- `rexenv` and `rex` both `x86_64 arm64`;
- `Dist_Archive_Command` **×5 in the arm64 slice and ×5 in the x86_64 slice**, thinned
  separately — the check exists because a half-populated fat binary is invisible to
  `lipo -archs`;
- `codesign --verify --deep --strict` → *valid on disk*, *satisfies its Designated
  Requirement*.

**§A's first half is answered too, and without launching anything**: `spctl -a -t exec`
— Gatekeeper's own assessment — **rejects** the app (ad-hoc signature, `TeamIdentifier=not
set`), which is the "quarantined → blocked" leg on the real shipped bytes.

**§A's launch half is CLOSED BY RULING, not by running it** (31 Aug 2026). Three reasons,
and the first alone is enough:

1. **The artefact is superseded.** The cask points at 0.4.0, which passed §A and SMOKE-TEST
   on its own bytes. Nobody reaches 0.3.0 now except by pinning an old URL deliberately.
2. **Running it here would mean launching a downgrade against a live machine.** The only
   Mac available carries 17 sites and a running stack; 0.3.0 would adopt those services and
   could rewrite the shared nginx config with its older generator.
3. **App data is schema v41 and 0.3.0 knows 38.** Its migration loop no-ops on a newer
   database rather than refusing, so it would run on a store whose shape it does not know —
   which is a way to learn something about downgrades, and not the thing §A is asking.

**What that costs, stated rather than buried:** whether these exact bytes LAUNCH on a clean
Mac is now permanently unknown, and the 6 downloads it had are unaccounted for. The reason
that is acceptable is (1) — the gate's purpose is to stop a broken artefact reaching users,
and this one has already been replaced by a gated one. **A superseded release's launch gate
is worth closing with a reason; it is not worth a clean-VM booking.** For the next release
the rule does not relax: §A runs BEFORE publishing, on the bytes being published.

> **The dmg answers a question it could not answer in August, and the answer is the bug.**
> `rex --version` off the mounted 0.3.0 volume prints `rex 0.3.0 · rexenv 0.4.0` — its own
> version with NO commit, then the RUNNING app's. That is exactly the defect the row below
> describes, now observed on the shipped artefact rather than inferred: a downloaded 0.3.0
> cannot say what built it, and pointing its `rex` at a live machine reports whatever is
> running there. Fixed 25 Aug, after this release; 0.4.0's row shows it working.

## A-prev) ✅ 0.2.0 — PUBLISHED (§A0 ✅, §A ✅ on the second run)



**0.2.0 PUBLISHED 16 Aug 2026** at `homebrew-tap/releases/tag/v0.2.0`, and the cask
bumped to it by `update-cask.yml` (commit `449b576`, `github-actions[bot]`).
**Verified independent of any local Homebrew, the check that caught the placeholder
hash in 0.1.0:** the published asset downloads **anonymously** (HTTP 200,
23,411,247 bytes) and hashes to exactly what the cask pins — and to exactly the
bytes §A0 and §A were run against. Three-way match, so what a user installs is what
was tested rather than something built beside it.

`rexenv_0.2.0_universal.dmg`, sha256
`bd019d8d5a9333908986575c1608c75f2d85a7246f7557e0d82566b95601e973`, 23,411,247
bytes, built from commit **`3755966`** with `npm run release:mac` on a clean tree.
(Commits after `3755966` are documentation only — including this paragraph — and
are not in the artefact. The tag belongs on `3755966`, and `v0.2.0` is there.)

_(Superseded before §A: `1cb01ead…`, built 15 Aug from `3a03610`. It shipped PHP
7.4 without the licence texts beside it — reproduced in `THIRD-PARTY-NOTICES.md`
only. That was rebuilt rather than shipped-then-fixed, because the licence
obligation is the one thing in this release that a follow-up release cannot
repair for the copies already distributed. Ledger #336.)_

**§A0 ✅** — run by hand on the rebuilt artefact, every leg: exactly one dmg;
`Contents/MacOS/rexenv` and `Contents/MacOS/rex` both `x86_64 arm64`;
`Dist_Archive_Command` ×5 in the arm64 slice and ×5 in the x86_64 slice
**separately**; `codesign --verify --deep --strict` clean; and the bundled
`rex --version` → `rex 0.2.0`, so the sidecar carries the release version rather
than a stale build. Release gate `verify-full: all green` ran first, at this
commit, on a clean tree.

**Staged as a DRAFT on the tap, 16 Aug 2026** — `rexenv/homebrew-tap`, title
`rexenv 0.2.0`, tag `v0.2.0`, `prerelease: false`, both assets attached (the dmg
and `rexenv_0.2.0_universal.dmg.sha256`, whose digest was re-checked against the
dmg with `shasum -c` before upload). A draft is invisible to `brew` and to the
tap's `update-cask.yml` poller, so nothing reaches a user until it is published.

**§A ✅ PASSED 16 Aug 2026, on the SECOND run.** Both halves observed: quarantined,
Gatekeeper **blocked** the launch; after `xattr -rd`, the app **launched**. That is
the pass — a run where the block was seen, on a setup where it could have been
missed.

**The FIRST run the same day was VOID and is kept here, because the difference is
the whole lesson.** On the developer's own login the app launched with no block, and
every measurement said the artefact was fine: `spctl --assess` → `rejected`,
`spctl --status` → assessments enabled, the synthetic flag `0083` identical to what a
real Chrome download carries on that machine (114 files in `~/Downloads`), and
`cp -R` from the mounted quarantined dmg propagating quarantine (`0283`). Faithful
setup, blockable artefact, no block — because that account had approved rexenv many
times and `/Applications/rexenv.app` already existed, so `cp -R` landed **over** a
bundle the account already trusted. **A first-launch test on a machine that already
knows the app cannot fail**, and a green that cannot fail is not a pass. What made
the second run valid is in §A-orig's boxed note: remove the existing bundle first,
and download through a browser rather than `curl`, which sets no quarantine at all.
**Both runs were on the developer's own login** — the account switch that note first
called for turned out not to be needed, which is worth knowing before the next
release budgets for one.

Publishing IS the §A sign-off; that rule does not relax for this release.

## A-prev) ✅ PASSED for 0.1.1 (and 0.1.0) — Apple-Silicon ad-hoc launch test

**0.1.1 (2026-08-13): §A0 ✅, §A ✅ — PUBLISHED.** `rexenv_0.1.1_universal.dmg` sha256
`14b64dae1ce633f31c46c5f033cdc2f5528c93ad14465f6db08db051bbd40241`, built from
commit `4ad5007`, released at `homebrew-tap/releases/tag/v0.1.1` and the cask bumped
to that hash by `update-cask.yml`. Verified independent of any local Homebrew: the
published asset downloads anonymously (200) and hashes to exactly what the cask pins.
§A0 passed by hand
(single dmg; both binaries `x86_64 arm64`; `Dist_Archive_Command` ×5 per slice;
`codesign --verify --deep --strict` clean) and the release gate `verify-full: all
green` ran first. Also checked on the *bundled* `rex`: `--version` against a deaf
listener returns in 2s where 0.1.0 hung — the fix is really in the artefact, not only
in `master`.

⚠️ **`brew upgrade` must be run from the account that owns Homebrew.** As of
2026-08-13 `/opt/homebrew` and the brew-installed `/Applications/rexenv.app` belong to
the **`rexenv-tester-1`** clean-account fixture, so from the developer's own login
`brew update` fails with *"/opt/homebrew is not writable"*, the tap clone stays at the
old commit, and `brew upgrade --cask rexenv` then reports "the latest version is
already installed" — about a cask it has not re-read. That is a stale local checkout,
NOT a failed release: hash the downloaded asset against the cask (as above) when the
two disagree, and run the upgrade path in the owning account.

**STATUS for 0.1.0: PASSED** on `rexenv_0.1.0_universal.dmg` sha256
`b29f21f7ef5c88e0d8c367c329e546a54708d5ed51623913cdb1f27377ab31ef` — built locally
2026-08-12 from commit `e3b6018`, and **this exact artefact is the one published** at
`github.com/rexenv/homebrew-tap/releases/tag/v0.1.0` (the app repo is private; see
`docs/RELEASING.md`). Quarantined dmg → Gatekeeper blocked the launch → `xattr -rd`
→ launched. **§A0 PASSED on it too**, run by hand from `release.yml`'s step: both
binaries `x86_64 arm64`; `Dist_Archive_Command` ×5 in *each* slice separately;
`codesign --verify --deep --strict` OK; exactly one dmg.

_(Superseded artefacts, all §A-passed in their day except the last: 2026-07-20
`d48bc8ba…`, 2026-07-21 `8d201724…`, 2026-07-22 `0e57f11c…`, and the never-§A-tested
2026-08-08 `aed8ad6a…` — whose hash sat in the cask as a placeholder and is the reason
`update-cask.yml` now compares the sha256 as well as the version.)_

Ad-hoc signing has not changed across any of these rebuilds, so the launch behaviour
should hold — but each rebuild carries work the previous pass never saw, which is why
§A is re-run rather than inherited. _(The 07-22 build added the deferred 21-fix pass:
the B25 timeout family, the B6/B13 cert pass, B7/B15/B28/B29, B12/B16/B26/B30 and the
B20 recorded-port allocator + migration. The 08-05 build adds MCP M2a/M2b and
`wp dist-archive`, the first feature to compile third-party code into the binary —
hence §A0.)_ On pass, the tap approach is re-validated for the shipping artefact.
(Canonical cask sha256 is still recomputed from the UPLOADED Release asset — see §D —
never from a local build.)

_(Prior passes: 2026-07-20 `d48bc8ba…`, 2026-07-21 `8d201724…`, 2026-07-22 `0e57f11c…`; then
the untested `f6252374…` — all superseded by `aed8ad6a…`.)_

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
cd <your rexenv checkout>
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

> ### ⚠ This script cannot produce a trustworthy result on a machine that already knows the app (16 Aug 2026)
>
> Run on the developer's own account during 0.2.0, it **did not block** — and the
> measurements say the setup was fine, which is what makes this worth writing down
> rather than retrying:
>
> - `spctl --assess --type execute` on the installed app → **`rejected`**, and
>   `spctl --status` → **assessments enabled**. Gatekeeper does refuse this
>   signature; there is nothing wrong with the artefact.
> - The synthetic flag `0083` is **exactly what a real Chrome download carries** on
>   that machine (114 files in `~/Downloads` have it). The value is not the problem.
> - `cp -R` from the mounted quarantined dmg **does** propagate quarantine
>   (`0283;…`), so step 2 works.
>
> So the artefact is blockable and the setup is faithful, yet no block appeared. The
> difference is the ACCOUNT: that login has approved rexenv many times, and
> `/Applications/rexenv.app` already existed — `cp -R` copies **over** it rather than
> replacing it. A first-launch test on an account that already knows the app is a
> test that cannot fail, which is the vacuous-green shape this repo keeps finding.
>
> **The fix, and it is cheaper than it first looked:**
> 1. **`rm -rf /Applications/rexenv.app` before copying** — required. Copying over an
>    existing bundle leaves a hybrid whose approval state is not the new build's.
> 2. **Download through a browser**, not `curl` — required, see below.
> 3. A clean account (`rexenv-tester-1`) or fresh user is the belt-and-braces
>    option, **but 0.2.0's passing run did NOT need it**: it was done on the
>    developer's own login with steps 1 and 2 only, and Gatekeeper blocked exactly
>    as it should. So the expensive half is optional and the cheap half is the one
>    that matters — recorded because the first draft of this note said the account
>    switch was required, which would have made every future §A cost an account
>    switch it does not need.
>
> **And delete the old advice, which was measurably wrong:** this note used to say
> that if the synthetic xattr doesn't trip Gatekeeper, "the definitive test is to
> `curl -LO` the dmg from any URL — a genuine download applies the quarantine for
> sure." **`curl` applies no quarantine at all** — measured 16 Aug 2026, no
> `com.apple.quarantine` on the fetched file. Quarantine is set by the *downloading
> application*, so the definitive path is a **browser** download (which is also the
> real tap-user path, §D). Following the old sentence would have produced a second
> no-op and read as a second pass.

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

**Why:** the real end-to-end a user experiences. The tap repo exists and is public —
`github.com/rexenv/homebrew-tap` (2026-08-08), `brew tap rexenv/tap` resolves and
`brew style --cask rexenv/tap/rexenv` is clean. Release + cask-bump are **automated**
(`docs/RELEASING.md`): publishing a Release pushes the version + sha256 bump to the tap
from the PUBLISHED asset.

**Both blockers are gone** (2026-08-12). The private-repo one: a private repo's release
asset 404s to `brew`'s unauthenticated fetch, so the dmg is released **on the tap itself**
while the source stays private, and the cask points there. And v0.1.0 **is published** —
the §A-passed `b29f21f7…` dmg, with the cask bumped to its hash by `update-cask.yml`.

**Verified so far:** the asset is fetchable with no auth (`curl -I` → 200), and
`brew fetch --cask rexenv` → **✔︎** (i.e. what users download matches the cask's pinned
sha256 — this is the check the old placeholder hash would have failed). Two things this
first real run taught, both now fixed in the tap:
- `update-cask.yml` compared only the **version**, so a placeholder sha256 under an
  unchanged version number skipped the bump and reported success. It compares both now.
- Current Homebrew refuses a third-party tap until it is trusted — **`brew trust rexenv/tap`**
  (or `brew trust --cask rexenv/tap/rexenv`) is a real user-facing install step.

**Install half: ✅ RAN 2026-08-12** on the dev Mac, after deleting §A's hand-copied
`/Applications/rexenv.app` (leave it and `brew install` collides). Results:
`brew install --cask rexenv` → moved the app, linked `rex` to `/opt/homebrew/bin/rex`;
`xattr -p com.apple.quarantine` → **No such xattr** (the postflight really did it);
`lipo -archs` on the *installed* binary → `x86_64 arm64`; `codesign --verify --deep
--strict` → clean; `open -a rexenv` → launched; `rex --version` → `rex 0.1.0 · rexenv
0.1.0`; `rex status --json` → full service snapshot. The DNS LaunchAgent was rewritten
to the `/Applications` path (checked: it does NOT keep an AppTranslocation path).

**`--zap` was NOT run and must not be, on any machine with real sites.** It trashes
`~/Library/Application Support/dev.rexenv.rexenv` — on the dev Mac that is **17 GB** of
live app data behind the developer's own `.rex` sites. The zap step belongs to a clean
Mac (`docs/SMOKE-TEST.md`), not to a machine that does daily work.

**Found while running it: `rex` hangs FOREVER against a half-alive app.** §A's launch
left an App-Translocated instance owning `config/rexenv-cli.sock`; it accepted
connections and never answered, so `rex --version` and `rex status` blocked in
`recvfrom` with no output and no timeout — until that pid was killed, at which point the
already-typed command completed. `request()` documents having no read timeout on purpose
(site create legitimately runs for minutes), but `soft_request()` inherits it while its
own doc comment says `--version` "must work WITHOUT the app". **Fixed the same day**
(ledger #300) — and **deliberately not re-cut into v0.1.0**: the published dmg carries
the hang, the fix ships in 0.1.1. Re-running §A0 + §A costs more than a wedge this rare,
and the cask can now rehash a same-version re-upload if that judgement ever changes.

```sh
# One-time online cask audit:
brew audit --cask --new --online rexenv/tap/rexenv   # after the Release exists

# The user path:
brew tap rexenv/tap
brew trust rexenv/tap                              # current brew refuses an untrusted tap
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

**Why this is here:** a machine that has never run Valet/Herd resolver setup has no foreign
`/etc/resolver/test`, so the foreign-file paths of the Valet/Herd resolver takeover have never
run against a real root-owned foreign file. Creating one on the dev machine to test was
deliberately refused. The logic is unit-tested against fixture directories
(`core::dns::owner_of`) and the ABSENT-file path is live-verified; these two paths are
fixture-only. Plan: `docs/archive/PLAN-valet-herd-import.md` §4.

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
6. **Hand it back** from **Settings → DNS & SSL** (the borrowed-resolver "Hand back" row) —
   NOT the import screen: after takeover the TLD reads "borrowed", which the `/import`
   consent panel filters out, so no card renders there (`Import.tsx` `blocked` filter). The
   row also shows for a *drifted* (reclaimed) TLD. Verify: `/etc/resolver/test` is
   byte-identical to step 1's `shasum`; the backup file is GONE; the record is GONE; the
   confirm warned that rexenv `.test` sites stop resolving.
7. Take it over again, then this time run **Settings → Remove system changes**. Verify the
   same restore happened as part of the single privileged prompt.
8. **Drift:** take it over, then run `valet install` (or relaunch Herd) so they reclaim the
   file. Restart rexenv. This step CHANGED on 15 Aug 2026 — it used to end "there is **no
   on-launch UI toast/banner**", which was true when written and is now the opposite of
   what must happen. A reviewer holding the old sentence would read the banner as a bug.
   Expect ALL FOUR surfaces, and check them in this order:
   - a) the **Sites banner** (ledger #334) — headline naming the SYMPTOM ("Your `.test`
     sites stopped resolving"), body naming Valet or Herd, and the way back ("You can
     take it back from Settings or Import"). Not a toast: it stays.
   - b) **Dismiss it, then reload the app.** It stays dismissed. Dismissal is **per-TLD**:
     if a second TLD is also drifted, dismissing one must not hide the other.
   - c) **Take that TLD back** (step 6's hand-back, then re-take it) so it reads as ours
     again, then lose it again → the banner **RE-SHOWS** despite the earlier dismissal.
     The dismissal set self-heals against the live answer; a dismissal that outlived the
     condition would silence the next real loss.
   - d) `rex doctor` → a **`Resolvers`** line that NAMES the TLDs, counts toward findings,
     and makes the **exit code non-zero** (a reclaimed TLD is a failure, not a warning —
     the sites are dark while DNS still reads ✓ on :15353). Also still the startup
     `log::warn`, `/import`, and Settings → DNS & SSL.
   **Tell:** `rex doctor` printing `Resolvers ✓` or omitting the line. Against an app whose
   payload predates the field it must read **⚠ unknown**, never ✓ — an absent field
   collapsing into "empty" is a question nobody asked reporting a confident pass.

   Then remove system changes and verify we left THEIR file alone and dropped our record +
   backup.

8b. **The zero-render control, and it belongs to every machine — run it even if you skip
   the rest of §F.** With NO drifted TLD (the ordinary state on any machine), the Sites
   route must render **no banner at all** and `rex doctor` must read `Resolvers ✓` with a
   zero exit. Nothing-taken-back is normal and must never grow a notice (#306's rule).
   **Tell:** a banner, an empty banner frame, or a doctor finding on a healthy machine —
   inventing a problem out of normality is the import bug in a new place.
9. **Backup-missing path:** take it over, delete `<app-data>/resolver-backups/test` by
   hand, then remove system changes. Expect our file removed and an honest message saying
   the backup was gone and to run `valet install`.
10. Throughout: `/etc/resolver/rex` must be untouched, and no file we did not create may
    ever be removed.

**Rollback — if the takeover goes wrong or you stop halfway (keep this beside the run).**
The takeover is **record-before-write** (`core/dns.rs` `take_over_resolver`): it writes the
`0600` backup and the `resolver_takeovers` row FIRST, then runs the single privileged
`osascript` that overwrites `/etc/resolver/<tld>`; a failure rolls back BOTH. So:
- **Cancelled admin prompt → nothing changed.** No cleanup. Verify: `cat /etc/resolver/<tld>`
  still shows their content and the DB has no row.
- **Crash / force-quit after the overwrite → a record + backup ALWAYS exist**, so hand-back /
  Remove system changes restores it normally. The "overwrote-but-untracked" state cannot occur
  by construction — there is no silent-alteration-without-a-recovery-path case.
- **Manual restore of ANY borrowed TLD:**
  ```sh
  sudo cp ~/Library/Application\ Support/dev.rexenv.rexenv/resolver-backups/<tld> /etc/resolver/<tld>
  sudo chmod 644 /etc/resolver/<tld>
  sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
  ```
  (the same original bytes also live in `resolver_takeovers.original` in `…/rexenv.db`.)
- **A TLD rexenv PLAIN-CREATED** (absent → create, no record — e.g. `.rex`, or any TLD added on
  a clean Mac; ours iff its content is exactly `nameserver 127.0.0.1` + `port 15353`, list with
  `grep -l 'port 15353' /etc/resolver/*`): `sudo rm -f /etc/resolver/<tld>` + the flush above.
  The per-site "Hand back" button REFUSES these (no record) — use Settings → Remove system
  changes, or the manual `rm`.
- **Backup truly gone** (both the `0600` file AND the row): rexenv can only remove ITS version
  and tell you to run `valet install`; it cannot restore the original owner's file — reinstall it
  via `valet install` / a Herd relaunch.
- **Metadata:** restore normalizes the file to mode `644` (content byte-identical, permissions
  not) — `chmod` by hand only if the original had a non-`644` mode.

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
is reproducible against a dev Mac with a real Valet + Herd install unless marked
CLEAN-VM.

**What a full import would CHANGE on the test machine — read before clicking:**
importing a `.test` site installs `/etc/resolver/test` (one admin prompt). If the
file is absent that is a plain create, NOT a takeover — rexenv owns it outright
and "Remove system changes" deletes it. It also creates real site rows pointing
at real project folders and issues certs. Deleting those sites later leaves the
folders untouched (Stage 0 guarantee). **Import ONE site, not the whole list.**

Before starting, run `cargo run --example valet_scan_check` (or a dry open of
`/import`) and note YOUR machine's numbers: total rows, ready count, dangling
symlinks, leftover confs, dedupe count, per-site PHP pins. The steps assert
against those recorded numbers — the point is that the screen reconciles
exactly with the scan, not that any machine matches the original pass.

1. **Populated list.** Sites → the "N sites found in Valet or Herd" banner
   (count = your ready count). Open it. Expect total rows + ready count matching
   your scan, and a source card per source (Herd/Valet), including any
   "parked folder … has no sites in it" note.
2. **Dangling symlinks** — every dangling link from your scan must be A ROW
   reading "can't import" AND naming its missing target path. Checkbox disabled.
   (A dangling link is a site the user thinks they have — never silently skipped.)
3. **Leftover configs** — every conf-with-no-site from your scan: "leftover
   config with no site folder". A conf on a TLD the config never mentions
   (the `<name>.dev`-style case) must still appear.
4. **Dedupe** — rows for domains present in both sources carry "also in Valet"
   (Herd's copy wins). No domain appears twice.
5. **PHP pins** — rows with isolation markers show their pinned minor (both
   marker formats — `php@8.4`-style and bare-digit); rows with no marker show
   the default. **Expected, not a bug:** a pin rexenv doesn't ship flips the row
   AMBER "needs attention — PHP x.y isn't one rexenv ships, choose a version to
   import it on", still shows their pin, and disables its checkbox until you pick one.
   **Needs a fixture project isolated to a minor rexenv genuinely does not ship** —
   this said "a legacy 7.4-isolated project" on the belief 7.4 was unshippable, which
   `docs/archive/PLAN-php-74-support.md` retired. Check the shipped set first and pick below
   it (7.2 today), or this gate silently stops testing the amber branch it names.
6. **Docroot resolution** — a Laravel/Bedrock row must show `(serving public/)`
   or `(serving web/)`, not the project root. **Two amber "needs attention" cases are
   also expected here, not bugs:** a project with a `LocalValetDriver.php` in its root
   shows its detected docroot but flips amber (rexenv won't execute their driver to
   confirm the root); and a served folder that overlaps an existing rexenv site, is too
   broad, or sits inside rexenv's app data reads "needs attention".
7. **Selection** — select-all ticks only the ready ones; the indeterminate
   state shows on a partial selection; disabled rows can't be ticked; the count
   in the bar matches; Rescan preserves nothing stale.
   - **Rescan feedback** (8 Aug): the icon must visibly spin (a 550ms floor sits on
     the SPINNER, never on the scan), the button reads "Rescanning…", and the
     subtitle carries the list's real age — `scanned just now` → `12s ago`. A rescan
     that finds nothing new must still be visibly a rescan.
8. **Import one site.** Pick a small static/PHP one. **"also copy databases" is TICKED
   by default** (8 Aug: rexenv is the whole stack, so a migration is a database migration
   too — the wire default stays off, only the screen opts in). **Untick it for the first
   pass**; the DB flow is the second pass below and §I in full. Expect the admin prompt ONCE, up
   front, before any site is created (the DB copy, when enabled, adds no admin prompt —
   it's a loopback SQL read/restore). Watch the row flip to `imported`. Then: it appears
   in Sites with the **external** badge, `https://<domain>` loads THEIR files, and
   deleting it leaves the folder on disk.
   - **With "also copy databases" ON** (do this as a second import): each site also gets
     its DB copied into rexenv's engine — a READ, their original database is never touched.
     The site keeps using the OLD database until you switch it over (its Database tab shows
     the exact change), the row carries a **DB pill**, and the summary reports "N databases
     copied / N failed". This is the Stage-2 flow §I exercises in full.
9. **Progress while it runs** (8 Aug, `valet-import://progress`). A card above the
   list, with the batch bar. Assert it is REPORTING, not animating:
   - the step line matches the phase the site's own provision job is on (open its log
     via the row's outcome to cross-check), and switches to the database job's own
     phases when databases are on;
   - `n of N done` only ever counts rows that reached a terminal outcome — a failed row
     still counts;
   - the bar never goes backwards, never reaches 100 while a site is still running, and
     FREEZES where it stopped if the run ends early;
   - unsettled picked rows read `importing…` (the in-flight one) or `waiting`, never the
     pre-run `ready`;
   - **a multi-GB database is the case this exists for** — during a long dump the bar
     must keep moving on the database job's phases, not sit still.
10. **Continue-on-failure** — hard to force naturally; if you want it, rename a
    project folder between the scan and the import so one row fails, and confirm
    the rest still import and the summary names the failure.
11. **Cancel** — the button sits beside the progress bar (moved off the header bar
    with the card). With several selected, cancel mid-run: the current site
    finishes, the rest report `skipped`, and no half-created site appears.
12. **Sidebar → Import** opens `/import` from anywhere. **Settings → DNS & SSL** shows
    nothing about importing any more (no "N sites can be imported" row, no leftover
    dumps) — only the borrowed-resolver hand-back rows, which are DNS. Owner, 12 Sep
    2026: import was a card inside DNS & SSL, and it is not part of DNS & SSL.
13. **Banner dismissal** — Dismiss on Sites, reload the app, it stays dismissed.

**CLEAN-VM only** (cannot be exercised here):
- The **empty state** (no Valet or Herd at all) — the "No Valet or Herd sites
  found" card. Most first-run users see this, so it matters as much as the
  populated list. Do NOT fake it by moving a real machine's Valet/Herd folders.
- The **resolver consent panel** — needs a foreign `/etc/resolver/test`; both
  TLDs read "absent" here, so the panel never renders. Covered by §F.
- **Hand-back row** in Settings — only appears for a BORROWED TLD, so it needs
  §F's takeover first.

## E) Other live/GUI items from the review & publish

- **🟢 Clean-Mac release QA** — `docs/SMOKE-TEST.md` on a fresh Mac / user account. The
  broadest confidence check; especially worth it before sharing with your QA friend.
- **🟢 `examples/*` live re-verification of the fixes whose full behavior needs real
  binaries** (the "live-check" scope notes in `docs/archive/CODEBASE-REVIEW.md`). All are
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
(step 7). their MySQL engine (DBngin in the original pass) handshake also confirmed the last open pre-auth
identification case (plan §2.1).

The sandbox proves the machinery (`db_dump_check`, `db_restore_check`); this pass proves
it against a REAL source and the packaged UI. **The user's side of the bargain (D6): rexenv
never starts or stops their database server, so step 2 is yours.**

**Preconditions — in this order:**

1. Rebuild + reinstall; confirm the commit via Settings → About or `rex version`.
2. **Start their MySQL engine (DBngin in the original pass) yourself** (DBngin.app → start the engine). rexenv will
   refuse honestly if 3306 is silent — that refusal is itself checkable (step 4).
3. Have `<site>.test` imported as a site (Stage 1) or import it now.

**The pass:**

4. WITH DBNGIN STILL STOPPED first: SiteDetail → <site>.test → Database → **Import database**.
   Expect the honest per-site refusal naming host, port and what to do ("start it in
   DBngin, then re-scan") — NOT a timeout, NOT a generic error. The plist claims
   `started`; the UI must not believe it.
5. Start DBngin's MySQL. Import again. Expect phases check → copy → start → restore →
   finish, progress monotonic, ≤99 until the settle.
6. **The interim state (§9 — the state users actually live in).** After success:
   - the summary reads "Imported — not yet connected", names the database, the table count, size
     and source, and says the site STILL reads the old database and the two DRIFT;
   - the Sites page shows the "DB imported · not connected" badge on <site>.test — same
     wording as the summary implies, because both render one `DbImportRecord`;
   - `DB_USER` in the site's wp-config is root, so the copy must state the 3-key change
     (host + user + empty password), not just DB_HOST.
7. Verify the copy is real: SiteDetail → Database (Adminer) → the copied database exists
   on rexenv's MySQL with the expected tables. The SITE meanwhile still serves from the
   old database (edit a post title via the site, confirm it does NOT appear in rexenv's
   copy — that's the drift, demonstrated).
8. **Their side untouched:** their engine still runs, the source database on 3306 intact (`shasum` of their
   datadir is overkill — check table counts via any client, or just that the site still
   works when pointed at it).
9. Cancel path: start an import, cancel during the copy. Expect "cancelled", no artifact
   left (the Import page shows no leftover), site row unchanged, DBngin untouched.
10. Failure keep: stop DBngin's MySQL MID-copy (this is the one legitimate way to kill a
    dump). Expect a frozen failed state naming the kept dump file; the Import page's
    "Leftover database dumps" lists it with a working Delete (it lived in Settings →
    DNS & SSL until 12 Sep 2026).
11. Batch: `/import` → tick "also copy databases" → import a small WP site. Expect the
    per-row "DB copied" chip, and a site with no database config to read "DB skipped"
    with the reason — never a failure.
12. Concurrency guard: while a database import runs, Retry/provision for the same site
    must refuse with "a database import is running for …", and vice versa.

---

## J) ✅ Connection rewrite (Stage 3) — packaged GUI pass on <site>.test

**PASSED 28 Jul 2026 — all 12 steps, human-verified on the packaged app
(`44b6a6d`), no deviations reported. Stage 3 SHIPPED.**

The sandbox proves the machinery end to end (`config_rewrite_check`, 11 live
assertions, passed 28 Jul 2026); this pass proved it against the REAL site and
the packaged UI. Steps kept for re-runs. Preconditions: `<site>.test` imported (Stage 1) with its database
imported (Stage 2, state "imported · not connected"), rexenv's MySQL running,
and `shasum the site's wp-config.php` noted BEFORE anything below.

1. **The consent card.** SiteDetail → <site>.test → Database. Expect the diff card:
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
10. **D2 delete confirm.** With a connected site (use a SCRATCH site, not the testbed):
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
- **Targets, from your real scan:** primary + secondary = two WordPress sites
  from the scan rows (their db names per their own wp-config) — pick ones whose
  content you'd shrug at losing, but NOT the §I/§J testbed site (§K uses it
  read-only as the "already imported" exhibit) and not a site whose source
  database you treasure (nothing here writes to their DBs, but you'll be
  signing into the sites and pressing delete buttons near them).
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
PROJ=/path/to/primary        PROJ2=/path/to/secondary     # from the scan rows
DB=<primary-db>               DB2=<secondary-db>
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
   `<site>.test` shows **already imported** (a prior stage's state, visible and
   disabled — the first seam). Counts reconcile with §G's totals minus any
   sites you've since imported/deleted.
2. **Import the primary** (site only — no DB checkbox). Expect: one admin
   prompt at most (resolver already ours), row flips `imported`, Sites shows
   the row with **external** badge, NO DB badge, Running.
3. **It serves THEIR site.** `https://<primary>.test` shows the real
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
    `rex_<primary-slug>`), no password anywhere; backup note carries the
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
    The edit appears in rexenv's Adminer copy; their their old database
    must NOT change (the final checksums assert it — don't check by writing
    anything their side).
13. **Interleave under load:** start the secondary's DB import; while its
    copy phase runs, try a THIRD import (any site) → **"a database import is
    already running (for <secondary>.test) — one at a time"**; and Apply on
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
    → `rex_<primary-slug>` on localhost + 127.0.0.1 only.
16. **Delete the primary** (state: imported, reverted). Ordinary confirm
    ("… its database, and its certificate"); delete. Then verify: row gone;
    `$PROJ` folder intact; rexenv's copy dropped (Adminer); the K15 query
    now returns NO `rex_<primary-slug>` (dropped BY THE RECORD — D3's
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

## L) 🟢 Phase-A never-resolves spot-check (ledger #19) — wire-level, one-off

**Why:** `phase_a_never_plans_a_system_dns_query` pins the DECISION layer (Phase A
builds no system-DNS plan), but "reqwest's `.resolve()` pre-pin never falls back to
getaddrinfo" is a claim about a dependency's internals. One wire-level observation
settles it for the pinned reqwest version; re-run only after a reqwest bump.

```sh
# Terminal 1 — watch for any OUTBOUND port-53 query for the tunnel hostname
# (loopback excluded: our own resolver on 15353 is not port 53):
sudo tcpdump -i any -l -n 'udp port 53' | grep -i trycloudflare

# Terminal 2 — start a share, watch the card through Unverified.
```

**Expected:** ZERO trycloudflare lines during Phase A (the probe phase before the
gate opens). Lines appearing only AFTER the badge settles are Phase B/browser
traffic and fine. Any hit during Phase A = the `.resolve()` pin leaked — file it.
Result: ____ (date, reqwest version).

---

## N) Local import — live pass against a STARTED Local site (owner-run) — single site ✅, network steps N10–N14 pending

**PASSED 12 Sep 2026**, owner-run on `ab12.local` (Local 10.1.2, MySQL 8.4.0) at
`805ee0d`. Its first attempt is why §2b of the plan changed: the TCP probe met ERR 1130
before Local's handshake and the job refused the server as unidentifiable; after the
socket-probe fix the copy came over (12 tables), the URL pass moved `ab12.local` →
`https://ab12.rex` (13 replacements), and the owner reported the rest of the pass fine.

What neither layer below this can give (`docs/archive/PLAN-local-import.md` §7): Local's
REAL per-site mysqld (`skip-name-resolve`, the socket where Local's docs place it),
a real Local WordPress database, and the packaged `/import` screen with Local rows.
The sandbox examples prove the socket sign-in and the URL pass against a mysqld and
a WordPress rexenv started itself; this pass is the Local half, and it needs a site
started IN Local — a button rexenv never presses.

### Safety — read before N0

- **Nothing of Local's is written.** The scan reads `sites.json`; the database copy
  is a non-locking read of the site's mysqld while Local runs it; the URL pass
  writes rexenv's COPY only.
- **Writes into the project only at N7**, and only if you press Connect (backed up,
  one-click revert — the §J machinery).
- **Persists on our side until N9:** the site row, the copy, a `rex_<slug>` user if
  you connect.

### Preconditions

1. Build at the commit under test; start the rexenv stack.
2. **Keep Local RUNNING** — unlike Herd in §K, Local must stay open: the site's
   database lives inside it. Check Services shows rexenv's edge green: Local's router
   can hold `:80`/`:443` in "Site Domains" mode; if it does, switch Local's router to
   "localhost" mode for this pass and record that you did.
3. Pick a SINGLE-site Local site whose content you'd shrug at losing.

### Steps

- **N0** — Import → a **Local** source card naming Local's app-data folder, with the
  note that `.local` sites import on `.rex` and each copied database has its URLs
  updated (rexenv's copy only). Multisite rows import as networks (N10 — since 12 Sep
  2026); only a network with a subsite on its own domain reads "can't import".
  **Expect** every Local site you have is a row — count them against Local's sidebar.
- **N1** — the chosen row shows `<name>.rex` and **"was <name>.local"**. If its PHP
  isn't shipped, the row has a picker: it stays unticked until you choose; choose →
  "ready".
- **N2** — with the site STOPPED in Local, import it with "also copy databases".
  **Expect** the site imports and the DB fails with *"… Local runs a site's database
  only while that site is started. Start "<name>" in Local, then retry"* — not
  DBngin's wording.
- **N3** — **start the site in Local.** Record
  `ls ~/Library/Application\ Support/Local/run/<id>/mysql/` (is `mysqld.sock` there?).
- **N4** — SiteDetail → Database → Import database. **Expect** ok; the job log says
  *"is a Local site: its database is Local's own server for it (127.0.0.1:<port>,
  signed in over its socket)"* and *"URLs: <name>.local → https://<name>.rex in
  rexenv's copy"*. The source label reads *Local's "<name>" site — MySQL <v> at …*.
- **N5** — rexenv's Adminer: the copy's `siteurl`/`home` are `https://<name>.rex`.
  Local's own Adminer (Local → Database): still `http://<name>.local`.
- **N6** — Rescan → the row reads **already here** (folder match, not name).
- **N7** — Database tab → connect. First Local import (db `local`): the 2-key diff
  (host, user → `rex_<slug>`), no password line. Open `https://<name>.rex` — it loads
  and does NOT redirect to `.local`. *(A second Local import restores as
  `local_<domain>` and gets the tell-only block — plan §9 Q1, expected today.)*
- **N8** — Revert the connection; open the site in Local — it still works there.
- **N9** — delete the rexenv site (revert-then-delete default). `~/Local Sites/<name>`
  is untouched; Local's site still starts and serves.

### Network steps — N10–N14 (added 12 Sep 2026, `docs/PLAN-local-multisite.md` T5)

**Pending.** The single-site pass above predates network import. What the layers below
already prove (`local_import_check` leg C): a real subdomain network's copy moves every
blog to `https://` on the new name, and the connect plan boots a subsite with the login
cookie on the new domain. What only this pass gives: Local's real network database, the
Import row, the batch adopt + reload actually SERVING `*.multi.rex`, and a browser login.
Use a Local network you'd shrug at losing (the owner's `multi.local`: subdomains, `ea1` +
`ea2`). Writes into the project only at N11/N12 if you connect — backed up, one-click revert.

- **N10** — Import: the network's row reads `multi.rex`, "was multi.local", and
  **"multisite network · subdomains · ea1.multi.rex, ea2.multi.rex"**, and is ready (not
  "can't import"). **Expect** the row imports as ONE site.
- **N11** — **start the network in Local**, then import its row with "also copy databases"
  and "also connect Local sites". **Expect** imported + DB copied + connected; the job log's
  URLs line; the site's WordPress tab shows it as a network with NO "Convert to multisite"
  card; rexenv's Adminer: `wp_blogs.domain` = `multi.rex`, `ea1.multi.rex`, `ea2.multi.rex`.
- **N12** — Database tab → the connection's diff (or its record after the batch):
  `DOMAIN_CURRENT_SITE` → `multi.rex`, no password line; the preview's sentence that Local
  can't load the network under its old name while connected.
- **N13** — open `https://multi.rex` and `https://ea1.multi.rex`: both load, neither
  redirects to `.local`. Log in at `https://multi.rex/wp-admin` → Network Admin opens, and
  a subsite's dashboard opens without logging in again.
- **N14** — Revert the connection: wp-config's `DOMAIN_CURRENT_SITE` reads `multi.local`
  again; open the network in Local — it loads. Delete the rexenv site (revert-then-delete);
  `~/Local Sites/multi` is untouched.

## Publish-blocking summary

> **This table is the thing a release-day reader clears, so it has to be the WHOLE set.**
> It was not: §F and §G are both marked 🚧 publish-blocking in the body and neither had a
> row here, so a release could be cleared from this table without either ever being seen
> — the guard-covers-claimed-surface shape this repo keeps paying for, in the file that
> gates shipping. Added 21 Aug 2026, along with §L.

| # | Check | Status |
|---|---|---|
| A0 | Artefact integrity, per slice — **0.4.0 `a7aecee7…`** | ✅ passed 27 Aug 2026 (by hand; CI does not run while the repo is private). **0.3.0 `381952fa…` ✅ too, run 31 Aug on the SHIPPED bytes** — downloaded, mounted, checked in place |
| A | Apple-Silicon ad-hoc launch (de-quarantine → launches) — **on the 0.4.0 dmg `a7aecee7…`** | ✅ **passed 27 Aug 2026**, with `docs/SMOKE-TEST.md` (row S) on the same bytes; publishing was the sign-off. **0.4.0 published 16:37Z; cask bumped 17:43Z and the anonymous download four-way-matched 30 Aug.** 0.3.0's launch half is CLOSED BY RULING (superseded artefact; §A-prev has the reasoning and what it costs), with Gatekeeper's own `spctl` rejection recorded on its shipped bytes |
| S | `docs/SMOKE-TEST.md` end-to-end on a clean Mac from the built dmg — **the OTHER half of the gate, and it had no row here until 30 Aug 2026** | ✅ passed 27 Aug 2026 on 0.4.0 `a7aecee7…`. Re-run per release: it is the only place the packaged app proves its own flows |
| A0-b | The UPDATE archive + §A0 on the bundle EXTRACTED from it | ✅ scripted (`scripts/release-assets.sh`, run by `pnpm release:mac` and re-checked in CI). **Green on the existing 0.5.0 build 6 Sep 2026** — the layout guard is plant-proven (a `./`-prefixed archive is refused) |
| M | **In-app self-update on a real Mac** (0.6.0 → 0.6.1) | ◐ **RAN 7 Sep 2026 on 0.6.0 → 0.6.1** — the apply, the swap, the relaunch, the changed cdhash, no quarantine, `rex --version` agreeing with About, the DNS agent re-execed from the new bundle (pid changed), leftovers swept, services outlived it, and the descriptor chain end to end. Details in §A 0.6.1. **Still owed, and honestly so:** whether any Gatekeeper/App-Management dialog appeared, which permission prompts returned, the startup notice, the tray item, the offline check and the public-share path — the update was run, those legs were not observed. It also produced the release’s one real finding: `auto_updates` does not stop `brew upgrade --cask rexenv` |
| B | Uninstall removes the root :443 daemon | 🚧 do when convenient (tears down your edge) |
| C | B31 CSP packaged smoke test | ✅ done |
| D | Full tap install dry-run (after Release + tap push) | 🚧 **`--zap` ONLY** — the install half passed 12 Aug 2026 and the cask has bumped cleanly through 0.1.1 / 0.2.0 / 0.3.0 / 0.4.0 since. The trigger used to read "do once the dmg is released", an event that happened four releases ago |
| E | Clean-Mac QA + example live-checks + deferred-pass wiring (B28/B29/B7/B20) + (deferred) signing | 🟢 nice-to-have |
| I | Database import: live DBngin source + packaged GUI pass (user starts DBngin) | ✅ passed 27 Jul 2026 |
| J | Connection rewrite (Stage 3): packaged GUI pass on <site>.test | ✅ passed 28 Jul 2026 |
| K | The whole migration as ONE journey (seams + reversibility) | 🚧 rebuild, then run |
| F | Resolver TAKEOVER + RESTORE — clean-VM only (fixture-tested, never live-run) | 🚧 **publish-blocking, and it was missing from this table until 21 Aug 2026** |
| G | `/import` screen — packaged-app GUI pass | 🚧 **publish-blocking, and it was missing from this table until 21 Aug 2026** |
| N | Local import: live pass on a STARTED Local site + the packaged screen's Local rows | ✅ passed 12 Sep 2026 (owner-run, `ab12.local`) |
| N10–N14 | Local import of a multisite NETWORK: the row, adopt + reload serving `*.<name>.rex`, the connect moving `DOMAIN_CURRENT_SITE`, a browser login across subsites, revert | ⏳ pending (owner-run; L1 leg C green 12 Sep 2026) |
| L | Offline/timeout behaviour of the update check | 🟢 nice-to-have (no row here before 21 Aug 2026) |
