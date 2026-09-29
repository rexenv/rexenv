# PLAN — an APT repository: `sudo apt-get install rexenv`

**Status: IN FLIGHT — 29 Sep 2026.** Owner: "linux er app ta apt-get diye jeno install kora jai
temon korte hobe". Rulings the same day (§2). Before 0.8.11: the repo is built first and 0.8.11 is
the first release the pipeline puts in it. **R1–R4 done the same day:** `rexenv/apt` live at
`https://rexenv.github.io/apt` (run 36590970354 — the first, 36588416009, failed its self-test:
the runner's `/home/runner` is 0750 and apt verifies as `_apt`, NO_PUBKEY; fixed in `07ea224`),
0.8.8–0.8.10 for amd64 + arm64, `scripts/check-apt-repo.sh` all green; the VM installed, upgraded
and refused a wrong key (SMOKE Linux); the tap's `install.sh` adds the repository (`f5d9dfe`).
**Owed:** R5's first release run (0.8.11), and the private key's backup in the owner's hands.

Until now the `.deb` was a file: `install.sh` downloaded it and ran `apt-get install ./rexenv.deb`,
and nothing verified it but the SHA-256 beside it (D-L6 of `docs/PLAN-linux-port.md`, "an apt
repository … later"). The one-command install plan left the repo out on purpose (`docs/archive/
PLAN-install-scripts.md` §5: "rexenv updates itself, and a repo is a signing key to guard"). An apt
repository adds what a file cannot: apt verifies the index's signature and every package's hash
against it, `apt-get install rexenv` works on a machine that has the source, and `apt upgrade`
moves rexenv with the rest of the system.

## 1. The shape

```
https://rexenv.github.io/apt/                 (Pages of the PUBLIC repo rexenv/apt)
  rexenv.gpg                                  the repo's public key, binary (for signed-by=)
  rexenv.asc                                  the same key, armored (for people who read it)
  dists/stable/{InRelease,Release,Release.gpg}
  dists/stable/main/binary-{amd64,arm64}/Packages{,.gz}
  pool/main/r/rexenv/rexenv_<v>_{amd64,arm64}.deb
  index.html                                  the four lines a user types
```

One suite (`stable`), one component (`main`), two architectures — what `release.yml` builds
(`rexenv_<v>_amd64.deb`, `rexenv_<v>_arm64.deb`). A user's line:

```
deb [signed-by=/etc/apt/keyrings/rexenv.gpg] https://rexenv.github.io/apt stable main
```

**The site is DERIVED, never accumulated.** Committing `.deb`s to a repository keeps every one in
git history forever (~30 MB a release) until GitHub's 1 GB Pages / repo limits bite. So nothing
binary is committed: the publisher downloads the debs of the newest `KEEP` (3) published tap
releases, verifies each against its `.sha256`, builds the indexes with `apt-ftparchive`, signs
`Release` (clearsigned `InRelease` + detached `Release.gpg`) and deploys the tree with
`actions/deploy-pages` (Pages source = GitHub Actions). Every run rebuilds the whole site from the
releases, so a failed run changes nothing and a re-run fixes anything.

## 2. Rulings (owner, 29 Sep 2026)

| # | Question | Ruling |
|---|---|---|
| A1 | Host | A new public repo `rexenv/apt`, GitHub Pages (free, no new infrastructure; a custom domain later if wanted). |
| A2 | The signing key | **Approval-gated**, like the app-update descriptors: the private key is a secret of an environment (`apt-signing`) with a required reviewer; each publish is one approval, beside the six descriptor approvals. |
| A3 | `install.sh` on Linux | **Adds the repository** (key + `sources.list.d/rexenv.list`) and runs `apt-get install rexenv`, so later updates arrive through `apt upgrade` too. No apt → the AppImage, as before. |
| A4 | Order | The repo first, then 0.8.11 — the first release the pipeline publishes into it. |

Not ruled, decided here: the key is **RSA 4096, no expiry** (apt on 22.04 verifies it; an expiring
key turns every install into "EXPKEYSIG" the day it lapses unless someone remembers to extend it —
rotation is the recovery for a lost key, §5). The in-app updater stays as it is: it installs the
same package through `dpkg -i`, and apt sees that version as installed — the two cannot disagree
about what is on the machine.

## 3. Steps

- **R1 — the repo.** `rexenv/apt` (public): `README.md`, `index.html`, `scripts/build-site.sh`
  (download → verify → `apt-ftparchive` → sign → the tree), `.github/workflows/publish.yml`
  (`workflow_dispatch`, environment `apt-signing`, `actions/deploy-pages`). Pages source: Actions.
- **R2 — the key.** Minted once (`gpg --batch --quick-gen-key`), the private half into the
  environment secret `APT_SIGNING_KEY` (required reviewer: the owner), the public half committed as
  `rexenv.asc` / `rexenv.gpg`. A backup of the private half is the owner's to keep; the agent's
  copy is deleted after the owner has it.
- **R3 — first publish.** Dispatch with the published 0.8.10 debs; verify on the Ubuntu VM:
  add the source → `apt-get update` (signature OK) → `apt-get install rexenv` → the app runs; and a
  tampered `Packages` refused.
- **R4 — `install.sh`.** In `rexenv/homebrew-tap`: the apt branch writes the key and the list and
  installs `rexenv` by name; the tap's `install-scripts.yml` runs it in fresh 22.04/24.04 containers.
- **R5 — the release.** `docs/RELEASING.md` step 8 gains the apt publish (dispatch → approve →
  `scripts/check-apt-repo.sh`, which reads `InRelease` and the `Packages` versions and says whether
  the repo names the tap's latest release); the release skill says so.
- **R6 — docs.** `docs/INSTALL.md` § Linux (the four lines, and the one command), `docs/PLATFORMS.md`
  §3 (installer row), SMOKE Linux (install by apt, `apt upgrade`), this file → `docs/archive/` when done.

## 4. What it does not do

- **No source packages, no PPA.** Launchpad builds from source on its own machines; rexenv's deb is
  a Tauri binary built on the CI runners with a pinned toolchain.
- **No old versions forever.** `KEEP` = 3; a machine pinned to an older one keeps it, it just cannot
  reinstall it from the repo.
- **Other distributions.** The deb is for Ubuntu 22.04+ (and what shares its libraries); the repo
  does not change that.

## 5. Key rotation (the recovery for a lost or leaked key)

A new key is minted, published beside the old one, and the next release's `install.sh` and the
app's docs point at it; existing machines keep verifying with the old key until they re-run the
one-command install (which rewrites `/etc/apt/keyrings/rexenv.gpg`). Nothing in the app pins this
key — the app-update descriptors have their own ed25519 key (ledger #348/#350), untouched by this.
