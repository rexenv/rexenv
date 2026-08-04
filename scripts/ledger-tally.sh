#!/bin/bash
# The CLAIM-LEDGER tally, computed — and checked against what the file claims.
#
# Why this is a script and not a habit. The ledger's own maintenance rule says a
# claim isn't finished until its row lands in the SAME commit, and the rows have
# obeyed it. The SUMMARY hasn't: on 4 Aug 2026 it still read the 3 Aug figures
# (139/39/37/5 of 220) while #221–#227 had already landed — seven rows and one
# ◐→✅ upgrade, inside a single day. The summary is the one part of a
# 500-line file anyone reads at a glance, so a stale summary misinforms more
# people than a stale row would.
#
# The file already told everyone to recompute mechanically. That instruction was
# followed for the rows and skipped for the line, which is the project's own
# finding about itself: unguarded prose rots, guarded prose does not. So the
# number is now GENERATED here and ENFORCED by verify.sh, rather than typed and
# remembered.
#
#   ./scripts/ledger-tally.sh           print the current line
#   ./scripts/ledger-tally.sh --check   fail if the file disagrees (verify.sh)
set -euo pipefail
cd "$(dirname "$0")/.."

LEDGER=docs/CLAIM-LEDGER.md

# Verdicts are counted by their LEADING emoji in the verdict column — the same
# one-liner the file documents, so this can never drift from the stated method.
count_verdict() { grep -c "| $1" "$LEDGER"; }

OK=$(count_verdict ✅)
HALF=$(count_verdict ◐)
OPEN=$(count_verdict 🔨)
NEVER=$(count_verdict 🚫)

# Distinct row numbers, not line count: the tally is a claim about ROWS.
ROWS=$(grep -o '^| [0-9]* |' "$LEDGER" | grep -o '[0-9]*' | sort -n | uniq | wc -l | tr -d ' ')

SUM=$(( OK + HALF + OPEN + NEVER ))

EXPECTED="**✅ $OK · ◐ $HALF · 🔨 $OPEN · 🚫 $NEVER** of $ROWS rows"

if [ "${1:-}" != "--check" ]; then
  echo "$EXPECTED"
  exit 0
fi

# A row carries exactly ONE leading verdict. If the counts don't add up to the
# row count, the tally is arithmetically fine and MEANINGLESS — some row has no
# verdict emoji, or two, and the totals silently stop describing the file. Catch
# that here rather than publishing a number that no longer counts what it says.
if [ "$SUM" -ne "$ROWS" ]; then
  echo "ledger-tally: verdicts ($SUM) != rows ($ROWS)." >&2
  echo "  Some row has no leading verdict emoji, or more than one. The tally" >&2
  echo "  would still add up while no longer counting what it claims to." >&2
  exit 1
fi

if ! grep -qF "$EXPECTED" "$LEDGER"; then
  echo "ledger-tally: the stated tally in $LEDGER is stale." >&2
  echo "" >&2
  echo "  computed: $EXPECTED" >&2
  echo "  in file:  $(grep -oE '\*\*✅ [0-9]+ · ◐ [0-9]+ · 🔨 [0-9]+ · 🚫 [0-9]+\*\* of [0-9]+ rows' "$LEDGER" || echo '(no tally line found)')" >&2
  echo "" >&2
  echo "  Paste the computed line in. A ledger row and its tally belong in the" >&2
  echo "  same commit, for the same reason the row and its comment do." >&2
  exit 1
fi

echo "ledger-tally: $EXPECTED (matches)"
