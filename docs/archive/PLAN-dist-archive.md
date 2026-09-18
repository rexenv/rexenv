# PLAN — `wp dist-archive` in the RepoPanel: build a distributable zip

**Status:** researched and ruled 4 Aug 2026; **COMPLETE** — all 9 tasks landed 4–5 Aug 2026.
Availability path **ruled: bundle** (§2, option b).

Ship one button in the Git/asset panel that turns a plugin or theme checkout into a
distributable `.zip` in the user's Downloads folder, the same way the DB export lands
there.

**Deliberately git-only.** Someone developing a plugin or theme has it in a repo;
zipping an installed-from-wp.org plugin serves no purpose. So it belongs beside the
existing repo actions (`RepoPanel.tsx:376`, the Fetch / Pull / Push row), not on
every asset.

---

## 1. What was measured, not assumed

Everything in §§1–4 was run against the bundled phar (`wp-cli 2.12.0`) and the bundled
PHP (`php-8.3.31`) on 4 Aug 2026, on fixtures under a scratch dir. Where a finding
contradicts the obvious expectation, the command and its output are quoted.

### 1.1 `dist-archive` is NOT in the bundled phar — and this machine said it was

```
$ php wp-cli.phar dist-archive --help
NAME  wp dist-archive
```

False. That resolved out of `~/.wp-cli/packages/` — `wp-cli/dist-archive-command 3.1.0`,
installed on the dev laptop in **December 2021**, pulled from VCS, last updated in March.
Neutralise the ambient source and it disappears:

```
$ WP_CLI_PACKAGES_DIR=<empty dir> php wp-cli.phar dist-archive --help
Error: 'dist-archive' is not a registered wp command.
```

Two consequences, one per file: the feature needs the package from somewhere (§2), and
**rexenv's wp-cli inherits a user's global package dir at all times** — bigger than this
feature, recorded separately as ledger **#228** + its own TODO item + an owner decision.
The way it was found is in the ledger's defect families ("the machine was the fixture").

### 1.2 It honours `.distignore` ONLY — the `.gitignore` fallback does not exist

Read at **v3.1.0 and v3.2.0** (identical on this point), and in the README:

```php
$this->checker = new GitIgnoreChecker( $source_dir_path, '.distignore' );
if ( ! file_exists( $source_dir_path . '/.distignore' ) ) {
    WP_CLI::warning( 'No .distignore file found. All files in directory included in archive.' );
```

`.gitignore` **syntax**, `.distignore` **file**. The fallback is pre-3.0 behaviour and is
gone. With the file present it works exactly as advertised: probe plugin went from
24.86 KB to **416 B**, `.git` and `node_modules` gone.

**Without the file it ships everything and reports success** — the default case is the
harmful one:

```
Warning: No .distignore file found. All files in directory included in archive.
Success: Created my-plugin.1.2.3.zip (Size: 24.86 KB)      ← exit 0

my-plugin/node_modules/leftpad/index.js
my-plugin/.git/objects/f3/7655ffbb76a892c94929337163d28b8ba0c9bb
```

A zip that ships `node_modules` and `.git` is worse than no feature, and it arrives as
a `Success:`. **This is why the feature REFUSES rather than warns** (§5, task 3).

### 1.3 Where it writes

- **Default target is `dirname(realpath(<path>))`** — the parent of the source. For a
  cloned asset that is `wp-content/plugins/`; for a **linked** asset `realpath` resolves
  the symlink first, so the default lands **inside the user's own repo's parent**. An
  explicit target is mandatory, always.
- **It never writes into the source tree** — confirmed with `git status` on the probe
  after a symlinked run.
- **It litters `TMPDIR` and never sweeps it.** Include/exclude list files go to
  `sys_get_temp_dir()/uniqid(…)`; and when the source contains a symlink, or the archive
  dir name differs from the folder name, it **copies the whole filtered tree there first**.
  Measured: 5 runs → 5 leftover dirs, one of them **304 KB** — a full copy of the probe.
  On a real plugin with `vendor/` that is tens of MB per click, and it happens on success
  as well as on cancel.

### 1.4 The overwrite prompt is a fatal error under a non-TTY

Existing file, no `--force`, stdin closed — exactly our job shape:

