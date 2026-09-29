# PLATFORMS — one feature, three operating systems

rexenv is built for **macOS, Windows and Linux (Ubuntu)**, and every new feature or fix is
built for all three **in the same change**. This file is where that rule becomes concrete:
what is written ONCE for every OS (§1), where an OS difference goes (§2), how each OS reaches
the same promise (§3), the checklist a feature walks (§4), how a claim is proven per OS (§5),
and each OS's own traps (§6–§8). `CLAUDE.md` states the rule; this file is the reference.

**"All OSes" / "every OS" means exactly THREE — macOS, Windows and Linux — everywhere in
rexenv's docs, scripts and skills.** Never two of them. (The live-check OS value that means it
was spelled `both` until 28 Sep 2026, a leftover of the two-OS days; it is `all` now.)

**Why this file exists — what one-OS work cost.**
- 20 Sep 2026: four Windows-only defects shipped to a user in one week (#698, #699, #701, #703),
  each written for macOS and merely *compiled* for Windows. The Database Browser pointed at
  `rexdb://localhost`, a URL no Windows webview can load, from a file that said "Windows …
  Phase 4". Owner ruling that day: both OSes, same change — widened to all three (macOS,
  Windows, Linux) when the Linux build landed.
- 21 Sep 2026: the first `verify.sh` run on Windows failed two TESTS that asserted macOS's
  spelling (`$ sudo chown` where Windows says `takeown /R /F`; a `:`-joined PATH).
