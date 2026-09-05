---
name: reconcile-todo
description: Reconcile docs/TODO.md against the code — move shipped rows to the month's archive with the script, then do the judgement pass the script cannot (rows open only on paper, rows describing guards that no longer exist, stale PLAN headers). Use when TODO.md has ticked rows, after a release, or when asked to audit the docs.
---

# reconcile-todo — the mechanical half is a script; the judgement half is you

Three reconciles (21 Aug, 5 Sep 2026, …) found the same four shapes every time.
Do the script first, then hunt each shape deliberately.

## 1. Mechanical: move ticked rows

```
./scripts/todo-reconcile.py            # dry run: how many, from which sections
./scripts/todo-reconcile.py --apply    # moves column-0 [x] blocks → docs/archive/SHIPPED-<yyyy-mm>.md
./scripts/todo-reconcile.py --count
```

Nested `[x]` under an OPEN parent stays (it is that parent's evidence). A `###`
subsection whose rows all moved goes whole. Then read `docs/TODO.md` top to bottom
— orphaned prose (a paragraph that introduced rows now gone) moves or dies.

## 2. Judgement: the four shapes

1. **Open only on paper.** For each open row, grep the code for the thing it
   names (function, example, ledger #). If it exists, the row is done: tick it with
   the commit that did it (`git log -S`), then move it. Struck-through titles with
   an open box are this shape.
2. **A row describing a guard that does not exist.** The dangerous one. If a row
   says "X FAILS when…", open X and check it fails. If it prints a NOTE and passes
   green, the row is wrong — fix the row (or the guard) in this pass.
3. **Shipped narrative inside an open row.** A row is open for ONE reason. If it
   runs longer than a screen with ✓ paragraphs, rewrite it to the open reason in
   the first line and move the original text to the archive under a "Judgement
   moves" heading.
4. **A parent whose children all closed.** Tick and move it.

Then the PLAN headers: `./scripts/status.py` prints each in-flight `docs/PLAN-*.md`'s
Status line — any that says "planned / not started / next: …" about a thing the
code has needs its header corrected (and, when fully shipped, the file moved to
`docs/archive/` with citations repointed: `grep -rl 'docs/PLAN-<name>.md'`, then
`doc-counts.sh --check` proves no pointer dangles).

## 3. Record and regenerate

- Rewrite the reconcile paragraph at the top of `docs/TODO.md`: date, HEAD, what
  this pass found per shape (counts, not adjectives).
- Add the new archive file to `docs/archive/README.md`'s table and CLAUDE.md's
  router row for `docs/TODO.md`.
- `./scripts/status.py --write`; `./scripts/doc-counts.sh --check`.
- Commit as `docs(todo): reconcile <date> — …`. `scripts/` untouched → no verify
  needed; if you changed a script, run `verify`.
