# Create a site FROM a Git repository

**Status: SHIPPED 11 Aug 2026, Stages 1–4** (`de07ed0` → HEAD), planned the same day
against `62a9f9e`. One open caveat, narrowed 21 Aug 2026: **Radicle** is unverified
against a real project. **Bedrock was verified the day it shipped — and was broken**
(`wp core install` was pinned to the docroot while Composer puts core in `web/wp`, so
every wp-cli call answered "This does not seem to be a WordPress installation"); fixed
via `wordpress::core_root`, ledger #294 + #299, and `git_site_provision_check` case 4
now installs WordPress and lands 12 tables.

Machine-verified (745 lib tests + `git_site_clone_check` on the sandbox tier); the
packaged-app half — real event streaming into WKWebView, a real remote, your own SSH
agent, and the non-fatal asset build — is `docs/SMOKE-TEST.md` § "a Laravel site FROM a
git repository" and has **not** been run yet. Questions 4–5 in §5 remain open.

Goal, in the user's words: *"Laravel developers keep their projects in git — rexenv must
let them paste a repo URL and get a working local site."* Today the Laravel card can only
make a **new** app (`composer create-project`); a developer with an existing repo has to
clone by hand, then link the folder, then wire `.env` themselves.

> **The one-sentence design:** a git site is the **Laravel card with a different source
> for the code** — everything after the code lands (database, `.env`, serve) is the
> provisioning path that already exists, and everything before it is the clone machinery
> that already exists. This plan is mostly about the seam between them.

---

## 1. What already exists (read this before designing anything)

Nearly every piece is built and shipped. Listing them here because the expensive mistake
would be writing a second clone path beside the reviewed one.

| Need | Already in the tree | Notes |
|---|---|---|
| Parse any pasted repo reference | `core/repo.rs::parse_source` | https / ssh:// / `git@host:path` / `owner/repo` / forge `/tree/<ref>` URLs; refuses `git://`, archives, whitespace |
| Validate URL + auth **before** any clone | `core/repo.rs::probe_remote` + `commands/repo.rs::repo_probe` | `git ls-remote`, 30s cap; **already site-independent** — the New Site dialog can call it with no site row in existence |
| Branch/tag picker UI | `src/components/wordpress/RefPicker.tsx` | searchable combobox, grouped, portaled — used at ALL THREE ref choices (New Site's From-Git, the add-plugin/theme Fetch, and the Repository panel's Checkout + Restore). The Fetch one was a plain `<select>` until 14 Aug 2026: fine against a three-branch fixture, useless against the remote list a real project answers with |
| Streamed, cancellable clone | `core/repo.rs::clone_repo` | `--no-recurse-submodules` (CVE-2024-32002 class), `protocol.ext.allow=never`, `GIT_TERMINAL_PROMPT=0`, process-group cancel, removes only the dir it created |
| Honest git error mapping | `core/repo.rs::map_git_error` | auth / not-found / no-such-ref, each with a `$`-prefixed fix line |
| Read-only repo inspection | `core/repo.rs::inspect_repo` | composer.json, package manager (`packageManager` > lockfile > npm), build script, `.nvmrc`/engines |
| `composer install` on the **site's** PHP | `core/repo.rs::composer_install` | pinned phar, never a system `composer` |
| Framework detection from the filesystem | `core/sites.rs::detect_project` | Laravel/Bedrock/Radicle/Craft/Statamic/Symfony/Magento + generic front controllers; **executes nothing** |
| Laravel knowledge | `core/laravel.rs` | `is_installed`, `artisan`, `env_path`, `wire_env` (the `.env` rewriter, unit-tested against a REAL generated `.env`) |
| Streamed provisioning card + phases | `commands/site_provision.rs` + `SiteProvisionCard.tsx` | weighted honest progress, cancel, per-job log, `provisioned=0` on death, Retry |
| Docroot ≠ project root | `sites.docroot_subdir` (v32) + `Site::served_root` | the reason `.env` is not a public URL on a Laravel site |
| `.git` / `.env` are 404 | dotfile guard in all three vhost templates | nginx regex location, Apache `R=404`, FrankenPHP two-matcher; `/.well-known/` exempt |

