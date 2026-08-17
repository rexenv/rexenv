# In-app binary updates — a signed version manifest (proposed)

**Status: PROPOSED, not started.** Written 8 Aug 2026 against `3bac4bc`.
**Revised 16 Aug 2026** after re-reading the record: the premise the original draft
argued from was a paraphrase of the objection, not the objection, and three of the
mechanisms it proposed to build already exist in shipped code. §0 is the correction;
§1–§3 are the trust decision; §4–§9 are what actually gets built. §12 is the task list.

Goal: PHP 8.3.32 ships upstream; a rexenv user gets it from **Settings → PHP → Update**,
not from waiting on a rexenv release. Herd does this for PHP and Node; today we cannot,
and the reason is not UI.

---

## 0. What the record actually says (and what it does not)

The objection is recorded in three places. Its canonical text is `c786ea3` (11 Jul 2026),
now archived at `docs/archive/SHIPPED-2026-07.md:1278-1280`:

> NO in-app TOFU updater — static-php publishes no checksums, so runtime
> update-discovery would **move pin trust from the signed app binary to the user's
> machine**; keep the signed-pin security model.

**Read the clause order.** "static-php publishes no checksums" is the premise of that
sentence; the objection is the emphasis. It is a *trust-anchor* argument, not an
availability one. The commonly-repeated summary — "blocked because the download source
has no hashes" — keeps the premise and drops the conclusion, which inverts what the
objection is about, because the premise is the half that self-hosting changed.

### Does self-hosting 7.4 moot it? No — and this project already wrote down why

`docs/PLAN-php-74-support.md:461-462`, about our own published sums for our own bytes:

> Publish a detached `.sha256` and a `SHA256SUMS` … and say in writing that **they are
> documentation, not a trust root — the pin in `binaries.rs` is.**

That is the whole answer in one line, written *for the self-hosted case*, by the work
that did the self-hosting. A checksum served from the same origin as the artifact
defends against transport corruption and nothing else: whoever can serve you a bad
tarball can serve you its matching digest. The discriminator was never "does a hash
exist upstream" — it is "does a hash exist that something other than the download host
vouches for."

The proof that this is the real rule, not a rationalisation: **every source that DOES
publish checksums is still a compiled-in pin.** Caddy (`checksums.txt`), PostgreSQL
(`.sha256`), Composer (`.sha256sum`), and rexenv's own 7.4 `SHA256SUMS` are all pinned
in `binaries.rs` exactly like static-php's unchecksummed bulk builds. Self-hosting moved
7.4 from "no published hash" to "our published hash" and changed the pin not at all.

### So the verdict: it was about something else, and it still stands

| The premise | Then | Now | Verdict |
|---|---|---|---|
| Upstream publishes no checksums | true of static-php | still true of static-php; **false** for our 7.4 | **moot, and it never was the blocker** |
| A runtime pin moves trust off the app binary | true | **still true** | **stands, unchanged** |
| Self-build + self-hosting is a blocked path | true | **false** — retired 14–15 Aug 2026 | superseded (that was the *sibling* objection, about 7.4, and it is the one that got resolved) |

The two objections travelled together in the same commit (`c786ea3` records both) and
are easy to merge in memory. Only the 7.4 one was resolved.

### Three things self-hosting genuinely changed

1. **It proved the distributor contract works.** `rexenv/runtimes` releases are immutable
   by construction — a rebuild is a new tag (`php-7.4.33-6`), never a re-upload — so a
   pin can 404 but can never silently drift (`binaries.rs:548-565, 610-612`). That is the
   availability half of a manifest, already built and already load-bearing.
2. **It created a licence obligation that the runtime path does not carry.** A *new*
   blocker that did not exist in July. See §9 — it fails open, silently, by two routes.
3. **It did not create any signing infrastructure.** There is still no key of any kind:
   no Developer ID (`tauri.conf.json:43` → `"signingIdentity": "-"`, ad-hoc), no
   notarization, no Tauri updater, no minisign, no pinned pubkey anywhere.

Point 3 deserves saying plainly, because it embarrasses the objection's own wording: the
phrase **"the signed app binary" describes something that does not exist.** The app is
ad-hoc signed and distributed through a Homebrew cask; the pin's actual anchor today is
the cask's `sha256` plus GitHub's account security. That does not argue for skipping a
manifest signature — it argues the opposite, and it means the manifest key would be the
**first signed thing in this project and immediately its most valuable secret.**

---

## 1. What is actually at stake on the user's machine

Before comparing options, the honest baseline — because "it's a local dev tool" is doing
a lot of unexamined work in these arguments, and it is wrong here.

- **`binaries::resolve` is name-generic.** `("caddy", …)` resolves through the identical
  path as PHP, and the resolved cache path is handed to `proxy::start_edge_daemon`, which
  runs ONE privileged shell: `cp {src} {bin} && chown root:wheel {bin} && chmod 755 {bin}
  && launchctl bootstrap system {plist}` (`platform/macos/mod.rs:1056-1062`, invoked at
  `core/proxy.rs:436-438`). Re-installed on every non-adopted start. **A manifest entry
  that can name a binary can name `caddy`, and `caddy` is a root LaunchDaemon with
  `KeepAlive` + `RunAtLoad`.** Live on this machine: `root … /Library/Application
  Support/dev.rexenv.rexenv/bin/caddy`, `-rwxr-xr-x root wheel`.
