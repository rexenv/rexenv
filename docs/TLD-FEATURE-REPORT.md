# Configurable TLD (v1) — implementation report

Built autonomously on 11 Jul 2026 per the approved scope: **v1 = a stored
`default_tld` setting; new sites are created under it.** No migrate-all (a
single site can be re-pointed via the existing Change-domain flow, which now
accepts any allowed TLD). No backward-compat data backfill (no existing users);
the SQLite schema-version migration itself is kept (v8).

**Self-verification state at HEAD (`b1e103d`):** `cargo test --lib` 224 passed /
0 failed · `cargo clippy --lib` clean except one pre-existing warning
(`too_many_arguments` on `rebuild_configs_for`, predates this work) ·
`cargo build --examples` green · `tsc --noEmit` green · `vite build` green.
Every commit below passed the same checks individually before being made.

---

## Commit 1 — `bb1ba89` · sites: policy-driven TLD validation

**What changed**
- New `src-tauri/src/core/tld.rs`: the TLD policy.
  - **Hard-blocked** (refused with a per-TLD reason): `local` (fights
    Bonjour/mDNS), `dev`, `app`, `page`, `home`, `corp`, `mail`, **all
    2-letter TLDs** (length rule), and gTLDs `com net org cloud site online`
    (`io`/`co` fall under the 2-letter rule).
  - **Safe set** (no warning): `test`, `localhost`, `example`, `invalid`
    (RFC 2606/6761).
  - **Warn tier**: everything else (e.g. `.rex`) — allowed, UI shows the
    shadow notice.
  - TLD syntax: lowercase letters only, 1–63 chars (digits/hyphens in a TLD
    are refused).
  - `BACKBONE_TLD = "test"` — in the safe set, can never be blocked.
- `validate_domain` (core/sites.rs) no longer hardcodes `.test`: it runs the
  same per-label charset/shape checks, then `tld::ensure_allowed` on the last
  label. Since `validate_domain` already gated **create, provision, and
  check_domain_change (change-domain)**, the policy holds at the backend trust
  boundary — a blocked TLD is refused even via direct IPC invoke.

**Tests added** — `tld.rs`: safe-set allowed w/o warning; backbone permanently
allowed; every hard-blocked TLD refused with a reason naming it; 2-letter rule
(io/co/uk/de/ai/sh/me/us); warn tier; syntax refusals. `sites.rs`: accepted
domains now include `.localhost/.example/.invalid/.rex/.internal`; blocked-TLD
domains refused **through the real `create` and `set_domain` entry points**
(nothing persisted / row unchanged).