**What does NOT exist:** a way to make the *site's own docroot* come from a repo. Every
git path in the tree today targets `wp-content/{plugins,themes}/<dir>`.

---

## 2. The decisions

### 2.1 Source, not type — "From Git" is a third way to fill a docroot

A site already has two sources: rexenv makes the folder (`NewSite.path == ""`), or the
user points at one (`path` non-empty → linked, `docroot_managed = 0`). Git is the third:
**rexenv makes the folder and fills it from a remote.**

So `NewSite` grows one field, not a new type:

```rust
pub struct NewSite {
    …
    pub path: String,          // non-empty → LINK an existing folder
    #[serde(default)]
    pub git_url: String,       // non-empty → CLONE into a folder we create
    #[serde(default)]
    pub git_ref: Option<String>,
}
```

`git_url` and `path` are **mutually exclusive** — both set is a caller bug, refused in
`provision_with` before anything is created. (Cloning *into* someone's existing folder is
a different, much more dangerous feature; it is explicitly not this one.)

Rejected alternative — a fourth type card ("From Git") in step 1: the type still has to
be known (it decides the phases, the binary plan, whether a database is downloaded), so a
"From Git" card would immediately ask "…of what?" and be a step that buys nothing.

### 2.2 The type is CHOSEN, then VERIFIED — never guessed after the fact

`ls-remote` can list refs; it cannot list files. So at plan time we do not know what is in
the repo, and the phase list + the ~600 MB database download must be decided before the
clone starts.

Therefore: **the user picks Laravel (as they do today) and the clone is verified against
that choice.** After the clone, `sites::detect_project(project_root)` runs (pure fs, zero
execution) and:

- Laravel shape found → continue; `docroot_subdir` is taken from the detection
  (`public`), not from a constant, so a repo with an unusual layout is served correctly.
