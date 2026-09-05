# PLAN — proving the WebKit/wry dialog claims (#166) and the custom-scheme redirect premise (#40)

Status: measured 15 Aug 2026, legs A/B built the same day; leg C is a SMOKE addition.
Both ledger rows carried a `🔨 L2` verdict assigned BEFORE anyone measured whether an
L2 check can even see the subject. It cannot — and building one anyway would have been
the string-proven kind again (the dotfile guards went eight months as a config
substring; the lesson here is the same one, a layer up).

## 1. The measurement: where does each claim's SUBJECT live?

An L2 wk-check is Playwright's WebKit with mocked IPC. Two facts about that harness
decide everything below, and both were checkable without writing a test:

- **Playwright WebKit has its own dialog machinery.** `window.confirm()` in the
  harness is answered by Playwright's `page.on('dialog')` layer, not by a
  WKUIDelegate — wry's missing panel methods, `class_addMethod`, the delegate
  re-set dance: none of it exists in that process. A wk-check "for" #166 would
  measure Playwright's dialog handling, pass forever, and prove nothing about the
  packaged app. (`docs/TESTING.md` §L2 already states the general limit: it proves
  render, never container behaviour.)
- **Playwright cannot register a `WKURLSchemeTask` scheme.** `rexdb://` requests in
  the harness are just an unknown scheme; `page.route()` interception is Playwright's
  network layer with its own redirect semantics. #40's premise — "WKWebView never
  follows a redirect returned by a custom-scheme handler" — is a fact about an API
  the harness does not contain. (`docs/TESTING.md` §L2: "real cookies/schemes
  (`rexdb://` doesn't exist in Playwright)" — the ledger's L2 label contradicted the
  testing doc and nobody had noticed.)

So: **zero of the stacked claims in #166/#40 are L2-observable.** The rows get
re-verdicted to the layers that can actually see them.

## 2. #166, decomposed — five claims, three layers

The module doc stacks five claims. Per claim, the layer whose process contains the
subject:

| Claim | Subject lives in | Layer |
|---|---|---|
| (a) `class_addMethod` is additive-only; a wry that ships its own panels wins automatically | the ObjC runtime | **L0** — lib test against the REAL runtime (macOS-only file, so no cfg gymnastics). Leg A, built. |
| (b) wry always sets a UI delegate; ours extends it; the three selectors then answer; the file-picker method is untouched; the re-set delegate survives | WebKit + the objc runtime, no UI needed | **L1** — an example on the main thread with a real `WKWebView` and a wry-shaped stub delegate. Leg B, built (`webview_dialogs_check`, sandbox tier). |
| (c) wry ≤0.55 implements no JS-dialog panel methods | wry's delegate class, only in a real tauri app | **L1, same example** — assert on wry's actual delegate class if reachable; otherwise the packaged app. Leg B asserts the mechanism on a stub; the wry-specific half is covered by (e): if wry grows panels, `class_addMethod` returns false and ours defer BY (a). |
| (d) a sheet renders; `confirm()` returns the button pressed; WebKit suspends the calling frame's JS | a native NSAlert a human can see | **L3 only.** Clicking a native sheet programmatically = synthetic events (standing directive: none on this machine). SMOKE gets the exact incident regression: Adminer drop-table confirm, Cancel then OK. Leg C. |
| (e) a future wry with native panels wins automatically | composition of (a)+(b) | proven by parts — (a) proves the mechanism, (b) proves ours install only where absent. |

**What legs A+B deliberately do NOT claim:** that a dialog is visible or answerable.
That is (d), it needs an eye, and pretending an automated leg covers it is how the
"three stacked wry/WebKit claims, zero tests" row would become "three stacked claims,
two vacuous tests".

## 3. #40, re-verdicted

The claim at `adminer.rs` ("WKWebView does NOT follow redirects returned by a
custom-scheme handler — `WKURLSchemeTask` has no redirect mechanism") is load-bearing
in the SAFE direction: the proxy follows redirects server-side and stays on the vhost
(#42, proven); anything else — off-vhost Location, >5 hops — is replayed to the
webview, where the design leans on WebKit dead-ending it as a blank frame rather than
navigating.

- The OUR-CODE half (server-side following, on-vhost only, POST→GET semantics) is
  already L0-proven (#42 `relative_locations_resolve_on_the_vhost`).
- The PREMISE half is a fact about WebKit's scheme-handler API in the packaged app.
  No automated layer in this repo contains it → **🚫, mapped to SMOKE** like every
  other third-party premise. The existing SMOKE §Database deep-link flow exercises
  Adminer's POST-redirect-GET on every login (if server-side following broke, login
  blank-frames immediately); leg C's drop-table step adds the confirm path. A WebKit
  that STARTED following custom-scheme redirects would change the failure mode of the
  off-vhost replay only — worth a sentence in SMOKE, not a fabricated check.
- Scope note, dated: the premise was true of WebKit as shipped through macOS 15
  (observed live in the packaged app when the module was written). It is a statement
  about Apple's API surface, re-checked implicitly by every release's SMOKE run.

## 4. What was built (this session)

- **Leg A** — `webview_dialogs.rs` test
  `class_add_method_is_additive_only_so_a_wry_with_native_panels_wins`: real ObjC
  runtime, throwaway class; control first (adding to a class WITHOUT the selector
  must succeed — a broken registration would otherwise read as "additive-only"),
  then the second add with a DIFFERENT imp must fail AND the first imp must still be
  the one installed. The control is the canary; the imp identity check is what makes
  "fails harmlessly" mean *harmlessly*.
- **Leg B** — `examples/webview_dialogs_check.rs` (sandbox tier): real `WKWebView`,
  stub delegate class implementing ONLY wry's file-picker selector, then
  `install_js_dialog_panels`. Asserts: all three panel selectors answered by the
  delegate's class afterwards; the file-picker imp is byte-identical before/after
  (ours never replace); the SAME delegate object is still installed after the
  re-set dance (a dropped delegate would kill the file picker silently); and a
  second install is a no-op (idempotent — the real app can call it per-window).
  Spawns nothing, binds nothing, shows NO dialogs.
- **Leg C** — SMOKE §Database gains the confirm-gated drop step (the 2026 incident
  that motivated the module: confirm-gated buttons silently no-oping).

## 5. Rejected shapes, so they stay rejected

- **An L2 wk-check for either row** — subject absent from the harness (§1). This was
  the queued idea; the measurement is why it dies.
- **Driving the native sheet with synthetic clicks** (osascript/CGEvent) — forbidden
  on this machine by standing directive, and fragile everywhere.
- **A tauri `mock_app` leg** — mock_app builds no real webview; the delegate under
  test would be ours all the way down.