- **There is no low-privilege entry.** Even a PHP-only compromise runs as the user, and
  the local CA private key is a plain `0600` file (`ca/rexenv-ca-key.pem`) whose cert is
  trusted with **no policy restriction** (`add-trusted-cert -r trustRoot -k <login>`,
  no `-p`; live `security dump-trust-settings` → `Number of trust settings : 0` = all
  policies). Any rexenv-spawned binary reads that key and has browser-wide MITM for this
  user. The prize does not require root.
- **macOS is not a backstop.** `prepare_binary` de-quarantines, relinks, then
  **manufactures** a signature: `codesign --force --sign -` (`macos/mod.rs:1597-1601`).
  Live: `spctl -a -t exec` **rejects** the prepared binary and rexenv executes it anyway.
  For a tree distribution it ad-hoc signs *every* Mach-O in the archive
  (`macos/mod.rs:1917-1922`). The pinned digest is the entire trust decision; nothing
  downstream re-checks anything.
- **A cached binary is never re-hashed.** The warm path compares a marker file the app
  itself wrote (`cache_matches_pin`, `binaries.rs:809-814`). The digest gates the
  *download*, never the *file at exec time*.

Existing ledger rows #67/#155 assert the root daemon never executes a *user-writable*
file — a claim about the file's **location**. A manifest changes its **provenance**,
which that guard never inspects.

---

## 2. The trust decision

The one fact that decides it: **the digest gate verifies the bytes against whoever
supplied the digest.** `binaries.rs:1998-2005` compares the streamed hash to
`spec.checksum`; an attacker-chosen `url` paired with an attacker-chosen `sha256` matches
perfectly and the gate is silent. Every option below is judged on that single sentence.

And the property being spent, stated precisely — this is *not* "we know which bytes run":

> Today, eight independent upstream hosts each face a separately compiled-in digest
> (`binaries.rs:203, 865, 910, 926, 937, 947, 975, 1007`). Compromising any one of them
> changes **nothing** for an installed user: the download fails closed on a mismatch.
> **The pin's product is that compromising a download host does not reach existing
> installs.** A runtime pin source spends exactly that, and concentrates eight
> independent hosts into one.

| | What it trusts | What it actually protects against | Cost | Verdict |
|---|---|---|---|---|
| **A. Signed manifest**, ed25519, pubkey compiled in | a signature made by a key that is *not* on the hosting infrastructure | host compromise, CDN swap, and a stolen GitHub token — none of which reach an installed user | **verify side ≈ free** (see below); **key custody is the real bill** | **Take this — with §3's structural limits, which are not optional** |
| B. Pin host + TLS | any webpki root, plus the host | nothing that matters here | zero | **Reject.** It grants "the bytes came from the host named in the manifest", which is worthless when the manifest is the thing you are worried about |
| C. Floor + additions-only, unsigned | the host, for anything new | the versions a user *already has* (they keep their compiled-in digests) | zero | **Not a substitute.** Adding is the whole feature, and every added version is attacker-chosen in both `url` and `sha256`. It is a *containment* property, not an *integrity* one — keep it as a complement to A, never instead of it |

**The verify side is nearly free, which the original draft did not know.** `ring v0.17.14`
is already in the normal dependency tree (transitively, via `rcgen`) and ships ed25519
verification. This needs a direct-dependency declaration, not a new crate entering the
supply chain. (Unconfirmed: whether the feature set `rcgen` enables exposes the signature
API directly — a build-config question, not a supply-chain one.)

### Which of these threats are theatre, honestly

- **TLS interception is largely theatre here, and by accident we are already ahead.**
  `reqwest` is built `default-features = false, features = ["rustls-tls", …]` with
  `webpki-roots` and no `rustls-native-certs` (`Cargo.toml:36`, confirmed in
  `Cargo.lock`). The downloader does **not** consult the system or login keychain — so
  neither a corporate MITM root nor *rexenv's own installed CA* can intercept it. Any
  argument for signing that leans on TLS-layer attacks is weak here.
- **Archive-level attacks are already hardened** and a manifest adds nothing:
  `safe_join` rejects `ParentDir | RootDir | Prefix`, `link_stays_within` rejects
  absolute and escaping symlink targets (`binaries.rs:2167-2209`).
- **Rollback (serial) defence is real but second-order.** It stops a host that keeps
  serving an older *validly signed* document to hold a user on a known-CVE patch. Worth
  building because it is ten lines; not worth leading the argument with.
- **The signature is not theatre — but its entire value is contingent on custody.** If
  the private key ends up in a GitHub Actions secret in the same account that hosts both
  the manifest and the app, then one account compromise gets all three and the signature
  is ceremony. **It means something only offline or in a hardware token.** Better to say
  that now than to discover it after building the machinery.
