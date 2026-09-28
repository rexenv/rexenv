---
name: live-check
description: Run rexenv's live examples (src-tauri/examples/*.rs) safely by tier — sandbox, service, network, stack, system — against real binaries and real app data, without touching the owner's sites or the running stack. Use when a ledger row needs an L1 proof, when asked to run a check live, or before a release (verify-full.sh).
---

# live-check — examples run against REAL app data; the fixture owns everything it touches

Read `src-tauri/examples/common/mod.rs` FIRST — the invariant. Anything an example
writes, spawns or deletes MUST be fixture-owned: `common::sandbox()` for a
throwaway `Platform`, `common::Reaped` / `OwnedService` for spawned services. On
24 Jul 2026 an `rm` of `docroot.parent()` deleted the owner's whole Sites folder.
Delete only paths the fixture created; print a derived path before removing it.

## Tiers (`scripts/live-checks.sh`)

```
./scripts/live-checks.sh list [tier]      # what is in each tier
./scripts/live-checks.sh                  # sandbox — safe with the stack running
./scripts/live-checks.sh service|network|stack
./scripts/live-checks.sh system <name>    # one at a time, changes the machine
cd src-tauri && cargo run --example <name>   # a single example
```

- **sandbox**: fixture ports + sandboxed paths; run anytime.
- **service**: brings its OWN copy of a fixed-port service — refuses beside a
  live one. Stop the stack first or expect an honest refusal.
- **network**: real downloads / wp.org; slow.
- **stack**: needs the real stack UP.
- **system**: root ops, resolver files — one at a time, foreground (privileged
  prompts cannot render from a background shell).

Every new example declares a tier AND an OS in the script's table, or the script
fails. The OS column: `all` (**macOS, Windows AND Linux** — spelled `both` until 28 Sep
2026), `macos`, `windows`, `linux`; the script skips an example on a host it does not list.
An OS-neutral example is `all`, and its fixtures come from `common::fixture_base()`
(`/private/tmp` on macOS, `/tmp` on Linux, the temp dir on Windows) — never a literal
path (a Linux run died creating `/private`).

## Running on the other OSes

An example run on the Mac proves the macOS half only. Windows and Linux runs happen on
their own hosts — `docs/TESTING.md` §"Proving a Windows claim" (cross-build the example
with cargo-xwin, run on the Dell / Win11 VM over SSH) and §"Proving a Linux claim"
(Ubuntu 22.04 arm64 UTM VM — debug builds, `CARGO_BUILD_JOBS=1`; the Dell's WSL2 for
x86_64; the check container for process/file halves). Record which host ran it.

## Rules learned the hard way

- Do not run `verify.sh` while a tier runs — devtools probes time out at 10s while
  Gatekeeper is busy, and the red is false.
- Examples share the real app-data dir: an example that calls `caddy stop` stops
  the owner's edge. Use adopt + reload, never stop-the-real-edge.
- Orphan php-fpm / nginx workers from a SIGKILLed master hold ports; check
  `logs/health.log` before blaming a port probe.
- A green example that asserted nothing is worse than none: read what it
  asserts. The 21 Aug audit found gates that reported green having checked nothing.
- Redirect output to a file (the script refuses a pipe); quote the verdict line.

Record the run: the ledger row's verdict (`ledger-row`), and the TODO row if it was
the owed live leg. `verify-full.sh` = verify + sandbox tier + wk-checks = release gate.
