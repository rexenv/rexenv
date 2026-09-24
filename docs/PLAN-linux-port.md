# PLAN — rexenv on Linux (Ubuntu first): the port, the decisions it forces, and the launch

**Status:** IN PROGRESS — started 24 Sep 2026 (overnight, owner asleep; the agent proceeds on
the defaults marked *assumed* below and the owner rules on §3 in the morning). Planned against
`b2ee968a`. Open work is the "Linux launch" row in `docs/TODO.md`; this file is the reasoning.

macOS and Windows ship. The owner wants a Linux build, **Ubuntu first**. The Windows port
(`docs/PLAN-windows-port.md`) did most of Linux's groundwork — W1 moved every Unix-only leak
behind a trait, W2 gave the binary catalog an OS dimension, W6 made `DnsManager` neutral about
what a route IS — so Linux is closer to "fill the stubs" than Windows ever was. This plan records
what is left, measured against the tree, so the morning starts from facts.

---

## 1. What the tree says (measured 24 Sep 2026)

- **`platform/linux/mod.rs` is 340 lines of `todo!()`** — every method of the 12 traits (13 with
  `LocalIpc`). Ledger #595's rule for Windows applies: a stub that stays returns `Error::Unported`
  or panics through `unported!`, never `todo!()` (a release build has no console, and one reached
  from an IPC command leaves the frontend waiting forever). The Windows stub guard scans only
  `windows/mod.rs`; L1 widens it to `linux/mod.rs`.
- **`core/` is already clean.** Ledger #163's scan refuses `std::os::unix`, `#[cfg(unix)]`,
  `cfg(target_os` and `Command::new("kill")` in `core/` production — so nothing there names an
  OS, and the Linux build reaches the same traits Windows does. Every `cfg(target_os)` left
  outside `platform/` is inventoried in §1.1.
- **The Unix transports work on Linux as-is.** `cli_server` and `mcp_server` gate only their
  socket transport with `cfg(unix)`, and the `rex` client dials a `UnixStream` under the same
  gate. `LocalIpc` for Linux is the macOS one, byte for byte (a unix-domain socket).
- **The catalog has `macos` and `windows` arms and no `linux` arm** (`binaries::manifest`). Every
  resolve passes `std::env::consts::OS`, so a Linux build resolves NOTHING until L2 — an honest
  "unknown binary" rather than a wrong download. §4 has the upstream measurement.
- **`RESOLVER_PORT` is 15353 off Windows, and that is right for Linux:** systemd-resolved's
  `DNS=` accepts `address:port` (since v246; Ubuntu 22.04 ships v249).
- **`words::current()` and `path_lookup::current()` answer macOS's words for any non-Windows
  build.** A Linux screen would tell the user to open Finder and run `xcode-select --install`. L3
  adds a `LINUX` set and a three-way `current()`.
- **`build.rs` already names the Linux sidecar** (`binaries/rex-x86_64-unknown-linux-gnu`, read
  from `TARGET`); `scripts/build-cli.sh` has no Linux arm (`exit 1`).
- **`tauri.conf.json` bundles `"targets": "all"`** — on Linux that is `.deb`, `.rpm` and
  `.AppImage`; there is no `bundle.linux` section, so the deb declares Tauri's default
  dependencies and nothing rexenv itself needs (see D-L1).
- **No Linux machine.** The Mac has Docker Desktop (aarch64 Ubuntu containers) and UTM with
  macOS and Windows VMs only. A container proves COMPILATION and the pure/process halves of the
  platform impls; it has no systemd, no polkit, no desktop and no browser — so DNS routing, the
  edge unit, CA trust, autostart and every GUI claim need an Ubuntu VM (§7).

### 1.1 OS branches outside `platform/` (grep inventory, 24 Sep 2026)

Every `cfg(target_os)`, `cfg(unix)`, `cfg(windows)` and `consts::OS` read outside `platform/`,
and what a Linux build gets from it. (a) = compiles and behaves right as-is; (b) = compiles,
Linux needs its own answer; nothing is (c) — `linux-check` was green on the first run.

