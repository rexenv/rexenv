# RELEASING.md — the automated release pipeline

Releases are driven **from GitHub**: a tag builds everything, a human click publishes
it, and the Homebrew tap updates itself. Two workflows implement this.

> **Read the interim section below first if you are cutting a release today.** This
> pipeline needs `rexenv/rexenv` to be public; while it is private the dmg is built
> locally and released from the tap.


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

## ⚠️ Today's flow: the repo is PRIVATE, so the dmg ships from the tap

Everything below the next heading describes the pipeline **as it will run once
`rexenv/rexenv` is public**. It is not the flow in effect right now.

**Why it can't be.** `brew` fetches a cask's `url` with **no authentication**. A private
repo's release asset answers **404** to an unauthenticated GET — so a cask pointing at a
release here installs for nobody, and the tap's poller cannot read this repo's releases
either. Publishing the *source* and publishing the *artefact* are separate decisions:
the source stays private, the dmg goes somewhere public.

**Where it goes.** Into a GitHub Release on **`rexenv/homebrew-tap`** — already public,
already the home of the cask, and same-repo so `update-cask.yml` still needs no secret
of any kind (`SOURCE_REPO` there points at itself; the cask's `url` +`verified:` match).

**And it is built locally, not in CI.** Uploading from this repo to the tap would need a
cross-repo credential — exactly the PAT this pipeline was designed to avoid (see the note
above). A local build also dodges the 10× macOS-minute multiplier on a private repo.

**Before a release that carries an in-app PHP update:** the manifest must be signed
and published, or the button offers nothing.

**Publishing the manifest is not a step in THIS pipeline.** It happens in
`rexenv/runtimes`, where the artifacts and the signing key live:

> Actions → **“Publish PHP update manifest”** → Run workflow.
> `dry_run` on for the first look; run it again with it off to publish.

The key is an **Environment secret with required reviewers** (`manifest-signing`),
not a repo secret. An earlier draft of this file argued the key should never touch
CI at all — the dmg is published locally to avoid a cross-repo credential, so the
manifest could inherit that answer. That reasoning was sound about *repo* secrets
and wrong about the alternative it implied: a procedure that only runs from one
laptop is not a security property, it is a bus factor. The reviewer gate keeps the
honest version of the claim — **reading the key needs a human approval GitHub logs**,
so the key is as safe as approving a run, not as safe as pushing a commit.

  - `scripts/gen-release-key.sh` — mints the pair ONCE, wired into no pipeline.
    A key a build can mint is a key an attacker who reaches the build can mint.
  - `scripts/check-php-pins.sh` — the one check only this repo can make: that
    runtimes' `PINS` list knows about every minor `PHP_VERSIONS` ships. A missing
    MINOR there means that minor can never be offered an update, silently.
  - The publisher itself lives in runtimes (`scripts/publish-manifest.sh`, and the
    workflow that runs it). This repo used to carry a second copy; two
    implementations of one document format in two repos is drift waiting to
    happen, and the copy here could not see the `PINS` that drive discovery.

The public half is compiled into `core/updates.rs`, so **rotation is an app
release** — which is the property that makes a stolen key survivable. Ledger
#348/#350.

1. Bump the version in all four manifests as in step 1 below, and commit.
2. `./scripts/verify.sh` — the bar, same as in CI. Green verdict = its own
   `verify: all green` line.
3. `pnpm release:mac` → `src-tauri/target/universal-apple-darwin/release/bundle/dmg/rexenv_<X.Y.Z>_universal.dmg`.
   Runs `scripts/release-mac.sh`, which PRE-CLEANS before building. `tauri build`
   shells out to a generated `bundle_dmg.sh` that attaches a temporary
   `rw.<pid>.<name>.dmg`; when that dies partway the image stays ATTACHED and every
   later build fails with only `error running bundle_dmg.sh` — naming neither the
   volume nor the file. It cost two builds on 18 Aug 2026 and left a volume mounted
   on the developer's Mac each time. The clean is scoped to images whose backing
   path is inside `src-tauri/target` — **never widen it to match a volume NAME**,
   which is random and says nothing about the owner (this machine has iOS simulator
   runtimes mounted). The previous finished `.dmg` is deliberately NOT deleted, only
   warned about: a build that fails after we removed it would leave you with
   neither, and §A0's "exactly one dmg" check is what the warning is for.
4. Run `docs/PUBLISH-TESTING.md` **§A0 by hand** — CI normally does it (the per-slice
   `lipo`/`strings`/`codesign` checks in `release.yml`'s "§A0 artefact integrity" step
   are the script; copy them). Then **§A**, which was always human-only.
5. Release it, draft-first — publishing IS the §A sign-off, that rule does not relax:
   ```sh
   V=<X.Y.Z>
   DMG=src-tauri/target/universal-apple-darwin/release/bundle/dmg/rexenv_${V}_universal.dmg
   shasum -a 256 "$DMG" | awk '{print $1 "  rexenv_'"$V"'_universal.dmg"}' > "rexenv_${V}_universal.dmg.sha256"
   gh release create "v$V" --repo rexenv/homebrew-tap --draft \
     --title "rexenv $V" "$DMG" "rexenv_${V}_universal.dmg.sha256"
   ```
   **If the create is interrupted while the dmg is uploading, delete the half-asset
   before retrying** (27 Aug 2026, 0.4.0): the draft is created first and the 26 MB
   upload follows, so a killed command leaves an asset in state `starter` holding the
   name, and every later `gh release upload` — `--clobber` included — answers
   **`HTTP 400: Bad Request`** naming only the upload URL. Nothing says "partial".
   ```sh
   gh api repos/rexenv/homebrew-tap/releases/<id>/assets --jq '.[] | "\(.id) \(.name) \(.state)"'
   gh api -X DELETE repos/rexenv/homebrew-tap/releases/assets/<asset-id>
   ```
   Then verify the upload rather than trusting exit 0: the asset's `digest` from the
   API must equal the local `shasum -a 256` and the sidecar's text. That is the same
   hash match §A records, done one step earlier, and it is what proves the bytes
   survived the wire.
   Tag the same `v<X.Y.Z>` **here** too, so a shipped dmg maps to a commit. Careful:
   **pushing a `v*` tag triggers `release.yml`**, which would spend tens of 10×-billed
   macOS minutes building a second dmg nobody can download. Keep the tag local
   (`git tag -a v<X.Y.Z> <commit>`) until the repo is public, or disable the
   **Release** workflow in the Actions tab first.

   **This is enforced, not remembered** (16 Aug 2026): `scripts/git-hooks/pre-push`
   refuses a `v*` tag push to `rexenv/rexenv` while the repo is private, and stands
   down on its own once it is public. It exists because this paragraph is a memory,
   and the piped-verdict rule proved that a rule relying on memory is not a control —
   it was walked into by the person who wrote it, in the session he wrote it.
6. Publish the tap release → **Update cask** picks it up (≤15 min, or Run workflow).

### Going public later — three things flip in one commit

The cask's `url`, the cask's `verified:`, and `SOURCE_REPO` in `update-cask.yml` must
all name the same repo; the workflow greps for that and fails loudly if they drift.
Move all three back to `rexenv/rexenv`, delete the interim releases from the tap (or
leave them — the cask only names the current version), and this section goes away.

## Cutting a release (the automated pipeline — for when the repo is public)

1. Bump the version in **all four** manifests (the workflow refuses a mismatch):
   `src-tauri/tauri.conf.json`, `package.json`, `src-tauri/Cargo.toml`,
   `cli/Cargo.toml`. Commit.
2. **Tag the commit you are SHIPPING — annotated, with a message that says what the
   release means.** Not necessarily the bump commit: work continues after a version
   bump, and what ships is HEAD at release time. Both real releases did this and the
   step used to imply otherwise, which made `v0.3.0` look misplaced when it was not —
   the workflow triggers on the tag and builds THAT ref, so **the tag is the release
   commit, by definition**. The bump commit is only where the number changed.
   `-a` matters: `v0.2.0`'s body explains that the minor was forced by a macOS floor
   rise, and six weeks later that is the only place the reason survives. `v0.3.0`'s
   message is bare `rexenv 0.3.0` and says nothing — don't repeat it. A published tag
   is not re-pointed or re-worded afterwards; CI built from it and the tap's release
   references it.
   Either:
   - `git tag -a v<X.Y.Z> -m "rexenv <X.Y.Z>" -m "<what this release means>" <commit>`
     then `git push origin v<X.Y.Z>`, or
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

- **`rexenv/rexenv` must be public** for the pipeline above to run at all. Two things
  depend on it: the cask's `url` is fetched by users' machines with no auth, and the
  tap's poller reads this repo's releases cross-repo. While it is private the poller
  finds nothing here and the dmg ships from the tap instead — see the interim section
  at the top, which is the flow in effect today — and, as of 21 Aug 2026, the ONLY flow that has ever
cut a release: 0.1.0, 0.1.1, 0.2.0, 0.3.0 and 0.4.0 all shipped through it. It was written as a
provisional note dated 2026-08-12; five releases in, "interim" describes the intention
rather than the practice.
- **No secrets to create.** That is the design — see the note above. It holds in the
  interim flow too, which is why the dmg goes to the tap rather than to a third repo
  the tap's own `GITHUB_TOKEN` could not read.

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
