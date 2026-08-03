#!/bin/bash
# The FULL gate (docs/TESTING.md §6): the fast pre-commit bar,
# then the L1 sandbox tier, then the L2 WebKit harness. Composition, not
# bloat — verify.sh stays the per-commit bar; run THIS before a release or
# after touching a layer's subject (config generators, examples/common, the
# harness routes, wk-checks).
#
# Same discipline as verify.sh: every exit code is LOAD-BEARING, no pipes
# between a check and its verdict, and green comes ONLY from this script's
# own final line.
set -euo pipefail

# ── The verdict must not be pipeable away ─────────────────────────────────────
# This script's exit code IS the verdict, and a pipe swallows it:
#   ./scripts/verify-full.sh ... | tail -3 && git commit
# runs the commit on `tail`'s status, not ours. That trap has been documented
# since 28 Jul 2026 ("verify-script-not-piped-checks") and was walked into again
# on 3 Aug by the person who documented it, landing a commit on a RED tier. So
# it is enforced here rather than remembered — the same reasoning as the
# unforgeable verdict line itself.
#
# A FILE redirect is fine (it keeps every line and the exit code) and so is a
# terminal. Only a PIPE is refused, because only a pipe both truncates the
# output and replaces the status.
#
# HONEST LIMIT — this closes one half, not both. It makes a piped run refuse to
# produce a verdict at all, so an `&& git commit` can never chain off a FALSE
# GREEN. It does NOT stop the chain: `script | tail && git commit` still reaches
# the commit, now after a loud refusal instead of a red verdict. Structurally
# binding the commit path needs a recorded-verdict receipt the hook checks
# (docs/TODO.md, "verdict receipt"); that is deliberately not bolted on mid-
# release.
if [ -p /dev/stdout ] && [ "${REXENV_ALLOW_PIPE:-0}" != "1" ]; then
  cat >&2 <<'PIPEMSG'
verify-full.sh: refusing to run with stdout piped.

  A pipe replaces this script's exit code with the last command's, so an
  `&& git commit` after it commits on a verdict that was never checked.

  Redirect to a file instead — it keeps everything, including the status:
      ./scripts/verify-full.sh ... > /tmp/out.log 2>&1; echo "exit=$?"
      tail -40 /tmp/out.log

  If you genuinely need a pipe and have handled the status yourself
  (`set -o pipefail`), re-run with REXENV_ALLOW_PIPE=1.
PIPEMSG
  exit 2
fi

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
