---
name: release
description: Cut a rexenv release — version bump in all four manifests, verify-full, the human gates (SMOKE-TEST, PUBLISH-TESTING §A0/§A), tag, draft release, tap bump. Use when asked to release, tag, bump the version, or prepare a dmg.
---

# release — the runbook is `docs/RELEASING.md`; this is the order and the gates

Read `docs/RELEASING.md` first: it has TWO flows (the interim private-repo flow
where the dmg ships from the tap, and the automated public one) and says which is
live today. Do not invent a third.

## Order

1. `./scripts/status.py` — nothing under *Release gates* that this release must
   close is still open without the owner knowing.
2. Reconcile first if TODO has ticked rows (`reconcile-todo`), so the release
   commit's docs are honest.
3. Bump the version in ALL FOUR manifests (the workflow refuses a mismatch):
   `src-tauri/tauri.conf.json`, `package.json`, `src-tauri/Cargo.toml`,
   `cli/Cargo.toml`. Then `./scripts/status.py --write`. One commit.
4. `./scripts/verify-full.sh > log 2>&1; echo exit=$?` — verify + sandbox tier +
   wk-checks. Green line only.
5. Build: `pnpm release:mac` (universal dmg). Regenerate `THIRD-PARTY-NOTICES.md`
   if pins moved.
6. **Human gates — the agent cannot run these; say so, list them:**
   `docs/SMOKE-TEST.md` on the built dmg (clean Mac), then
   `docs/PUBLISH-TESTING.md` §A0/§A before publishing. Both, in that order.
7. Tag the commit being SHIPPED — annotated, message says what changed since the
   last tag (`git log --oneline <last-tag>..HEAD` is the source).
8. Draft release → owner publishes → the tap's **Update cask** runs on that
   publish (a minute; Run workflow if not). Then `rexenv/runtimes` → "Publish app
   update manifest" and `scripts/check-app-manifest.sh`. Sanity:
   `brew audit --cask --online rexenv/tap/rexenv`.

## Do not

- Publish, push a tag, or bump the cask without the owner's explicit go — these
  are outward-facing and irreversible.
- Hand-edit the cask hash; the tap's workflow compares sha256 AND version.
- Skip the human gates because the machine tiers were green: the dmg's
  install path is the part no tier sees.
