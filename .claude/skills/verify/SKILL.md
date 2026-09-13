---
name: verify
description: Run rexenv's pre-commit bar (scripts/verify.sh) the only way its verdict counts, read the result, and know what to do when it is red or flaky. Use before any commit touching src/, src-tauri/, cli/, or scripts/.
---

# verify — the ONE green that counts

`scripts/verify.sh` is the bar: lib tests (both crates) + example builds + clippy
`--all-targets` at zero + tsc + eslint + the generated-doc gates (`ledger-tally`,
`doc-counts`, `notices-check.py`, `status.py --check`). Its own `verify: all green` line and exit code
are the verdict. An ad-hoc `cargo test && tsc` is never a gate: shell state resets
between tool calls, so it can run from the wrong cwd and a `&&` chain passes on a
partial check — that is how a broken commit landed on 28 Jul 2026.

## Run it like this — redirect, never pipe

```
./scripts/verify.sh > "$SCRATCH/verify.log" 2>&1; echo "exit=$?"
tail -30 "$SCRATCH/verify.log"
```

The script REFUSES a piped stdout (a pipe replaces its exit code). It takes
minutes (cargo). Run it in the background and keep working on files OUTSIDE
`src/ src-tauri/src src-tauri/examples cli/src scripts` — those five paths are the
receipt's fingerprint, and touching them mid-run prints `NO RECEIPT`, which the
pre-commit hook then refuses. Docs and `.claude/` are outside the fingerprint.

## Reading a red

- **A generated-doc gate is red** — the fix is in the message: paste the numbers in
  (`doc-counts.sh`), regenerate (`status.py --write`), or fix the tally line. Never
  hand-edit a generated number.
- **`probe_version` / devtools timeouts at 10s** — usually `live-checks.sh` is
  running at the same time (Gatekeeper busy). Do not interleave; re-run after.
- **A copy-guard / must-say test fails** — the UI copy and the facts it states
  drifted apart. Fix the copy or the guard, never delete the guard.
- Anything else: it is a real failure. Quote the output, fix, re-run. Do not
  `--no-verify` unless the user asked for a WIP commit.

After green: `./scripts/verify-receipt.sh check` should say the tree is covered.
Then commit — see `finish-task`.
