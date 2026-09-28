# PLAN — every release from GitHub Actions, every OS at once (27 Sep 2026)

**Owner ruling, 27 Sep 2026:** "proti release e mac, windows ebong linux er jonno eksathe sob
release hobe, ebong release build korar jonno amader device er upore nirvor na kore sorasori
github action use korte, jate kono device dependency na thake — always github theke accurate
vabe release hoi, karon ek ek device e ek ek rokom hote pare."

## 1. Why (what a device-built release cost)

- The Linux port's first x86_64 host (the Dell's WSL Ubuntu 26.04) produced a deb that installs on
  nothing older than 26.04 — a build carries its host's glibc. The same day, three distro-shaped
  bugs (`policykit-1` gone on 26.04, `libaio.so.1t64`, `libxml2.so.16`) showed how much a host
  decides. A hosted runner is the same machine every time, and its floor is a label the
  workflow states and asserts (`objdump -T` for `GLIBC_`, ledger-shaped).
- Until now the dmg was built on the owner's Mac and uploaded to the tap by hand ("Today's
  flow" in RELEASING.md, five releases), Windows on the Dell, Linux on a VM — three machines,
  three toolchains, one person's afternoon per release. Nothing checked that all three OSes
  shipped together; nothing could, because nothing built them together.

## 2. What landed (`.github/workflows/release.yml`, rewritten)

One tag (or Actions → Release → version) → five jobs:

| job | runner | produces |
|---|---|---|
| `versions` | ubuntu-latest | the four-manifest guard; creates the tag on the dispatch path |
| `macos` | macos-14 | `rexenv_<V>_universal.dmg` + `.app.tar.gz` (verify.sh, `pnpm release:mac`, §A0 per slice, `release-assets.sh --check`) |
| `windows` | windows-latest | `rexenv_<V>_x64-setup.exe` + `_x64.zip` (verify.sh, `pnpm release:win` → §A0-windows) |
| `linux` ×2 | ubuntu-22.04, ubuntu-22.04-arm | `rexenv_<V>_{amd64,arm64}.deb` + `_{amd64,aarch64}.AppImage` (verify.sh, `pnpm release:linux` → §A0-linux, glibc ≤ 2.35 asserted) |
| `publish` | ubuntu-latest | waits for ALL four; checks the eight assets + eight `.sha256`; drafts ONE release on `rexenv/homebrew-tap` with everything attached |

Each lane uploads a run artifact; `publish` refuses to draft unless every asset is there and
matches its digest — **a release is all three OSes or nothing**. The draft is still a draft:
§A (Gatekeeper, a human at a Mac) and the click to publish are the human gate, as before.
`linux-build.yml` stays as the dispatch-only Linux x86_64 build for inspection without a tag.

## 3. What the owner must do once

- **`TAP_TOKEN`** — Settings → Secrets → Actions on `rexenv/rexenv`: a fine-grained PAT,
  repository access `rexenv/homebrew-tap` only, permission `Contents: read and write`, nothing
  else. RELEASING.md used to argue against any cross-repo credential; the ruling accepts this
  one because it is scoped to one public repo's releases and because the alternative (a
  person's laptop as the release machine) is the thing being retired. When the source repo
  goes public the draft moves back here and the token goes away.
- **The arm64 Linux runner.** `ubuntu-22.04-arm` is free on public repositories and a paid
  "larger runner" on private ones. The first tag run will show whether the lane is picked up;
  if not, the choices are to enable/pay for arm runners in the org's Actions settings, or to
  make `rexenv/rexenv` public. Shipping without aarch64 is not one of them (the ruling).
- **macOS minutes** on a private repo are billed at 10×; a universal build + verify.sh is
  ~40 minutes of macos-14 → ~400 billed minutes per release. The free tier is 2,000/month.

## 4. The release, step by step (RELEASING.md carries the same list)

1. Bump the four manifests, commit (`scripts/check-versions.sh` locally too).
2. `git tag -a vX.Y.Z -m … && git push origin vX.Y.Z` (or Actions → Release).
3. Wait for the draft on `rexenv/homebrew-tap`. Download the dmg, run §A, **Publish**.
   Publishing fires the tap's `update-cask.yml`.
4. `rexenv/runtimes` → Actions → "Publish app update manifest", dry-run then publish, **six
   times**: macOS, Windows, Linux deb x86_64, deb aarch64, AppImage x86_64, AppImage aarch64.
5. `./scripts/check-app-manifest.sh` (+ `--windows`, + `--linux <kind> <arch>` ×4).

## 5. Not changed

The human gate (§A + the Publish click), the draft-first rule, the immutable-bytes rule
(never re-issue a version), the six descriptors' serials and key, the patch-from-a-tag
recipe. The tap stays the public home of the artefacts while the source is private.

## 6. Owed

