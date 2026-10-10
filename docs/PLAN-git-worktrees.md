# Git worktree workflow — one branch, one running site

**Status:** BUILT 10 Oct 2026 — W1–W11 (W7 = adopt a worktree another tool made, Shape B), the
panel, `rex worktree`, the MCP `worktree` tool; live checks green on macOS, the Dell and the
Ubuntu VM (#814–#823, #830–#831). SMOKE GUI rows: 9 of 10 run on the macOS VM, 10 Oct 2026 — three defects found and fixed (#848). Prune and the adopt/serve/prune/re-clone CLI + MCP arms built (#849). Legs 13–15 and the
prune leg ALL PASS on the Dell's Windows 10 and the Ubuntu VM (10 Oct 2026). Open: the SMOKE GUI
rows on Windows/Linux. Was:
PLANNED 9 Oct 2026. Owner request ("Git worktree workflow"); the
agent wrote it while the owner was away. **Owner answered 9 Oct 2026** (§9). Domain:
`feature-x.shop.rex` first, falling back to `shop-feature-x.rex` (§2.2). The WP plugin/theme
shape comes FIRST (§2.1, task order in §8). Planned against `00bc4dd1` (v0.8.13). Task
list: §8, mirrored as one row in `docs/TODO.md`.

> **The one-sentence design:** a worktree site is a **linked site whose folder is a
> `git worktree` of another site's checkout** — everything after the folder exists
> (vhost, PHP pool, cert, DNS) is the linked-site path that already ships; what is new is
> making the worktree, giving it its own database and config, and removing it without
> ever running `rm -rf` on anything.

---

## 0. Why this feature, and who it is for

A developer working on `shop.rex` gets a hotfix request while half-way through a feature
branch. Today they stash, switch branch, maybe re-run migrations against the ONE database,
and switch back — and the database is now in whichever branch's shape ran last. With
worktrees: `feature-x` keeps running at `shop-feature-x.rex` with its own database while
`main` runs at `shop.rex`, both at once, both with HTTPS.

The second audience is bigger and newer: **AI coding agents work in worktrees.** Claude
Code (`.claude/worktrees/…`), Cursor background agents, Conductor and similar tools create
one worktree per task so parallel agents never share a checkout. Those worktrees have no
running site today — the agent can edit PHP but cannot load a page. rexenv already ships
an MCP server; a `worktree` action there is what turns "agent in a worktree" into "agent
in a worktree with a live URL it can test".

## 1. What already exists (read before designing anything)

| Need | Already in the tree | Notes |
|---|---|---|
| Serve an arbitrary folder as a site | linked sites, `docroot_managed = 0` (`docs/archive/PLAN-linked-sites.md`) | teardown never `remove_dir_all`s a linked folder — only rexenv's own mu-plugins by exact path |
| A site whose root is a git checkout | `repo_site_info` / `SITE_KIND = "site"` (`commands/repo.rs:232`, `:1037`) | `present` tests `root.join(".git").exists()` — already true for a worktree (`.git` is a FILE there) |
| `.git` as a file accepted | `core/repo.rs::scan_unmanaged` (`:1831`, test `:3673`) | worktrees + submodules |
| Every git call safe + cancellable | `core/repo.rs` (`with_git_env`, `GIT_TERMINAL_PROMPT=0`, process group via `ProcessSupervisor::spawn_streamed`) | system git through `devtools::resolve_git` + `ShellRunner::git_preflight` |
| Branch picker | `src/components/wordpress/RefPicker.tsx` | used at three places already |
| Honest git errors | `core/repo.rs::map_git_error` | auth / not-found / no-such-ref |
| composer / npm install + asset build on the site's PHP | `core/repo.rs::composer_install`, git-site provisioning phases | `docs/archive/PLAN-git-site-clone.md` |
| `.env` rewriting | `core/laravel.rs::wire_env` | tested against a real generated `.env` |
| URL rewrite inside a copied WP DB | `core/wordpress.rs::rehome_urls_on_copy` (`:2854`), `url_rehome_pairs`, `network_rehome_pairs` | wp-cli `search-replace --all-tables` + proof that `siteurl` moved |
| Dump/restore building blocks | `dbdump::DefaultsFile`, `dbrestore::record_provenance` / `prepare_target` / `feed` / `verify_complete` | **but** `dbdump::gate` REFUSES a source that is rexenv's own server (`classify_self_import`, `dbdump.rs:96`) — on purpose, for the Valet/Herd import |
| Streamed job card with phases, cancel, retry | `commands/site_provision.rs` + `SiteProvisionCard.tsx` | `ProvisionJobs::busy_for` stops two jobs on one site |
| Tree copy without shelling out | `core/scratch.rs::clone_tree` (`:415`) | used for scratch sites |

**What does NOT exist:** any `git worktree` call; any site→site relation (no
`parent_site_id`); any same-server DB copy; any "duplicate site".

## 2. The decisions (owner answers in §9)

### 2.1 Which sites can have worktrees — two shapes, **Shape A first** (owner, 9 Oct)

**Shape A (built first): a plugin or theme checkout inside a WordPress site.** This is how
most WordPress developers work. The repo is `wp-content/plugins/my-plugin` (a
`site_git_assets` row, or any `wp-content/{plugins,themes}/<dir>` with a `.git` that
`scan_unmanaged` finds), and the site itself is not in git. The child site is **the parent
site copied, with that ONE asset directory replaced by a worktree of the asset's repo**:

1. rexenv creates the child docroot `<sites_dir>/<child-domain>/` (a managed folder,
   `docroot_managed = 1`).
2. It copies the parent's docroot into it with `clone_tree`, **skipping the asset dir**.
   Other git-checkout assets are copied whole, `.git` included, so each is an independent
   copy. The dialog lists them with their size.
3. `git -C <parent asset dir> worktree add <child>/wp-content/plugins/<dir> <branch>`.
4. The asset's own deps run in the worktree (`composer install`, package-manager install,
   build). This is the asset path that already exists (`core/repo.rs` ~`:2390`).
