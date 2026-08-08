# RELEASING.md — the automated release pipeline

Releases are driven **from GitHub**: a tag builds everything, a human click publishes
it, and the Homebrew tap updates itself. Two workflows implement this:

```
git push origin v<X.Y.Z>            (or: Actions → "Release" → Run workflow)
        │
        ▼
.github/workflows/release.yml   — macos-14 runner
        │  version guard: tag == tauri.conf.json == package.json == both Cargo.tomls
        │  scripts/verify.sh (the bar — lib tests + examples + clippy + tsc)
        │  pnpm release:mac   → rexenv_<X.Y.Z>_universal.dmg
        │  §A0 artefact integrity (per-slice payloads, lipo, codesign) — automated
        ▼
   DRAFT GitHub Release  ·  dmg + .sha256 attached
        │
        │  ← THE HUMAN GATE: download the dmg, run PUBLISH-TESTING §A
        │    (quarantine → Gatekeeper blocks → xattr -rd → launches).
        │    CI cannot do this; it needs a real Mac and a GUI.
        ▼
   click "Publish release" on GitHub
        │
        ▼
.github/workflows/update-tap.yml  — ubuntu runner
        │  downloads the PUBLISHED asset (never a local build), sha256s it,
        │  rewrites version + sha256 in rexenv/homebrew-tap Casks/rexenv.rb, pushes
        ▼
   brew update && brew upgrade --cask rexenv   (users)
```

## Cutting a release

1. Bump the version in **all four** manifests (the workflow refuses a mismatch):
   `src-tauri/tauri.conf.json`, `package.json`, `src-tauri/Cargo.toml`,
   `cli/Cargo.toml`. Commit.
2. Either:
   - `git tag v<X.Y.Z> && git push origin v<X.Y.Z>`, or
   - GitHub → Actions → **Release** → *Run workflow* → enter `<X.Y.Z>` (creates the
     tag for you; token-pushed tags don't re-trigger the workflow).
3. Wait for the draft release. Download the attached dmg and run
   `docs/PUBLISH-TESTING.md` **§A** on it (§A0 already ran in CI). Record the pass
   next to the dmg's sha256 in that doc.
4. **Publish** the release. The tap bumps itself within minutes.
5. Sanity check: `brew update && brew audit --cask --online rexenv/tap/rexenv`,
   or the full §D dry-run for a first-time setup.

## One-time setup (required before the first automated release)

- **`TAP_PUSH_TOKEN` secret** on `rexenv/rexenv` (Settings → Secrets and variables →
  Actions): a **fine-grained PAT** scoped to the single repo `rexenv/homebrew-tap`
  with **Contents: Read and write** only. The default `GITHUB_TOKEN` cannot push
  cross-repo, so without this secret `update-tap.yml` fails at checkout.
- **`rexenv/rexenv` must be public** (or the dmg hosted somewhere public): the cask's
  `url` is fetched by users' machines with no auth. Draft-building works on a private
  repo; `brew install` does not.

## Rules the pipeline encodes (don't undo them by hand)

- **The release is born a draft.** §A is publish-blocking and human-only; publishing
  IS the sign-off. Never flip the workflow to publish directly.
- **The cask hash comes from the published asset.** `update-tap.yml` downloads what
  users will download and hashes that. Hand-editing the cask from a local build's
  hash reintroduces the exact staleness bug the old staging copy had.
- **Prereleases don't touch the tap** (`if: !prerelease`) — the cask tracks stable.
- **§A0's payload list lives in the workflow now.** When something new is compiled
  into the binary, add its per-slice check to the "§A0 artefact integrity" step in
  `release.yml` (and to `docs/PUBLISH-TESTING.md` §A0) in the same commit.
- Runner note: `macos-14` is arm64; the universal build cross-compiles the x86_64
  slice via the checked-in `rustup target add`. Private-repo macOS minutes bill at a
  10× multiplier — the verify + universal build takes tens of minutes per run.