- **Key custody is a new concentration of risk, not a restatement of today's.** Today a
  malicious pin must clear a release, a hand-built dmg, and a cask `sha256` bump. A
  signing key is a direct root-on-every-user primitive in one artifact.

### The keyless option, noted and declined

`rexenv/runtimes` already produces SLSA build provenance via
`actions/attest-build-provenance`, verified live and unauthenticated
(`PLAN-php-74-support.md:455-460`). A Sigstore-keyless manifest signature would remove
long-lived key custody entirely. **Declined for now:** verifying it in-app means
embedding Fulcio/Rekor roots and materially more code, and it relocates the anchor to
"whoever can push a tag to the rexenv repo" — which, for a single-maintainer project, is
the same person as "whoever holds the key", for strictly more machinery. Revisit if the
project ever has more than one release operator. **The doc's own rule still binds: the
attestation is a maintainer-side ceremony check and must never move onto the download
path** (`api.github.com`, 60/hr).

---

## 3. Structural limits — the part that is not optional

A signature answers "did the maintainer say this" and nothing else. These four make the
blast radius survivable when the answer is wrongly yes. Each is a *construction*, not a
schema note, because a schema note is a comment.

1. **Name allowlist.** A manifest may describe `php` and `php-fpm`. Nothing else — and
   specifically never `caddy` (§1). Enforced in the merge, where an entry with an
   unlisted `name` is **dropped, not rejected**, so one bad row cannot deny the rest.
   The original draft specified `name` as "matches `manifest()`'s `name` argument", which
   is precisely the unrestricted form.
2. **Scheme + host allowlist.** `https://` only, and only hosts rexenv already downloads
   PHP from. There is **no scheme or host constraint anywhere on the download path
   today** — `http_client()` sets only a user-agent and a connect timeout
   (`binaries.rs:1745-1757`) — an absence that has never mattered because every URL is a
   compiled-in `format!`. A manifest makes `http://attacker/` a valid entry with no code
   change required to accept it.
3. **Re-verify on every read, never a stored verdict.** The cached manifest lives in
   `rexenv.db`, which is `-rw-r--r-- wpdev staff`. Store the **detached signature
   alongside the document** and check it inside `cached()`, every call. A "verified"
   boolean in a user-writable file is not verification; a `sqlite3 UPDATE` would bypass
   ed25519 entirely and land on §1's root path.
   *(This is the original draft's own contradiction: §4 stored the JSON and the serial and
   no signature, while §8 demanded an L0 test that `cached()` rejects a tampered manifest.
   Unsatisfiable as written.)*
4. **Patch-of-a-known-minor only.** `version` must parse as `x.y.z` where `x.y` is
   already in `PHP_VERSIONS`. Not cosmetic: `fpm_port` is arithmetic
   (`9700 + major*10 + minor`, `php.rs:153-160`), so `fpm_port("8.10")` and
   `fpm_port("9.0")` both return **9790**. That collision is unreachable today only
   because `PHP_VERSIONS` is a curated compile-time list. See §4 for why new minors are
   out of scope anyway.

---

## 4. What the feature IS — patch updates, user-pressed, within a minor already installed

Not new minors. Not engines. Reasons, in order of weight:

1. **The mechanism already exists end-to-end.** This is the finding that most changes the
   shape of the work. Shipped today (`lib.rs:325-373`): bump detection
   (`php::seed_registry`) → prefetch through the download hub
   (`downloads::plan_for_php` + `prefetch`) → pool restart
   (`ServiceManager::restart_pools_for`) → `await_ready` → GC of the superseded trees
   (`binaries::gc_outdated_php_caches`). **We are not building an update mechanism. We
   are replacing its trigger and its source of truth**, and everything downstream is
   unchanged. The original draft proposed to build several of these.
2. **Minimal trust surface.** The manifest carries `(version, url, sha256)` for two
   allowlisted names. Nothing else needs to cross the boundary — notably **not the
   fpm port**, which is computed, never supplied (`php.rs:153-160`).
3. **A runtime minor would lie in the UI.** `eol_since` and `xdebug_supported` are
   per-minor compile-time tables. A manifest-delivered 8.6 would render with no EOL date
   and Xdebug silently unavailable — a capability loss presented as a fact.
4. **It is the actual reason to want the feature.** Shipping a PHP *security patch*
   without waiting on a rexenv release is the use case. "Add 8.6 the day it lands" is a
   convenience, and it can ride the same rails later once the anchor is proven.

---

## 5. What happens to sites — and the premise that needs correcting first

**Today, a patch bump already silently restarts live pools at launch.** `lib.rs:325-373`
runs on every launch after an app update that moved a pin: it prefetches and restarts
each bumped minor's pool with no user action and no user-visible notice. So "an update
that silently restarts someone's stack" is not a risk this feature introduces — it is
what ships.

That sets the bar in the right direction: **the in-app path must be strictly more
conservative than the shipped app-update path, not less.**

