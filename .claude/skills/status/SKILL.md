---
name: status
description: What is pending in rexenv right now, and what state the project is in — generated from the tree, never from memory. Use at session start, when asked "what's open / ki baki / obostha ki", or before picking up work.
---

# status — the project's state, derived

Run it. Do not answer from memory or from a doc's prose — both have been stale
within a day of being written (see the header of `docs/TODO.md`).

```
./scripts/status.py
```

It prints, from the tree: HEAD / last tag / commits since; every open row of
`docs/TODO.md` by section with its line number; the claim-ledger tally; each live
each in-flight `docs/PLAN-*.md`'s own Status line; the counts the docs must agree with.

The same report is committed as `docs/STATUS.md` (tree-derived lines only) and
`verify.sh` + the pre-commit hook fail when it drifts — so it is safe to read the
committed file when you cannot run the script.

## Reading it

- **"Now"** rows are code/test work anyone can pick up. **"Release gates"** need a
  human, a clean Mac, or a network. **"Parked"** needs an explicit go from the
  owner — never pick one up silently. **"Blocked"** needs external work.
- The row title is the first bold phrase; the reason it is open is in its first
  sentence at `docs/TODO.md:<line>`. Read the row before starting — several rows
  record a wrong premise that was found on the way.
- Open rows + `docs/CLAIM-LEDGER.md`'s 🔨 rows together are the whole backlog.

## When a row is done

Tick it in the SAME commit as the work (`- [x] … ✓ <date> — <evidence>`), then
`./scripts/status.py --write` and stage `docs/STATUS.md`. The `finish-task`
skill walks the full pre-commit list.
