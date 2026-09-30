# RELEASING.md — the automated release pipeline

Releases are driven **from GitHub**: a tag builds everything, a human click publishes
it, and the Homebrew tap updates itself. Two workflows implement this.

> **Public since 30 Sep 2026 — the pipeline runs HERE again.** `release.yml` builds on the
> tag push (free minutes on a public repository). From 27 to 30 Sep 2026, after this repo's
> Actions quota ran out on the first run (macOS lanes bill 10× on a private repo, the arm64
> runner was a larger runner), the same two workflows ran from `rexenv/runtimes` with a
> cross-repo checkout (`REXENV_SRC_TOKEN`); 0.8.8–0.8.11 were drafted from there. That interim
> is retired: the runtimes copies (`rexenv-release.yml`, `rexenv-verify.yml`) and the token are
> gone, and a change to `release.yml` here is live at once. Lesson kept from the interim: the
> release-notes fix (`63e602ac`, ledger #736) landed here and the 0.8.10 cut found the copy
> still shipping the "Draft until §A …" warning as the public notes — two copies of a
> pipeline drift.
>
> **Since 27 Sep 2026 every release is built ONLY on GitHub Actions, every OS at once**
> (owner ruling, `docs/PLAN-ci-release.md`): one tag → macOS + Windows + Linux ×2 lanes →
> one DRAFT — on `rexenv/rexenv` since 0.8.11 (30 Sep 2026); on the tap before — with all eight assets. The "build locally, upload to
> the tap by hand" flow that cut 0.1.0–0.8.7 is retired; its section below is kept as the
> record of why the artefacts live on the tap.


```
git push origin v<X.Y.Z>            (or: Actions → "Release" → Run workflow)
        │  the ANNOTATED tag's body = the public release notes (scripts/tag-notes.sh)
        │
        ▼
.github/workflows/release.yml   — macos-14 runner
        │  version guard: tag == tauri.conf.json == package.json == both Cargo.tomls
        │  scripts/verify.sh (the bar — lib tests + examples + clippy + tsc)
        │  pnpm release:mac   → rexenv_<X.Y.Z>_universal.dmg
        │                     + rexenv_<X.Y.Z>_universal.app.tar.gz (the in-app update)
        │  §A0 artefact integrity (per-slice payloads, lipo, codesign) — automated,
        │                     on the .app AND on the bundle extracted from the archive
        ▼
        │
        │  ── and, on a windows-latest runner, the same tag's Windows half:
        │  version guard (the same script) · scripts/verify.sh (the same bar)
        │  pnpm release:win  → rexenv_<X.Y.Z>_x64-setup.exe   (NSIS, per-user, UNSIGNED)
        │                    + rexenv_<X.Y.Z>_x64.zip         (the in-app update: the
        │                      install directory's contents, flat — no uninstall.exe)
        │  §A0-windows (one installer, the rex sidecar, PE x64, embedded payload +
        │                     update key, NotSigned; the zip extracted and re-checked,
        │                     its VERSIONINFO read the way the swap reads it) — then
        │                     ATTACHED to the draft above, not a second release.
        │                     **Never run: the repo is private.**
        │
        │  ── and, on an ubuntu-24.04 runner, the same tag's Linux x86_64 half:
        │  version guard · scripts/verify.sh · pnpm release:linux
        │                    → rexenv_<X.Y.Z>_amd64.deb + rexenv_<X.Y.Z>_amd64.AppImage
        │  §A0-linux (one deb + one AppImage for the version and THIS arch, the rex
        │                     sidecar, the ELF arch, embedded payload + update key, the
        │                     deb's control/members/polkit action/Depends, and both
        │                     artefacts answering --print-version — what the in-app
        │                     updater re-checks before dpkg -i / the exchange) — then
        │                     ATTACHED to the draft. aarch64 is built on the VM by hand
        │                     (arm runners are paid on a private repo). **Never run.**
        ▼
   DRAFT GitHub Release  ·  dmg + tar.gz + setup.exe + deb + AppImage + their .sha256 attached
        │
        │  ← THE HUMAN GATE: download the dmg, run PUBLISH-TESTING §A
        │    (quarantine → Gatekeeper blocks → xattr -rd → launches).
        │    CI cannot do this; it needs a real Mac and a GUI.
        ▼
   click "Publish release" on GitHub
        │
        ▼
rexenv/homebrew-tap .github/workflows/update-cask.yml
        │  needs a trigger for a release in ANOTHER repo — see "Going public later"
        │  (today, releases are published in the tap and fire its own event):
        │  latest PUBLISHED release ≠ cask version → downloads the asset, sha256s it,
        │  rewrites version + sha256, pushes — with the tap's own built-in token
        ▼
   brew update && brew upgrade --cask rexenv   (users)
```

**The asset NAMES are an interface, not a detail** (28 Sep 2026). The one-command
installers in the tap (`install.sh`, `install.ps1` — `docs/archive/PLAN-install-scripts.md`) build
`rexenv_<X.Y.Z>_universal.app.tar.gz`, `_x64-setup.exe`, `_<amd64|arm64>.deb` and
`_<amd64|aarch64>.AppImage` plus each `.sha256` (`<hash>  <name>`) from the tap's
`releases/latest` tag. Rename one and every new install on that OS fails, while the cask —
which names only the dmg — keeps passing. The tap's `install-scripts.yml` installs the
latest published release on every OS on `release: published` and weekly; a red run there
after a publish is this, and the fix is in the scripts or the names, in the same change.

**Why the bump lives in the tap repo, not here.** A workflow pushing to its OWN repo
uses the built-in `GITHUB_TOKEN`, so the pipeline needs **no PAT, no deploy key, no
stored secret**. Pushing from this repo to the tap would need a cross-repo credential:
the org has deploy keys disabled, and GitHub has no API to mint a PAT — so it would be
a hand-made token that expires and silently breaks releases.

**The trigger is the release itself, not a clock** (11 Sep 2026). `update-cask.yml` used
to poll on a `*/15` cron; GitHub runs schedules when it has capacity, and that day the
runs landed 4.5 hours apart while a published 0.7.0 sat unshipped. It now runs on the
tap's own `release: published` — a human publishing is an event that starts workflows,
so still no credential — and checks out the default branch explicitly, because on a
release event the checkout is the tag, a detached HEAD the bump cannot push from.
Verified 2026-08-08: the tap workflow's explicit `permissions: contents: write` is
granted (`Contents: write` in the run log) even though the org default is read.

## The artefacts live on `rexenv/rexenv` (since 0.8.11) — the tap keeps the cask and the installers

**Owner ruling, 30 Sep 2026, the day the repo went public:** "ekhon theke rexenv tei release
gulo dite … jeno oikhan thekei sob download korte pare" — releases live here, every download
comes from the source repository, and the in-app self-update must keep working. What that
changed, and what it deliberately did not:

- **Where it goes.** A GitHub Release on **`rexenv/rexenv`**, drafted by `release.yml`'s
  `publish` job with the workflow token — no cross-repo secret for the draft. The eight assets
  and their `.sha256` files keep their names (an interface: `install.sh`, `install.ps1`, the apt
  publisher and the cask all build them from the version).
- **The tap keeps the cask and the one-command installers** (`rexenv/homebrew-tap`: `brew tap
  rexenv/tap` needs a tap, and the scripts' URLs are printed everywhere). Its `update-cask.yml`
  bumps the cask from the PUBLISHED dmg's hash — but a release here is an event in another
  repository the tap never sees, so **`release-published.yml` here sends it a
  `repository_dispatch` (`rexenv-release`) on `release: published`, with `TAP_TOKEN`** (the
  fine-grained PAT that used to create the draft on the tap; `contents: write` there covers a
  dispatch). The tap also polls once a day, the fallback for a failed dispatch. `SOURCE_REPO`
  in that workflow and the cask's `url` both name `rexenv/rexenv`; the workflow refuses to bump
  when they drift.
- **The self-update descriptor did NOT move**: it is a committed file on `rexenv/runtimes`
  whose URL is compiled into every shipped build. What moved is the descriptor's `url` FIELD
  (signed data) — `rexenv/runtimes`'s publisher (`TAP_REPO`, now `rexenv/rexenv`) reads the
  newest published release here. Every app since 0.6.0 accepts BOTH download prefixes
  (`core/app_update.rs` `ALLOWED_RELEASE_PREFIXES`), so a 0.8.10 install updates to 0.8.11 from
  here, and a descriptor published before the move (naming a tap asset) still verifies on an
  app built after it.
- **`rexenv/apt`'s publisher and the website's release sync read the newest releases here**
  (`TAP` → `rexenv/rexenv` in `build-site.sh`; `sync-release.mjs`).
- **0.8.8, 0.8.9 and 0.8.10 were mirrored here on the day of the move** — the same bytes as
  the tap's, every `.sha256` re-checked, the tap's release notes — so `releases/latest`, the
  cask's flipped `url` (`v0.8.10` at the time), apt's `KEEP=3` window and the website's
  changelog had a history to read the moment the consumers flipped. The tap's releases stay as
  they were: the cask only names the current version, and the descriptors already published
  point at them.
- **Before 30 Sep 2026** (0.1.0–0.8.10): the dmg was released on the tap because this repo was
  private — `brew` fetches a cask's `url` with no authentication and a private repo's release
  asset answers 404 — and the tap's own `release: published` bumped the cask with no token. From
  0.1.0 to 0.8.7 the dmg was also BUILT locally (retired 27 Sep 2026: a release is what
  `release.yml` builds on hosted runners, all three OSes or nothing).

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

1. Bump the version in all four manifests as in step 1 below, and commit, then
   `./scripts/check-versions.sh` — the guard CI has (and, while this repo is private,
   never runs). Five releases were cut with nothing checking this.
   **If the release adds a migration** (`MIGRATIONS.len()` grew since the last tag), its note
   says that going back to rexenv 0.7.1 or older afterwards is not supported: those builds
   predate the schema guard (ledger #593) and would open the newer database without refusing.
   Every build after them refuses on its own.
2. `./scripts/verify.sh` — the bar, same as in CI. Green verdict = its own
   `verify: all green` line.
3. `pnpm release:win` (on Windows, in Git Bash) → `src-tauri/target/release/bundle/nsis/rexenv_<X.Y.Z>_x64-setup.exe`.
   Runs `scripts/release-windows.sh`, which PRE-CLEANS for the Windows reason: the
   app's DNS agent is the app's own binary, it outlives the app by design and its
   watchdog puts it back, so a link step that has to replace `rexenv.exe` dies with
   `Access is denied (os error 5)` naming neither the holder nor the fact that
   closing the app does not release it (measured 19 Sep 2026). It kills by image
   name — ours — and then PROVES the file is writable before starting a 20-minute
   build that would otherwise fail at the end. It builds `--bundles nsis` only:
   `bundle.targets` is `"all"`, which on Windows also means an MSI, and WiX installs
   per-machine and wants admin — the opposite of D5. The narrowing lives in the
   script, not the config, so the macOS build is not changed for a Windows reason.
   **After the build it runs `scripts/release-windows-check.sh`** — §A0's Windows
   half: exactly one installer named for the version, the `rex` sidecar present, the
   PE machine field read off the header (not the filename), the embedded
   `Dist_Archive_Command` payload, and `NotSigned` — asserted, because
   `docs/INSTALL.md` promises the user a specific "Unknown Publisher" dialog and that
   page becomes a lie the day a certificate appears without it being rewritten.
   **It also produces the update archive**, `rexenv_<X.Y.Z>_x64.zip`: `rexenv.exe` and
   `rex.exe` at the root and nothing else. Flat because the swap's extractor strips
   nothing; without `uninstall.exe` because the installer writes that at install time and
   the swap carries the installed one across (`platform/windows/app_bundle.rs`). The
   archive is extracted and re-checked — PE x64, the embedded payload, the update key, and
   the executable's own `ProductVersion`/`ProductName`, the words the swap verifies —
   because what a user receives is what comes OUT of it, not what went in.

3b. **Linux (not yet cut — the port landed 24 Sep 2026, `docs/PLAN-linux-port.md`).**
   `pnpm release:linux` on an Ubuntu 22.04+ host of EACH arch runs `scripts/release-linux.sh`:
   plain `tauri build` (`bundle.targets` "all" → `.deb`, `.rpm`, `.AppImage`;
   `bundle.linux.deb.depends` names webkit2gtk 4.1, the appindicator, xdg-utils, libnss3-tools,
   `pkexec | policykit-1` — 26.04 has no `policykit-1` package at all, measured on the Dell's WSL
   25 Sep 2026; a deb naming only it is uninstallable there — and MySQL's `libaio1 | libaio1t64`/
   `libnuma1`), then restarts the `rexenv-dns` user unit if a
   dev launch registered one on the binary the build replaced (Linux does not lock a running
   executable, so the build succeeds and the OLD agent keeps running — the opposite failure to
   Windows's). **Then `scripts/release-linux-check.sh` — §A0's Linux half:** exactly one deb and
   one AppImage named for the version and THIS arch, the `rex` sidecar, the ELF arch read off
   the header, the embedded `Dist_Archive_Command` payload and update key, the deb's control
   fields (`rexenv`, the version, the arch), its members (`usr/bin/{rexenv,rex}`, the polkit
   action, the desktop entry) and `Depends`, and BOTH artefacts answering `--print-version` with
   the version — extracted (`dpkg-deb -x`) and extract-and-run (`APPIMAGE_EXTRACT_AND_RUN=1`),
   never installed, because these are exactly the facts the in-app updater verifies on the
   user's machine before `dpkg -i` or the exchange (`platform/linux/app_bundle.rs`); a release
   that fails here is one every installed copy would refuse. Writes the `.sha256` sidecars. The
   pre-L7 0.8.7 bundles on the VM failed it on `--print-version` (they predate the flag) — the
   check working, not a bug. **A 4 GB host kills the release `rustc` (OOM, measured on the VM 24
   Sep 2026): give the builder 8 GB or a swapfile, `CARGO_BUILD_JOBS=1`, and ~10 GB of disk (one
   release target + the bundles).** Two archs are two hosts, and BOTH build on the 22.04 floor — a
   deb runs on nothing older than the glibc it was linked against (the Dell's WSL Ubuntu 26.04
   build proves x86_64 and installs only from 26.04 up): x86_64 is `release.yml`'s
   `release-linux` job on `ubuntu-22.04` (attaches to the draft like the Windows job), and
   `linux-build.yml` is the same build on dispatch with no release, asserting the binary's
   highest `GLIBC_` symbol version is ≤ 2.35 and uploading the pair as a run artifact;
   aarch64 is the 22.04 UTM VM by hand, uploaded to the tap release beside the dmg. The AppImage needs `libfuse2` (`libfuse2t64` on 24.04) to MOUNT; rexenv's own checks
   never mount it. **The in-app update reads ONE
   descriptor per package kind and arch** — `app-manifest-linux-deb-x86_64.json`,
   `-deb-aarch64`, `-appimage-x86_64`, `-appimage-aarch64`, each + `.sig`, same schema and key
   as macOS's — so a Linux release is four artifacts and four publisher runs (step 8's
   `--linux <deb|appimage> <arch>` mode); a variant with no document is simply never offered
   anything. No §A0-linux check exists yet.
4. `pnpm release:mac` → `src-tauri/target/universal-apple-darwin/release/bundle/dmg/rexenv_<X.Y.Z>_universal.dmg`.
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
   **After the build it runs `scripts/release-assets.sh`**, which writes
   `rexenv_<X.Y.Z>_universal.app.tar.gz` + its `.sha256` — what an in-app update
   downloads, since a self-update replaces a DIRECTORY and cannot use a dmg — asserts
   the archive's layout (exactly one top-level `rexenv.app/`, no AppleDouble members;
   both shapes break the in-app extractor), and re-runs §A0 **on the bundle that comes
   back out of the archive**, which is the copy an updating user actually receives.
   Checking the artefact and shipping a different one is the gap that closes.
5. Run `docs/PUBLISH-TESTING.md` **§A0 by hand** for the .app and the dmg — CI normally
   does it (the per-slice `lipo`/`strings`/`codesign` checks in `release.yml`'s "§A0
   artefact integrity" step are the script; copy them). The EXTRACTED-bundle half of §A0
   already ran in step 3. Then **§A**, which was always human-only.
6. Release it, draft-first — publishing IS the §A sign-off, that rule does not relax:
   ```sh
   V=<X.Y.Z>
   DMG=src-tauri/target/universal-apple-darwin/release/bundle/dmg/rexenv_${V}_universal.dmg
   shasum -a 256 "$DMG" | awk '{print $1 "  rexenv_'"$V"'_universal.dmg"}' > "rexenv_${V}_universal.dmg.sha256"
   TAR=src-tauri/target/universal-apple-darwin/release/bundle/macos/rexenv_${V}_universal.app.tar.gz
   gh release create "v$V" --repo rexenv/homebrew-tap --draft \
     --title "rexenv $V" "$DMG" "rexenv_${V}_universal.dmg.sha256" \
     "$TAR" "$TAR.sha256"
   ```
   (`release-assets.sh` prints this exact line with the paths filled in.) **The new
   assets must never end in `_universal.dmg`**: the tap's `update-cask.yml` selects the
   cask's asset by that suffix and `head -1`, so a second match would silently hash the
   wrong file into the cask.
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
   (Retired 27 Sep 2026: this step used to say "keep the tag local — pushing it would
   build a dmg nobody can download", and `scripts/git-hooks/pre-push` refused the push.
   Pushing the tag IS the release now; the hook instead refuses a tag whose version
   disagrees with the four manifests AT THE TAGGED COMMIT — the one thing a local hook
   can still catch before a 40-minute build, and cheaper than a second tag.)
7. Publish the tap release → **Update cask** runs on that publish and bumps the cask
   within a minute (Actions → Update cask → Run workflow if it did not).
   Publishing is also what makes the update archive reachable at all: a draft's assets
   answer 404 for everyone, so the §A gate protects in-app updaters for free.
8. **`rexenv/runtimes` → Actions → "Publish app update manifest"** — dry run first, then
   for real. Six documents now (macOS, Windows, Linux deb/AppImage × x86_64/aarch64), one
   run each, each behind the `manifest-signing` approval. **Approve them together only
   because runtimes PR #12 (27 Sep 2026) taught the script to rebase and retry its push:**
   the first 0.8.8 round approved all six from one page and five were rejected
   `main -> main (fetch first)` — each run had committed its own file seconds after another
   pushed. **A rerun does not fix that**: it checks out the run's ORIGINAL commit and
   pushes from the same stale main. Queue a NEW run instead. Two runs on the SAME document are
   serialised by their concurrency group but each reads the serial from the commit it was
   triggered at — runtimes PR #14 (28 Sep 2026) pulls the checkout to the tip first; before it,
   approve same-document runs one at a time. It reads the tap's `releases/latest` (so it can never name a draft or a
   prerelease), takes the archive's immutable API digest, re-hashes what it downloaded,
   increments its own serial, signs with the reviewer-gated key and commits
   `app-manifest.json` + `.sig`. **Until this runs, no installed rexenv is offered
   anything** — the dmg is downloadable, the cask is bumped, the website is updated, and
   the feature that just shipped is off. That is the whole reason step 8 exists.
   **Windows has its own document, `app-manifest-windows.json` + `.sig`** — same schema,
   same key, same serial rule — because one `release` names one artifact and the macOS one
   is a universal `.app.tar.gz`. **The runtimes publisher has a `--windows` mode** (merged
   19 Sep 2026, https://github.com/rexenv/runtimes/pull/3): Actions → "Publish app update
   manifest" → `windows: true`, `dry_run` first, as for macOS — after a release that carries
   the zip. Until it has RUN, no installed Windows rexenv is offered anything, exactly as
   step 8 says of macOS.
   **Linux has FOUR documents** (`app-manifest-linux-<deb|appimage>-<x86_64|aarch64>.json`
   + `.sig`, L7 of `docs/PLAN-linux-port.md`): the publisher's `--linux <kind> <arch>` mode,
   one run per artifact the release carries, `dry_run` first. A `.deb` is verified on the
   user's side by its control fields and members before `dpkg -i`, an AppImage by running it
   with `--print-version` — so the artifact the descriptor names MUST be the one `tauri build`
   produced, unrenamed. Until each has RUN, that kind+arch is offered nothing.
   **And the winget manifest** (the tap's Windows counterpart): `./scripts/winget-manifest.sh`
   renders the three manifests for the tap's latest release into
   `src-tauri/target/winget/<version>/` — the installer URL and sha256 from the PUBLISHED
   asset, downloaded and hashed here rather than trusted from the API, `Scope: user` because
   that is the only mode rexenv ships, and `AppsAndFeaturesEntries` naming the uninstall entry
   the NSIS installer writes (`ProductCode: rexenv`). `winget validate --manifest <dir>` on a
   Windows machine says "Manifest validation succeeded" (measured 19 Sep 2026 against the
   first installer, rendered from `--local`). **Every URL in it must be one the PUBLIC can open**:
   the first render pointed `PackageUrl`/`PublisherSupportUrl`/`LicenseUrl` at `rexenv/rexenv`, which
   is private and answers 404 to everyone including winget's own URL validation — they name the tap
   now, and `LicenseUrl` is omitted rather than pointed at a file nobody can read. Submission is a PR
   to `microsoft/winget-pkgs` under `manifests/r/rexenv/rexenv/<version>/`, one version per PR — their
   rule. Their repo is far too big to clone for three YAMLs: fork it (`gh repo fork --clone=false`),
   branch from upstream `master`, PUT the three files through the contents API, then `gh pr create`
   (first submission: 0.8.3, PR 437674, 20 Sep 2026). No signing
   requirement stands in the way (their SmartScreen check is the URL's reputation, not the
   binary's signature; `docs/TODO.md` W11).
9. `./scripts/check-app-manifest.sh` (and `--windows` for the second document, `--linux <deb|appimage> <x86_64|aarch64>` for each of the four Linux ones) — verifies
   the published descriptor against the key compiled into THIS tree, warns when the tap is ahead of it (the forgotten step 7), and
   compares the descriptor's sha256 to the published asset's digest.
   **Run within five minutes of step 7 and it may say "only the CDN is behind".** Installed
   apps read `raw.githubusercontent.com`, which caches the file (the new one appeared 3 min
   10 s after 0.7.1's commit), so before blaming a missed publish the check reads the
   committed file through the contents API and verifies that signature too. That line means
   wait and re-run — never publish again. The release is done only at `all green`; until
   13 Sep 2026 this step printed the forgotten-click advice inside the cache window
   (ledger #594).

### A patch release cut from a published tag (first used for 0.7.1, 13 Sep 2026)

When a fix must ship without everything master gained since the last tag — 0.7.1 was the
notices fix alone, while 65 commits of Windows groundwork and features stayed on master
(owner ruling) — cut it from the tag. Do not rebuild the published version:

- **Never re-issue a published version's bytes.** The cask's sha256 breaks, and a user who
  already has 0.7.0 and the tap would hold different things under one name. PHP 7.4's
  candidate was rebuilt only because it had not been published yet; that is the line.
- `git worktree add -b release/X.Y.Z <dir> vX.Y.W`, so the main checkout the IDE works in is
  untouched. Apply only the fix, the docs its same-commit rule demands, and the four-manifest
  bump; `cargo update --workspace --offline` in `src-tauri/` and `cli/` moves only the
  workspace entries of both lockfiles, and `scripts/check-versions.sh` confirms all four agree.
  **A tag older than 13 Sep 2026 needs one tooling fix first:** its `verify-receipt.sh`
  writes to `$ROOT/.git/…`, and in a worktree `.git` is a file — so `verify.sh` passes every
  gate and then fails writing the receipt, and the pre-commit hook can never pass. 0.7.1's
  first `verify-full` died exactly there. The fix (the checkout's own
  `git rev-parse --absolute-git-dir`) is on master since then; carry that one line onto the
  release branch as its own commit. It ships in no binary. **So does the second:** the
  examples' `curl` calls had no `--max-time`, and 0.7.1's second `verify-full` sat 50 minutes
  on a fixture FrankenPHP that accepted a request and never answered — carry the bounds too.
- **Before `verify-full`, check nothing already listens on :5199** (`lsof -nP -iTCP:5199
  -sTCP:LISTEN`). Its wk-checks start vite there and then only ask whether the port answers,
  so a vite left behind by an earlier run — 0.7.1's release found one four days old — would
  have the WebKit checks test whatever tree THAT vite serves, and pass.
- Point the worktree's `src-tauri/target` at the main checkout's with a symlink. A second cold
  universal build needs more disk than this Mac had free (19 GiB on 13 Sep), and the relative
  bundle paths `release-mac.sh`, `release-assets.sh` and §A0 use keep working through it.
  **Move — never delete — any older dmg / `.app.tar.gz` out of `bundle/` first**: §A0
  requires exactly one dmg, and the old one may be the only copy of something.
- `scripts/wk-checks/node_modules` is untracked, so a worktree has none and `verify-full.sh`
  stops. Link the main checkout's when `scripts/wk-checks/package*.json` are unchanged since
  the tag; install otherwise.
  **Never link the ROOT `node_modules` the same way — install it** (`pnpm install
  --frozen-lockfile`; pnpm hardlinks from its store, so it costs little disk). Vite serves only
  files inside the project root, and a symlinked `node_modules` resolves to the main checkout's
  path: 0.7.2's WebKit checks failed 118 scenarios on `403 Forbidden` for the fontsource fonts
  ("outside of Vite serving allow list", 14 Sep 2026), every page's console carrying the error
  while the panels that load no font passed. The wk-checks harness's own `node_modules` is
  plain Node and does not go through vite, which is why that one link is safe.
- The tag's body (= the draft's notes, step 2 below) says in one line what the patch fixes — for 0.7.1, that earlier copies
  carry an incomplete licence list. Users who downloaded before have a right to know.
- After it ships, master records the shipped commit: `git merge -s ours release/X.Y.Z`
  keeps the tag's commit in master's history without taking its tree (master already
  carries the fix). Tag locally, as for every release while the repo is private.

### The host moved on 30 Sep 2026 — what flipped, where

Written as "Going public later — two things flip in one commit" while the repo was private; the
move happened the day it went public (owner's ruling). Flipped, each in its own repo, in this
order: (1) 0.8.8–0.8.10 mirrored onto `rexenv/rexenv` (byte-identical, so a flipped `url` had
something to point at); (2) the tap — the cask's `url` and `SOURCE_REPO` in `update-cask.yml`
in ONE commit (the workflow greps the url for `SOURCE_REPO` and fails loudly if they drift), the
`repository_dispatch` trigger + a daily schedule on `update-cask.yml`, `install-scripts.yml`
and `notify-website.yml`, `REPO`/`$Releases` in `install.sh`/`install.ps1`; (3) `rexenv/runtimes`
— `TAP_REPO` in `publish-app-manifest.sh`; `rexenv/apt` — `TAP` in `build-site.sh`;
`rexenv/website` — `sync-release.mjs` and the docs' download links; (4) this repo —
`release.yml` drafts here, `release-published.yml` sends the dispatch, the check scripts read
the releases here. **The self-update descriptor did not move** (above). The cask's
`verified:` was once a third flip, dropped when brew 6.0.22 deprecated it; the About page's
Changelog link points at the website and never moved.

## Cutting a release (the pipeline — the only flow since 27 Sep 2026)

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
   - GitHub → Actions → **Release** → *Run workflow* → enter `<X.Y.Z>` and the notes
     (creates the annotated tag for you; token-pushed tags don't re-trigger the workflow).

   **The tag's body IS the public release notes** — the workflow puts it on the draft
   verbatim (`scripts/tag-notes.sh`), and the website's changelog sync reads it from there.
   So write it for a user: what changed for them, per OS where it differs. The `versions`
   job refuses a lightweight or subject-only tag before any runner builds. Until 28 Sep
   2026 the draft was created with a fixed maintainer warning as its body ("Draft until
   PUBLISH-TESTING §A passes …, a draft's assets 404 for everyone"), step 4 said only
   "Publish", and **0.8.8 and 0.8.9 went public with that warning as their release
   notes** — replaced by hand with their tag bodies the same day. The gate reminder now
   goes to the run's summary page, which is never published.
3. Wait for the draft release **here, on `rexenv/rexenv`** — `publish` drafts it only when all
   four lanes delivered (dmg + app.tar.gz, setup.exe + zip, amd64/arm64 deb + AppImage, every
   `.sha256` matching). Download the attached dmg and run `docs/PUBLISH-TESTING.md` **§A** on
   it (§A0 already ran in CI, per OS). Record the pass next to the dmg's sha256 in that doc.
4. **Publish** the release — its notes are already the tag's body; read them once as a
   user would. Publishing fires `release-published.yml` → the tap's `update-cask.yml` (the
   cask bumps from the published dmg's hash) and its installer test; a failed dispatch is a
   red run here, and the tap polls daily as the fallback.
5. Sanity check: `brew update && brew audit --cask --online rexenv/tap/rexenv`,
   or the full §D dry-run for a first-time setup.
6b. **`rexenv/apt` → Actions → "Publish apt repository"** (after the release is published here —
   a draft's assets are not public): one approval (`apt-signing`), then
   `./scripts/check-apt-repo.sh` — the signature by the repository's key, each `Packages` against
   `InRelease`, the newest version = the latest release here. Until it runs, `apt upgrade` offers nothing
   new; `install.sh` meanwhile installs the release's `.deb` directly (`docs/PLAN-apt-repo.md`).
6. `rexenv/runtimes` → "Publish app update manifest", dry-run then publish, **six times**
   (macOS, Windows, Linux deb/AppImage × x86_64/aarch64) — step 8 below; then
   `./scripts/check-app-manifest.sh` for each.

## One-time setup (required before the first automated release)

- **`TAP_TOKEN`** on `rexenv/rexenv` (Settings → Secrets → Actions): a fine-grained PAT with
  repository access to `rexenv/homebrew-tap` only and `Contents: read and write`. Since 30 Sep
  2026 it is used by `release-published.yml` (the dispatch that bumps the cask), which refuses
  with a sentence naming it when it is missing; the draft itself needs no secret.
- **The arm64 Linux lane** runs on `ubuntu-22.04-arm`, free on public repositories and a paid
  larger runner on private ones; if the first tag run leaves that lane queued, enable arm
  runners for the org or go public — the ruling does not allow shipping without it.
- **macOS minutes** are billed 10× on a private repo (~400 per release); the free tier is 2,000.
- **The repo is public (30 Sep 2026)** and the releases live here — the section above. The
  private-era note (the cask's `url` fetched with no auth, the tap's poller reading this repo
  cross-repo, "the dmg ships from the tap instead") described 0.1.0–0.8.10.
- **One secret, for one dispatch.** The draft here needs none; `TAP_TOKEN` exists so a
  publish here reaches the tap's cask bump. (While the draft landed on the tap the same token
  created it — the tap's own `GITHUB_TOKEN` could bump the cask but not read another repo.)

## Rules the pipeline encodes (don't undo them by hand)

- **A dispatch builds THE TAG, never the branch** (#765, 30 Sep 2026). `workflow_dispatch` runs
  on the default branch; when the tag already exists the `versions` job checks it out (and
  runs the version guard on its tree) and every lane checks out `env.TAG`. 0.8.11's third draft
  was built from master HEAD, three docs/workflow commits past the tag — `rex --version` on the
  VM read `21f8a1b`, the tag said `7d9ec97` — and the tag was moved onto the built commit (the
  source differed by a comment; the binaries had been gated). A tag-push run never had the
  problem: `github.ref` is the tag.

- **The release is born a draft.** §A is publish-blocking and human-only; publishing
  IS the sign-off. Never flip the workflow to publish directly.
- **The draft's body is only ever the tag's body.** It turns public the moment the draft
  is published, and nothing between the two replaces it. A maintainer instruction goes to
  `$GITHUB_STEP_SUMMARY`, never into `--notes` (0.8.8/0.8.9, step 2).
- **The cask hash comes from the published asset.** `update-cask.yml` downloads what
  users will download and hashes that. Hand-editing the cask from a local build's
  hash reintroduces the exact staleness bug the old staging copy had.
- **Prereleases don't touch the tap** (`if: !prerelease`) — the cask tracks stable, and
  the update descriptor refuses a non-three-segment version outright, so a prerelease can
  never be offered in-app either.
- **No release asset may end in `_universal.dmg` except the dmg.** `update-cask.yml`
  selects by that suffix with `head -1`; a second match would hash the wrong file into
  the cask, and every user's `brew install` would fail its checksum.
- **A release is not finished when it is published.** The descriptor in `rexenv/runtimes`
  is a second click, and until it happens no installed rexenv is offered anything.
  `scripts/check-app-manifest.sh` is what notices.
- **The cask declares `auto_updates true`, so a PLAIN `brew upgrade` skips rexenv** — that
  is what stops brew and the app from installing over each other. Three things to know
  before someone "fixes" this: **naming the cask overrides it** (`brew upgrade --cask
  rexenv` acts, because an explicit request is not the case `auto_updates` covers), so do
  `--greedy` and `reinstall`; all of them install whatever the user's **local tap
  checkout** names, so any of them can move someone BACKWARDS — measured 7 Sep 2026, the
  named form put 0.6.0 back over a self-updated 0.6.1 and called it an upgrade; and brew
  reads `CFBundleShortVersionString` out of the installed app instead of its own receipt,
  which is what makes `brew info --cask rexenv` stay honest after an in-app update.
  This bullet, the cask's comment and the tap README all said "with or without a cask
  named" until the release that tested it.
- **The signing key is pinned in THREE files and they must move together.**
  `RELEASE_PUBKEY` in `src-tauri/src/core/updates.rs`, and `EXPECTED_PUBKEY` in both
  `scripts/publish-manifest.sh` and `scripts/publish-app-manifest.sh` on `rexenv/runtimes`.
  Both publishers refuse to sign with a key the shipped app does not pin, because a wrong
  key signs perfectly well and publishes a document every install rejects **in silence** —
  no error, no log, users simply stop being offered anything. Rotation order: ship an app
  release carrying the new public half FIRST, then update the two publishers, then publish.
- **§A0's payload list lives in the workflow now.** When something new is compiled
  into the binary, add its per-slice check to the "§A0 artefact integrity" step in
  `release.yml` (and to `docs/PUBLISH-TESTING.md` §A0) in the same commit.
- Runner note: `macos-14` is arm64; the universal build cross-compiles the x86_64
  slice via the checked-in `rustup target add`. Private-repo macOS minutes bill at a
  10× multiplier — the verify + universal build takes tens of minutes per run.

## The app manifest's `minimumSystemVersion` (since 23 Sep 2026)

The signed app manifest carries `minimumSystemVersion`, and `core::app_update::offer_for`
refuses to offer a release to a host below it. **It must equal `tauri.conf.json`'s
`minimumSystemVersion` — 13.0 since the legacy tiers landed** (`docs/PLAN-macos-13-floor.md`
T4). A manifest still saying 15.0 would silently stop offering updates to every macOS 13/14
user the app now runs for; one saying less than 13.0 would offer a build no tier serves.
`scripts/check-app-manifest-test.sh`'s fixture says 15.0 because it tests the reader's
shape, not the shipped value — the value to publish is the conf's.

## The version catalog's `minMacos` (since 23 Sep 2026)

`publish-manifest.sh` (in runtimes) now writes `minMacos` on every `php` / `php-fpm` entry,
read with `vtool` off the artifact it just hashed, and refuses to publish a Mach-O whose floor
it cannot read. rexenv offers a macOS 13 / 14 host only an entry whose floor it meets, and an
entry WITHOUT the field is never offered to such a host (ledger #711). **Published as serial 7
on 23 Sep 2026**: every `php` / `php-fpm` entry carries `minMacos: "12.0"` (our own builds), so
a 13 / 14 host is offered the same PHP updates a 15 host is. Standard hosts saw no change.
