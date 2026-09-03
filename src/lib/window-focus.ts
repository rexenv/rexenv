import { focusManager } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Teach TanStack Query what "the user came back" means in a Tauri window.
 *
 * Its default focus source is the DOM (`visibilitychange` + window `focus`).
 * Inside wry/WKWebView that fires unreliably — the webview keeps thinking it is
 * visible and focused while the whole app sits behind a browser window — so
 * `refetchOnWindowFocus` was effectively dead and the WordPress panels showed
 * yesterday's truth: activate a plugin in wp-admin, switch back to rexenv, and
 * the list still said Inactive until you left the tab and returned.
 *
 * The native window's own focus event is the fact that is actually true, so it
 * becomes the source. DOM events stay as a fallback for the dev-server/browser
 * case (and for `wk-checks`, which drives a real browser, not the app shell).
 */
export function initWindowFocus(): void {
  focusManager.setEventListener((handleFocus) => {
    // ONE source at a time. The two used to feed the same latch: the DOM fires
    // `visibilitychange`/`focus` unreliably in WKWebView (it can report
    // focused while the whole app sits behind a browser), and TanStack dedupes
    // `setFocused` on change — so a stray DOM `true` while the window was
    // backgrounded made the REAL native `true` on return a no-change, and the
    // refetch this module exists for (#254/#255) never ran. Native when there
    // is a Tauri window; DOM only when there is not.
    let stopNative: (() => void) | null = null;
    let dropped = false;
    let native = false;
    // Outside the app shell (plain `vite dev` in a browser, the /dev review
    // routes, wk-checks) there is no Tauri window at all — and `getCurrentWindow`
    // THROWS SYNCHRONOUSLY there (it reads `__TAURI_INTERNALS__.metadata`), which
    // a `.catch()` on the promise never sees. That threw during app bootstrap and
    // rendered a blank page; the DOM listeners below are the whole story there, so
    // this stays a downgrade, never an error. (Caught by `wk-checks/repopanel.js`,
    // which is exactly the browser context this must survive.)
    try {
      const win = getCurrentWindow();
      native = true;
      void win
        .onFocusChanged(({ payload: focused }) => handleFocus(focused))
        .then((un) => {
          if (dropped) un();
          else stopNative = un;
        })
        .catch(() => {});
    } catch {
      /* no Tauri window here */
    }

    // Browser fallback: `blur` too, because an alt-tab at the WINDOW level
    // fires blur/focus and never `visibilitychange`, so a focus-only listener
    // could only ever say "true" and the latch never reset.
    const onDom = () => handleFocus(document.visibilityState !== "hidden" && document.hasFocus());
    if (!native) {
      window.addEventListener("visibilitychange", onDom, false);
      window.addEventListener("focus", onDom, false);
      window.addEventListener("blur", onDom, false);
    }

    return () => {
      dropped = true;
      if (!native) {
        window.removeEventListener("visibilitychange", onDom);
        window.removeEventListener("focus", onDom);
        window.removeEventListener("blur", onDom);
      }
      stopNative?.();
    };
  });
}