```
Warning: Archive file already exists
Do you want to skip or replace it with a new archive? [s/r]:
Fatal error: Uncaught Exception: Caught ^D during input in .../cli/Streams.php:145
Stack trace: #0 … #16 {main}
```

Not a clean skip — an uncaught PHP fatal with a 17-frame trace, straight into the log
pane. **Handled structurally, not caught**: build into a fresh empty temp dir of ours, so
an occupied path is unreachable (§5, task 4).

### 1.5 Version, and the linked-asset wrinkle

Version is **read, never asked**: `style.css` `Version:` for themes → any root `*.php`
docblock `@version` / `Version:` → `composer.json` `version`. Probe's `Version: 1.2.3`
produced `my-plugin.1.2.3.zip`. **No version anywhere → `<dirname>.zip`, silently,
exit 0** — worth one line in the result, not a block.

The wrinkle, measured: link `~/code/my-awesome-plugin` into wp-content as `awesome-slug`
and the zip is named **`my-plugin.1.2.3.zip`** with internal dir **`my-plugin/`** — from
the *real folder*, not from the slug rexenv displays. `--plugin-dirname` would override
it. **We don't.** Their repo folder is what they ship; rexenv silently renaming someone's
plugin to match our own label is worse than a name that differs from it. So the UI shows
the **filename actually produced**, never a predicted one.

---

## 2. Availability — ruled: BUNDLE

| Path | Verdict |
|---|---|
| `wp package install` | **Rejected.** Network at first use, **composer at runtime**, and it writes `~/.wp-cli/packages/` — a directory the user owns and versions. Mutating it on a machine we otherwise keep pinned and checksum-locked trades the whole posture for one button. |
| **Bundle** | **RULED.** `--require=<vendor>/autoload.php` registers the command with the packages dir empty — **proven**, produced a correct zip. Tree is `wp-cli/dist-archive-command` + `inmarelibero/gitignore-checker`, **both MIT**, the transitive one has **zero deps** beyond PHP ≥7.1, ~470 KB of PHP. `~/.wp-cli` never touched. |
| Reimplement in Rust | **Rejected.** Would kill the external `zip`, the temp litter and the prompt bug — but buys a *compatibility claim* ("our `.distignore` means what wp-cli's means") we would have to defend forever, unproven, and larger than the feature. |

**External dependency either way:** `/usr/bin/zip`, present on stock macOS (`WP_CLI::launch`
shells out to it). Named here so it is a known dependency rather than a discovery.

---

## 3. Where it runs, and what it must not disturb

The checkout may be the user's own project directory (linked sites, Stage 0). The standing
rule is that our conveniences inside their folder are **disclosed up front and swept by
exact known path** (`PLAN-linked-sites.md` §6). A build artefact appearing in their
`git status` is not in that class and is not acceptable — so:

- the archive is built in **our** temp dir under app-data, never beside the source;
- `TMPDIR` is pointed **at that dir** for the child, so the tool's own litter (§1.3)
  lands inside the thing we delete;
- the dir is removed on **all three** exits — ok, failed, **cancelled**.

---

## 4. MCP — M-later, and it can't free-ride on `wp_run`

Yes eventually, and cheaply: `wp_run` already refuses agent-supplied targets and derives
the docroot from the `ScratchSite` witness, so a `dist_archive` tool is that same witness
plus one recorded asset dir. The reason it needs its own tool rather than riding the raw
runner: **the zip has to land somewhere, and that somewhere is not an agent's choice** —
the target rule is the tool. Tagged M-later; nothing here needs a retrofit when it comes.

---

## 5. The tasks — one commit each, `scripts/verify.sh` green per commit

Ledger rows land in the SAME commit as the invariant comment they describe (CLAUDE.md).

- [x] **1 — Vendor the package, pinned.** ✓ 4 Aug, ledger #229 — 3.1.0 + gitignore-checker 1.0.4 vendored (77 files, 368 KB), embedded via build.rs codegen (no new crate), materialised version-stamped under app-data, version pinned against the tree's own installed.json and plant-proven both ways; notices section + PROVENANCE.md + bump script. Build the two-package tree with composer
  **once, offline-reproducibly**, and land it as a checksum-pinned artefact the app
  resolves like any other binary (`BinaryProvider`) or as a bundled resource — decide by
  which keeps "no network on first use" true, and record which and why. Add both MIT
  packages to `THIRD-PARTY-NOTICES.md` **in the same commit** (the notices file is
  generated from the real graphs; a hand-added row must say so). Guard: the vendored
  tree's version is asserted against a pinned constant, so a silent bump is a build
  failure, not a behaviour change.

