---
name: release
description: Cut a rexenv release — version bump in all four manifests, verify-full, tag, the CI build of macOS + Windows + Linux from one tag, the per-OS human gates (SMOKE-TEST sections, PUBLISH-TESTING §A0/§A), draft, tap bump, update descriptors. Use when asked to release, tag, bump the version, or prepare a dmg / setup.exe / deb.
---

# release — the runbook is `docs/RELEASING.md`; this is the order and the gates

Read `docs/RELEASING.md` first. **Since 27 Sep 2026 every release is built ONLY on
GitHub Actions, every OS from one tag** (owner ruling, `docs/PLAN-ci-release.md`):
macOS + Windows + Linux (x86_64 and aarch64) → one DRAFT on `rexenv/homebrew-tap`
with every asset, or nothing. Never build a release artefact on a device; a local
`pnpm release:mac|win|linux` is a proof run, not a release. Do not invent another flow.
While the interim note at the top of RELEASING.md stands, the workflow runs from
`rexenv/runtimes` — read which is live before triggering.

## Order

1. `./scripts/status.py` — nothing under *Release gates* that this release must
   close is still open without the owner knowing — for ANY of the three OSes.
2. Reconcile first if TODO has ticked rows (`reconcile-todo`), so the release
   commit's docs are honest.
3. Bump the version in ALL FOUR manifests (the workflow refuses a mismatch):
   `src-tauri/tauri.conf.json`, `package.json`, `src-tauri/Cargo.toml`,
   `cli/Cargo.toml`. Then `./scripts/status.py --write`. One commit.
4. `./scripts/verify-full.sh > log 2>&1; echo exit=$?` — verify + sandbox tier +
   wk-checks. Green line only, and read whether `windows-check` / `linux-check`
   SKIPPED: a release must not be the first time an OS is compiled.
5. `THIRD-PARTY-NOTICES.md`'s crate and npm tables are checked by verify
   (`notices-check.py`, ledger #592, the macOS, Windows and Linux graphs); the
   downloaded-binary sections are not — update those by hand if pins moved on any OS.
6. Tag the commit being SHIPPED — annotated, message says what changed since the
   last tag (`git log --oneline <last-tag>..HEAD` is the source). The owner pushes /
   triggers — see "Do not".
7. CI builds every lane and runs §A0 per OS (`release-windows-check.sh`,
   `release-linux-check.sh`, the macOS per-slice checks). A lane that fails means no
   draft — fix and re-tag per RELEASING.md, never ship a partial set.
8. **Human gates, per OS — the agent cannot run these; say so, list them:**
   - macOS: `docs/SMOKE-TEST.md` main body on the drafted dmg (clean Mac), then
     `docs/PUBLISH-TESTING.md` §A0/§A.
   - Windows: `docs/SMOKE-TEST.md` § Windows on the drafted `setup.exe`.
   - Linux: `docs/SMOKE-TEST.md` § Linux on the drafted deb / AppImage (P1 first).
9. Owner publishes the draft → the tap's **Update cask** runs on that publish. Then
   `rexenv/runtimes` → "Publish app update manifest" once per descriptor (macOS,
   Windows, Linux deb/AppImage × x86_64/aarch64) and `scripts/check-app-manifest.sh`
   for each (`--linux` for the Linux ones). Sanity:
   `brew audit --cask --online rexenv/tap/rexenv`.

## Do not

- Publish, push a tag, or bump the cask without the owner's explicit go — these
  are outward-facing and irreversible.
- Hand-edit the cask hash; the tap's workflow compares sha256 AND version.
- Skip an OS's human gate because the machine tiers were green: each installer's
  install path is the part no tier sees, and each OS's is different.
