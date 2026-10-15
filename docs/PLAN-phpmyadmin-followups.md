# PLAN — phpMyAdmin follow-ups: the third update family, the dark palette, the sweep

**Status: in progress (15 Oct 2026)** — (1) the family and (3) the sweep shipped in the app
(`feat(phpmyadmin)` commits of 15 Oct 2026); the publisher half is a `rexenv/runtimes` PR
the owner merges; (2) the palette is the next commit. Parent: `docs/archive/
PLAN-db-console-open-and-phpmyadmin.md` §9. Owner's ask, 15 Oct 2026: "phpMyAdmin
follow-ups gulo o kore felo".

## 1. The third `updates::Family` — what was decided, and why

The Adminer plan's shape (`docs/archive/PLAN-adminer-updates.md`), re-argued rather than
copied. Each point was checked against the files:

- **The grant.** `Shape::Dir` through `prepare_script_dir` (`is_script_tree`): no
  executable is looked for, chmod-ed or signed; the tree is COPIED into its own docroot
  and interpreted by the pool. Strictly below PHP's grant, beside Adminer's. The shape
  guard asserts `Dir` AND `is_script_tree` — a `Dir` that is not a script tree goes
  through `prepare_binary_dir`, which on Windows refuses a tree with no `.exe` (#598).
- **The ceiling is evidence.** `PHPMYADMIN_MAX_MAJOR = 5`: 5.2.3 was run on PHP
  8.3.32/8.4.23/8.5.8 and rendered logged in on all three OSes before the pin. phpMyAdmin
  6 moves the tree (`libraries/classes/` → `src/`, no `libraries/constants.php`) and the
  theme format; `verify_tree` would refuse it at apply time, and the ceiling keeps a 6.x
  row from ever being OFFERED. Raising it = `phpmyadmin_check` green on a 6.x tree first.
- **The host is a path prefix, and a narrow one.** `https://files.phpmyadmin.net/phpMyAdmin/`
  — the same server's `/themes/` is where the dark theme is pinned FROM, and a catalog
  row must not be able to name one (a theme is matched to the tree's series by hand).
  Tested with the `/themes/` URL, `http`, the bare host, and GitHub.
- **Arch-free, one row.** `any`, as Adminer: a zip of PHP is the same bytes everywhere,
  and two rows would make a half-published version structurally legal (#355).
- **Within the ceiling, any newer version is an update** (5.2.x → 5.3.0 included): the
  probe re-checks at apply time, and a same-minor rule would refuse the feature the way a
  same-major rule would have for Adminer.
- **`phpmyadmin_spec`: pin first, catalog second, extractor from the NAME**
  (`ZipTree { strip: 1 }` / `index.php`). A document that could name an extractor would
  choose which extractor runs — the larger grant (#362's reasoning).
- **ONE version answer**: `phpmyadmin::effective_version` = choice → floored by the pin
  (`phpmyadmin_floored`) → vouched through `binaries::manifest`. `db_console_set` stages
  THIS, not the bare pin — a console updated through the manifest and flipped away must
  come back at the chosen version.
- **The apply order differs from Adminer's, deliberately.** Adminer probes in a throwaway
  dir, then persists, then restages. phpMyAdmin's probe is INSIDE the stage (`ensure`:
  copy → config → probe → rename), so the order is vouch → download → `ensure` → THEN
  record the choice. The reverse would leave "X chosen, pin staged" after a probe refusal,
  with nothing to restage it — no start ever runs `ensure` (#175's rule for the console).
- **"Pin moved under the docroot."** After an app release re-pins above what the docroot
  holds, `effective != staged`; the card offers the effective version with the same
  button, no manifest involved. Nothing restages on launch (that would be a download on
  the start path).
- **No CLI verb** for the apply (Adminer has none either); `rex db console` prints the
  offer. MCP: `stack` actions `phpmyadmin_update_check` (manage) / `phpmyadmin_update_apply`
  (system), the Adminer pair's scopes.
- **The publisher** (`rexenv/runtimes`, its own PR): `PHPMYADMIN_PIN`/`PHPMYADMIN_MAX_MAJOR`
  beside Adminer's; discovery is a LISTING of `phpmyadmin/phpmyadmin` releases
  (`RELEASE_x_y_z` tags, drafts/prereleases excluded); the artifact is the project's
  own `phpMyAdmin-<v>-english.zip` from `files.phpmyadmin.net`, hashed by us AND checked
  against the `.sha256` the project publishes beside it (refuse on disagreement — the
  bytes a user receives must be the ones the project signed off); `arch: any`;
  `manifest-guard.py` learns the family; `check-php-pins.sh` compares both pairs.

## 2. The palette — decided

- phpMyAdmin 5.2 ships `pmahomme`, `bootstrap`, `metro`, `original`; none dark, and
  `bootstrap`'s compiled `theme.css` has no `data-bs-theme` color modes (one occurrence,
  checked), so Bootstrap 5.3's switch is not available without rebuilding the theme.
- **BooDark 1.2.0** (`adorade/boodark`, MIT, `supports: ["5.2"]`, Bootstrap 5.3.8 — the
  same Bootstrap 5.2.3 bundles) is the dark theme, pinned as a download
  (`phpmyadmin-theme`, `ZipTree { strip: 1 }` → `theme.json`, sha256 from the project's
  theme page AND `.sha256`), never a manifest row. ~657 KB, 328 files. darkwolf 5.2 was
  the alternative (2022, GPL); BooDark is maintained (Oct 2025) and Bootstrap-native.
- **The mechanism is Adminer's**: rexenv writes the RESOLVED palette (`dark`/`light`) to
  `.rexenv-theme` in the docroot by rename; the generated `config.inc.php` reads it per
  request and sets `ThemeDefault` to `boodark` when it says `dark` AND the theme dir
  exists, `pmahomme` otherwise; `ThemeManager = false` so the console shows no picker
  (the app decides, as for Adminer). The frame is `key`ed on the palette (already), so a
  theme change reloads the document.
- The theme is staged by `ensure` beside the tree (`themes/boodark/`), and the probe
  checks `theme.json`'s `supports` contains the tree's `Version::SERIES` — the gate that
  catches a 5.3/6.0 bump outrunning the vendored theme.

## 3. The sweep — decided

`is_outdated_phpmyadmin_cache` / `gc_outdated_phpmyadmin_caches` at launch, beside the
PHP sweep: keep = the pin + the effective version; `phpmyadmin-x.y.z` names only (never
`phpmyadmin-theme-…`, never a staging dir); an empty keep-set sweeps nothing. Worth it
where Adminer's 324 KB was not: ~52 MB per tree, one left behind per update.

## 4. Proof

L0 as the ledger rows name (#871–#874); L1 `phpmyadmin_update_check` (network tier,
`all`) — the chain to the stage, with the floor legs running on every machine and the
download/probe legs once a row is published; L2 `dbconsole-*` ×6; SMOKE rows ×3 OSes,
open until the first phpMyAdmin row is published.
