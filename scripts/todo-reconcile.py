#!/usr/bin/env python3
"""Move ticked blocks out of docs/TODO.md into the month's SHIPPED archive.

Why a script. docs/TODO.md's own rule is "ONLY open work lives here", and the
rule has now failed three times by hand: 21 Aug 2026 found 57 finished blocks
sitting among 50 open rows; 5 Sep 2026 found 96 more (173 ticked boxes against
46 open). Each time the reconcile was a one-off editing session that nobody
could repeat, so the drift restarted the moment it ended. This is the repeatable
half: the mechanical move. The judgement half (is a row open only on paper? does
a doc still describe a guard that no longer exists?) stays with the reader —
`.claude/skills/reconcile-todo/SKILL.md` walks it.

What counts as a block: a column-0 `- [x]` item and every line under it until the
next column-0 item, heading, or column-0 prose paragraph. Nested `[x]` under an
OPEN parent stays — it is that parent's evidence. A `###` subsection whose items
all moved is moved whole (heading + prose), so the archive keeps the grouping.

    ./scripts/todo-reconcile.py            dry run: what would move, and where
    ./scripts/todo-reconcile.py --apply    do it (TODO.md rewritten, archive appended)
    ./scripts/todo-reconcile.py --count    one line: open / ticked column-0 rows
"""
import datetime
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TODO = ROOT / "docs" / "TODO.md"
ARCHIVE_DIR = ROOT / "docs" / "archive"

ITEM = re.compile(r"^- \[( |x)\] ")
H2 = re.compile(r"^## ")
H3 = re.compile(r"^### ")


def parse(lines):
    """Yield (kind, section, sub, lines). kind ∈ {h2, h3, prose, open, done}."""
    section = sub = None
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        if H2.match(line):
            section, sub = line.rstrip("\n"), None
            yield ("h2", section, sub, [line])
            i += 1
        elif H3.match(line):
            sub = line.rstrip("\n")
            yield ("h3", section, sub, [line])
            i += 1
        elif ITEM.match(line):
            kind = "done" if ITEM.match(line).group(1) == "x" else "open"
            block = [line]
            i += 1
            while i < n:
                nxt = lines[i]
                if H2.match(nxt) or H3.match(nxt) or ITEM.match(nxt):
                    break
                if nxt.strip() == "":
                    # a blank line ends the item only if column-0 prose follows
                    j = i + 1
                    while j < n and lines[j].strip() == "":
                        j += 1
                    if j < n and not lines[j].startswith((" ", "\t")) and not ITEM.match(lines[j]) \
                            and not H2.match(lines[j]) and not H3.match(lines[j]):
                        break
                    block.append(nxt)
                    i += 1
                    continue
                if not nxt.startswith((" ", "\t")):
                    break
                block.append(nxt)
                i += 1
            yield (kind, section, sub, block)
        else:
            block = [line]
            i += 1
            while i < n and not (H2.match(lines[i]) or H3.match(lines[i]) or ITEM.match(lines[i])):
                block.append(lines[i])
                i += 1
            yield ("prose", section, sub, block)


def main(argv):
    apply = "--apply" in argv
    count = "--count" in argv
    lines = TODO.read_text().splitlines(keepends=True)
    nodes = list(parse(lines))

    if count:
        o = sum(1 for k, *_ in nodes if k == "open")
        d = sum(1 for k, *_ in nodes if k == "done")
        print(f"todo: {o} open · {d} ticked (column-0 rows)")
        return 0

    # A ### subsection moves whole when every item under it is done.
    sub_state = {}
    for kind, sec, sub, _ in nodes:
        if sub is None or kind not in ("open", "done"):
            continue
        sub_state.setdefault((sec, sub), set()).add(kind)
    movable_subs = {k for k, v in sub_state.items() if v == {"done"}}

    keep, moved = [], []  # moved: (section, sub, lines)
    for kind, sec, sub, block in nodes:
        whole = sub is not None and (sec, sub) in movable_subs
        if kind == "done" or (whole and kind in ("h3", "prose")):
            moved.append((sec, sub, block))
        else:
            keep.append(block)

    if not moved:
        print("todo-reconcile: nothing ticked at column 0 — TODO.md is already open-only.")
        return 0

    today = datetime.date.today()
    archive = ARCHIVE_DIR / f"SHIPPED-{today:%Y-%m}.md"
    by_section = {}
    for sec, sub, block in moved:
        by_section.setdefault(sec, []).append(block)

    print(f"todo-reconcile: {sum(1 for s, _, b in moved if ITEM.match(b[0]))} ticked blocks "
          f"→ {archive.relative_to(ROOT)}")
    for sec, blocks in by_section.items():
        items = sum(1 for b in blocks if ITEM.match(b[0]))
        print(f"  {items:3d}  {sec}")
    if not apply:
        print("dry run — pass --apply to move them.")
        return 0

    out = []
    if not archive.exists():
        out.append(
            f"# Shipped — {today:%B %Y}\n\n"
            "⚠️ **Historical.** Completed rows moved out of `docs/TODO.md` by "
            "`scripts/todo-reconcile.py`, with their ✓ evidence as written at the time. "
            "May contradict current code — the live description is `docs/ARCHITECTURE.md`.\n"
        )
    out.append(f"\n## Reconcile of {today:%-d %b %Y}\n")
    for sec, blocks in by_section.items():
        title = sec[3:] if sec else "(no section)"
        out.append(f"\n### From “{title}”\n\n")
        for b in blocks:
            text = "".join(b)
            if not text.endswith("\n"):
                text += "\n"
            out.append(text)
            if not text.endswith("\n\n"):
                out.append("\n")
    with archive.open("a") as f:
        f.write("".join(out))

    # collapse runs of >2 blank lines left behind by the removals
    text = "".join("".join(b) for b in keep)
    text = re.sub(r"\n{3,}", "\n\n", text)
    TODO.write_text(text)
    print(f"applied: {TODO.relative_to(ROOT)} rewritten, {archive.relative_to(ROOT)} appended.")
    print("Now READ the result: the header's reconcile date, orphaned prose, and")
    print("open rows whose work already landed (open only on paper).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
