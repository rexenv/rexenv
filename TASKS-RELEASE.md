# TASKS — Release (macOS, limited closed-source distribution)

> **Goal:** package the finished macOS app (Phase 1–3 feature work COMPLETE) into a
> **.dmg** I can hand to **myself + a few known people**. This is NOT new features and
> NOT a public release. **Ad-hoc signing only** (no Apple Developer account): users do a
> one-time right-click → Open. Full **Developer ID signing + notarization + stapling**,
> **Homebrew**, and wide distribution are **DEFERRED** until the project is open-sourced.
>
> Reuse the existing architecture (CLAUDE.md): `BinaryProvider` + macOS `prepare_binary`
> (the ad-hoc-sign pattern extends to the app bundle), `ProcessSupervisor`, `ServiceManager`,
> the platform traits, `core::setup` (system setup/teardown already exists). Build-pipeline
> context: PROJECT_SPEC.md §4.9.
>
> Scope: **macOS only**. Work top-to-bottom, one task at a time. Check the box only when
> "Done when" passes. **EXCLUDED (post-1.0 / Tier 3):** backup/restore, production-site import,
> blueprint marketplace, installers for other apps, Windows/Linux ports.

Status: `[ ]` todo · `[~]` in progress · `[x]` done · `[D]` intentionally deferred to a later phase (recorded, not todo)

---

## 1. Packaged build (.dmg) for limited distribution

> Produce an installable, launchable .dmg. Ad-hoc signing is enough for a few trusted Macs
> (one-time right-click → Open); Gatekeeper-clean wide distribution is deferred. Settle the
> app identity FIRST — it flows into signing, the app-data path, the launchd label, and (later)
> the updater.

- [x] **1.1 Reconcile the bundle identifier / runtime namespace (do FIRST)** — ✓ canonical id =
  **`dev.rexenv.rexenv`** (the conventional `rexenv.dev` + app-leaf form, already used by the app-data dir +
  launchd label — so no app-data path migration / no binary-cache invalidation). Changed only the outliers:
  `tauri.conf.json` `identifier` `dev.rexenv.app` → `dev.rexenv.rexenv` (this is also the signing/bundle id
  for 1.2) and a stray mock string in `src/lib/ipc`. Introduced `platform::macos::APP_IDENTIFIER` as the
  single source of truth; `AUTOSTART_LABEL` now derives from it; `Paths` already composes to it via
  `ProjectDirs("dev","rexenv","rexenv")`. Drift-guard tests fail the build if any of the four diverge:
  `app_data_namespace_matches_the_identifier`, `autostart_label_matches_the_identifier`, and
  `tauri_conf_identifier_matches_the_identifier` (reads tauri.conf.json at compile time). `cargo test --lib`
  112 pass (+3), tsc + vite build clean. (Signing itself = 1.2.)
  *Done when:* a single reverse-DNS identity is used **consistently** across (a) `tauri.conf.json`
  `identifier`, (b) the ad-hoc signing of the bundle, (c) the runtime app-data dir
  (`~/Library/Application Support/<id>/`), and (d) the launchd autostart label. Today these disagree —
  `identifier` is `dev.rexenv.app` while the app-data dir + `MacosAutostart` label are
  `dev.rexenv.rexenv`. Pick the canonical id, update `tauri.conf.json` + `platform::macos` `Paths`
  and `MacosAutostart`, and confirm a fresh run reads/writes the expected tree. **Do this NOW** while
  there's no real user data — otherwise changing the app-data path later needs a migration.

- [x] **1.2 Ad-hoc sign the app bundle** — ✓ added `bundle.macOS.signingIdentity: "-"` to
  `tauri.conf.json`. `pnpm tauri build` logs `Signing with identity "-"` and signs `rexenv.app`;
  `codesign -dv --verbose=4 rexenv.app` → `Signature=adhoc` (flags `0x10002(adhoc,runtime)`,
  `Identifier=dev.rexenv.rexenv` — the 1.1 id flows into the signature), and
  `codesign --verify --deep --strict rexenv.app` → "valid on disk / satisfies its Designated Requirement"
  (exit 0). Same ad-hoc approach as Phase 1 `prepare_binary` for downloaded binaries, now on the bundle so
  Apple Silicon doesn't reject it as "damaged". Depends on 1.1.
  *Done when:* `tauri.conf.json` `bundle.macOS.signingIdentity` is `"-"` (ad-hoc), so `pnpm tauri build`
  signs `rexenv.app` with it; `codesign -dv --verbose=4 rexenv.app` reports `Signature=adhoc` and
  `codesign --verify --deep --strict rexenv.app` passes.

