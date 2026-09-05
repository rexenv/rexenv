# PLAN — in-app self-update: rexenv notices a published release, replaces itself, and reopens

Status: **planned, not started** (6 Sep 2026). Today the only update path is `brew upgrade
--cask rexenv`; a user who installed from the dmg has no path but downloading the next dmg
and dragging it over. This plan gives the app a signed update channel of its own — a check,
an offer, a verified download, an atomic bundle swap and a clean reopen — reusing the trust
machinery `core/updates.rs` already carries for PHP and Adminer. **T0 comes first and is a
measurement, not code**: whether macOS App Management lets an ad-hoc-signed bundle rename
itself in `/Applications` is documented nowhere, and the error handling below is a function
of that answer.

Produced from three independent designs (grain-of-the-codebase, maximal-plugin-reuse,
risk-first) run against the real files, plus nine read-only research reports whose evidence
is cited inline. Where the three designs disagreed the disagreement is recorded, not hidden.

---

## 1. The problem, stated as the user hit it

- The cask is the only updater. `docs/INSTALL.md` §Updating tells everyone else: "quit rexenv
  and replace `rexenv.app` in Applications with the new `.dmg`'s copy (drag over, replace)".
- The tap README goes further and says rexenv *does not* self-update and that an in-app
  updater would desynchronise brew. That was written for a cask without `auto_updates`; it
  becomes wrong the day this ships and is rewritten in the same change (T9/T10).
- `docs/TODO.md:544` has carried "Packaging polish: Tauri updater (keypair, endpoint,
  `latest.json`)" under Phase 4+ since the packaging era. This plan supersedes that row: the
  answer is not "wire the Tauri updater", it is "extend the signed-manifest channel this
  project already owns".
- A second, quieter bug is fixed on the way: after ANY bundle replacement — including today's
  manual drag-replace — the KeepAlive DNS LaunchAgent keeps executing the old, unlinked
  binary until something restarts it. `MacosDnsAgent::install` skips the reload when the plist
  bytes are unchanged (`platform/macos/mod.rs:988-1005`), the watchdog only kicks when the
  probe FAILS (the old agent answers fine), so nothing does. See §6.4.

## 2. The rulings, with the reasoning that produced them

**P1 — No `tauri-plugin-updater`. The channel is rexenv's own signed document.**
All three designs, including the one briefed to maximise plugin reuse, refused the plugin's
install and relaunch on the same verified grounds:

- `install_inner` renames the bundle into a `tempfile::TempDir` that is **deleted on every
  return path**, has no restore step, and on *any* `PermissionDenied` runs
  `osascript … "rm -rf '<app>' && mv -f '<new>' '<app>'" with administrator privileges`
  (plugin `updater.rs:1325-1378`). Rust maps **both EACCES and EPERM** to `PermissionDenied`,
  so an App-Management refusal becomes a root `rm -rf` of the user's app with no backup.
  Upstream issues #3505 (data loss) and #3506 are exactly this. It is also a privileged
  prompt outside `PrivilegeManager`, which this project forbids.
