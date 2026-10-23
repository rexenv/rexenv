import { useCallback } from "react";
import { useNavigate } from "react-router-dom";

/** Settings → Updates: where every kind of update lives, and the one place that installs
 *  (`docs/PLAN-unified-updates.md` D3). Still exported under its old name so the three doors
 *  that import it did not all change in the same commit as the page; the value is the page. */
export const ABOUT_PATH = "/settings?section=updates";

/** Updates, ASKED to check: the page sees `check=1`, runs its own "Check now" once,
 *  and strips the flag (`UpdatesSection`). */
export const CHECK_FLAG = "check";
export const ABOUT_CHECK_PATH = `${ABOUT_PATH}&${CHECK_FLAG}=1`;

/** The About card's "Check now" mutation key. The TopBar pill reads it through
 *  `useIsMutating` to draw its spinner — it observes the card's check, it never
 *  starts one itself. */
export const APP_UPDATE_CHECK_MUTATION = ["app-update-check"] as const;

/** "Check for Updates…" from anywhere: go to the card and have it CHECK.
 *
 *  ONE hook, three doors — the macOS app menu, the tray (all three OSes, through
 *  the same `menu://check-updates` event) and the TopBar's idle button — so what
 *  "the user asked" does is decided in one place (`docs/archive/PLAN-update-notice.md`).
 *
 *  It used to navigate and INVALIDATE `["app-update"]` — but that query is
 *  `app_update_state`, a pure read, so every door landed on the stored answer
 *  and fetched nothing (found 15 Oct 2026; owner ruling the same day: the click
 *  opens About AND runs the check). The check itself stays the card's: it owns
 *  the mutation, its failure copy and its 6 s deadline, so this only asks via
 *  `check=1` — a second caller of the check would be a second place deciding
 *  what a failed check looks like. */
export function useCheckForUpdates(): () => void {
  const navigate = useNavigate();
  return useCallback(() => {
    navigate(ABOUT_CHECK_PATH);
  }, [navigate]);
}
