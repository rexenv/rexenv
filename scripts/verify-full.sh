#!/bin/bash
# The FULL gate (docs/PLAN-testing-strategy.md §6): the fast pre-commit bar,
# then the L1 sandbox tier, then the L2 WebKit harness. Composition, not
# bloat — verify.sh stays the per-commit bar; run THIS before a release or
# after touching a layer's subject (config generators, examples/common, the
# harness routes, wk-checks).
#
# Same discipline as verify.sh: every exit code is LOAD-BEARING, no pipes
# between a check and its verdict, and green comes ONLY from this script's
# own final line.
set -euo pipefail
cd "$(dirname "$0")/.."

bash scripts/verify.sh
bash scripts/live-checks.sh sandbox

# L2 needs the harness deps (a one-time `npm install` + `npx playwright
# install webkit` in scripts/wk-checks). Missing deps = NOT green: a gate
# that silently skips a layer stops being a gate.
if [ ! -d scripts/wk-checks/node_modules ]; then
  echo "verify-full: wk-checks deps missing — cd scripts/wk-checks && npm install && npx playwright install webkit" >&2
  exit 1
fi

VITE_LOG="$(mktemp -t rexenv-verify-vite)"
npx vite --port 5199 --strictPort >"$VITE_LOG" 2>&1 &
VITE_PID=$!
trap 'kill "$VITE_PID" 2>/dev/null || true' EXIT

up=0
for _ in $(seq 1 40); do
  if curl -fsS -o /dev/null http://localhost:5199/; then
    up=1
    break
  fi
  sleep 0.5
done
if [ "$up" -ne 1 ]; then
  echo "verify-full: vite never came up on :5199 (log: $VITE_LOG)" >&2
  exit 1
fi

(cd scripts/wk-checks && node run-all.js)

echo "verify-full: all green"
