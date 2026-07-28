# Contributing to rexenv

rexenv is a native (no-Docker) local dev environment: a Tauri 2 app — Rust backend,
React/TS frontend — that runs a real local stack (edge proxy with HTTPS, nginx,
multi-version PHP, MySQL/MariaDB/Postgres/Redis, WordPress tooling, DNS, mail,
tunnels). macOS is complete; Windows/Linux are deliberate `todo!()` stubs.

Read `docs/ARCHITECTURE.md` before changing anything — it replaces reading the
codebase end-to-end. `docs/MAP.md` answers "where does X live".

## Build and run

Prerequisites (macOS): Rust stable · Node LTS + pnpm · Xcode Command Line Tools.

```bash
pnpm install
pnpm tauri dev          # the app (Vite + cargo, first build is slow)
```

The app manages real system state (a root LaunchDaemon on :443, `/etc/resolver/*`
files, a login-keychain CA, LaunchAgents). First run walks Onboarding and asks for
those permissions. `Settings → Remove system changes` undoes all of it.

## The verification gate

```bash
scripts/verify.sh        # THE pre-commit bar: lib tests + example builds + clippy -D warnings + tsc
scripts/verify-full.sh   # release gate: verify + L1 sandbox tier + WebKit harness
scripts/live-checks.sh   # tiered live checks (see the tier table inside)
```

Green comes ONLY from a script's own final line (`verify: all green`). An ad-hoc
`cargo test` or `tsc` run is not a gate: shell state resets between steps and a piped
exit code once let a broken commit through. The scripts set their own cwd; their exit
code is the verdict.

The project's test metric is `docs/CLAIM-LEDGER.md` (claims proven / claims provable),
never line coverage. The layer model — what each test level can and cannot prove — is
`docs/TESTING.md`. If you write an invariant comment ("never", "always", "safe
because"), its ledger row lands in the SAME commit.

## Working conventions

- **Plan first.** Non-trivial work starts with a written plan (docs/PLAN-*.md for
  features); surface assumptions before building. One task at a time.
- **One commit per task**, each leaving verify.sh green. Tick the matching item in
  `docs/TODO.md` with a one-line ✓ evidence note in the same commit.
- **Live checks are the integration layer.** `src-tauri/examples/*.rs` run against
  REAL binaries and REAL app data. Read the invariant in
  `src-tauri/examples/common/mod.rs` FIRST: anything an example writes, spawns or
  deletes must be fixture-owned (`common::sandbox()`, `common::Reaped`, fixture
  ports). Every example declares a tier in `scripts/live-checks.sh`.
- **Honest UI.** Status must derive from reality (ownership AND liveness, never a
  bare port-listen); progress bars move only on real completions; refusals name the
  consequence, never "busy"; unknowable state says so ("can't determine") instead of
  guessing. Recorded facts (db_name, override ports, docroot ownership, content dir)
  are read from the record, never re-derived.
- **Frontend:** typed IPC wrappers in `src/lib/ipc/` only (never raw `invoke`);
  design tokens from `src/styles/tokens.css` (never hardcoded hex); JetBrains Mono
  for all technical values. WKWebView is the shipping engine — verify UI in the
  WebKit harness (`scripts/wk-checks/`), not just Chrome.

## Deliberate decisions (don't "fix" these)

Much of this codebase looks odd on purpose. Before changing something surprising,
check `docs/ARCHITECTURE.md` and this list:

- **No Docker; binaries download on demand** through `BinaryProvider`, pinned +
  checksum-locked. Nothing third-party ships inside the app bundle.
- **Services OUTLIVE the app** (closing rexenv stops nothing; next launch adopts) —
  EXCEPT repo jobs/watchers and tunnels, which die with the app on purpose.
- **Edge admin is a private unix socket, never TCP :2019** — a TCP admin on a root
  Caddy is arbitrary file r/w as root.
- **One php-fpm pool per PHP version, not per site.** Per-site needs ride the
  request (fastcgi_param / SetEnv), not the pool.
- **The `rex` CLI never links the app lib** — remote control only, over a private
  socket, dispatching to the same command fns as the UI. App not running = exit 2,
  by design (a headless second brain is the bug class the stack guard exists to kill).
- **`commands/` are thin; `core/` is platform-agnostic; ALL OS code sits behind the
  11 traits in `platform/traits.rs`.** Windows/Linux stubs stay `todo!()` until an
  OS port fills them — never restructure around them.
- **TLS leaves ≤398 days** (Safari rejects longer), CA trust in the LOGIN keychain
  (System keychain is unreachable from a detached-root osascript).
- **Quote every path in generated configs** — app-data paths contain spaces.
- **wp-cli never via `wp db …`/shell redirection** — bundled DB clients with
  shell-free I/O (PATH assumptions break in a Finder-launched app).
- **Repo scripts never run implicitly**; builds use the DEVELOPER'S toolchain
  (login-shell env) except composer (always our pinned phar via the site's PHP).
- Screen comps in `design/` and the archive docs are historical — `docs/archive/`
  may contradict current code and never wins.

## Licence and sign-off (DCO)

rexenv is Apache-2.0 (see `LICENSE`). By submitting a contribution you agree it
is licensed under Apache-2.0 (the licence's §5 covers this), and we additionally
use the Developer Certificate of Origin: sign each commit off with
`git commit -s`, which appends

```
Signed-off-by: Your Name <you@example.com>
```

The sign-off certifies the DCO 1.1 (https://developercertificate.org): in short,
that you wrote the contribution or otherwise have the right to submit it under
the project's licence. No CLA.

## Pull requests

Keep changes surgical: touch what the task needs, match surrounding style, no
drive-by refactors. Say in the PR what you verified and how (which script, which
example, what you ran live). If tests fail, say so — an honest red beats a fake green.
