# Stage 1 — import Valet/Herd sites (read-only scan → review → import)

**Status: SHIPPED 26 Jul 2026** (commits `42af4fc` → `7d7c340`), with TWO
verifications still outstanding: the clean-VM resolver-takeover paths (§9 here =
`PUBLISH-TESTING.md` §F) and the packaged-app GUI pass of the `/import` screen
(`PUBLISH-TESTING.md` §G — the screen has only ever been type-checked). All four
decisions resolved (§11). Planned 26 Jul 2026 against `5e8231f`, verified against
a live Valet 4.12.0 + Herd 1.29.0 install (the migration doc's "reference
install"). Stage 1 of `docs/PLAN-valet-herd-migration.md`; builds directly on
Stage 0 (`docs/PLAN-linked-sites.md`). This doc is the canonical home of the
scan's mess taxonomy (§2) and the resolver ownership/takeover design (§4).

Databases are Stage 2. Connection-config rewriting is Stage 3. Stage 1 ends with the
user's Valet/Herd sites served by rexenv, from their existing folders, with their
databases still pointing wherever they already point.

## 1. What Stage 0 already gives us

Most of the per-site machinery exists and is live-verified:

- `core::sites::detect_project` — pure-fs classifier, **already handles Bedrock and
  Radicle** (wp-config not at the served root), Laravel, Craft, Statamic, Symfony,
  Magento, generic `public|web|www`, plain PHP, static. No extension needed for Bedrock;
  the Stage 1 additions are only what the scan itself needs.
- `core::sites::has_custom_valet_driver` — flags `LocalValetDriver.php`.
- `core::sites::validate_linked_docroot` — the refusal set, and the source of each row's
  honest "can't import, because…".
- `commands::sites::inspect_linked_folder` — **already the per-row scanner**. The scan is
  essentially "enumerate their parked/linked paths, run this per path".
- Linked provisioning: non-empty `NewSite.path` serves in place, `docroot_managed=false`,
  never written into, never deleted.

## 2. The scan (`core/valet.rs`, read-only)

**Hard rule: filesystem reads only.** We never write to their trees, never start or stop
their services, never execute anything in a project folder, and in Stage 1 never open a
file *inside* a user project — `detect_project` does existence probes only. (Reading
wp-config/.env is Stage 2, with its own secrets handling.)

### Discovery

1. Locate sources: Valet home (`~/.config/valet`, else legacy `~/.valet`), Herd
   (`~/Library/Application Support/Herd/config/valet`).
2. Per source read `config.json`: `tld = tld ?? domain ?? "test"`, `loopback`, `paths`.
3. Sites come from two places, per Valet's own model:
   - **Linked** — symlinks in `<home>/Sites`; hostname = link name + tld.
   - **Parked** — each immediate subdirectory of every parked path; hostname = dir name
     + tld. (None on the reference install, but Valet's default model.)
   Links win over parked on a name collision, as Valet does.
4. Per-site nginx conf (`<home>/Nginx/<fqdn>`) supplies: the isolated PHP version
   (`# ISOLATED_PHP_VERSION=`), proxy-ness, and secured-ness pairs with
   `Certificates/<fqdn>.crt`.
5. Dedupe across sources by domain, **Herd wins** (it auto-migrates Valet on first launch,
   so its copy is the superset), with a note recording that the site also exists in Valet.

### Mess tolerance — every case, and how it surfaces

Every "live example" below was observed on the reference install (a real two-tool
setup). Nothing crashes, nothing is silently dropped.

| Case | Live example | How it surfaces |
|---|---|---|
| `config.json` missing / unparseable / unreadable | — | Source row with an issue note ("couldn't read Valet's config"), zero sites from it. Never fatal to the other source. |
| Duplicate parked path differing by a trailing slash | Herd listed `…/Sites` twice | Canonicalize + dedupe before scanning. Silent — it's noise, not information. |
| Parked path that doesn't exist, or is empty | a parked `~/Herd/`, empty | Counted in the source summary ("1 parked folder, empty"), no rows, no warning. |
| Symlink present but target missing | **a third of the Herd rows**, several Valet ones | A ROW, status **unsupported — "folder missing"**, target path shown. Never silently skipped: a dangling link is a site the user thinks they have. |
| Conf + cert with no Sites entry at all | two orphan confs (one with a doubled suffix, `<name>.test.test`) | A ROW, status **unsupported — "leftover config, no site folder"**. Nothing to import; listed so the count reconciles with what they see in Herd. |
| Valet proxy entry | none on the reference install | A ROW, status **unsupported — "proxy to `<url>`, not a site"**. Detected by the `# valet stub: *proxy*` first line + `proxy_pass`. |
| Conf on a different TLD from `config.tld` | a `.dev` conf | Normal row; its TLD joins the set that needs a resolver (§4). (That one's target was also dangling, so it landed unsupported for that reason.) |
| Isolation marker format drift | `8.4`-style AND bare `82`-style side by side | Parser accepts `php@8.1`, `8.1`, and bare `81`. Both forms coexist in real installs — the bare form is not legacy. |
| Prefs claim PHP versions whose binaries are absent | plist claimed 7.4–8.1; `bin/` had only php82–85 | **Prefs are never read.** Only the per-site conf marker matters, and only against OUR pinned set. |
| Site name that isn't a valid hostname label | — | Status **unsupported — "name isn't a valid hostname"**. Must never reach cert issuance or config generation. |
| Non-UTF8 / unreadable directory entries | — | Skipped, counted in a source note ("2 entries unreadable"). |
| Domain already exists in rexenv | — | Status **already imported**, row shown unticked and disabled. |
| Folder overlaps an existing rexenv site, or is inside the sites folder / blast radius | — | Status **needs attention**, with `validate_linked_docroot`'s own message as the reason. |
| `LocalValetDriver.php` present | — | Status **needs attention — "custom driver decides this project's docroot"**, with our detected docroot shown so they can confirm or correct. |

### Status taxonomy (per row)

- **importable** — ticked by default.
- **needs attention** — importable only after an explicit per-row choice (PHP version
  substitution, custom-driver confirmation). Unticked by default.
- **unsupported** — cannot be imported (missing folder, proxy, no folder, bad hostname).
  Shown, never tickable.
- **already imported** — shown, disabled.

### Cost note

`validate_linked_docroot` iterates `list(conn)` per call, so an N-site scan is N
sites-list queries under the DB lock. Fine at Valet scale (~30 on the reference install); if it ever matters,
hoist the list once and pass it in.

## 3. PHP mapping and the two traps

We pin 8.0–8.5; **7.4 has no build and never will** (static-php.dev never published it).

- Marker minor is in our pinned set → map straight across.
- Marker minor is not pinned (7.4, or anything newer than we ship) → **needs attention**
  with an explicit choice: import on a named available version, or skip this site. Never
  a silent substitution.

**Trap 1 — never write an unpinned version.** A site row on an unpinned minor makes the
serve phase fail (`ensure_php_pool` → "no pinned PHP build"), leaving a `provisioned=0`
half-site. The review UI refuses to tick such a row until a version is chosen.

**Trap 2 — `set_installed` before creating anything.** A row on a *pinned but not
installed* minor serves once, then dies at the next Start-all (`start_inputs` only plans
and starts `installed_minors`) — and it cannot be cleaned up afterwards, because
`set_installed(minor, false)` refuses any minor a site uses. So the import does, in this
order, copying `commands/php.rs:31-52`:

1. `php::set_installed(conn, minor, true)` for every distinct minor in the selection —
   brief DB lock, no service lock.
2. `downloads::prefetch(plan_for_php(minor))` once per minor, unlocked, before the loop.
3. Only then start creating sites.

## 4. The resolver — the root-touching part

### 4.1 What's wrong today

`dns::ensure_resolver` skips only when the file's content is exactly ours; a Valet file
(no `port` line) falls through to a privileged `printf >` that **silently overwrites it**.
Ownership in this system is content equality, and that has a second consequence: once we
overwrite their file it carries our signature, so today's teardown sweep would `rm` it and
the user is left with **neither their file nor ours**.

### 4.2 Ownership classification (new, pure, testable)

```rust
pub enum ResolverOwner {
    Absent,                        // no file — a plain install, nothing to consent to
    Ours,                          // our exact signature — no-op
    Foreign { content: String },   // someone else's — consent required
}
pub fn resolver_owner(platform, tld, port) -> ResolverOwner
```

Factored like `tlds_matching_signature` (which already takes a directory) so it is
unit-testable against fixture files in a temp dir — necessary, because the dev machine had no
`/etc/resolver/test` to exercise the foreign path against (§9).

### 4.3 Making silent overwrite structurally impossible

`ensure_resolver` gains a foreign check and **refuses** with a typed error naming the
holder. Its four call sites (onboarding `.rex`, site create, provision retry,
change-domain) all get the honest refusal instead of clobbering. The only write path that
may replace a foreign file becomes `take_over_resolver`, which cannot run without first
recording a backup — so "silently overwrote someone's file" stops being reachable rather
than merely being avoided by callers.

### 4.4 The consent flow

Shown in the migration screen, per TLD needing a resolver, before any import runs:

1. **Detect** — `resolver_owner` per distinct TLD across the selection.
2. **Present** — their file's content and ours, side by side, verbatim, in mono blocks,
   with who we believe owns it (Valet/Herd, inferred from what's installed) and the plain
   consequence: *while rexenv owns this file, `.test` resolves through rexenv; Valet and
   Herd recreate it whenever you run `valet install` or Herd's onboarding.*
3. **Consent** — an explicit checkbox, unticked. `ConfirmDialog` has no checkbox
   affordance, so this is an inline panel in the screen, not a modal.
4. **Alternatives, equally weighted** — *Import on `.rex` instead* (re-home: no resolver
   conflict, their URLs change) and *Cancel*.
5. **Take over** — in this order, so a cancelled prompt can never lose their content:
   read content → write our backup (`write_private`, born 0600) → insert the record →
   *then* run the privileged install. If the privileged step fails or is cancelled, roll
   the record and backup back; nothing changed on disk.

### 4.5 The record (v18)

No JSON-in-settings precedent exists (blueprints use a table column), and this must
survive to teardown, so it is schema:

```sql
CREATE TABLE resolver_takeovers (
  tld         TEXT PRIMARY KEY,
  original    TEXT NOT NULL,   -- their exact content, as read
  backup_path TEXT NOT NULL,   -- our 0600 copy under app-data
  taken_at    TEXT NOT NULL
);
```

Backups live in `<app-data>/resolver-backups/<tld>-<timestamp>` — a new directory; the
existing "safety backup" precedent (DB dumps) targets Downloads, but that's for user
artifacts and this is ours.

### 4.6 The backup file's lifecycle — no orphans by construction

The record is the **sole owner** of its backup file, and the backup is named per TLD, not
per timestamp: `<app-data>/resolver-backups/<tld>`. The TLD is `[a-z]{1,63}`, so it is a
safe filename, and **at most one backup per TLD can exist by construction** — a timestamped
name is what would make an orphan representable, so we don't use one.

Every transition creates or deletes the file in the same operation as the row:

| Transition | Backup file |
|---|---|
| Takeover | written (`write_private`, 0600) before the row is inserted |
| Takeover rolled back (privileged step failed/cancelled) | deleted with the row |
| Restore (hand-back or uninstall) | deleted after a successful restore |
| Drift — they took the file back, record dropped as moot | **deleted with the row** |
| Re-takeover of the same TLD | same path, overwritten (`write_private` re-hardens an existing file to 0600 before writing) — the newest backup is the right one to restore, since it's what we actually replaced |
| Backup missing at restore time | nothing to delete; we remove our file and say so |

Belt for the crash window between writing the file and inserting the row: a startup sweep
deletes any file in `resolver-backups/` with no matching record. Cheap (one `read_dir`
against one query) and it makes accumulated litter unrepresentable rather than unlikely.

### 4.7 Hand it back — one click, not "uninstall rexenv"

If we borrow their file, the return path has to be visible, so the resolver card gets a
per-TLD **"Hand `/etc/resolver/test` back to Valet"** action. It is the same operation as
the uninstall restore (our content + record → restore the backup, drop the record, delete
the backup), just wired to a button.

Its confirm must state both consequences plainly:

- their `.test` sites resolve through Valet/Herd again;
- **any rexenv site on that TLD stops resolving** until they take it over again or re-home
  it to `.rex` — with the count of affected sites shown, since we know it.

### 4.8 Uninstall — the decision table

Evaluated per TLD inside `run_system_teardown`, still one privileged prompt:

| File content now | Record? | Action | Why |
|---|---|---|---|
| ours | yes | **restore their backup**, drop record | we replaced it; put it back |
| ours | no | `rm` (today's behavior) | we created it |
| foreign | yes | **leave it alone**, drop record | they took it back (`valet install`); not ours to touch |
| foreign | no | leave it alone | never enumerated — today's behavior |
| absent | yes | drop record | nothing to restore |
| absent | no | nothing | — |

Backup file missing but the record says we took over → `rm` our version and **say so** in
the completion message ("couldn't find our copy of Valet's `/etc/resolver/test`; run
`valet install` to restore it"). Removing is closer to their pre-rexenv state than leaving
ours, and silence would be the worst option.

**We never remove a file we didn't create.** Foreign files without a record are invisible
to the sweep, exactly as today.

### 4.9 Escaping

Restore runs as root. Their content must never be interpolated into a shell string, so the
command is `cp <sh_quote(backup_path)> /etc/resolver/<tld> && chmod 644 …` — `sh_quote`
already exists and takes a `&Path`; the app-data backup path contains spaces, and the
existing DNS commands set no quoting precedent (they're safe only via the `[a-z]{1,63}`
label invariant, which still holds for the TLD half).

### 4.10 Drift — the cheap check now, the watcher later

The health watchdog only probes the wire, never the file, so if Valet or Herd takes
`/etc/resolver/test` back while we own it, our resolver keeps answering on 15353, the
watchdog stays green, and every rexenv site on that TLD goes dark with no explanation.
That is exactly the honest-UI failure we keep designing away.

**Now: the cheap check, at the two places we already look at environment truth** — app
startup and `rex doctor`. Reading one small file per taken-over TLD is nearly free and
needs no watcher. When a TLD we hold reads foreign:

> the `.test` resolver is no longer ours — Valet or Herd took it back. Your rexenv `.test`
> sites won't resolve until you take it over again or move them to `.rex`.

with the take-over action right there.

**Later: the continuous version.** *(Reconciliation note, 28 Jul 2026: Stage 2
shipped WITHOUT picking this up, and the cheap check's user surface is itself
still unwired — both now tracked as one item in `docs/TODO.md`.)* Polling belongs
with a watcher and can wait;
nothing in Stage 1 depends on it.

Note `dns_status.resolverInstalled` is a bare `path.exists()` on `.rex` that gates
`FirstRunGate` and the Onboarding step-2 lock, so its semantics must **not** change here.
Drift reporting is a separate per-TLD query.

## 5. The import loop

**Sequential, one site at a time.** Parallel is technically allowed (the busy refusal is
per-domain) but everything else argues against it: the download hub has a single batch
slot, each serve phase takes the services lock and does a full edge reload, and
`site_provision_active` only ever adopts the newest job.

Per batch, before the loop: resolve resolver consent per TLD (one foreground prompt at a
predictable moment, not mid-batch), `set_installed` per distinct minor, then one prefetch.

Per site: reuse `site_provision::start` + poll `state_of` — the exact drive loop
`create_site` and the CLI already use. No parallel create path. Each row gets a live
outcome (`StepDot` vocabulary) and, on failure, the failing phase plus its log key.

**Failure semantics — continue, never abandon.** Each site is INDEPENDENT, unlike Run-all
for repo assets where stopping is right because building after a failed install is
pointless. Here a failure on site 3 must not cost sites 4–20. So: sequential,
**continue-on-failure**, per-row terminal status (`imported` / `failed` + its reason and
log key / `skipped` by choice), and an end-of-run summary naming exactly which succeeded
and which did not.

**Cancel stops after the current site.** The in-flight site finishes or fails on its own
terms; the remainder are left un-attempted and clearly marked as such. We never abandon a
site mid-import, because a half-created site is precisely the `provisioned=0` state we
make users clean up by hand.

## 6. Two fixes that belong in this stage

**`build_plan` over-fetches for linked sites.** It branches on `site_type == Wordpress`
but lacks the `linked` check `phase_defs` right above it already has, so importing a
linked WordPress site prefetches **MySQL (~600 MB) + PHP CLI + wp-cli** for phases the job
will never run. On a 12-site Valet import that is a large, pointless first download — not a
micro-optimisation but the difference between a fast import and a ~600 MB download during
someone's first five minutes with the app. Give it the same `linked` awareness.

**Q2a — "serving paused" instead of a false success.** The serve phase reports
`created — serving at https://…` even when a shadow-binding Herd is answering :443. Ride
a `servingBlocked` **field on `SiteProvisionState`**, not a new `status` value: `status` is
a closed TS union and the card's ternaries treat anything that isn't `ok` as failure, so a
new status would render the ✕ glyph and a frozen bar. Source the flag from the
ServiceManager's cached `edge_blocked` (free — the serve phase already holds the lock)
rather than a 3s probe per site, and have the import summary do **one** explicit
`edge_answers_as_ours` probe at the end, surfacing "imported — serving paused until you
quit Herd" with the copy-paste quit command the existing detection already produces.

## 7. UI

**Route `/import`, no permanent nav item** — migration is a near-one-time action and a
permanent nav slot would misrepresent it. Entry points:

- **A Sites-page affordance**, shown only when a scan would actually find something, and
  most prominent where it is most useful — an empty or near-empty site list is exactly
  when someone wants to import. **Dismissible**, and it stays dismissed, so it never nags
  someone who has no Valet or Herd.
- **A Settings card**, always available, honest when a scan finds nothing.

Components to copy, all existing:
- **`PluginsPanel`** (`WordPressManager.tsx:2648+`) — `Set<string>` selection, select-all
  with `indeterminate`, a `selectable` list that excludes non-actionable rows (exactly our
  unsupported/already-imported rows), and the bulk bar with count + Clear.
- **`StepDot`** (`repoJobUi.tsx`) — `pending/running/ok/failed/skipped` maps 1:1 onto
  per-row import outcomes.
- **Onboarding's planned-rows-overlaid-by-live-state** pattern for progress: static scan
  rows, live status keyed by row id. No batch-job machinery exists to reuse and none is
  proposed — the loop is one job at a time, and the screen owns the roll-up.

**Shipped since (8 Aug 2026):** "also copy databases" is now TICKED by default on the
screen — rexenv is the whole stack, so someone leaving Valet/Herd is leaving their
database engine too, and the unticked box left them with imported sites still reading
the old engine. The WIRE default stays off (`#[serde(default)]`), so only the screen
opts in and a scripted caller that never mentions databases never copies one.

The batch also streams `valet-import://progress` (`ImportProgress`)
between the terminal row events, because a large site — or a multi-GB database — was
otherwise minutes of silence. The tick carries the stage, the running job's OWN phase
label verbatim, that job's pct, and a batch pct of `settled rows + the in-flight site's
fraction` (60/40 split between a site and its database when databases were requested).
Honest by the provision-card rules: monotonic, 99-capped until settle, frozen where the
work stopped, and every number comes from a job that really reported it.

## 8. Commit sequence

1. `fix(dns)` — ownership classification; `ensure_resolver` refuses a foreign file
   (typed error, all four call sites); fix the stale `configure_resolver` doc claiming
   onboarding batches CA-trust with the resolver install (it doesn't and can't). Tests
   against fixture dirs. **Ships the bug fix on its own.**
2. `feat(db)` — v18 `resolver_takeovers`, backup write via `write_private`,
   restore-aware teardown (§4.8 table), `sh_quote`d restore command. Tests for every row
   of the table.
3. `feat(core)` — `core/valet.rs`: discovery + mess tolerance (§2), against fixture trees
   built by the tests.
4. `feat(commands)` — scan IPC, per-TLD resolver ownership IPC, takeover IPC.
5. `fix(provision)` — `build_plan` linked-awareness; `servingBlocked` field.
6. `feat(import)` — the import command: consent → `set_installed` → prefetch → sequential
   `start`+poll → per-row outcomes.
7. `feat(ui)` — the migration screen, resolver consent panel, entry points.
8. `test(examples)` — `valet_scan_check`: scan the dev machine's real trees read-only
   and print the classification, asserting nothing is written.
9. `docs` — ARCHITECTURE, plan, TODO tick with ✓ evidence.

## 9. Verification, including an honest gap

Unit tests for: the marker parser (all three formats), dedupe with Herd winning, every row
of the mess table against fixture trees, the resolver ownership classifier, and every row
of the restore decision table.

`valet_scan_check` runs the scan against the **real** Valet/Herd trees on the dev machine and
asserts read-only behavior — a real install is an unusually good fixture (~30 sites, a third
dangling, 2 conf-only orphans, a custom-TLD conf, both marker formats).

**The gap: `/etc/resolver/test` does not exist here**, so the takeover and restore paths
cannot be live-verified without creating a foreign root-owned file, which I won't do on
the dev machine. They will be unit-tested against fixture directories, and the live check
covers only the absent-file path. Stated rather than papered over; a clean-VM pass would
close it.

## 10. Explicitly out of scope

Databases (Stage 2), connection-config rewriting (Stage 3), importing their certificates
(never — we always issue our own), touching their nginx/dnsmasq/services in any way, and
any "clean up your old environment" affordance.

## 11. Decisions — all resolved 26 Jul 2026

1. **Entry points** — route + dismissible Sites affordance (only when a scan would find
   something) + Settings card. No permanent nav item. §7.
2. **Hand-back button** — YES. If we borrow their file, returning it must be one click,
   not "uninstall rexenv". §4.7.
3. **Drift detection** — the cheap check now at startup and `rex doctor`; the continuous
   watcher deferred to Stage 2. §4.10.
4. **Sequential import** — yes, and continue-on-failure: each site is independent, so a
   failure on site 3 must not cost sites 4–20. Cancel stops after the current site. §5.