5. `wp-config.php` is rewritten: `DB_NAME`, plus `WP_HOME`/`WP_SITEURL` when defined.
6. The DB is cloned (§2.4) and its URLs rehomed.

Shape A needs **site copy** (files + DB). Shape B reuses most of it.

**Shape B (second): a site whose own root is a git checkout** (`repo_site_info.present`).
That covers Laravel, Bedrock, Radicle, Blank PHP, Symfony… and a WordPress site whose whole
docroot is a repo. Here the worktree IS the docroot: a linked child (`docroot_managed = 0`)
at `Worktrees/<child-domain>/` BESIDE the sites folder (as built, W10 — §9 Q8), made by
`git -C <parent-root> worktree add`. The ignored
files it needs are copied or rebuilt as §2.5 says.

In both shapes the worktree folder is **never inside the parent's own tree**. A worktree
nested in its own repo would show up in the parent's `git status` and in the parent site's
served files.

### 2.2 Domain: `feature-x.shop.rex` first, `shop-feature-x.rex` as the fallback (owner, 9 Oct)

- **Default:** `<branch-slug>.<parent-domain>`, e.g. `feature-x.shop.rex`. It reads as
  "this branch of shop".
- **Fallback:** `<parent-label>-<branch-slug>.<tld>`, e.g. `shop-feature-x.rex`. It is used
  when ANY of these holds. The dialog shows the reason in one line.
  1. **The parent is a subdomain multisite.** nginx already serves `*.shop.rex` from the
     parent's block (`core/services.rs::server_block`). An exact-name child would win in
     nginx and silently shadow the network sub-site of that name, current or future.
  2. **The name is already taken.** That means a site domain, an alias (`site_domains`), a
     parent alias that carries a wildcard, or a sub-site domain in the parent's network.
  3. **The parent's own domain is already nested** (e.g. `a.shop.rex` → `a-fix.shop.rex`).
     Each worktree would otherwise add a level. Kept flat on purpose.
- If the fallback is also taken, `-2`, `-3`… are appended.
- `branch-slug`: the branch name lowercased, with `/` and anything outside `[a-z0-9-]`
  turned into `-`, then trimmed. It is a pure function with tests. `feature/checkout-v2` →
  `feature-checkout-v2`.
