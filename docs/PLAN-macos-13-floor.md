# PLAN — lowering the macOS floor from 15 to 13: every component's `minos`, measured, and the pin each would need

**Status:** MEASURED, NOT STARTED — 23 Sep 2026. Every macOS component's `minos` is now
measured on BOTH slices (arm64 + x86_64), for the CURRENT pin and for the newest upstream
build that would run on macOS 13. No pin has been changed. The decision this file exists to
inform — whether to pay for 13, stop at 14, or keep 15 — is the owner's; the cost per
component is in §4. Planned against `6a671903`. Tracked as the "macOS 13 floor" row in
`docs/TODO.md` (Parked); this file is the measurement behind it.

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
| xdebug 8.5 | 3.5.3 | **3.4.5-3** | 13.0 / — (x86_64 blob found, not swept) |
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
| Bulk re-pin | Redis 8.2.1, MariaDB 12.0.2 / 11.4.8, openssl@3 3.5.2, pcre2 10.46, httpd 2.4.65, apr-util 1.6.3_1, Xdebug 3.4.5 ×5 | ~20 ghcr digests, each bundle relinked + load-tested again |
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