- **Never automatic. Never at launch.** The refresh is best-effort and silent; the
  *update* is a button, pressed once, per minor.
- **Name the restart before it happens**, and report it after: `restarted the 8.3 pool`.
- **Sites are untouched.** `sites.php_version` stores a **minor** (`"8.3"`), never a
  patch. No config regeneration, no edge reload, no per-site migration.
- **Failure leaves the old patch serving**, because the only thing that switches a pool
  is the persisted patch value.

### The live bug this rests on, which must be fixed first

`seed_registry` (`php.rs:189-219`) pushes a minor onto `bumped` when the stored patch
differs from the pin — and then calls `upsert_php_version` **unconditionally, in the same
iteration**. The row therefore already equals the new pin before the caller has tried
anything. So when the prefetch fails, `lib.rs:352-353` logs and returns with
`// retry next launch` — and **that comment is false**: the next launch computes no bump,
because the row moved on the launch that failed. The user is left with a database saying
8.3.32 and a pool running 8.3.31, permanently, silently.

The root cause is that **`php_versions.patch` records what is *pinned*, not what is
*installed and running*.** That confusion is survivable while the two are the same value
by construction. A runtime source makes them different by design, and then it is fatal.
Fixing it is task 2 (§12) and it is a correctness fix on shipped code, worth doing
whether or not the rest of this is ever built.

---

## 6. Disk — the premise needs correcting too, and the GC is a landmine

> **Shipped 18 Aug 2026.** The rule below is what `php_caches_to_keep` implements and
> `lib.rs` calls; the keep-set is the live pools **unioned with** `php::effective_patches`.
> The analysis is kept because the landmine is the useful half: this section describes a
> GC that would have deleted the tree the user just installed, and nothing in the original
> feature draft mentioned the GC at all.

**A GC already exists.** `gc_outdated_php_caches` runs at every launch and removes any
`php-<version>/` tree whose version is not in the keep-set. Versions do **not** accumulate
with no story today.

The real numbers, measured on this machine, are also bigger than "~31 MB": `bin/` is
**2.9 GB across 34 entries**, and each PHP minor costs **two** trees (`php-<patch>/` CLI
plus `php-fpm-<patch>/` FPM) at 68–104 MB each — **~136–208 MB per minor.**

**The landmine (fixed — this is what it was):** `is_outdated_php_cache` was keyed on
`php::patch_for_minor` — the compiled-in pin, and the exact function a runtime selection
re-points. Under runtime selection it would have deleted, at the next launch, **the tree
the user just selected and is currently running.** And after §7's revert-on-failure it
would have deleted the newly downloaded tree the original draft explicitly promised to
keep ("**The new tree is not deleted**"). The draft never mentioned the GC.

Two further ways the fixed version was still wrong, both found after it was written:
`want.values()` alone missed pools running an OLD patch mid-restart, and a keep-set built
from the LIVE pools alone deletes everything the moment the stack is stopped — with no
pools running, `running_php_patches` returns an empty set rather than "I don't know". The
union of both is the answer, and neither half is redundant.

There is also a foot-gun in this *today*, independent of the feature:
`restart_pools_for` propagates with `?` (`service_manager.rs:1216-1219`), so a failure on
the first minor aborts the loop and every later minor keeps its old master alive — and
the caller catches the error, logs, and **falls through to the GC**, which unlinks those
running masters' trees. On macOS the process survives on the unlinked inode, so it looks
healthy right up until the pool cannot be restarted.

**The rule the GC implements:** keep `{the compiled-in pins} ∪ {each minor's effective
patch} ∪ {every patch a pool is live on}`, delete the rest. Both halves are load-bearing —

> **A floor whose bytes were deleted is not a floor.** Keeping the compiled-in pin's tree
> on disk is what makes "falls back to the pins" an offline guarantee rather than an
> offline *download*.

Cleanup answers, then: an unused version is removed **by the launch GC, automatically,
when nothing references it** — never by a user-facing "delete" button, because the two
things worth keeping are both derivable and neither is a preference.

---

## 7. Offline

The requirement is that a fully-cached install keeps working with the network unplugged,
doing everything it does today. Concretely:

- `refresh()` is best-effort, off the startup path, and gates nothing. Failure is
  *silent-but-visible*: the row reads `couldn't check` with the reason on hover — never
  an error toast, never a blocked screen.
- `cached()` is pure: no network, and it re-verifies the stored signature (§3.3) rather
  than trusting a flag.
- No signature, no manifest, stale serial, no network → the compiled-in pins resolve
  exactly as today. The manifest can only ever ADD a version or move a minor forward.
- The floor's bytes stay on disk (§6), so the fallback is a fallback and not a download.

