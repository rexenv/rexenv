# PLAN — in-app Adminer version updates

**Status: SHIPPED 18 Aug 2026** (designed and built the same day; header corrected
21 Aug, when the reconcile found it still saying "being built"). Originally:
designed 18 Aug 2026. Produced by a four-reader map →
three independent designs (minimal-diff / generalise / security-first) → three
judges (correctness / security / fit) → an adversarial synthesis, all against the
real files. `docs/PLAN-binary-updates.md` is the parent design; this is the
second family riding the same signed manifest.

## Evidence gathered before the design, and what it changed

Two facts were measured on this machine and are load-bearing:

- **Upstream Adminer is at 6.0.1** (released 14 Aug 2026); rexenv pins 5.4.2, six
  releases behind, across a MAJOR boundary. So this feature has content the day it
  ships, and a compat ceiling of "same major as the pin" would deliver almost none
  of it.
- **The wrapper's binding surface survives the major bump.** `WRAPPER_INDEX_PHP`
  subclasses `\Adminer\Adminer` and overrides `login`/`loginForm`/`headers`/`csp`,
  and calls `\Adminer\nonce()`. Static grep cannot check this — the released
  `adminer-<v>-en.php` is a compressed stub. Run through the bundled PHP 8.3.32,
  a probe declaring exactly that subclass shape binds cleanly against **5.4.2,
  5.5.1 and 6.0.1**, with no fatal and no missing symbol.

  That is why the ceiling below is set from what has been PROBED rather than from
  the pin's major — and why the runtime probe, not the version number, is the
  real gate. A ceiling that has never been tested is superstition with a constant
  name.

## In-app Adminer version updates — synthesised design

Base: **DESIGN 1** (minimal diff, `updates::Family` enum, marker-based staging, no new outcome type, no sweep, direct third-party host). Every fatal flaw the judges raised is fixed below, and the grafts they required are in. Where I diverge from a judge I say so and give the verified reason.

Adminer becomes the **second artifact family in the existing signed manifest**: no schema change, no new table, no new command module. The whole design rests on one new concept — `updates::Family` — replacing the `ALLOWED_NAMES` const and collapsing three PHP-shaped rules (name allowlist, version limit, completeness set) into one enum.

---

### What I re-verified in the files (the six that changed the design)

1. **`the_host_allowlist_is_not_a_bare_domain` (updates.rs:830-843) passes the vrana prefix UNCHANGED.** It asserts only `https://` + trailing `/` + `host != "github.com"`. `https://github.com/vrana/adminer/releases/download/` satisfies all three. **DESIGN 3's replacement guard is not just churn — it is broken**: it requires `Ownership::Ours ⟺ is_self_distributed(prefix)` plus "ThirdParty needs ≥4 path segments", but `SELF_DISTRIBUTED_HOSTS` (binaries.rs:714) is `["https://github.com/rexenv/", "https://dl.rexenv.dev/"]`, so `dl.static-php.dev/` is ThirdParty with **zero** path segments. It fails on an existing correct entry. Rejected.

2. **Mirroring is a verified dead end.** `is_self_distributed` (binaries.rs:738) is HOST-keyed → `licenses_spec` (binaries.rs:769) fires → its table is `"7.4.33" => (…), _ => ("","")` → empty hex → `Err(missing())` (binaries.rs:793) → `stage_licenses` fails the resolve (resolve_file, binaries.rs:1905). A mirrored Adminer version the app was built before can **never** resolve. Also derives a meaningless per-arch `licenses-{php_arch}.tar.gz`.

3. **Every `.php` in the adminer docroot is directly executable.** `sites.rs:2061-2069` builds the vhost as `RewriteMode::Single`; `services.rs:482` emits `location ~ \.php$` for every site. So `https://adminer.rexenv.rex/adminer.php` today executes **raw Adminer with no wrapper** — no `login()` override, no `csp()`, no `headers()`. This is a pre-existing hole, and it is exactly the surface the compat bound protects. **New ruling (see below): stage the real Adminer as a DOTFILE.**

4. **`NGINX_DOTFILE_DENY` (services.rs:440) precedes the php location**, asserted at `dotfile_paths_are_denied_before_php_execution` (services.rs:980-1000) for all three rewrite modes. So a dotfile costs nothing and adds no executable path.

5. **`newer_than`'s `minor` parameter is redundant.** php.rs:420 passes `&v.minor` and `&effective`; `effective` comes from `floored`, whose `newer()` already requires `minor_of(sel) == minor_of(pin)`. Deleting it is a net simplification.

6. **`InFlight` is ALREADY keyed** (`MinorSet = Mutex<HashSet<String>>`, php.rs:411). Designs 2 and 3 both propose "generalise it to take a key" — work already done. It needs `pub(crate)` and one message fix.

7. **324 KB measured**: `du -sk ~/Library/Application Support/dev.rexenv.rexenv/bin/adminer-5.4.2` → `324`. The no-sweep ruling holds.