- **To prove (W2):**
  - the DNS agent answers a two-level name (`feature-x.shop.rex`) on all three OSes; the
    Windows NRPT rule is per-namespace, so it should, but this must be measured;
  - Caddy picks the child's exact-name leaf by SNI over any parent cert;
  - the per-site cert is issued for the exact name.
- The user can override the domain in the dialog. The default is the rule.

### 2.3 The row: a site that knows its parent

Migration v45 (as built in W1): a table of its own, `site_worktrees (site_id PK → sites
ON DELETE CASCADE, parent_id → sites with NO on-delete, shape asset|site, worktree_path,
adopted, created_at)`. It is not two columns on `sites`, because the relation is a fact
about two rows. Columns would also have to be threaded through ~40 `Site { … }` literals
for a field almost no site has. `parent_id`'s missing `ON DELETE` makes SQLite itself
refuse deleting a parent that has children. In Shape A that path is the asset dir inside the child; in
Shape B it is the docroot. The **branch is read live from git**
(`git -C <worktree_path> branch --show-current`) and never stored. `sites.git_ref` is
already documented as "chosen at creation, never kept in sync" (`records_asset_ref`,
`commands/repo.rs:1183`). A stored branch would lie the first time the user runs
`git switch` in the worktree.

- **Shape B** is `docroot_managed = 0` (linked), so the generic delete path physically
  cannot remove the folder. Removal goes only through §2.6.
- **Shape A** is `docroot_managed = 1`: the child's copied WordPress is rexenv's to delete.
  But it CONTAINS a worktree, and that worktree can hold uncommitted work. So teardown's
  `remove_dir_all` on a docroot is **refused while any `.git` FILE (a registered worktree)
  exists under it**. §2.6 removes the worktree through git first. This guard goes into the
  generic teardown, not only the worktree path. It also protects a user who ran
  `git worktree add` into a managed site by hand.
- PHP version, web server and Xdebug are copied from the parent at creation, then
  independent.
- Deleting a parent that has live children is refused, with the list of children ("remove
  these worktrees first"). A worktree whose main repo has vanished is a broken checkout.

### 2.4 The database — three choices, default **clone**

| Choice | What happens | When |
|---|---|---|
| **Clone parent DB** (default) | new DB `<parent_db>_<slug>` on the same engine, copied from the parent, then URLs rehomed (WP) or `.env` rewritten (Laravel) | branches whose migrations differ |
| **Fresh** | empty DB; Laravel runs `migrate --seed` (the existing `git_migrate` phase); WP runs install | throwaway branches, tests |
| **Share parent DB** | same `DB_NAME` | read-only UI branches; dialog says in one line that migrations here change the parent's data |

Same-server copy needs its **own small path** (`core/dbclone.rs`), because `dbdump::gate`
refuses rexenv's own server by design and must stay that way (its refusal is the Valet/Herd
import's guard against importing a site into itself). `dbclone` reuses `DefaultsFile`,
`record_provenance` (so `db_created` is set BEFORE `CREATE DATABASE` and teardown drops
only what rexenv made), `prepare_target`, `feed`, `verify_complete`. MySQL/MariaDB:
bundled `mysqldump --single-transaction | mysql`. PostgreSQL: `CREATE DATABASE … TEMPLATE`
needs no connections on the source — not true while a site runs — so `pg_dump | psql`.
SQLite (Laravel `database.sqlite`): file copy.

### 2.5 Untracked files a branch needs to run (Shape B)

Shape A does not need this section: the copied parent already has config, uploads and
other assets, and only the asset's own deps are rebuilt (§2.1 step 4). In Shape B,
`git worktree add` gives tracked files only. A running site also needs what `.gitignore`
hides: `.env`, `wp-config.php` (WP repos), `vendor/`, `node_modules/`, built assets,
`storage/` (Laravel), `wp-content/uploads/`.

- **Config files** (`.env`, `wp-config.php`, `auth.json`, `.env.local`): copied from the
  parent, then rewritten for the child's domain + DB (`laravel::wire_env`; for
  `wp-config.php` only the `DB_NAME` define, by the same parser the config-rewrite code
  uses). A fixed, named list — not "every ignored file".
