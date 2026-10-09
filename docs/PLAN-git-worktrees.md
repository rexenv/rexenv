# Git worktree workflow — one branch, one running site

**Status:** PLANNED 9 Oct 2026 — not started. Owner request ("Git worktree workflow"); this
plan was written by the agent while the owner was away, so every decision in §2 is a
recommendation waiting for the owner's yes (the open questions are §9). Planned against
`00bc4dd1` (v0.8.13). Task list: §8, mirrored as one row in `docs/TODO.md`.

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

## 2. The decisions (recommendations — owner confirms, §9)

### 2.1 Which sites can have worktrees

**v1: a site whose own root is a git checkout** (`repo_site_info.present`): Laravel,
Bedrock, Radicle, Blank PHP, Symfony… and a WordPress site whose whole docroot is a repo.

**v2 (Stage 4): a plugin or theme checkout inside a WordPress site** — the far more common
WordPress shape (the repo is `wp-content/plugins/my-plugin`, the site is not in git). The
child site there is "the parent site, copied, with that ONE plugin directory replaced by a
worktree of the plugin's repo". It needs the site-copy machinery v1 builds, so it is
staged after it, not beside it.

### 2.2 Where the worktree lives, and its domain

- Folder: `<sites_dir>/<parent-name>--<branch-slug>/` — inside the Sites folder (same
  volume, visible to the user, the place they already look), never inside the parent's
  checkout (a worktree nested in its own repo shows up in the parent's `git status` and in
  the parent site's served tree).
- Domain: `<parent-name>-<branch-slug>.<tld>` (e.g. `shop-feature-x.rex`). **Not**
  `feature-x.shop.rex`: a subdomain-multisite parent already owns `*.shop.rex`, and an
  alias of the parent could own it too. Collisions with an existing domain get `-2`, `-3`.
- `branch-slug` = branch name lowercased, `/` and non-`[a-z0-9-]` → `-`, trimmed to keep
  the label ≤ 63 bytes (DNS limit) — a pure function with tests.
- The user can override folder and domain in the dialog; the defaults are the rule.

### 2.3 The row: a linked site that knows its parent

Migration: `sites.worktree_of INTEGER NULL REFERENCES sites(id)`. Nothing else — the
**branch is read live from git** (`git -C <path> branch --show-current`), never stored:
`sites.git_ref` is already documented as "chosen at creation, never kept in sync"
(`records_asset_ref`, `commands/repo.rs:1183`), and a stored branch would lie the first
time the user runs `git switch` in the worktree.

- `docroot_managed = 0` (linked) — so the generic delete path physically cannot remove the
  folder. Removal goes only through §2.6.
- PHP version, web server, Xdebug: copied from the parent at creation, then independent.
- Deleting a parent with live children is refused with the list of children ("remove these
  worktrees first") — a worktree whose main repo vanished is a broken checkout.

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

### 2.5 Untracked files a branch needs to run

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

1. A worktree child is always `docroot_managed = 0` — no rexenv code path `remove_dir_all`s
   a worktree; only `git worktree remove` deletes one.
2. The child's DB is dropped only when the CHILD row has `db_created = 1`; a `share` child
   can never drop the parent's DB.
3. `dbdump::gate`'s self-import refusal is untouched; `dbclone` is the only same-server
   copy and it refuses a target name that already exists.
4. Branch is never stored as truth — always read from git.
5. An adopted worktree is never `git worktree remove`d without an explicit user action.
6. A parent with children cannot be deleted.

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
| W0 | Owner answers §9; plan updated | §9 has answers |
| W1 | Migration `worktree_of`; `Site` model; delete-parent-with-children refusal | L0 tests; ledger #6 |
| W2 | `core/worktree.rs`: `list` (porcelain parse), `add` (existing/new branch), `slug`/domain derivation, git version floor in `git_preflight` + `words.rs` ×3 | L0 tests on all parsers |
| W3 | `core/dbclone.rs` (MySQL/MariaDB, PostgreSQL, SQLite) on top of `dbrestore` pieces | L0 + L1 copy on a sandbox server; ledger #2, #3 |
| W4 | Child provisioning job: linked row → config copy + rewrite → deps phases → DB (clone/fresh/share) → URL rehome → serve; reuses `site_provision` card | L1 `worktree_site_check` add leg green |
| W5 | Remove flow (§2.6) incl. dirty-file confirmation and Windows lock message | L1 remove leg green; ledger #1 |
| W6 | Adopt / Serve existing worktrees + prune (§2.7) | L1 leg: a worktree made by plain `git worktree add` gets served; removal leaves its folder; ledger #5 |
| W7 | UI: Worktrees section, child banner, Sites-list grouping, Re-clone DB | Playwright WebKit pass; DESIGN.md rules kept |
| W8 | CLI `rex worktree …` + MCP `repo` `worktree` action + dial levels | `cli` tests; CLI-ROADMAP.md; MCP parity test |
| W9 | Stage 4: plugin/theme worktree inside a WP site (§2.1 v2) | own L1 leg; separate sign-off |
| W10 | Per-OS proof: L1 on macOS, Windows (Dell), Linux (UTM VM); SMOKE sections ×3; ARCHITECTURE/MAP/TESTING updated | ledger rows name the OS of each proof |

## 9. Open questions for the owner

1. **Domain shape** — `shop-feature-x.rex` (recommended, §2.2) or `feature-x.shop.rex`
   (prettier, collides with subdomain multisite)?
2. **Default DB mode** — clone (recommended) or ask every time?
3. **Dependencies** — always run `composer install`/npm in the worktree (correct, slow), or
   offer "copy vendor/node_modules from parent" (fast, wrong when the branch changed
   `composer.lock`)? Recommendation: copy when the lockfiles are byte-identical to the
   parent's, install otherwise — automatic, no knob.
4. **Stage 4 priority** — is the plugin/theme-repo shape (most WordPress devs) more
   urgent than the whole-site-repo shape? If yes, swap W4–W6 and W9 order.
5. **Placement** — Worktrees as a section in the Repository tab (recommended), or its own tab?
