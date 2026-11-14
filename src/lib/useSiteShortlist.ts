import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { setSitePinned, siteShortlist } from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import type { Site, SiteShortlist } from "@/types";

export const SITE_SHORTLIST_KEY = ["site-shortlist"] as const;
const EMPTY: SiteShortlist = { pinned: [], recent: [], openedAt: {} };

/** The person's pinned and recently opened sites, in core's order. One query key
 *  for every surface that shows them (switcher, Sites list, sidebar, site page),
 *  so a pin made in one is on screen in the others without a second read. */
export function useSiteShortlist(): SiteShortlist {
  const { data } = useQuery({ queryKey: SITE_SHORTLIST_KEY, queryFn: siteShortlist });
  return data ?? EMPTY;
}

/** Pin / unpin. A scratch site is refused by core, and the refusal is its own
 *  sentence — the UI does not offer the pin there, so this is the backstop. */
export function usePinSite() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ site, pinned }: { site: Site; pinned: boolean }) => setSitePinned(site.id, pinned),
    onSuccess: (_, { site, pinned }) => {
      void qc.invalidateQueries({ queryKey: SITE_SHORTLIST_KEY });
      toast.success(pinned ? `${site.name} pinned — it is in the sidebar now.` : `${site.name} unpinned.`);
    },
    onError: (e) => toastBackendError(e),
  });
}

/** Split a list of sites the way every surface groups them: pinned (core's
 *  order), then recently opened (core's order), then the rest as given. A site
 *  is in exactly one group. */
export function groupByShortlist<T extends { id: string }>(
  sites: T[],
  list: SiteShortlist,
): { pinned: T[]; recent: T[]; rest: T[] } {
  const byId = new Map(sites.map((s) => [s.id, s]));
  const pick = (ids: string[]) => ids.map((id) => byId.get(id)).filter((s): s is T => !!s);
  const taken = new Set([...list.pinned, ...list.recent]);
  return {
    pinned: pick(list.pinned),
    recent: pick(list.recent),
    rest: sites.filter((s) => !taken.has(s.id)),
  };
}
