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

- The first tag run: it has never executed end to end (the old workflow never ran either —
  the repo was private and the flow was local). The first real release is the proof; expect
  one round of runner-shaped fixes (the Windows lane's `release-windows.sh` has run only on
  the Dell).
- A Linux 22.04 x86_64 **run** of the floor deb: the Dell's WSL gets an Ubuntu-22.04 distro for
  it (in progress).
