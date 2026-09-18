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
# remembered — and as of 21 Aug 2026 that includes the 🚫-premises clause, which
# had been deliberately exempted as "hand-curated" and was therefore the one part
# that drifted (it said five; there were twelve).
#
#   ./scripts/ledger-tally.sh           print the current line
#   ./scripts/ledger-tally.sh --check   fail if the file disagrees (verify.sh)
set -euo pipefail
cd "$(dirname "$0")/.."

# BYTES, not characters. In a UTF-8 locale Git Bash's grep does not match a
# pattern outside the BMP: on the Dell, 19 Sep 2026, `grep -c '| 🔨'` (U+1F528,
# four bytes) counted ZERO against a ledger the Mac counted 14 in, while the
# three-byte ✅ and ◐ counted correctly. Every pattern here is a fixed UTF-8
# sequence, which is exactly what a byte-oriented grep is for.
export LC_ALL=C

LEDGER=docs/CLAIM-LEDGER.md

# Verdicts are counted by their LEADING emoji in the verdict column — the same
# one-liner the file documents, so this can never drift from the stated method.
# `|| true` because `grep -c` EXITS 1 on zero matches, and under `set -e` that
# killed this script mid-assignment with an empty log and rc=1 — the Windows
# gate above failed for two minutes' worth of guessing before anyone could see
# which count had gone to zero. A zero is data here: the `SUM != ROWS` check
# below is what reports it, in a sentence.
count_verdict() { grep -c "| $1" "$LEDGER" || true; }

OK=$(count_verdict ✅)
HALF=$(count_verdict ◐)
OPEN=$(count_verdict 🔨)
NEVER=$(count_verdict 🚫)

# Distinct row numbers, not line count: the tally is a claim about ROWS.
ROWS=$(grep -o '^| [0-9]* |' "$LEDGER" | grep -o '[0-9]*' | sort -n | uniq | wc -l | tr -d ' ')

SUM=$(( OK + HALF + OPEN + NEVER ))

EXPECTED="**✅ $OK · ◐ $HALF · 🔨 $OPEN · 🚫 $NEVER** of $ROWS rows"

# The 🚫 PREMISES — rows whose verdict is ◐ or ✅ but which carry a 🚫 somewhere
# inside, because one leg of the claim is inherently unprovable. This clause used
# to be hand-curated and was explicitly exempted from this script ("deliberately
# outside the tally"), and on 21 Aug 2026 it said FIVE when there were TWELVE:
# the exemption was the whole reason it drifted. So it is computed too now. A
# number in a generated line that is not generated is the next stale number.
PREMISES=$(awk -F'|' '
  /🚫/ && /^\| [0-9]+ \|/ {
    row = $2; gsub(/ /, "", row);
    verdict = $(NF-1); sub(/^ +/, "", verdict);
    if (verdict !~ /^🚫/) print row
  }' "$LEDGER" | sort -n | sed 's/^/#/' | paste -sd, - | sed 's/,/, /g')
PREMISE_COUNT=$(printf '%s' "$PREMISES" | tr ',' '\n' | grep -c '#' || true)
EXPECTED_PREMISES="plus $PREMISE_COUNT 🚫 premises living inside ◐/✅ rows ($PREMISES)"

if [ "${1:-}" != "--check" ]; then
  echo "$EXPECTED, $EXPECTED_PREMISES."
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

if ! grep -qF "$EXPECTED_PREMISES" "$LEDGER"; then
  echo "ledger-tally: the 🚫-premises clause in $LEDGER is stale." >&2
  echo "" >&2
  echo "  computed: $EXPECTED_PREMISES" >&2
  echo "  in file:  $(grep -oE 'plus [0-9]+ 🚫 premises living inside ◐/✅ rows \([^)]*\)' "$LEDGER" || echo '(no premises clause found)')" >&2
  echo "" >&2
  echo "  A row that adds a 🚫 leg to an otherwise-proven claim belongs in that" >&2
  echo "  list; that is what the list is FOR. Paste the computed clause in." >&2
  exit 1
fi

echo "ledger-tally: $EXPECTED · $PREMISE_COUNT 🚫 premises (matches)"