- The first tag run, **v0.8.8, run 36296169737 (27 Sep 2026): all four lanes red in
  verify.sh, no draft** — and the round of runner-shaped fixes it predicted: (1) the step's
  `bash -e` swallowed the log (`set +e` now, and `verify-ci.yml` runs the bar on any release
  runner without a tag); (2) `notices-check.py` needs a `cargo fetch` per target on a cold
  runner — every OS was red on it; (3) seven Linux pure tests built Linux text with
  `Path::join` and got `\` on windows-latest, two tier tests asserted "macOS 14" on a host
  whose words say "Windows 14"; (4) macos-14's `openssl` is LibreSSL (no `pkeyutl -rawin`) —
  the descriptor scripts pick an OpenSSL 3 now; (5) **the Linux killer**: `dist_archive`'s test
  helper ended a child's group with `/bin/kill -KILL -<pid>`, and procps's kill on a pid that
  is no group leader signals the caller's own group — `cargo test` and the runner died with
  no log, four runs in a row, until the Ubuntu container bisect found it (ledger #733). The
  Linux lib tests had never run anywhere before that day; (6) `pgrep -g` counted a zombie
  leader as alive (a cancel took the whole grace); (7) the openssl probe's candidate list had
  lost the PATH entry to a rewrite. **Then the account's Actions spending limit was hit** (27
  Sep 2026, ~12:40 UTC: "The job was not started because recent account payments have failed
  or your spending limit needs to be increased") — the macOS lanes bill 10× and the arm64
  runner is a larger runner. State at that point: macos-14 green, windows-latest and both
  Linux lanes green through every test and gate but the openssl probe, whose fix is on master
  unverified on CI. The Windows lane's `release-windows.sh` has still run only on the Dell.
  **The owner's answer (27 Sep 2026): run the pipeline from `rexenv/runtimes` (public, free
  minutes) until this repo goes public** — runtimes PR: `rexenv-release.yml` + `rexenv-verify.yml`,
  the same workflows with a cross-repo checkout; needs `REXENV_SRC_TOKEN` (Contents: read on
  rexenv/rexenv) and `TAP_TOKEN` in runtimes' secrets. The alternative was to raise the spending
  limit (Settings → Billing → Actions). Then: `rexenv-verify.yml` on
  `windows-latest,ubuntu-22.04,ubuntu-22.04-arm`, then move the `v0.8.8` tag to HEAD (it points
  at the bump commit, before these fixes; nothing was released from it) and let `release.yml`
  draft on the tap. **Done, from runtimes: run 36322831560** (tag at the bump commit) was green
  on Windows and both Linux lanes and red on macOS — the openssl VERSION probe still picked
  LibreSSL, so the probe now signs on a throwaway key and keeps the candidate that can
  (073d5ee1). **Run 36324734214** (tag moved to 073d5ee1): all four lanes green, eight
  assets staged — and `publish` refused the draft: the Windows zip's `.sha256` read
  `<hash>  *rexenv_0.8.8_x64.zip`. Git Bash's `sha256sum` hashes in binary mode on Windows
  and prints `*name`; `release-windows-check.sh` took the name from awk's `$2`. The 0.8.6
  sidecar on the tap carries the same star (checked 27 Sep; nothing ever read these files:
  the updater trusts the descriptor's digest, and `brew` has no Windows). The writer spells the
  name now. The publish gate's `sha256sum -c` is what found it — keep it strict. **Run
  36329787548** (tag at 49c07350): Windows red in verify.sh on the `repo` idle-watchdog test —
  the 18 Sep "not yet explained" Dell flake, now explained: #600's per-spawn handle sweep
  cleared other threads' fresh child pipes (measured 119/400 on the Dell). Fixed on master (the
  sweep runs once at start); the lane was re-run for the tag as it stood — green, and **`publish`
  drafted `v0.8.8` on `rexenv/homebrew-tap` with all eight assets and eight sidecars (27 Sep
  2026, ~15:30 UTC): the first release built entirely on GitHub Actions.** §A0 by hand on the
  downloaded assets and §A on the 15.8 UTM VM (upgraded install; `docs/PUBLISH-TESTING.md`
  0.8.8 record) passed the same evening. Owner's steps from here: Publish, then the six
  runtimes descriptor publishes. Published 16:48 UTC; cask bumped; macOS + Windows descriptors
  live and the in-app update measured on the VM (0.8.7 → 0.8.8) and the Dell (0.8.5 → 0.8.8);
  the six-from-one-page push race is fixed in runtimes PR #12 (`docs/RELEASING.md` step 8).
- **0.8.9, 28 Sep 2026:** bump `9f099de4` tagged and built (run 36346337657, all green, drafted)
  — then nine commits landed on master, so the tag moved to `bfcd8cf1`, the stale draft was
  deleted, and run 36386674456 built it again: all four lanes green first time, draft with 16
  assets, §A0/§A green the same hour (`docs/PUBLISH-TESTING.md`). A tag that moves is cheap
  while nothing is published from it; the pre-push guard re-checked the manifests both times.
- A Linux 22.04 x86_64 **run** of the floor deb: the Dell's WSL gets an Ubuntu-22.04 distro for
  it (in progress).