**One existing divergence to close, because offline makes it bite.** The download
planner's `is_cached` tests file existence only (`binaries.rs:1302`), while `resolve`
additionally requires `cache_matches_pin` **and** `licenses_satisfied`, and **deletes the
cache dir** before re-downloading (`binaries.rs:1444-1456`). When a version's expected
digest changes without its directory name changing — which is exactly what an upstream
in-place rebuild plus a re-pin produces, and what happened on 5 Jul 2026 — the planner
reports "cached, nothing to do" and `resolve` then deletes and re-downloads ~100 MB with
no hub progress row. Offline, that turns a working install into a broken one. Minor
today; a manifest that can re-pin a digest for an existing version makes it routine.

---

## 8. Where it plugs into the code

| Fact | Today | After |
|---|---|---|
| Which patch a minor runs | `php::patch_for_minor(minor)` → `binaries::PHP_VERSIONS` (`php.rs:62`) | the minor's **registered** patch (DB), which the merge sets to `max(compiled pin, manifest selection)` |
| The digest for a version | `binaries::php_sha256(kind, version, arch)` (`binaries.rs:717`) | compiled-in table first, then the verified manifest cache |
| The URL for a version | `php_url(kind, version, arch)` (`binaries.rs:575`) | unchanged for known versions; manifest `url` wins when present, subject to §3.2 |
| Whether we owe licences | `is_self_distributed` → `php_self_hosted_tag` literal match (`binaries.rs:555-565, 658-660`) | **derived from the artifact host** (§9) |

New module `core/updates.rs` — platform-agnostic; fetch, verify, cache, query:

```rust
pub struct VersionCatalog { /* verified entries, merged over the compiled-in pins */ }
pub fn cached(conn: &Connection) -> VersionCatalog;                 // no network; re-verifies the stored signature
pub async fn refresh(conn: &Connection) -> Result<VersionCatalog>;  // fetch + verify + persist (document AND signature)
pub fn newest_for_minor(cat: &VersionCatalog, minor: &str) -> String;
```

**The floor is a version comparison, not a flag.** `registered = max(pin, selected)` by
semver ordering makes "the compiled-in pin is the floor" structural: with no manifest,
`selected` is absent and the result is today's behaviour byte for byte; when the app ships
a newer pin than the user's selection, the pin wins and today's launch-bump path runs
unchanged. There is no boolean anyone can get backwards.

**`patch_for_minor` returns `Option<&'static str>`** (`php.rs:62`). A runtime patch cannot
be `&'static`, so this becomes `Option<String>`.

**And the pin — not the registry — is what a pool actually runs today.**
`PoolManager::ensure` resolves its binary with `patch_for_minor(minor)` (`php.rs:599-604`)
and holds no `Connection`; the same is true of thirteen other call sites (`downloads`,
`sites`, `commands/terminal`, `commands/wordpress`, `commands/php`,
`mcp_server/scratch`, and four examples). `php_versions.patch` is therefore a **mirror of
the pin for the UI and for bump detection**, not a source of truth — which is why moving
the source of truth is the feature's core architectural change and not a tidy-up. See
§12 P4.

---

## 9. The licence obligation does NOT survive the move to runtime

Asked directly: does the guard written for a compile-time list still hold for versions
added at runtime? **No. It fails open, silently, by two independent routes**, and the test
that is supposed to catch it cannot see the case by construction.

1. **The obligation is keyed on a compile-time literal.** `is_self_distributed`
   (`binaries.rs:658-660`) delegates to `php_self_hosted_tag`, which is
   `match version { "7.4.33" => Some(…), _ => None }` (`binaries.rs:555-565`). A
   runtime-added self-hosted version answers `false` → no obligation is detected →
   `licenses_satisfied` returns `true` **vacuously** (`binaries.rs:706-708`) → rexenv
   publishes an interpreter it distributes, with no licence texts, and nothing anywhere
   says so.
2. **Enforcement exists on one resolve path out of four.** Only the single-file `resolve`
   calls `licenses_satisfied` and fetches the licence archive. `resolve_dir` /
   `resolve_bundle` / `resolve_file` never do — **and a manifest entry chooses which path
   it takes, via its `archive` field.** An entry declaring `"archive": "targztree"`
   routes around the obligation entirely.
   **Latent today, not live** — checked before relying on it: `php`/`php-fpm` are
   `Archive::TarGz` single-file specs (`binaries.rs:882-893`), so `7.4.33`, the only
   artifact that currently owes licences, takes the enforcing path. It goes live the
   moment a self-distributed artifact is a **tree or a bundle** — which is precisely what
   the self-hosted `php-debug` build already wired at `PHP_DEBUG_BASE_URL` will be, and
   what any manifest entry may simply declare itself to be.
3. **The guard that should catch this iterates `PHP_VERSIONS`.**
   `every_php_we_distribute_ourselves_ships_its_licences` (`binaries.rs:2957`) loops the
   compile-time slice. A manifest-supplied version is not in it. Same for every other pin
   invariant — arch-pairing, 64-hex-digest, source-host — six loops in `binaries.rs`'s
   test module plus the `manifest_sweep_check` and `php_versions_check` examples all begin
   `for v in PHP_VERSIONS`. A runtime version gets **none** of them.

This is the same family as ledger #318 and the "one-fact lifetime guard" pattern: a check
written against a compile-time fact, kept after that fact became mutable.

