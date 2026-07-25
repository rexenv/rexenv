# Link an existing folder — serve a site from anywhere on disk (Stage 0)

**Status: PLANNED — awaiting three decisions (§11). No code written.** Planned 26 Jul
2026 against `3b0c009`. Stage 0 of the Valet/Herd migration ladder
(`docs/PLAN-valet-herd-migration.md`), but shipped as a **first-class feature**: linking
an existing project is a top ask on its own, and migration merely reuses it.

Goal: a site whose docroot is `~/code/myapp` — served in place, no copy, no move, and
rexenv never deletes the folder.

## 1. The marker: `sites.docroot_managed`, not `linked`

The delete guard today is a lexical prefix test against the CURRENT `sites_dir` setting
(`core/sites.rs:636-650`). Re-point Sites folder at `~/code` and deleting a site whose
docroot is `~/code/myapp` passes the guard → `remove_dir_all` erases the project. The
setting is stored completely unvalidated (`commands/settings.rs:28-35` special-cases only
`default_tld`; the UI writes the picker string raw), so nothing prevents that.

**A second class of undeletable docroot exists, and it is easy to miss.**
`check_docroot_move` (`core/sites.rs:359-390`) imposes NO sites-dir constraint on the
destination — it rejects only a missing source, a relative destination, a destination
inside the source, `target == src`, and an existing target. So a managed site's docroot
can already be moved anywhere, and the move confirm dialog PROMISES
(`src/routes/SiteDetail.tsx:573-576`):

> …a folder outside the rexenv sites folder is kept — not deleted — if you ever delete
> the site.

A `linked` bool defaulting to 0 would mark those moved-out rows deletable and destroy
files today's code preserves. Hence one bit named for what it authorizes:

**`docroot_managed` — "rexenv created this directory and may remove it on teardown."**

| Row | Value | Set when |
|---|---|---|
| Normal created site | 1 | `provision()`, right after it `create_dir_all`s the docroot |
| Linked site | 0 | at link time — we never created it |
| Moved outside the sites dir | 1 → 0 | once, at move time |
| Every pre-v17 row | backfilled | §2 |

Two invariants:

- **Decided where the fact is known** — we either made the directory or we didn't.
- **Monotonic toward safety** — a move may downgrade 1→0, never upgrade 0→1. Linked
  sites refuse `move_site_docroot` entirely; moving the user's project isn't ours to do.

**Why this is structural.** After migration, `teardown` consults one recorded boolean.
The path comparison happens exactly once per row — at create, link, move, or backfill —
and the answer is frozen on the row. Nothing at delete time re-derives from a mutable
setting. Same "recorded, not derived" guarantee already load-bearing for `db_name` (v6)
and `override_port` (v14, ARCHITECTURE §2).

**Why not filesystem truth?** For wp-content assets, deletion keys off the filesystem —
`repo::partition_symlink_deletes` (`core/repo.rs:1645-1668`) lstats each entry because
`wp plugin delete` would walk into a symlink and destroy the real checkout. That works
because a symlink is *observably* a symlink. A site docroot is a plain directory with no
bit distinguishing "I created this" from "the user pointed me here" — provenance must be
recorded when known. Different evidence, same principle. Expect a reviewer to ask.

## 2. Migration v17 + Rust backfill

A plain `NOT NULL DEFAULT` cannot express this — it would have to guess for moved-out
rows. Use the v14 shape (nullable column + Rust backfill + NULL-tolerant reader):

```sql
-- v17
ALTER TABLE sites ADD COLUMN docroot_managed INTEGER;
```

Then `core::sites::backfill_docroot_managed`, run once at startup beside
`backfill_override_ports` (`src/lib.rs:202`), evaluating **today's exact three-root
prefix test** per row and recording the answer. Every existing row keeps precisely
today's behavior: in-tree sites stay deletable, moved-out sites stay preserved. Zero
disruption — the same reasoning v14 uses to record each site's current derived port.