- **Dependencies**: the git-site provisioning phases (`composer install`, package-manager
  install, asset build) run in the worktree — the same, already reviewed path. An opt-in
  "copy vendor/ from parent" fast path is §9 Q3.
- **Uploads / storage**: copied with `clone_tree`, never symlinked — Windows symlinks need
  Developer Mode or admin, and a symlink would let a child's media delete reach the
  parent's files. A size line in the dialog ("copies 2.3 GB of uploads") with a "skip
  uploads" checkbox.

### 2.6 Removing a worktree — the only path that deletes files

`git -C <parent-root> worktree remove <path>` — git itself refuses when the worktree has
uncommitted or untracked changes; rexenv shows git's refusal and offers **Force** only
behind a confirmation that lists the dirty files (`git status --porcelain=v2`, already
parsed by `parse_status_v2`). Order:

1. stop serving (vhost out, reload — the existing linked-site teardown),
2. drop the child DB **only if** `db_created = 1` on the child (never the shared parent DB),
3. `git worktree remove` (git deletes the folder, including our copied ignored files —
   force needed there, so step 3 runs with `--force` ONLY after step 0 below proved the
   ignored files present are exactly the ones rexenv copied, or the user confirmed),
4. delete the row.

**Shape A** runs the same steps 0–3, with `<path>` being the asset dir inside the child.
Only after git has removed it, and the `.git`-file guard (§2.3) confirms no worktree is
left under the docroot, does the normal managed teardown `remove_dir_all` the child's
copied WordPress. If git refuses, nothing else is deleted: the site stays up and the row
stays, so the user can commit and Retry.

Step 0: before any of this, list dirty tracked/untracked (non-ignored) files. Ignored
files are expected (vendor, .env). Branch is **never** deleted — that is the user's call
in git, not ours. Windows: a process holding a file in the folder (php-cgi, an editor,
`node` watcher) makes removal fail with "Access is denied"; the error names the folder and
says what usually holds it, and the row stays so Retry works.

### 2.7 Worktrees made outside rexenv (adopt)

`git -C <parent-root> worktree list --porcelain` shows every worktree of the repo,
including ones Claude Code / another tool / the user made. The Worktrees panel lists them
with a **Serve** button: that creates the linked child row + DB + config for an existing
worktree folder (no `git worktree add`). On removal of an adopted worktree rexenv **never**
runs `git worktree remove` unless the user asks — it just stops serving, because the tool
that made it owns its lifecycle. Folders that vanished → `git worktree prune` offered, row
marked missing.

### 2.8 Surfaces

- **SiteDetail → Repository tab → "Worktrees" section** (under `RepoPanel`, in
  `SiteRepoTab.tsx`): list (branch, domain, dirty count, ahead/behind of base, DB mode),
  New worktree (RefPicker: existing branch, or new branch from base), Open, Serve
  (adopt), Remove. Child sites appear in the Sites list grouped under the parent with a
  branch chip.
- **Child SiteDetail**: a banner "worktree of `shop.rex` on `feature-x`", plus
  **Re-clone DB from parent** (destructive to the child DB; confirmation).
- **CLI**: `rex worktree <domain> list | add <branch> [--new --from <base>] [--db clone|fresh|share] [--skip-uploads] | serve <path> | remove <child-domain> [--force]`.
- **MCP**: `worktree` action on the `repo` tool — `list` (Read), `add`/`serve` (Changes),
  `remove` (Full — deletes files). An agent's own worktree can be served in one call:
  `serve {path}` where path is its cwd; the parent is found by `git rev-parse
  --git-common-dir` matched against site roots.

## 3. Three platforms (`docs/PLATFORMS.md` §4)

Common, in `core/` once: everything above — git is the system binary through the existing
`resolve_git`, DB clone uses the bundled clients, file copy is `clone_tree`.