**The fix direction — derive the obligation from the artifact host, never from a version
match.** If the bytes come from `github.com/rexenv/`, rexenv is the distributor, full
stop. That is structural: it cannot be forgotten for a new version, because there is no
list to forget to update. Then:

- the merge **refuses** any entry whose host is ours and which carries no licence
  artifact for **both** arches — fails closed, and it is refused at merge time so it never
  reaches a download;
- `licenses_satisfied`'s vacuous `true` becomes reachable only when the host says nothing
  is owed;
- enforcement moves to a point all four resolve paths pass through.

**Doing this is correct today, on its own merits, with no manifest anywhere** — it closes
a latent hole in the exact direction the project is already moving (`php-debug` is wired,
self-hosted, and will not be a single file), and it converts route 1 from a list somebody
must remember to extend into a fact nobody can forget.

---

## 10. UI

`Settings → PHP versions` (`src/routes/Settings.tsx`, `PhpVersionsSetting`), per row:

```
8.3   ● installed · running   8.3.31   [Update to 8.3.32]   [Make default]
8.4   ○ not installed         8.4.23
```

- **Update** appears only when the manifest offers a newer patch for a minor that is
  **installed**. Never shown for a minor the user does not have.
- While updating: the existing download-hub byte row plus the phase line — real bytes,
  same components as the provision card, not a spinner.
- After: the row states the patch it is now on, plus `restarted the 8.3 pool`.
- `Checked just now / 2h ago` beside the section header, from the stored fetch time — the
  Import screen's `scanned 12s ago` honesty. **A refresh that finds nothing must still
  visibly have run.**
- Rollback is the same flow with an older manifest version; the old tree is usually still
  cached, so it is a settings write plus a pool restart. Unlike the DB engines there is no
  data directory behind a PHP patch, so this needs **no** "your databases won't be
  visible" warning. Do not copy that dialog.

---

## 11. Claims this introduces (CLAIM-LEDGER rows, same commit as the code)

| Claim | Layer that can prove it |
|---|---|
| A manifest with a bad/absent signature is never used | L0 — tamper each field, assert `cached()` falls back to the compiled-in pins |
| A stored manifest is re-verified on every read, not trusted from a flag | L0 — `UPDATE` the cached JSON in place, assert `cached()` refuses it |
| A manifest with a serial ≤ the stored one is refused | L0 — replay an older signed document |
| A manifest can only describe `php`/`php-fpm`, over https, from an allowlisted host | L0 — entries naming `caddy`, `http://`, or a foreign host are dropped |
| A manifest version must be a patch of a known minor | L0 — `9.0.0` and `8.10.0` are dropped (the `fpm_port` collision) |
| No artifact is executed without a SHA-256 match | L0 exists (`binaries.rs`) + L1 for the manifest-supplied digest path |
| The compiled-in pins run when the network is gone — **and their bytes are still on disk** | L0 for the resolve; L0 for the GC keep-set |
| The GC never deletes a tree a pool is running | L0 — registered ≠ pin, assert the registered tree survives |
| A failed pool restart leaves the minor on its previous patch | L1 — point a minor at a deliberately broken tree, assert revert + honest error |
| A self-distributed artifact ships its licences **however it was described** | L0 — host-derived, across all four resolve paths, incl. a `targztree` entry |
| ⚠ Our signature attests provenance-at-pin-time, NOT upstream build integrity | 🚫 accepted posture — stated here and in the module doc, not provable by us |
| ⚠ The manifest key is the app's most valuable secret and its custody is not a code property | 🚫 operator posture |

---

## 12. Task list

**REVERSED 17 Aug 2026, and BUILT.** The signed manifest ships. The 16 Aug ruling
turned entirely on key custody — "a practice kept for years, not a commit" — and
that objection dissolved on a fact neither reading had noticed: **the app-side
code is identical whether the private half sits in a CI secret or a hardware
token, because the app holds only the public key.** So custody can start weak and
improve later for the price of a key rotation, with no redesign. §1–§3 stand
unchanged as the analysis; what changed is that its blocker had an exit.

**Status: code complete, DARK until the key ceremony runs.** `RELEASE_PUBKEY` is
empty, so nothing is trusted, `fetch` refuses, and the button never renders. The
ceremony is `scripts/gen-release-key.sh`, by hand, in this order: CI secret
first, then pin the public half, then publish a signed manifest. Any other order
ships a build that trusts a key nobody can sign with.

### The rulings, with the reasoning that produced them

- **P1 — key custody: don't build it.** The caveat in §2 is disqualifying, not a
  footnote. A key in a CI secret in the account that hosts the manifest and the app is
  ceremony: one compromise takes all three, and we would have spent real complexity to
  move a trust boundary six inches. Offline or in a hardware token is the only version
  worth having, and that is **a custody practice kept for years, not a commit** — an
  unowned practice is worse than no signature, because the signature is what everyone
  downstream would then be trusting.

  And the sharper argument is §0's third finding: **there is no signed app binary at
  all.** A manifest key would be the first signed thing in this project and instantly
  its most valuable secret. Protecting PHP patches with a key that outranks everything
  it protects is backwards. **If signing ever happens here, it starts with the app.**