For the window between schema step and backfill, a `NULL` reader falls back to the legacy
prefix test (v14's documented between-phases pattern). After backfill it is dead code for
existing installs and never runs for new rows.

Nothing pre-existing can violate it: nullable, no default, no constraint, no UNIQUE — the
B21/B18 lesson (a UNIQUE-index migration that fails on existing data bricks the DB).

**Migration machinery facts** (verified): `MIGRATIONS` is a `&[&str]`, index+1 ==
resulting `user_version`, currently length 16 → v17 is an append. Each step runs in its
OWN transaction with the `user_version` bump inside it (`state/db.rs:192-225`), so a
failed step rolls back whole and re-runs clean. `execute_batch` means one migration string
may hold several `;`-separated statements. `open_in_memory()` (test-only) skips
`configure`, so **`foreign_keys` is OFF in unit tests** — CASCADE deletes don't fire there.

**Test** (template: the v16 `provisioned` test, `state/db.rs:328-358`): replay
`MIGRATIONS[..16]`, insert two rows the old way — one under the sites dir, one moved out —
run `migrate`, assert both read `docroot_managed = None` through `store::get_site` (which
also exercises `SITE_COLUMNS` + `row_to_site` wiring), run the backfill, assert 1 and 0
respectively, then re-run and assert idempotency (v14's backfill test does exactly this at
`core/sites.rs:1469-1505`).

## 3. What delete removes — linked vs managed

`teardown` is **silent today** either way: no log, no signal, no return distinguishing
"docroot removed" from "docroot kept", and no test asserting either direction. Fix that
here — `teardown` returns `{ existed, docroot_removed }` so the UI can say what happened.

For a **linked site** (`docroot_managed = 0`):

| Removed | Where |
|---|---|
| `sites` row | SQLite (cascades `site_env` v7, `site_git_assets` v12) |
| leaf cert + key | `<app-data>/certs/<domain>/` |
| FrankenPHP config + log, tunnel log | `<app-data>/config`, `<app-data>/logs` |
| the site's server block | config regen + reload |
| our mu-plugins written INTO their docroot | tunnel + login mu-plugins — our litter; remove by exact known path, refusing symlinks |
| our database | our engine only, only the name we generated (`DROP … IF EXISTS` = no-op when we never created one) |

**Never removed:** their directory, or anything in it we didn't write. Their external
database is never touched — we only ever name our own. Guard is
`if site.docroot_managed == Some(true)`; a linked path never reaches `remove_dir_all`
regardless of the sites-dir setting or where the folder sits.

Pre-existing gap noticed while mapping this: **Apache per-site config/log are never
removed** by `teardown` (`apache::config_path`/`log_path` exist but aren't called), and
`change_site_domain`'s cleanup has the same omission. Out of scope here; worth a separate
fix.

New tests cover **both** directions against temp fixtures the test creates itself (managed
→ docroot gone; linked → docroot and contents intact), closing the gap that no test
asserts removal at all. Fixture scoping per the Sites-deletion incident: delete only paths
the fixture created.

## 4. Linked sites never provision

Link means *adopt what's there*. A pure-fs classifier runs at link time — no PHP
execution, the norm `core/repo.rs:1674-1675` already states ("detection itself never
executes repo code"). Same detector Stage 1 needs, so it is built once, here.

- **Existing WordPress** (`wp-load.php` / `wp-config.php`) → type `wordpress`, all WP
  phases skipped, `provisioned = 1`. We serve their install; we don't touch it.
- **`public/` or root `index.php`** → type `php`, docroot resolved accordingly.
- **Empty folder** → link allowed with an honest "nothing to serve yet" note.

Implementation: pass a `linked` flag to `phase_defs` (`commands/site_provision.rs:121-136`)
so the job runs the `prepare, fetch, serve` set regardless of type. Weights renormalize
over the job's own phase keys (`PROVISION_PHASE_WEIGHTS`, `core/sites.rs:786-808`), so
**omitting phases needs no weight bookkeeping**.

**The sharp reason not to route linked sites through the WP phases**, even though they
look idempotent: `configure` is only HALF idempotent. `wp config create` is skipped when
`wp-config.php` exists, but `database::create_database` runs UNCONDITIONALLY right after
(`commands/site_provision.rs:691-700`) and the phase is always marked `"ok"`, never
`"skipped"`. Pointed at an adopted Valet site, that creates a stray empty database beside
their real one. Skipping the phase set avoids the class.

## 5. Preflight validation

New `validate_linked_docroot`, modelled on `check_docroot_move` and
`repo::validate_link_target` (`core/repo.rs:1607-1641`, which already refuses containment
in both directions):

- absolute; exists; is a directory
- canonicalized before storing (kills `..` and symlink games)
- passes the existing B26 charset check (`validate_docroot_path`, `core/sites.rs:471-489`
  — rejects `" $ { } \` + control chars; spaces fine)
- not inside another site's docroot, and does not contain one
- not inside the managed sites dir (create a normal site instead)
- **refuse `$HOME` itself and `/`**, warn on broad containers

The last rule is a security call, not tidiness: a linked docroot can be exposed publicly
via the tunnel feature, and serving `$HOME` would leave `~/.ssh` one dotfile-guard bug from
the internet. The dotfile deny exists in all three vhost templates; depth of defense here
is cheap.

Note `create()` already accepts an arbitrary path verbatim (`core/sites.rs:221-272`) — the
single line that discards it is `core/sites.rs:778`, `new.path = docroot.display()…`.
The TS comment "empty → core computes the docroot" (`src/types/index.ts:72`) is misleading:
a NON-empty caller path is discarded too.

## 6. UI + the disclosure

"Use an existing folder" on step 2 of New Site, replacing the hardcoded `path: ""`
(`NewSiteDialog.tsx:156`), using the same `pickFolder` wrapper as Settings and the move
card (`src/lib/ipc/index.ts:33-45` — lazily dynamic-imports `@tauri-apps/plugin-dialog`,
never a top-level import; returns null on cancel). Show detected type + resolved docroot
after picking. "External folder" badge on Sites + SiteDetail, derived from
`docroot_managed = 0` — honest for linked AND moved-out sites.

UI primitives available are minimal — `src/components/ui/` has only `button`, `dialog`,
`menu`, `toaster`. Use the dialog's local `FIELD_INPUT`/`FIELD_SELECT` constants
(`NewSiteDialog.tsx:368-371`) and the `Field` wrapper; spread `TECH_INPUT` on inputs.

Disclosure appears **before** linking, in the tone `LinkFolderPanel.tsx:80-84` established
("it stays where it is, you keep your own git workflow there") plus the part that must not
be discovered later: **WordPress conveniences write INSIDE the folder** — tunnel and login
mu-plugins, add-from-Git clones, wp-cli — while our vhost, certificate and database live
outside it, and deleting the site never deletes the folder.

## 7. CLI

`rex site create <domain> --path ~/code/myapp`: one `Option<String>` on `SiteCreateArgs`
(`cli_server.rs:55-71`), one `("--path", "path")` tuple in `cmd_site_create`'s flag array
(`cli/src/main.rs:383-427`), one `USAGE` line. Same validation path as the UI — no
parallel logic. (Note the CLI's default site type is `wordpress`; the UI's is `php`.)

## 8. Verification

`cargo test --lib` + `cargo build --examples` per commit, plus a packaged live check
`examples/linked_site_check.rs`: temp project outside the sites dir → link → serve over
the real stack → curl → delete the site → assert the directory and its files still exist.
Written with the Drop-guard discipline from §9, not the leaking shape.

## 9. Prerequisite: the examples orphan class fix

Reaped 4 orphan php-fpm workers on 26 Jul 2026 (PIDs 54106/54107 on :9998,
68265/68266 on :9799, dated Jul 15–16; SIGTERM sufficed). Root cause is a **class defect
in two examples**, and it must be fixed before Stage 0 adds more examples:

- `examples/xdebug_pool_check.rs` (:9998) and `examples/apache_site_check.rs` (:9799)
  spawn php-fpm as a raw `Child` and clean up with a bare `let _ = fpm.kill();` near the
  end of `main`. Two defects stacked: **SIGKILL straight to the master**, leaking the
  workers that hold the port (php-fpm workers survive a killed master — `Proc::alive`'s
  doc comment names this exact mode), and **cleanup that isn't panic-safe** — everything
  between spawn and that line is `.expect()`/`.unwrap()`, so a failed assertion unwinds
  past it.
- `examples/terminal_check.rs` / `terminal_site_check.rs` are NOT defective: their
  `.kill()` is on a `TerminalSession`, which has a `Drop` impl that reaps the child, so
  cleanup survives a panic. A PTY shell also has no worker fanout. **This is the pattern
  to copy.**
- Production does it right: `Proc::terminate` (`core/proc.rs:86`) sends SIGTERM, polls
  20×100ms, escalates to SIGKILL last. `core::proc::Proc` is public and examples link the
  app lib, so the fix needs no new abstraction: wrap in `Proc::Child(child, Instant::now())`
  inside a Drop guard, then sweep the fixture port (a dead master ≠ dead workers).

## 10. Commit sequence

1. `fix(examples)` — the §9 class fix, so new examples don't inherit the broken shape.
2. `feat(db)` — v17 column, Rust backfill, migration + idempotency tests.
3. `feat(core)` — `validate_linked_docroot`, `provision()` honors a caller path, teardown
   switches to the marker and reports its outcome, both-direction teardown tests.
4. `feat(sites)` — fs classifier, linked phase-set skip, move refusal/downgrade rules.
5. `feat(ui)` — picker, disclosure, badge.
6. `feat(cli)` — `--path`.
7. `docs` — ARCHITECTURE §, TODO tick with ✓ evidence, this doc updated.

## 11. Open decisions (blocking)

1. **Name `docroot_managed`** (authorizes deletion) rather than `linked` — it must cover
   moved-out sites or the move dialog's promise breaks. Agree?
2. **Linked sites never provision** — adopt existing WordPress, skip the phases. Also
   want "install WordPress into this empty linked folder" as an explicit opt-in, or leave
   it out of Stage 0?
3. **Refuse linking inside the managed sites dir**, and **refuse `$HOME` and `/`**?

## 12. Threading a new `Site` column — the exact edit set

Recorded because it is easy to miss a spot (the compiler catches most, not all):

1. `state/db.rs` — append the v17 migration string.
2. `state/models.rs` — add the field to `Site`. `SITE_COLUMNS` is documented "in struct
   order" and `row_to_site` uses **positional `row.get(N)`** → append at the END. Serde:
   `#[serde(skip)]` (backend-only, like `override_port`) vs camelCase + `#[serde(default)]`
   (UI-visible, like `provisioned`). A badge implies UI-visible.
3. `state/store.rs:13` — append to `SITE_COLUMNS`.
4. `state/store.rs` `row_to_site` — add `row.get(16)?`; nullable bool idiom is
   `row.get::<_, Option<i64>>(N)? .map(|v| v != 0)`.
5. `state/store.rs` `insert_site` — column name, `?N` placeholder, `params!` value.
6. `state/store.rs` — a setter following `set_site_provisioned` exactly (`UPDATE … WHERE
   id = ?`, return `affected > 0`).
7. `core/sites.rs:248` — the `Site { … }` literal in `create` (won't compile otherwise).
8. Fixture `Site` literals that will also break the build: `core/sites.rs:1380` (`fp_site`),
   `core/sites.rs:1570` (`site_at`), `core/downloads.rs:718`,
   `core/service_manager.rs:2664`, `core/logs.rs:325`, `examples/repo_run_all_check.rs:81`,
   `examples/cli_repo_check.rs:56`.
9. `src/types/index.ts` — mirror if serde-exposed.

## 13. Notes for later stages (found here, needed there)

- **Q2a "serving paused"** (Stage 1): the seam is the `match checks` at
  `commands/site_provision.rs:805-820`; `JobEnd::Ok(String)` already carries an arbitrary
  summary rendered verbatim by the card. Ride it as a **field on `SiteProvisionState`**,
  NOT a new `status` value — `status` is a closed TS union and the card's ternaries
  (`SiteProvisionCard.tsx:135-157`) treat anything not `ok` as failure, so a new status
  would render the `✕` glyph and a frozen "stopped" bar. `END_COPY` (`:30-35`) is the
  advisory-copy extension point.
- The provision card's success text has **no green styling** — `job.summary` renders in
  muted mono (`SiteProvisionCard.tsx:213-220`); only the `✓` glyph is coloured.
- `site_provision_retry` re-ensures prepare artifacts including `create_dir_all` on the
  docroot and a Blank-PHP `index.php` **only if missing** — check it stays correct for
  linked rows.
- The classifier built in §4 is the Stage 1 detector — extend it there for Bedrock
  (`web/`), Radicle (`public/`), Craft (`web/`), Symfony/Statamic (`public/`), Magento
  (`pub/`), Drupal (`docroot|public|web`), plus flags for `LocalValetDriver.php` and
  `~/.config/valet/Drivers/*` (arbitrary PHP choosing the docroot — never parse it, flag
  the site as needs-attention).
