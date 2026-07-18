# SIGNING.md — Developer ID signing + notarization (macOS)

The plan for turning rexenv's current **ad-hoc-signed** bundle into a **Developer ID-signed +
notarized + stapled** build, so Gatekeeper opens it on any Mac with no right-click-Open workaround —
the prerequisite for a public Homebrew **cask**.

This is review finding **B32** and TODO "Developer ID signing + notarization" (Release 1.6). It needs a
paid Apple account + a Developer ID cert, which is an operator task, not a code change. **Nothing here
has been applied** — the build still ad-hoc signs (`tauri.conf.json` → `bundle.macOS.signingIdentity:
"-"`). When you have the cert wired, the change below is mechanical.

## Prerequisites (operator side — not in the repo)

- Apple Developer Program membership ($99/yr).
- A **Developer ID Application** certificate + its private key in your login keychain
  (Apple Developer portal → Certificates, or Xcode → Settings → Accounts → Manage Certificates →
  "Developer ID Application"). Confirm with: `security find-identity -v -p codesigning`
  → you should see `Developer ID Application: <Name> (<TEAMID>)`.
- For notarization, **one** of: an App Store Connect **API key** (`.p8`, recommended — headless/CI
  friendly), or your Apple ID + an **app-specific password** (appleid.apple.com).
- Your 10-character **Team ID**.

## 1. The only config change: stop forcing ad-hoc

`src-tauri/tauri.conf.json`, current:

```json
    "macOS": {
      "signingIdentity": "-",
      "minimumSystemVersion": "11.0"
    },
```

Change to — remove the `"-"` (leaving it forces ad-hoc and **overrides** the env var below):

```json
    "macOS": {
      "minimumSystemVersion": "11.0"
    },
```

Then supply the real identity at build time via `APPLE_SIGNING_IDENTITY` (below) so the cert name/secrets
never land in git. (Alternatively hardcode `"signingIdentity": "Developer ID Application: <Name>
(<TEAMID>)"`, but env is cleaner for a public repo.)

- **Hardened runtime:** no config needed — Tauri's bundler already applies the runtime option to
  `codesign` (visible even today: the ad-hoc bundle reports flags `0x10002(adhoc,runtime)`), so a
  Developer ID build is hardened automatically. There is no separate `hardenedRuntime` key in Tauri's
  macOS bundle config.
- **While you're in there:** drop the dead `"android"` block (unrelated cleanup; no Android target).

## 2. Notarization — env vars for `pnpm release:mac`

`tauri build` signs **and** notarizes **and** staples automatically when the Apple credentials are
present in the environment. Pick one auth path:

**Option A — App Store Connect API key (recommended):**
```sh
export APPLE_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAMID>)"
export APPLE_API_ISSUER="<issuer-uuid>"
export APPLE_API_KEY="<key-id>"
export APPLE_API_KEY_PATH="/absolute/path/AuthKey_<key-id>.p8"
```

**Option B — Apple ID + app-specific password:**
```sh
export APPLE_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAMID>)"
export APPLE_ID="you@example.com"
export APPLE_PASSWORD="<app-specific-password>"   # NOT your Apple ID password
export APPLE_TEAM_ID="<TEAMID>"
```

(For CI, where the cert isn't already in a keychain, also export `APPLE_CERTIFICATE` = base64 of the
exported `.p12` and `APPLE_CERTIFICATE_PASSWORD`; Tauri creates a temporary keychain to import it.)

Then build the universal release as today:
```sh
pnpm release:mac      # = tauri build --target universal-apple-darwin
```
Tauri signs `rexenv.app` (including the bundled `rex` sidecar) with the Developer ID + hardened runtime,
submits to `notarytool`, and staples the ticket to both the `.app` and the `.dmg`.

## 3. Entitlements — you almost certainly need NONE

rexenv is distributed with **Developer ID** (not the Mac App Store), so it is **not sandboxed** — it
keeps full filesystem / network / process access with **no** entitlements file. For the first notarized
build, do **not** add one, and do **not** set `bundle.macOS.entitlements`.

The single case that would require one: if the app ever `dlopen`s a **downloaded** dylib into its **own**
process, the hardened runtime's library validation would reject the differently-signed lib. rexenv does
**not** do this — it runs downloaded binaries as **separate child processes** — so leave entitlements
empty. If that ever changes, add a `.plist` with:
```xml
<key>com.apple.security.cs.disable-library-validation</key><true/>
```
and point `bundle.macOS.entitlements` at it. (JIT / unsigned-executable-memory entitlements are **not**
needed: WKWebView's JIT runs in a separate system WebContent process with its own entitlements.)

## 4. rexenv-specific gotchas (so ad-hoc signatures aren't mistaken for a bug)

- The **downloaded helper binaries** (php-fpm, nginx, caddy, mysqld, mariadbd, mailpit, cloudflared, …)
  are **ad-hoc** signed at runtime by `prepare_binary` (de-quarantine → relink Homebrew dylibs →
  codesign LAST) and de-quarantined. This is CORRECT and unchanged by notarization: they run as **child
  processes** of the notarized app; locally-generated, de-quarantined binaries are allowed by Gatekeeper
  and do **not** need their own Developer ID / notarization. Do not try to notarize them.
- The **root edge LaunchDaemon** runs a `root:wheel 0755` **copy** of caddy under
  `/Library/Application Support/dev.rexenv.rexenv/bin/` — also ad-hoc signed, launched by launchd.
  Fine; unaffected.
- The **`rex` sidecar** (`externalBin`) is signed with the app's Developer ID as part of the bundle — no
  separate step.

## 5. Verification

On the **build** Mac:
```sh
codesign -dv --verbose=4 "…/rexenv.app"                 # Authority=Developer ID Application…; flags include 'runtime'
codesign --verify --strict --verbose=2 "…/rexenv.app"   # valid on disk (avoid deprecated --deep; Tauri signs inside-out)
spctl -a -vvv -t exec "…/rexenv.app"                    # accepted; source=Notarized Developer ID
xcrun stapler validate "…/rexenv.app"                   # The validate action worked!
xcrun stapler validate "…/rexenv_*.dmg"                 # staple present on the dmg too
```

On a **separate clean Mac** (or a fresh VM / user account) — the real test, since the build Mac trusts
its own cert:
- Download the `.dmg` the way a cask user would (`curl`/browser, so it gets the `com.apple.quarantine`
  xattr), open it, launch the app — it must open with **no** "unidentified developer" / "damaged"
  prompt and **no** right-click-Open.
- Run `docs/SMOKE-TEST.md` end to end.

## 6. Follow-ups once this lands

- Update `docs/INSTALL.md` — remove the Gatekeeper right-click-Open workaround section.
- Flip TODO.md "Developer ID signing + notarization" (Release 1.6) to done, and mark B32 fixed in
  `docs/CODEBASE-REVIEW.md`.
- The Homebrew cask then needs no `xattr`/quarantine workaround.
