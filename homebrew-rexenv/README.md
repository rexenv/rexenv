# homebrew-rexenv — custom Homebrew tap for rexenv

A personal Homebrew **cask** tap for [rexenv](https://rexenv.rex.bd) — a native,
no-Docker local WordPress & web development environment for macOS.

> ⚠️ **This is a staging copy inside the app repo.** To make it a real tap, push the
> **contents of this `homebrew-rexenv/` directory** (i.e. the `Casks/` folder + this
> README) to a **new GitHub repo named `homebrew-rexenv`** under your account
> (`github.com/rudlinkon/homebrew-rexenv`). The `homebrew-` prefix is what makes
> `brew tap rudlinkon/rexenv` resolve to it.

## Install

```sh
brew tap rudlinkon/rexenv
brew install --cask rexenv
```

`rex` (the CLI) is put on your PATH automatically by the cask.

## ⚠️ Security: unsigned / un-notarized (ad-hoc), and what that means

This build is **ad-hoc code-signed** (Tauri `signingIdentity: "-"`), **not notarized**
by Apple — there is no paid Apple Developer ID behind it. Consequences:

- The app **is** validly code-signed (ad-hoc), which satisfies the Apple Silicon
  requirement that arm64 code carry a signature — so it **runs** on both Apple
  Silicon and Intel.
- But macOS **Gatekeeper** quarantines any download and refuses to launch a
  non-notarized app *while it is quarantined*. To make it launch, the cask's
  `postflight` **removes the quarantine attribute** (`xattr -dr
  com.apple.quarantine`). **This deliberately bypasses Gatekeeper's notarization
  check.**

Install this **only if you trust this source** (it's a personal build, distributed
to a small known audience). If the postflight can't remove the attribute on your
setup, run it yourself once:

```sh
sudo xattr -rd com.apple.quarantine /Applications/rexenv.app
```

A future signed + notarized build (with a Developer ID) would remove the need for
any of this — see `docs/SIGNING.md` in the app repo.

## Uninstall — do the in-app step FIRST

rexenv installs **privileged, system-level** things that Homebrew **cannot** remove:
a **root LaunchDaemon** running the edge proxy on **:443**, `/etc/resolver/*` files,
and a **local-CA trust** in your login keychain. Before uninstalling:

1. In the app: **Settings → "Remove system changes"** (removes the root edge daemon,
   DNS resolver files, and CA trust — one admin prompt).
2. Then:
   ```sh
   brew uninstall --cask rexenv          # removes the app + the `rex` symlink
   brew uninstall --zap --cask rexenv    # also trashes ~/Library app-data + LaunchAgents
   ```

`--zap` intentionally leaves your **`~/rexenv/Sites`** folder alone (that's your work).

## Updating the cask for a new release

1. Build + upload the new `rexenv_<version>_universal.dmg` to a GitHub Release
   tagged `v<version>` on `github.com/rudlinkon/rexenv`.
2. Recompute the checksum **from the uploaded asset** and bump the cask:
   ```sh
   shasum -a 256 rexenv_<version>_universal.dmg
   ```
   Update `version` and `sha256` in `Casks/rexenv.rb`, commit, push.
3. `brew update && brew upgrade --cask rexenv` for users.

## Status

The **static** side is verified: the app is validly ad-hoc signed
(`codesign --verify` passes), universal (`x86_64 arm64`), and the cask passes
`brew style`. The **final gate** before announcing this tap is the empirical
Apple-Silicon launch test (quarantine → blocked → `xattr -rd` → launches) — see
`docs/PUBLISH-TESTING.md` in the app repo, section A.