- [x] **2 — `core/dist_archive.rs`, the mechanism.** ✓ 4 Aug, ledger #230 — one argv builder that REFUSES the tool's own default target under both of a linked checkout's names, resolved-path comparison on both sides, `WP_CLI_PACKAGES_DIR` neutralised for this spawn only, no idle watchdog by policy. 6 lib tests, two of which caught the code. Resolve the asset dir via the
  existing `repo::asset_dest` (unchanged for cloned and linked). Spawn through
  `run_step_streamed` — the existing program+args+cwd+env runner with process-group
  cancel — as `php -d memory_limit=512M wp-cli.phar --require=<vendored>/autoload.php
  dist-archive <asset-dir> <our-temp> --force`, with the login-shell env
  (`core::devtools`). **No parallel path**: `repo_script_job` is the template.
  Ledger row: *the target handed to `dist-archive` is never the asset dir's parent* —
  and the guard must read the **argv actually spawned**, not a constant beside it.

- [x] **3 — Refuse when `.distignore` is absent.** ✓ 4 Aug, ledger #231 — a precondition inside `argv` (not a warning read back), checked at the canonical source, one predicate shared with the UI, message plant-proven to stay actionable. Empty files accepted as a stated limit. A precondition checked before the run,
  naming what is missing and why it matters, never a warning recovered from output.
  This is the feature's point (§1.2): the default case is the harmful one, and the tool
  reports it as a success. Ledger row, and **plant-proven** — delete the fixture's
  `.distignore` and the refusal must fire with a message a stranger can act on.

