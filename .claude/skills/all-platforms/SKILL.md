---
name: all-platforms
description: Design and build a rexenv feature or fix so it works on macOS, Windows AND Linux in the same change — the common part written once in core/, every OS difference written per OS with all three filled, and a per-OS proof. Use when starting, planning or implementing ANY feature or bug fix, and before committing one.
---

# all-platforms — a feature that works on one OS is a third of a feature

"All platforms" = exactly three: **macOS, Windows and Linux**. Never two of them.

The reference is `docs/PLATFORMS.md`. Read §3 (the per-OS mechanism table) and the trap
section of every OS the change touches (§6 macOS · §7 Windows · §8 Linux) BEFORE writing
code. This skill is the order; the file is the facts.

Why it exists: four Windows-only defects shipped in one week (Sep 2026) from work written
for macOS and only compiled for Windows; the first Linux DNS design copied macOS's shape
and sent the whole internet to loopback (#717).

## 1. Split the work — say it in the plan

Write down two lists before any code:

- **Common** (once, in `core/` or the frontend): the rule, the data, the flow, the UI.
  `core/` names no OS — no `cfg(target_os)`; if it needs an OS fact, it asks a trait for a
  capability (`may_spawn_with_admin_token`, #698).
- **Per OS** (one answer each for macOS, Windows, Linux): every path, URL/origin, command,
  refusal, sentence, and every behaviour that touches the OS.

If the per-OS list is empty, check again: paths, separators, shell commands, prompts, file
managers, trust stores, ports (53 vs 15353) and webview engines are the usual hiding places.

## 2. Put each difference in its home (PLATFORMS §2)

| Difference | Home |
|---|---|
| a value / sentence | `platform/words.rs` — a field on `PlatformWords`, `MACOS` + `WINDOWS` + `LINUX` all filled |
| a behaviour | a trait method in `platform/traits.rs`, impl in `platform/{macos,windows,linux}/` |
| pure text an impl produces | a `*_rules.rs`-style module in the OS folder, unit-tested on any host |
| a download | an OS arm in `binaries::manifest` + `docs/PORTS.md`'s table for that OS |
| shared by macOS + Linux | a neutral `platform/` module both call (`resolver_files.rs`) |
| cannot be done on an OS yet | `Error::Unported` / `unported!` + a TODO row + an honest refusal — never `todo!()` |

A `cfg(target_os)` outside `platform/` (app shell only) gets a row in
`docs/PLAN-linux-port.md` §1.1 naming what the other OSes get.

## 3. Build all three in the same change

Not "macOS now, Windows next commit". The commit that adds the macOS answer adds the
Windows and Linux answers. Tests assert `platform::words::current()` / the platform's own
command, never one OS's literal. Check every END of a policy (CSP, frame, header, URL) on
every OS.

## 4. Prove it per OS (PLATFORMS §5)

- `verify.sh` = native build + `windows-check` + `linux-check`. Those two are COMPILE gates;
  a `SKIPPED` line means that OS was not even compiled — say so in the report.
- A behaviour claim needs a RUN on that OS: `docs/TESTING.md` §"Proving a Windows claim" /
  §"Proving a Linux claim" (headless example → installed package → GUI).
- A new example declares its OS in `scripts/live-checks.sh` (`all` = macOS, Windows AND Linux).
- The ledger verdict names the OS: proven on one only = `◐ (<os> only)`.
- Every OS without a run gets its row in `docs/SMOKE-TEST.md`'s section for that OS.

## 5. Docs, per OS, same commit

Behaviour → the common doc section. Mechanism → `docs/PLATFORMS.md` §3 + the OS's section
of `INSTALL.md` / `SMOKE-TEST.md` / `PORTS.md`. Then `finish-task`.

## Report

For each OS: what was built, what proves it (compiled only / example run on <host> /
installed-package run / GUI), and what a human still owes (the SMOKE row).
