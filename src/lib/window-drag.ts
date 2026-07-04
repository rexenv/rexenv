import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MouseEvent } from "react";

/**
 * Elements inside a drag region that must keep receiving clicks instead of
 * starting a window drag. `.no-drag` is the explicit opt-out for anything
 * not covered by the tag/role list (e.g. a custom action passed to TopBar).
 */
const INTERACTIVE_SELECTOR =
  'button, a, input, select, textarea, [role="button"], .no-drag';

/**
 * Tauri webviews don't support Electron's `-webkit-app-region: drag`; window
 * dragging must be started imperatively. Attach to `onMouseDown` of a
 * title-bar element (pair with the `.drag-region` class for the CSS side).
 * Double-click toggles maximize, matching native title bars.
 */
export function onTitleBarMouseDown(e: MouseEvent<HTMLElement>) {
  if (e.button !== 0) return;
  if ((e.target as Element).closest(INTERACTIVE_SELECTOR)) return;
  e.preventDefault();
  const win = getCurrentWindow();
  if (e.detail === 2) {
    void win.toggleMaximize();
  } else {
    void win.startDragging();
  }
}
