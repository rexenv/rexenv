#!/bin/bash
# The pre-commit bar: lib tests + example builds + frontend typecheck.
#
# Every check's exit code is LOAD-BEARING. No pipes, no grep filters, no
# `cmd | tail` here — a `tsc | head; echo $?` pipeline once reported head's
# exit status and let a failing typecheck through to a commit (28 Jul 2026).
# Filter output when reading it; never between a check and its verdict.
set -euo pipefail

# ── The verdict must not be pipeable away ─────────────────────────────────────
# This script's exit code IS the verdict, and a pipe swallows it:
#   ./scripts/verify.sh ... | tail -3 && git commit
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
verify.sh: refusing to run with stdout piped.

  A pipe replaces this script's exit code with the last command's, so an
  `&& git commit` after it commits on a verdict that was never checked.

  Redirect to a file instead — it keeps everything, including the status:
      ./scripts/verify.sh ... > /tmp/out.log 2>&1; echo "exit=$?"
      tail -40 /tmp/out.log

  If you genuinely need a pipe and have handled the status yourself
  (`set -o pipefail`), re-run with REXENV_ALLOW_PIPE=1.
PIPEMSG
  exit 2
fi

cd "$(dirname "$0")/.."

(cd src-tauri && cargo test --lib)
(cd src-tauri && cargo build --examples)
# Zero-warning baseline established 28 Jul 2026 — a bar that ships with known
# warnings trains people to ignore it. Pre-existing 8-arg fns carry explicit,
# reasoned allows; new warnings fail the build.
(cd src-tauri && cargo clippy --lib -- -D warnings)
npx tsc --noEmit
# The ledger's tally is a claim about the ledger, so it is checked like one.
# It went stale within a day of being typed (4 Aug 2026) while every ROW obeyed
# the same-commit rule — this project's own finding is that unguarded prose rots
# and guarded prose doesn't, so the number is generated and enforced rather than
# remembered.
./scripts/ledger-tally.sh --check

echo "verify: all green"
