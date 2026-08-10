# PLAN — preferred browser (+ real app icons for browser & editor)

Status: **in progress** (2026-08-11). Owner: this file is the design record; tick
tasks here and in `docs/TODO.md` as they land.

## What the user asked for

> "Open in code editor" already lets you pick the editor in Settings — give the
> browser the same treatment. Every "open in browser" in rexenv should use the
> browser the user chose. And show the browser's own icon on the button, with a
> small chevron next to it; clicking the chevron lists every browser installed on
> the machine so you can send this one URL somewhere else.

Two decisions the user made up front (asked before building):

1. **A chevron pick is one-time.** Clicking "Firefox" in the dropdown opens THIS
   url in Firefox and changes nothing. The default is changed in Settings only —
   a menu that silently rewrites a preference is the classic "why is everything
   opening in Firefox now" bug.
2. **Scope:** browser split-button (SiteDetail header + Quick-links tile + Sites
   row icon), the editor Quick-links tile gets the same chevron, and both
   Settings pickers show the app's real icon.

## What already existed (do not rebuild)

The editor half is **done and shipped**: `preferred_editor` setting,
`ShellRunner::detect_editors` + `open_in_editor`, `list_editors`/`open_in_editor`
commands, `usePreferredEditor()`/`openSiteInEditor()` in `src/lib/useEditor.ts`,
and a Settings → "Code editor" select. This plan mirrors that shape for browsers
rather than inventing a second one, and adds icons to both.

## Where the preference is enforced — the one load-bearing decision

**In the backend, inside `open_external`.** Not in the UI.

rexenv opens `http(s)` URLs from ~12 call sites (site header, quick tile, Sites
row, Tunnels, Mail, Adminer, magic login, WordPressManager visit/admin, plugin
and theme rows, the provision-done card…). If the preference were applied by a
UI helper, honouring it would mean editing every call site — and the next call
site someone adds silently opens in the system default. That is exactly the
drift class the project has already paid for twice (a "whole-surface" claim that
only checked one place inside the surface — see `docs/CLAIM-LEDGER.md`).

So `open_external(target)`:

- `target` is `http://` or `https://` **and** a `preferred_browser` is stored
  **and** that browser is still installed → `open -a <App> <url>`.
- anything else (a folder, a file, no preference, preference uninstalled) →
  the OS default handler, exactly as today.

The preference is a *stored id*, and installed-ness is re-checked **at every
open**, not once at save time — a one-time check on a mutable fact is a
snapshot, and the user can uninstall a browser any day (`one-fact-lifetime-guards`).

`open_in_browser(browser_id, url)` is the separate, explicit one-time path used
only by the chevron menu.

### Guard: `open_in_browser` takes URLs, never paths

`open -a "Google Chrome" /some/path` happily opens a *file* in Chrome. The
chevron menu is a URL affordance, so `open_in_browser` **rejects anything that
is not `http://` or `https://`** before it reaches `open`. Claim + verdict go in
`docs/CLAIM-LEDGER.md` in the same commit as the guard.

## Icons — real ones, extracted, never hand-drawn

Hand-drawn brand SVGs would mean hardcoded brand hex (against `docs/DESIGN.md`)
and would rot every time a vendor restyles. Instead the icon is read off the
installed app bundle:

    Info.plist CFBundleIconFile → Contents/Resources/<name>.icns
    sips -s format png -Z 64 <icns> --out <tmp>  →  base64  →  data: URI

Measured on the dev Mac: **24 ms and ~3.6 KB of base64 per app** at 32 px, so a
64 px icon for ~8 browsers is a one-off ~30–60 KB payload, cached in-process and
again by TanStack Query. Fallbacks, in order: `CFBundleIconFile` (append `.icns`
if the plist omits the extension — Safari stores `AppIcon`), then `AppIcon.icns`,
then `app.icns`, then **`None`** → the UI renders the existing monochrome
`Globe`/`Code` lucide icon. An app whose icon lives only in a compiled
`Assets.car` (no `.icns`) is therefore honest-degraded, never broken.

`EditorApp` gains the same optional `icon` field.

## Default-browser detection (for the icon shown when nothing is chosen)

macOS keeps the answer in LaunchServices:

    defaults read com.apple.LaunchServices/com.apple.launchservices.secure LSHandlers
    → { … LSHandlerRoleAll = "com.google.chrome"; LSHandlerURLScheme = https; … }

Parsed by a **pure function** (`parse_default_browser_bundle_id`) so it is unit
testable without a machine that has Chrome: take the `{…}` block containing
`LSHandlerURLScheme = https`, read its `LSHandlerRoleAll`, map the bundle id
(case-insensitively) onto the `BROWSERS` table. Unknown/absent → `None`, and the
UI falls back to the first detected browser. This is a *display* fact only — the
actual open still goes through `open`, which asks LaunchServices itself.

## Surface

### Rust

| File | Change |
|---|---|
| `platform/traits.rs` | `BrowserApp {id,name,icon,systemDefault}`; `EditorApp.icon`; `detect_browsers()`, `open_in_browser()` (default `Unsupported`); `parse_default_browser_bundle_id()` + tests |
| `platform/macos/mod.rs` | `BROWSERS` table, icon extraction + in-process cache, default-browser read, `open_in_browser` with the http(s) guard |
| `commands/system.rs` | `list_browsers`, `open_in_browser`; `open_external` consults `preferred_browser` |
| `lib.rs` | register the two commands |

Windows/Linux keep the `todo!()`/default-empty shape — adding an OS stays
"fill the stubs".

### Frontend

| File | Change |
|---|---|
| `src/types` | `BrowserApp`, `EditorApp.icon` |
| `src/lib/ipc/index.ts` | `listBrowsers()`, `openInBrowser()` |
| `src/lib/useBrowser.ts` | `useBrowsers()`, `usePreferredBrowser()` (preferred → system default → first → null) |
| `src/components/ui/app-icon.tsx` | data-URI `<img>` with the lucide fallback |
| `src/components/ui/split-button.tsx` | primary action + chevron, built on the existing portaled `Menu` |
| `src/routes/SiteDetail.tsx` | header "Open in browser" + Quick-links Browser tile + editor tile get the chevron |
| `src/routes/Sites.tsx` | row globe button shows the browser's icon (no chevron — the row is dense) |
| `src/routes/Settings.tsx` | new "Web browser" row (System default + detected); both pickers show icons |

## Tasks

- [ ] 1. Backend: detection, icons, default read, `open_in_browser` guard,
      `open_external` preference. Unit tests for the LSHandlers parser.
- [ ] 2. Frontend plumbing: types, ipc, `useBrowser`, `AppIcon`.
- [ ] 3. `SplitButton` + SiteDetail header/tiles + Sites row icon.
- [ ] 4. Settings: "Web browser" row + icons on both pickers.
- [ ] 5. Docs (ARCHITECTURE/MAP/DESIGN/CLAIM-LEDGER/TODO) — each in the commit
      that changes the behaviour, not after.

Gate for every commit: `scripts/verify.sh` → `verify: all green`.
