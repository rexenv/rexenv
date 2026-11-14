import { create } from "zustand";
import { useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { getSitesServing, servicesStatus, startServices } from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import type { Site } from "@/types";

/** How long a site may take to answer after Start all before the click gives up. */
const SERVE_WAIT_MS = 60_000;

/** One start at a time, app-wide: every "Start & …" control reads the same flag,
 *  so a second click on another row cannot start the stack twice. */
const useStarting = create<{ siteId: string | null; set: (id: string | null) => void }>((set) => ({
  siteId: null,
  set: (siteId) => set({ siteId }),
}));

function refresh(qc: QueryClient) {
  for (const key of ["services", "global-status", "sites-serving", "databases"])
    void qc.invalidateQueries({ queryKey: [key] });
}

/** **"Start & …" — the click that starts rexenv's services, then does what it said.**
 *
 * Owner, 14 Nov 2026: people press Magic Login without pressing Start all first,
 * and got an error and a browser on "can't connect". The control used to be
 * disabled with "Start all first" (the clean-VM lesson of 18 Sep 2026: never open
 * a URL nothing serves) — honest, and a dead end. So the control now NAMES the
 * start ("Start & Magic Login") and does both.
 *
 * Three promises ride on it (ledger #969):
 * - **Nothing opens unless the site is serving.** `ensure` resolves `true` only
 *   after the backend's own serving answer says this site answers; a failed
 *   start (a busy port, a cancelled password prompt) shows Start all's own error
 *   and opens nothing.
 * - **A site the USER stopped is never started by this.** That switch is the
 *   person's decision; callers do not offer "Start & …" there.
 * - **The start is said before it happens.** No caller runs `ensure` behind a
 *   label that does not say "Start" — seven services and, on macOS, possibly a
 *   password prompt are not a side effect of "Magic Login". */
export function useServeFirst() {
  const qc = useQueryClient();
  const startingId = useStarting((s) => s.siteId);
  const setStarting = useStarting((s) => s.set);

  /** Start all. `false` (with the backend's error shown) when it did not start. */
  const startStack = async (): Promise<boolean> => {
    try {
      await startServices();
      refresh(qc);
      return true;
    } catch (e) {
      refresh(qc); // a failed start may have left a partial stack — show it
      toastBackendError(e);
      return false;
    }
  };

  /** Start all, then wait until THIS site answers. `true` only then. */
  const ensure = async (site: Pick<Site, "id" | "domain">): Promise<boolean> => {
    if (useStarting.getState().siteId) return false; // a start is already running
    setStarting(site.id);
    try {
      if (!(await startStack())) return false;
      const deadline = Date.now() + SERVE_WAIT_MS;
      while (Date.now() < deadline) {
        const serving = await getSitesServing().catch(() => []);
        if (serving.find((s) => s.domain === site.domain)?.serving) {
          refresh(qc);
          return true;
        }
        await new Promise((r) => setTimeout(r, 500));
      }
      toast.error(
        `rexenv's services started, but ${site.domain} is not answering yet — nothing was opened. The Services page shows what is still down.`,
      );
      return false;
    } finally {
      setStarting(null);
    }
  };

  return { ensure, startStack, startingId, busy: startingId !== null };
}

/** Is the edge (the router every site is reached through) running? `undefined`
 *  until the first answer. It tells "rexenv's services are off" from "the user
 *  stopped this one site" for a site that is not serving. */
export function useEdgeUp(): boolean | undefined {
  const { data } = useQuery({ queryKey: ["services"], queryFn: servicesStatus, refetchInterval: 2000 });
  return data ? !!data.find((s) => s.isRouter)?.running : undefined;
}
