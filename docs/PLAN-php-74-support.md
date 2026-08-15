# PHP 7.4 support — where the binary comes from, and what shipping it costs

**Status: SHIPPED 15 Aug 2026.** PHP 7.4.33 is a first-class minor: built by
`rexenv/runtimes` (immutable releases; the live tag is `php_self_hosted_tag` in
`core/binaries.rs` and is deliberately not restated here — it has moved twice
since this line first spelled `-1`), pinned by SHA-256, and run-proven through
rexenv's own download → verify → relink → sign → exec path
(`php_versions_check`: *"all 7 PHP versions resolved"*). Planned against `67712c7`.

Supersedes the standing claim, repeated in four docs and eight tests, that
**7.4 "has no build and never will."** That sentence was true about
*static-php.dev* and false about *PHP*. It is retired, and every doc that carried
it is corrected rather than merely annotated.

**What shipped vs what is deferred**, so the difference is stated and not discovered:

| | |
|---|---|
| ✅ 7.4.33 cli + fpm, both arches, pool 9774 | 60 extensions incl. **gd, intl, mysqli, redis** |
| ✅ EOL tell covering 7.4 **and** 8.0/8.1 | ✅ FrankenPHP × 7.4 refused in core (#326) |
| ✅ Licence texts ship beside the bytes (#336) | ✅ …and a guard on the notices claim |
| ❌ **No Xdebug on 7.4** — measured, §10c | ❌ No `opcache` — spc cannot build it for 7.4 |
| ⏳ Extension set narrower than the 8.x rows | S1.2 in `docs/TODO.md` |

Goal: a rexenv site can run PHP 7.4, from the same picker, with the same pool model, as
every other minor — because the legacy WordPress and Laravel projects developers actually
have to open are on 7.4, and today rexenv refuses them by name.

---

## 1. What was believed, and what is actually true

`core/binaries.rs:24-26` says:

> 7.4 is deliberately absent: static-php.dev never published it — offering it needs a
> self-built + self-hosted artifact (same blocked path as the Xdebug debug build).

The first clause is still true. Re-verified 14 Aug 2026 — all eight URL shapes 404:

```
404  https://dl.static-php.dev/static-php-cli/bulk/php-7.4.33-{cli,fpm}-macos-{aarch64,x86_64}.tar.gz
404  https://dl.static-php.dev/static-php-cli/bulk/php-7.4.30-{cli,fpm}-macos-{aarch64,x86_64}.tar.gz
```

The second clause — "same blocked path" — is what turned out to be wrong, in three ways:

| Belief | Reality (proven, 14 Aug 2026) |
|---|---|
| static-php-cli cannot build 7.4 | It can. There is **no minimum-version floor anywhere in spc**; `--with-php` is format-validated only (`command/DownloadCommand.php:100-106`). `spc download --with-php=7.4 php-src` + `spc extract php-src` were **run** and completed, taking spc's own sub-8.0 patch branch. spc main still carries a variable literally named `$json_74` (`builder/macos/MacOSBuilder.php:92`); spc v3 lists `'7.4'` in `SUPPORTED_MAJOR_VERSIONS` (`Package/Target/php.php:54`). 7.4 was a first-class macOS-arm64 CI target as recently as Jan 2025 (`git show dc19d0c6:.github/workflows/build-macos-aarch64.yml`) — it was dropped from the **CI matrix**, not from the code. |
| PHP 7.4 no longer compiles on current macOS | It does, and fast. A full WordPress extension set built against **today's** deps (curl 8.21.0, ICU 78.3, OpenSSL 3.6.3, libzip 1.11.4, oniguruma 6.9.10, pcre2 10.47) — configure exit 0, make exit 0, **zero compile errors**, on Apple clang **21** / macOS 26.6.1, i.e. four clang majors *newer* than the GitHub runner default. Clean build of PHP itself at `-j3` (the arm64 runner's core count): **73.7 s**. Runtime proof: `intl` formats `1.234,56 €` for de_DE; `mb_strtoupper("héllo")=HÉLLO`; gd/mysqli/sodium/zip/dom live; AES round-trips. |
| Only a `dl.` host could serve it | GitHub Releases is strictly better and needs **zero** app-side changes — see §6. |

Two corrections to things this repo currently asserts, both landing in the same commit as
the fix (docs ship WITH the code):

- **`docs/PLAN-valet-herd-import.md:95`** — *"7.4 has no build and never will"*. Wrong on
  the second half. A Valet/Herd project isolated to 7.4 is exactly the import cohort this
  feature exists for.
- **`docs/PORTS.md:40`** — *"NO 7.4 (never published — needs self-hosting, like the Xdebug
  build)"*. The parenthetical is right about the mechanism and wrong about it being blocked.

**A doc that is now WRONG outranks a doc that is merely incomplete.** These two sentences
send the next reader to build a workaround for a wall that is not there.

---

## 2. Three candidate sources, all three investigated to the point of running

Nothing below is inference. Each row was executed.

### (A) Homebrew bottle bundle — `shivammathur/php` `php@7.4` + its dylib closure

The mechanism rexenv already owns (`BundleSpec` / `resolve_bundle` / `prepare_binary_tree`,
used for redis, mariadb, httpd, xdebug). **It works**: 37 ghcr bottles (php@7.4 tag
`7.4.33_13-4` + 36 homebrew-core deps, 47 dylibs) merged into one rexenv-shaped tree,
relinked by a faithful port of `MacosBinaryProvider::prepare_binary_tree`, produced
`PHP 7.4.33 (cli)` and `php-fpm 7.4.33` serving a real FastCGI request:
`PHPVER=7.4.33|SAPI=fpm-fcgi|GD=y|CURL=y|MYSQLI=y|SODIUM=y|OPENSSL=y|TLS=ok`.

The x86_64 gap the brief recorded as a blocker is **closed** — tag `7.4.33_13-4` ships an
x86_64 `sonoma` bottle, matching rexenv's arm64_sonoma+sonoma convention. ghcr pin
durability is **real** — openssl@3 3.3.2, icu4c@74 74.2, and even formulas *deleted* from
homebrew-core (openssl@1.1 1.1.1w, jpeg 9f) are all still pullable, and blobs are
content-addressed so a pinned digest cannot change under us.

