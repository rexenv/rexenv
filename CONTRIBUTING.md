# Contributing to rexenv

rexenv is a native (no-Docker) local dev environment: a Tauri 2 app — Rust backend,
React/TS frontend — that runs a real local stack (edge proxy with HTTPS, nginx,
multi-version PHP, MySQL/MariaDB/Postgres/Redis, WordPress tooling, DNS, mail,
tunnels). macOS is complete; the Windows port is in progress (its stubs fail as
`Error::Unported`), and Linux stubs are deliberate `todo!()`.

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
scripts/verify.sh        # THE pre-commit bar: lib + cli tests, example builds,
                         #   clippy --all-targets -D warnings in BOTH crates, tsc,
                         #   and the two generated-number gates (ledger-tally, doc-counts)
scripts/verify-full.sh   # release gate: verify + L1 sandbox tier + WebKit harness
scripts/live-checks.sh   # tiered live checks (see the tier table inside)
scripts/windows-check.sh # Windows x64 COMPILE check via cargo-xwin — runs inside verify.sh;
                         #   SKIPPED there without the toolchain + XWIN_ACCEPT_LICENSE=1 (TESTING.md §6)
```

Green comes ONLY from a script's own final line (`verify: all green`). An ad-hoc
`cargo test` or `tsc` run is not a gate: shell state resets between steps and a piped
exit code once let a broken commit through. The scripts set their own cwd; their exit
code is the verdict.

### The hooks

Install once per clone — this enables BOTH hooks below:

```bash
git config core.hooksPath scripts/git-hooks
```

#### `pre-commit` — the verify receipt

`verify.sh` records a receipt on green (`.git/rexenv-verify-receipt`), and the
`pre-commit` hook refuses a commit that touches code without a matching one. It
exists because the rule above ("never a piped check") was walked into by the person
who wrote it, in the session it was written in — a rule that relies on memory is not
a control.

- **Doc-only commits are never gated**, and rebase, merge, cherry-pick and revert are
  skipped — none of them should make `--no-verify` routine, because an override you
  need daily stops being an override.
- **`git add` does not invalidate the receipt.** It fingerprints file CONTENT, so
  staging, unstaging and `git commit --amend` with no code change all pass, while any
  edit at all fails. (The first version mixed in `git status --porcelain` and blocked
  every commit, since staging flips the status letters.)
- **Override explicitly** when you mean it: `git commit --no-verify`. The refusal
  message says so, and says what to run instead.
- **What it does not catch** is written in the hook itself — chiefly a PARTIAL commit
  (the receipt covers the whole tree, so a staged subset was never put through the bar
  on its own) and the quality of the bar itself.

#### `pre-push` — no `v*` tag to a private `rexenv/rexenv`

`.github/workflows/release.yml` fires on `push: tags: ["v*"]`. While this repo is
private that build spends tens of macOS-runner minutes at the 10x private multiplier
to produce a dmg **nobody can download** — `brew` fetches a cask url with no auth and
a private repo's asset answers 404, which is the whole reason the artefact ships from
the tap (`docs/RELEASING.md`). It also drafts a release *here*, beside the real one on
the tap: a second artefact waiting to be published by mistake.

RELEASING.md already said "keep the tag local". That was a memory, and the same
finding applies as above — so it has a mechanism now.

- **Only `v*` tags to this repo's own remote.** Branches, other tags, forks and
  mirrors are untouched; none of them trigger our workflow. Tag **deletions** pass.
- **It retires itself.** It asks GitHub whether `rexenv/rexenv` is still private and
  stands down the moment it is public — because then pushing a tag becomes the
  intended release path, and a guard that outlives its reason trains `--no-verify`
  into a habit, which would kill the pre-commit hook too.
- **Unknown counts as private** (no `gh`, not logged in, offline). The errors are not
  symmetrical: a wrong refusal costs one flag, a wrong allow costs a billed build and
  a stray release. This only runs when you push a `v*` tag, so strict is cheap.
- **Override explicitly**: `git push --no-verify origin v<X.Y.Z>`. The message says so.
- **What it cannot see**: Actions → Release → *Run workflow*, which starts the same
  build from the web UI.

The project's test metric is `docs/CLAIM-LEDGER.md` (claims proven / claims provable),
never line coverage. The layer model — what each test level can and cannot prove — is
`docs/TESTING.md`. If you write an invariant comment ("never", "always", "safe
because"), its ledger row lands in the SAME commit.

## Working conventions

- **Both platforms, same change.** macOS and Windows are both shipping targets. Anything
  OS-shaped — a path, a URL or origin, a command line, a refusal, a user-facing sentence —
  goes in `platform/words.rs` or behind a `platform/traits.rs` capability, never inline in
  one OS's spelling. `verify.sh`'s `windows-check` only proves it COMPILES there; a Windows
  claim needs a Windows run (`docs/TESTING.md` §"Proving a Windows claim").
- **Plan first.** Non-trivial work starts with a written plan (`docs/PLAN-<feature>.md`
  while in flight; it moves to `docs/archive/` when shipped); surface assumptions before
  building. One task at a time. `./scripts/status.py` shows what is open and in flight.
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
- **The MCP endpoint's capability comes from WHICH REGISTRY a tool is in**, never from
  anything the tool says about itself — read-only tools live in `mcp_server/tools.rs` and
  take a `ReadCtx` that has no mutating method; executing tools live in
  `mcp_server/scratch.rs` and can only reach a site through a witness whose only
  constructor checks `origin='agent'`. Adding a tool to the wrong file is a failing test,
  and adding one that touches a user's site is a compile error. It ships OFF, and it is
  deliberately not called a sandbox (`docs/ARCHITECTURE.md` §8.3).
- **The `rex` CLI never links the app lib** — remote control only, over a private
  socket, dispatching to the same command fns as the UI. App not running = exit 2,
  by design (a headless second brain is the bug class the stack guard exists to kill).
- **`commands/` are thin; `core/` is platform-agnostic; ALL OS code sits behind the
  13 traits in `platform/traits.rs`.** Unfilled Windows stubs return `Error::Unported`
  (or panic through `unported!` where the trait cannot return an error — never `todo!()`,
  ledger #595); Linux stubs stay `todo!()`. A port fills them — never restructure around them.
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
