import { useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";

/** Settings → About: where the update card lives, and the one place that installs. */
export const ABOUT_PATH = "/settings?section=about";

/** The About card's "Check now" mutation key. The TopBar pill reads it through
 *  `useIsMutating` to draw its spinner — it observes the card's check, it never
 *  starts one. */
export const APP_UPDATE_CHECK_MUTATION = ["app-update-check"] as const;

/** "Check for Updates…" from anywhere: go to the card AND re-read its state.
 *
 *  ONE hook, three doors — the macOS app menu, the tray (all three OSes, through
 *  the same `menu://check-updates` event) and the TopBar's idle button — so what
 *  "the user asked" does is decided in one place (`docs/PLAN-update-notice.md`).
 *  It INVALIDATES rather than running the check: the card owns that mutation and
 *  its failure copy, and a second caller would be a second place deciding what a
 *  failed check looks like. */
export function useCheckForUpdates(): () => void {
  const navigate = useNavigate();
  const qc = useQueryClient();
  return useCallback(() => {
    navigate(ABOUT_PATH);
    void qc.invalidateQueries({ queryKey: ["app-update"] });
    void qc.invalidateQueries({ queryKey: ["app-update-readiness"] });
  }, [navigate, qc]);
}