- `AppHandle::restart()` on the main thread **bypasses rexenv's ONE quit gate**
  (`ExitRequested` → `confirm_quit_or_prompt`; `Exit` → tunnel/repo cleanup, ledger #436);
  off the main thread it can be cancelled by `prevent_exit()` leaving a thread sleeping
  forever with `restart_on_exit` latched. `process::restart` spawns the child *before*
  `exit(0)`, so the child's one-shot `connect()` in `claim()` can hand off to a dying parent
  and leave zero instances (ledger #441), and it inherits dead stdio (tauri#15742).
- `latest.json` is **unsigned** and its `version` is not bound to the signed bytes, so a
  compromised host can serve an old, validly-signed archive under a higher version string —
  a rollback the serial gate in `accept_with` (`updates.rs:495-525`) already closes for free.
- The plugin forces `TAURI_SIGNING_PRIVATE_KEY` into `tauri build` once a pubkey is
  configured, which puts the signing key on the build machine and makes RELEASING.md's
  "No secrets to create" false.
- Its TLS uses `rustls-platform-verifier` (the OS keychain), which trusts **rexenv's own
  installed CA** — whose private key is a `0600` file. rexenv's own client is reqwest 0.12
  with webpki roots.

What it would have bought — a streaming download with progress, safe extraction, semver
comparison — the tree already has: `binaries::download` (incremental sha256, Range resume,
bounded retries, hub progress), `binaries::extract_tar_gz_tree` (path- and symlink-bounded),
and `updates::segments`/`newer`. Cost of the in-house path: **zero new crates** (`tar`,
`flate2`, `sha2`, `ring`, `libc` are already direct dependencies).

**P2 — One signed document, one key, one ceremony.** The app's release descriptor is a second
signed document verified against the **same** compiled-in `RELEASE_PUBKEY`
(`core/updates.rs:60`) through a shared byte-verify seam, with its **own** monotonic serial.
A second ed25519 key would live on the same laptop, in the same reviewer-gated Environment,
approved by the same person — a ceremony, not a custodian. The honest cost is recorded as a
🚫 posture row: one key compromise takes PHP, Adminer and the app together. See §14 D1.

**P3 — The descriptor is a committed file on `rexenv/runtimes`, not a release asset.**
`app-manifest.json` + `.sig` on `main`, fetched from
`https://raw.githubusercontent.com/rexenv/runtimes/main/app-manifest.json`. Same shape as the
PHP manifest, and for the same recorded reason: a moved release tag broke in production once
(ledger #368). The URL is compiled in and **never flips** when `rexenv/rexenv` goes public,
because runtimes is public today and stays; the artifact URL is signed *data*, so the
going-public flip list gains zero compiled-in things. The cost is a **second publish click**
per release (§9), detected by `scripts/check-app-manifest.sh`.

**P4 — The app is not a `Family`.** `updates::Family`'s doc locks `manifest.json` to artifacts
that flow through `binaries::resolve*`, and `only_the_declared_families_are_nameable`
(`updates.rs:1144-1169`) pins the names to exactly `[php, php-fpm, adminer]`. An unknown name
would resolve as `Shape::Single` — chmod, `prepare_binary`, spawn as a service — the wrong
shape for a bundle. The app's grant is also strictly *above* PHP's (arbitrary native code as
the user, **plus** re-exec as the DNS agent and the tunnel guard, **plus** replacing the
binary that enforces the agent dial and `settings_access`), and the `Family` doc requires the
third addition to be argued the same way Adminer was argued as strictly *below*. It cannot
be. So: a separate document, separate serial, separate module; #348/#361 stay intact.

**P5 — The swap is ours, atomic, and never privileged.** Stage a verified sibling in the
bundle's own parent directory, then one `renamex_np(RENAME_SWAP)` syscall. Never a write
inside the launched bundle (EPERM even as root on Tahoe), never a copy-over (produces a
hybrid — `PUBLISH-TESTING.md:422-428`). Every pre-flight failure is a refusal naming the
consequence with a copy-paste fix; there is **no admin prompt**, because root does not bypass
App Management anyway and "system changes only through platform traits" is the rule.

**P6 — The relaunch goes through the ONE quit gate.** `app.exit(0)`, and the detached
relauncher is spawned from `RunEvent::Exit` — *after* the gate has already let the quit
through — so a user who answers "Keep sharing" leaves no helper waiting. The helper waits on
kqueue `NOTE_EXIT` for our pid (identity-checked with `process_start_token`) and only then
`open`s the bundle path: LaunchServices, clean stdio, launchd parent, and the single-instance
socket race is removed by construction rather than argued away.

**P7 — Install is a GUI click. Check is a read, everywhere.** No `rex update`, no MCP tool,
no timer path that installs. A self-update replaces the process that enforces the agent-access
dial and `settings_access`; the relaunch kills the caller's socket mid-call so an agent could
never observe the result; and `cloudflared` already runs `--no-autoupdate` in this tree for
the same class of reason (`core/tunnels.rs:945`). "No auto-update, ever" (ledger #350) extends
from PHP to the app.

## 3. What the updater must never do

Every line here becomes a ledger row in §12 and a code comment beside the thing it describes.

1. Never install bytes whose SHA-256 the signed document did not name.
2. Never trust TLS, a version string, or a stored "verified" flag — re-verify on every read.
3. Never accept a document whose serial is not greater than the highest ever accepted.
4. Never write inside the running bundle, and never copy over it.
5. Never leave the path without a complete bundle: the swap is one atomic syscall, or it is
   restored, or it never happened.
6. Never delete the previous bundle until the NEW process has launched and confirmed itself.
7. Never raise a privileged prompt — refuse with a fix instead.
8. Never install without a person's click in the GUI.
9. Never say "up to date" or "update available".
10. Never let the tray fetch, lock, or install.

## 4. Architecture

### 4.1 Modules

| New / touched | What it owns |
|---|---|
| `src-tauri/src/core/app_update.rs` (new, OS-free) | The trust boundary and every rule: URLs, allowlist prefixes, settings keys, `AppRelease`/`Offer`/`Phase`/`AppUpdateState` types, `enabled`, `fetch`, `accept`, `cached`, `offer_for`, `preflight_decision`, `consent_sentence`, `busy_sentence`, `apply`, `finish_at_launch`, the in-process offer snapshot. |
| `src-tauri/src/core/updates.rs` (refactor only) | `verify_signed_bytes(pubkey_hex, doc, sig_hex)` and `fetch_signed_pair(url, sig_url)` extracted and shared. Existing behaviour and all 17 tests unchanged. |
| `src-tauri/src/core/macho.rs` (+fn) | `pub fn archs(path) -> Option<Vec<Arch>>` — reads FAT/Mach-O headers in pure Rust (the file already parses them for the floor check), so "is it universal" needs no `lipo` shell-out in `core`. |
| `src-tauri/src/platform/traits.rs` | The 12th capability trait `AppBundle` (§4.2). `DnsAgentManager` unchanged; the agent-identity probe lives in `core/dns.rs`. |
| `src-tauri/src/platform/macos/app_bundle.rs` (new) | `MacosAppBundle`: bundle facts (`statfs`/`access`/`stat`/`statvfs`, Caskroom probe, translocation and `/Volumes` path tests), staging, Info.plist read, `codesign --verify --deep --strict`, `renamex_np(RENAME_SWAP)`, leftover sweep. |
| `src-tauri/src/platform/macos/relauncher.rs` (new) | `--relaunch-after <pid> <start-token> <bundle>`: identity check, kqueue `NOTE_EXIT`, then `open '<bundle>'`. 120 s cap. |
| `src-tauri/src/platform/{windows,linux}/mod.rs` | `AppBundle` stubs — `todo!()`, per the standing rule. |
| `src-tauri/src/commands/app_update.rs` (new, thin) | `app_update_state`, `app_update_check`, `app_update_apply`, `app_update_skip`, `app_update_unskip`. |
| `src-tauri/src/main.rs` | Third pre-Tauri dispatch arm: `--relaunch-after`, beside `--dns-agent` and `--tunnel-guard`. |
| `src-tauri/src/lib.rs` | Launch-sweep check step; 6 h poller; `finish_at_launch`; leftover sweep; relauncher spawn at `RunEvent::Exit`; tray model field; app-menu "Check for Updates…". |
| `src-tauri/src/core/dns.rs` | The agent answers `TXT _build.rexenv-agent.rex` with `"<version> <commit>"`; `agent_build_identity(port)`. |
| `src-tauri/src/core/tray.rs` | `TrayModel::update: Option<String>`, `TrayAction::UpdateTo(String)` (`tray:update:<v>`). |
| `src-tauri/src/core/downloads.rs`, `commands/downloads.rs` | `label_for("rexenv", v)`; a `retry_download` arm so the panel's Retry resumes the app download instead of trying to resolve a pinned binary. |
| `src-tauri/src/core/settings_access.rs` | The rulings in §4.4. |
| Frontend | `src/components/settings/AppUpdateCard.tsx`, five IPC wrappers + one event, DTOs, mocks, `UpdateWatch` + `CheckUpdatesMenuWatch` in `App.tsx`, a Settings nav badge, a `DevUiReview` view, `scripts/wk-checks/appupdate.js`. |
| Scripts | `scripts/release-assets.sh`, `scripts/check-versions.sh`, `scripts/check-app-manifest.sh`, `scripts/probes/app-swap-probe.sh`. |
| Other repos | `rexenv/runtimes`: `scripts/publish-app-manifest.sh` + `publish-app-manifest.yml`. `rexenv/homebrew-tap`: cask `auto_updates true`, README rewrite. |

### 4.2 The 12th trait

```rust
pub trait AppBundle: Send + Sync {
    fn facts(&self, exe: &Path) -> Result<BundleFacts>;
    fn stage(&self, facts: &BundleFacts, archive: &Path, expect: &StagedExpect) -> Result<StagedBundle>;
    fn swap(&self, installed: &Path, staged: &StagedBundle) -> std::result::Result<SwapReceipt, SwapFailure>;
    fn sweep_leftovers(&self, parent: &Path, my_version: &str, delete_previous: bool) -> Result<Vec<Leftover>>;
    fn spawn_relauncher(&self, pid: u32, start_token: &str, bundle: &Path) -> Result<()>;
    fn os_version(&self) -> Result<String>;
}
```

`BundleFacts { path, parent, kind: InstallKind, parent_writable, owned_by_me, same_device,
read_only, canonical, free_parent, free_appdata }`;
`InstallKind { Applications, UserApplications, HomebrewCask, DevBuild, DiskImage,
Translocated, Elsewhere(PathBuf) }`;
`SwapFailure { NotWritable, PolicyBlocked, CrossDevice, ReadOnly, Unsupported, Other(String) }`
— the EPERM→`PolicyBlocked` classification is a macOS fact and lives in `platform`, while the
*decision* it drives lives in `core` (so the decision is L0-testable on fixture facts).
`Platform` gains `fn app_bundle(&self) -> &dyn AppBundle`.

### 4.3 Data flow

**Check** (launch sweep, 6 h poller, "Check now", app menu): `fetch()` with no `Connection`
held → `accept(conn, doc, sig)` under ONE brief `state.db.lock()` (verify → serial gate) →
`offer_for(release, CARGO_PKG_VERSION, os_version, skipped)` → write
`app_update_check = {checkedAt, offered}` **only on success** → install the in-process
snapshot (tray, badge, `rex status`) → emit `app-update`.

**Apply** (one GUI click): `InFlight::claim("app")` → `facts()` → `preflight_decision` (every
refusal costs zero bytes) → `mkdir` the staging dir (the writability probe) → hub
`item_started("rexenv", v)` → stream to `<app-data>/updates/<v>/rexenv_<v>_universal.app.tar.gz`
with incremental sha256, `Content-Length == size`, 200 MiB cap, Range resume → digest ==
descriptor → extract → `verify_staged` → `swap` → persist `app_update_notice {from,to,at}` →
`item_done` → `app.exit(0)`.

**Quit gate** (unchanged): `ExitRequested` → `confirm_quit_or_prompt`; `Exit` → repo/tunnel
cleanup → **then** spawn the relauncher if the swap happened.

**Next launch** (new binary): `claim_at_startup` → AppState → `adopt_startup` → DNS agent
install + identity probe (kickstart if stale) → `sweep_leftovers(delete_previous=false)` →
… healthy … → `sweep_leftovers(delete_previous=true)` → `finish_at_launch` reads
`app_update_notice` and pushes a `StartupNotice` naming the version it reads **from itself**.

### 4.4 State, events, commands

Settings keys (KV rows — no migration; the Adminer precedent) and their `settings_access`
rulings:

| Key | Holds | `rex config` |
|---|---|---|
| `app_update_release` / `_sig` | The signed document and its signature, stored together | Denied |
| `app_update_release_serial` | The rollback high-water mark | Denied |
| `app_update_check` | `{checkedAt, offered}` in ONE value, written only after a verified fetch | ReadOnly |
| `app_update_notice` | `{from, to, at}`, consumed once by the new process | ReadOnly |
| `app_update_skipped` | A version string, compared LIVE against the offer | ReadWrite + `UNVALIDATED_BUT_SAFE` |
| `app_update_auto_check` | Absent or junk = on; exactly `"false"` = off | ReadWrite + `UNVALIDATED_BUT_SAFE` |

Commands: `app_update_state`, `app_update_check`, `app_update_apply`, `app_update_skip`,
`app_update_unskip`. Events: `app-update`, `menu://check-updates`; download progress rides the
existing `download-progress` hub snapshot (item id `rexenv-<version>`, label `rexenv <version>`).
Tray id: `tray:update:<version>`. Argv: `--relaunch-after`. Schema stays **v44**.

## 5. The trust boundary

Shaped exactly like `core/updates.rs` (#348), because that shape has already been argued and
tested:

- **Anchor.** ed25519 over the exact bytes of `app-manifest.json`, verified with `ring`
  against the compiled-in `RELEASE_PUBKEY`. Same dark-state rule: an empty key means
  `enabled()` is false, nothing is fetched, nothing is offered, and the card is absent.
- **Re-verified on every read.** `rexenv.db` is `-rw-r--r--`, so a stored verdict would put
  the decision where the attacker is. `cached()` re-runs the verification and degrades to
  "no offer" on any failure — it can never fail a screen.
- **Replay.** `app_update_release_serial` is a monotonic high-water mark with its own key:
  older → refused *before any write*; equal → accepted and nothing written (the ordinary
  every-launch state); newer → three keys written.
- **Location is never trust.** The artifact URL must sit under one of two compiled-in
  `releases/download/` prefixes (`github.com/rexenv/homebrew-tap/`, `github.com/rexenv/rexenv/`)
  — checked on the **pre-redirect** URL, since GitHub's final hop is a signed
  `release-assets.githubusercontent.com` URL that expires in about an hour.
- **Bytes.** Streamed to disk with incremental SHA-256 against the digest the *signed
  document* names, `Content-Length` compared to the signed size, capped at 200 MiB, and
  extraction begins only after the digest matches.
- **The staged tree must be what was promised**: `CFBundleIdentifier == dev.rexenv.rexenv`,
  `CFBundleShortVersionString == offer`, `CFBundleExecutable == rexenv`,
  `LSMinimumSystemVersion ≤ this OS`, `Contents/MacOS/rex` present, both Mach-Os fat with
  `x86_64 + arm64`, and `codesign --verify --deep --strict` clean. A signed-but-wrong archive
  (a stale tar, a thin build, a wrapper directory) is refused with the bundle untouched.
- **Rollback is bound two ways**: the serial, and the fact that the version lives *inside* the
  signed document next to the digest, then again inside the staged bundle's own Info.plist.
- **TLS is transport privacy, never trust** — rexenv's own reqwest 0.12 + webpki roots, so
  neither a corporate inspection root nor rexenv's own local CA can intercept it.
- **What the signature does not attest**, said in the module doc as `updates.rs` says it: it
  means "these are the bytes the maintainer signed", never "this build is safe". With the key
  on one laptop and in one reviewer-gated Environment, one compromise takes PHP, Adminer and
  the app together.
- **No privilege anywhere.** The bundle is user-owned, `/Applications` is `root:admin 775` so
  the rename is an ordinary user operation for an admin account, and every failure is a
  refusal with a fix.
- **Untrusted text.** Release notes are rendered as a text node, length-capped, never as
  HTML/Markdown and never interpolated into a native menu title.

## 6. The swap and the relaunch

### 6.1 Pre-flights (every one of them before a single byte is downloaded)

Path is `…/X.app/Contents/MacOS/rexenv`; `canonicalize(exe) == exe` (no symlink ancestor, or
`current_exe` and the swap disagree about the target); parent ∈ `{/Applications,
~/Applications}`; not under `/Volumes/`; no `/AppTranslocation/` component; parent volume not
`MNT_RDONLY`; parent writable by this uid; bundle owned by this uid; bundle and parent on the
same `st_dev`; free space ≥ 3× the archive on the parent volume and ≥ 2× in app-data; OS ≥ the
descriptor's `minimumSystemVersion`; no busy job registry; no live public share. Each refusal
message names the consequence and, where one exists, ends with a `$ …` line that
`toastBackendError` renders as a copyable command (`toast.ts:64-77`).

### 6.2 Stage → verify → swap

Extract into `<parent>/.rexenv-update-<token>/rexenv.app` (dot-prefixed so Finder and
Spotlight skip it; the same parent by construction, so `EXDEV` is impossible), `chmod 0755`
the root, verify per §5, then:

```
renamex_np("/Applications/rexenv.app", "<stage>/rexenv.app", RENAME_SWAP)
```

One syscall. Afterwards the install path holds the new bundle and the staging path holds the
previous one. `libc::renamex_np` and `RENAME_SWAP` are present in the pinned `libc 0.2`
(verified in the crate source on this machine). `ENOTSUP`/`EINVAL` → `Unsupported`; `EPERM` →
`PolicyBlocked` (§6.5 branch O2/O3); anything else → `Other`. **Nothing is deleted on any
failure**, and the running process keeps executing its old inode safely — Apple's own
guidance is that a rename-based replacement avoids the code-signing crash that an in-place
overwrite causes.

### 6.3 Rollback, stated precisely

- The previous bundle is deleted **only** by the new process, once it is healthy: AppState
  built, CLI socket claimed, `adopt_startup` finished, DNS answering.
- Leftovers are classified by the **version inside them**, never by a marker file: older than
  me → the previous bundle (keep until healthy, then delete); newer or equal → a staged
  leftover from an interrupted apply (delete now). Derived beats typed, so a crash between the
  swap and any write cannot mislead the sweep.
- The post-update notice states the version the new process reads from **itself**
  (`CARGO_PKG_VERSION`); if that disagrees with `app_update_notice.to`, the notice says the
  update did not take. Measured, not assumed.
- **Force-quit matrix.** During the download: nothing changed, the partial is kept for Range
  resume. During extraction: a staged tree with no swap, deleted at the next launch. During
  the swap: atomic — whichever bundle is at the path is complete. Between swap and exit: the
  new bundle is installed and the old process is still running its old inode; the user quits
  and reopens, and that launch completes the update. Helper killed: nothing happens; opening
  rexenv gives the new version.
- **If the new app will not open at all**, the previous bundle is still on disk and INSTALL
  carries the two-line restore (`mv` the broken one aside, `mv` the previous one back). No
  automatic rollback in v1 — the old process is gone by then and nothing can honestly watch.

### 6.4 The DNS agent, and the bug this fixes for everyone

`run_agent` starts answering `TXT _build.rexenv-agent.rex` with `"<version> <commit>"` on the
loopback port. At launch, after the plist install (which byte-compares and usually skips), the
app asks the running agent who it is; `None` (a pre-feature agent) or a different answer →
`kickstart()` once, which restarts the job in place with no Background-Task-Management
notification. This is a direct measurement rather than an mtime proxy, and it repairs the
manual drag-replace case as well as the self-update one. Cost: a sub-second `.rex` resolution
gap, documented in ARCHITECTURE.

### 6.5 T0 — the measurement the design branches on

`scripts/probes/app-swap-probe.sh` builds a throwaway `/Applications/RexSwapProbe.app` (its
own bundle id, ad-hoc signed), launches it through LaunchServices, and from inside it measures
with `errno`: rename-aside + rename-in (then restored), `renamex_np(RENAME_SWAP)`, and — for
contrast — an in-place write into `Contents/`. Alongside it, `log stream --predicate
'subsystem == "com.apple.TCC"'` is captured and filtered for `SystemPolicyAppBundles`, and a
human notes whether a "prevented from modifying apps" notification or a Gatekeeper dialog
appears; the probe then `open`s the swapped copy and records whether the new version ran. Both
a quarantined and an unquarantined leg are run. Nothing but `RexSwapProbe.app` is created or
removed, and the script refuses if any argument names `rexenv.app`.

| Outcome | What the plan becomes |
|---|---|
| **O1** both rename shapes work, no notification, clean relaunch | Ship as written; `RENAME_SWAP` primary, no fallback needed. |
| **O2** `RENAME_SWAP` → EPERM, the rename pair works | Two-rename becomes primary with an explicit restore on the second rename's failure; the window between them is recorded as a 🚫 premise and INSTALL gains the restore recipe. |
| **O3** both → EPERM (App Management blocks an ad-hoc self-replace) | `cp -R` the installed bundle into staging FIRST (so a restore exists), then `rm -rf` + rename, restoring on failure. Re-probed on the next macOS major; the ledger row states it rests on undocumented behaviour. |
| **O4** delete-then-create also EPERM | **No in-app install.** The check, the offer, the notification, the trust core, the surfaces and the release flow still ship; the button becomes "Download rexenv X" (the dmg, revealed in Finder) and the drag-replace stays. T3/T4's swap halves are cut. |
| **O5** the swap works but the "prevented from modifying apps" notification fires anyway | Ship; the consent sentence pre-warns and the message names the Allow path; a SMOKE leg re-checks per macOS major. |
| **O6** Gatekeeper dialog on the relaunched bundle | Dump xattrs: quarantine present (unexpected — the app writes it and sets no `LSFileQuarantineEnabled`) → strip in `verify_staged` (`xattr -dr`, the `prepare_binary` precedent) and re-probe; a dialog with no quarantine → treat as O4 until understood. |
| **O7** `mkdir` of the staging dir in `/Applications` fails for an admin user | Staging moves to `~/Applications/.rexenv-update-*` with an explicit same-device check. |

## 7. Failure catalogue

The spine the design was derived from. "core" = `core/app_update.rs` (OS-free), "plat" =
`platform/macos/app_bundle.rs`.

| # | Failure | Guard | Lives in | Proof |
|---|---|---|---|---|
| R1 | Mid-swap crash leaves no bundle; the KeepAlive agent respawns a missing binary | One atomic `RENAME_SWAP` — that state does not exist | plat | L1 sandbox (SIGKILL at each step); L3 |
| R2 | The swap syscall fails | Single syscall ⇒ the installed bundle is untouched by definition | plat → core | L0 errno→message; L1 |
| R3 | `EXDEV` | Sibling staging in the bundle's own parent + a `st_dev` pre-flight | plat facts, core decision | L0; L1 |
| R4 | Running from the dmg (`EROFS`) | Refuse before download: `/Volumes/` or `MNT_RDONLY` | plat facts | L0; L1 with a fixture image |
| R5 | Standard user — parent not writable | `access(W_OK)`; the staging `mkdir` is the same write | plat facts | L0; L1 (chmod 555 fixture) |
| R5b | `EPERM` at the swap despite `W_OK` — App Management | Refuse, **never** retry with privileges; wording set by T0 | plat classifies, core renders | T0 + L3 only |
| R6 | App Translocation | Refuse on an `/AppTranslocation/` path (no supported detection beyond it) | plat facts | L0; T0 quarantined leg |
| R7 | Opened through a symlink or alias | `canonicalize(exe) == exe` | plat facts | L0; L1 |
| R8 | Bundle owned by another login (brew installed by another user) | `st_uid == getuid()` | plat facts | L0; L3 |
| R9 | Multi-user Mac: other users' agents and TCC | No guard possible; each user settles at their next launch via §6.4 | — | 🚫 posture |
| R10 | Not in an Applications folder / a dev build | Parent allowlist; `InstallKind::DevBuild` → no button | plat + core | L0 |
| R11 | Low disk | `statvfs` 3×/2× | plat + core | L0 |
| R12 | Forged or unsigned descriptor; a higher version over old bytes | The signature binds `{serial, version, sha256, size, url, minAppVersion, minimumSystemVersion}`; the staged Info.plist must state the offered version | core | L0 with a real openssl fixture |
| R13 | Replay of an old signed descriptor | Monotonic serial; version strictly newer; three numeric segments; no prerelease | core | L0 |
| R14 | Compromised signing key | Not defensible in-app: custody, the click, rotation-by-release | — | 🚫 posture |
| R15 | Relaunch race with the socket lock; dead stdio | Helper spawned at `Exit`, waits `NOTE_EXIT` + start-token, then `open` | plat + main.rs | L1 sandbox; L3 for `open` |
| R16 | Live shares, in-flight jobs, terminals, in-process DNS | Shares and jobs **refuse** by name; terminals and in-process DNS **warn** in the consent sentence | commands + core | L0; L2; L3 |
| R17 | Stale DNS agent after any replacement | The agent's TXT build identity, checked at launch (§6.4) | core/dns + plat | L0; L1 sandbox; L3 |
| R18 | `--hidden` inherited into a clicked relaunch | The helper never passes `--hidden`; `first_window_decision` still applies at login | plat | L0 argv; L3 |
| R19 | Dishonest vocabulary | "Update to X (NN MB)" only from a verified offer; "checked N ago" only after a success; a sibling copy guard over the card file | core + copy_scan | L0 |
| R20 | The download is not what the descriptor says | Pre-redirect host prefix; `Content-Length == size`; incremental sha256; 200 MiB cap; extraction only after the digest matches | core | L0; L1 network |
| R21 | Malformed staged bundle | Structural extractor refusals + the §5 verification list | core + plat | L0; L1 sandbox |
| R22 | Offline or timeout | Launch check logs only; honest footer; bounded retries with Range resume | core | L1 network |
| R23 | A release that needs a newer macOS | `minimumSystemVersion` in the descriptor, re-checked on the staged Info.plist | core | L0 |
| R24 | TCC grants re-asked (the ad-hoc cdhash changes every build) | None possible; the consent sentence pre-warns | — | 🚫 posture; L3 |
| R25 | "Skip this version" hides a later release | The skipped version is compared LIVE to the offer; a manual check clears it | core | L0 |
| R26 | A stale or fetching tray | The tray reads an in-process snapshot only | lib.rs/tray | L0 |
| R27 | Force-quit at any stage | §6.3's matrix | — | L1 sandbox |
| R28 | Leftover `.rexenv-update-*` directories | Launch sweep by version | plat | L1 sandbox |
| R29 | Helper-flag drift across versions (the OLD binary spawns the helper) | `--relaunch-after` is a cross-version contract with a round-trip test | core argv | L0 |
| R30 | LaunchServices registering the hidden staging copy | Dot-prefixed directory; deleted once healthy; `open` the PATH, never `-b` | plat | L3 |

## 8. Surfaces and copy

**Settings → About**, a new `AppUpdateCard` between the hero line and `BuildFactsCard`, with
`data-probe="app-update"` plus `data-running`/`data-offered`/`data-checked-at`/`data-phase`/
`data-install-kind` for the L2 harness. States: *none* (`rexenv 0.5.0 is what you are running.`
+ `Checked 2h ago.`), *check failed* (`Couldn't reach the update server yet, so nothing here
says whether a newer rexenv exists.`), *offered* (mono `v0.6.0 · 31.2 MB`, a "What's new"
link to the release, the consent sentence **above** the ghost button `Update to v0.6.0`, and a
secondary `Skip this version`), *skipped*, *refused* (the pre-flight message rendered in place
with its copyable `$` line and **no** button), *needs a newer macOS* (a chip, no button),
*downloading/staging* (the hub's `Track` + `bytes / total · speed · ETA`, frozen on failure),
*installed* (`rexenv 0.6.0 is installed — reopening…`, or `…takes effect when rexenv next
opens` if the quit was cancelled).

The consent sentence has **one source** (Rust, in the state DTO — a TSX copy is a guard
failure, per DESIGN.md's rule): *"Downloads rexenv 0.6.0 (31.2 MB), verifies its signature and
checksum, swaps it into /Applications in one step, quits, and reopens on 0.6.0. Your sites,
databases and DNS keep running throughout — services outlive the app."* Conditional lines
append: the Homebrew note, `N public shares will stop when it quits.`, `3 open terminals will
close.`, `DNS is running inside the app right now; sites may not resolve for a few seconds.`,
and always: *"macOS may ask again for permissions it had granted this copy — rexenv has no
Apple developer signature yet, so each build has a new identity."*

**Other surfaces**, because this is a menu-bar app whose window is usually closed: a tray item
`Update to 0.6.0…` read from the in-process snapshot (it opens About; it never installs), an
app-menu `Check for Updates…`, a `toast.info` once per offered version with an *Open About*
action, a mono Settings nav badge, and — after the relaunch — a `StartupNotice` naming the
version the new process measured (`StartupNoticeHost` gains a `route` field, replacing its
hard-coded `/tunnels`).

Banned everywhere on the card, enforced by a sibling copy guard: "update available", "updates
available", "up to date", "up-to-date". Version and size are shown **before** the click;
progress moves only on real completions and freezes on failure; the "updated to X" sentence is
spoken by the **new** process only.

## 9. Release flow

Per release, interim (local-build) flow — RELEASING.md rewritten in the same change:

1. Bump the four manifests. 2. `verify-full.sh`. 3. `pnpm release:mac`, which now calls
`scripts/release-assets.sh` after the build: `check-versions.sh` (the four manifests **and**
the built `Info.plist`, all plain semver — brew's `auto_updates` comparison depends on it),
exactly one `.app` and one dmg, then
`COPYFILE_DISABLE=1 tar -C bundle/macos -czf rexenv_<V>_universal.app.tar.gz rexenv.app`,
assert exactly one top-level `rexenv.app/` entry and no `._` AppleDouble entries, extract to a
temp dir and **re-run §A0 on the extracted bundle**, write the `.sha256` sidecars, and print
the exact four-asset `gh release create … --draft` line. 4. §A0 + §A + SMOKE by hand.
5. Draft with four assets; verify each API `digest` against the local `shasum`; local tag.
6. Publish the tap release — still the §A sign-off. 7. **New:** `rexenv/runtimes` → Actions →
*Publish app update manifest* → dry run, then real: it reads the tap's `releases/latest` (so
it can never name a draft or a prerelease), finds the tar.gz by exact name, takes its immutable
API digest, re-downloads and re-hashes, records the size and `minimumSystemVersion`, increments
its own serial, signs with the reviewer-gated Environment key, self-verifies, refuses if the
pubkey is not the pinned one, and commits `app-manifest.json` + `.sig`. 8. `check-app-manifest.sh`
locally: verify the published pair against the pinned pubkey and warn when the tap's latest tag
is newer than the manifest — the forgotten-second-click detector.

`release.yml` mirrors steps 3–5 in the same commit. The tap's `update-cask.yml` selects assets
by `endswith("_universal.dmg")`, so the new names must never end that way — asserted in
`release-assets.sh`. The tap gains `auto_updates true` (brew ≥ 5 Apr 2026 then compares the
installed Info.plist and never downgrades on a plain `brew upgrade`) and a README rewrite that
also records what `--greedy` and `brew reinstall` still do.

**Going public** costs one variable in the runtimes publisher; nothing compiled into the app
moves. **0.5.0 cannot self-update to 0.6.0** — it has no updater — so the first in-app update
is 0.6.0 → 0.6.1, and that run is PUBLISH-TESTING §M.

## 10. What is deliberately NOT in scope

- Automatic install without a click — never. "No auto-update, ever" (#350) extends to the app.
- `tauri-plugin-updater`, `tauri-plugin-process`, and any `updater:*` / `process:*` capability.
- Windows and Linux `AppBundle` implementations (`todo!()` stubs, Phase 4 by rule).
- Delta updates, update channels, prerelease opt-in (prereleases are refused outright).
- A rollback button (the previous bundle is kept and the manual restore is documented).
- `rex update` or an MCP update tool — check is a read on `status`; install is a GUI click.
- An admin-privileged install path, a dmg-mount install path, or a standard-user install path.
- Developer ID signing and notarization (`docs/SIGNING.md`, blocked on a paid account). The
  design works without it *unless* T0 returns O4/O6, in which case the feature parks behind it.
- A schema migration — settings KV rows suffice; the schema stays v44.

## 11. Proof limits, written before the work

- **L0** proves the rules: signature, serial, offer comparison, URL prefixes, skip-live, the
  pre-flight decision over fixture facts, Mach-O header parsing, argv round-trips, the copy
  guards, the source scans, the settings rulings, the tray ids.
- **L1 sandbox** proves the mechanics on fixture bundles in a fixture parent: stage → verify →
  swap → restore → sweep, and the relauncher's ordering (the marker appears only after the
  parent is gone, and never on a wrong start token). A fixture bundle is single-arch and signed
  by the example, so `StagedExpect.archs` is parameterised and "universal" stays L3.
- **L1 network** proves the live chain against the real published document and the compiled-in
  key, including the same-serial no-op.
- **L2** proves the card's states and copy against mocked IPC, including a frozen bar.
- **L3 only** can see: the real `/Applications` bundle swapping itself under App Management,
  Gatekeeper on the relaunched ad-hoc bundle, which TCC prompts return, the BTM notification,
  the `--hidden` relaunch, `open` resolution with the previous bundle still on disk, and
  brew's Info.plist comparison. T0 measures the decisive one **before** the error handling is
  designed, which is the ledger's standing first step for third-party behaviour.
- Not claimed: "a failed install always leaves the bundle" — that is claimed for every path
  before the swap and for the atomic or restored swap, and nowhere else.

## 12. Claims this introduces (CLAIM-LEDGER rows, same commit as the code)

Next free row is **#517**. They land in a new section `## core/app_update.rs + commands/app_update.rs + platform AppBundle (self-update)`.

| # | Claim | Layer that can prove it |
|---|---|---|
| 517 | 🚫 Whether App Management lets an ad-hoc bundle rename itself in /Applications is undocumented — measured by T0, re-measured per macOS major; every update changes the cdhash, so TCC grants are re-asked | T0 + SMOKE |
| 518 | The app installs only bytes whose SHA-256 an ed25519-signed document names, verified against the compiled-in key over the exact bytes and re-verified on every read; TLS is transport, never trust | L0 + L1 network |
| 519 | A descriptor whose serial is not greater than the highest accepted is refused before any write; the high-water mark is its own key, Denied to `rex config` | L0 |
| 520 | An offer is a LIVE comparison — strictly newer, no prerelease, `minimumSystemVersion` ≤ host, not the skipped version — never a stored flag | L0 |
| 521 | The artifact URL must sit under a compiled-in `releases/download/` prefix, checked pre-redirect; the document is trusted for its signature, never its location | L0 |
| 522 | "Checked N ago" rests on ONE key holding timestamp and data, written only after a verified fetch; a failed check ages nothing | L0 |
| 523 | Auto-check is honoured before any I/O; a manual check always works; the request carries `rexenv/<version>` and nothing else | L0 |
| 524 | The bundle is replaced only by a whole-bundle atomic swap of a verified sibling on the same volume — never a write inside the launched bundle, never a copy-over | L0 + L1 sandbox + L3 |
| 525 | Every error path before the swap leaves the installed bundle untouched; the swap is atomic or restored; the previous bundle survives until the NEW app has launched and confirmed its own version | L1 sandbox + L3 |
| 526 | The updater never raises a privileged prompt: an unwritable parent, a foreign owner, a /Volumes or translocated path, a read-only volume or a symlink ancestor is a refusal with a copy-paste fix | L0 + L3 |
| 527 | `core/` stays OS-free for self-update: every filesystem, codesign, launchctl and `open` call lives behind `AppBundle` (#163 canary extended) | L0 source scan |
| 528 | An update is applied only from a GUI click: no CLI arm, no MCP tool, no launch-time or timer path calls apply | L0 source scan |
| 529 | The relaunch passes through the ONE quit gate; `AppHandle::restart`/`process::restart` are never called; the helper waits for THIS pid (start-token checked) to be gone before `open`, so the single-instance socket can never be handed to a dying parent | L0 + L1 sandbox |
| 530 | An apply is refused while a public share is live or a job is running, naming each — nothing is killed silently | L0 + L2 |
| 531 | One apply at a time, and the download reports into the ONE download hub | L0 |
| 532 | The DNS agent never keeps serving an older build than the app — the agent states its build and the app kickstarts a stale one at launch (this also repairs the manual drag-replace case) | L0 + L1 + L3 |
| 533 | The card never says "up to date" or "update available"; the consent sentence has ONE source and sits in front of the button; progress moves only on real completions and freezes on failure | L0 + L2 |
| 534 | The tray reads the offer from an in-process snapshot — never fetches, never locks — and its item opens About; it never installs | L0 |
| 535 | Release assets never end in `_universal.dmg` except the dmg; the tar has exactly one top-level `rexenv.app/` entry and no AppleDouble entries; §A0 re-checks the EXTRACTED bundle | L3 scripted |
| 536 | 🚫 The app descriptor key IS the PHP manifest key: one custody, one rotation, and one compromise takes PHP, Adminer and the app together | posture |

## 13. Task list

One task, one commit, docs in the same commit. **Human gates** are marked: the agent cannot
run them.

### T0 — Measure the swap on a real Mac before designing its error handling (HUMAN GATE)
- **Done when:** `scripts/probes/app-swap-probe.sh` + `examples/app_swap_probe.rs` (tier
  `demo`) build, launch and drive `/Applications/RexSwapProbe.app` through the three shapes in
  §6.5, quarantined and not; the log, the TCC capture, the notification observation and the
  relaunch result are pasted into this file as **§T0 result** with the macOS build; the
  outcome letter (O1–O7) is recorded and the affected sections of §6 are rewritten to match.
- **Files:** `scripts/probes/app-swap-probe.sh`, `src-tauri/examples/app_swap_probe.rs`,
  `scripts/live-checks.sh`, `docs/PLAN-self-update.md`, `docs/TESTING.md`
- **Docs:** TESTING §5 manual row; the tier line; this plan's §T0.
- **Ledger:** #517 (forward-recorded 🚫 posture).

### T1 — The trust core: `core/app_update.rs`, the shared verify seam, `macho::archs`, the settings rulings
- **Done when:** `verify.sh` is green with new sentence tests — an offer is strictly newer, a
  prerelease is never offered, a skipped version is compared live, the serial is its own
  high-water mark, a tampered cache is refused on every read, the URL prefix is not a bare
  host, a release needing a newer macOS is not offered, both manifest modules verify through
  one seam (plant: flip one byte, both refuse), and `archs` reads a synthetic FAT header —
  while `updates.rs`'s existing 17 tests are untouched; the six settings rulings land and the
  #344 guard stays green.
- **Files:** `core/app_update.rs`, `core/updates.rs`, `core/macho.rs`, `core/mod.rs`,
  `core/settings_access.rs` + ARCHITECTURE, MAP, README tree, CLAIM-LEDGER, TESTING, TODO, STATUS.
- **Docs:** ARCHITECTURE §7 gains the second signed document **and** corrects "www.php.net is
  the ONE read-only egress"; MAP subsystem row; ledger #518–#521 + tally.
- **Depends on:** T0.

### T2 — Transport, the launch ride, the 6 h poller, the check commands, and a minimal About card
- **Done when:** `verify.sh` green including both reachability guards; the log shows the
  manifest accepted or the check skipped with a reason at launch; `rex config` answers for the
  auto-check key and refuses the serial with its ruling text; the card's footer keeps
  yesterday's timestamp when the network is cut (never "checked just now");
  `examples/app_update_check.rs` (network tier) proves fetch → verify against the compiled-in
  key → accept → same-serial no-op → offer against a fixture running-version.
- **Docs:** INSTALL's "ONE routine outbound request" rewritten to the honest list with the
  opt-out; ARCHITECTURE cadence paragraph; the `StartupNotice` `route` field; MAP's IPC export
  count; ledger #522–#523.
- **Depends on:** T1.

### T3 — The 12th trait `AppBundle`: facts, pre-flights with fixes, stage + verify, swap, sweep
- **Done when:** `verify.sh` green with #163's canary extended to the new platform file; L0
  tests on every pre-flight predicate over fixture facts; `examples/app_bundle_swap_check.rs`
  (sandbox tier) builds two fixture bundles under the sandbox root, stages, swaps, asserts the
  new version is at the path and the previous is in staging, sweeps by version, proves a failed
  verification leaves the original byte-identical, and proves each refusal's fix text — never
  touching `/Applications`.
- **Docs:** "11 Rust traits" → 12 everywhere it is counted (CLAUDE.md included); ARCHITECTURE
  §8 "Self-update: the swap"; MAP + README tree; ledger #524–#527.
- **Depends on:** T0, T1.

### T4 — Apply end to end: refusals, hub download, stage, swap, notice, the helper, exit through the gate, `finish_at_launch`
- **Done when:** three source-scan L0s are green (never `restart`, GUI-only apply, the exit
  goes through the gate), plus a refusal test over fixture registry state;
  `examples/app_relaunch_check.rs` (sandbox tier) proves the helper writes its marker only
  after the parent is gone and never on a wrong start token; the download appears in the
  existing footer and panel, and Retry resumes it.
- **Docs:** ARCHITECTURE §8 "the relaunch" (gate, helper, socket race, refusals, the
  cancelled-quit state, `--hidden`); the stale `lib.rs` "no in-app updater" comment corrected;
  #436/#441 amended to cite the helper; ledger #528–#531.
- **Depends on:** T2, T3.

### T5 — The DNS agent states its build, and a stale one is kickstarted at launch
- **Done when:** the agent answers the build TXT on loopback; the launch path kickstarts on a
  mismatch and logs it; L0 on the pure compare; `examples/dns_agent_identity_check.rs`
  (sandbox tier) serves on a fixture port and queries it.
- **Docs:** ARCHITECTURE's DNS paragraph — the byte-compare skip is not enough, and why;
  ledger #532.
- **Depends on:** T4.

### T6 — The full card, the one-source consent sentence, hub progress, skip, the watcher, the badge, the copy guard, the L2 harness
- **Done when:** the sibling copy guard and a "the consent sentence appears in Rust and in no
  `.tsx`" guard are green; `verify-full.sh` green with `scripts/wk-checks/appupdate.js`
  asserting every state at both widths — version and size on *offered*, the copyable fix on
  *refused*, a frozen bar on *failed*, no button on *none*/*skipped*, and none of the four
  banned phrases anywhere.
- **Docs:** DESIGN honest-UI bullet; TESTING L2 + the wk-checks README row; ledger #533.
- **Depends on:** T4.

### T7 — The tray item and the app-menu "Check for Updates…"
- **Done when:** the tray tests are extended and green (round-trip, unknown/empty id,
  uniqueness, bootstrap unchanged, still exactly five routes, and the item present iff the
  model offers a version); the tray's must-not scan gains `app_update_apply`; the model reads
  the in-process snapshot, asserted by a source scan.
- **Docs:** ARCHITECTURE tray paragraph; a SMOKE leg; ledger #534.
- **Depends on:** T2.

### T8 — The read surface for `rex` and MCP, with no new dispatch arm
- **Done when:** `rex status` prints one update line when offered and nothing otherwise;
  `--json` and MCP `stack_status` carry the same field at Read level; no new verb, no
  completions change, no count bump; `cli_socket_check` re-run after the rebuild.
- **Docs:** CLI-ROADMAP note row explaining that `rex update` is deliberately not built.
- **Depends on:** T4.

### T9 — The release flow, and every release doc that becomes WRONG (HUMAN GATE)
- **Done when:** a local `pnpm release:mac` produces the tar.gz and sidecars beside the dmg
  with the asserted layout, the extracted bundle passes §A0, and a deliberately mismatched
  version makes `check-versions.sh` exit 1 naming the file; `check-app-manifest.sh` fails on a
  planted tampered signature and warns when the tap is ahead; `release.yml` mirrors the script.
- **Docs:** RELEASING (steps 3/5/7, "Going public later", "Rules the pipeline encodes"),
  PUBLISH-TESTING (§A0 additions, the §A record shape, a new §M with its summary row),
  SMOKE-TEST section, INSTALL §Updating rewritten, TODO:382's stale "three things"; ledger #535.
- **Depends on:** T4.

### T10 — The runtimes publisher and the tap changes (HUMAN GATE, other repos)
- **Done when:** `rexenv/runtimes` has `publish-app-manifest.sh` + its `workflow_dispatch`
  workflow (dry-run default, `manifest-signing` environment, its own concurrency group,
  self-verifying, refusing a pubkey mismatch), and a dry run against v0.5.0 refuses cleanly
  because that release has no tar.gz; `rexenv/homebrew-tap` has `auto_updates true`, a rewritten
  README, and `brew style` clean. This repo's commit records both and ticks the row.
- **Ledger:** #536 (🚫 posture).
- **Depends on:** T9.

### T11 — The first real in-app update on a real Mac, 0.6.0 → 0.6.1 (HUMAN GATE)
- **Done when:** PUBLISH-TESTING §M and the SMOKE section are recorded on this Mac and on a
  clean account: quarantined dmg install, the offer arriving, the apply, real bytes in the
  footer, the swap, a relaunch with no Gatekeeper dialog, no quarantine xattr on the new
  bundle, the DNS agent pid changed and answering, `rex --version` agreeing with About, the
  leftover gone, both LaunchAgents still enabled, which TCC prompts returned, and a plain
  `brew upgrade` doing nothing on the brew account. Every ledger row from #517 gets its final
  verdict.
- **Depends on:** T5, T6, T7, T8, T9, T10.

### T12 — Archive the plan as a design record
- **Done when:** the Status line reads shipped with the row range and the live proof; the file
  is `git mv`'d to `docs/archive/`, the archive README table and the CLAUDE.md router
  parenthetical gain it, every citation is repointed, and `doc-counts.sh` + `status.py --check`
  are green.
- **Depends on:** T11.

## 14. Decisions the owner may want to overrule

- **D1 — one key or two.** This plan reuses `RELEASE_PUBKEY` (P2). The alternative is a second
  ed25519 key that never enters CI at all, which is *better custody for the more powerful key*
  but doubles the ceremony and reintroduces the bus factor RELEASING.md explicitly rejected.
  Cheap to change before T1 lands; expensive after.
- **D2 — where the descriptor lives.** This plan commits it to `rexenv/runtimes` (P3), which
  costs a second publish click per release. The alternative is signing it locally at build time
  and attaching it to the tap's draft release: one click, the §A gate preserved by construction,
  but a compiled-in tap URL and a laptop-only key.
- **D3 — how loud the notification is.** This plan uses a toast once per version plus a tray
  item and a badge. A quieter option is badge-only; a louder one is a window that opens itself,
  which the Accessory activation policy and Sparkle's convention both argue against.

## §T0 result

_Not run yet. T0 fills this in with the raw log, the macOS build, the outcome letter, and which
sections of §6 were rewritten because of it._
