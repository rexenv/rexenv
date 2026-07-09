import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { downloadsState, onDownloadProgress } from "@/lib/ipc";
import type { DownloadsSnapshot } from "@/types";

const KEY = ["downloads"] as const;
const EMPTY: DownloadsSnapshot = { batch: null, items: [] };

/**
 * THE download-manager state for the UI: seeds from `downloads_state` on
 * mount, then stays live via `download-progress` snapshot events pushed
 * straight into the query cache (full snapshots — no merging, no ordering
 * races). Mount ONCE (StatusFooter) and pass the snapshot down as props;
 * never derive download state anywhere else.
 */
export function useDownloads(): DownloadsSnapshot {
  const qc = useQueryClient();
  const { data } = useQuery({
    queryKey: KEY,
    queryFn: downloadsState,
    // Events are the refresh mechanism; never refetch on a timer.
    staleTime: Infinity,
  });
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void onDownloadProgress((snap) => qc.setQueryData(KEY, snap)).then((f) => {
      if (cancelled) f();
      else unlisten = f;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [qc]);
  return data ?? EMPTY;
}
