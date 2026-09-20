---
name: finish-task
description: Close a task in rexenv the way this project requires — docs updated in the same commit, TODO ticked with evidence, ledger row for any new invariant, STATUS regenerated, verify green, explicit staging, one commit. Use every time a piece of work is done and about to be committed.
---

# finish-task — nothing is done until the docs and the tick land WITH it

This project has paid twice for "update the docs later" (a doc asserting a guard
that no longer existed; a tally stale within a day). The commit that does the
work carries everything below, or it is a commit plus an untracked debt.

## 1. Walk the docs table (CLAUDE.md, "Docs ship WITH the code")

For every kind of thing the change touched, update the named doc NOW:
user-visible behaviour → `docs/ARCHITECTURE.md`; new/renamed module or entry
point → `docs/MAP.md` + README tree; invariant comment ("never/always/the ONE
place") → `docs/CLAIM-LEDGER.md` row + verdict (`ledger-row` skill); a probe,
example or tier → `docs/TESTING.md` + `scripts/live-checks.sh`; a hand-tested
flow → `docs/SMOKE-TEST.md` / `docs/PUBLISH-TESTING.md`; port / pin / checksum →
`docs/PORTS.md`; install or first-run prompt → `docs/INSTALL.md`; a `rex` command
→ `docs/CLI-ROADMAP.md`; a design token or honest-UI rule → `docs/DESIGN.md`.

A doc that is now WRONG outranks one that is merely incomplete: if the fix closes
a gap some doc lists as open, correct that entry. Say what it cost, not just what
it does — the failure that motivated a rule is the half that survives.

## 1b. Both platforms — did this ship for Windows too?

macOS and Windows are both shipping targets (CLAUDE.md, "Both platforms"). Before
the commit, answer these out loud:

- Did anything OS-shaped get spelled INLINE — a path, a URL or origin, a command
  line, a refusal, a user-facing sentence? It belongs in `platform/words.rs` or a
  `platform/traits.rs` capability, with BOTH answers filled in. (`rexdb://localhost`
  hardcoded for macOS is how the Windows Database Browser shipped blank, #703.)
- Does `core/` name an OS? It may not — ask for a capability instead (#698).
- Is a policy enforced at two ends (a CSP, a header, a URL shape)? Check every end,
  not the one you changed (#699 + #702 + #703 were all needed for ONE panel).
- Is the claim proven on Windows, or only compiled there? `windows-check` is a
  compile gate. If there is no Windows run, the ledger verdict is `◐` and
  `docs/SMOKE-TEST.md`'s Windows section carries the row —
  `docs/TESTING.md` §"Proving a Windows claim" says how to get a real one.

## 2. Tick the TODO row, in this commit

`- [x] **title** ✓ <d Mon yyyy> — <one line of evidence: test/example/ledger #>`.
If the work landed but the row is not in `docs/TODO.md`, add the row ticked — a
reconcile later should find nothing "open only on paper". A parent row closes
when its last child does (that was missed once; check).

## 3. Regenerate what is generated

```
./scripts/status.py --write        # docs/STATUS.md
./scripts/ledger-tally.sh --check  # if the ledger changed
./scripts/doc-counts.sh --check    # if a command/export/migration count moved
```

## 4. Verify (code or scripts touched)

`verify` skill. Green verdict = the script's own line; receipt must cover the tree.

## 5. Stage EXPLICITLY, then commit

```
git add <each file you changed>      # NEVER git add -A / git add .
git status --short                   # nothing of the owner's IDE edits swept in
git commit -m "<type>(<scope>): <what changed, in one honest line>"
```

The owner edits in the IDE while the agent works; `-A` once swept their
uncommitted docs into an agent commit. Commit as the git config identity — never
pass `-c user.email`. Types in use: `feat`, `fix`, `test`, `docs`, `refactor`.
One task = one commit; do not batch.

## 6. Report

Say what shipped, what proves it (test / example / ledger #), and what is still
owed by hand (the row keeps saying so). If verify was not run, say that.