- Anything else → the `clone` phase **fails with a specific message** naming what was
  found ("this repo looks like WordPress (Bedrock), not Laravel — create it as a
  WordPress site instead"), the site sits at `provisioned = 0`, Retry/Delete are there.

This is the honest option. Silently re-typing the site would change the database plan and
the phase list *after* they were fixed, and the card would then be describing a job it is
not running.

**Which types may be cloned (settled during Stage 1, was open question 3).** Laravel and
Blank PHP. **WordPress is refused** — not because it is hard, but because a WordPress
checkout without its *database* is not a site: no posts, no options, no users. "Cloned
successfully" would hand back something that cannot serve a page. It belongs behind the
database-import work (`docs/PLAN-valet-herd-db-import.md`), not beside it, so
`validate_git_source` says so with the two ways out (clone it as Blank PHP; or clone it
yourself and link the folder). Refusing outright rather than half-supporting it also
keeps the phase list and the blueprint guard from having to describe a shape the product
does not have — the two are asserted equal by
`the_guard_admits_exactly_the_shapes_phase_defs_gives_a_blueprint_phase`.

### 2.3 Cloning into a docroot that already exists: staging + `rename`

`clone_repo` refuses an existing `dest` — deliberately, so it can safely remove a partial
checkout on failure without ever deleting a directory it did not create. That guard is
load-bearing and stays untouched.

But `provision_with` creates `Sites/<domain>` during prepare (before the job is visible),
and a Retry re-creates it. So the clone phase:

```
staging = <sites_dir>/.rexenv-clone-<domain>-<8 hex>      # same filesystem ⇒ rename is atomic
clone_repo(dest = staging)                                 # dest absent: existing guard satisfied
std::fs::remove_dir(&docroot)?                             # OS refuses if NON-EMPTY — structural
std::fs::rename(&staging, &docroot)?
```

`remove_dir` (not `remove_dir_all`) is the whole safety argument: the operating system
itself refuses to remove a non-empty directory, so this can never delete a user's files
even if every assumption above is wrong. A non-empty docroot therefore surfaces as an
honest error ("`<path>` is not empty — rexenv only clones into a folder it just created")
and the staging dir is removed. *(This is the `example-cleanup-scope` lesson applied at
the product level: don't reason about which paths are safe to delete — pick a call that
cannot delete the wrong thing.)*

Staging lives **inside the sites dir** and not in app-data on purpose: the Sites folder is
user-configurable and may be on another volume, where `rename` would fail with `EXDEV`
after a multi-minute clone. The dot-prefixed name keeps it out of Finder and out of the
sites-folder listing; it is removed on every exit path (ok, failure, cancel).

### 2.4 Idempotent Retry: the clone phase skips a checkout that is already there

Retry re-enters every phase. `clone` skips when `docroot/.git` exists (mirroring
`app_install`'s `laravel::is_installed` skip), so a failure in `composer install` does not
re-download the repository — and cannot, because the docroot is no longer empty.

### 2.5 Order of operations: `.env` before `composer install`

Not obvious, and worth recording because it is easy to "fix" the wrong way:

`composer install` on a Laravel repo runs `post-autoload-dump: @php artisan
package:discover`, which **boots the application**. If `.env` is not there yet, that boot
runs on defaults — including Laravel's SQLite default — and any package that touches
config during discovery sees a configuration that will never exist again. Writing `.env`
first costs nothing and makes every subsequent boot see the real thing.

`composer create-project`'s own `post-root-package-install` script (`copy('.env.example',
'.env')`) does **not** run on a plain `composer install` — that hook fires only for
`create-project`. So rexenv does the copy itself.

Full sequence for a git-sourced Laravel site:

| # | Phase key | What it does |
|---|---|---|
| 1 | `prepare` | resolver, cert, docroot, row (runs INLINE, before the job is visible) |
| 2 | `fetch` | binary plan: site PHP, composer phar, database engine |
| 3 | `clone` | staging clone → verify shape → `remove_dir` + `rename` → record `docroot_subdir` |
| 4 | `db` | spawn the engine, await ready |
| 5 | `configure` | `CREATE DATABASE`, `.env.example` → `.env`, `wire_env` (APP_URL + DB_*) |
| 6 | `deps` | `composer install` (pinned phar on the site's PHP) |
| 7 | `finalize` | `artisan key:generate --force`, `artisan migrate --force` |
| 8 | `serve` | pool + edge reload + await ready |

Eight phases — exactly the count the WordPress path already renders, so the card needs no
layout change.

`PROVISION_PHASE_WEIGHTS` gains `clone` 20, `deps` 30, `finalize` 5 (unknown keys already
default to 5; naming them is what keeps the bar from racing through the two network
phases and then parking).

**`finalize` was added to the NEW-app Laravel path too**, which is a visible change to a
shipped flow and deliberate: migrations used to be a silent tail of `configure`, and the
cloned path needs them *after* its own dependency step. One phase key, two labels
("running migrations" / "app key + migrations"), one implementation — the alternative was
the same fifteen lines, and the paragraph of reasoning that makes them readable, copied
into both branches. A card that names the step it is on is also the better half.

**Migrations run.** Same argument as the new-app path already records: the database this
site advertises must not be empty while the app's tables live somewhere else. A freshly
created, empty database makes `migrate --force` a safe operation — there is nothing to
lose. If the repo's migrations fail, that is the *repo's* state and the failure is shown
verbatim; the site is left at `provisioned = 0` with a Retry.

### 2.6 No repo code runs without consent — the consent just moves earlier

The standing rule (`ARCHITECTURE` §9, "Repo scripts never run implicitly") is that
clone+detect executes nothing and each install/build is one explicit click. A one-shot
"create my site from this repo" flow cannot be click-by-click without being a worse
product, so the consent moves to **before** the job instead of disappearing:

The dialog shows, above the Create button, exactly what will run — with the same
disclosure sentence the Git add panel uses:

> Creating this site runs the repository's own code: `composer install` (which runs the
> project's Composer scripts), then `php artisan key:generate` and `php artisan migrate`.

`migrate` gets a checkbox (default ON, with "the database is brand new and empty" as the
reason shown). `composer install` has no checkbox: a Laravel app without `vendor/` cannot
boot, so offering to skip it would be offering to create a site that 500s.

### 2.7 Agents may not clone

`Ownership::Agent` (MCP-created scratch sites) refuses `git_url` outright. Downloading and
then **executing** third-party code chosen by a model, on the user's machine, with no
click, is not something the scratch-site capability tier covers. `Ownership::User` (app +
`rex` CLI) is the only source. → CLAIM-LEDGER row.

### 2.8 Provenance lives on the `sites` row (v33), not in `site_git_assets`

```sql
-- v33
ALTER TABLE sites ADD COLUMN git_url TEXT;
ALTER TABLE sites ADD COLUMN git_ref TEXT;
```

Nullable; NULL = not from git, which is **exact** for every pre-v33 row.

**Written at the INSERT, not when the checkout lands** (revised during Stage 1 — the
first draft recorded it after the clone, on the reasoning that a site should never
advertise a repo it does not hold). Retry is this design's recovery path: a job that dies
mid-clone leaves `provisioned = 0`, and the Retry button — possibly after an app restart,
with the in-memory job registry long gone — has nowhere else to learn which repository to
fetch. So the row states the site's **source**, and `provisioned` states whether the code
actually arrived. Two fields, two facts; the first alone was never the whole answer.

Rejected: a `site_git_assets` row with `kind='site'`. That table is keyed
`(site_id, kind, dir_name)` and its whole semantics are about a folder under wp-content —
`asset_dest` refuses unknown kinds, `derive_dir_name` refuses the empty name, the badge
renders in the plugin/theme list, and the unlink-delete guard stats the resolved asset
path. Bending all four to carry a fact about the site itself trades one honest column for
four dishonest special cases. The site row is also what the delete/teardown path already
reads.

### 2.9 Private repos: the user's own keys, exactly as today

No token storage, no credential UI. The clone rides the cached login-shell env
(`ShellRunner::login_shell_env`) so the developer's ssh-agent and `~/.ssh/config` apply,
and `GIT_TERMINAL_PROMPT=0` turns a hidden credential prompt into a fast, mapped error
instead of a frozen job. An https URL to a private repo therefore fails at the **probe**,
before a site row exists, with the existing message that points at the `git@` form.

### 2.10 `.git` and `.env` are below the served root — and the guard still matters

For Laravel the clone puts `.git/` and `.env` at the project root, one level **above**
`public/`, which is all the web server can ever reach. That is defence in depth on top of
the dotfile guard, not a replacement for it: Stage 4 (any-PHP repo) will produce sites
whose docroot IS the project root, and a tunnel republishes a docroot to the internet.
The guard stays the primary control; this plan adds a second one for the Laravel case.

---

## 3. Stage 1 — the build (this is what ships first)

Small commits, one task each, `scripts/verify.sh` green before every one.

1. **v33 migration + model/store** — `sites.git_url`, `sites.git_ref`, `Site` fields,
   `store::create_site` insert, a `set_site_docroot_subdir` setter (the column exists
   since v32 but nothing can write it after create), the v33 upgrade-path test beside its
   neighbours in `db.rs`.
2. **`NewSite.git_url`/`git_ref` + the mutual-exclusion refusal** in `provision_with`,
   plus the `Ownership::Agent` refusal. Unit tests for both.
3. **`core::laravel::env_from_example`** — copy `.env.example` → `.env`, never overwrite
   an existing `.env`, honest error when neither exists. Unit-tested.
4. **`core::sites::clone_into_docroot`** — the staging + `remove_dir` + `rename` dance,
   with the shape verification. Unit tests for the name derivation and the
   non-empty-docroot refusal; the network half is the live check.
5. **`phase_defs` + weights + the `clone`/`deps`/`finalize` phases** in
   `commands/site_provision.rs`. Unit test: a git Laravel site's phase list, and that a
   linked site never gets a clone phase.
6. **Frontend** — `NewSiteInput.gitUrl`/`gitRef` in `src/types`, the source selector +
   URL + Fetch + `RefPicker` + disclosure block in `NewSiteDialog`, wired to the existing
   `repoProbe` IPC. No new IPC command is needed for the probe.
7. **Live check** `git_site_clone_check` (sandbox tier) — builds a **local** fixture repo
   (a minimal Laravel-shaped tree: `artisan`, `public/index.php`, `composer.json`,
   `.env.example`), clones it over `file://` (already permitted by `clone_args`'
   `protocol.file.allow=user`), and proves: the docroot ends up with the checkout, the
   `.env` is wired to the site's database, a non-empty docroot is REFUSED, a cancelled
   clone leaves the docroot empty and no staging dir behind. Hermetic — no network.