| Where | Branch | Linux |
|---|---|---|
| `main.rs:33` tunnel guard entry (now `any(macos, linux)`) | kqueue watcher / pidfd watcher | (a) since 25 Sep 2026: `linux/parent_death_guard.rs`, the macOS shape over `pidfd_open` + `poll` (ledger #722). NOT `PR_SET_PDEATHSIG`: it binds to the spawning THREAD, and a retired tokio pool thread would have ended the share with the app alive |
| `main.rs:44` relauncher (`cfg(any(macos, windows))`) | self-update relaunch | (a) D-L7: no in-app update, nothing to relaunch |
| `lib.rs:31/37` CLI socket claim (`cfg(unix)`/`cfg(windows)`) | unix socket vs named pipe | (a) the unix arm |
| `lib.rs:70, 170, 284, 294` dock/activation policy (`cfg(macos)`) | Accessory vs Regular | (a) no dock concept; the tray is the way back |
| `lib.rs:267` About menu item (`cfg(macos)`) | edits the app menu | (a) GTK has no app menu; the tray's About raises the same event |
| `lib.rs:306` WebView2 accelerator keys (`cfg(windows)`) | Ctrl+R reload | (a) webkitgtk has no browser shortcuts in an app |
| `lib.rs:1575–1610` tray icon (`cfg(not(windows))` template / `cfg(windows)` colour) | the glyph | **(b)** the macOS template glyph is a black mark on Ubuntu's dark top bar — Linux takes the colour icon (L4) |
| `lib.rs:2351`, `commands/tunnels.rs:407` `activate_app` (`cfg(macos)`) | bring to front | (a) Wayland forbids it anyway; the dialog is modal to the window |
| `core/proxy.rs:132, 966` `LISTEN_PROBE_WAIT` | Windows 3 s vs 500 ms | (a) Linux refuses a loopback listen at once, like macOS |
| `core/terminal.rs` (`consts::OS` reads) | PATH separator, wp wrapper, export line | (a) every `os == "windows"` branch falls to the unix arm |
| `core/binaries.rs` (`consts::OS` reads) | the catalog | (b) → L2: Linux arms |
| `core/app_update.rs:91, 612` | the descriptor name per OS | (a) D-L7: `manifest-linux.json` is L7 |
| `core/app_info.rs:33` | the OS name for About | (a) already answers `"linux" => "Linux"` |
| `core/sites.rs:1276` blast radius | Windows path shape | (a) unix arm |
| `core/firefox.rs:295` | a test fixture path | (a) `cfg(unix)` |
| `core/ports.rs:466, 673`, `confverify.rs:247, 270`, `sites.rs:4333, 5185`, `setup.rs:410` | test-only `cfg(macos)` | (a) tests skipped, never production |
| `src/` (TSX) | — | (a) NO OS string in the frontend: every OS word comes through `usePlatformWords` (the `words.rs` frontend scan holds this) |

## 2. What Linux has that the other two do not

- **A supervisor with `Restart=always`, per user and system-wide (systemd).** The DNS agent is a
  *user* unit (`~/.config/systemd/user/rexenv-dns.service`, `systemctl --user enable --now`);
  the root edge is a *system* unit (`/etc/systemd/system/rexenv-edge.service`) installed by one
  privileged script, the macOS LaunchDaemon shape with `launchctl` spelled `systemctl`.
- **A privilege prompt that is the desktop's own (polkit `pkexec`).** One dialog, names the
  program, runs the script as root. No AppleScript, no UAC step process.
- **`/proc`.** `pid_exe` is a readlink of `/proc/<pid>/exe`; `pid_command` is
  `/proc/<pid>/cmdline`; `pids_named` and `owned_pids` are one directory walk. No `lsof`
  dependency for identification (Ubuntu desktop ships `lsof`, Ubuntu server does not).
- **php-fpm exists.** The pool model is `PoolModel::Fpm`, exactly macOS — no php-cgi group,
  no D1. static-php-cli publishes Linux `cli` and `fpm` tarballs for every version rexenv pins
  (§4).
- **`sendmail_path` works** (Unix PHP), so mail catching needs no SMTP keys.

## 3. Decisions for the owner — RULED 24 Sep 2026 (the overnight defaults, confirmed or changed)

The owner ruled on every row on 24 Sep 2026, after the VM run. Confirmed as built: D-L1
(`.deb` + AppImage), D-L2 (the dummy-link route — the drop-in had already failed P1),
D-L3 (two stores, two prompts; the snap Chromium database added), D-L4 (22.04), D-L5 (both
archs; an x86_64 host is still owed), D-L6 (unsigned), D-L9 (the UTM VM), D-L10
(`linux-check` stays in `verify.sh`). **Changed:** **D-L7 — self-update IS in v1**: a `.deb`
install updates through `pkexec dpkg -i <downloaded .deb>` (one prompt per update), an
AppImage through the macOS-shaped file swap with no prompt — `manifest-linux.json`, a real
`LinuxAppBundle`, a relauncher (task L7). **D-L8 — PHP 7.4 IS wanted on Linux**: no static
build exists upstream, so rexenv builds it in `rexenv/runtimes` as it does for macOS (task
L8). The table below is the record of what was assumed while the owner slept.

| # | Decision | Assumed (what was built) | Alternatives |
|---|---|---|---|
| **D-L1** | **Package format.** | `.deb` for Ubuntu 22.04+ (x86_64 and arm64), AppImage as the "any distro" fallback; `.rpm` built by Tauri but not published. `bundle.linux.deb.depends` lists `libwebkit2gtk-4.1-0`, `libayatana-appindicator3-1`, `xdg-utils`, `libnss3-tools`, `pkexec \| policykit-1` (26.04 dropped `policykit-1`; measured 25 Sep 2026) (see D-L3). | Snap / Flatpak (sandboxed — cannot write `/etc/systemd`, cannot `pkexec`; wrong fit for a tool that owns a root edge). |
| **D-L2** | **DNS route mechanism.** | **REVISED after P1 failed on the VM (24 Sep 2026).** The first design — a global resolved drop-in `Domains=~rex` — routed `example.com` to rexenv's resolver too (the hazard §6 predicted). Built instead: a dummy link `rexenv0` with a link-scoped routing domain per TLD and `default-route no` (`resolvectl dns/domain/default-route`), applied by a root oneshot unit `rexenv-dns-route.service` (`PartOf=systemd-resolved`) from markers under `/etc/rexenv/dns.d/<tld>` whose bytes are the macOS resolver-file signature. Measured right on the VM (P1b): `.rex` → loopback, `example.com` public, `curl` 200, NM `unmanaged`, survives a resolved restart. Hosts without resolved get an honest refusal. | NetworkManager's dnsmasq plugin (only where NM manages DNS; no port syntax); `nameserver 127.0.0.1` in `resolv.conf` (all names — the P1 failure by another road). |
| **D-L3** | **CA trust: where "trusted" lives.** | Both halves, in order: (1) the NSS user store `~/.pki/nssdb` via `certutil` — what Chrome/Chromium/Brave/Edge read on Linux, a USER op with no prompt; (2) the system store via `/usr/local/share/ca-certificates/rexenv-local-ca.crt` + `update-ca-certificates`, a ROOT op batched into the same `pkexec` as the DNS route — what `curl`, PHP, WP-CLI, Composer read. Firefox stays `core::firefox`'s policy file, as on the other OSes. `is_trusted` asks NSS (`certutil -L`); missing `certutil` (libnss3-tools) → the deb depends on it, and the message says so. | NSS only (Chrome works, `curl`/WordPress HTTP to a `.rex` site fails); system only (browsers refuse). |
| **D-L4** | **Ubuntu floor.** | 22.04 LTS (first LTS with `libwebkit2gtk-4.1`; Tauri 2 needs 4.1). The check image (`scripts/linux-check/Dockerfile`) compiles against 22.04's libraries so a newer-only API cannot slip in. | 24.04 only. |
| **D-L5** | **Architectures.** | x86_64 AND aarch64: every upstream in §4 publishes both except nginx (rexenv's own build — L2 pins x86_64 first; aarch64 waits on a `rexenv/runtimes` release) and Redis/MariaDB bottles (x86_64 only). The Mac's Docker runs aarch64 containers, so that slice is the one the agent can exercise. | x86_64 only for v1. |
| **D-L6** | **Signing.** | None (the Windows ruling: no money for certificates). A `.deb` is verified by the apt repo's key only if there is a repo — there is not; the download page carries SHA-256s. | An apt repository on `dl.rexenv.dev` (later). |
| **D-L7** | **Self-update.** | **RULED IN, 24 Sep 2026** ("Ekhon-i, v1 te"). Built as L7: the `.deb` through `dpkg -i` in the polkit step (the one root command, after `dpkg-deb` verified the package), the AppImage by the macOS exchange beside itself, one descriptor per kind and arch. The overnight default had been "not in v1: `AppBundle` returns `Error::Unported`", on the argument that `pkexec dpkg -i` is a root op over the whole system — the ruling accepted that in exchange for one prompt. | Stay `Unported` and tell the user to `apt install` the new `.deb` (the overnight default). |
| **D-L8** | **Which tools the Linux catalog carries.** | Caddy, PHP (cli+fpm, all seven minors), nginx (own build), MySQL (Oracle's glibc2.28 generic tarball), PostgreSQL (theseus-rs), FrankenPHP, Mailpit, cloudflared, plus the OS-agnostic phars. **Not in v1:** Redis, MariaDB, httpd, Xdebug — each a Homebrew Linux bottle (x86_64 only) or a build rexenv does not have; `ships_on` refuses them on Linux so Settings says so (D4's shape). | Pin the x86_64 bottles now (relink is `patchelf`, not `install_name_tool` — new code, untested). |
| **D-L9** | **Test hardware.** | An Ubuntu 22.04 VM in UTM on the Mac (aarch64, ~20 GB disk — **the Mac has 7.8 GB free after tonight's cache purge, so this is blocked on disk**); the Docker image for everything that needs no systemd. | An old x86_64 laptop with Ubuntu (the Dell dual-boot?), a cloud VM. |
| **D-L10** | **Where the Linux check runs.** | In `verify.sh`, like `windows-check`: SKIPPED (exit 3) when Docker is not running, red on a real break. Costs minutes per run while Docker is up. | `verify-full.sh` only. |

## 4. Upstream availability (measured 24 Sep 2026 — every row HEAD-probed, then hashed in full)

| Binary | Linux artifact | x86_64 | aarch64 |
|---|---|---|---|
| Caddy 2.11.4 | `caddy_2.11.4_linux_{amd64,arm64}.tar.gz` | ✓ 17 MB, SHA-512 matches `checksums.txt` | ✓ 16 MB, matches |
| PHP 8.0.30–8.5.8 (cli + fpm) | static-php.dev `php-<v>-{cli,fpm}-linux-{x86_64,aarch64}.tar.gz` | ✓ all 12 (25–31 MB) | ✓ all 12 |
| PHP 7.4.33 | `rexenv/runtimes` `php-7.4.33-7`: `php-7.4.33-{cli,fpm}-linux-{x86_64,aarch64}.tar.gz` (L8, 25 Sep 2026 — static-php.dev never built one) | ✓ 30–32 MB, SHA256SUMS match | ✓ |
| nginx 1.30.4 | `rexenv/runtimes` — none published; jirutka `nginx-1.30.4-{x86_64,aarch64}-linux` static | ✓ 7 MB (jirutka) | ✓ 6.6 MB |
| MySQL 8.4.6 / 8.0.44 | `mysql-<v>-linux-glibc2.28-{x86_64,aarch64}.tar.xz` (`.tar.gz` **404** — xz only) | ✓ 920 / 891 MB | ✓ 909 / 878 MB |
| PostgreSQL 18.6.0 / 17.11.0 / 16.15.0 | theseus-rs `postgresql-<v>-{x86_64,aarch64}-unknown-linux-gnu.tar.gz` + `.sha256` | ✓ 11–12 MB, all match the published `.sha256` | ✓ all match |
| FrankenPHP 1.12.4 | `frankenphp-linux-{x86_64,aarch64}` | ✓ 170 MB | ✓ 163 MB |
| Mailpit 1.30.3 | `mailpit-linux-{amd64,arm64}.tar.gz` | ✓ 10 MB | ✓ 9.6 MB |
| cloudflared 2026.6.1 | `cloudflared-linux-{amd64,arm64}` | ✓ 39 MB | ✓ 37 MB |
| WP-CLI, Composer, Adminer | OS-agnostic | ✓ | ✓ |

Pinned in `core/binaries.rs` (L2, 24 Sep 2026) with `docs/PORTS.md`'s Linux table. **The first
hashing pass was wrong five times**: `curl -sL | shasum` with no `--fail` and no retry turns a
stream cut short into a full-length digest of the wrong bytes. Two were caught by the publisher's
`.sha256` (PostgreSQL 18.6.0 aarch64 hashed as an EMPTY stream; 17.11.0 both archs) before the
pins landed; three more (cloudflared both archs, PHP 8.0.30 x86_64) had no publisher digest and
were caught only by `manifest_sweep_check`'s own re-hash — its "DIGEST MISMATCH" for 3 of 188
targets, 25 Sep 2026. Each was downloaded twice more with `--fail --retry`, both runs agreed with
the sweep, and the pins were corrected. Every artifact without a publisher digest now has THREE
agreeing full downloads behind it; the lesson is the sweep's own rule, sharpened: a pin is what a
COMPLETE download hashed, and a hash of a stream nobody checked for completeness is not one.

## 5. Tasks

Each ends in something observable; each is its own commit.

- **L0 — Linux compile check, on the Mac.** `scripts/linux-check.sh`: runs `cargo clippy
  --all-targets -- -D warnings` for both crates inside `scripts/linux-check/Dockerfile`'s Ubuntu
  22.04 image, with a named volume for the target dir and the registry so the second run is
  incremental. Exit 3 (SKIPPED in `verify.sh`) when `docker` is missing or its daemon is not
  running; any other non-zero exit is red. The same placeholder-sidecar dance as
  `windows-check.sh` (`tauri_build` refuses a missing `binaries/rex-<triple>`), the same
  error-site inventory. *Done when:* the script has a load-bearing exit code, the first run's
  inventory replaces §1.1, and `verify.sh` prints its line.
- **L1 — Fill `platform/linux/`.** Paths (`directories` → `~/.local/share/rexenv`), permissions
  and `LocalIpc` (macOS's, verbatim), the supervisor (`/proc` + `ss` for ports, `kill` for
  signals — the L0 tests for the pure parsers run in the container), shell (`xdg-open`,
  `org.freedesktop.FileManager1.ShowItems` over `dbus-send` for reveal, `$SHELL -ilc` for the
  login env, editors/browsers/terminals found on `PATH` and by `.desktop` id), privileges
  (`pkexec`), autostart (`~/.config/autostart/rexenv.desktop`, `X-GNOME-Autostart-enabled`),
  DNS (D-L2), cert trust (D-L3), DNS agent and edge (systemd units, §2), binaries (`chmod +x`;
  tree prepare is a no-op — static-php builds are static, MySQL/PostgreSQL tarballs carry
  `RUNPATH $ORIGIN/../lib`), `AppBundle` (D-L7 — `Unported`). The Windows stub guard widened to
  `linux/mod.rs`. *Done when:* no `todo!()` under `platform/linux/`, L0 green, and the pure
  halves (unit contents, drop-in contents, `ss` parse, `.desktop` parse) have L0 tests that run
  on the Mac.
- **L2 — Linux arms in the catalog.** ✓ **24 Sep 2026.** `(name, "linux", version)` arms for
  D-L8's set, both archs, every checksum from a full streamed download; `ships_on` refuses the
  bottles by the same absence Windows uses; `Archive::TarXzTree` + a Linux-only `xz2`
  behind `platform::xz_decoder` (Oracle publishes no `.tar.gz` for Linux); PHP 7.4 has no
  arm (§4); `manifest_sweep_check` enumerates the Linux set (floor 86 → 130) — **not yet RUN**
  against the network after the change; `docs/PORTS.md` Linux table.
- **L3 — Build plumbing and words.** ✓ 24 Sep 2026 (words landed with L1). `build-cli.sh`'s
  Linux arm stages the HOST triple (proven in the Ubuntu container); `bundle.linux.deb.depends`;
  `scripts/release-linux.sh` — never run, no Ubuntu host. Original scope: `build-cli.sh` Linux arm (`rex-<triple>` for the host
  triple), `tauri.conf.json` `bundle.linux` (deb depends, desktop entry, AppImage), `words::LINUX`
  + three-way `current()`, `path_lookup` (UNIX already), the TSX platform reads from §1.1,
  `scripts/release-linux.sh` (the `tauri build` recipe for a VM). *Done when:* `tauri build`
  on an Ubuntu host would need nothing that is not in the tree (proven on the VM in L6).
- **L4 — The app shell on Linux.** ✓ 24 Sep 2026: the tray takes the colour icon (an
  appindicator draws it as-is on a dark top bar — ledger #721); everything else in §1.1 falls
  to a default that is right for Linux, the tunnel guard excepted (a `PR_SET_PDEATHSIG` shape
  is a TODO). Original scope: Tray (`tray-icon` needs `libayatana-appindicator`), no dock
  concept (the macOS accessory-policy code stays `cfg(macos)`), `titleBarStyle`, the tunnel
  guard (Linux HAS `PR_SET_PDEATHSIG` — the guard process is unnecessary; `prctl` on the child),
  `run_relauncher`/`activate_app` (Unported / no-op), the mail `sendmail_path` (Unix — as macOS).
  *Done when:* `main.rs` and `lib.rs` compile for Linux with every `cfg(macos)` item either
  shared or given a Linux answer, and L0 is green.
- **L5 — Docs.** ✓ 24 Sep 2026; `notices-check.py` walks the Linux graph since 25 Sep (#724 —
  the Mac's registry DOES resolve it offline; the earlier note here was a guess, not a measurement).
  Original scope: `ARCHITECTURE.md` (the OS rule now says three), `MAP.md`, `PORTS.md`,
  `TESTING.md` ("Proving a Linux claim"), `SMOKE-TEST.md` Linux section, `INSTALL.md`,
  `RELEASING.md`, `CLAIM-LEDGER.md` rows (every new "never"), `TODO.md` ("Linux launch" under
  *Now*; the Phase 4+ row retired), `STATUS.md` regenerated.
- **L6 — The VM run.** **STARTED 24 Sep 2026 — the VM exists and the app has RUN on it.** Ubuntu
  22.04 arm64 in UTM (cloud image + cloud-init seed, no installer; `ssh rexenv@192.168.64.7`);
  `tauri build` there produced the first `.deb` and AppImage; the deb installed; onboarding
  through polkit (rexenv's own sentence), the DNS route, both trust stores, Start all with the
  root edge unit, a WordPress site served over HTTPS to `curl` and to Chromium. Found and fixed
  the same day: the resolved drop-in (D-L2, #717), the snap Chromium NSS database (#726), the
  thread-summed RAM figure (#725), `rex`'s Linux socket path (#727). P4 passed after a reboot (autologin → hidden autostart, every unit back, services up); the
  rebuilt deb proved #725 (260 MB) and #727 (`rex` answers); 7.4's Install button on Linux is
  #728. P2's kill-9/Stop-all/Start-all legs and the tunnel guard (owner-approved test tunnel) passed
  too. Owed: Firefox (snap). Original scope: Install Ubuntu 22.04 in UTM (D-L9), `tauri build` there, install
  the `.deb`, run SMOKE-TEST's Linux section — and FIRST the §7 P1 probe. Every ledger row L1
  marks `◐ (Docker only)` becomes `✓ (Ubuntu 22.04 VM)` or a bug.
- **L7 — Self-update on Linux (RULED IN, 24 Sep 2026) — BUILT 25 Sep 2026.** What landed
  differs from the sketch in two places, both measured: (1) ONE descriptor per package kind
  AND arch (`app-manifest-linux-<deb|appimage>-<x86_64|aarch64>.json` + `.sig`, `core::app_update::
  manifest_urls_for`), not one `manifest-linux.json` with entries — the macOS/Windows schema
  names ONE artifact per document and the reader is shared, so a per-variant document reuses
  every line of it; the platform's `AppBundle::descriptor_variant` picks it and a dev build
  (`None`) fetches nothing. (2) The AppImage's version is read by RUNNING the staged file with
  `--print-version` (`main.rs` answers it before Tauri loads; `APPIMAGE_EXTRACT_AND_RUN=1` so a
  host without libfuse2 still answers) — an AppImage embeds no version field a reader could
  trust. The deb is verified with `dpkg-deb -f` (Package/Version/Architecture) and `-c`
  (`usr/bin/rexenv` + `usr/bin/rex` present) BEFORE the one root command, `/usr/bin/dpkg -i
  '<staged>'`, runs through the polkit step (`--privileged-step`, rexenv's sentence). The
  AppImage swap is `renameat2(RENAME_EXCHANGE)` with the rename pair as fallback, the
  previous copy left in the stage dir for the health sweep, as on macOS. `InstallKind` grew
  `SystemPackage` and `PortableFile`; `preflight` asks the STAGING folder (app-data
  `updates/`) for room and writability for a package, never `/usr/bin`. The relauncher waits
  on a pidfd (capped 120 s) and starts `/usr/bin/rexenv` or the image detached. Proof: L0
  `app_bundle_rules::tests` + `app_update::tests`; L1 `linux_app_swap_check` (system tier)
  on the VM — fixture image exchanged, fixture package `rexenv-swapfixture` refused twice
  then installed by the platform's own command and removed, beside the real installed deb
  (ledger #729, #730). **Owed:** a signed Linux descriptor (the runtimes publisher's
  `--linux <deb|appimage> <arch>` mode is a PR for the owner to merge and RUN — publishing is
  his gate), then `app_update_check` on the VM and a real 0.8.x → 0.8.y apply through the
  About screen (SMOKE-TEST's Linux "In-app update" rows).
- **L8 — PHP 7.4 for Linux (RULED IN, 24 Sep 2026) — SHIPPED 25 Sep 2026.** rexenv/runtimes
  PR #7 added `ubuntu-24.04` / `ubuntu-24.04-arm` lanes to `php-74.yml` + `build-php74.sh` (the
  OS read from `uname`; `ldd`/`file` gates instead of `otool`/minos). Three dry runs found:
  aarch64 links `sapi/cli/php` only with `-fPIC -fPIE` (small-GOT overflow on
  `zend_ce_traversable`); the licence sweep needed freetype's `LICENSE.TXT` spelling and a
  SQLite public-domain note; the static ELFs export no Zend symbols (no Xdebug, as on macOS).
  The owner published `php-7.4.33-7`; rexenv pins the four Linux tarballs + two licence
  tarballs on THAT tag (`PHP_7_4_33_LINUX_TAG` — the macOS pin stays on `-6`, the release that
  proved it), `ships_on` answers true, and #728's refusal for 7.4 is gone by the same table.
- **L9 — The release leg (25 Sep 2026).** `scripts/release-linux-check.sh` is §A0's Linux half,
  run by `release-linux.sh` after the build: one deb + one AppImage for the version and THIS
  arch, the `rex` sidecar, the ELF arch, the embedded payload + update key, the deb's control
  fields / members / polkit action / `Depends`, and both artefacts answering `--print-version`
  — the facts the in-app updater (L7) re-checks on the user's machine, asserted first where a
  failure costs a rebuild rather than a user's dialog. `release.yml` gained a `release-linux`
  job on `ubuntu-24.04` (x86_64) that attaches the pair to the draft like the Windows job;
  aarch64 is the VM by hand (arm runners are paid on a private repo). The descriptors are
  step 8's four `--linux` runs (L7). The download page lives outside this repo (the tap's
  release + the website); the deb and AppImage sit beside the dmg there, with `.sha256`s.
  Neither CI job has run — the repo is private — so the x86_64 pair is still owed a host.

## 6. Linux hazards to design for, not discover

- **Route-only domains and the answer-everything resolver (P1) — IT HAPPENED.** The global
  drop-in sent `example.com` to `127.0.0.1:15353` on the first VM run (24 Sep 2026): resolved
  uses the global servers for every name no link claims, `~rex` or not. The dummy-link route
  replaced it the same day (D-L2); the lesson stands for any future DNS change on Linux — a
  GLOBAL server is a default route whatever its domains say, only a LINK scopes.
- **The polkit dialog names `/bin/sh` unless the deb's action file is installed** — with it
  (`linux/dev.rexenv.rexenv.policy`, `exec.path` = `/usr/bin/rexenv`) the step runs as
  `rexenv --privileged-step <script>` and the dialog carries rexenv's sentence (#723). An
  AppImage never gets that: pkexec matches the path exactly.
- **`pkexec` strips the environment** (`HOME`, `PATH` reduced to a safe set). Every privileged
  script must use absolute paths (`/usr/bin/systemctl`, `/bin/cp`) and never rely on `$HOME`.
- **Wayland.** `xdotool`-style activation does nothing; `activate_app` is a no-op, and "bring
  rexenv to front" is the tray's job. Screenshots in a smoke run come from `gnome-screenshot`,
  not a click helper.
- **AppImage has no `/usr/share/applications` entry** → no autostart `.desktop` can name a
  stable path. Autostart on AppImage records `current_exe()`, and `refresh` re-points it when
  the file moved — the macOS "not an .app bundle" branch.
- **The deb's postinst runs as root at install time** — rexenv must NOT use it for anything
  (no unit install, no CA): the first launch does system setup through `pkexec` with consent,
  the same story as the other two OSes.
- **`certutil` and `update-ca-certificates` are separate stores with separate failure modes.**
  `is_trusted` reports the NSS half (the browsers); the system half's failure is a plain error
  from the privileged step, never a silent partial trust.
- **`ss -p` shows only this user's processes' names**; a root edge's listener shows a pid and
  no name. The identification path reads `/proc/<pid>/exe` (readable for root pids? NO — it is
  `EACCES` cross-user). The edge is therefore identified by its admin socket, exactly as on
  macOS (`proxy::admin_alive`), never by the pid table.

## 7. What an agent on the macOS dev machine cannot prove

Docker proves compilation, the pure parsers, and anything that is a process or a file
(`/proc` reads, `chmod`, unix sockets, the login-env probe). It cannot prove:

- **P1 — DNS scoping** (§6, first bullet): a VM, `resolvectl query example.com` after the
  drop-in, must NOT answer `127.0.0.1`.
- **P2 — the edge unit**: `systemctl status rexenv-edge` after Start-all, the admin socket owned
  by the user across a config reload (the macOS chown loop, spelled `stat -c`).
- **P3 — CA trust**: Chrome opens `https://x.rex` with no warning; `curl https://x.rex` succeeds.
- **P4 — autostart**: the `.desktop` file starts rexenv hidden at sign-in.
- **P5 — the GUI**: the tray icon, the window, the webview's `rexdb://localhost` origin.

Each is a row in `SMOKE-TEST.md`'s Linux section, and each ledger row that rests on one says
`◐ (Docker only)` until a VM run flips it.

## 8. Docs this changes as it lands

`docs/ARCHITECTURE.md`, `docs/MAP.md`, `docs/PORTS.md`, `docs/CLAIM-LEDGER.md`,
`docs/TESTING.md`, `docs/SMOKE-TEST.md`, `docs/INSTALL.md`, `docs/RELEASING.md`,
`docs/DESIGN.md` (window controls), `docs/CLI-ROADMAP.md` (the socket is the same),
`CLAUDE.md` (the "both platforms" rule becomes "every shipping platform").
