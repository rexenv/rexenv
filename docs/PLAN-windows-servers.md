# PLAN — per-site web servers on Windows: Apache and FrankenPHP (OpenLiteSpeed stays refused)

Status: **IN FLIGHT — T1–T5 landed 24 Oct 2026 (ledger #909: the mirror `httpd-windows-2`, the catalog arms, `apache::Layout`, the stop event; T4 `frankenphp::MailCatchIni` — SMTP ini keys on Windows, forward-slash conf paths; T5 `apache_unavailable` per OS, the SiteDetail switch filtered by the offered set); next T7 the run on the Win11 VM, then T6's remaining doc sweep. Written 24 Oct 2026 (owner ask: "Windows has only nginx; add the other
servers, plan first, then one task at a time"). T1–T7 below; each lands as its own commit with
its docs, the Windows proof comes from a build run on the Win11 VM (W4's debt, PLAN-windows-port
§4).**

## 1. What is true today (measured 24 Oct 2026)

- A site's server is `WebServer { Nginx, Apache, Frankenphp, Openlitespeed }`
  (`state/models.rs:45`), one column (`sites.web_server`), one gate: `core::sites::
  ensure_server_available_on(server, os)` asks `binaries::ships_on(name, pin, os)` — **the pins
  ARE the gate** (ledger #642). Windows has a pin for nginx and php only, so on Windows the New
  Site picker offers Nginx alone, and a switch to anything else is refused in core before any
  download (#643, #786). Nothing in `platform/windows/` is `Unported` for this: the refusal is
  the generic core sentence "web server X is not available on this platform yet"; OpenLiteSpeed
  alone has a per-OS sentence (`words.rs` `openlitespeed_unavailable`, #774).
- The override topology is OS-neutral: Caddy `:443` → `127.0.0.1:<override port>` (FrankenPHP
  8200–8299, Apache 8300–8399, OpenLiteSpeed 8400–8499, FNV of the domain) → for Apache and
  OpenLiteSpeed `fcgi://127.0.0.1:<pool port>`, the SAME `97xx` port the shared nginx uses
  (`php::fpm_port`); FrankenPHP serves with its embedded PHP. **On Windows the pool port is a
  php-cgi group (`php_cgi::start_group`, `-b 127.0.0.1:97xx`), speaking FastCGI on the same
  port** — so Apache's `SetHandler "proxy:fcgi://127.0.0.1:{fpm_port}"` needs no Windows
  variant, only a measurement (mod_proxy_fcgi has never been pointed at php-cgi's listener).
- Upstream artifacts (checked 24 Oct 2026):
  - **Apache httpd**: no Windows build from the ASF. Apache Lounge publishes
    `httpd-2.4.69-261002-Win64-VS18.zip` (14.5 MB, stored, members under `Apache24/`, ships
    `modules/mod_proxy_fcgi.so`, `mod_rewrite.so`, every module `core/apache.rs` loads except
    `mod_mpm_event` and `mod_unixd`, which do not exist on Windows — `mpm_winnt` is compiled in);
    sha256 `9f1f93cab9a0c6dc5e628aa640784bb30347f93fcd01b46a4c0d6b5ba76bf4e2`, published in
    their `.zip.txt`; Apache-2.0, "unmodified source"; needs the VC++ redistributable
    (14.51+). **Apache Lounge keeps only the current build**, so a pin to their URL rots on
    their next release — the same shape `php-windows.yml` solved by MIRRORING into an
    immutable `rexenv/runtimes` release.
  - **FrankenPHP 1.13.3**: upstream's own `frankenphp-windows-x86_64.zip` (60 MB, 83 members:
    `frankenphp.exe`, the official `php8ts.dll` + `ext/*.dll` ×36, OpenSSL, ICU, `license.txt`)
    — a `Dir`, not the `Single` the other OSes pin. Immutable GitHub release URL, no mirror
    needed. PLAN-windows-port §4 W4 recorded the asset at 1.12.4 and never ran it.
  - **OpenLiteSpeed**: Linux/macOS/FreeBSD only, upstream's `build.sh` quits elsewhere
    (archive/PLAN-openlitespeed.md:50). **Permanently refused on Windows; unchanged here.**

## 2. Decisions (taken by the agent, owner asleep; each reversible by a pin change)

- **D1 Apache = Apache Lounge, mirrored.** The zip is re-hosted on `rexenv/runtimes`
  (`httpd-windows-2`) with its Apache Lounge sha256 re-verified at mirror time, beside a
  `licenses-httpd-2.4.69-windows.tar.gz` (the httpd source tarball's LICENSE + NOTICE and
  Apache Lounge's ReadMe) — rexenv is the DISTRIBUTOR of that zip, so the licence artifact
  rides with it like every self-distributed row (`is_self_distributed`, ledger #336). D4 of
  PLAN-windows-port called this "a third-party trust decision nobody has made yet": made now,
  on the strength of Apache Lounge being the build every Windows PHP stack (XAMPP-less setups,
  Laragon, WampServer) has shipped for a decade, built from unmodified ASF source under
  Apache-2.0; the owner may reverse it by un-pinning.
- **D2 PHP through mod_proxy_fcgi to the shared php-cgi group** — the macOS config unchanged
  (one pool per minor on every OS; NTS vs TS stays moot; `mod_fcgid`, which would spawn a
  second pool model, and `mod_php`, which needs TS PHP, are out).
- **D3 FrankenPHP = upstream's Windows zip**, `Dir` shape, member `frankenphp.exe`. Its
  embedded PHP must equal `frankenphp_embedded_php` (8.5.12) — asserted by running
  `frankenphp.exe php-cli -r 'echo PHP_VERSION;'` in T7; a mismatch makes the pin per-OS.
  Mail catch: FrankenPHP's Caddyfile `php_ini` block sets `sendmail_path` to the shim on
  macOS/Linux; on Windows it sets the ini SMTP keys (`SMTP`/`smtp_port`) exactly as the
  php-cgi group does (`php_cgi.rs:54`) — the rule moves to core as `mail_catch_ini_lines(os,…)`
  so both pools and FrankenPHP read one source.
- **D4 Per-OS Apache layout lives in core as data keyed by `os`** (`apache::Layout::for_os`):
  module directory (`lib/httpd/modules` bottle vs `modules`), `TypesConfig` path
  (`.bottle/etc/httpd/mime.types` vs `conf/mime.types`), the MPM lines (event + unixd vs
  none), the binary name (`bin/httpd` vs `bin/httpd.exe`), the archive shape (`Bundle` vs
  `Dir`). `core/` names no OS by `cfg`; it already takes `os: &str` everywhere in `binaries.rs`
  — same pattern. The generated conf keeps ONE text with those five holes.
- **D5 Stop policy**: httpd on Windows (mpm_winnt) is a parent + one child; the parent's
  clean-shutdown handle is the named event `ap<pid>_shutdown` (what `httpd -k shutdown`
  signals). `stop_policy::clean_exit_events` gains it (third name; a name belongs to exactly
  one program). The group root is the parent (`owned_master` climbs). Measured in T7; if the
  child survives `TerminateProcess` on the parent the way MySQL's monitor did, the job object
  already covers it (every spawn is job-confined on Windows).
- **D6 VC++ runtime**: httpd's DLLs link the same redistributable php.net's PHP does;
  `BinaryProvider::runtime_problem` already gates PHP and MySQL starts, and the Apache override
  start goes through the same check (its refusal sentence names httpd).
- **D7 UI**: the SiteDetail server `<select>` filters by `offeredWebServers` like the New Site
  dialog (today it lists all four and core refuses — honest but a dead option). The New Site
  note's "Apache and FrankenPHP aren't part of rexenv on Windows yet" goes; the only per-OS
  sentence left is OpenLiteSpeed's. `words.rs` gains `apache_unavailable` for the OS that still
  has no Apache pin (Linux: Homebrew's httpd bottles are x86_64-only and unrelinked, PORTS D-L8)
  and fixes the drifted Linux `openlitespeed_unavailable` ("choose Nginx, Apache or FrankenPHP"
  — Apache is not offered on Linux).
- **D8 Out of scope**: Apache on Linux (its own plan; needs a runtimes build), OpenLiteSpeed on
  Windows (never), `.htaccess` `php_value` lines (ignored under proxy_fcgi on every OS, as
  before), Xdebug/debug pools for overrides.

## 3. Tasks — one commit each, docs in the same commit, verify green before each

| # | Task | Proof |
|---|---|---|
| T1 | `rexenv/runtimes` `httpd-windows.yml`: download Apache Lounge's zip + `.zip.txt`, re-verify sha256 against the published one AND the pin below, unzip and assert `Apache24/bin/httpd.exe` + the nine `modules/*.so` the conf loads exist, fetch `httpd-2.4.69.tar.bz2` from an ASF mirror (verified against its `.sha512`), pack LICENSE + NOTICE + ReadMe.txt into `licenses-httpd-2.4.69-windows.tar.gz`, run `httpd.exe -v` + `-t` on `windows-latest`, publish release `httpd-windows-2` (zip, licences, SHA256SUMS, attestation). Dispatched from the branch (releases are repo-level); PR left for the owner. | run green, `gh release view httpd-windows-2` |
| T2 | Catalog: `("httpd","windows","2.4.69")` → `Dir`, `ZipTree { strip: 1 }` (drops `Apache24/`), member `bin/httpd.exe`, `Sha256` of OUR mirror; licences arm; `("frankenphp","windows","1.13.3")` → `Dir`, `ZipTree { strip: 0 }`, member `frankenphp.exe`, upstream URL, sha256 hashed from the download; `shape_of_on("httpd","windows") = Dir`, `("frankenphp","windows") = Dir`; `HTTPD_VERSION` stays 2.4.68 for macOS (the bottle) — the Windows row carries its own version via `pins_on(os)`? No: one `pins().httpd` is read by the gate, so the Windows arm is keyed on the SAME pin string… → **decision: `HTTPD_VERSION` moves to 2.4.69 on every OS only if a 2.4.69 bottle exists; else the catalog's httpd row for Windows is keyed by the macOS pin's version and NAMES 2.4.69 in its URL** — the pin is a catalog key, the artifact's own version is what `httpd -v` says (PORTS records both). Tests 7507/7542/7564 updated; PORTS Windows table rows; THIRD-PARTY-NOTICES rows for Apache httpd (Windows) + FrankenPHP (Windows). | L0 + windows-check |
| T3 | `core/apache.rs` `Layout::for_os`: bin name, module dir, MPM lines, mime path, shape; `ensure_httpd_dir` resolves by the layout's shape (`resolve_bundle` vs `resolve_dir`); adoption's `cached_bundle_dir(…,"bin/httpd")` → the layout's bin; `stop_policy::clean_exit_events` + `ap<pid>_shutdown`; `runtime_problem` before the Apache start on Windows. Config tests per OS (macOS text unchanged byte-for-byte; Windows text has `modules/`, `conf/mime.types`, `bin/httpd.exe`, no mpm/unixd). `ensure_server_available_on` now passes Windows Apache — tests 3273/3307 flip. | L0, planted; windows-check |
| T4 | FrankenPHP on Windows: `ensure_frankenphp_bin` resolves the `Dir` and returns `<dir>/frankenphp.exe`; `frankenphp_block` takes the core mail-catch rule (sendmail shim vs SMTP ini keys by OS); Caddyfile paths quoted (spaces in `%LOCALAPPDATA%`). Tests per OS. | L0; windows-check |
| T5 | UI + words: SiteDetail `<select>` filtered by `offeredWebServers`; New Site note; `apache_unavailable` (three constants) + the Linux OLS sentence fixed; TS mirror; `the_new_site_dialog_offers_the_servers_core_allows` asserts Windows offers Nginx + Apache + FrankenPHP, not OLS. | L0 + tsc/eslint |
| T6 | Docs: PLATFORMS §3 row (per-site servers: Windows = FrankenPHP + Apache (Apache Lounge, mirrored), OLS never) + §7 traps (mpm_winnt parent/child, the shutdown event, the VC runtime); ARCHITECTURE 277–279; PORTS 92–94 + Windows artifact rows; INSTALL "What is not in the Windows build"; PLAN-windows-port D4 + §4 W4; ledger #642/#643 verdicts + new rows (the layout rule, the mirror); SMOKE § Windows rows (Apache override, FrankenPHP override, the switch, the stop). | docs gates |
| T7 | **Run it on Windows**: a `tauri build`-shaped binary (cargo-xwin release with the frontend embedded and `build.devUrl` stripped, the Linux recipe's Windows twin) on the Win11 ARM VM (x64 under emulation — functional, slow): create a Blank PHP site on Apache → 200 through Caddy, `X-Powered-By` from the php-cgi group, `.htaccess` rewrite honoured, `Server:` header Apache; switch nginx ↔ Apache ↔ FrankenPHP on one site; FrankenPHP site 200 with `PHP_VERSION` 8.5.12, `mail()` lands in Mailpit via the SMTP keys; Stop site → the httpd parent AND child gone, port free; app quit + relaunch adopts the running httpd (marker = conf path on the cmdline); `rex site list` shows the server column. Ledger rows get `◐ (Win11 VM, emulated x64)` until the Dell runs the same. | installed-binary run + screenshots |

## 4. What this does NOT change

The edge (Caddy), the shared nginx, the php-cgi pool model, the port map, the MCP/CLI surface
(`offered_web_servers` is `Never` in parity and stays so; `rex site list` already prints the
server). No new IPC command.
