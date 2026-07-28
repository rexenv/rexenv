#!/bin/bash
# The pre-commit bar: lib tests + example builds + frontend typecheck.
#
# Every check's exit code is LOAD-BEARING. No pipes, no grep filters, no
# `cmd | tail` here — a `tsc | head; echo $?` pipeline once reported head's
# exit status and let a failing typecheck through to a commit (28 Jul 2026).
# Filter output when reading it; never between a check and its verdict.
set -euo pipefail
cd "$(dirname "$0")/.."

(cd src-tauri && cargo test --lib)
(cd src-tauri && cargo build --examples)
npx tsc --noEmit

echo "verify: all green"
