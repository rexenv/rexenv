---
name: ledger-row
description: Add or update a docs/CLAIM-LEDGER.md row for an invariant comment ("never", "always", "the ONE place", a safety posture) — the row, its verdict, and the generated tally, in the same commit as the comment. Use whenever you write or change such a comment, or when a proof lands for an existing row.
---

# ledger-row — an invariant comment is not finished until its row exists

`docs/CLAIM-LEDGER.md` is the project's test metric: claims proven / claims
provable, never line coverage. A comment that asserts safety without a row is a
comment nobody will ever check; the project shipped one false safety comment that
way.

## The row

Table columns: `| # | where (file, fn, the doc §) | the claim, quoted or tightly paraphrased | verdict + proof name + what it does NOT certify |`.
Next `#` = last row + 1 (`grep -E '^\| [0-9]+ \|' docs/CLAIM-LEDGER.md | tail -1`).
Put it in the section for its subsystem (the file is grouped by blast radius, top
first).

Verdicts: ✅ proven (a NAMED lib test or example exercises exactly this claim) ·
◐ half-proven (say which half, and the layer for the other) · 🔨 provable-unproven
(name the layer: L0 lib test, L1 example vs real binary, L2 WebKit harness, L3
scripted manual) · 🚫 inherently unprovable (real second device / third-party
internals / human eye → maps to a SMOKE step or an accepted posture).

**The verdict names the OS.** rexenv is built for macOS, Windows and Linux
(`docs/PLATFORMS.md`). ✅ means proven on every OS the claim covers; proven on one
only is `◐ (macOS only)` / `◐ (Windows only)` / `◐ (Docker only)`, with the other
OS's run as a row in that OS's section of `docs/SMOKE-TEST.md`. A claim that is
OS-specific by nature says which OS in the "where" column (`platform/linux/…`).

Two rules with teeth:
- **Measure which layer can even see the subject before assigning a verdict.**
  Two L2 legs were planned for subjects Playwright cannot contain
  (`docs/archive/PLAN-webview-dialog-proofs.md`).
- **A ✅ names the plant.** Say how the test was proven load-bearing (what was
  broken to make it fail). A test that cannot fail proves nothing.

## The tally

```
./scripts/ledger-tally.sh --check
```
The summary line is generated; paste what the script prints, never hand-count.
`verify.sh` enforces it.

## Same commit

Comment + row + tally + (if it closes a TODO row) the tick — one commit. Point
the comment at the row: `(ledger #NNN)`.
