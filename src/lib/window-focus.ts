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
    const onDom = () => handleFocus(document.visibilityState !== "hidden");
    window.addEventListener("visibilitychange", onDom, false);
    window.addEventListener("focus", onDom, false);

    let stopNative: (() => void) | null = null;
    let dropped = false;
    // Outside the app shell (plain `vite dev` in a browser, the /dev review
    // routes, wk-checks) there is no Tauri window at all — and `getCurrentWindow`
    // THROWS SYNCHRONOUSLY there (it reads `__TAURI_INTERNALS__.metadata`), which
    // a `.catch()` on the promise never sees. That threw during app bootstrap and
    // rendered a blank page; the DOM listeners above are the whole story there, so
    // this stays a downgrade, never an error. (Caught by `wk-checks/repopanel.js`,
    // which is exactly the browser context this must survive.)
    try {
      void getCurrentWindow()
        .onFocusChanged(({ payload: focused }) => handleFocus(focused))
        .then((un) => {
          if (dropped) un();
          else stopNative = un;
        })
        .catch(() => {});
    } catch {
      /* no Tauri window here */
    }

    return () => {
      dropped = true;
      window.removeEventListener("visibilitychange", onDom);
      window.removeEventListener("focus", onDom);
      stopNative?.();
    };
  });
}