- 24 Sep 2026: the first Linux DNS design (a global systemd-resolved drop-in) routed the
  WHOLE internet to `127.0.0.1` on the VM — a mechanism that looked like the macOS one and was
  not (ledger #717).
- Linux was then built on the same traits in two days, because W1/W2/W6 of the Windows port had
  already moved every OS leak behind `platform/`. Keeping that true is the point of §1–§2.

---

## 1. Common — written ONCE, for all three OSes

These are OS-neutral. They live in `core/` (or the frontend) and never branch on the OS.
A change here is automatically a change on all three — which is exactly why an OS spelling
must never leak into them.

- **The product rules** — site model, request topology (edge `:443` → shared Nginx
  `:18088` → one PHP pool per version → app → DB), config generation (three rewrite templates,
  every path quoted), SQLite app state and migrations, the binary catalog's pins + digests,
  port gating (`core/ports::ensure_free`), ownership AND liveness for "running", the locking
  rule, services outliving the app, the MCP registries, the `rex` command set.
- **`core/` names no OS.** No `cfg(target_os)`, `cfg(unix)`, `std::os::unix` or
  `Command::new("kill")` in production `core/` — the scan test
  `core_production_code_never_names_an_os_or_carries_an_os_cfg` (in `platform/traits.rs`) fails the build (ledger #163). When
  `core/` needs to know something only the OS knows, it asks the platform for a CAPABILITY
  (`may_spawn_with_admin_token`, #698) — never "am I on Windows?".
- **The frontend is OS-blind.** TSX renders what Rust sends. A sentence that states a RULE or
  names an OS mechanism (the consent text, the restart text, "Show in Finder") comes from Rust
  (`platform/words.rs`), and `core/copy_scan.rs` guards it. The only OS fact the UI reads is a
  flag Rust sends (`window_controls_in_content`, `imports_other_tools`).
- **Tests assert what the PLATFORM says** — `platform::words::current()`, the code's own
  separator, the platform's own command — never one OS's literal.
- **The docs are common by default.** A section describes the behaviour; the per-OS mechanism
  is the §3 table or an OS section of the doc (see §4 step 6).

## 2. Different — where an OS difference goes (the decision table)

| The thing that differs | Goes in | Shape to copy |
|---|---|---|
| A **value**: path, URL/origin, command line, separator, refusal text, user-facing sentence | a field on `PlatformWords` in `platform/words.rs` — **all three constants filled** (`MACOS`, `WINDOWS`, `LINUX`) | `db_browser_origin`, `reveal`, `path_sep`, `take_ownership` |
| A **behaviour**: how DNS is routed, how a prompt is raised, how a process is found | a method on one of the 13 traits in `platform/traits.rs`, implemented in `platform/{macos,windows,linux}/` | `DnsManager`, `PrivilegeManager`, `EdgeSupervisor` |
| A **yes/no the core must branch on** | a capability method on a trait (not an OS check) | `may_spawn_with_admin_token` (#698) |
| **Pure text or rules** an impl produces (a unit file, a registry value, an NRPT line) | a `*_rules.rs` / pure module inside the OS folder, unit-tested on ANY host (`platform/mod.rs` includes them under `cfg(test)`) | `windows/autostart_rules.rs`, `linux/units.rs`, `linux/dnsroute.rs` |
| A **pinned download** | an OS arm in `binaries::manifest(…, os, arch)` + a row in `docs/PORTS.md`'s table for that OS | the Windows and Linux artifact tables in PORTS |
| **Shared by macOS and Linux** (both Unix) | a neutral module in `platform/` both call — not a copy | `platform/resolver_files.rs` (macOS resolver files = Linux route markers), `APP_QUALIFIER`/`APP_ORG`/`APP_NAME` |
| An OS that **cannot** do it yet | `Error::Unported("<what>")`, or `unported!("<what>")` where the method cannot return an error — **never `todo!()`** (#595, widened to Linux #716); plus a TODO row and an honest refusal in the UI | `LinuxAppBundle` bundle trees |
| An OS that **will not** do it (a decision) | a refusal in `core/` with a sentence from `words.rs`, recorded in that OS's "not in v1" section of INSTALL + SMOKE | Linux D-L8 list |

`cfg(target_os)` outside `platform/` is allowed only for app-shell wiring (`lib.rs`, `main.rs`:
tray, dock policy, the About menu, the relauncher) and each one is inventoried in
`docs/PLAN-linux-port.md` §1.1. A new one gets a row there saying what the OTHER two OSes get.

## 3. The same promise, three mechanisms

The user-visible promise is the common row; the columns are how each OS keeps it. Mechanism
detail and the reasoning live in the linked plan and the OS's code; this is the map.

| Promise | macOS | Windows | Linux (Ubuntu) |
|---|---|---|---|
| Supported floor | 13+ (13/14 = legacy pin tiers, `PLAN-macos-13-floor`) | 11 x64; 10 22H2 best-effort (D6) | 22.04+ x86_64 / aarch64 (glibc ≤ 2.34 asserted) |
| Webview (what the UI really runs in) | WKWebView | WebView2 | webkitgtk |
| `*.rex` routed to the resolver | `/etc/resolver/<tld>` (root) | NRPT rule `.rex → 127.0.0.1` (UAC) | dummy link `rexenv0` carrying `192.0.2.53/32` + `resolvectl` link-scoped domains; markers `/etc/rexenv/dns.d/<tld>`; `rexenv-dns-route.service` (#717, #734) |
| Resolver port (`platform::RESOLVER_PORT`) | UDP 15353 | UDP **53** (NRPT has no port field — D2) | UDP 15353 |
| DNS agent that outlives the app | per-user LaunchAgent (`--dns-agent`) | scheduled task `\rexenv\dns-agent` at logon | systemd **user** unit |
| Edge on `:443` | root LaunchDaemon | unelevated user process, `127.0.0.1` only (no Firewall alert) | systemd **system** unit + wrapper (#718) |
| Edge admin (never TCP 2019) | unix socket `0600` | AF_UNIX socket via Winsock (#611) | unix socket `0600`, owned by the user across reloads |
| Privileged step | osascript admin prompt, **foreground** | UAC (`elevation.rs`) | `pkexec` + rexenv's polkit action (#723); absolute paths (`pkexec` strips `PATH`) |
| Boot/login files survive a power cut (#740) | the step's shell ends in `/bin/sync` (`durable::flushed_script`); the login item + DNS agent plists via `write_durable` | nothing to do: registry, NRPT, cert store, Task Scheduler are the OS's durable stores | the step's shell ends in `/bin/sync`; the autostart entry + DNS user unit via `write_durable` |
| CA trust | user's **login keychain** | `Cert:\CurrentUser\Root` — never LocalMachine (#613) — + Firefox root | system CA store + NSS dbs, incl. snap Chromium's (#726) |
| Autostart | macOS login item | HKCU `Run` + `StartupApproved` (#623) | XDG `~/.config/autostart` |
| PHP pool (`pool_kind`) | php-fpm, one per version | php-cgi group | php-fpm, one per version |
| App data | `~/Library/Application Support/dev.rexenv.rexenv` | `%LOCALAPPDATA%\rexenv\rexenv\data` | `$XDG_DATA_HOME/rexenv` (`~/.local/share/rexenv`) |
| `rex` CLI + MCP transport | unix socket | named pipes (#620, #632); the edge dial is AF_UNIX | unix socket (#727) |
| `rex` on PATH | symlink, one admin prompt | a **copy** in `%LOCALAPPDATA%\rexenv\bin` on the user Path, no prompt (#634) | one admin prompt |
| Binary preparation (`prepare_binary`) | de-quarantine → relink Homebrew dylibs → ad-hoc codesign **last** | strip Mark-of-the-Web → refuse a non-runnable PE (`pe.rs`) | set executable; soname shims in `lib-compat/` at spawn (`libaio.so.1t64`, #731); bottle trees `Unported` (D-L8) |
| Installer / in-app update | `.dmg` + `.app.tar.gz`, Homebrew cask | unsigned NSIS per-user `setup.exe` + zip | `.deb` (`dpkg -i` under polkit) / AppImage (`renameat2` exchange) (#729/#730) |
| One-command install (`install.sh` / `install.ps1` in `rexenv/homebrew-tap`, `docs/archive/PLAN-install-scripts.md`) | `curl` fetches the `.app.tar.gz` → `/Applications`; curl writes no quarantine, so no Gatekeeper dialog; `xattr -dr` anyway, as the cask's postflight | `Invoke-WebRequest` fetches `setup.exe` — no Mark of the Web (measured Win10 22H2 + Win11 24H2), so no SmartScreen dialog — then `/S`: per-user, no UAC. Smart App Control, where on, still blocks | `apt-get install ./…deb` under `sudo` (Depends resolved); no apt → AppImage in `~/Applications`. No gate to meet |
| Compile gate in `verify.sh` | native | `windows-check` (cargo-xwin) — #584 | `linux-check` (Ubuntu 22.04 container) — #719 |
| Where a RUN happens | the dev Mac; UTM macOS 13.6→15.8 VM | the Dell (Win10) + Win11 ARM VM; CI `windows-verify.yml` | Ubuntu 22.04 arm64 UTM VM; Dell WSL2 (x86_64); CI `linux-build.yml` |
| Human checklist | `SMOKE-TEST.md` main body | `SMOKE-TEST.md` § Windows | `SMOKE-TEST.md` § Linux |
| Install guide | `INSTALL.md` § macOS | `INSTALL.md` § Windows | `INSTALL.md` § Linux (Ubuntu) |

When a row here changes (a new mechanism, a new host), update it in the same commit.

## 4. The checklist — every feature, every fix

Walk it while DESIGNING, not after. Say the answers out loud in the plan or the commit.

1. **What is the common part?** Write it in `core/` (or the frontend) once. If you catch
   yourself writing "on macOS …" inside `core/`, stop — that is §2's job.
2. **What differs per OS?** For each difference pick its home from §2. Fill **all three**
   answers in the same change. If one OS truly cannot, it returns `Error::Unported` / a
   decided refusal with a sentence — never a silent no-op, never `todo!()`, never "later".
3. **Does it touch a mechanism in §3?** Then read that OS's column and §6–§8 before writing:
   the traps there are ones that already shipped once.
4. **Both ends of every policy.** A CSP, a frame, a header, a URL shape is enforced in more
   than one place; check every end on every OS (#699 + #702 + #703 were all needed for ONE
   panel on Windows).
5. **Tests assert the platform's answer**, and pure per-OS rules get a unit test that runs on
   any host (the `*_rules.rs` shape) — so a Mac-run `verify.sh` still exercises Windows' and
   Linux's text.
6. **Docs, per OS, same commit.** Behaviour → the common section of the doc. Mechanism →
   §3 above and the OS's section in `INSTALL.md` / `SMOKE-TEST.md` / `PORTS.md`. A step the
   main SMOKE body describes with a Mac mechanism needs its Windows and Linux equivalent in
   those sections, or a line saying it is the same there.
7. **Prove it per OS** (§5), and let the ledger row say which OS the proof came from.

## 5. Proving it — compile is not a verdict

- `verify.sh` runs the native build plus **two compile gates**: `windows-check` and
  `linux-check`. A green bar says the tree BUILDS for all three; it says nothing about whether
  the feature WORKS there. A gate that prints `SKIPPED` proved nothing for that OS — say so.
- **A behaviour claim needs a run on that OS**: `docs/TESTING.md` §"Proving a Windows claim"
  and §"Proving a Linux claim" list the ways, cheapest first (headless example over SSH →
  the real installed package → the GUI).
- **Examples declare where they can run** — the third column of `scripts/live-checks.sh`:
  `all` (macOS, Windows AND Linux), `macos`, `windows`, `linux`. An example whose logic is
  OS-neutral is `all` — and `all` is a claim that it RUNS on all three, so an `all` example
  nobody has run on Windows or Linux is a claim to check, not a fact. Its fixtures use `common::fixture_base()`
  (`/private/tmp` on macOS, `/tmp` on Linux, the temp dir on Windows), never a literal path.
- **Ledger verdicts name the OS.** ✅ means proven on every OS the claim covers. Proven on one
  OS only = `◐ (macOS only)` / `◐ (Docker only)` etc., and the missing OS's run is a row in
  that OS's SMOKE-TEST section until someone runs it.
- **Release**: one tag builds every OS on GitHub Actions (`docs/RELEASING.md`); §A0 runs per
  OS in CI (`release-windows-check.sh`, `release-linux-check.sh`); the human gates are per OS.

---

## 6. macOS — the traps

- **Binary preparation order is fixed:** de-quarantine → relink Homebrew dylibs to `/usr/lib`
  → ad-hoc codesign **LAST** (a relink after the signature invalidates it).
- **Privileged prompts run in the foreground.** A backgrounded osascript cannot show the
  auth dialog; it just hangs.
- **CA trust is a USER op into the login keychain.** The System keychain is unreachable from
  a detached-root osascript.
- **TLS leaves ≤ 398 days** — Safari/WebKit rejects longer leaves even with the CA trusted
  (a common rule, enforced for every OS, but the reason is WebKit).
- **WKWebView is the shipping engine**, not Chrome: ITP-blocked iframe cookies, custom-scheme
  302s never followed, no JS dialogs in wry. Check UI in `scripts/wk-checks/` (WebKit).
- **Quarantine is written by the DOWNLOADING app**: a browser sets `com.apple.quarantine`,
  `curl` and reqwest do not (measured 16 Aug 2026). The in-app update and the one-command
  install stand on that; Homebrew is the exception — it adds quarantine and its cask's
  postflight strips it again. A synthetic `xattr -w` is how PUBLISH-TESTING §A fakes a download.
- **Legacy tiers**: 13 and 14 resolve older pins (`PinSet::for_tier`); a feature that needs a
  newer binary says so in the tier's words (`needs_newer_os`).
- Mechanism detail: `docs/ARCHITECTURE.md` (written against macOS), `docs/INSTALL.md` § macOS.

## 7. Windows — the traps

- **A bare `cargo`/`cargo-xwin` `rexenv.exe` is a DEV build** — its webview loads
  `localhost:1420` and looks broken. Only `tauri build` (on the Dell) makes a testable app;
  cross builds are for examples.
- **The resolver answers on :53** (NRPT has no port). A UI line saying 15353 on Windows is
  macOS's number leaking.
- **`hosts` is never touched** — shared by every tool on the machine.
- **NRPT has no fallback** — with the agent down, `.rex` fails rather than asking the interface's
  server, so a host running rexenv upstream cannot mask a broken Windows route (measured on the
  Win11 VM, 28 Sep 2026 — unlike Linux, §8).
- **Trust goes to CurrentUser\Root, never LocalMachine** (#613). Windows shows its own
  certificate dialog; a No is a cancel, and setup offers the step again.
- **Bind `127.0.0.1`, not all interfaces** — an all-interfaces bind raises the Firewall alert
  and exposes sites to the LAN.
- **Process-wide state mutation beside a spawn races other threads' spawns** (#600's handle
  sweep broke concurrent children): mutate global state only on one thread at main start.
- **Job objects**: the installer's Finish page and SSH shells start rexenv inside a job; the
  start-up hop out of it is `job_guard` (#692). **Explorer passes no arguments**, so the hop
  carries `--hidden` in a one-shot marker (`hop-hidden`, ≤ 20 s, #742) — any other flag a
  confined launch had is lost.
- **Unsigned** (owner ruling, D5): a BROWSER-downloaded `setup.exe` meets SmartScreen, and its
  text is documented, not worked around. The one-command install meets none — `Invoke-WebRequest`
  writes no Mark of the Web, which is what SmartScreen keys on (measured 28 Sep 2026; the owner
  took that path the same day, `docs/archive/PLAN-install-scripts.md` D2). `install.ps1` never calls
  `Unblock-File` and never touches Defender: "download, unblock, run silently" is the shape AMSI
  flags in a script piped to `iex`. **Smart App Control**, where on, blocks unsigned apps
  either way — no command changes that.
- **An interactive Windows PowerShell is not the one CI runs.** A person typing `irm … | iex`
  gets PSReadLine 2.0.0, and in that session `[System.Runtime.InteropServices.RuntimeInformation]`
  resolves to PSReadLine's internal class — `OSArchitecture` reads `$null`. CI, SSH and scheduled
  tasks are non-interactive and load none of it (`Import-Module PSReadLine` there does not
  reproduce it either). The first `install.ps1` refused every Windows desktop that way (fixed
  29 Sep 2026, tap #2). A PowerShell claim meant for a person is proven in a `-NoExit` session
  on a signed-in desktop.
- **An App Control policy runs PowerShell in ConstrainedLanguage** — Smart App Control is one.
  There, setting a static property or calling a method on a non-core .NET type fails ("… is
  supported only on core types in this language mode"), and even `Get-FileHash` fails (measured
  on the Dell, PS 5.1). A script for `iex` checks `$ExecutionContext.SessionState.LanguageMode`
  before its first .NET call. **Smart App Control cannot be switched On for a test:** it turns On
  only after a clean install's evaluation, and an install where it is Off resets a hand-set
  `VerifiedAndReputablePolicyState = 1` at the next refresh (measured on the Win11 VM, 29 Sep 2026).
- Proof hosts, SSH, the click helpers: `docs/TESTING.md` §"Proving a Windows claim";
  design record `docs/PLAN-windows-port.md`; user side `docs/INSTALL.md` § Windows.

## 8. Linux — the traps

- **Never a global resolved drop-in** — it sends every name to rexenv's loopback resolver.
  DNS routing is the dummy link with link-scoped domains (#717). P1 of SMOKE's Linux section
  runs first because it is the row that can break the tester's own internet.
- **A link with only a link-local address gets NO DNS scope** from systemd-resolved — the dummy
  link must carry a real address (`192.0.2.53/32`, TEST-NET-1), or the route routes nothing.
  0.8.8 shipped without it (#734, measured 28 Sep 2026).
- **A dummy link's resolved settings are volatile.** They are bus calls, not a file: a resolved
  restart drops them (the unit is `PartOf=` resolved so it re-applies), and once — cause unknown —
  something reverted them after a re-setup (29 Sep 2026; `resolvectl revert rexenv0` reproduces the
  state). The marker alone therefore never says "installed": Linux `route_owner` also asks resolved
  whether the link routes the TLD (#741).
- **A VM's or WSL's upstream DNS is the HOST's, and a host running rexenv answers `.rex` itself.**
  That hid the dead route above through every Linux proof for four days. Ask WHICH link answered
  (`resolvectl query -i rexenv0`, `Current Scopes:`), never only what the answer was — the same
  "check the whole surface" rule, in DNS.
- **`pkexec` strips `PATH`** — every privileged command is absolute-pathed, and content
  travels through `printf` with `%` doubled.
- **A written file is not a file on disk.** ext4's delayed allocation left
  `rexenv-edge.service` ZERO bytes after a power cut ~30 s after Start all, its enable symlink
  intact — systemd reads an empty unit as masked (29 Sep 2026). Every privileged step now ends in
  `/bin/sync`, and the app's own boot files are written durably (#740); a new root write gets that
  for free, a new user-side boot file must use `durable::write_durable`.
- **Trust has two stores** (system + NSS), and snap browsers carry their own NSS db (#726).
- **Distributions rename sonames** (`libaio.so.1` → `libaio.so.1t64` on 24.04+, `libxml2`):
  the `lib-compat/` shims and the deb's declared dependencies are both needed (#731).
- **The floor is 22.04's glibc.** A deb built on a newer Ubuntu (the Dell's WSL 26.04) is a
  proof, never a release artefact; releases build on `ubuntu-22.04` in CI.
- **`PR_SET_PDEATHSIG` is the wrong tool** — it binds to the spawning THREAD; parent-death
  watching is a pidfd (#722).
- **The VM is small**: debug builds only, `CARGO_BUILD_JOBS=1`, `source ~/.cargo/env` over SSH.
  `fixture_base()` is `/tmp` (no `/private`).
- Proof hosts: `docs/TESTING.md` §"Proving a Linux claim"; design record
  `docs/PLAN-linux-port.md`; user side `docs/INSTALL.md` § Linux (Ubuntu).

## 9. Known gaps in the docs themselves

Recorded so a reader does not mistake them for the rule:
- `docs/ARCHITECTURE.md` and `docs/PORTS.md` were written against macOS and say so in their
  titles; their per-OS mechanism is §3 above plus PORTS' Windows/Linux artifact tables.
- `docs/SMOKE-TEST.md`'s main body is the macOS checklist; Windows and Linux are sections that
  say what changes. A new flow gets its row in every section it differs in.
