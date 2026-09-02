#!/bin/bash
# The counts the docs state about the CODE — computed, and checked against what
# the docs say.
#
# Why this exists, and why it is not six edits. On 21 Aug 2026 the first
# reconcile after v0.3.0 found six hand-written counts stale at once:
# ARCHITECTURE said migrations were "currently 33" (v37) and lib tests "539 and
# growing" (897), MAP and README both put the schema at "v1–v25", MAP said the
# IPC bridge had "217 exports" (233), `commands/wordpress.rs` "60 cmds" (61) and
# `commands/repo.rs` "24 cmds" (26). Every one of them was true when it was
# typed. None had been wrong in a way that broke anything — they were wrong in
# the way that costs a reader their trust in the rest of the file.
#
# `scripts/ledger-tally.sh` had already proved the shape that works on exactly
# this problem: the number is GENERATED and ENFORCED, so it cannot drift from
# the thing it counts. This is that, for the counts about the code.
#
# The rule this encodes: **a number in the docs is either derived or it is not a
# number.** A count that cannot be computed cheaply (how many tests pass) should
# not be written down at all — say where to get it instead. ARCHITECTURE's test
# count was changed that way rather than being added here.
#
#   ./scripts/doc-counts.sh           print what the code says
#   ./scripts/doc-counts.sh --check   fail if a doc disagrees (verify.sh)
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
note() { printf '  %-34s %s\n' "$1" "$2"; }

# ── computed from the code ────────────────────────────────────────────────────

# The schema version: the NUMBER OF ENTRIES in `MIGRATIONS`, because that is what
# decides it — `migrate_with` numbers each element by its INDEX and writes that
# to `user_version`. It used to be read off the highest `// vNN —` comment, which
# is prose, and prose drifted: the v38 grants work landed as TWO array elements
# under ONE `// v38` heading, so every doc said 38 while a real database was at
# 39, and this check could never see it — it was comparing prose to prose.
#
# The comments are still checked, against this count, below. A number derived
# from the thing that decides it beats a number derived from a description of it.
SCHEMA=$(awk '/^const MIGRATIONS/,/^\];/' src-tauri/src/state/db.rs | grep -cE '^    "')
SCHEMA_COMMENT=$(grep -oE '// v[0-9]+ —' src-tauri/src/state/db.rs | grep -oE '[0-9]+' | sort -n | tail -1)

# Tauri commands, per file and in total. `#[tauri::command]` is the one way in.
cmds_in() { grep -c '#\[tauri::command\]' "src-tauri/src/commands/$1.rs"; }
CMD_WORDPRESS=$(cmds_in wordpress)
CMD_REPO=$(cmds_in repo)

# The IPC bridge's exported functions — the UI's only invoke path.
IPC_EXPORTS=$(grep -cE '^export (async )?function ' src/lib/ipc/index.ts)

# `rex` commands: the dispatch arms in `cli_server.rs`, which is what decides
# what the CLI can ask for. `docs/CLI-ROADMAP.md` said "42 commands shipped"
# from 16 Jul while the tree had 53 — a status line nobody re-counts, in the one
# file a reader consults to know what exists.
# Command NAMES on dispatch arms, not arm LINES — and not the two-segment
# subset. Until 2 Sep 2026 this counted `^\s+"word.word" =>`, which silently
# missed every three-segment name (`wp.user.password`, `wp.plugin.install`, …)
# and every alternation arm (`"a" | "b" =>`): 56 counted where 87 commands
# answer. A generated number that measures a SUBSET is worse than a typed one,
# because nobody re-derives it — it was the count that gated the doc, so it
# read as authoritative for six weeks.
CLI_CMDS=$(grep -oE '^\s+"[a-z_.]+"( \| "[a-z_.]+")* =>' src-tauri/src/cli_server.rs \
  | grep -oE '"[a-z_.]+"' | wc -l | tr -d ' ')

if [ "${1:-}" != "--check" ]; then
  echo "doc-counts (computed from the code):"
  note "SQLite schema version" "v$SCHEMA"
  note "commands/wordpress.rs commands" "$CMD_WORDPRESS"
  note "commands/repo.rs commands" "$CMD_REPO"
  note "src/lib/ipc/index.ts exports" "$IPC_EXPORTS"
  note "rex commands (cli_server arms)" "$CLI_CMDS"
  exit 0
fi

# ── checked against the docs ──────────────────────────────────────────────────
#
# Each check names the file, what the doc must say, and how to fix it. A failure
# here is never "the docs are untidy" — it is "a reader following this file will
# be told something the code contradicts".
expect() { # <file> <literal the file must contain> <what it is>
  if ! grep -qF "$2" "$1"; then
    echo "doc-counts: $1 is stale about $3." >&2
    echo "  it must contain: $2" >&2
    fail=1
  fi
}

if [ "$SCHEMA_COMMENT" != "$SCHEMA" ]; then
  echo "doc-counts: MIGRATIONS has $SCHEMA entries but the highest '// vNN —' comment is v$SCHEMA_COMMENT." >&2
  echo "  The ENGINE numbers migrations by array index, so the real version is $SCHEMA." >&2
  echo "  A comment heading two array entries is the way this drifts — give each" >&2
  echo "  element its own '// vNN —' line, or merge them into one element (only safe" >&2
  echo "  BEFORE any database has run them: merging after the fact leaves existing" >&2
  echo "  installs ahead of fresh ones, and a later migration never runs on them)." >&2
  fail=1
