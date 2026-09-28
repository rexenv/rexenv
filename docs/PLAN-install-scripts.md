# PLAN — one-command install: `install.sh` (macOS + Linux) and `install.ps1` (Windows)

**Status:** IN FLIGHT — 28 Sep 2026. The owner asked for a Claude-Code-style install
(`curl -fsSL … | bash`, `irm … | iex`) on all three OSes, and asked whether it can get past
Gatekeeper, SmartScreen and "whatever Linux has". The research is §1, the rulings §2 (the
owner took every recommendation the same day), the design §3, the tasks and their proof §4.
**T1–T5 built and run the same day** (ledger #738, the runs in §4 and in
`docs/SMOKE-TEST.md`); what is left is the push (tap first, then the website), the first CI
run, and the desktop-only SMOKE rows. Tracked as the "One-command install" row in
`docs/TODO.md`.

```
curl -fsSL https://rexenv.rex.bd/install.sh | bash      # macOS, Linux
irm https://rexenv.rex.bd/install.ps1 | iex             # Windows
```

---

## 1. What the OS gates actually key on (measured, not remembered)

**macOS — Gatekeeper assesses a file that carries `com.apple.quarantine`, and the DOWNLOADING
APP sets that attribute.** Browsers, Mail and AirDrop set it; `curl` does not — measured
16 Aug 2026 (`docs/PUBLISH-TESTING.md` §A, the note that deleted the old "`curl -LO` applies
quarantine for sure" advice). The in-app self-update already lives on this path: reqwest
downloads the `.app.tar.gz`, no quarantine, the swapped bundle launches (row M, 7 Sep 2026).
The Homebrew cask is the odd one out — brew ADDS quarantine to what it fetches and then its
`postflight_steps` strips it again (`xattr -r -d`, measured with `--debug` 6 Sep 2026). So a
`curl | bash` install meets no Gatekeeper dialog without doing anything; the script still runs
`xattr -dr com.apple.quarantine` on the bundle — a no-op today, and the same step the cask
runs, so both install paths end in the same state. What it needs and has: a VALID ad-hoc
signature, which Apple Silicon requires of any arm64 code whatever its quarantine.

**Windows — SmartScreen's app-reputation check fires on a file carrying the Mark of the Web**
(an NTFS `Zone.Identifier` stream). Browsers write it (Edge wrote `ZoneId=3` even for a LAN
download, 19 Sep 2026). **PowerShell's `Invoke-WebRequest` and `curl.exe` do not — measured
28 Sep 2026 on the Dell (Windows 10 Pro 19045, PowerShell 5.1.19041.7725): both files had
only `:$DATA`.** So `install.ps1` downloads `setup.exe` with `Invoke-WebRequest` and runs it
with `/S`: no MotW → no SmartScreen dialog; per-user NSIS → no UAC. It deliberately does NOT
call `Unblock-File`: there is nothing to unblock, and "download an exe, unblock it, run it
silently" is the shape Defender's AMSI heuristics look for in a script piped to `iex`.

Two things no install command gets past, and the script does not try:

- **Smart App Control** (Windows 11, on by default only on clean installs that pass its
  evaluation). When it is ON it checks EVERY executable, MotW or not, and blocks unsigned
  ones with no "Run anyway". Only a signature fixes that (D5 ruled unsigned). Unmeasured:
  the Win11 VM's SAC state is §4's T5 row.
- **Defender antivirus** — it scans the download and, through AMSI, the script text `iex`
  evaluates. Nothing in the script disables, excludes or evades it; a false positive is
  reported to Microsoft, not worked around.

The Dell has `EnableSmartScreen = 0` by policy, so it can prove the MotW half and NOT "no
dialog appeared" — that needs the Win11 VM (SmartScreen on), from its desktop session.

**Linux — there is no Gatekeeper or SmartScreen.** No quarantine attribute, no reputation
service. A browser-downloaded AppImage lacks `+x` (not a security gate — `chmod`); `dpkg`
does not verify a local `.deb`'s signature at all (apt verifies REPOSITORIES, and rexenv has
none — D-L6). The one prompt is `sudo` for `apt-get install ./rexenv.deb`: a system package
is a root operation, and that is a permission, not a gate to bypass. The AppImage fallback
needs no root.