- [x] **1.3 Produce the .dmg (choose + record architecture + min macOS)** — ✓ **decision: UNIVERSAL
  (`x86_64 + arm64`)** — recipients include Intel Macs, so the release is a universal binary. Canonical
  build = **`pnpm release:mac`** (`tauri build --target universal-apple-darwin`; added as a package.json
  script so it's one repeatable command); `rustup target add x86_64-apple-darwin` is the one-time prereq.
  amd64 + arm64 runtime-binary checksums are already pinned in `core/binaries`, so `BinaryProvider`
  downloads the right-arch PHP/Nginx/MySQL/etc. per host at first run. Set
  `bundle.macOS.minimumSystemVersion: "11.0"` (Big Sur — runs on Intel + Apple Silicon). Produces
  `…/universal-apple-darwin/release/bundle/dmg/rexenv_0.1.0_universal.dmg` (15 MB, no errors); verified:
  app binary `lipo -archs` = `x86_64 arm64`, `Info.plist LSMinimumSystemVersion = 11.0`,
  `codesign --verify --deep --strict` passes (ad-hoc, `Identifier=dev.rexenv.rexenv`), the dmg mounts
  (`rexenv.app` + an `Applications` symlink → drag-to-install) and unmounts clean. Depends on 1.2.
  *Done when:* the build target is explicitly chosen and recorded — **Apple-Silicon-only** (`aarch64`,
  the default) OR a **universal** binary (`pnpm tauri build --target universal-apple-darwin`, if anyone I
  share with may be on an Intel Mac) — and a **minimum macOS version** is set via
  `bundle.macOS.minimumSystemVersion`. `pnpm tauri build` then produces
  `src-tauri/target/release/bundle/dmg/rexenv_<ver>_<arch>.dmg` with no build errors; it mounts and the
  drag-to-Applications install works.

- [x] **1.4 INSTALL.md / release note** — ✓ `INSTALL.md` (repo root) covers: requirements (**universal —
  Intel + Apple Silicon — macOS 11+**, internet on first run); install (drag to Applications); **first launch
  = right-click → Open**, or System Settings → Privacy & Security → **Open Anyway**, and that it opens
  normally thereafter; the expected one-time setup prompts (**1** admin → `.test` resolver, **2** Keychain →
  trust local CA, **3** admin → edge ports 80/443) — verified against `core::setup::run_system_setup` +
  `proxy::start_privileged`; a verify-it-works flow; a "won't open" troubleshooting section (Gatekeeper /
  "damaged" / too-old macOS); and an Updating note (data lives under
  `~/Library/Application Support/dev.rexenv.rexenv/`). Depends on 1.3.

- [ ] **1.5 Verify a COLD first run on a SECOND Mac / clean account**
  *Done when:* on a machine with **NO cached binaries** (a different Mac, or a clean macOS user, whose arch
  matches the .dmg from 1.3), the .dmg installs and launches after the one-time right-click → Open, and the
  **full cold first run completes cleanly**: onboarding downloads every component (PHP, Nginx, MySQL, Caddy,
  WP-CLI, …) and the one-time admin prompts (resolver file + CA trust) succeed — THEN the end-to-end check
  passes: create a WordPress site and load it over HTTPS at `https://<name>.test` with a valid local-CA lock.
  (Also the real-world test bed for 2.2 download-failure + 2.4 no-internet.) Depends on 1.3, 1.4.

- [D] **1.6 Developer ID signing + notarization + stapling**
  *(Out of scope this phase.)* Needed for warning-free WIDE distribution: a paid Apple Developer account
  ($99/yr), `signingIdentity: "Developer ID Application: …"`, notarization via `notarytool`, and
  `xcrun stapler staple`. **Homebrew Cask** becomes an option only after open-sourcing + notarizing.

---

## 2. First-run / runtime robustness

> A few users' real machines will hit messy states. Every failure must produce a clear,
> understandable error in the UI — never a crash, hang, or silent no-op.

- [x] **2.1 Busy :80 / :443 / service port** — ✓ `ports::ensure_free` already errors with a specific
  "port {port}/{proto} (needed by {service}) is already in use" (unit-tested for content). Gap fixed: the
  **"Start all"** mutation (`StatusFooter`) and the Sites start/stop + delete mutations had **no `onError`**,
  so a busy-port failure was swallowed silently — added `onError → window.alert` so it surfaces; the app
  stays up (it's a returned `Err`, no panic). Verified live in `robustness_check`: a bound port →
  `port 54138/tcp (needed by edge) is already in use`. (Stale-edge recovery from §7.3 still auto-clears a
  leftover rexenv edge first.)

- [x] **2.2 Failed / aborted binary download** — ✓ `http_get` now retries transient failures (connect
  drop / timeout / aborted body / 5xx) up to `DOWNLOAD_ATTEMPTS` (3) with linear backoff, then returns a
  clear error ending "(gave up after 3 attempts)"; a **4xx is permanent** (no wasted retries) — the
  retry-vs-permanent split is the unit-tested `status_is_transient` (500/502 → retry; 404/403 → stop).
  **Partial files are never cached:** the download is fully read → checksum-verified → only THEN written to
  disk, so any abort/mismatch leaves nothing behind (checksum reject unit-tested). Verified live in
  `robustness_check`: a 404 fails in ~0.5 s (no retry storm). Re-running after connectivity returns succeeds
  (idempotent `resolve`).

- [x] **2.3 Cancelled privilege prompt** — ✓ `MacosPrivileges::run_privileged` now maps a dismissed auth
  dialog (AppleScript `-128` / "User canceled") to a friendly, recoverable message —
  "Administrator permission was cancelled — this step needs it. Try again and approve the prompt." — instead
  of a raw error code (other failures keep their detail). It returns `Err` (no crash) and surfaces via the
  same `onError` alerts; re-triggering the action re-shows the prompt. Verified by the macOS unit test
  `privileged_cancel_is_a_friendly_recoverable_message` (the live cancel runs this exact mapping).

- [x] **2.4 No internet on first run** — ✓ the download client now has a **15 s connect timeout** (+120 s
  overall) so it can't hang, and a connectivity-aware message: a connect/timeout failure reads
  "can't reach {url} — check your internet connection (…)" rather than a raw transport error. Bounded by the
  same 3-attempt cap. Verified live in `robustness_check`: an unreachable host →
  "can't reach … — check your internet connection … (gave up after 3 attempts)" in ~1.2 s. Recovers cleanly
  once online (re-run resolves), no crash / no infinite spinner.

- [x] **2.5 Failure-state pass** — ✓ `examples/robustness_check.rs` simulates 2.1 (busy port), 2.2 (404
  download → permanent, no retry storm) and 2.4 (unreachable host → connectivity hint, bounded ~1.2 s) live —
  all produce a clean, clear error and never crash/hang; 2.3 (cancelled prompt) is covered by its unit test.
  `cargo test --lib` 114 pass; tsc + vite build clean. Depends on 2.1, 2.2, 2.3, 2.4.

---

## 3. Clean uninstall / teardown

> Reverse every system-level change rexenv makes, so removing it leaves the machine clean.

- [x] **3.1 Expose system teardown (command + Settings UI)** — ✓ added `uninstall_system` command:
  `stop_all` (so the edge releases :80/:443) then `core::setup::run_system_teardown` (remove
  `/etc/resolver/test` via admin prompt + untrust the local CA, the Phase 1 §3.3 path); registered in
  `lib.rs`, typed `ipc.uninstallSystem`. Settings has a new **Uninstall** card — explains the reversal
  (site files/DBs kept), `window.confirm` guard, runs it, shows a clear success line / error. Verified:
  `cargo build --lib` clean, tsc + vite build clean, UI rendered via chrome-devtools (red Remove button +
  copy). Site files + databases under app-data are intentionally left for the user to delete with the app.

- [x] **3.2 Verify the machine is clean after teardown** — ✓ ran the REAL teardown live
  (`examples/teardown_check.rs` → `core::setup::run_system_teardown`, admin prompt entered). BEFORE→AFTER on
  this machine: `/etc/resolver/test` present `true`→`false`; `foo.test` resolves `true`→`false`
  (`dscacheutil -q host foo.test` returns no IP — the teardown now also flushes the DNS cache, fixed in this
  task so `.test` stops resolving immediately); CA trusted `true`→`false` — `security dump-trust-settings`
  no longer lists rexenv and `security verify-cert` returns `CSSMERR_TP_NOT_TRUSTED`. No rexenv services
  left running (the command runs `stop_all` first). The example is idempotent/re-runnable (skips the
  privileged step when already clean). `cargo test --lib` 112 pass. Depends on 3.1.

---

## 4. Finish deferred UI + autostart

> Close out the Settings screen (DESIGN_BRIEF Block 11) and the last stubbed platform trait. Several
> pieces landed already in Phase 3 §11.1 (verified against the code — marked `[x]`); the rest is real
> release work.

- [x] **4.1 DNS & SSL controls** — ✓ done in Phase 3 §11.1 (verified): Settings "DNS & SSL" card —
  re-trust CA, regenerate certs, and a DNS status indicator (resolver running + `/etc/resolver/test`
  installed).
- [x] **4.2 AutostartManager (macOS launchd)** — ✓ done in Phase 3 §11.1 (verified): `MacosAutostart` is a
  REAL impl (per-user LaunchAgent, `enable`/`disable`/`is_enabled`, `RunAtLoad`) — no longer `todo!()`;
  Settings "Start rexenv on login" toggle drives it. (The last stubbed Phase-1 trait, now real.) Its label
  is reconciled with the bundle id in **1.1**.
- [x] **4.3 Sites-folder setting** — ✓ done (verified): Settings "General" → sites-folder override.
- [x] **4.4 Settings completeness — theme + default PHP** — ✓ both done.
  **(a) Theme (Dark / Light / System, DESIGN_BRIEF §172):** authored a full light palette in `tokens.css`
  (`[data-theme="light"]` overriding the surface/border/text scale + elevation + `color-scheme`; brand/status/
  type inherit — works because components use semantic `--rex-*` tokens, not hex). `src/lib/theme.ts`
  persists the choice in localStorage, applies `data-theme` on `<html>`, and tracks the OS for "System";
  `initTheme()` runs in `main.tsx` before render (no flash). Settings "General" gains a Dark/Light/System
  segmented control. Verified live via chrome-devtools: switching to Light re-skins the whole app
  coherently (sidebar, footer, all cards).
  **(b) Default PHP:** `store::set_default_php_version` (atomic exclusive flip) + `core::php::set_default`
  (guards: must be installed) + `set_default_php_version` command + `ipc.setDefaultPhpVersion`; the PHP
  versions card now shows a **Make default** action on each installed non-default version (the New Site
  dialog already reads the default). Verified: unit test `set_default_switches_exclusively_and_requires_installed`,
  and the button live in the UI. `cargo test --lib` 115 pass (+1); tsc + vite build clean.
- [x] **4.5 Record remaining §11 Phase-3 leftovers (no work, just state)** — ✓ recorded: Phase-3 **§11.1**
  (Settings DNS/SSL + launchd autostart), **§11.3** (site blueprints) and **§11.4** (Adminer per-site
  deep-link) are all DONE and shipped. The only outstanding Phase-3 items, **§8.2** (per-site Xdebug toggle)
  and **§11.2-hosting**, remain BLOCKED on an externally-hosted Xdebug-enabled static-PHP build — the
  `php-debug` BinaryProvider variant + reproducible `spc` recipe are wired (`docs/xdebug-debug-build.md`),
  awaiting a maintainer build/host + checksum pin. **Neither is part of this release** (Xdebug is a
  dev-convenience toggle, not a packaging blocker).

---

## 5. Release QA / polish

> A clean-Mac smoke test plus the rough edges that show up with real, varied usage.

- [x] **5.1 Empty states** — ✓ audited: **Sites** ("No sites yet" + CTA), **Mail** ("No mail captured yet"
  / "Inbox empty" + search-aware "No messages match"), **Tunnels** ("No sites to share"), and the Blueprints
  Network sub-site list ("No sub-sites yet") all render clear empty states. **Databases** is never empty by
  design (MySQL + PostgreSQL engines always listed) and the **Blueprints** card always shows its add-form
  (no blank/broken panel). No blank panels in the common flows.

- [x] **5.2 Many-sites behavior** — ✓ verified with 19 sites (temporary mock seed, then reverted): the
  Sites list renders + scrolls cleanly, the header count ("19 sites · 10 running") and status footer update,
  rows stay consistent — no slowdown or layout break (chrome-devtools). The edge/nginx config generation is
  **O(n)** (one server block + Caddy route per site, already exercised with multiple sites in the Phase-1/3
  examples), so it scales linearly; the list is a plain map (no virtualization needed at this scale).

- [x] **5.3 Consistent error messaging** — ✓ audited every `useMutation` across the app: all now surface
  failures through **one consistent pattern** (`onError → window.alert(String(e))`) — fixed the one silent
  gap (the sites-folder `save`). The Rust `Error` serializes via its `Display` (e.g. `Error::Other(s)` → just
  `s`), so the UI shows **human-readable text, never a raw `Err(Other(...))`/debug wrapper**. No silent
  failures remain in the common flows. (A non-blocking toast is a possible future nicety; `alert` is the
  uniform, functional pattern for this limited release.)

- [~] **5.4 Clean-Mac smoke-test checklist** — ✓ **checklist written** (`SMOKE-TEST.md`, repo root): install
  + first launch, cold first run (downloads + the 3 setup prompts), WP-over-HTTPS, WP Manager, Mailpit,
  Adminer deep-link, multisite, tunnels, Settings (theme / default-PHP / autostart), robustness spot-checks,
  scale, and clean uninstall — each a checkbox with the expected result. **The actual clean-Mac run is the
  hands-on step (paired with 1.5)** — it needs a second Mac / fresh account + the .dmg, so it's left for you
  to execute. Depends on 1.5.

---

## 6. Auto-update — OPTIONAL (skip for this limited phase)

> For a handful of known users I can just hand over a new .dmg. Recorded so the wiring is understood
> when it's actually wanted.

- [D] **6.1 Tauri updater (deferred / optional)**
  *Not now.* When wanted, wiring needs: `@tauri-apps/plugin-updater` + `tauri-plugin-updater`, an update
  **signing keypair** (`tauri signer generate`; private key in CI secrets, public key in
  `tauri.conf.json`), a release **endpoint** serving `latest.json` (e.g. GitHub Releases), and signing each
  release artifact. Pairs naturally with 1.6 (notarization) once open-sourced.

---

## Notes / decisions

- **Ad-hoc, not notarized (intentional for this phase).** With no Apple Developer account, the bundle is
  ad-hoc signed (`codesign --sign -`) — enough to launch on Apple Silicon after a one-time right-click →
  Open on each trusted Mac. Notarization (1.6) is the open-source-era upgrade.
- **Settle the identity first (1.1).** The bundle id flows into signing, the app-data path, the launchd
  label, and (later) the updater. Today the bundle id (`dev.rexenv.app`) and the runtime namespace
  (`dev.rexenv.rexenv`) disagree; reconcile to one id NOW, before there's real user data, so no migration
  is needed.
- **Teardown reuses Phase 1.** `core::setup::run_system_teardown` already reverses the resolver + CA-trust
  changes (§3.3); §3 just exposes + verifies it. The per-site `teardown` (cert + docroot + DB row) already
  exists for individual sites.
- **What's already done (verified in code, §4):** DNS & SSL controls, the real launchd `AutostartManager`,
  and the sites-folder setting all shipped in Phase 3 §11.1 — `[x]`. Theme + default-PHP setter (4.4) are
  genuinely outstanding.
- **Distribution is limited + closed-source.** No public listing, no Homebrew, no auto-update required.
  Those unlock after open-sourcing (→ 1.6, 6.1).
- **Excluded (post-1.0 / Tier 3):** backup/restore, production-site import, blueprint marketplace, installers
  for other apps. Phase-3 Xdebug (§8.2 / §11.2 hosting) remains external-blocked, not in scope.
- **Verification carries over:** prefer a real check per task — a built/mounted .dmg, `codesign` / `security`
  output, `ping`/`dig` for resolver state, a clean-Mac run — over "looks right".