- [x] **4 — Our temp dir, `TMPDIR`, and the sweep.** (Root moved from app-data to the OS temp dir on 18 Sep 2026, ledger #679: the space in `Application Support` broke zip's unquoted `-i@` include file on every Mac.) ✓ 5 Aug, ledger #232 — `ScratchDir` is a `Drop` guard so the three exits need not be enumerated correctly; `TMPDIR` redirects the tool's litter inside it; `out/` and `tmp/` split so the archive is unambiguous. Plant-proven, including the happy-path-only version that leaks on failure. Fresh dir per run under app-data;
  `TMPDIR` set to it for the child; removed on ok / failed / **cancelled**. Ledger row
  covering all three exits — a sweep proven on the success path only is the
  coverage/surface family, and cancel is the leg that matters, since the tool litters
  hardest exactly when interrupted.

- [x] **5 — Land it in Downloads.** ✓ 5 Aug, ledger #233 — numbered on collision like the DB export, exclusion from `hard_link`/`create_new` rather than a prior `exists()` check, `EXDEV` fallback removes its own partial. The collision test was rewritten after planting showed it did not discriminate. Mirror `logs::download` / `database::export_to_downloads`
  exactly: `UserDirs::download_dir()`, **numbered on collision** (`name.1.2.3.zip`, then
  `-1`, `-2`), **never overwrite**, partial removed on failure. Because the build happens
  in temp, the numbering is ours and §1.4's prompt is structurally unreachable — state
  that in the doc comment, since it is the reason the fatal never needs handling.

- [x] **6 — `repo_dist_archive` command.** ✓ 5 Aug, ledger #234 — same RepoJobs entry, busy check, log key and events as `repo_script_job`; `.distignore` and the three binaries resolved before the job exists; `ArchiveResult` returns the real path plus `versionMissing`. IPC wrapper + TS type landed with it. Beside `repo_script_job`: same `RepoJobs`
  entry, same one-job-per-dir busy check, same `repo-<domain>-<dir>.log`, same streamed
  state/output events. Returns the **real produced path**.

- [x] **7 — The button.** ✓ 5 Aug, ledger #235 — copy approved with the 'at the top of this checkout' redline, landed verbatim and pinned by a copy guard whose own first version was defective (it read its own explanatory comment) and was fixed to scan only what renders. One in the git-ops row (`RepoPanel.tsx:376`), same `BTN`, same
  `opsDisabled`. Disabled with a title naming the missing `.distignore` and the fix.
  Result names the **filename actually produced** (§1.5), plus a quiet note when no
  version was found. Copy reviewed before it lands, then held by the copy guard.

- [x] **8 — The proofs.** ✓ 5 Aug, ledger #236 — L0 across tasks 2–5 (19 lib tests in the module); L1 `dist_archive_check` (sandbox tier) with the negative control baked in and plant-proven both ways. Downloads deliberately not exercised — stated, not omitted. L0 for the argv, the refusal, the collision numbering and the
  sweep's three exits. **L1 (`sandbox` tier)** for the one thing no unit test reaches:
  the vendored command **resolves with `WP_CLI_PACKAGES_DIR` neutralised** — the negative
  control from §1.1, so this can never again pass because a dev machine had the package
  installed since 2021. Declare the tier in `scripts/live-checks.sh`; fixtures
  production-shaped per `examples/common/mod.rs`.

- [x] **9 — SMOKE step.** ✓ 5 Aug — new §Git assets section, 5 steps, with step 2 a HOLD (the zip must be opened and inspected). Ledger #236 amended rather than a new row. Also states plainly that the rest of the Git panel has no SMOKE coverage. One numbered step in the MCP-style gate shape: a real plugin
  checkout with and without `.distignore`, the zip opened and **looked at**, and the
  linked-asset case where the link name differs from the folder name (§1.5) — the one
  the UI is deliberately not hiding.

---

## 6. Ledger rows this feature will add

Named now so they are written with the code, not after it: the vendored version is
pinned (task 1) · the target is never the asset dir's parent (2) · the refusal precedes
the run (3) · the temp sweep covers all three exits (4) · Downloads never overwrites (5)
· the produced filename is reported, never predicted (7) · the command resolves without
the ambient packages dir (8, L1).

## 7. After shipping — what the first real use found (6 Aug 2026, `d93afde`)

Three faults, none in `core/dist_archive.rs` and none about the archive itself:
the panel around it. Recorded here because two of them are older than this feature
and Build zip only made them visible.

1. **The step sat at pending `○` for the whole run** — read as "nothing is
   happening", which is what it looked like. Not styling: `StepDot` does spin, the
   step never reached `running`. Tauri does not replay events and listener
   registration is `await`ed, so everything a job emits between the spawn and the
   attach is **lost** — and a short job (dist-archive, an up-to-date fetch) fits
   ENTIRELY inside that window, log tail included ("(no output yet)"). The panel now
   re-reads `repo_job_state` + the log tail once its listeners are attached, and
   drops that read if a live event beat it there. Every start button also spins while
   its own call is in flight: `repo_dist_archive` resolves PHP and the WP-CLI phar
   BEFORE the job exists, so on a cold machine the click showed nothing at all.
   *The general fact, not a dist-archive one: an event stream with an async attach
   needs a catch-up read, or the fastest jobs are exactly the ones that look dead.*
2. **A bogus offered row under Build zip** — "Dependencies changed with this
   dist-archive — re-install below" plus "Run all" / "wp dist-archive" buttons.
   The offered row filtered by a BLACKLIST of op-step keys, so every op added after
   it was written leaked into the row; `archive` was simply the next one. Now a
   whitelist of the three steps a job can append (`composer`/`install`/`build`,
   `commands/repo.rs`) — the same coverage/surface family as ledger #197, fixed the
   way that family has to be fixed: state the closed set, not the exceptions.
3. **The zip re-announced itself on every re-expand** of the asset row. The
   toast dedup was a per-mount ref and collapsing the row unmounts the panel; the
   finished archive job is then re-adopted on each mount, because `set_step` only
   sets `finished_ok` for jobs with **≥ 2 steps** and every single-step job
   (`archive`, `fetch`, `pull`, `check`, `script`) therefore never reports finished.
   Dedup moved to a module-level set of job ids. **The `≥ 2` rule is still there** —
   left alone deliberately: `finishedOk` also drives GitAddPanel's post-clone UI, so
   changing its meaning is its own task with its own evidence, not a rider on a
   toast fix.

## 8. Related

`docs/PLAN-linked-sites.md` (what may be written into a user's folder) ·
`docs/PLAN-mcp-server.md` (the M-later tool) · ledger **#228** and the "machine was the
fixture" family entry (why §1.1 is written the way it is).
