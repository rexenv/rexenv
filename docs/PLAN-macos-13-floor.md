# PLAN — lowering the macOS floor from 15 to 13: every component's `minos`, measured, and the pin each would need

**Status:** PLANNED — 23 Sep 2026. Measurement done (§1–§5, both slices, every component);
**the owner ruled the shape the same day (§6)**: macOS 15 stays the STANDARD — every feature,
every latest pin — and the app additionally RUNS on macOS 13 and 14 with a per-host pin set
and a per-feature refusal where no build exists. Task list in §7; **T0–T3 landed 23 Sep 2026** (ledger #707, #708, #709, #710), T4 next.
Planned against `99942bca`. Tracked as the "macOS 13 floor" row in `docs/TODO.md` (Now).

The stated floor is `tauri.conf.json` `minimumSystemVersion: 15.0`, asserted equal to
`max(minos)` over `binaries::DEFAULT_STACK` on both arches by
`examples/macos_floor_check.rs` (ledger #433). `docs/PORTS.md` §"Measured macOS floors"
carries the per-binary numbers for the pins that ship; this file carries the ALTERNATIVES.

---

## 1. Why the floor is 15 today — one binary

Nothing in rexenv's own code names a macOS version. Tauri 2 / wry / the `objc2` APIs the
app calls (`NSAlert`, `NSWindow`, `WKWebView`, kqueue) all exist since macOS 10.13. The
floor is set entirely by the deployment target baked into the binaries rexenv downloads,
and today **exactly one default-stack binary sits at 15.0: cloudflared 2026.6.1**. Every
other default-stack binary is 12.0 on both slices (`macos_floor_check`, 23 Sep 2026, ALL
PASS at 15.0):

```
caddy 2.11.4        12.0 / 12.0
nginx 1.30.4 (ours) 12.0 / 12.0
php 8.3.32 (ours)   12.0 / 12.0     php-fpm 8.3.32   12.0 / 12.0
mailpit 1.30.3      12.0 / 12.0
cloudflared 2026.6.1 15.0 / 15.0    ← the floor
```

So "macOS 13" is not one decision; it is one cheap re-pin for the DEFAULT stack, plus a
separate re-pin per optional engine, each binding only the user who enables it — and two
components with no upstream build for 13 at all (§3.2, §3.5).

## 2. How this was measured

Every number below was read from the artifact's load commands, never from a filename,
a release note or a runner label — those lied three times before (`PORTS.md`: the PHP
8.0 row, the nginx x86_64 slice, PostgreSQL's June builds). Method, per artifact:

- download the exact upstream artifact (release tgz / CDN tarball / ghcr bottle blob);
- extract **every** file and sweep **every** Mach-O in it — server, clients AND dylibs
  (`bin/`, `lib/*.dylib`, `lib/*/*.so`) — not the headline binary;
- `vtool -show-build`: `minos` from `LC_BUILD_VERSION`, or `version` from
  `LC_VERSION_MIN_MACOSX` on older x86_64 builds that still carry the legacy command
  (`core::macho` parses both, so a re-pin to such a build stays visible to the check);
- report the MAX and which file carries it; delete the download.

Homebrew bottles: `formulae.brew.sh` no longer lists a `ventura` bottle for ANY of the
formulas rexenv bundles (Homebrew dropped macOS 13 from its build matrix), so the
candidates were found by walking each formula's ghcr tag list newest→oldest and reading
each tag's OCI index for an `os.version: macOS 13.x` platform. Blobs are
content-addressed, so a ventura bottle published in 2025 is still fetchable by digest.

The scripts were throwaway (scratchpad); the durable check is `macos_floor_check`, which
re-derives §1 from the pins whenever they change.

## 3. The matrix — current pin vs. newest build that runs on macOS 13

`minos` written `arm64 / x86_64`. "13-capable" = the newest upstream build with
`max(minos) ≤ 13.0` on BOTH slices.

### 3.1 Default stack

| Component | Current pin | minos | 13-capable pin | minos | Cost of the move |
|---|---|---|---|---|---|
| Caddy | 2.11.4 | 12.0 / 12.0 | **same** | — | none (2.10.2 also 12.0; 2.9.1 is 11.0) |
| nginx (ours, `rexenv/runtimes`) | 1.30.4 | 12.0 / 12.0 | **same** | — | none — we set this target ourselves |
| PHP 8.1.34 / 8.2.32 / 8.3.32 / 8.4.23 / 8.5.8 (ours) | `php-8x-3…7` | 12.0 / 12.0 | **same** | — | none |
| PHP 7.4.33 (ours) | `php-7.4.33-6` | 12.0 / 12.0 | **same** | — | none |
| Mailpit | 1.30.3 | 12.0 / 12.0 | **same** | — | none (1.28.0 also 12.0) |
| FrankenPHP | 1.12.4 | 12.0 / 12.0 | **same** | — | none |
| **cloudflared** | **2026.6.1** | **15.0 / 15.0** | **2025.4.0** | 13.0 / 10.13 | ~17 months of client behind; flip is exactly 2025.4.0 → 2025.4.2 (Cloudflare's darwin toolchain moved from SDK 14.2 to 15.x there). Every 2025.4.2+ release measured is 15.0 on both slices |

cloudflared, the full series measured (arm64 / x86_64):

```
2026.9.1  2026.6.1  2026.5.0  2026.1.1     15.0 / 15.0
2025.11.1 2025.8.0  2025.4.2               15.0 / 15.0
2025.4.0  2025.2.1  2025.2.0  2025.1.1  2025.1.0   13.0 / 10.13
2024.12.2 2024.8.3                         13.0 / 10.13
```

**With cloudflared at 2025.4.0 the default stack's max(minos) is 12.0 on both slices** —
lower than 13. The stated floor would then be a CHOICE (13.0 or 14.0), not a measurement,
and `macos_floor_check` asserts equality in both directions, so the check's rule needs to
become "stated ≥ max(minos)" — or the stated floor must move to 12.0, which nobody has
tested and which this plan does not propose.

### 3.2 PHP 8.0.30 — static-php.dev, NOT ours

| Build | minos |
|---|---|
| 8.0.30 cli + fpm, arm64 | **14.0** |
| 8.0.30 cli + fpm, x86_64 | 13.0 |

The one PHP minor rexenv does not build itself (`binaries.rs` `php_self_hosted_tag`: its
x86_64 self-build aborts in static-php-cli's sanity check). static-php.dev rebuilds the bulk
artifact in place, so there is no older 8.0.30 to pin — the served bytes are 14.0 on
Apple Silicon. **On a 13 floor, 8.0 is either dropped, offered as "macOS 14+ only" (the
per-minor refusal shape `XdebugStatus` already has), or self-built with the x86_64 abort
fixed.**

### 3.3 MySQL — official CDN tarballs

MySQL's `macosNN` tarball is built with deployment target **NN − 1**, on every version
measured, both slices, all ~100 Mach-Os per tree:

| Version | tarball | minos (arm64 / x86_64) |
|---|---|---|
| **8.4.6 (pin)**, 8.4.5, 8.4.4 | macos15 | 14.0 / 14.0 |
| **8.4.3**, 8.4.2, 8.4.0 | macos14 | **13.0 / 13.0** |
| **8.0.44 (pin)**, 8.0.43, 8.0.42, 8.0.41 | macos15 | 14.0 / 14.0 |
| **8.0.40**, 8.0.36 | macos14 | **13.0 / 13.0** |
| 8.0.33 | macos13 | 12.0 / 12.0 |

13-capable pins: **8.4.3** (8.4 LTS; 3 patch releases behind) and **8.0.40** (4 behind).
Both fit 13.0 with zero margin. Note the URL template in `binaries.rs` hardcodes
`macos15` — a re-pin changes the template per version, not just the digest. Datadir:
8.0.40 → 8.0.44 is an in-series upgrade, so an existing 8.0.44 user must never be handed
8.0.40 — a lower floor applies to NEW installs, and the per-series datadir rule already
keeps 8.0 and 8.4 apart.

### 3.4 Homebrew bottle bundles — Redis, MariaDB, Apache, Xdebug

Homebrew publishes NO ventura bottle for any current version. The newest tag per formula
whose ghcr index still carries `macOS 13.x` bottles, and the measured minos of those blobs:

| Formula | Current pin (sonoma, 14.0) | Newest tag with a ventura bottle | minos (arm64 / x86_64) |
|---|---|---|---|
| redis | 8.8.0 | **8.2.1** | 13.0 / 13.0 |
| mariadb | 12.3.2 | **12.0.2** | 13.0 / 13.0 |
| mariadb@11.4 | 11.4.12 | **11.4.8** | 13.0 / 13.0 |
| openssl@3 (bundled into redis + mariadb) | 3.6.3 | **3.5.2** | 13.0 / 13.0 |
| pcre2 (bundled into mariadb + httpd) | 10.47 | **10.46** | 13.0 / 13.0 |
| httpd | 2.4.68 | **2.4.65** | 13.0 / 13.0 (124 Mach-Os) |
| apr | 1.7.6 | **1.7.6** (same version — apr still publishes a ventura blob; the DIGEST changes) | 13.0 / 13.0 |
| apr-util | 1.6.3 | **1.6.3_1** | 13.0 / 13.0 |
| xdebug (shivammathur) 8.0–8.4 | 3.5.3 | **3.4.5** | 13.0 / 13.0 (8.3 measured; 8.0/8.1/8.2/8.4 same tag, blobs found, not swept) |
| xdebug 8.5 | 3.5.3 | **none** — 3.4.5-3 is 13.0 but was built against the 8.4 Zend API (pre-GA); loads into no PHP 8.5 (`legacy_pins_check`, 23 Sep 2026) | — |
| xdebug 7.4 | 3.1.6 | **3.1.6** (same version, frozen minor — its ventura blob, not the pinned sonoma one) | 13.0 / — |

Every candidate is an older release of the same formula, so the bundle recipes
(`resolve_bundle`, `prepare_binary_tree`'s relink) apply unchanged; what changes is every
digest, and the load-test at pin time (`PORTS.md`'s "all 10 downloaded + load-tested"
rule for Xdebug) has to be re-run against each. These are the multi-hundred-MB trees;
that re-pin is the bulk of the work.

**A standing hazard this walk exposed, unrelated to 13:** `formulae.brew.sh` today lists
NO Intel macOS bottle at all for `redis` (8.10.2) or `mariadb` (13.0.2) — only
`arm64_*` and Linux. The pinned `sonoma` x86_64 blobs still resolve by digest, but the
NEXT routine bump of either formula has no x86_64 macOS bottle to pin. That is its own
TODO row, not this plan's.

### 3.5 PostgreSQL — theseus-rs portable builds: NO arm64 build for 13 exists

| Version | minos (arm64 / x86_64) |
|---|---|
| **18.6.0 / 17.11.0 / 16.15.0 (pins)** | 15.0 / 15.0 |
| 17.4.0, 17.2.0, 16.8.0, 16.6.0, 15.12.0 | 15.0 / **13.0** |
| 16.4.0, 16.3.0, 16.2.0, 15.8.0 | **14.0** / 13.0 |
| 15.6.0 | 14.2 (`hstore_plperl.so`) / — |

theseus-rs builds arm64 on Apple Silicon GitHub runners, which have never been older than
macOS 14; x86_64 was built on `macos-13` runners until ~early 2025. **There is no upstream
PostgreSQL build that runs on an Apple Silicon Mac at macOS 13.** On a 13 floor PostgreSQL
is either self-built (the nginx / PHP route, `rexenv/runtimes`), or offered as "macOS 14+"
on arm64 (16.4.0) and "macOS 13+" on x86_64 (≤17.4.0) — a per-arch floor the descriptor
does not model today.

### 3.6 OS-agnostic — no floor

WP-CLI 2.12.0, Composer 2.10.2, Adminer 6.1.0 are phars/PHP run by the bundled PHP.

## 4. What 13 costs, summed

| Tier | Components | Work |
|---|---|---|
| Free | Caddy, nginx, PHP 7.4 + 8.1–8.5, Mailpit, FrankenPHP | already 12.0 on both slices |
| One re-pin | cloudflared → 2025.4.0 | new digests; accept a 17-month-old tunnel client (Cloudflare may deprecate old quick-tunnel clients server-side — untested) |
| One re-pin, older patch level | MySQL → 8.4.3 / 8.0.40 | new digests + per-version URL template; 3–4 security patches behind |
| Bulk re-pin | Redis 8.2.1, MariaDB 12.0.2 / 11.4.8, openssl@3 3.5.2, pcre2 10.46, httpd 2.4.65, apr-util 1.6.3_1, Xdebug 3.4.5 ×4 | ~18 ghcr digests, each bundle relinked + load-tested again |
| Self-build or drop | PHP 8.0.30 (arm64 is 14.0), PostgreSQL (arm64 ≥ 14.0 everywhere) | either a `rexenv/runtimes` build with `MACOSX_DEPLOYMENT_TARGET=13.0`, or a per-minor / per-engine "needs macOS 14+" refusal |
| Check + docs | `macos_floor_check` rule (≥ instead of ==), `PORTS.md` table, `INSTALL.md` requirement line, `tauri.conf.json` | small |
| Proof | a macOS 13 VM run of the smoke test | `minos` is metadata; dyld enforcement is a property of the OLD host and cannot be observed on this machine (`PORTS.md`) |

**Floor 14 for comparison:** cloudflared 2025.4.0 is the ONLY change — every engine
already sits at 14.0 (sonoma bottles, macos15 MySQL, PHP 8.0) and PostgreSQL 16.4.0 is
14.0 on arm64. Same proof requirement (a macOS 14 VM).

## 5. What this plan does NOT decide

- Whether Cloudflare still accepts a 2025.4.0 client for quick tunnels a year from now.
- Whether an Intel Mac on macOS 13 actually loads a `LC_VERSION_MIN_MACOSX 10.13` binary
  next to `LC_BUILD_VERSION 13.0` ones — expected yes; unmeasured, as every run-claim here.
- Per-arch floors in the app descriptor (`offer_for`), which §3.5's PostgreSQL split would need.

---

## 6. The design the owner chose — one standard, two legacy tiers

Ruled 23 Sep 2026. Three rules, in the owner's order:

1. **A component whose latest build needs 15 gets its LAST 13-capable build on 13** —
   the binary that downloads is chosen by the host's macOS version from a predefined
   per-version mapping. On 15 the standard pin stays exactly what it is today.
2. **A component with no 13-capable build at all is DISABLED on the old host**, with the
   refusal saying which macOS it needs (PostgreSQL on 13 is the example).
3. **Where rexenv can build it, rexenv builds it** (`rexenv/runtimes`, the nginx / PHP route)
   and the row moves from rule 2 to rule 1 later — a follow-up, never a blocker for shipping
   rules 1 and 2.

macOS 15 is therefore the *required* version for the full feature set, and 13 is the
*minimum* the app runs on with a stated, visible subset.

### 6.1 The tier — ONE fact, derived once, platform-owned

```
BinaryTier::Standard   host ≥ 15.0      every pin as today
BinaryTier::Legacy14   host 14.x        cloudflared 2025.4.0; PostgreSQL 16.4.0; all else standard
BinaryTier::Legacy13   host 13.x        the §6.2 set; PostgreSQL + PHP 8.0 refused
```

- Derived from the host version by the PLATFORM (`Platform::binary_tier()`, a capability
  in `platform/traits.rs`; macOS reads `core::macho::host_macos()`, Windows and Linux return
  `Standard` — a tier is a macOS fact and `core/` may not name an OS). Below 13.0 the app
  does not launch: `minimumSystemVersion` becomes `13.0` and the installer/Gatekeeper refuse
  earlier, as they refuse 14 today.
- Computed at launch, published like `CATALOG` (`binaries::install_tier`), read by
  `binaries::pins()` — a `PinSet` struct holding every version the 18 consumer files read
  as constants today. The constants stay as the Standard tier's values; consumers move to
  the accessor. **No consumer may read a `*_VERSION` constant directly after this lands** —
  a guard test walks `src-tauri/src` for the pattern, the same shape as ledger #163's
  no-OS-in-core guard.
- **Recomputed every launch, never stored.** macOS only moves up, so a 13 host that becomes
  a 15 host simply resolves the standard pins next launch (§6.4 says why that is safe).

### 6.2 The Legacy13 pin set (every number measured in §3, both slices)

| Component | Standard (15+) | Legacy13 |
|---|---|---|
| Caddy, nginx, Mailpit, FrankenPHP, PHP 7.4 + 8.1–8.5 | as today | **same** (12.0) |
| cloudflared | 2026.6.1 | **2025.4.0** |
| MySQL | 8.4.6 / 8.0.44 (`macos15`) | **8.4.3 / 8.0.40** (`macos14`) — URL template per version |
| Redis (+ openssl@3) | 8.8.0 (+ 3.6.3) | **8.2.1 (+ 3.5.2)**, ventura blobs |
| MariaDB (+ openssl@3, pcre2) | 12.3.2 / 11.4.12 | **12.0.2 / 11.4.8** (+ 3.5.2, 10.46), ventura blobs |
| Apache httpd (+ apr, apr-util, pcre2) | 2.4.68 (+ 1.7.6, 1.6.3, 10.47) | **2.4.65** (+ 1.7.6 ventura, 1.6.3_1, 10.46) |
| Xdebug 8.1–8.4 / 8.5 / 7.4 | 3.5.3 / 3.5.3 / 3.1.6 | **3.4.5 / none (NotPinned) / no row either way** — 8.5's ventura blobs predate 8.5 GA and load into no 8.5 |
| **PostgreSQL** | 18.6.0 / 17.11.0 / 16.15.0 | **refused — "needs macOS 14"** (rule 2; rule 3 later) |
| **PHP 8.0.30** | static-php.dev | **refused — "needs macOS 14"** (arm64 is 14.0; the x86_64 slice would run, but one rule per feature beats a per-arch feature — owner may relax) |
| WP-CLI, Composer, Adminer | as today | same (no floor) |

Legacy14 differs from Standard in exactly two rows: cloudflared 2025.4.0 and PostgreSQL
16.4.0 (arm64 14.0 / x86_64 13.0 — the newest theseus build that runs on 14). Every other
standard pin is already 14.0 or lower.

### 6.3 Refusals are data the UI renders, not strings the UI owns

A feature the tier cannot serve is refused where its catalog is built, in Rust, with the
sentence beside the rule (`platform/words.rs`, per CLAUDE.md "a UI string that describes a
RULE lives in Rust"): `Availability::NeedsMacos { major: 14 }` on the PostgreSQL engine
row and the PHP 8.0 minor row. It surfaces in every place that lists them — the
Databases engine picker, New Site's PHP + DB pickers, Settings' PHP install list, the DB
version switch, `rex php install` / `rex db versions --set`, and the MCP server (parity
rule) — as a disabled row with the sentence, never a silent omission (`XdebugStatus` is
the existing shape). Onboarding says ONCE, on a legacy host, what the subset is
(`docs/DESIGN.md` honest-UI rule).

### 6.4 An OS upgrade is a forward pin move — and only forward

A Legacy13 install that upgrades to macOS 15 resolves the standard pins on next launch:
MySQL 8.4.3 → 8.4.6 and 8.0.40 → 8.0.44 are in-series upgrades `mysqld` performs on its
own datadir; MariaDB 12.0.2 → 12.3.2 and 11.4.8 → 11.4.12 likewise; Redis and cloudflared
hold no state. PostgreSQL was refused, so there is no datadir to carry. The reverse move
never happens (macOS does not downgrade), so no engine is ever handed a NEWER datadir than
it can read. The forward path gets one L1 proof per engine (start a 13-pinned datadir on
the standard binary) before this ships.

### 6.5 The signed catalog and the floor check learn about tiers

- `updates::Artifact` gains `min_macos` (set by `scripts/publish-manifest.sh` from
  `vtool`, refused by the reader when absent on a macOS entry); `VersionCatalog::newer`
  skips an artifact whose `min_macos` outranks the host. Otherwise a Legacy13 host is
  offered a PHP patch built at 14.0 the week it is published — the exact failure this plan
  exists to prevent, arriving by the update path instead of the pin path.
- `macos_floor_check` becomes per-tier: for each tier, `max(minos)` over that tier's
  default stack ≤ the tier's floor, AND `tauri.conf.json`'s stated floor == the LOWEST
  tier's floor (13.0). The `==` rule stays, applied to the right set: a stated 13.0 with a
  Legacy13 stack at 12.0 is fine (a floor is a promise about the OS, not a measurement of
  the bytes); a Legacy13 stack that drifts to 14.0 is red.
- `manifest_sweep_check` enumerates the Legacy13 artifacts too (they are new pins), and
  the unit test `manifest_pins_every_pinned_php_version`'s shape extends to every tier row.

### 6.6 Rule 3 — the self-build track (follow-ups, in value order)

1. **PostgreSQL, both slices, `MACOSX_DEPLOYMENT_TARGET=13.0`**, `rexenv/runtimes`, the
   nginx recipe. Flips §6.2's PostgreSQL row from refused to a pin on 13 AND 14.
2. **PHP 8.0.30** — blocked on static-php-cli's x86_64 abort (`binaries.rs`,
   `php_self_hosted_tag`); an arm64-only self-build would need a per-arch feature, which
   §6.2 declines. Stays refused until the abort is fixed upstream or worked around.
3. **Redis / MariaDB / httpd** — not needed for 13 (ventura blobs exist), but the Intel
   bottle row in `docs/TODO.md` ("Blocked on external work") says Homebrew is dropping
   the x86_64 macOS platform too; one `rexenv/runtimes` build per engine ends both
   dependencies. Not part of this plan's ship line.

### 6.7 What this plan accepts

- cloudflared 2025.4.0 is ~17 months old on legacy hosts; if Cloudflare's edge stops
  accepting it, the tunnel feature on 13/14 degrades to a refusal ("needs macOS 15") —
  the same shape as rule 2, so the code path already exists.
- MySQL 8.4.3 / 8.0.40 are 3–4 security patches behind on legacy hosts. Stated in
  `INSTALL.md`.
- `minos` is metadata: every row above is proven on a real 13 and a real 14 VM before the
  release that carries it (§7 T7). Intel hardware is still unavailable; the x86_64 rows
  rest on measured metadata alone, as `PORTS.md` already says of the Intel digests.

## 7. Task list

Each task is one commit with its docs (CLAUDE.md's table); order is dependency order.
"Done when" is what the commit proves, not what it intends.

**T0 — the tier, and nothing that uses it yet.** ✓ 23 Sep 2026, ledger #707.
`BinaryTier` enum in `core/binaries.rs`; `Platform::binary_tier()` in `platform/traits.rs`
(macOS impl from `host_macos()`, Windows + Linux `Standard`); `install_tier` / `tier()`
beside `CATALOG`. Unit tests: 12.x → the platform refuses (not a tier), 13.7 → Legacy13,
14.0 → Legacy14, 15.0 and 26.x → Standard. Ledger row: "the tier is derived, never stored".
*Done when:* tests green, no consumer changed, `verify.sh` green.

**T1 — `PinSet` and the accessor; consumers migrate; the guard lands.** ✓ 23 Sep 2026, ledger #708 — the guard is STRUCTURAL (constants private; the compiler refuses a direct read), not a source scan.
`pins()` returns the Standard set; the 18 consumer files read `pins().mysql_version` etc.;
the `*_VERSION` constants become the Standard set's initialisers. Guard test: no direct
constant read outside `binaries.rs`. *Done when:* behaviour byte-identical on 15
(`macos_floor_check` ALL PASS unchanged), guard green, ledger row.

**T2 — Legacy13 + Legacy14 pins, with digests.** ✓ 23 Sep 2026, ledger #709 — tables stay tier-blind (a legacy row is its own version); `legacy_pins_check` is the run proof on this host.
For every §6.2 row: download both slices, hash, pin (MySQL: per-version URL template;
bottles: ventura blob digests; cloudflared 2025.4.0 tgz digests). `manifest(...)` and
`bundle_manifest(...)` take the tier through `pins()`, not a new parameter. Unit test per
tier: every pinned artifact has a manifest for both arches. `PORTS.md`: a Legacy table
beside the standard one. *Done when:* `manifest_sweep_check` (network tier) resolves and
re-hashes every Legacy artifact; the bundles relink and load-test (`prepare_binary_tree`)
on this Mac.

**T3 — the refusals.** ✓ 23 Sep 2026, ledger #710 — `Availability` became two derived accessors (`php_minor_needs_macos`, `engine_needs_macos`) rather than an enum; the test tier is thread-local under `cfg(test)`.
`Availability::NeedsMacos` on PostgreSQL (13) and PHP 8.0 (13); the sentence in
`platform/words.rs`; every list site in §6.3 renders it disabled-with-reason; `rex` and
MCP refuse with the same words (parity test). Copy-scan guard covers the new sentence.
*Done when:* an L0 test drives each list site with a fixture tier and asserts the row is
present, disabled, and carries the sentence; no site omits the row.

**T4 — the floor moves: `13.0`, and the checks follow.**
`tauri.conf.json` `minimumSystemVersion: 13.0`; `macos_floor_check` per-tier (§6.5);
`DEFAULT_STACK` becomes per-tier (`default_stack(tier)`); `INSTALL.md` requirements
rewritten ("13 runs, 15 required for everything — what 13/14 lack"); `SMOKE-TEST.md` gains a
Legacy section; `ARCHITECTURE.md` gets the tier paragraph. *Done when:* `macos_floor_check`
ALL PASS with three tiers, `status.py --check` green, `doc-counts.sh` green.

**T5 — the catalog learns `min_macos`.**
`updates::Artifact.min_macos`; reader refuses a macOS entry without it; `newer` filters by
host; `scripts/publish-manifest.sh` fills it from `vtool`; `check-app-manifest-test.sh`
covers the field. *Done when:* an L0 test proves a 14.0 artifact is not offered to a 13.7
host and is offered to 15.0; a published test manifest round-trips.

**T6 — forward-upgrade proofs (§6.4).**
One L1 example per stateful engine: init a datadir on the Legacy13 binary, start it on
the Standard binary, query it. MySQL 8.4 + 8.0, MariaDB 12 + 11.4. Sandbox tier,
fixture-owned. *Done when:* `live-checks.sh` lists them and they pass here.

**T7 — the human gate: real 13 and 14.**
UTM arm64 VMs for macOS 13.7 and 14.x (the 15.6.1 VM recipe in `SMOKE-TEST.md`): clean
install, onboarding says the subset, WordPress + Laravel sites serve over HTTPS, MySQL /
MariaDB / Redis / Apache / Xdebug start from the Legacy pins, a tunnel opens on cloudflared
2025.4.0, PostgreSQL and PHP 8.0 show the refusal, self-update offers the next build.
Then upgrade the 13 VM to 15 in place and re-run T6's proof through the GUI. *Done when:*
`SMOKE-TEST.md`'s Legacy section carries ✓ rows with VM + build ids, and the ledger rows
say which OS each proof came from.

**T8 — release.** `RELEASING.md`'s order; the release note names the subset. Rule 3
(§6.6) opens as its own TODO rows after this ships.

Not in this list, deliberately: any Intel run (no hardware), the PostgreSQL self-build
(T8's follow-up), and relaxing PHP 8.0 to x86_64-only-on-13 (owner's call, one line in T3
if taken).
