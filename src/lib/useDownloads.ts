import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { downloadsState, onDownloadProgress } from "@/lib/ipc";
import type { DownloadsSnapshot } from "@/types";

const KEY = ["downloads"] as const;
const EMPTY: DownloadsSnapshot = { batch: null, items: [], seq: 0 };

/**
 * THE download-manager state for the UI: seeds from `downloads_state`, then
 * stays live via `download-progress` snapshot events pushed straight into the
 * query cache. Every snapshot is FULL (no merging) and carries the hub's
 * monotonic `seq`, and the cache keeps the higher seq — never the one that
 * merely arrived last.
 *
 * Why the seed is refetched every time a listener mounts (18 Sep 2026, clean
 * VM): the onboarding Install step mounts this hook and unmounts on Continue;
 * the Domains step mounts nothing; the footer mounts it minutes later. Every
 * event in between was lost — nobody was listening — and the footer then read
 * the cache the Install step had left (`staleTime: Infinity`): two rows
 * "downloading 0 B" for a batch that had entirely failed, forever, with
 * nothing left to emit. So: register the listener FIRST (no gap), then
 * refetch the seed (closes the one that just ended), and let `seq` settle
 * which of a racing event and seed is the newer.
 */
export function useDownloads(): DownloadsSnapshot {
  const qc = useQueryClient();
  const { data } = useQuery({
    queryKey: KEY,
    queryFn: async () => newest(qc.getQueryData<DownloadsSnapshot>(KEY), await downloadsState()),
    // Events are the refresh mechanism; never refetch on a timer. Mounting a
    // listener refetches explicitly below.
    staleTime: Infinity,
  });
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void onDownloadProgress((snap) =>
      qc.setQueryData<DownloadsSnapshot>(KEY, (cur) => newest(cur, snap)),
    ).then((f) => {
      if (cancelled) {
        f();
        return;
      }
      unlisten = f;
      // Listener is live — now close the gap that existed before it was.
      void qc.invalidateQueries({ queryKey: KEY });
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [qc]);
  return data ?? EMPTY;
}

/** The higher-seq snapshot wins; a tie keeps the incoming one (same state). */
function newest(cur: DownloadsSnapshot | undefined, next: DownloadsSnapshot): DownloadsSnapshot {
  return cur && cur.seq > next.seq ? cur : next;
}