**Note:** `foo.test.evil` (previously rejected because `.test` wasn't last) is
now a *valid* hostname under warn-tier `.evil` — intentional, covered by a test
comment.

## Commit 2 — `509ec95` · dns: per-TLD resolver files

**What changed**
- `DnsHandler` answers **any** A query with `127.0.0.1` (was: NXDomain outside
  `.test`). TLD scope lives entirely in which `/etc/resolver/<tld>` files
  exist — no in-process TLD state, **no DNS restart on TLD add**. The
  loopback-only caveat is documented in the module docs and at the answer site
  (the server must never bind a non-loopback interface).
- `DnsManager` trait: `resolver_path(tld)`, `install_command(tld, port)`,
  `uninstall_command(&[String])` (batch rm + ONE cache flush; flush-only when
  the list is empty so no bare `rm -f`). `resolver_contents(port)` unchanged —
  it doubles as the **ownership signature**. Windows/Linux stubs updated,
  still `todo!()`.
- `core::dns` new helpers: `resolver_installed`, `ensure_resolver` (skip when
  content matches → at most one privileged prompt per TLD, on first use),
  `installed_tlds` (enumerates the resolver dir for files whose content equals
  `nameserver 127.0.0.1\nport 15353\n` — foreign files never touched),
  `remove_all_resolvers`.
- `setup.rs`: system setup installs the `.test` **backbone** (always,
  regardless of default TLD); `run_system_teardown` (→ Settings "Remove system
  changes" / uninstall) now removes **all** rexenv-owned resolver files.
- `dns_status` reports the backbone file path.

**Tests added** — any-TLD (`.rex`, `.example`, even `.com`) answers loopback;
signature scan picks only our files among foreign ones + missing-dir → empty;
macOS uninstall batches files with a single flush; empty-list command has no
`rm`.

## Commit 3 — `7dad2de` · sites: default_tld setting + wiring

**What changed**
- **Migration v8** seeds `settings(default_tld) = 'test'` with `INSERT OR
  IGNORE` (keeps any pre-existing value).
- `core::sites::default_tld(conn)` — the setting, else `test`; a stored value
  that fails policy (smuggled row) **falls back to `test`** instead of
  resurfacing.
- `core::sites::set_default_tld(conn, tld)` — trims a leading `.`,
  policy-gates, stores. The generic `set_setting` IPC command routes the
  `default_tld` key through this setter, so the KV path can't bypass the gate.
- New IPC commands: `default_tld`, `set_default_tld`, `tld_policy`
  (`{allowed, warn, reason}` — display metadata only).
- `create_site` and `change_site_domain` call `dns::ensure_resolver` for the
  domain's TLD **before any row/docroot/backup work** — first use of a TLD =
  one privileged prompt; `.test` and already-installed TLDs are no-ops; a
  declined prompt aborts with nothing changed. The TLD label comes from
  `core::sites::domain_tld`, which re-runs full domain validation first.

**Tests added** — v8 fresh-seed + upgrade-no-clobber; setting round-trip incl.
`.rex` normalization, blocked refusals (`local/dev/io/com`), smuggled-value
fallback; `domain_tld` yields a label only from valid domains.

## Commit 4 — `dc6cb57` · wp-login: host allow-list accepts the site's domain

**⚠ Judgment call — the one change slightly outside the literal 4-commit
split.** The one-click "Log in as / Open admin" mu-plugin hardcoded
`.test`/`.localhost` in its Host check, so on an `.rex` site every magic link
would be **denied** — i.e. "a new site on .rex works" would fail its login
feature. Minimal fix: the mu-plugin is now a per-site template; rexenv injects
the site's own (validate_domain-vetted, `[a-z0-9.-]`-only → can't escape the
quoted PHP string) domain into the allow-list (exact host + subdomains for
multisite). All other guards (tunnel-header denial, loopback client,
single-use, TTL, `hash_equals`) unchanged; `.test`/`.localhost` suffixes kept.
A domain change is picked up because `issue()` rewrites the file on content
mismatch. `ensure_muplugin`/`issue` gained a `domain` param (both command
callers + the live-check example updated).

**Tests** — template injects the domain, no leftover placeholder; rewrite on
domain change; existing guard needles still asserted.

## Commit 5 — `b1e103d` · ui: default-TLD picker + TLD-aware dialogs

**What changed**
- **Settings → DNS & SSL → "Default domain ending"** card: `.`-prefixed input
  + Save. Blocked TLD → backend reason shown in red, Save disabled (backend
  refuses regardless). Warn tier → "may shadow a real internet TLD" notice;
  `.rex` specifically adds "ICANN could delegate it for real use in the
  future". Permanent note: `.test` always stays active; first site on a new
  TLD asks for the password once.
- **NewSiteDialog**: suffix, taken-domain message, and multisite examples
  follow the configured default TLD (`useQuery(["default-tld"])`).
- **ChangeDomainDialog**: regex relaxed to any letters-only TLD (syntax only —
  policy from `tld_policy` query); blocked → red reason + button disabled;
  warn → shadow notice; copy explains the one-time password prompt.
- Uninstall copy (three places) now says **all** rexenv resolver files are
  removed.
- `src/lib/ipc`: `defaultTld` / `setDefaultTld` / `tldPolicy` wrappers with
  non-Tauri mock fallbacks; `TldPolicy` type in `src/types`.

---

## Human-verify checklist (things only you can check at runtime)

1. **Create a site on `.rex`**: Settings → DNS & SSL → set default to `rex`
   (expect the shadow + ICANN note) → New site → suffix shows `.rex` → create.
   Expect ONE admin-password prompt (resolver install) on the first `.rex`
   site, none on the second. Verify `https://<name>.rex` loads with a valid
   cert, and `/etc/resolver/rex` exists containing
   `nameserver 127.0.0.1` / `port 15353`.
2. **Blocked TLD refused in UI AND via devtools**: in Settings, type `local`,
   `dev`, `io`, `com` — each shows its reason, Save disabled. Then in the
   webview devtools console run
   `window.__TAURI__.core.invoke("set_default_tld", { tld: "dev" })` and
   `window.__TAURI__.core.invoke("create_site", { site: { name: "x", domain: "x.local", type: "php", phpVersion: "8.3", webServer: "nginx", path: "" } })`
   — both must reject with the policy message (this is the trust-boundary
   check). Also try `invoke("set_setting", { key: "default_tld", value: "com" })`
   — must reject too.
3. **Shadow warning for an odd TLD**: type e.g. `banana` in the Settings
   picker → amber "may shadow a real internet TLD" note (allowed). Same
   warning appears in Change-domain when entering `foo.banana`.
4. **`.test` unaffected**: with default set to `rex`, existing `.test` sites
   still serve, `https://adminer.rexenv.test` (Databases tab) still loads, and
   `/etc/resolver/test` is untouched.
5. **Change-domain across TLDs**: change an existing site `foo.test →
   foo.rex` (dialog should show warn note; expect no prompt if `.rex` resolver
   already exists). Verify site serves, and **one-click "Open admin" still
   logs in on the `.rex` domain** (commit 4's fix — please test this
   explicitly).
6. **Uninstall removes ALL resolver files**: with both `test` and `rex`
   installed, Settings → Remove system changes → one admin prompt → both
   `/etc/resolver/test` and `/etc/resolver/rex` gone (and a foreign
   `/etc/resolver/*` file you create by hand with different content must
   survive).
7. **DNS behavior**: `dscacheutil -q host -a name foo.rex` → 127.0.0.1;
   a TLD with no resolver file (e.g. `foo.banana` before any `.banana` site
   exists) must NOT resolve.

## Flagged / couldn't self-verify

- **Privileged prompts, keychain, real DNS, real browsers** — everything in
  the checklist above; unit tests cover command construction and policy only.
- **`ensure_resolver` runs before the duplicate-domain check** in
  `create_site`: an already-taken domain on a brand-new TLD would show the
  password prompt before failing with "domain already in use". Harmless
  (resolver install is idempotent and wanted anyway), but noting the ordering.
- **v1 UI limitation (per spec)**: NewSiteDialog only offers the current
  default TLD — to create one site on `.test` while defaulting to `.rex`,
  either flip the default back or create-then-change-domain.
- **Cosmetic `.test` copy left untouched** (scope discipline): Onboarding
  ("anything.test" — still accurate, setup installs the backbone), Services'
  resolver description, WordPressManager/search-replace placeholders,
  `mock.ts` demo data.
- **`examples/wp_login_check.rs`** was updated for the new `issue()`
  signature and compiles, but the live WP login example needs a running stack
  to execute — not run here.
- **Not built (out of scope v1)**: migrate-all-sites to a new TLD; per-site
  TLD choice in NewSiteDialog; showing installed-TLD list in Settings.
