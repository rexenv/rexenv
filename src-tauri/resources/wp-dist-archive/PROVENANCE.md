# Vendored: `wp dist-archive`

**Do not hand-edit anything in `vendor/`.** It is generated. Regenerate with
`scripts/build-wp-dist-archive.sh`, whose header carries the bump procedure.

## What this is

`wp dist-archive` builds a distributable `.zip` from a plugin or theme checkout,
honouring `.distignore`. It is **not** part of WP-CLI — it is the separate
composer package `wp-cli/dist-archive-command`, plus the library it matches paths
with, `inmarelibero/gitignore-checker`. Both MIT.

## Why it is vendored rather than installed

Because `wp package install` writes `~/.wp-cli/packages/` — a directory the user
owns — needs the network, and runs composer at runtime, on a machine rexenv
otherwise keeps pinned and checksum-locked.

And because of how this was nearly missed. Asked whether the bundled phar had
`dist-archive`, running `wp dist-archive --help` said **yes** — from a package
installed on that laptop in December 2021. The phar has no such command. Had the
answer been taken, the feature would have shipped working on exactly one machine.
See ledger #228 and the "machine was the fixture" entry in
`docs/CLAIM-LEDGER.md`. The rule that came out of it: a command rexenv depends on
is a command rexenv carries.

## Provenance

`composer.lock` is the record — exact versions, dist references, licences and a
content hash. `composer.json` is deliberately named `wp-cli/wp-cli` at our pinned
phar version: that is the trick WP-CLI's own package installer uses, so the
package's `wp-cli/wp-cli` requirement is satisfied by the phar we already ship
instead of vendoring a second copy of WP-CLI (6 packages and several MB).

| | |
|---|---|
| wp-cli/dist-archive-command | **3.1.0** (`e91730cddd4b`) |
| inmarelibero/gitignore-checker | **1.0.4** (`57cdaa05ceaa`) |
| resolved against | wp-cli **2.12.0** (`binaries::WP_CLI_VERSION`) |
| files / size | 77 / 368 KB |

## The version coupling (found 4 Aug 2026)

| dist-archive | requires | usable here? |
|---|---|---|
| v3.1.0 | `wp-cli/wp-cli ^2` | yes — our 2.12.0 |
| v3.2.0 | `wp-cli/wp-cli ^2.13` | **no** — composer refuses until WP-CLI is bumped |

So a dist-archive bump can require a WP-CLI bump first. Composer says this
plainly rather than producing something that half-works, which is the good case.

## Three things move together, or the build fails

1. this tree — `scripts/build-wp-dist-archive.sh`
2. `core::wp_packages::DIST_ARCHIVE_VERSION`
3. the two rows in `THIRD-PARTY-NOTICES.md`

(1) and (2) are enforced: `the_vendored_tree_is_the_version_we_pinned` reads the
version out of `vendor/composer/installed.json` — the artefact, not a second copy
of the number — and fails if they disagree.

## Behaviour worth knowing before changing the pin

Measured at v3.1.0, and the feature above it is built around these:

- **`.distignore` only.** There is no `.gitignore` fallback (that was pre-3.0).
  With no `.distignore` the command archives *everything*, `.git` and
  `node_modules` included, and reports `Success:` — which is why rexenv refuses
  rather than warns.
- **It litters `TMPDIR` and never sweeps it**, on success as much as on failure.
- **An occupied target path** triggers an interactive prompt that becomes an
  uncaught PHP fatal under a non-TTY.

Full measurements: `docs/archive/PLAN-dist-archive.md` §1.