It is still the wrong choice, for five reasons that are structural rather than fixable:

1. **Two Homebrew-prefix leaks that are invisible on any machine that has Homebrew.**
   The bottle bakes `PHP_CONFIG_FILE_PATH=/opt/homebrew/etc/php/7.4`; `core::services::start_fpm`
   passes only `-F -y <conf>` (`core/services.rs:175-182`) — no `-n`, no `-c`. Observed on
   this machine: the bundled binary printed `with Xdebug v3.1.2` and `with Zend OPcache`
   and loaded the developer's *own* extensions from `/opt/homebrew/lib/php/pecl/20190902`.
   Worse, bundled `libcrypto.3.dylib` carries `OPENSSLDIR: "/opt/homebrew/etc/openssl@3"`,
   whose `cert.pem` is a symlink into Homebrew's `ca-certificates`. With that path absent —
   **a Mac without Homebrew, which is rexenv's entire premise** — every openssl-stream TLS
   call from 7.4 fails. Proven: `SSL_CERT_FILE=/dev/null` → `file_get_contents('https://…')`
   returns `false`; adding `-d openssl.cafile=/etc/ssl/cert.pem` → `true`. rexenv's static
   8.x builds bake `OPENSSLDIR: "/etc/ssl"`, which macOS always ships. **No test run on a
   developer Mac will ever reveal this bug.**
2. **Copyleft the bundle cannot drop.** The load commands are compiled into the bottle, so
   these ship or php does not launch: `libsybdb` (freetds, GPL-2.0-or-later), `libaspell`/
   `libpspell` (LGPL-2.1), `libodbc` (LGPL-2.1+), `libltdl` (GPL-2.0+), `libintl` (gettext,
   GPL-3), `libgmp` (GPL-3/LGPL-3). **None** of these appear in the static 8.x builds. rexenv
   would be modifying each dylib (`install_name_tool` + ad-hoc re-sign) and redistributing it,
   linked against a PHP-3.01 core. That is exposure the bottle path *creates*.
3. **Pin surface.** 38 bottles × 2 arches ≈ 76 digests, plus 59 exact `include` entries, of
   which 12 are symlinks whose real target embeds an upstream version (`libzstd.1.dylib` →
   `libzstd.1.5.7.dylib`, …) so **both hops** must be pinned and both strings move on any
   bump. rexenv's largest existing bundle is httpd, at 4 parts. The lazy alternative fails
   loudly: whole-`lib/` includes are 426 MB and hard-error.
