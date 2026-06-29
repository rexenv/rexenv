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

- [ ] **2.1 Busy :80 / :443 / service port**
  *Done when:* if the edge port (or any fixed service port) is already taken (e.g. another stack on :443),
  startup surfaces a specific "port X in use by …, free it and retry" message in the UI and leaves the app
  usable — no crash. (Builds on `core::ports::ensure_free` + the §7.3 stale-edge recovery.)

- [ ] **2.2 Failed / aborted binary download**
  *Done when:* a binary fetch that fails (network drop mid-download) or mismatches its checksum retries a
  bounded number of times, then shows a clear error naming the binary + cause; a partial file is discarded
  (not cached as valid). Re-running succeeds once connectivity returns.

- [ ] **2.3 Cancelled privilege prompt**
  *Done when:* if the user cancels the macOS admin prompt (resolver write / CA trust), the app shows a clear
  "setup incomplete — <feature> needs this; retry" state and stays usable; re-triggering the prompt works.

- [ ] **2.4 No internet on first run**
  *Done when:* with no connectivity and binaries not yet cached, the app explains that first-time setup needs
  internet to download components, names what's missing, and recovers cleanly once online (no crash, no
  infinite spinner).

- [ ] **2.5 Failure-state pass**
  *Done when:* 2.1–2.4 are each simulated on a clean machine and confirmed to produce a clean, understandable
  error state (notes/screenshots captured). Depends on 2.1, 2.2, 2.3, 2.4.

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
- [ ] **4.4 Settings completeness — theme + default PHP**
  *Done when:* Settings lets the user (a) pick the app **theme** (per DESIGN_BRIEF) and (b) set the
  **default PHP version** for new sites (a control + command that writes the `is_default` flag the New Site
  dialog already reads — today the PHP card only *displays* the Default badge, with no way to change it);
  both persist and take effect.
- [ ] **4.5 Record remaining §11 Phase-3 leftovers (no work, just state)**
  *Done when:* it's documented that §11.3 (blueprints) and §11.4 (Adminer deep-link) are DONE, and that
  §8.2 (per-site Xdebug) + §11.2-hosting stay BLOCKED on a hosted Xdebug static-PHP build (external —
  `docs/xdebug-debug-build.md`) and are NOT part of this release.

---

## 5. Release QA / polish

> A clean-Mac smoke test plus the rough edges that show up with real, varied usage.

- [ ] **5.1 Empty states**
  *Done when:* Sites, Mail, Tunnels, Databases, and Blueprints each render a clear empty state (no sites /
  no caught mail / no tunnels / etc.) instead of a blank or broken panel.

- [ ] **5.2 Many-sites behavior**
  *Done when:* with ~15–20 sites the Sites list, status footer, and edge config stay responsive and correct
  (scrolling, start/stop, reload), with no obvious slowdown or layout break.

- [ ] **5.3 Consistent error messaging**
  *Done when:* IPC/command errors surface through one consistent UI pattern (toast/inline) with
  human-readable text — no raw `Err(Other(...))` strings or silent failures in the common flows.

- [ ] **5.4 Clean-Mac smoke-test checklist**
  *Done when:* a written checklist (create WP site → HTTPS load → wp-admin → Mailpit catches a mail →
  Adminer deep-link → tunnel → multisite convert → teardown) is run on a clean Mac from the .dmg and all
  items pass. Depends on 1.5.

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