- **P2 — ship the read-only version (§13).** "8.3.32 exists, this build pins 8.3.31":
  no key, no fetch of anything executable, no new trust surface. It turns the actual
  user complaint — *I don't know I'm behind* — into information without moving the
  security model an inch. If people then ask for the button, that is evidence, and it
  arrives alongside whatever has been learned about custody by then.

- **P3 — no mirror.** Another host to keep honest, for a feature we are not building.

- **P4 — delete `php_versions.patch` and derive it.** The pin is what a pool runs:
  thirteen call sites resolve through `patch_for_minor` and hold no `Connection` (§8).
  A mirror that can disagree with the thing it mirrors is the two-sources-of-truth shape
  removed everywhere else in this codebase, and it has already produced one live bug.
  Deriving it makes that bug **unrepresentable rather than fixed**.

  Bump detection has to move with it. It can: a running master's command line names its
  patch, and that is already how adoption identifies a pool
  (`Supervisor::owned_listeners` → `ps -p <pid> -o command=`). Derived from the live
  process, the comparison cannot disagree with reality, because it *is* reality.

### Work

1. ✓ **The GC keeps what is running, not what is pinned.** `gc_outdated_php_caches` keeps
   `{compiled-in pins} ∪ {each minor's registered patch}`. Today those are equal, so this
   is behaviour-identical — and it closes the existing foot-gun where a failed
   `restart_pools_for` lets the GC unlink a live master's tree (§6). — *done 16 Aug 2026,
   `8be6810`, ledger #338, plant-proven.*
2. ✓ **A failed patch bump is retried, not swallowed.** `seed_registry` detected the bump
   and committed it in the same statement, so a failed prefetch left `lib.rs`'s
   `// retry next launch` false and the registry permanently lying about what runs. The
   row now moves only in `confirm_patch`, after that minor's own pool is ready, and the
   launch loop is per-minor. — *done 16 Aug 2026, `c44d717`, ledger #339, plant-proven.*
   **Deliberately fixed standalone ahead of P4, which supersedes it by design**: it is
   live on the shipped path, and a fix that waits on a refactor is a fix that has not
   happened.
3. ✓ **The user's default PHP version survives a relaunch.** Not on the original list —
   found by a scoping pass over the code task 2 had just edited. `upsert_php_version`
   listed `is_default` in its `ON CONFLICT SET`, sourced from the pin, so "Make default"
   silently snapped back to 8.3 on every launch. `installed` sat one line away,
   excluded for exactly that reason. — *done 16 Aug 2026, `89b7599`, ledger #340,
   plant-proven both directions.*

**The order was nearly wrong, and the reason is worth keeping.** The natural reading —
"close the licence gap next, it is the security one" — would have destroyed work:
`is_cached` **agreed** with `resolve_dir` and `resolve_file` (all three member-existence
only) and only `resolve` diverged, so adding `licenses_satisfied` to those paths first
would have turned one divergence into three, and the unification would then have had to
reconcile three predicates instead of extracting one seam. The two items did not conflict
on content, only on order — the kind of conflict that survives review.

4. ✓ **One cache predicate, one dispatch** (§7). `cached_path` is the single answer to
   "would this resolve without downloading", returned by `is_cached` (planner),
   `cached_bin` (sync adoption) and all four resolves — three callers had answered it
   differently, so the planner promised "cached" about trees `resolve` then deleted and
   re-fetched with no hub row. `Shape`/`shape_of` is the single name→resolver mapping,
   which also fixed `composer` being planned through `resolve` (publishing `dir/composer`)
   while every consumer called `resolve_file` (reading `dir/composer.phar`) — the same
   artifact downloaded twice. **§15A ruled and shipped**: `needs_repair` separates "never
   downloaded" from "downloaded, now incomplete", and the app's launch task repairs the
   second — so login-start stays offline rather than prefetching there, which would have
   retired ledger #175 ("never download, never prompt", L0-proven and smoke-tested).
   Residual window stated, not closed: upgrade → never open the app → reboot → login-start
   still refuses, now worded "not ready yet" because the bytes ARE there.
   — *done 16 Aug 2026, `2add15e`, ledger #341, both defects plant-proven separately.*
5. ✓ **The licence obligation is host-derived and enforced on every resolve path.**
   Route 1 (`1f979c5`): the duty reads the artifact's HOST, the licence URL is a sibling
   of the artifact's own URL, ours-but-unpinned refuses — and it closed a gap the old
   `name`-keyed rule could not see, the already-wired self-hosted `php-debug`. Route 2
   (`f0c04d1`): `stage_licenses` is called by `resolve`, `resolve_file` and `resolve_dir`
   alike, so the duty attaches to the artifact rather than to the shape 7.4 happens to
   have; bundles REFUSE rather than fetch, because a `BundlePart` has no version to key a
   digest on. Ledger #336 amended twice, plant-proven both times.

