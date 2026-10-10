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

# The tree as it is NOW, before a single check runs. Compared again at the end:
# a file edited while the bar was running means the thing that passed is not the
# thing on disk, and certifying it would be exactly the false green this whole
# mechanism exists to stop. (`|| true` — a clone without git still verifies, it
# just gets no receipt.)
TREE_BEFORE="$(./scripts/verify-receipt.sh fingerprint 2>/dev/null || true)"

# THIS host, in the same vocabulary live-checks.sh uses (its os column). Git Bash on a
# Windows host reports MINGW64_NT-… — W12 runs this script there, so that spelling is
# the point rather than an afterthought.
case "$(uname -s)" in
  Darwin) HOST_OS=macos ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT) HOST_OS=windows ;;
  *) HOST_OS=other ;;
esac

# The lib test binary needs an application manifest ON WINDOWS, and only there.
#
# Measured on the Dell 17 Sep 2026, the first time these tests ran on a Windows host:
# the binary linked and then died at load with STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139)
# before one test ran. The missing entry point is comctl32's `TaskDialogIndirect`, which
# exists only in Common-Controls v6 — the side-by-side assembly a binary must ASK for.
# rfd (under tauri-plugin-dialog) imports it; Tauri embeds that dependency in the APP
# exe's manifest, and a `cargo test` binary carries no manifest at all (measured: zero
# manifest resources), so it binds the System32 v5.82 stub, which lacks the export.
#
# Why here and not in `build.rs`, which is where a reader would look first: a build
# script's `rustc-link-arg` reaches EVERY artifact, and the app binary then dies with
# LNK1123 against Tauri's own manifest resource (measured both ways: red with the arg,
# `rexenv.exe` built clean the moment it was removed). The `-tests`/`-bins` suffixes do
# not help — cargo refuses `-tests` by name, because this crate's tests live inside the
# lib and it has no `[[test]]` target. Scoping the flags to the test INVOCATION is what
# reaches that one binary and nothing else; proven on the Dell, same run: 1232 tests
# executed, and `cargo build --bin rexenv` stayed green beside it.
WIN_TEST_FLAGS=()
if [ "$HOST_OS" = windows ]; then
  # A WINDOWS path, not the POSIX one Git Bash's `pwd` prints. `mt.exe` reads a
  # `/c/Users/…` argument as an OPTION and refuses it — measured on the Dell:
  # `mt : command line error c1010007: Unexpected/Unknown option "/c/Users/…"`,
  # then LNK1327, on the first build script cargo linked.
  WIN_MANIFEST="$(cygpath -m "$(pwd)/src-tauri/windows-test.manifest" 2>/dev/null || echo "$(pwd -W)/src-tauri/windows-test.manifest")"
  WIN_TEST_FLAGS=(--config "target.x86_64-pc-windows-msvc.rustflags=[\"-Clink-arg=/MANIFEST:EMBED\",\"-Clink-arg=/MANIFESTINPUT:$WIN_MANIFEST\"]")
fi

# Room first: an ENOSPC mid-link is a state of the machine, not a verdict (disk-guard.sh).
bash scripts/disk-guard.sh selftest
bash scripts/disk-guard.sh before
(cd src-tauri && cargo test --lib ${WIN_TEST_FLAGS[@]+"${WIN_TEST_FLAGS[@]}"})
# The `cli` crate ships its own binary and had NO tests until 12 Aug 2026, so
# the bar never entered this directory — and the first bug it grew (`rex
# --version` hanging forever against an app that accepts and never answers,
# §D) lives entirely here. A gate that skips a shipped crate is not a gate.
(cd cli && cargo test)
(cd src-tauri && cargo build --examples)
# Zero-warning baseline established 28 Jul 2026 — a bar that ships with known
# warnings trains people to ignore it. Pre-existing 8-arg fns carry explicit,
# reasoned allows; new warnings fail the build.
#
# `--all-targets`, since 21 Aug 2026, and the reason is the same one that added
# the `cli` crate's tests on 12 Aug: a gate that skips a shipped target is not a
# gate. `--lib` covered the library and nothing else — not `main.rs`, not the 134
# files in `examples/`, not `#[cfg(test)]` code — and the examples are precisely
# where that week's audit found 22 defects. Clearing the backlog it exposed took
# ~48 fixes; four deliberate cases carry an explicit `#[allow]` WITH its reason
# (a test name whose capitals are load-bearing, two `items_after_test_module`
# where hoisting production code would make the diff unreviewable, a fixture
# tuple, and the tunnel `Child`s whose reaping belongs to a Drop guard).
(cd src-tauri && cargo clippy --all-targets -- -D warnings)
(cd cli && cargo clippy --all-targets -- -D warnings)
npx tsc --noEmit
# The frontend's OTHER half. `tsc` proves types; it says nothing about the rule
# that decides whether a component renders at all.
#
# Adopted 24 Aug 2026 (ledger #396) after the rule was measured rather than
# argued about: run over all 72 `src` files it found three deliberate
# dependency lists and three suppressions that suppressed nothing — and,
# decisively, `rules-of-hooks` catches the crash fixed in `e9fc144` (a `useQuery`
# after an early return) which reached a user-visible failure on `/sites/:id` and
# was found by a WebKit render check written for something else. Proven by
# reverting that commit and watching this line go red.
#
# Two rules, not a style linter. A general ruleset over a codebase that has never
# run one produces hundreds of findings, and a gate nobody can get to zero
# becomes `--max-warnings 999`. See `eslint.config.js` for why each rule is on.
npx eslint "src/**/*.{ts,tsx}"
# The ledger's tally is a claim about the ledger, so it is checked like one.
# It went stale within a day of being typed (4 Aug 2026) while every ROW obeyed
# the same-commit rule — this project's own finding is that unguarded prose rots
# and guarded prose doesn't, so the number is generated and enforced rather than
# remembered.
./scripts/ledger-tally.sh --check
# The counts the docs state about the CODE (schema version, command counts, IPC
# exports). Same reasoning as the tally above, and the same evidence behind it:
# six of them were stale at once on 21 Aug 2026, none in a way that broke
# anything and all in the way that costs a reader their trust in the file.
./scripts/doc-counts.sh --check
# THIRD-PARTY-NOTICES.md's Rust table against the graphs that ship, both
# directions. It was typed, and "regenerate before each release" kept it true
# until it didn't: mysql_async's crates and the rex CLI's whole graph were
# missing from five releases' Licenses dialog before a hand count found them
# (13 Sep 2026, ledger #592).
./scripts/notices-check.py
# check-app-manifest.sh against file:// fixtures and a throwaway key: CDN lag must
# read as CDN lag, not as a forgotten publish (13 Sep 2026, ledger #594). Offline.
./scripts/check-app-manifest-test.sh
./scripts/status.py --check
# Every Rust install takes rust-toolchain.toml's version: CI's unpinned `stable` moved to
# 1.99 on 2 Oct 2026 and kept Windows verify red for four days on a lint no dev machine
# could reproduce (ledger #792).
./scripts/toolchain-pin-check.sh

