/** Drains the startup-notice queue ONCE and toasts each notice. A notice's door
 *  (label + route) comes from the backend producer and is rendered as sent — this
 *  component attached "Open Tunnels" to every `warn` until 23 Oct 2026, which put a
 *  Tunnels button under the self-update's "did not take" warning
 *  (`docs/PLAN-unified-updates.md` T0, ledger #901). Lives in its own file so the
 *  WebKit harness can mount it (`DevUiReview` `view=notices`). */
import { useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { startupNotices } from "@/lib/ipc";
import { toast } from "@/lib/toast";

export function StartupNoticeHost() {
  const shown = useRef(false);
  const navigate = useNavigate();
  useEffect(() => {
    if (shown.current) return; // StrictMode double-mount must not drain twice
    shown.current = true;
    startupNotices()
      .then((list) => {
        for (const n of list) {
          // There is no "warning" toast kind, and `error` would be a lie — the
          // app did the right thing. A notice WITH a door gets the ACTION form:
          // it names where to look and stays up 10s rather than 4, which is the
          // difference between telling someone and technically having told them.
          if (n.action) {
            const { label, route } = n.action;
            toast.info(n.message, { label, onClick: () => navigate(route) });
          } else {
            toast.info(n.message);
          }
        }
      })
      .catch(() => {
        // A failed drain loses a courtesy, never a record: the same facts are
        // in rexenv.log, and a toast is not worth an error screen.
      });
    // `navigate` is stable for the app's lifetime and the ref guard makes a
    // re-run a no-op anyway; it is listed so the hooks rule stays a rule.
  }, [navigate]);
  return null;
}