**The trust this asks for, said plainly.** `curl | bash` trusts `rexenv.rex.bd` (Cloudflare
Pages) and GitHub. Whoever controls the site controls what every NEW user runs. So the
script is public (`rexenv/homebrew-tap`, readable at the URL and in git), short, and fetches
only from `github.com/rexenv/homebrew-tap/releases`. The `.sha256` it checks sits beside the
asset, so it proves the bytes arrived intact — NOT that they are rexenv's; HTTPS to GitHub is
what carries authenticity, exactly as for a browser download of the same file. Verifying the
minisign-signed descriptor in `rexenv/runtimes` would raise that bar (an attacker would need
both repos); it is not done in v1 because neither a stock Mac nor Windows PowerShell 5.1 can
check an ed25519 signature without shipping a verifier (§5).

## 2. Rulings (owner, 28 Sep 2026 — "tumar kachhe jegulo recommended hoi seiguloi koro")

| # | Decision | Ruled |
|---|---|---|
| **D1** | Where the scripts live | **`rexenv/homebrew-tap`** (public — a `curl \| bash` script must be readable by anyone), served as `https://rexenv.rex.bd/install.sh` / `install.ps1` through a 302 in the website's `public/_redirects` to `raw.githubusercontent.com/rexenv/homebrew-tap/main/…`. One copy, no sync job. |
| **D2** | SmartScreen | The command path meets no SmartScreen dialog, by the OS's own design (§1). `docs/PLATFORMS.md` §7's "SmartScreen text is documented, not worked around" now says which path meets it: a BROWSER-downloaded `setup.exe` still does, word for word as `docs/INSTALL.md` records. |
| **D3** | rexenv already installed | **Touch nothing.** Print where it is and its version, point at Settings → About → Check now (rexenv updates itself), exit 0. Same on all three OSes. A brew-installed copy counts. |
| **D4** | Linux without apt | **AppImage fallback** into `~/Applications/rexenv.AppImage`, no root, with a line saying that distro is untested. |

## 3. Design

**Common (both scripts, the same order):**
1. Refuse early what cannot work, with a sentence: an unsupported OS or architecture, a
   macOS below the cask's floor (13), an Ubuntu below 22.04.