4. **Download weight.** 209.6 MB compressed per arch (241.6 MB with intl's icu4c), versus
   71.1 MB for the static 8.3 cli+fpm pair. `aspell` alone is 121.2 MB downloaded to keep
   0.54 MB of dylib, and a ghcr layer cannot be partially fetched.
5. **Resolver shape mismatch.** `php`/`php-fpm` resolve today as single-file binaries
   (`core/php.rs:439,474` → `binaries::resolve`), and `resolve`/`resolve_bundle` are
   documented as disjoint (`binaries.rs:731-733`). A bottle 7.4 needs new arms in
   `resolve_any`, `is_cached`, `cached_bin`, both `php.rs` call sites, and the
   `php_versions_check` example — for one minor.

### (B) Self-build, plain autotools

Proven end to end (§1). Produces a single Mach-O whose complete dylib closure is
`/usr/lib/libSystem.B.dylib`, `libz.1.dylib`, `/usr/lib/libresolv.9.dylib` — checked against
rexenv's real gate at `platform/macos/mod.rs:1579-1591`: `/usr/lib/*` passes untouched and
`libz.` is one of the four `KNOWN` names. **`relink_to_system_libs` accepts this binary
unmodified**, and it still ran after `codesign --force -s -`. x86_64 cross-compiles from
arm64 (`CC="clang -arch x86_64" --host=x86_64-apple-darwin`) and both SAPIs execute.

The gap: it proves *PHP* builds in 74 seconds; it used Homebrew's **prebuilt** static `.a`
files for the deps. Building ICU/OpenSSL/curl/libzip/gd from source with a consistent
`MACOSX_DEPLOYMENT_TARGET` is the unmeasured cost, and it is the whole job.

### (C) Self-build **through static-php-cli** — the choice

(B)'s proof plus (A)'s "someone already does this in production", minus both of their costs.

- **The dependency chain is spc's problem, not ours.** `spc download --prefer-pre-built`
  fetches pre-built static deps; `spc build` drives the rest.
- **A shipping commercial product already does exactly this.** Laravel Herd's macOS PHP
  binaries are spc-built — `php82 -r 'echo ini_get("static-php-cli.version");'` prints
  `2.8.6` (that ini key is injected only by spc's `SourcePatcher::patchSPCVersionToPHP`),
  the compiled-in configure line matches spc's macOS `config/env.ini:144` verbatim, and
  `otool -L` shows only system libs. Herd ships PHP **7.4** through 8.5.
- **The artifact shape is byte-for-byte the shape rexenv already consumes**: one member per
  `.tar.gz`, `php` / `php-fpm`, `Archive::TarGz`. App-side cost = one manifest arm + four
  SHA-256 consts.
- **`php -m` parity with the 8.x rows** is achievable because it is the same builder that
  produced them — the honesty problem a divergent extension set would create does not arise.
- **We choose the extension set**, so the GPL/LGPL dylibs (A) forces on us (freetds, pspell,
  gettext, odbc, gmp) simply are not in the build.
- **`--custom-url php-src:<url>` / `--custom-git php-src:<branch>:<repo>`** exist
  (`command/DownloadCommand.php:36-37`), which is what lets us build a *patched* source
  instead of php.net's tarball. That matters — see §3.

### Decision

**(C) — build PHP 7.4.33 with static-php-cli in a public `rexenv/runtimes` repo, host the
four artifacts as immutable GitHub Release assets, pin their SHA-256s in `core/binaries.rs`.**

| | (A) bottle bundle | (C) spc self-build |
|---|---|---|
| Proven running | ✅ | ✅ (PHP+exts; spc leg proven to extract/patch) |
| App-side wiring | new bundle arms in 6 places | 1 manifest arm + 4 consts |
| Pins to maintain per security update | ~76 digests | 4 |
| Download per arch | 209.6 MB | ~35 MB (8.x parity) |
| TLS on a Homebrew-less Mac | **broken** | fine (`OPENSSLDIR=/etc/ssl`) |
| Reads developer's own php.ini | **yes**, silently | no |
| GPL/LGPL dylibs redistributed | 6 | none (set is ours) |
| opcache | ✅ (shared `.so`, new ini wiring) | ❌ (§4.1) |
| intl | shared `.so` + a 38th bottle | in-tree |
| Owner of the pin's durability | shivammathur + Homebrew | us |

(A) is kept in this document rather than deleted because it is a **real, working fallback**
if the build repo stalls, and because the relink bug it exposed (§5.2) is ours either way.

---

## 3. The source is NOT php.net's tarball

Vanilla `php-7.4.33.tar.gz` **fails** against OpenSSL 3.6:

```
ext/openssl/openssl.c:1520:51: error: use of undeclared identifier 'RSA_SSLV23_PADDING'
```

(confirmed removed: `grep -rn RSA_SSLV23_PADDING /opt/homebrew/opt/openssl@3/include/openssl/` → nothing).

Build **`shivammathur/php-src-backports` @ branch `PHP-7.4-security-backports`**, the same
source Homebrew's `php@7.4` formula (revision 13) uses. Tip `5a576d8eb53e44aff3af9259cfd29e599f604471`;
tarball SHA-256 `d82887f2166e8526ea9b1cfd8c5ecf5649718f0b6e341380d333eba8066429a4`, downloaded
and hashed 14 Aug 2026 — byte-identical to the formula's pin. It guards the OpenSSL 3 case at
`ext/openssl/openssl.c:1524` with `#ifdef RSA_SSLV23_PADDING`, and carries the Xcode-16 clang
inline-asm fix, the `ITIMER_PROF`→`ITIMER_REAL` Apple-Silicon timeout fix, and
`-Wstrict-prototypes`/scanf function-pointer fixes.

**The branch is actively maintained** — last 8 commits landed 2026-07-30, carrying 2026
security work (phar circular-symlink GHSA-vc5h-9ppw-p5f3, ext-pgsql `E'…'` SQL injection,
libgd CVE-2026-9672, an `openssl_encrypt()` heap overflow). So the premise "7.4 is EOL,
therefore the pin never moves" is **false**, and correcting it *strengthens* the case for
(C): there is a rebuild cadence either way, and (C) costs 4 constants per round where (A)
costs ~76 digests.

**The branch is rebased, not appended.** Committer dates of the last 8 commits are all
`2026-07-30T08:55:00Z` while author dates span 2021→2025 — a replayed history. So
`5a576d8…` stops being reachable from any ref at the next security update, and GitHub's
codeload archive for an unreachable commit is not contractually guaranteed to persist.

→ **Mirror the source tarball as an asset in the same rexenv release as the binary**, and
pin its SHA-256 in the workflow. This is not optional; it is the only thing that makes the
build reproducible from URLs alone.

---

## 4. What a 7.4 row costs that an 8.x row does not

These are the honest divergences. Each needs a decision, and each needs to be *said* in the
UI rather than discovered.

### 4.1 No opcache

spc cannot build opcache for 7.4: `builder/extension/opcache.php:18` guards on
`getPHPVersionID() < 80000`, and the escape hatch does not rescue it —
`SourcePatcher::patchMicro()` early-returns on `'74'` and its patch series is
`['80','81','82','83','84','85']`, so the static-opcache patch never lands.

This is a **static-php-cli** limitation, not a PHP one: a normal autotools build produces a
working `modules/opcache.so` (proven — loaded as `zend_extension`, `opcache_enabled=YES`).
7.4 has no JIT, so the Apple-Silicon JIT problem does not exist.

**Ruling: ship 7.4 without opcache.** It is a performance feature; WordPress runs fine
without it; and taking it would mean either abandoning spc (losing extension-set parity and
the dependency chain) or shipping a tree instead of a binary (losing the whole app-side
saving). The Settings row must say so — see §5.5.

### 4.2 WordPress will flag every 7.4 site, permanently

Called live 14 Aug 2026:

```
api.wordpress.org/core/serve-happy/1.0/?php_version=7.4.33
→ {"recommended_version":"8.3","minimum_version":"7.4",
   "is_supported":false,"is_secure":false,"is_acceptable":false}
```

WP core runs — the current offer (WP 7.0.4) has `php_version: 7.4`, exactly the floor — but
**every 7.4 site will show WordPress's own outdated-PHP dashboard nag and a Site Health
critical.** If rexenv does not pre-announce this in the version picker, users will file it
as a rexenv bug. This is the single highest-probability support cost of the feature.

Note the headroom: **zero**. The next WP release that raises the floor to 8.0 turns every
rexenv 7.4 site into one that cannot update WordPress. rexenv should read `php_version` from
the version-check offer it already talks to rather than hardcoding a belief.

### 4.3 EOL, and rexenv currently says nothing about it

`grep -ri 'eol\|end of life' src/` → **0 hits**. PHP 8.0 has been EOL since Nov 2023 and the
app is silent. Shipping 7.4 without an EOL tell makes it the *second* silently-dead runtime,
which `docs/DESIGN.md`'s "the sentence in front of the button that starts it" rule forbids.

**Adding a badge for 7.4 only, and leaving 8.0 silent, is worse than doing nothing** — it
implies 8.0 is supported. The tell must cover both, or it must not ship.

### 4.4 OpenSSL 3 legacy provider is off

Proven on the self-build: `openssl_encrypt('s','bf-cbc',…)` returns `false`, while
`aes-256-cbc` works. Blowfish/RC4/DES and RC2-bearing PKCS#12 are dead unless the legacy
provider is compiled in **and** activated at MINIT (a static build cannot dlopen
`legacy.dylib`). Legacy WP plugins do hit this. **Not 7.4-specific** — rexenv's existing 8.x
static builds behave identically — so this is a *decision to record*, not a 7.4 blocker.

### 4.5 The x86_64 artifact is on a clock

`macos-15-intel` / `macos-26-intel` are **standard** GitHub-hosted labels, free and unlimited
on public repos (4 vCPU / 14 GB) — the paid tier is `-large`/`-xlarge`, not `-intel`. x86_64
Actions support ends **August 2027**. The fallback is proven: working x86_64 cli+fpm
cross-compiled from arm64. Residual: a cross-built artifact cannot run a native smoke test on
the build host, so the Intel binary is permanently less-tested than arm64 — the same standing
caveat `docs/PORTS.md` already carries.

### 4.6 Xdebug on 7.4 — available, but the pin model has to change

`ghcr.io/v2/shivammathur/extensions/xdebug/7.4` exists; newest tag **3.1.6** (the last release
for 7.4). rexenv pins **one** global `XDEBUG_VERSION = "3.5.3"` (`binaries.rs:115`), derives
the bundle id as `(format!("xdebug-{minor}"), XDEBUG_VERSION)` (`binaries.rs:414`), and gates
`bundle_manifest` on `v == XDEBUG_VERSION` (`binaries.rs:871`). So `xdebug-7.4 @ 3.1.6`
returns `None` today. `xdebug_bottle` must carry a per-minor **version** column;
`XDEBUG_VERSION` becomes the default, not the law.

Whether the `.so` then loads depends on our build exporting Zend symbols. The mechanism is
now measured exactly, on this machine's own cache:

| cached build | exported symbols (`nm -gU`) | `_OnUpdateBool` |
|---|---|---|
| `php-fpm-8.0.30` | 98 | **0** ← why 8.0 has no Xdebug |
| `php-fpm-8.1.34` | 39 236 | 1 |
| `php-fpm-8.3.31` | 39 300 | 1 |

Since we own the 7.4 build, this is a **build-time gate we control** (`-Wl,-export_dynamic`,
no symbol strip), asserted in CI with `nm -gU | grep _OnUpdateBool` before an asset is
uploaded — not something to discover on a user's machine.

---

## 5. Stage 0 — what lands BEFORE any binary exists

Every item here is independently correct, independently verifiable, and blocks nothing on
the build repo. Three of them are latent bugs the investigation surfaced; they are worth
fixing whether or not 7.4 ever ships.

### 5.1 The fixture inversion — do this first, and derive it

**7.4 is currently this codebase's canonical "version we do not ship".** It is the negative
fixture in eight Rust asserts, one live example, one smoke-test step and one publish-testing
step. Every one either hard-fails or — worse — keeps passing while no longer testing
anything.

Sites: `core/php.rs:775,784,847`; `core/sites.rs:3855,3910-3913`;
`mcp_server/scratch.rs:1290,1298-1299`; `examples/mcp_scratch_check.rs:391,394,425`;
`core/binaries.rs:2448`.

The dangerous one is `binaries.rs:2448`:

```rust
assert!(!is_outdated_php_cache("php-7.4.33")); // unpinned minor
```

After 7.4 ships this **still passes**, for the opposite reason — 7.4.33 becomes the pinned
patch, so "not outdated" is now trivially true and the unpinned-minor branch is untested. A
green assert that proves nothing is exactly the vacuous shape this repo has been bitten by.

**Replace the literal with a DERIVED fixture** — a minor asserted absent from `PHP_VERSIONS`
— not with another hardcoded literal. Picking `7.3` or `8.6` re-arms the identical trap for
whoever ships that minor next, and this is the second time the same trap has fired.

Also `docs/SMOKE-TEST.md:395` (*"switch that scratch site to PHP 7.4"* → must be refused) and
`docs/PUBLISH-TESTING.md:383` (the amber import row rests on the reviewer happening to have a
7.4 Valet project) need genuinely-unshippable fixtures, or they silently stop testing the
branch they name.

### 5.2 `needs_tree_relink` lets an escaping `@loader_path` through — a real hole

`platform/macos/mod.rs:1660-1665` returns `false` for **anything** starting with
`@loader_path/`, treating it as already-in-tree. The shivammathur bottle's install names are
`@loader_path/../../../../opt/<formula>/lib/…` — which **escape the bundle**. Both the
rewrite loop (`mod.rs:1796`) and the post-relink VERIFY loop (`mod.rs:1814`) skip them, so
`prepare_binary_tree` reports success over 49 Mach-Os and the published binary then dies:

```
dyld[4357]: Library not loaded: @loader_path/../../../../opt/tidy-html5/lib/libtidy.58.dylib
```

Reproduced exactly; patching the predicate to **resolve** the relative path and require
containment under the bundle root made the same tree run.

This is a guard whose claimed surface is "nothing unresolvable remains" and whose check
covers only part of it — the same family as the four false guards already in the ledger. The
unit test at `mod.rs:2378` currently **enshrines** the blanket allowance.

rexenv's existing bundles never hit it (homebrew-core bottles use `@@HOMEBREW_PREFIX@@`,
correctly flagged), so this is latent, not live. Fix it anyway: it is one predicate, it has a
plantable failing test, and leaving it means the bottle fallback (§2A) is unusable.

### 5.3 A broken bundle caches forever, with no self-heal

`resolve_bundle` early-returns on `dir.join(spec.member).exists()` (`binaries.rs:893-905`);
`ensure_member_extracted` checks only that one marker file exists, never that the tree
*loads*; `is_cached` uses the same marker. Combined with §5.2, a user's first php-fpm spawn
aborts with an opaque dyld error and every later resolve short-circuits to the same dead
tree — no re-download, no repair.

The general lesson applies to 7.4 regardless of source: **the live check must exec the
published binary, not assert a file exists.**

### 5.4 Per-minor Xdebug version

Per §4.6. Tests at `binaries.rs:2312-2313,2347` assert the global pin and become per-minor.
`binaries.rs:2344`'s `for minor in ["8.0", "7.4", "9.0", ""]` — keep `7.4` in the
unsupported list only if 7.4 ships without Xdebug; otherwise remove it.

### 5.5 The UI's hand-copied core rules, and the EOL tell

`src/routes/SiteDetail.tsx:909-915` mirrors the Xdebug support rule as a literal
`minor === "8.0"`. `src/lib/ipc/index.ts:406` hand-copies "FrankenPHP sites and PHP 8.0 are
refused". **A hand-maintained UI copy of a core rule is precisely the "guard covers claimed
surface" defect this repo has paid for four times.**

Add `xdebugSupported: boolean` and `xdebugVersion: string | null` to the `PhpVersion` DTO
(`state/models.rs:409-419`), derived in `core/php.rs:139-142` from `binaries::xdebug_supported`
— **derived, not a new DB column** — and to `src/types/index.ts:527-533`. Drive the UI from
data.

Then the EOL tell (§4.3), covering 8.0 and 7.4, on the Settings row at
`src/routes/Settings.tsx:343-344`, and in the version picker before the button.

Also: `src/lib/mock.ts:90-97` gets the 7.4 row **first** (`{ minor: "7.4", patch: "7.4.33",
fpmPort: 9774, installed: false, isDefault: false }`) so the vite-dev UI matches the shipped
set. *Fixtures must look like production.*

**Nothing in `src/` sorts PHP versions** — order comes from SQLite `ORDER BY minor`
(`store.rs:1101`), a TEXT collation where `"7.4" < "8.0"` happens to be correct. It would
break at a two-digit major. Worth a TODO row; not a 7.4 blocker.

### 5.6 Ports

`fpm_port("7.4") = 9700 + 7*10 + 4 = 9774`; `debug_fpm_port("7.4") = 9974`. Both free, both
derived, no collision. But **both fall outside every documented range** — `9780–9785`,
`9981–9985`, ARCHITECTURE's "978x" — and the range stops being contiguous. Four wrong doc
lines: `docs/PORTS.md:15,16,40,41` and `docs/ARCHITECTURE.md:60`.

---

## 6. Stage 1 — `rexenv/runtimes`, the build repo

**One public repo, `rexenv/runtimes`.** Not `rexenv/php-builds`: a neutral name moots the
PHP License §4/§6 naming question entirely, at zero cost, and the repo will hold the Xdebug
debug build and any future self-built runtime too.

Public is **required** for free Sigstore attestations (Free/Pro/Team give attestations on
public repos only). That is a separate decision from `rexenv/rexenv` staying private, and it
means the build recipe is public — fine here, but ratify it rather than assume it.

### 6.1 Immutability by construction

The two sources rexenv already pins — static-php.dev and FrankenPHP — **both rebuild release
assets in place**, which is why `core/binaries.rs` carries two long comments explaining that a
checksum mismatch usually means a rebuild rather than tampering. Our own repo is the chance to
make a pin permanent:

- **Immutable releases turned on**, and *asserted in CI* (query the repo/org setting and fail
  the build). It is a setting; someone can turn it off, and nothing in the artifact records
  whether it was on. Asserting it is what turns a checkbox back into a guarantee.
- **A never-reused tag per build**: `php-7.4.33-1`; a rebuild is `-2`, never a re-upload.
- **Tag and build number in every filename.**
- **`--clobber` banned** by a self-lint in the workflow.
- **The full tag in the pinned URL** in `binaries.rs` — not a stable `BASE_URL` + a separate
  version const. That shape is how a pin goes soft.

Residual: deleting an immutable release is still possible, and the tag cannot be reused
afterwards — so the failure mode is a loud 404, not a silent byte-swap. Acceptable.

### 6.2 Download mechanics need zero app changes — verified

`http_client()` never calls `.redirect()`, so reqwest's default `Policy::limited(10)` follows
GitHub's 302 to `release-assets.githubusercontent.com`. `Range` survives the cross-host
redirect (only `AUTHORIZATION`/`COOKIE`/`PROXY_AUTHORIZATION`/`WWW_AUTHENTICATE` are
stripped), so resume works. No header is needed and `Authorization` must **not** be sent.
Release-asset downloads carry **no rate limit** — zero `x-ratelimit-*` headers; the 60/hr
unauthenticated cap is on `api.github.com`, which the download path never touches.

That last point is load-bearing: `status_is_transient` (`binaries.rs:1355`) is 5xx-only, so a
403 from an exhausted API quota would be classed Permanent and fail outright with no retry.
**Keep the download path API-free.**

### 6.3 Provenance

`actions/attest-build-provenance` is free on public repos and produces exactly what
`core/binaries.rs:309`'s FrankenPHP re-pin procedure already describes — verified live:
`GET https://api.github.com/repos/{owner}/{repo}/attestations/sha256:<digest>` returns the
SLSA bundle **unauthenticated**. rexenv's own would name a `refs/tags/…` builder, strictly
stronger than FrankenPHP's `refs/heads/main`.

Publish a detached `.sha256` and a `SHA256SUMS` (house convention per `RELEASING.md`), and say
in writing that they are documentation, not a trust root — the pin in `binaries.rs` is.

**Attestation is a maintainer-side check, not a runtime guarantee.** rexenv's app never
verifies Sigstore at download time; it trusts the pinned SHA-256. Provenance protects the pin
*ceremony*. It can never move into the app, because that would put `api.github.com` (60/hr)
on the download path.

**Bit-for-bit reproducibility is NOT achievable** — PHP's build embeds timestamps/paths and
Mach-O ad-hoc signatures include a CDHash over the whole file. Two runs differ. Do not promise
reproducible builds; the attestation is what proves origin.

### 6.4 The workflow, in outline

```yaml
strategy:
  matrix:
    include:
      - { runner: macos-15,       arch: aarch64 }
      - { runner: macos-15-intel, arch: x86_64  }
steps:
  - pin Xcode explicitly (sudo xcode-select -s …)      # Xcode 27 hard-errors below macOS 12
  - export MACOSX_DEPLOYMENT_TARGET=12.0               # NOT 11.0 — see §10b. spc's own macOS default, and what every PHP rexenv already ships is. Before EVERY dep build, not just php
  - shivammathur/setup-php@v2 with php-version 8.4     # spc needs PHP >= 8.4 on the BUILD host
  - fetch the backports tarball, VERIFY its sha256 against the pin, mirror it as an asset
  - spc download --with-php=7.4 --custom-url "php-src:file://…" --for-extensions "$EXTS" --prefer-pre-built
  - spc build "$EXTS" --build-cli --build-fpm
  - gate: nm -gU buildroot/bin/php-fpm | grep _OnUpdateBool          # §4.6 — Xdebug viability
  - gate: otool -l buildroot/bin/php | grep -A3 LC_BUILD_VERSION     # minos == 11.0, on deps too
  - gate: otool -L → only /usr/lib + /System (+ libz)                # §2B, rexenv's real gate
  - gate: ./buildroot/bin/php -m | grep -x mysqli                    # the WP-critical set
  - tar -C buildroot/bin -czf php-7.4.33-{cli,fpm}-macos-<arch>.tar.gz {php,php-fpm}
  - shasum -a 256 → SHA256SUMS
  - actions/attest-build-provenance
  - gh release create php-7.4.33-1 …                                 # never --clobber
```

Filenames deliberately mirror the static-php.dev shape so `binaries.rs`'s manifest arm is a
URL swap and nothing else.

Budget: ~74 s for PHP itself; the dep chain is the unmeasured cost and `--prefer-pre-built` is
what keeps it from being ICU-dominated. First CI run must **record wall time per dep** so the
number stops being a guess.

### 6.5 Licence obligations — the bill, stated plainly

rexenv becomes a **distributor of PHP** the moment these assets go up. `docs/xdebug-debug-build.md:75`
already predicted this; `THIRD-PARTY-NOTICES.md:6-12` ("rexenv redistributes none of them")
becomes **flatly false** and must change in the same commit.

- **PHP License 3.01** §2/§6 — ship the licence text alongside the artifacts.
- **Statically linked deps travel inside the binary**, so their licences travel too: OpenSSL
  (Apache-2.0), libpng, IJG, FreeType (FTL/GPLv2 dual), libwebp (BSD), oniguruma (BSD-2),
  libzip (BSD-3), libsodium (ISC), ICU (Unicode-3.0). This is not optional.
- **The readline GPLv3 trap does not fire** — proven on the real bottle: `bin/php` and
  `sbin/php-fpm` link `/usr/lib/libedit.3.dylib`, no `libreadline`; readline enters the
  dependency tab only transitively via `sqlite`, and only the `sqlite3` *CLI* links it, never
  `libsqlite3.0.dylib`. Assert the same on our own artifact in CI.
- **Any LGPL dep creates a live source-offer obligation**, not a one-time file. Mirroring the
  source tarballs in the same release is the durable answer and costs a few hundred MB of free
  storage. **Choose an extension set with no LGPL/GPL dep and this section stays short** — that
  is the concrete reason §2's "we choose the set" matters.

> This is a reading of licence texts, not legal advice. It is the one item on this plan that
> **cannot be fixed by a later commit** — it must be settled before the first asset is
> uploaded. `group@php.net` grants written permission on naming questions and has historically
> been responsive, if certainty is wanted.

### 6.5-outcome — SETTLED 16 Aug 2026, and the paragraph above did not prevent it

The obligation is discharged twice over: the licence texts are published beside
the artifacts (`licenses-<arch>.tar.gz`, PHP-3.01 + every statically linked dep,
a dep with no findable licence FAILING the build) **and pinned + downloaded onto
the user's machine** into `bin/php-7.4.33/licenses/`, inside the same atomic
publish as the interpreter. Reproducing them in `THIRD-PARTY-NOTICES.md` alone
was defensible under §2 and was still only an argument; a licence obligation is
the last place to hold a position that needs defending. Ledger #336.

**But the honest record of what happened here is the reason this section is kept
rather than ticked.** The paragraph above names `THIRD-PARTY-NOTICES.md:6-12` by
line number, quotes the sentence, and calls it the one item a later commit cannot
fix. Then 7.4 shipped, the same-commit docs sweep ran, and **the sentence did not
move** — it sat false in a public repo for a day and was found by reading, not by
any check. The most specific warning this plan contains failed on the release it
was written for.

So: **flagging is not a mechanism.** `core::copy_scan` records the identical
finding one layer down — a correct implementation with the lesson written beside
it did not stop the same mistake three times, and the third author was citing the
second. What closed this was a test that runs
(`the_notices_cannot_disclaim_distribution_while_we_distribute`), keyed on
`is_self_distributed` so it covers the next self-built runtime rather than this
one. When a future plan identifies something a later commit cannot fix, the
deliverable is the guard, not the paragraph.

_(The banned sentence is quoted above deliberately and safely: the guard scans
`THIRD-PARTY-NOTICES.md` and `README.md`, not this file. Do not "helpfully"
restore it to either of those — that is what the ban is for.)_

---

## 7. Stage 2 — wiring it into rexenv

### 7.1 The one edit that must NOT come first

`manifest()` (`binaries.rs:562-578`) hardcodes **one** URL template and gates only on
`php_sha256(…).is_some()`. **Pin four 7.4 checksums before branching the URL and the manifest
starts returning `Some` with a link that 404s forever** — and `manifest_pins_every_pinned_php_version`
(`binaries.rs:2387`) stays green, because it asserts URL *shape* only. The failure surfaces on
a user's machine as "php-fpm 7.4 download failed".

The fix already exists in-tree as a pattern: `php_debug_spec` (`binaries.rs:531-541`) +
`PHP_DEBUG_BASE_URL`. **Add the 7.4 source branch first**, then a unit test asserting
`manifest("php","7.4.33","macos",arch).url` does **not** contain `dl.static-php.dev`, then the
checksums.

### 7.2 Everything else is derived and changes for free

`PHP_VERSIONS` fans out to ~30 places; only four are hand-maintained duplicates. `all_minors`,
`available_minors`, `patch_for_minor`, `fpm_port`, `seed_registry`, the ports registry, the
downloads planner, per-minor pool naming and `php_versions_check`'s iteration all derive from
the one list. Adding `"7.4.33"` is a one-line edit **after** §7.1.

Two live hazards to close while there:

- **Existing rows.** Any site row with `php_version = "7.4"` silently moves from the 8.3
  default pool (9783) to a 7.4 pool (9774) that `seed_registry` marks *not installed* — those
  sites go from quietly working to 502. Reachable today because `mcp_server/scratch.rs:564`
  accepts an arbitrary php string with no validation against the pinned set. Fix the
  validation; migrate or refuse the rows.
- **FrankenPHP × 7.4 is impossible** (FrankenPHP embeds 8.x only), and today a FrankenPHP site
  *silently ignores* the site's PHP version — tolerable at 8.1→8.5, catastrophic at 7.4→8.5.
  Gate it in **core**, at create and at switch, the way `ensure_server_available` already
  refuses OpenLiteSpeed.

### 7.3 Docs that change in the same commit

`docs/PORTS.md:15,16,40,41` · `docs/ARCHITECTURE.md:60` · `docs/PLAN-valet-herd-import.md:71,95,98` ·
`docs/PLAN-valet-herd-migration.md:246-247,251` · `docs/SMOKE-TEST.md:395-397` ·
`docs/PUBLISH-TESTING.md:380-383` · `docs/CLAIM-LEDGER.md` row 224 + new rows + `ledger-tally.sh` ·
`docs/TODO.md:117-122` ("6 minors × cli/fpm × 2 arches" → 7) and `:650-654` (B33) ·
`docs/TESTING.md` · `docs/INSTALL.md` · `README.md:6,22,159,202` · `THIRD-PARTY-NOTICES.md:6-12` ·
`docs/xdebug-debug-build.md` (already stale — it describes the self-build as the live §8.2
path, but `docs/PORTS.md:41` records Xdebug shipping via ghcr for 8.1–8.5; **fix it before**
anyone picks up B33 and builds the wrong thing).

---

## 8. Stage 3 — proving it, which `verify.sh` cannot do

`php_versions_check` is **network** tier and `php_pools_serve` is **service** tier, so neither
runs in `scripts/verify.sh`. **A green `verify: all green` after this change proves the code
compiles and the tests were repaired. It proves nothing about the 7.4 artifact existing,
downloading, signing, or serving.** That gap closes by hand, or it does not close.

- `php_versions_check` picks 7.4 up for free **only because** we chose the single-tarball
  `manifest()` path (§2). Under (A) it would have broken (`resolve` and `resolve_bundle` are
  disjoint).
- A new live check must **exec the published `php`** and assert version + `mysqli` + Mach-O
  arch — not assert a file exists (§5.3).
- `xdebug_pool_check` exercises only the default minor; parameterise it over supported minors
  or add a 7.4 entry, plus the `docs/TESTING.md` line.
- Smoke test: create a 7.4 WordPress site, confirm it serves, confirm the WP outdated-PHP nag
  appears **and that rexenv said so first** (§4.2).

---

## 9. What this unblocks

- **B33** (`docs/TODO.md:650`) — the php-debug download host, open since the Xdebug work.
  `rexenv/runtimes` answers it: **GitHub Releases, and delete `dl.rexenv.dev`.** Note the
  prize is smaller than the TODO row implies — `docs/PORTS.md:41` records Xdebug already
  shipping for 8.1–8.5 via ghcr, so B33 unblocks Xdebug on **8.0 only**.
- **OpenLiteSpeed** (`docs/TODO.md`, "Blocked on external work") — blocked on "the same
  hosting infra as the Xdebug build". Same answer.
- The **Valet/Herd import** cohort whose projects are isolated to 7.4, currently surfaced as
  "needs attention" with no path forward.

---

## 10. Open decisions for the owner

1. **Create `rexenv/runtimes` as a PUBLIC repo?** Required for free attestations; makes the
   build recipe public. (§6)
2. **Ship 7.4 without opcache?** The alternative is abandoning spc. Recommendation: yes, ship
   without, and say so on the Settings row. (§4.1)
3. **Does the EOL tell ship with this feature?** It must cover 8.0 as well as 7.4. If it is
   not wanted now, the honest move is a TODO row saying so explicitly — not a partial badge
   that implies 8.0 is fine. (§4.3)
4. **Licence read before the first upload.** (§6.5)
5. **Is 7.4 offered like any other minor, or as an opt-in "legacy" row?** This plan assumes
   first-class. If it ships behind a flag, roughly half the doc list in §7.3 changes shape
   from "wrong" to "needs a caveat".

---

## 10b. What the first builds actually cost — measured, 14 Aug 2026

§11 said "the dep chain is the unmeasured cost" and "first CI run is the
measurement". Here is the measurement, including the parts the plan got wrong.
None of the four failures was PHP 7.4 refusing to compile, which was the risk
everyone expected.

| Run | Died at | Real cause | Fix |
|---|---|---|---|
| 1 | `./configure` | **Unknowable** — spc runs configure itself and does not echo its output, so the failure was a bare `Command exited with non-zero code: 1` and `config.log` went to the bit bucket with the workspace | `--debug`, then a `config.log` artifact |
| 2 | `spc download` | **`api.github.com` 403.** `--prefer-pre-built` asks which pre-built dep archives exist; unauthenticated that is 60 req/hr **per IP**, shared across GitHub's whole macOS runner fleet. spc even says `no github token found, skip` and carries on into the failure | `GITHUB_TOKEN: ${{ github.token }}` |
| 2 (Intel) | building PostgreSQL | `explicit_bzero.c:22: call to undeclared function 'memset_s'` — clang 16+ makes that an error. **A knock-on of the 403**, not a standing problem: with the token, libpq comes pre-built and this never runs | (none needed) |
| 3 | PHP `configure` | **`GD build test failed`.** spc builds an extension's *suggested* libs only when asked, so `gd.php` emitted a bare `--enable-gd` — no freetype, no jpeg, no webp — and 7.4's bundled GD fails its own link test | `--with-suggested-libs` + explicit `--for-libs` |

Three things worth keeping:

- **`--prefer-pre-built` has no `postgresql` asset** — the hosted release carries
  `icu` and not libpq (checked directly). So `pgsql` always builds PostgreSQL from
  source, and that build is one clang release away from breaking again. If it does,
  the choice is a `-Wno-implicit-function-declaration` in `SPC_DEFAULT_C_FLAGS`
  (spc's `GlobalEnvManager` only fills vars that are UNSET, so an exported one
  wins) or dropping `pgsql` and taking the divergence from the 8.x rows.
- **The deployment target in §6.4 was wrong.** It said `MACOSX_DEPLOYMENT_TARGET=11.0`,
  from `INSTALL.md`'s macOS 11 claim. Measured instead: every PHP binary rexenv
  already ships is `minos 12.0` — spc's own macOS default. 11.0 would have made 7.4
  the only row with a lower floor, bought nothing, and failed our own gate on an
  otherwise-good build. It is 12.0. **And the measuring turned up a real defect that
  is not this feature's**: `INSTALL.md` promises macOS 11 while the pinned nginx is
  `minos 15.0`, so on macOS 11–12 the app installs and cannot run its own web
  server. Filed in `docs/TODO.md`.
- **Debuggability was the expensive gap, not the toolchain.** Two rounds were spent
  learning *what* failed rather than fixing it, because a 120-line tail of
  `config.log` showed configure's later probes instead of the failure both times.
  The log is now uploaded whole, and the inline excerpt anchors to the last
  `configure: error` with the context ABOVE it. At ~40 minutes a round trip, a build
  that cannot explain itself is the costliest thing in this plan.

## 10c. It builds. What it actually took — 14 runs, 14 Aug 2026

**PHP 7.4.33 builds green on `macos-15` (arm64) and `macos-15-intel`**, cli + fpm,
39 extensions including **gd, intl and mysqli**, dylib closure `/usr/lib` +
`/System` only, `minos 12.0`, ~22 MB per artifact, ~7 minutes per arch.

**None of the real blockers was PHP refusing to compile**, which is the risk the
whole plan was written around. Three were fixes upstream had already made and 7.4
never received, because it went EOL first:

| What broke | 7.4 | 8.3 |
|---|---|---|
| `PHP_TEST_BUILD` (killed gd) | `AC_RUN_IFELSE` — **executes** a probe linked against libpng/webp/jpeg/freetype; one traps in a static initializer → `Illegal instruction`, `$? = 132` | `AC_LINK_IFELSE` — links, never runs |
| `ext/bcmath` | K&R definitions (`void bc_add (n1, n2, …)`) — **C23 removed the syntax** and the runner's clang defaults to `-std=gnu23` | n/a — `-std=gnu17` fixes it |
| `ext/intl` | hardcoded `PHP_CXX_COMPILE_STDCXX(11, …)`; ICU 74+ headers need C++14/17 | probes `icu-uc --atleast-version=74`, asks for 17 |

**The pattern is the finding**: when 7.4 fails against a modern toolchain, look at
what php-src did in 8.x before inventing anything. These are not workarounds.

The check that would have short-circuited three rounds cost seconds and was
available the whole time: **rexenv already ships a static PHP 8.3 with gd, built
by the same tool from the same libraries.** A working control sat in the binary
cache, and "what differs between the version that works and the one that doesn't"
beats another 40-minute experiment. Runs 5 and 6 — narrow extension set, and every
dependency built from source — were both aimed at libraries that were innocent.

Costs that were about DEBUGGABILITY rather than the build:

- spc does not echo configure's output, so the first failure was
  `Command exited with non-zero code: 1` and `config.log` died with the workspace.
- A 120-line tail of `config.log` showed configure's *later* probes twice. The
  failure line was in the middle both times. Uploading the whole file as an
  artifact ended the guessing in one round.
- `--prefer-pre-built` asks `api.github.com` unauthenticated — 60/hr **per IP**,
  shared across the runner fleet, so it 403s and the build dies before compiling.

Two gates caught things worth catching, one of them my own error:

- The arch gate compared `file(1)`'s output to `aarch64`; macOS says `arm64`. It
  failed a good binary — the cheap direction for a strict gate, and allowed once.
- The licence collector walked past **libxml2**, whose file is named `Copyright`.
  A statically linked dependency with no findable licence is now a build FAILURE,
  not a warning: we are the distributor, and a warning in a green build is one
  nobody reads.

### Xdebug on 7.4: no — measured, not assumed

The built binaries export ~22,400 symbols but **not `_OnUpdateBool`**, so
`xdebug.so` cannot dlopen into them — the same wall PHP 8.0 hits (its build
exports 98). So **7.4 ships without the Xdebug toggle**, and rexenv needs no code
for that: `xdebug_supported()` answers `false` for any minor with no bottle row,
so the toggle is unofferable by construction and the UI already says why (#320,
#321). Whether `--no-strip` or an export flag would change the answer is S1.2
work, not a blocker.

## 11. Risks that survive the plan

- **The dep chain is the unmeasured cost.** PHP builds in 74 s; ICU/OpenSSL/curl/gd from
  source do not. `--prefer-pre-built` is the mitigation and it is unproven for 7.4 specifically.
  First CI run is the measurement.
- **The backports branch is one volunteer's rebased branch.** If it stops, the artifact
  quietly becomes a frozen, known-vulnerable PHP. Record the exact source commit in the pin
  comment (as `binaries.rs` already does for FrankenPHP) and mirror the tarball (§3).
- **Deployment-target discipline is all-or-nothing.** One dep built without
  `MACOSX_DEPLOYMENT_TARGET` (12.0 — §10b) produces a silent `minos` bump. Assert it per-dep in CI, not
  just on `php`.
- **Xcode 27 hard-errors below macOS 12.** Pin `xcode-select` and treat the pin as part of the
  reproducibility contract.
- **The Intel binary will always be less-tested.** Build it natively while `macos-15-intel`
  exists; land the cross-compile path before August 2027.
- **`nm -gU` on the built 7.4 is not yet run** — §4.6's Xdebug conclusion is a build-time gate,
  not a proven fact. If it fails, 7.4 has no Xdebug (like 8.0) and `xdebug_supported` returns
  false for free; §5.4 then costs nothing.
- **7.4 runs the developer's own code on loopback**, so the untrusted-input surface is small
  and the choice is deliberate. The realistic risk is a compromised composer package or WP
  plugin, where an unpatched interpreter widens the blast radius. Real, bounded — and it must
  be *said*, which is what §4.3 is for.
