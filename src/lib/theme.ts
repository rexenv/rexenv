/**
 * App theme (Dark / Light / System) — §4.4 / DESIGN_BRIEF §172.
 *
 * The preference is a pure UI setting stored in localStorage so it can be applied
 * synchronously before first paint (no flash). "system" follows the OS via
 * `prefers-color-scheme`. Applying = setting `data-theme` on <html>, which swaps
 * the token palette in styles/tokens.css.
 */

export type Theme = "dark" | "light" | "system";

const KEY = "rexenv.theme";

/** The stored preference (defaults to "system"). */
export function getStoredTheme(): Theme {
  const v = typeof localStorage !== "undefined" ? localStorage.getItem(KEY) : null;
  return v === "dark" || v === "light" || v === "system" ? v : "system";
}

function prefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-color-scheme: dark)").matches
  );
}

/** Resolve a preference to the concrete palette to apply. */
function resolved(theme: Theme): "dark" | "light" {
  if (theme === "system") return prefersDark() ? "dark" : "light";
  return theme;
}

/** Apply a theme to the document (sets `data-theme` on <html>). */
export function applyTheme(theme: Theme): void {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.theme = resolved(theme);
  // Notify from HERE, not only from `setTheme`: under "system" the OS moving is
  // a theme change nobody chose, and anything mirroring the palette OUTSIDE
  // this document — the Adminer console, which renders in its own process —
  // would otherwise never hear about it.
  for (const fn of listeners) fn();
}

/** The concrete palette in force right now — what `data-theme` says, which is
 *  what the token layer is actually painting with. */
export function currentTheme(): "dark" | "light" {
  const applied = typeof document !== "undefined" ? document.documentElement.dataset.theme : null;
  return applied === "dark" || applied === "light" ? applied : resolved(getStoredTheme());
}

/** Subscribers notified whenever the stored preference changes (any control). */
const listeners = new Set<() => void>();

/** Subscribe to preference changes; returns an unsubscribe. Shape fits
 *  `useSyncExternalStore(subscribeTheme, getStoredTheme)` so every theme
 *  control (Settings tiles, sidebar toggle) shares one source of truth. */
export function subscribeTheme(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

/** Persist + apply a theme choice. */
export function setTheme(theme: Theme): void {
  if (typeof localStorage !== "undefined") localStorage.setItem(KEY, theme);
  applyTheme(theme); // notifies
}

/**
 * Apply the stored theme and keep "system" in sync with the OS. Call once at
 * startup (before render). Returns nothing; the media listener lives for the app's
 * lifetime.
 */
export function initTheme(): void {
  applyTheme(getStoredTheme());
  window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener?.("change", () => {
    if (getStoredTheme() === "system") applyTheme("system");
  });
}