**Remaining:**

6. ✓ **Delete `php_versions.patch`; derive it** (P4). Landed as two commits as required.
   Part one (`67f17c8`): what a pool RUNS became a live fact, read from the master's
   EXECUTABLE via `lsof -d txt` — `ps -o comm=` returns the rewritten title, which names
   the minor and never the patch, and the 7.4/8.0 masters still showing their real path
   is exactly what makes an argv implementation pass a hand check. Part two (`c9c2eb0`):
   migration v36 drops the column, `list_versions` derives `patch` from the pin, and
   bump detection moved after adoption. Ledger #339 RETIRED with its reason, #338/#340
   amended, #342 added. **§15B honoured**: `PhpVersionView::serving` carries the live
   fact beside the pin so the row says both — without it the deletion would have traded
   a fixed bug for a hidden one.

7. ✓ **The read-only "a newer patch exists" row** (P2, §13) — *done 16 Aug 2026,
   `core/php_upstream.rs`, ledger #343, plant-proven.* Landed last as planned, so it
   compares against `patch_for_minor` rather than a column that no longer exists — which
   also dissolved the risk that it would compare against "last confirmed serving" and be
   wrong in exactly the moment a user most needs it honest.

**All seven done.** The signed manifest is not being built (§12), and what shipped
instead answers the question that motivated it without moving the security model.

Everything §11 lists for the *manifest* is not owed, because the manifest is not being
built. The rows that survive are the ones about the GC keep-set, the retry, and the
licence obligation — all three already landed or scoped above.

---

## 13. What ships instead

**This is the decision, not the fallback.** The PHP row states the patch it runs and
that a newer one exists — read-only, no update button, no key, and nothing executable
fetched. A document that can only make the UI say "newer exists" cannot make the app run
anything, which is why it needs none of §2's machinery.

It answers the complaint that actually motivated all of this — *am I on something
stale?* — and leaves the upgrade itself to a rexenv release, where the trust anchor
already is.

Constraints it inherits from §7, which are not negotiable for a check nobody asked to
depend on:

- **Best-effort, off the startup path, gates nothing.** A failed check reads
  `couldn't check` with the reason on hover. Never a toast, never a blocked screen.
- **Offline is a first-class state**, not an error: a check that has never succeeded says
  so plainly rather than implying the build is current.
- **`checked N ago` beside the section header** — a check that finds nothing must still
  visibly have run, the same honesty as the Import screen's `scanned 12s ago`.
- **The row still reads the same when the network is gone**, because everything it
  states about the *installed* patch is local.

Header line for whoever picks this up: the fetched document is UI input and nothing
else. The moment anything downstream of it selects bytes, §1–§3 apply again in full.

## 15. Two product questions inside the remaining work

Both are zero-disruption calls, which makes them the user's, not the implementer's.
**Both were ruled 16 Aug 2026 — A is shipped, B binds the P4 work.**

**A. Closing the `is_cached` divergence adds a login-time refusal.** — *RULED: repair at
app launch. Prefetching at login was rejected because it retires ledger #175, which is
L0-proven and smoke-tested; exempting the licence leg was rejected because it re-splits
the predicate. Shipped in `2add15e`.* `uncached_names`
feeds auto-start's guard, which refuses login-start by name ("binaries not downloaded
yet (php-fpm) — open rexenv and press Start all once"). Once `is_cached` checks the
licence leg, that fires on the first launch of **every existing install whose 7.4 cache
predates 1f979c5** — which ledger #336 already records as every install predating it.
The refetch happens either way (`resolve` repairs it silently today), so what the change
buys is moving an invisible download into a visible planned one; what it costs is a new
login-time refusal on upgrade. Options: exempt the licence leg from the planner
(pin-only), or let auto-start prefetch the licence delta instead of refusing.

**B. Deleting `php_versions.patch` can hide the very bug that fixing it made visible.** —
*RULED: get the live pool fact to the view and say both. A derived row that can only show
the pin is the same silent lie in nicer clothes; it just moves where the lie lives.*
With the column gone the Settings row derives from the pin, so during a failed bump the
UI would assert 8.3.32 while the pool serves 8.3.31 — the identical silent lie #339
shipped to end, now structurally unsayable rather than merely absent. "Unrepresentable"
is only true if step 6's live fact reaches the view. If it does, the row can say
`pinned 8.3.32, serving 8.3.31`, which is strictly better than today. If it does not,
this trades a fixed bug for a hidden one, and `docs/DESIGN.md`'s honest-UI promise should
record which way it went.

## 14. Status of the signed manifest

Not being built (§12, ruled 16 Aug 2026). §0–§11 are kept as the analysis behind that
decision — specifically §0 (what the recorded objection actually said), §1 (what a
compromise reaches on this machine), and §2's table (what each option costs and what it
really protects). If the question is reopened, **it reopens at P1, not at the code**:
the blocker is a custody practice nobody owns, and it is downstream of the app itself
being unsigned.
