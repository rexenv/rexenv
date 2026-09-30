---
name: verify
description: Run rexenv's pre-commit bar (scripts/verify.sh) the only way its verdict counts, read the result, and know what to do when it is red or flaky. Use before any commit touching src/, src-tauri/, cli/, or scripts/.
---

# verify — the ONE green that counts

`scripts/verify.sh` is the bar: lib tests (both crates) + example builds + clippy
`--all-targets` at zero + tsc + eslint + the generated-doc gates (`ledger-tally`,
`doc-counts`, `notices-check.py`, `status.py --check`) + the two cross-OS compile gates,
`windows-check` (cargo-xwin; needs `XWIN_ACCEPT_LICENSE=1`) and `linux-check` (Ubuntu
22.04 container; needs Docker running). Its own `verify: all green` line and exit code
are the verdict.

**Read the lines above the verdict too.** `verify: windows-check SKIPPED — …` or
`verify: linux-check SKIPPED — …` still ends green, but that OS was not compiled: the
report must say so, and a change touching `platform/` or an OS arm is not done until
the skipped gate has run somewhere. And both gates are COMPILE checks — a green one
says the OS builds, never that the feature works there (`docs/PLATFORMS.md` §5). An ad-hoc `cargo test && tsc` is never a gate: shell state resets
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
- **`core::php::tests::the_breaker_*` red, nothing else** — those two spawn a real
  spinner and judge it on CPU over a 4 s window, so a loaded machine starves them
  (PhpStorm indexing `src-tauri/target` at 700%, a release build, a VM). Before
  29 Sep 2026 their OWN leaked spinners were the load: `ps -eo pid,ppid,command |
  grep '[ ]yes$'` found three `yes` with ppid 1. Kill any such orphan, wait for
  `uptime` under ~7, re-run. Never loosen the window to make it pass.
- **A copy-guard / must-say test fails** — the UI copy and the facts it states
  drifted apart. Fix the copy or the guard, never delete the guard.
- Anything else: it is a real failure. Quote the output, fix, re-run. Do not
  `--no-verify` unless the user asked for a WIP commit.

After green: `./scripts/verify-receipt.sh check` should say the tree is covered.
Then commit — see `finish-task`.

## L2 is not in the bar

`verify.sh` only LOADS `scripts/wk-checks/*.js`; the scenarios run in `verify-full`. A change to a
component an L2 scenario renders (a site tab, a row, a card, a dialog) re-runs that scenario before
the commit: `npx vite --port 5199 --strictPort &` then `cd scripts/wk-checks && ONLY='^<scenario>'
node uireview.js` (or the file's own script). 30 Sep 2026: the stopped-stack gate (#752) silently
took the Adminer frame out of every `dbtab-*` scenario for ten days — found by 0.8.11's `verify-full`.
