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
rexenv/homebrew-tap .github/workflows/update-cask.yml
        │  polls every 15 min (or Actions → "Update cask" → Run workflow for now):
        │  latest PUBLISHED release ≠ cask version → downloads the asset, sha256s it,
        │  rewrites version + sha256, pushes — with the tap's own built-in token
        ▼
   brew update && brew upgrade --cask rexenv   (users)
```

**Why the bump lives in the tap repo, not here.** A workflow pushing to its OWN repo
uses the built-in `GITHUB_TOKEN`, so the pipeline needs **no PAT, no deploy key, no
stored secret**. Pushing from this repo to the tap would need a cross-repo credential:
the org has deploy keys disabled, and GitHub has no API to mint a PAT — so it would be
a hand-made token that expires and silently breaks releases. The cost of the swap is
latency (≤15 min, or instant via Run workflow), which a release does not care about.
Verified 2026-08-08: the tap workflow's explicit `permissions: contents: write` is
granted (`Contents: write` in the run log) even though the org default is read.

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
4. **Publish** the release. The tap picks it up within 15 minutes — or immediately
   from `rexenv/homebrew-tap` → Actions → **Update cask** → *Run workflow*.
5. Sanity check: `brew update && brew audit --cask --online rexenv/tap/rexenv`,
   or the full §D dry-run for a first-time setup.

## One-time setup (required before the first automated release)

- **`rexenv/rexenv` must be public** (or the dmg hosted somewhere public). Two things
  depend on it: the cask's `url` is fetched by users' machines with no auth, and the
  tap's poller reads this repo's releases cross-repo. While it is private the poller
  logs "no published release … (or it is not public) — nothing to do" and exits green.
- **No secrets to create.** That is the design — see the note above.

## Rules the pipeline encodes (don't undo them by hand)

- **The release is born a draft.** §A is publish-blocking and human-only; publishing
  IS the sign-off. Never flip the workflow to publish directly.
- **The cask hash comes from the published asset.** `update-cask.yml` downloads what
  users will download and hashes that. Hand-editing the cask from a local build's
  hash reintroduces the exact staleness bug the old staging copy had.
- **Prereleases don't touch the tap** (`if: !prerelease`) — the cask tracks stable.
- **§A0's payload list lives in the workflow now.** When something new is compiled
  into the binary, add its per-slice check to the "§A0 artefact integrity" step in
  `release.yml` (and to `docs/PUBLISH-TESTING.md` §A0) in the same commit.
- Runner note: `macos-14` is arm64; the universal build cross-compiles the x86_64
  slice via the checked-in `rustup target add`. Private-repo macOS minutes bill at a
  10× multiplier — the verify + universal build takes tens of minutes per run.