# The L2 probes parse. They only RUN at release time (verify-full.sh / a manual
# sweep), so an edit that breaks one is invisible until then: a stray backtick
# in a comment inside contrast.js's template literal made that probe a syntax
# error, and it stayed one through several commits because nothing cheap looked.
# `node --check` needs no browser and no dev server, so the parse can be a
# pre-commit gate even though the probe itself cannot.
for f in scripts/wk-checks/*.js; do
  node --check "$f" || { echo "verify: $f does not parse"; exit 1; }
done

# The Windows compile check (docs/PLAN-windows-port.md W1). Owner ruling, 12 Sep
# 2026: part of THIS bar, not only the release gate — while the port is under way a
# Windows break has to surface in the commit that made it, not at release time,
# when it is twenty commits deep. It needs a toolchain this bar cannot assume
# (cargo-xwin, brew llvm + lld, and Microsoft's SDK licence accepted through
# XWIN_ACCEPT_LICENSE=1 — consent this script never gives on anyone's behalf,
# ledger #583). So windows-check's exit 3, "cannot run on this machine", becomes a
# SKIPPED line printed right above the verdict, and every other non-zero exit is a
# Windows compile break that fails the bar. Exit codes are captured, never piped.
# The template carries its own XXXXXX: BSD mktemp appends them, GNU (Git Bash on the
# Windows runner, W12) refuses a template without at least three. Valid on both.
WC_LOG="$(mktemp -t rexenv-windows-check.XXXXXX)"
set +e
./scripts/windows-check.sh > "$WC_LOG" 2>&1
wc_code=$?
set -e
WC_SKIPPED=""
case "$wc_code" in
  0) grep '^windows-check:' "$WC_LOG" || true ;;
  3) WC_SKIPPED="$(grep -m1 '^windows-check:' "$WC_LOG" || head -n1 "$WC_LOG" || true)" ;;
  *)
    cat "$WC_LOG"
    echo "verify: windows-check is RED (exit $wc_code) — the tree no longer compiles for Windows"
    exit 1
    ;;
esac

# The Linux compile check (docs/PLAN-linux-port.md L0), the same contract: it runs the
# same clippy question inside an Ubuntu 22.04 container, so it needs Docker's daemon up
# — closed on the dev Mac most days — and exit 3 becomes a SKIPPED line; any other
# non-zero exit is a Linux break and fails the bar. Wired the day the Linux impls landed
# (24 Sep 2026) so a Linux break surfaces in the commit that made it, as Windows's does.
# Pending the owner's D-L10 ruling on whether it stays in this bar or moves to
# verify-full.sh (it costs minutes per run while Docker is up).
LC_LOG="$(mktemp -t rexenv-linux-check.XXXXXX)"
set +e
./scripts/linux-check.sh > "$LC_LOG" 2>&1
lc_code=$?
set -e
LC_SKIPPED=""
case "$lc_code" in
  0) grep '^linux-check:' "$LC_LOG" || true ;;
  3) LC_SKIPPED="$(grep -m1 '^linux-check:' "$LC_LOG" || head -n1 "$LC_LOG" || true)" ;;
  *)
    cat "$LC_LOG"
    echo "verify: linux-check is RED (exit $lc_code) — the tree no longer compiles for Linux"
    exit 1
    ;;
esac

# The receipt (see scripts/verify-receipt.sh). Written LAST, and only when the
# tree is still the one that was checked.
if [ -n "$TREE_BEFORE" ]; then
  if [ "$TREE_BEFORE" = "$(./scripts/verify-receipt.sh fingerprint)" ]; then
    ./scripts/verify-receipt.sh write "$TREE_BEFORE"
  else
    echo "verify: NO RECEIPT — the code changed while this run was in flight, so this"
    echo "        green belongs to a tree that is no longer on disk. Re-run before committing."
  fi
fi

if [ -n "$WC_SKIPPED" ]; then
  echo "verify: windows-check SKIPPED — ${WC_SKIPPED}"
fi
if [ -n "$LC_SKIPPED" ]; then
  echo "verify: linux-check SKIPPED — ${LC_SKIPPED}"
fi
# Every gate has passed; drop the ~8 GB of example binaries the next run relinks.
bash scripts/disk-guard.sh after
echo "verify: all green"