8. **Docs in the same commits** — ARCHITECTURE §9 bullet, MAP.md, CLAIM-LEDGER rows +
   tally, TESTING.md + `scripts/live-checks.sh`, SMOKE-TEST.md manual flow, TODO.md tick.

### Claims for the ledger (Stage 1)

| Claim | Where | Proof |
|---|---|---|
| A clone can never delete a docroot's existing contents | `clone_into_docroot` | `remove_dir` (OS refuses non-empty) + unit test + live check |
| A failed/cancelled clone leaves no staging directory | `clone_into_docroot` | live check asserts the sites dir has no `.rexenv-clone-*` |
| An agent can never create a site from a repo | `provision_with` | unit test over `Ownership::Agent` |
| `git_url` and `path` can never both apply | `provision_with` | unit test |
| A git Laravel site serves `public/`, never the project root | `docroot_subdir` from `detect_project` | unit test + live check `curl` for `.env` → 404 |

---

## 4. Stages 2–4 (designed, not built)

- **Stage 2 — frontend assets. SHIPPED 11 Aug 2026** (`240566b` →). A Laravel app with
  Vite throws *"Unable to locate file in Vite manifest"* until `npm run build` has run,
  so Stage 1 was honest-but-incomplete for most real repos. Built as an `assets` phase
  of the CLONE rather than of Laravel — any repository can carry a `package.json` — with
  the package manager taken from the repo's own `packageManager` field (lockfile second)
  and run from the developer's login-shell Node. Two things came out differently from
  this plan, both deliberate:
  - **Default ON, not opt-in.** The default that produces a working site is the one that
    builds. It is disclosed rather than hidden: the box above Create lists every command
    that will run, `install` (postinstall scripts included) and `run build` among them.
  - **Non-fatal, which is what "must not mark an otherwise-serving site incomplete"
    actually required.** The job settles `ok` with an `assets_warning` the card shows as
    a warning banner — the same "succeeded, but" shape as `serving_blocked`. Failing the
    job would park a created, wired, serving site behind a Retry that re-runs the clone,
    the database and Composer to reach the one step that was never rexenv's to
    guarantee.
  - **Still open:** re-running the build LATER. It is offered only at create; a per-site
    step runner belongs with Stage 3's RepoPanel.
