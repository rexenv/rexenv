# PLAN — PHP update documents per OS (the signed manifest Windows and Linux read)

Status: **BUILT — T1 (rexenv `9695b502`), T2 (runtimes PR #49–#51), T3 published 24 Oct 2026 (`manifest-linux.json` / `manifest-windows.json`, serial 12 after the serial-1 replay finding, D5), read live on the Dell ("manifest accepted (6 entries)", `Adminer 6.2.0` offered). Owed: the Linux reader read live on the Ubuntu VM, and the first PHP offer the day php.net ships a patch above the pins (SMOKE).**

## 1. What is true today (24 Oct 2026)

- The app never asks php.net or static-php.dev. It fetches ONE signed document,
  `rexenv/runtimes` `manifest.json` + `.sig` (`core/updates.rs` `MANIFEST_URL`), at launch and
  every 6 h behind the one auto-check switch, verifies it against the compiled-in key, and
  `Family::Php::newer_than(installed, arch)` turns its rows into Settings → Updates offers.
  Discovery upstream (`runtimes` `discover-php.yml`, daily) ALREADY asks php.net's release index
  (not static-php.dev — moved after the 21 Oct security release went unnoticed); a human builds
  (`php.yml`, `php-windows.yml`) and runs "Publish PHP update manifest".
- **The document is macOS's.** Every php/php-fpm row is a macOS tarball; the publisher's guard
  forbids an `os` key in it (`manifest-guard.py --document macos`, the frozen contract: shipped
  Macs keep rows they do not understand). Since rexenv PR #5 an unmarked php row is admitted on
  macOS only — so on Windows and Linux the same document yields NO PHP offers, and those users
  get a PHP patch only when an app release moves the pin. The design ruled on 13 Sep 2026
  (PLAN-windows-port §3b) is one document per OS, `manifest-<os>.json`, same schema and key;
  the readers' half landed 30 Sep (ledger #755); the publisher's half and the readers' URL never did.
- The resolver is pins-first, catalog second (`php_spec`): a catalog version becomes a
  `BinarySpec` whose `archive`/`member` are DERIVED from the name, never read from the document.
  That derivation is macOS's (`TarGz`, `php`/`php-fpm`); the Windows and Linux `manifest()` arms
  read pins only.

## 2. Decisions

- **D1 One document per OS, same schema, same key, own serial:** `manifest.json` (macOS,
  frozen, unchanged), `manifest-linux.json`, `manifest-windows.json`, each with `.sig`. The reader
  picks by `std::env::consts::OS` the way `app_update::manifest_urls_on` already does for the app
  descriptor. A per-OS document carries everything that OS needs: its PHP rows marked `"os"`,
  AND the Adminer/phpMyAdmin file rows (unmarked, as on macOS) — a reader reads one document.
- **D2 Linux rows are OURS**: `php-8x-linux-N` releases (cli/fpm/licenses × x86_64/aarch64),
  arch spelled `x86_64`/`arm64` as the reader asks, no `minMacos`. A patch with no Linux build
  is "BUILD IT FIRST" exactly as macOS.
- **D3 Windows rows are php.net's NTS zips** from `downloads.php.net/~windows/releases/archives/`
  (the archive keeps every release; the pool is php-cgi inside the same zip, so ONE `php` row
  per arch — both `arm64` and `x86_64` name the same x64 zip, as the pins do for every `Arch`).
  Not self-distributed → no licences row. The reader's `ALLOWED_HOSTS` gains that one path
  prefix; the resolver derives `ZipTree { strip: 0 }` / `php.exe` for a Windows catalog row.
  The 7.4/8.0/8.1 backport builds move by PIN (same version, new bytes), never by manifest.
- **D5 A per-OS document's serial is lifted above the macOS document's** (found live, 24 Oct
  2026): a rexenv keeps ONE highest-accepted serial, and every Windows/Linux install before
  this read `manifest.json` (serial 11), so `manifest-windows.json` at serial 1 was refused as
  a replay on the Dell. The publisher lifts on every publish; the first real documents are
  serial 12. A per-document serial memory in the app would be the cleaner reader, but it cannot
  help the installs already out there — the lift is what reaches them.
- **D4 The publisher is one script with `--document macos|linux|windows`**, each document its own
  serial, carry-forward and guard; the workflow gains a `document` dropdown and a per-document
  concurrency group. Discovery stays OS-neutral (php.net's newest patch per minor); the daily
  issue says all three documents want publishing.

## 3. Tasks

| # | Task | Proof |
|---|---|---|
| T1 | rexenv reader: `updates::manifest_urls_on(os)` (macOS unsuffixed; `-linux`, `-windows`), `fetch_with` uses the host's; `ALLOWED_HOSTS` + php.net's archive prefix; `binaries::catalog_php_spec(name, version, arch, os)` derives archive/member per OS and the Windows/Linux php arms fall back to it after their pins | L0: a catalog with os-marked rows resolves `("php", v, "windows")` to `ZipTree{0}`/`php.exe` and `("php"/"php-fpm", v, "linux")` to `TarGz`; macOS unchanged; the URL table per OS |
| T2 | runtimes `publish-manifest.sh --document`, `publish-manifest.yml` dropdown, `manifest-guard.py` arch spelling for the PHP docs, `rexenv/runtimes/docs/MANIFEST.md` §2.4 | dry runs of both documents green; `manifest-guard --self-test` |
| T3 | Publish `manifest-linux.json` and `manifest-windows.json` (serial 1 each; the owner approves `manifest-signing` twice) | `curl` both + `.sig`; the Dell's and the Ubuntu VM's `rexenv.log` say "runtime updates: manifest accepted (N entries)" from the per-OS URL; `rex status --json` carries no PHP offer (nothing newer than the pins exists today) — the OFFER itself is proven the day php.net ships the next patch (SMOKE row) |
| T4 | Docs: PLATFORMS §3 row, ARCHITECTURE, PLAN-windows-port §3b closed, ledger #755 amended + new row, TODO | docs gates |