Per OS (no new trait expected; if one appears it goes in `platform/traits.rs` with all
three filled):
- **macOS** — none known. Check: `git worktree` with Xcode CLT git (2.39+) — fine.
- **Windows** — paths: worktree folder path length (`core.longpaths` is not ours to set —
  surface git's error); file locks during remove (§2.6); `\` vs `/` in `worktree list
  --porcelain` output (git prints `/` — a test asserts the parser takes both). Git for
  Windows ≥ 2.17 needed for `worktree remove` — `git_preflight` gains a version floor with
  the sentence in `words.rs`.
- **Linux** — none known; distro git on 22.04 is 2.34.

Proof: the L1 example (§7) runs on all three (macOS host, the Dell for Windows, the UTM VM
for Linux); the ledger rows say which OS the proof came from.

## 4. Invariants (each becomes a ledger row in the commit that writes its comment)

1. No rexenv code path `remove_dir_all`s a worktree; only `git worktree remove` deletes one.
   Shape B children are `docroot_managed = 0`. Shape A's managed teardown refuses while any
   `.git` FILE exists under the docroot, and that guard sits in the generic teardown (§2.3).
2. The child's DB is dropped only when the CHILD row has `db_created = 1`; a `share` child
   can never drop the parent's DB.
3. `dbdump::gate`'s self-import refusal is untouched; `dbclone` is the only same-server
   copy and it refuses a target name that already exists.
4. Branch is never stored as truth — always read from git.
5. An adopted worktree is never `git worktree remove`d without an explicit user action.
6. A parent with children cannot be deleted.
7. A worktree child never takes a name under a subdomain-multisite parent, or a name an
   existing site, alias or network sub-site answers to (§2.2 fallback).

## 5. What is out of scope

- Merging branches, opening PRs, rebasing — the user's git client does that.
- Syncing DB changes between child and parent (a migration diff tool is its own feature).
- Worktrees of a repo no rexenv site is rooted in (no parent site → nothing to clone).

## 6. Risks

- **Disk**: N worktrees × (vendor + node_modules + uploads + DB). The dialog shows the
  estimate; the Worktrees list shows each child's size.
- **Long first start**: composer + npm per worktree takes minutes — the job card already
  streams it; §9 Q3's copy-vendor fast path is the mitigation.
- **Agents creating many worktrees**: MCP `add` counts against the same per-client cap the
  scratch sites use.

## 7. Testing

- L0: slug + domain derivation, porcelain parser (both separators), the remove-order state
  machine, the "dirty files" classifier (ignored vs not), `dbclone` refusing an existing
  target, delete-parent-with-children refusal.
- L1 example `worktree_site_check` (sandbox tier, fixture-owned repo + sites dir via
  `common::sandbox()`): add → serves HTTP 200 on the child domain with its own DB → write
  a row in the child DB, parent unchanged → remove (dirty refused, clean succeeds, folder
  gone, DB dropped, parent DB intact).
- SMOKE: a section per OS for the GUI flow, plus the MCP leg (an agent serves its own
  worktree).

## 8. Task list

| # | Task | Done when |
|---|---|---|
| W0 | Owner answers §9; plan updated | ✓ 9 Oct 2026 (Q1, Q4 answered; Q2, Q3, Q5 take the recommendation unless the owner says otherwise) |
| W1 | Migration v45 `site_worktrees`; `SiteWorktree` model + store; delete-parent-with-children refusal; the `.git`-file teardown guard | ✓ 9 Oct 2026 — `core/worktree.rs` (`foreign_checkout_under`), `sites::delete_preflight` (called at `delete_site_owned` step 0) + teardown's keep; 6 lib tests, 5 plants each caught; ledger #814, #815 (◐ macOS only, the other OSes' run is W11) |
| W2 | `core/worktree.rs`: `list` (porcelain parse), `add` (existing/new branch), slug + domain derivation with the fallback rules, a git version floor | ✓ 9 Oct 2026 — `derive_domain` (three fallback reasons; a fourth, length, was unreachable and was removed), `domain_taken` (the nginx `server_name` set), `parse_porcelain`/`list`/`add`, `require_worktree_git` (≥ 2.17, the hint is `words.git_install`, so no new words). L0 tests + L1 `worktree_git_check` on macOS; ledger #816. Two-level names measured on macOS only (`zz-probe.shop.rex` → 127.0.0.1); Windows/Linux resolution and the per-name leaf are proven in W5's HTTPS leg and W11 |
| W3 | `core/dbclone.rs` (MySQL/MariaDB, PostgreSQL) on top of the `dbrestore` pieces | ✓ 9 Oct 2026 — `clone_database` + `DbEngine::{database_exists, list_tables, dump_to_file}` (the export now calls `dump_to_file`); L1 `db_clone_check` green on MySQL and PostgreSQL; a plant on the not-ours refusal caught; ledger #817. SQLite (Laravel `database.sqlite`) moved to W10, the only shape that has it |
| W4 | **Site copy** (Shape A base): `copy_site_tree` of the parent docroot minus one asset dir, `wp-config.php` rewrite, DB clone + rehome, as a `site_provision` job | ✓ 9 Oct 2026 — `drive_worktree_copy` (copy → db → db_copy → configure → urls), `start_with`'s `AfterInsert` records the relation with the row; v45 gained the REQUEST columns (branch, base, asset, skip_uploads) before it shipped, so Retry knows what was asked; refused for now: subdomain networks, Bedrock layouts; ledger #819. Building it found #818 (a doubled URL prefix in every rehome to a name containing the old one, Change domain included) |
| W5 | **Shape A**: plugin/theme worktree on top of W4 (`worktree add` into the copy, the asset's deps) | ✓ 9 Oct 2026 (job half) — the `worktree` + `deps` phases; `commands::worktree::{plan_child, start}`. L1 `worktree_site_check` ALL PASS on macOS: the child on `feature/probe`, the parent on `main`, own DB, URLs moved, both delete preflights refuse. The UI entry (the plugin/theme row) moved to W8 with the rest of the UI |
| W6 | Remove flow (§2.6), both shapes: dirty-file confirmation, git-first then the ordinary delete | ✓ 9 Oct 2026 — `release_for_delete` at `delete_site_owned` step 0 (so the Sites list's own Delete does it), `worktree_remove(id, force)` for "remove anyway"; the order became git FIRST (a refusal leaves everything intact), not the §2.6 draft's serve-off → DB → git. L1 legs 6–8 ALL PASS, plant caught; ledger #820. Windows' file-lock wording waits for a Windows run (W11) |
| W7 | Adopt / Serve existing worktrees + prune (§2.7) | ✓ 10 Oct 2026 (Shape B only, §9 Q6) — `worktree_adoptable` + `worktree_serve` (linked, `adopted`, named after the live branch), the "made elsewhere" list under the Repository tab's panel; delete leaves the folder. L1 legs 14–15, plant caught; ledger #830. Prune and the CLI/MCP arms ✓ 10 Oct 2026 (#849): "Clean up missing (n)" on the made-elsewhere list, `rex worktree … adoptable/serve/prune/reclone-db`, the MCP `worktree` tool's adoptable/serve/prune/reclone |
| W8 | UI: Worktrees section (asset row for Shape A, Repository tab for Shape B), child banner, Sites-list grouping, Re-clone DB | ✓ 10 Oct 2026 (Shape A) — `AssetWorktrees` under a git plugin/theme's repo panel (list, New-worktree dialog with live preview, Remove with a second yes for dirty), `WorktreeOfLine` on the child's header, a branch chip on the Sites row; six commands registered (parity: five `Gap` → W9, `worktree_relations` `Never`). `wk-checks/worktree.js` green, plant caught; ledger #821; SMOKE § Git worktree sites. **Not built**: Re-clone DB from parent (§9 question), Sites-list GROUPING under the parent (a chip only), Shape B's Repository-tab entry (W10) |
| W9 | CLI `rex worktree …` + MCP `worktree` tool + dial levels | ✓ 10 Oct 2026 — `rex worktree <domain> list \| add <dir> <branch> [--theme --from --domain --skip-uploads] \| remove [--force]` (arms `worktree.list/add/remove`); the MCP tool is its OWN tool `worktree` (not an action on `repo`, whose whole surface is `run`): list/preview Read, create Changes, remove Full and worktree-only. L0 + a plant; ledger #822; parity: three called by name, `worktree_create`/`worktree_of` → `Tool("worktree")`. A live `rex` / real-MCP-client run is a SMOKE row |
| W10 | **Shape B**: whole-site repo as a linked child (§2.5 ignored files, lockfile-identical vendor copy) | ✓ 10 Oct 2026 — `start` adds the worktree BEFORE the row at `Worktrees/<domain>` BESIDE the sites folder (the linked-folder rule refuses inside it — found by the first L1 run; §9 Q8); `drive_worktree_site` (config → deps → db → db_copy → urls); the request's asset became optional (CLI `add <branch>`, MCP `kind: "site"`, the Repository tab's panel). L1 `worktree_site_check` legs 9–12 ALL PASS (WordPress parent); ledger #823. Laravel proven live 10 Oct 2026 by `worktree_laravel_check` (the `.env` rewrite, the vendor copy, the database, the delete — plant caught); the Composer-WordPress `.env` leg is still unproven live |
| W11 | Per-OS proof: L1 on macOS, Windows (Dell), Linux (UTM VM); SMOKE sections ×3; ARCHITECTURE/MAP/TESTING updated | ✓ (automated layers) 10 Oct 2026 — **every live check of this feature ran green on all three OSes**: `worktree_git_check`, `db_clone_check` (MySQL + PostgreSQL), `worktree_site_check` (12 legs, both shapes) on macOS, the Dell (Win10) and the Ubuntu 22.04 VM; the worktree lib tests on all three (Windows via an xwin-built test binary, Linux in the check container). The Windows runs found three things: rexenv's own mu-plugins read as uncommitted work (a product bug, fixed, #820); two Mac-shaped tests; the comctl32 manifest an example needs. `worktree_laravel_check` ran on macOS only. **Owed: the GUI rows of SMOKE § Git worktree sites, on all three** (a person, or the VMs' GUI tooling) |

## 9. Owner's answers (9 Oct 2026) and what is still open

1. **Domain** — ANSWERED: `feature-x.shop.rex` first, `shop-feature-x.rex` when it would
   cause a problem. §2.2 lists the three rules that decide "a problem".
2. **Default DB mode** — not asked yet; the plan goes with **clone**.
3. **Dependencies (Shape B)** — not asked yet; the plan copies `vendor/`/`node_modules`
   when the lockfiles are byte-identical to the parent's, and installs otherwise. No knob.
4. **Priority** — ANSWERED: the WP plugin/theme shape first (Shape A, W4–W5). The
   whole-site repo is W10.
5. **Placement** — not asked yet. Shape A's entry point is the plugin/theme row in the
   WordPress tab, where the repo lives. Shape B's is the Repository tab.


6. **W7 — serving a plugin worktree another tool made** (asked 10 Oct 2026, blocks W7). A
   Shape A child needs the plugin folder INSIDE the copied site, but a worktree Claude Code or
   the user made lives wherever they put it. The three ways:
   - (a) a symlink from the copy's `plugins/<dir>` to that folder. WordPress follows it, but
     Windows needs Developer Mode or admin to create one, and `plugins_url()` can resolve
     through the link oddly.
   - (b) offer `git worktree move` into the copy. This moves the other tool's folder out from
     under it.
   - (c) do not adopt Shape A at all, and only offer adopt for Shape B (a whole-site repo,
     where the worktree IS the docroot, so a linked site serves it in place).

   **DECIDED 10 Oct 2026 (owner: "suggest"; agent chose c):** adopt Shape B only — the whole-site
   worktree another tool made (Claude Code's `.claude/worktrees/<name>`, a plain `git worktree
   add`) is served as a linked child; a plugin worktree made elsewhere is not adopted.
7. **"Re-clone DB from parent"** — **DECIDED 10 Oct 2026 (owner: build it if there is a use):**
   there is (a child whose data drifted, wanted fresh from the parent): a button on the child,
   confirm, then `dbclone` + rehome again. **Built the same day** (`worktree_reclone_db`, the
   child header's "Re-clone DB from parent"; L1 leg 13; #831).
8. **Where Shape B worktree folders live** (decided by the agent 10 Oct 2026, reversible). They are
   at `Worktrees/<domain>` BESIDE the sites folder (`~/rexenv/Worktrees/…`), because the
   linked-folder rule refuses anything inside the sites folder. The alternative is an exception
   to that rule for worktree children, which keeps them in `Sites/` but makes the rule
   conditional. **DECIDED 10 Oct 2026 (owner): fine as is.**