- **Stage 3 — a Git panel on the site itself. SHIPPED 11 Aug 2026.** It was mostly
  plumbing, as expected — one `job_target` seam in `commands/repo.rs` maps a new
  `kind: "site"` to the project root, and nine commands plus the whole `RepoPanel`
  followed. Three things worth keeping:
  - **One panel, not two.** A second implementation would have meant a second answer to
    "is this checkout dirty" — the question every destructive confirmation is built on.
    `wp dist-archive` is the only piece hidden for a site (a project root is not a
    distributable), and it is *absent* rather than disabled: the disabled-with-a-reason
    treatment teaches someone who could fix it, and here there is nothing to fix.
  - **Never an upward walk.** `repo_site_info` is a single `<site.path>/.git` test. A
    linked Laravel site stores `…/app/public`, so a parent search would find the
    project's repo one level up — and one level further, a `~/code` repo holding forty
    projects, where the panel's Checkout button is a catastrophe nobody asked for. The
    tab is absent, and the empty state names the folder that was looked at.
  - **A site target has no path segment at all.** `dir_name` is the domain, used for
    display and the log key, never joined into a path — so this is the one kind that
    cannot have a traversal bug, and the asset kinds' M7 gate is untouched.
  - It also closes Stage 2's known gap: the repo's package.json scripts are listed here,
    so a Vite build can be re-run (or `dev` watched) long after create.