fi

expect docs/ARCHITECTURE.md "\`user_version\` migrations, currently $SCHEMA" "the schema version"
expect docs/MAP.md "App state (SQLite v1–v$SCHEMA, store)" "the schema version"
expect README.md "SQLite + migrations (v1–v$SCHEMA)" "the schema version"
expect docs/MAP.md "\`commands/wordpress.rs\` ($CMD_WORDPRESS cmds)" "the wordpress command count"
expect docs/MAP.md "\`commands/repo.rs\` ($CMD_REPO cmds" "the repo command count"
expect docs/MAP.md "the ONLY invoke path; $IPC_EXPORTS exports" "the IPC export count"
expect docs/CLI-ROADMAP.md "$CLI_CMDS commands shipped" "the rex command count"

# ── every docs/*.md path the tree cites must EXIST ────────────────────────────
#
# The inverse of the checks above: not what the docs say about the code, but
# what everything says about the docs. Added 23 Aug 2026 with the sweep that
# repaired four code comments citing the TODO file for rows that had moved to
# the archive — and it immediately found one more the sweep was not looking for.
#
# **Stated limit, because it is the more common failure and this does not catch
# it:** a pointer rots most often when a ROW moves between files, not when a
# file disappears. `core/apache.rs` cited a "Deferred services" plan that had
# been archived; the path it named still existed, so nothing here would have
# fired. Verifying that needs anchors in the target, and an anchor convention is
# a bigger change than this problem has earned. What this covers is the
# rename/delete class, which is cheap and total.
#
# `docs/archive/` is excluded as a SOURCE of citations: those files record what
# was true when they were written, and editing history to satisfy a linter is
# the opposite of what an archive is for. They are still checked as TARGETS — a
# live file may point into the archive, and often should. `dist/` is excluded
# because it is BUILT: a stale bundle there would fail this check for a citation
# no longer in any source file.
#
# The leading boundary is load-bearing: without it a path in the SIBLING repo —
# `runtimes/docs/<file>`, which `docs/PLAN-adminer-updates.md` legitimately names
# — matches from its `docs/` onward and is reported as dangling. Qualifying such
# a path with its repo is the fix on the doc side; the boundary is the fix on
# the scanner side, and both were needed.
#
# Note what these comments do NOT contain: a literal path of the form this
# scanner matches. Writing the sibling-repo example out in full made THIS FILE a
# citation of a file that does not exist, and the scan reported itself — the
# scanner-counts-itself trap `core/binaries.rs` already carries a note about.
#
# The `|| true` is not defensive noise. Under `set -euo pipefail` a grep that
# matches NOTHING exits 1, which fails the pipeline, which fails the command
# substitution, which aborts the script — exit 1 with no output at all. That is
# the exact state the landmark below exists to report, and without this the
# landmark could never print: a broken scan would look like a failing check with
# no reason given. Found by planting it.
doc_refs() {
  { grep -rhoE '(^|[^A-Za-z0-9._/-])docs/[A-Za-z0-9._/-]+\.md' \
    --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.sh' \
    --include='*.js' --include='*.md' \
    --exclude-dir=node_modules --exclude-dir=target --exclude-dir=.git \
    --exclude-dir=archive --exclude-dir=dist \
    . || true; } | sed -E 's/^[^d]//' | sort -u
}
DOC_REFS=$(doc_refs)
DOC_REF_COUNT=$(printf '%s\n' "$DOC_REFS" | grep -c . || true)

# A landmark, for the reason ledger-tally and the WCAG scan carry one: a scan
# that matched nothing would report zero dangling pointers and look perfect.
# The floor is set just under the live count so it is a real tripwire and not a
# number that can never be reached — the docs directory alone carries most of it.
if [ "$DOC_REF_COUNT" -lt 25 ]; then
  echo "doc-counts: the doc-pointer scan found only $DOC_REF_COUNT paths (expected 25+)." >&2
  echo "  That is a broken scan reporting a clean tree, not a clean tree." >&2
  fail=1
fi

for ref in $DOC_REFS; do
  if [ ! -f "$ref" ]; then
    echo "doc-counts: $ref is cited in the tree but does not exist." >&2
    grep -rlF "$ref" \
      --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.sh' \
      --include='*.js' --include='*.md' \
      --exclude-dir=node_modules --exclude-dir=target --exclude-dir=.git \
      --exclude-dir=archive --exclude-dir=dist . 2>/dev/null | sed 's/^/    cited by /' >&2
    echo "  Repoint it, or qualify it if it names a path in ANOTHER repo" >&2
    echo "  (rexenv/runtimes and rexenv/homebrew-tap both have their own docs/)." >&2
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "" >&2
  echo "  These numbers are generated (scripts/doc-counts.sh) precisely so that" >&2
  echo "  updating them is part of the change that moved them, not a later" >&2
  echo "  errand nobody runs. Run ./scripts/doc-counts.sh and paste them in." >&2
  exit 1
fi

echo "doc-counts: schema v$SCHEMA · wordpress $CMD_WORDPRESS cmds · repo $CMD_REPO cmds · ipc $IPC_EXPORTS exports · rex $CLI_CMDS cmds · $DOC_REF_COUNT doc paths cited (all match, all exist)"