2. Already installed → D3.
3. Version: `https://github.com/rexenv/homebrew-tap/releases/latest` answers `302` to
   `…/releases/tag/v<X.Y.Z>`; the script reads that `Location` — one request, no API, no
   rate limit, never a draft or a prerelease (the same "latest" the cask's `livecheck` and
   the website's `/download` use). Anything not `X.Y.Z` is refused.
4. Download `rexenv_<X.Y.Z>_<kind>` and its `.sha256` (`<hash>  <name>`) into a fresh
   temp directory; a mismatch refuses and installs nothing.
5. Install, then start rexenv unless `REXENV_NO_LAUNCH=1` (CI, a test over SSH).
6. The whole script is one function called on the LAST line, so a download cut short runs
   nothing. The PowerShell one also runs inside `& { … }` and never calls `exit`: `iex`
   evaluates in the USER'S session, where `exit` closes their window and a top-level
   `$ErrorActionPreference = 'Stop'` would outlive the script.

**Per OS — the asset each installs (all already published by `release.yml`):**

| OS | Asset | Install | "Already installed" |
|---|---|---|---|
| macOS (universal) | `rexenv_<v>_universal.app.tar.gz` — the in-app update archive, exactly one top-level `rexenv.app/` (ledger #537), so no `hdiutil` | bundle id checked (`dev.rexenv.rexenv`), moved into `/Applications` (`sudo` only if the folder is not writable), `xattr -dr com.apple.quarantine`, `open` | `/Applications/rexenv.app` or `~/Applications/rexenv.app` |
| Windows x64 | `rexenv_<v>_x64-setup.exe` | `Invoke-WebRequest` (no MotW) → `Get-FileHash` → `setup.exe /S` (NSIS silent: per-user, Start-menu + desktop shortcut, no Finish page — Tauri 2.11.3 `installer.nsi`), then `Start-Process` the `rexenv.exe` the uninstall key's `InstallLocation` names | `HKCU`/`HKLM` `…\Uninstall\rexenv` (`DisplayVersion`) |
| Linux amd64/arm64, apt | `rexenv_<v>_<amd64\|arm64>.deb` | `sudo apt-get install -y /abs/path.deb` (apt resolves `Depends`; on failure one `apt-get update` and one retry); temp dir `0755`, file `0644` so apt's `_apt` sandbox can read it | `dpkg-query -W rexenv` = installed |
| Linux, no apt | `rexenv_<v>_<amd64\|aarch64>.AppImage` | `~/Applications/rexenv.AppImage`, `chmod +x`, a hint when `libfuse.so.2` is missing (`APPIMAGE_EXTRACT_AND_RUN=1`) | that file exists |

Windows on Arm: warned, not refused — the browser-downloaded `setup.exe` installs there too,
and INSTALL.md's "unsupported" (D6) means no promise, not a block. 32-bit Windows is refused.

**The coupling this creates.** Both scripts build asset NAMES (`rexenv_<v>_<kind>`). Renaming
an asset in `scripts/release-assets.sh` / `release.yml` breaks every new install silently —
so the tap's `install-scripts.yml` installs the latest published release on every OS after
each publish and weekly, and `docs/RELEASING.md` names the scripts beside the asset list.

## 4. Tasks and proof

| # | Task | Proof |
|---|---|---|
| T1 | `install.sh`, `install.ps1`, README "Install with one command" in `rexenv/homebrew-tap` | shellcheck clean; the runs below |
| T2 | `.github/workflows/install-scripts.yml` in the tap: shellcheck + PSScriptAnalyzer, then a REAL install on `macos-latest`, `ubuntu-22.04`, `ubuntu-24.04-arm`, `windows-latest` (Windows PowerShell 5.1 AND pwsh 7) and the AppImage fallback in a Fedora container; each runs the script twice (the second must say "already installed" and change nothing). Triggers: the scripts changing, `release: published`, weekly | the first run after push |
| T3 | `rexenv.rex.bd/install.sh` + `/install.ps1` → 302 to the raw tap files (website `public/_redirects`) | `curl -fsSL https://rexenv.rex.bd/install.sh \| head` after deploy |
| T4 | This repo's docs: INSTALL (the command per OS; the stale "Not yet installable" banners), PLATFORMS §3 row + §7 ruling, RELEASING (asset-name coupling), SMOKE-TEST rows per OS, TODO, STATUS | `status.py --check`, `doc-counts.sh` |
| T5 | Runs by OS: Linux in fresh Docker containers (22.04 arm64 as root and as a sudo user, 24.04, 22.04 amd64 emulated, Fedora → AppImage); macOS on the UTM VM (fresh install → launches, no quarantine; second run → D3); Windows on the Dell (mechanics) and the Win11 VM from its DESKTOP session (no SmartScreen dialog, SAC state recorded) | recorded in `docs/SMOKE-TEST.md` per OS |

## 5. Not in v1 (and why)

- **Descriptor-signature verification in the script.** The strongest check available (the
  release key the app compiles in), but it needs an ed25519 verifier the stock OS lacks —
  LibreSSL on macOS and .NET Framework in PowerShell 5.1 have none. A cross-check of the
  asset's sha256 against the UNVERIFIED descriptor in `rexenv/runtimes` is possible without
  one, but the descriptor is published by a separate click that can lag the release by
  minutes to hours, and an install failing in that window is worse than the check is worth.
- **An apt repository** (D-L6's "later") — would make `apt upgrade` and signature checks
  work, but rexenv updates itself, and a repo is a signing key to guard.
- **Choosing a version** (`REXENV_VERSION=`) — nothing needs it; the in-app update and the
  release assets cover downgrades by hand.
