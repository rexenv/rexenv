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

# The schema version: the highest `// vNN —` migration comment in db.rs, which is
# the same marker the migration list itself is written from.
SCHEMA=$(grep -oE '// v[0-9]+ —' src-tauri/src/state/db.rs | grep -oE '[0-9]+' | sort -n | tail -1)

# Tauri commands, per file and in total. `#[tauri::command]` is the one way in.
cmds_in() { grep -c '#\[tauri::command\]' "src-tauri/src/commands/$1.rs"; }
CMD_WORDPRESS=$(cmds_in wordpress)
CMD_REPO=$(cmds_in repo)

# The IPC bridge's exported functions — the UI's only invoke path.
IPC_EXPORTS=$(grep -cE '^export (async )?function ' src/lib/ipc/index.ts)

if [ "${1:-}" != "--check" ]; then
  echo "doc-counts (computed from the code):"
  note "SQLite schema version" "v$SCHEMA"
  note "commands/wordpress.rs commands" "$CMD_WORDPRESS"
  note "commands/repo.rs commands" "$CMD_REPO"
  note "src/lib/ipc/index.ts exports" "$IPC_EXPORTS"
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

expect docs/ARCHITECTURE.md "\`user_version\` migrations, currently $SCHEMA" "the schema version"
expect docs/MAP.md "App state (SQLite v1–v$SCHEMA, store)" "the schema version"
expect README.md "SQLite + migrations (v1–v$SCHEMA)" "the schema version"
expect docs/MAP.md "\`commands/wordpress.rs\` ($CMD_WORDPRESS cmds)" "the wordpress command count"
expect docs/MAP.md "\`commands/repo.rs\` ($CMD_REPO cmds" "the repo command count"
expect docs/MAP.md "the ONLY invoke path; $IPC_EXPORTS exports" "the IPC export count"

if [ "$fail" -ne 0 ]; then
  echo "" >&2
  echo "  These numbers are generated (scripts/doc-counts.sh) precisely so that" >&2
  echo "  updating them is part of the change that moved them, not a later" >&2
  echo "  errand nobody runs. Run ./scripts/doc-counts.sh and paste them in." >&2
  exit 1
fi

echo "doc-counts: schema v$SCHEMA · wordpress $CMD_WORDPRESS cmds · repo $CMD_REPO cmds · ipc $IPC_EXPORTS exports (all match)"