- **Stage 4 — any PHP repo, and WordPress. SHIPPED 11 Aug 2026.** Two halves, and the
  first was smaller than it looked while the second was larger.
  - **Any PHP repo** was almost done by Stage 1 — `detect_project` classifies Symfony,
    Craft, Statamic and Magento, and the clone already recorded their `docroot_rel`. What
    was missing is that `vendor/` is gitignored in every one of them, so the clone
    produced a document root pointing at a front controller that could not run. A cloned
    Blank-PHP site now gets the `deps` phase (skipped, not failed, without a
    `composer.json`) and no database engine.
  - **WordPress** turned out to need almost no new phases: `core_download`, `configure`
    and `core_install` were ALREADY skip-aware, so a cloned site runs the same four with
    a dependency step in front. This plan said it belonged behind the database-import
    work; what it actually needed was to stop pretending the database question does not
    exist — the dialog says, before Create, that the code comes from the repository and
    the database is new and empty, with the Database tab's dump import as the other half.
  - **Roots' Bedrock/Radicle is the one real fork.** Composer owns core and `.env` owns
    the configuration, so one answer (`sites::wordpress_core_from_composer`) turns off
    both `core_download` and `wp config create`, and `wordpress::wire_bedrock_env` writes
    the database, the URLs and any UNSET salts. ⚠ **Unverified against a real Bedrock
    project** — the key set is from Roots' documented example, filed in `docs/TODO.md`
    with a SMOKE-TEST checklist rather than left as a comfortable silence.
  - **The bug this stage found**: the served root and the content dir were recorded at
    CREATE, from a folder that was still empty — so a cloned Bedrock site would have had
    every mu-plugin written into a `web/wp-content` it does not load. Both are now
    re-read from the checkout.

---

## 5. Open questions (asked; answer changes only the marked items)

1. ~~**`artisan migrate` in the create flow**~~ — **answered: default ON with a
   checkbox**, shipped 11 Aug (v34; the choice is recorded so Retry honours it).
2. ~~**Node assets**~~ — **answered: Stage 2**, shipped 11 Aug (§4). Default ON and
   non-fatal; say if you'd rather it were opt-in.
3. ~~**Which types may be cloned**~~ — **settled**: Laravel + Blank PHP in Stage 1,
   WordPress added in Stage 4 once the database promise could be STATED rather than
   designed around (§2.2 records the earlier refusal and why it changed).
4. **Placement** — the New Site dialog's source selector (recommended), or a row in the
   existing `/import` route? The Import route is about migrating a whole existing dev
   environment (Valet/Herd); one repo is a *new site*, not a migration.
5. **`.env` handling when the repo ships a committed `.env`** (bad practice, happens) —
   recommended: keep it, rewrite only `APP_URL` + `DB_*` through the existing `wire_env`,
   and say so in the log. Never silently replace a file the repo shipped.