8. **`manifest_from_catalog` does not exist anywhere in the tree** (`grep -rn` over `src-tauri/` returns only the CLAIM-LEDGER #355 row citing it). The catalog consultation is `php_spec` (binaries.rs:595).

9. **`updates.rs` contradicts itself in FOUR places**, not three: :35-40 module header ("`RELEASE_PUBKEY` is empty, so `verify` refuses everything"), :252 (`verify_with`'s "With `RELEASE_PUBKEY` empty — the shipping state —"), :499 (a stale doc comment stacked above the live one on `the_pinned_key_is_real_and_only_it_verifies`), :572 (`scripts/publish-php-manifest.sh`, deleted). The const at :56 is `faa52f96…`.

10. **`commands/settings.rs:28-29` asserts a guard that does not exist.** It claims `every_gated_setting_key_is_routed_here` "fails the build when a third one appears and this match does not learn about it." `core/sites.rs:3292-3294` iterates a hardcoded `[("DEFAULT_TLD_KEY",…), ("SITES_DIR_KEY",…)]` and asserts only that those two are present. A third key does not break it. **Verified false — a doc asserting a nonexistent guard, the exact failure class the ledger rules exist for.**

11. **`the_version_rows_never_promise_an_update_they_cannot_deliver` (copy_scan.rs:272)** only fires on lines containing `php`, `upstream` or `patch`. An Adminer card saying "up to date" would **not** be caught.

12. **The publisher can silently reset the serial to 1 TODAY.** publish-manifest.sh:135-146 is `if gh release view … && gh release download …; then CUR=…; fi`. A transient failure of *either* leaves `CUR=0` → `SERIAL=1` → `gh release delete` + publish. Every installed app then refuses it (`accept_with`'s `m.serial < highest`, updates.rs:349). **Total silent update outage with a correctly signed document.** Every design placed its superset assertion inside this same conditional, so the assertion would no-op on exactly the failure it exists for. This is a pre-existing bug and gets its own commit, first.

13. **Nothing outside `core/adminer.rs` depends on the docroot copy being named `adminer.php`.** `examples/adminer_check.rs:19` asserts the *cache* member name (`spec.member`, unchanged). Only adminer.rs:235 and its assertion at :468 name the docroot file.

---

### DECISIONS

#### 1. `updates::Family` replaces `ALLOWED_NAMES`

```rust
pub enum Family { Php, Adminer }
impl Family {
    pub fn of_name(name: &str) -> Option<Family>;   // the new `nameable`
    fn names(self) -> &'static [&'static str];      // ["php","php-fpm"] | ["adminer"]
    fn track(self, version: &str) -> Option<String>;
    fn arch_ok(self, arch: &str) -> bool;
}
```

`nameable(name)` keeps its exact signature (`Family::of_name(name).is_some()`), so `php_spec`'s point-of-use assertion (binaries.rs:608) is untouched.

The `ALLOWED_NAMES` doc comment (updates.rs:70-78) is rewritten from one exclusion into a **per-variant grant**:

- **Php** — `Shape::Single` → `resolve` → `set_executable` + `prepare_binary` (de-quarantine → dylib relink → ad-hoc codesign) → spawned by `ServiceManager` as a long-lived master.
- **Adminer** — `Shape::File` (binaries.rs:1452) → `resolve_file` (binaries.rs:1881), which explicitly does **neither** chmod nor codesign, never spawns, and whose bytes are interpreted by an already-running php-fpm pool as the user.

The caddy sentence stays — it is still why the list exists — but it is now the **worst case, not the rule**, and the comment also names what the list keeps out on the near side: `nginx`, `mysql`, `frankenphp`, `cloudflared`, `httpd`, the xdebug bottles, and the two closest neighbours `wp-cli` and `composer`, which are the other `Shape::File` artifacts and run as the user against every site's DB.

**Adding `adminer` does NOT raise the ceiling over `php` — it is strictly below it on every axis, and the comment must say so**, or the third addition is argued from convenience. What IS worse about Adminer, stated rather than hidden (D3's framing, kept): **control ownership**. rexenv's security controls for this vhost live *inside the artifact's own plugin API*. A PHP update cannot turn off a control rexenv wrote; an Adminer update can. That is why §4's probe is not optional.

#### 2. Shape guard (graft, scoped)

`shape_of` (binaries.rs:1452) ends in `_ => Shape::Single` — a manifest-nameable name the app does not recognise falls through to the **most privileged** shape, the one `resolve` handles. Nothing asserts today that a nameable artifact is File- or Single-shaped, and `shape_of` is keyed on the name the manifest supplies.

New L0 guard `every_family_name_has_the_shape_its_grant_declares`: **exact equality per variant** — `Php` ⇒ `shape_of("php") == shape_of("php-fpm") == Shape::Single`; `Adminer` ⇒ `shape_of("adminer") == Shape::File`.

*Not* D2's "must not be Dir/Bundle, PHP whitelisted by name" form: whitelisting the one high-privilege entry by name collapses back to the membership check it claims to replace, and D2's own `adminer-licenses` (TarGzTree → `Shape::Dir`) fails its own guard. Exact equality per declared grant has no by-name exception.

#### 3. Version comparison, and where the ceiling lives

`Family::track` answers both, in one function:
- `Php` → `Some(php::minor_of(v))` iff `php::patch_for_minor(minor).is_some()`
- `Adminer` → `Some(major_of(v))` iff `major == adminer::WRAPPER_API_MAJOR`

`acceptable` reads `f.track(&a.version).is_some()`; `newer_in(f, cand, have)` is `f.track(cand) == f.track(have) && segments(cand) > segments(have)`.

The bound is **derived from the version**, not a constant. This is D3's fatal flaw fixed: D3 specified `track_of` returning `ADMINER_COMPAT_MAJOR` (the constant), which makes every Adminer version share the track "5", so `newer_in_track("6.0.1","5.4.2")` → true — inverting the defence-in-depth property its own WHY paragraph claims.

`pub const WRAPPER_API_MAJOR: &str = "5"` lives in **core/adminer.rs**, beside the wrapper it describes, not in binaries.rs — the bound is a fact about the WRAPPER, not a policy about Adminer, and that placement is what stops it drifting when someone edits the wrapper. Its doc names exactly what it protects, all verified present in `WRAPPER_INDEX_PHP`: `class RexenvAdminer extends \Adminer\Adminer` (adminer.rs:173), `login()` (:176, the passwordless-loopback grant), `loginForm()`, `headers()` (:219, drops Adminer's blanket `X-Frame-Options: deny`), `csp()` (:225, re-adds `frame-ancestors` scoped to the app's webview origins), plus `\Adminer\nonce()` and `\Adminer\SERVER`.

#### 4. The binding probe (graft — the one real gap in DESIGN 1)

`examples/adminer_login_gate_check.rs:22-44` extracts the text between the `rexenv-loopback-gate` markers and runs **that function standalone**. It proves the gate LOGIC and **structurally cannot see an override that stopped binding**. Under a compiled-in pin that is tolerable — the pair is fixed at build time. Under a manifest it is not.

New `adminer::verify_pair(php_bin, staged_dir) -> Result<()>`, run at **apply time, in the staging dir, before anything reaches the live docroot**. It generates a probe script that requires the staged `index.php` inside an output buffer with `$_SERVER` minimally faked and `session.save_path` pointed at the staging dir, and asserts from a `register_shutdown_function` (so `exit`/`die` inside Adminer's bootstrap cannot skip it):

1. `class_exists('RexenvAdminer', false)` — proves `adminer_object()` was **called** by Adminer's bootstrap *and* that `\Adminer\Adminer` resolved (the class declaration is inside the hook, so a renamed/renamespaced base class is a hard fatal here).
2. For each of `login`, `loginForm`, `headers`, `csp`: `ReflectionMethod` on `RexenvAdminer` declares it **and** `\Adminer\Adminer` also declares it — i.e. it **overrides** rather than adds a dead method. This is the check that catches the dangerous direction: `headers()` still stripping `X-Frame-Options` while `csp()` has become a dead method is clickjacking on a passwordless DB console.

Failure refuses the apply, names the failing assertion, leaves the old pair serving, and **keeps the downloaded tree** (verified bytes; deleting them makes the retry re-fetch).

**Honest limit, written into the doc comment and the ledger row:** this proves the overrides BIND and shadow real parent methods. It does **not** prove the served HTTP response carries the scoped `frame-ancestors` — that needs a running pool. That half is a separate `service`-tier example and is named as unproven until it lands.

I take **D2's narrow reflection form, not D3's five-assertion form**. D3 requires `login()` to return true/false for a *set* `\Adminer\SERVER` — a constant Adminer defines during its own bootstrap and which cannot be redefined, so that assertion cannot be written as specified.

#### 5. Where the docroot copy goes — a DOTFILE, not a version-named file, not `adminer.php`

`ensure` stages the real Adminer as **`.adminer.php`**, and `WRAPPER_INDEX_PHP`'s last line becomes `require __DIR__ . '/.adminer.php';`.

This is a strict improvement over DESIGN 1's `adminer.php` + `.staged-version`, and it closes the pre-existing bypass **using a guard that already exists and is already ordering-asserted**, rather than adding a new nginx location:

- `NGINX_DOTFILE_DENY` (services.rs:440) denies `location ~ /\.(?!well-known(/|$))` with a 404, and `dotfile_paths_are_denied_before_php_execution` (services.rs:980) already asserts it precedes `location ~ \.php$` in all three rewrite modes. `/.adminer.php` therefore 404s.
- `WRAPPER_INDEX_PHP` stays a plain `const` with a **literal** `require` — no version templating, no assertion that has to move per version. The test at adminer.rs:468 changes its literal once and stays a literal.
- `ensure` deletes a legacy `docroot/adminer.php` when it stages `.adminer.php`, so existing installs lose the bypass rather than keeping it as a stranded live URL.

Staleness predicate: `.staged-version` dotfile marker.

```
stale = read_to_string(dir/".staged-version").ok().as_deref().map(str::trim) != Some(version)
        || !dir.join(".adminer.php").exists()
```

On stale: `fs::copy(&src, dir/".adminer.php.new")` → `fs::rename(new, ".adminer.php")` → **then** write the marker. Marker last: a death in between costs one restage, never a truncated served file.

`index.php` gets the **same** treatment — `std::fs::write` onto the live served entrypoint (adminer.rs:283-285) is the identical mid-write hazard on the file nginx actually serves. Temp-in-same-dir + rename for both. (The FIT judge caught that DESIGN 1 fixed one and left the other.)

**Why size dies:** adminer.rs:272-273 compares `metadata(dst).len() != metadata(src).len()`. Two Adminer releases of identical byte length — a minified single file; an identifier swapped for one of equal length — stage as "not stale", so the docroot keeps serving the OLD code while the setting, the button and the row all say otherwise. A security patch is the button's entire reason to exist. Same hole on the revert-to-pin path.

**Honest limit, stated in the doc comment that replaces adminer.rs:260-264:** the marker proves the docroot copy's **VERSION**, not its integrity. Truncation cannot *originate* in `ensure` (`fs::copy` returning `Ok` means complete, and the publish is a rename), and the source dir just passed `cached_path`'s `cache_matches_pin` gate (binaries.rs:1541/1559). Out-of-band corruption of the docroot copy is **out of scope and said so** — note the asymmetry honestly: the wrapper self-heals every start via its exact-content compare, the Adminer copy does not.

**Why not `adminer-5.4.3.php` (D2/D3):** with a sweep, at most one bypass URL exists at a time — the same count as today's `/adminer.php`, so DESIGN 1's "permanently-accumulating" argument overshoots and is restated. The real advantages are (i) the dotfile adds **zero** executable paths and removes one, (ii) `WRAPPER_INDEX_PHP` stays a constant with a literal `require`, (iii) the wrapper's exact-content compare stays a pure function of the build profile.

#### 6. Arch — a third required value `"any"`, gated in BOTH directions

An `adminer` row MUST be `"any"`; a `php`/`php-fpm` row must NOT be. `Family::arch_ok` owns it. Lookup gains `fn arch_matches(entry, want) -> bool { entry == want || entry == "any" }`, used by `VersionCatalog::artifact` (updates.rs:178) and `newer_than`'s filter. `catalog_arch` (updates.rs:86) is untouched and stays the ONE `Arch`→string mapping.

Making `artifact` permissive is safe for PHP **only** because `acceptable` drops a `php` row carrying `"any"` at verify time — state that in the comment, or the permissive matcher looks like a hole.

Two identical rows is a fiction stated twice and makes a half-published adminer version structurally legal (ledger #355's exact failure). `Option<String>` makes the field forgettable and weakens the PHP side.

#### 7. Completeness becomes per-family; `newer_than` loses `minor`

`newer_than(&self, family: Family, have: &str, arch: &str)`. Track filter: `family.track(&a.version) == family.track(have)`. Completeness: `family.names().iter().all(|n| self.artifact(n, v, arch).is_some())`.

**Appending `"adminer"` to today's `ALLOWED_NAMES` is a total silent regression, not a partial feature** — `newer_than`'s `.all(...)` (updates.rs:166) would require an `adminer` row at 8.3.32, so every PHP button goes dark. The existing test builds its fixture from a literal `["php","php-fpm"]` (updates.rs:~858), so it stays green. **Every fixture is rebuilt from `Family::names()` in the same commit.**

#### 8. Host — direct, path-bearing prefix

Add `"https://github.com/vrana/adminer/releases/download/"` to `ALLOWED_HOSTS` (updates.rs:65-68). The comment records **what it costs** (one outside maintainer's GitHub account becomes part of the bound on a signing-key compromise; GitHub permits replacing an asset under an existing tag, which the compiled-in pin catches by failing closed but a freshly-generated manifest would re-hash and bless) and **why it is nonetheless right** (the URL is not the trust anchor — the document is signed and carries the sha256, and every install ALREADY downloads from that exact host via binaries.rs:1092-1099; refusing it in the manifest while allowing it in the binary is inconsistency, not defence).

`SELF_DISTRIBUTED_HOSTS`' comment (binaries.rs:709-714) currently cites Adminer **by name** as a github.com artifact that is not ours. That sentence stays true and becomes load-bearing a second way — it is *why* the mirror was rejected and *why* no licence obligation attaches. Restate it explicitly (D3's graft).

**I do not scope the host per family.** The security judge wants `Family::hosts` because a flat list makes the vrana prefix a legal `url` for a `php` row. That is real, and it is bounded by two facts I verified: a `php` row still has to pass `Family::Php`'s track rule (a patch of a minor rexenv ships) *and* `php_spec` consults the **compiled-in pin first**, so a `php` row can only ever *add* a version — and the resulting bytes are digest-checked against whatever the signed document says either way. Per-family hosts is a real improvement and I record it as the **first thing to add if a third family lands**; buying it now costs a second table for two rows and one delta. This is a knowing trade, in `residualRisks`, not an omission.

#### 9. Selection — one settings row, filtered ON READ

Key `adminer_version`, through `store::get_setting`/`set_setting`. No table, no column, no migration — migration v36 already deleted a column that mirrored a compile-time constant, and Adminer has no existing row to hang a column on.

Filter on read, mirroring `DbEngine::effective_version` (core/db.rs:149-155), and deliberately **not** routed through `commands/settings.rs`' generic setter. Reason: `commands/settings.rs:31-42` is a documented door around every validating setter, and `every_gated_setting_key_is_routed_through_its_validating_setter` (core/sites.rs:3283) **does not enforce coverage** — verified, it asserts a hardcoded pair. Filter-on-read makes the door inert with no write-path code to get wrong: a hand-written `adminer_version = 6.0.1` in a world-writable `rexenv.db` fails `binaries::manifest(...).is_some()` and falls back to the pin.

**The false claim at commands/settings.rs:28-29 is corrected in the same body of work** (it is a doc asserting a guard that does not exist).

```rust
// core/updates.rs, beside `floored`
pub fn adminer_floored(selected: Option<&str>) -> String {
    match selected {
        Some(s) if newer_in(Family::Adminer, s, binaries::ADMINER_VERSION) => s.to_string(),
        _ => binaries::ADMINER_VERSION.to_string(),
    }
}

// core/adminer.rs — THE ONE accessor
pub const VERSION_KEY: &str = "adminer_version";
pub fn effective_version(platform: &dyn Platform, conn: &Connection) -> String {
    let sel = store::get_setting(conn, VERSION_KEY).ok().flatten();
    let v = updates::adminer_floored(sel.as_deref());
    // `manifest` returning a spec IS the vouch: pin, or VERIFIED catalog.
    match binaries::manifest("adminer", &v, std::env::consts::OS, platform.binaries().arch()) {
        Some(_) => v,
        None => binaries::ADMINER_VERSION.to_string(),
    }
}
```

`floored` cannot be reused — it is `php::patch_for_minor(minor)?`-shaped by construction. Two independent refusals of a major fall out free: `acceptable` keeps 6.x out of the catalog, and `adminer_floored`'s `newer_in` refuses a 6.x selection even if one reached the row.

#### 10. `ensure` takes the version as a PARAMETER — and so does `start_core`

**Divergence from DESIGN 1, and it fixes a fatal flaw the FIT judge found.** DESIGN 1 proposed a `ServiceManager::set_adminer_version` mirror field. That field needs a fallback (a manager constructed before any command sets it holds `""` and `resolve_file(…, "")` fails `start_core`), and any fallback to the pin reproduces D3's receipt bug: the planner planned the SELECTION, `uncached_names` cleared the start as offline-safe against that plan, and the stager then resolves the PIN — downloading inside the services lock on the one path whose contract is that it never downloads (ledger #175). It also puts a **fifth** `ADMINER_VERSION` site into `core/service_manager.rs`.

So: `pub async fn ensure(platform: &dyn Platform, version: &str)`, and `start_core(platform, ca, sites, php_minors, adminer_version: &str)` / `start_all(…, adminer_version: &str)`. Compiler-enforced, no drift, no default, no fifth pin site. Examples pass `binaries::ADMINER_VERSION` (the scanner walks `src-tauri/src` only — verified at php.rs:2011).

Unlike `db_versions`, there is no watchdog respawn path for Adminer — `adminer::ensure` is called from exactly one place (service_manager.rs:488) — so a persistent mirror buys nothing.

**This is also the structural close on the consent hazard.** Because `ensure` takes the version it is given and never consults the catalog or SQLite, a signed manifest physically cannot move Adminer on anyone's machine at the next Start. Ledger #350's "No auto-update, ever" stops being a discipline and becomes a signature.

#### 11. The planner moves in the SAME commit

`plan_for_start_with` (downloads.rs:244) gains `adminer: &str` beside `patches: &PatchMap`; downloads.rs:263 uses it. `plan_for_start_pinned` (downloads.rs:998-1004) passes `binaries::ADMINER_VERSION` — that is its whole documented job. Three production callers move: `commands/downloads.rs:55`, `commands/services.rs:107`, `commands/services.rs:275`, each from the snapshot they already build (`start_inputs`, commands/services.rs:66, already `#[allow(clippy::type_complexity)]` — it gains a seventh element). The assertion at downloads.rs:868 follows.

A **parameter**, not a `Connection` read — `plan_for_start_with` must stay callable without a database (`PatchMap`'s stated reason, downloads.rs:186-192).

#### 12. `adminer_spec` — pin first, catalog second

binaries.rs:1092 changes from the literal `("adminer", _, "5.4.2")` to `("adminer", _, v) => adminer_spec(v, arch)`. `adminer_spec` is a direct sibling of `php_spec` (binaries.rs:595): pin first (`v == ADMINER_VERSION` → today's `format!` URL + `ADMINER_5_4_2_SHA256`), then `updates::nameable("adminer")` **asserted at the point of use** (copied verbatim from php_spec's reasoning — and it is what makes `shape_of`'s name-keying safe), then `CATALOG.read()` → `artifact("adminer", v, catalog_arch(arch))`, with `Archive::Raw` and `member: "adminer.php"` **hardcoded in the spec function**.

Deriving archive+member from the name keeps the publisher unable to describe a shape the app did not compile in — the same reason `php_spec` does it. Growing the schema with `archive`/`member` would let a signed document choose **which extractor runs**, a strictly larger grant than choosing bytes.

#### 13. IPC — three commands, in the existing `commands/database.rs` (178 lines)

```rust
#[tauri::command] pub fn adminer_status(state) -> Result<AdminerStatus>              // offline, instant
#[tauri::command] pub async fn adminer_update_check(state) -> Result<AdminerStatus>  // fetch + accept + re-read
#[tauri::command] pub async fn adminer_update_apply(state, version: String) -> Result<AdminerStatus>

pub struct AdminerStatus {
    /// What the docroot HOLDS — measured from `.staged-version`, never inferred.
    /// `None` before the first stage (a fresh install that never started services).
    pub staged: Option<String>,
    /// What a stage would produce now: the selection, floored by the pin.
    pub effective: String,
    pub updatable: Option<String>,
    pub checked_at: Option<String>,
}
```

`adminer_update_check` copies `php_update_check` exactly: `fetch()` UNLOCKED, then `accept` + `install_catalog` under one brief lock. A second refresh at the same serial writes nothing (ledger #360) — the property that makes a second family's polling free.

`adminer_update_apply` order: claim `InFlight("adminer")` → refuse anything `binaries::manifest` does not vouch for → `prefetch` with no lock held and nothing written → **`verify_pair` on a staging pair** → persist the selection → `adminer::ensure(platform, &version)` → return the **re-measured** `AdminerStatus`.

**No new outcome type.** `PhpUpdateOutcome.restarted` is a MEASUREMENT (commands/php.rs:352-356). Adminer's apply owns its entire effect: `ensure` needs nothing running (there is no OPcache anywhere in the tree — the only `zend_extension` is Xdebug at services.rs:195 — so a re-staged file takes effect on the next request), and a rename either happened or errored. A hardcoded `restarted: true` would put a constant where a measurement was — the defect #354 fixed. Returning the re-measured row satisfies DESIGN.md:38 maximally: the effect is not merely IN the return value, it IS the return value.

`InFlight` (php.rs:415-470) becomes `pub(crate)` and takes a `noun: &'static str` so the message is not PHP-shaped ("a PHP adminer update is already running"). It is already keyed by `String`; its `clear_poison` half and its `std::ptr::eq` test are untouched. One comment stating no PHP minor can ever be named `adminer`.

#### 14. UI — Databases screen, its own card below the engine card

Row label `Database browser · Adminer 5.4.2`; button `Update to 5.4.3`; tooltip `Download Adminer 5.4.3, check rexenv's login gate and frame rules still bind, and replace the copy rexenv serves.` (it names the probe, because that is the thing that can refuse). Footer `Checked rexenv's signed update list 4h ago.`; never-checked: `The update list hasn't been fetched yet, so nothing here says whether a newer Adminer exists.` **The words "up to date" and "latest" appear nowhere.** ~550 ms floor on the spinner only (DESIGN.md:155) paired with the real timestamp. The version is echoed as a FACT (no control) in the browse header (Databases.tsx:166, today `{browse.label} · Adminer`).

Not a row inside the engine table (Databases.tsx:190): it would be the only row with no `StatusPill`, no pid/port and no meters, and would quietly reverse the recorded divergence at DESIGN.md:260. Not Settings → Services: that card is titled "PHP versions" and `PhpVersionsSetting` is a picker; Adminer is not a `ServiceInfo` at all.

**ONE fact.** No `upstream` chip. PHP carries two facts because static-php.dev trails php.net; rexenv downloads Adminer's OWN release asset (binaries.rs:1092-1099), so transplanting Settings.tsx:634-637 would ship a straight falsehood inside a rule about honesty. The trade — an upstream Adminer security release is invisible until it is signed — gets a DESIGN.md line, not silence.

Per-state SCENARIOS, not one fixture: `mockAdminerStatus()` reads `?adminer=settled|offer|unchecked|stale-check|unstaged` off `location.search` (the `mockResolverDrift` pattern, mock.ts:206-216). **Three explicit arms in `DevUiReview.tsx`'s `mockIPC` switch** (:871-886) — ledger #356 records that a missing arm falls to the default and returns a scalar, and `list_php_versions`/`php_update_check` did exactly that and the whole view rendered nothing. `uireview.js`'s `seen`-set pattern (:255-268) cannot work on a single row, so per-state scenarios ARE the coverage mechanism.

`the_version_rows_never_promise_an_update_they_cannot_deliver` (copy_scan.rs:272) gains `"adminer"` in its line-relevance filter **and** a presence assertion on the Adminer card, so it cannot pass by the feature having been deleted.

#### 15. Publisher

`ADMINER_PIN="5.4.2"` + `ADMINER_MAX_MAJOR="5"` as **separate variables, never rows in `PINS`** — verified: `rexenv/scripts/check-php-pins.sh:46-47` parses that array with `grep -oE '"[0-9]+\.[0-9]+:[0-9]+\.[0-9]+\.[0-9]+"'`, so `"5.4:5.4.2"` would be reported as a phantom extra minor.

Discovery is a **LISTING, not a probe walk**: `gh api repos/vrana/adminer/releases --paginate --jq`, filtered `.draft==false`, `.prerelease==false`, `tag_name =~ ^v[0-9]+\.[0-9]+\.[0-9]+$`, ordered `sort -V` above `ADMINER_PIN`, bounded by `ADMINER_MAX_MAJOR`; asset by **exact** name `adminer-X.Y.Z-en.php` from `browser_download_url`; then still `curl` + `shasum`. The two-consecutive-misses walk (:158-180) exists only because dl.static-php.dev publishes no index; `gh` is already a hard dependency (:104). `.draft==false` is load-bearing and non-obvious: the workflow runs `gh` **with** `GH_TOKEN`, so a draft release is visible to it and would publish as if real — while the anonymous view a maintainer checks by hand hides it (the "the MACHINE was the fixture" failure).

Three document-integrity changes, all pre-existing bugs made worse by a second family:
- **The serial reset (my find, none of the three caught it).** Split publish-manifest.sh:135-146: `gh release view` failing = genuine first run (`FIRST_RUN=1`, `CUR=0` legitimate); `gh release view` succeeding but `gh release download` failing = **refuse to publish**, because republishing at serial 1 is a total silent update outage that every installed app enforces on itself.
- **Explicit versions ADD to discovery** rather than replacing it (:159-162). `./scripts/publish-manifest.sh 8.3.32` today publishes only that version's rows and deletes every other — a documented flow (MANIFEST.md §2) and a workflow input.
- **Superset assertion** between manifest.json being written (:240-241) and `openssl pkeyutl -sign` (:246), **mandatory whenever `FIRST_RUN` is 0**: every `name+version+arch` triple in the previously-downloaded document still present (or at/below a pin), else refuse. The post-publish step (publish-manifest.yml:92-105) prints an entry COUNT, and a scalar count is not a diff.

Both product sets computed independently and emitted unconditionally; early exit (:193-199) becomes "no PHP AND no Adminer"; the `KEPT` gate (:238) becomes "no complete artifact of any kind"; `PUBLISHED` (:144-145) name-partitioned.

`minAppVersion` stays document-wide. Per-artifact is a schema change for a case the compat-major bound already refuses.

#### 16. Shipping order

App before UI (DESIGN.md:99 — "a control whose only outcome is an error is not rendered"; `binaries::manifest("adminer", <anything but 5.4.2>, …)` returns `None` today). App before publisher (`acceptable` drops adminer rows on name AND version today, so an early publish delivers nothing, burns a serial irreversibly, and under replacement semantics can remove the PHP rows on the way — the same rule MANIFEST.md §4 states for key rotation). Publisher-side truncation bugs first, because they exist today and this change amplifies them.

---

### INVARIANTS RENEGOTIATED (each with its ledger row in the same commit)

- **updates.rs:70-78** — `ALLOWED_NAMES`' doc becomes a per-variant grant on `Family`. Ledger #348 re-verdicted.
- **updates.rs:65-68** — `ALLOWED_HOSTS` admits a third-party release surface for the first time; the comment records the cost.
- **updates.rs:161-175** — `newer_than`'s "BOTH binaries present" becomes per-family. Ledger #355 amended, **and its `core/binaries.rs (manifest_from_catalog)` citation corrected** — no such function exists.
- **updates.rs:224-238** — `acceptable`'s version limit becomes `Family::track(..).is_some()`; the PHP branch is unchanged and Adminer's replacement rule is stated explicitly.
- **updates.rs:106-118** — `Artifact.arch` gains `"any"`, still required, gated per family both ways. `Artifact.name`'s doc stops saying "php or php-fpm".
- **updates.rs:35-40, :252, :499, :572** — four stale self-descriptions, three of them saying the update path is inert when it is live.
- **adminer.rs:235 + :468** — the wrapper's `require __DIR__ . '/adminer.php'` becomes `'/.adminer.php'`; the assertion moves with it.
- **adminer.rs:260-264** — "refreshed only when missing / a different size" deleted; replaced by the marker predicate **with its integrity limit stated**.
- **binaries.rs:1092** — the adminer arm's version literal stops being the only Adminer version that can resolve; gains pin-first/catalog-second, asserted by binaries.rs:3641's precedence test growing an adminer leg.
- **binaries.rs:709-714** — `SELF_DISTRIBUTED_HOSTS`' comment restated now that a vrana prefix sits on the manifest allowlist.
- **commands/settings.rs:28-29** — corrected: it claims a coverage guard that `core/sites.rs:3292` does not implement.
- **copy_scan.rs:272** — the wording guard's relevance filter widens to Adminer.
- **service_manager.rs:462, :440** — `start_core`/`start_all` gain the Adminer version as a parameter.
- **downloads.rs:244** — `plan_for_start_with` gains it too; `PatchMap`'s parameter-not-Connection rule applies unchanged.
- **docs/DESIGN.md:260** — the card sits BESIDE the engine card, so the ENGINE-focused divergence holds; the exception is recorded rather than rediscovered as a contradiction.
- **docs/PORTS.md:52** — "Adminer | 5.4.2" becomes a FLOOR.
- **runtimes publish-manifest.sh:26-27** — "`name` may only be `php` or `php-fpm`. Nothing else." lands in the same change as the app-side widening.
- **runtimes MANIFEST.md §3** — "all four artifacts or none" stops being THE completeness rule and becomes PHP's; Adminer's is "one file or none".

---

### FILE MAP

| File | Change |
|---|---|
| `src-tauri/src/core/updates.rs` | `Family` enum replaces `ALLOWED_NAMES`; `arch_matches` + `ARCH_ANY`; `newer_in`; `newer_than(family, have, arch)`; `adminer_floored`; vrana prefix in `ALLOWED_HOSTS`; four doc fixes; new guards |
| `src-tauri/src/core/binaries.rs` | `adminer_spec` sibling of `php_spec`; :1092 arm; `SELF_DISTRIBUTED_HOSTS` comment; :3641 grows an adminer leg. `licenses_spec`, `is_self_distributed`, `is_outdated_php_cache`, `php_caches_to_keep` UNCHANGED, deliberately |
| `src-tauri/src/core/adminer.rs` | `WRAPPER_API_MAJOR`, `VERSION_KEY`, `effective_version`, `staged_version`, `verify_pair`; `ensure(platform, version)`; `.adminer.php` dotfile stage + atomic publish for both files + legacy cleanup; wrapper `require` + :468 |
| `src-tauri/src/core/downloads.rs` | `plan_for_start_with(+adminer)`, `plan_for_start_pinned` passes the pin, :868 assertion |
| `src-tauri/src/core/service_manager.rs` | `start_core`/`start_all` take `adminer_version: &str`; :488 passes it. No mirror field, no pin |
| `src-tauri/src/core/php.rs` | `list_versions` call site; the sibling scanner `the_adminer_pin_is_never_used_as_a_version_to_stage`. `the_pin_is_never_used_as_a_patch_to_run` left alone |
| `src-tauri/src/core/copy_scan.rs` | wording guard widened to Adminer |
| `src-tauri/src/commands/database.rs` | `AdminerStatus` + three thin commands |
| `src-tauri/src/commands/php.rs` | `InFlight` `pub(crate)` + `noun` |
| `src-tauri/src/commands/settings.rs`, `core/sites.rs` | the false coverage claim corrected |
| `src-tauri/src/commands/{downloads,services}.rs` | three planner call sites; `start_inputs` gains the version |
| `src-tauri/src/lib.rs` | register three commands beside :895 |
| `src/{types,lib/ipc}/index.ts`, `src/lib/mock.ts`, `src/routes/{Databases,DevUiReview}.tsx`, `scripts/wk-checks/uireview.js` | types, wrappers + mock branches, scenario fixture, the card, harness arms + probes |
| `src-tauri/examples/{adminer_check,adminer_login_gate_check,adminer_update_check,manifest_sweep_check}.rs`, `scripts/live-checks.sh` | probe wiring, honest limits, new network-tier example |
| `scripts/check-php-pins.sh` | reverse Adminer pin + ceiling check |
| `runtimes/scripts/publish-manifest.sh`, `runtimes/docs/MANIFEST.md`, `runtimes/README.md`, `runtimes/.github/workflows/*.yml` | serial-reset fix, explicit-versions fix, superset assertion, Adminer discovery, two-product docs |
| `docs/{ARCHITECTURE,CLAIM-LEDGER,PLAN-binary-updates,DESIGN,SMOKE-TEST,PORTS,TESTING,TODO}.md` | per the CLAUDE.md table, in the same commits |
