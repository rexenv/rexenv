import { useMutation } from "@tanstack/react-query";
import { Check, RotateCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { retryDownload } from "@/lib/ipc";
import { toastBackendError } from "@/lib/toast";
import type { DownloadItem, DownloadsSnapshot } from "@/types";

/** "1.2 GB" / "45.6 MB" / "312 KB" — download sizes, mono-rendered. */
export function fmtBytes(n: number): string {
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
  if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

/** Whole-percent progress, or null while the total is unknown. */
export function pctOf(item: DownloadItem): number | null {
  if (item.totalBytes == null || item.totalBytes === 0) return null;
  return Math.min(100, Math.floor((item.downloadedBytes / item.totalBytes) * 100));
}

/**
 * The shared progress track (5px, same geometry in EVERY state so phase
 * transitions never shift layout): determinate brand fill when the fraction is
 * known; a pulsing full-width fill for indeterminate (pre-headers bytes=0 /
 * total=null) and `preparing`; a quiet error tint when failed.
 */
export function Track({
  pct,
  state,
}: {
  pct: number | null;
  state: "run" | "ok" | "idle" | "error" | "stopped";
}) {
  return (
    <div className="h-[5px] overflow-hidden rounded-full bg-rex-well">
      {state === "stopped" ? (
        // Ended-early fill FROZEN at pct (install card): never snaps to 100
        // ("error"'s full tint), never resets to 0 ("idle") — the bar stops
        // where the work stopped; the status text says why.
        <div className="h-full rounded-full bg-status-error-bg" style={{ width: `${pct ?? 0}%` }} />
      ) : state === "error" ? (
        <div className="h-full w-full rounded-full bg-status-error-bg" />
      ) : state === "idle" ? null : pct == null ? (
        <div className="h-full w-full animate-pulse rounded-full bg-gradient-to-r from-brand-strong to-brand-light opacity-60 motion-reduce:animate-none" />
      ) : (
        <div
          className="h-full rounded-full bg-gradient-to-r from-brand-strong to-brand-light transition-[width] duration-300"
          style={{ width: `${state === "ok" ? 100 : pct}%` }}
        />
      )}
    </div>
  );
}

function Row({ item }: { item: DownloadItem }) {
  const retry = useMutation({
    mutationFn: () => retryDownload(item.name, item.version),
    // The hub row flips back to `downloading` via events; errors land back on
    // the row too — the toast is just the immediate acknowledgement.
    onError: (e) => toastBackendError(e),
  });
  const pct = pctOf(item);

  // Right-hand status, JetBrains Mono like every technical value.
  const meta = (() => {
    switch (item.phase) {
      case "pending":
        return "queued";
      case "downloading":
        return pct == null ? fmtBytes(item.downloadedBytes) : `${pct}%`;
      case "preparing":
        return "preparing…";
      case "done":
        return "done";
      case "cached":
        return "cached";
      case "failed":
        return "failed";
    }
  })();

  // Second line while downloading: bytes / total · speed · ETA (each part only
  // when actually known — never a guessed number).
  const detail =
    item.phase === "downloading"
      ? [
          item.totalBytes != null
            ? `${fmtBytes(item.downloadedBytes)} / ${fmtBytes(item.totalBytes)}`
            : fmtBytes(item.downloadedBytes),
          item.bytesPerSec != null ? `${fmtBytes(item.bytesPerSec)}/s` : null,
          item.bytesPerSec && item.totalBytes
            ? `${Math.max(1, Math.round((item.totalBytes - item.downloadedBytes) / item.bytesPerSec))}s left`
            : null,
        ]
          .filter(Boolean)
          .join(" · ")
      : null;

  const trackState =
    item.phase === "failed"
      ? "error"
      : item.phase === "done"
        ? "ok"
        : item.phase === "cached" || item.phase === "pending"
          ? "idle"
          : "run";

  return (
    <div className="rounded-[8px] px-2 py-1.5">
      <div className="mb-1 flex items-baseline justify-between gap-2">
        <span
          className={cn(
            "truncate text-[0.75rem]",
            item.phase === "cached" || item.phase === "done"
              ? "text-rex-text-muted"
              : "text-rex-text",
          )}
        >
          {item.label}
        </span>
        <span
          className={cn(
            "flex flex-none items-center gap-1 font-mono text-[0.625rem]",
            item.phase === "failed" ? "text-status-error-bright" : "text-rex-text-muted",
          )}
        >
          {(item.phase === "done" || item.phase === "cached") && (
            <Check className="h-[11px] w-[11px] text-status-running" strokeWidth={2.4} />
          )}
          {meta}
        </span>
      </div>
      <Track pct={pct} state={trackState} />
      {detail && (
        <div className="mt-1 truncate font-mono text-[0.59375rem] text-rex-text-muted">{detail}</div>
      )}
      {item.phase === "failed" && (
        <div className="mt-1.5 flex items-start justify-between gap-2">
          <span
            className="line-clamp-2 flex-1 text-[0.65625rem] leading-[1.4] text-status-error-bright"
            title={item.error ?? undefined}
          >
            {item.error}
          </span>
          <button
            onClick={() => retry.mutate()}
            disabled={retry.isPending}
            className="flex flex-none items-center gap-1 rounded-[6px] border border-rex-border-strong bg-rex-surface-2 px-2 py-1 text-[0.65625rem] font-medium text-rex-text-bright transition-[filter] hover:brightness-110 disabled:opacity-60"
          >
            <RotateCw className={cn("h-[10px] w-[10px]", retry.isPending && "animate-rex-spin")} strokeWidth={2.2} />
            Retry
          </button>
        </div>
      )}
    </div>
  );
}

/**
 * Per-binary download list — popover anchored above the status footer. Data
 * comes down from the single `useDownloads` mount in StatusFooter.
 */
export function DownloadPanel({ snapshot }: { snapshot: DownloadsSnapshot }) {
  return (
    <div className="absolute bottom-full left-2.5 right-2.5 z-50 mb-1.5 rounded-[11px] border border-rex-border-strong bg-rex-surface-2 p-[5px] shadow-menu">
      {snapshot.batch && (
        <div className="flex items-baseline justify-between px-2 pb-1 pt-1.5">
          <span className="text-[0.6875rem] font-semibold text-rex-text-bright">
            {snapshot.batch.action}
          </span>
          <span className="font-mono text-[0.625rem] text-rex-text-muted">
            {snapshot.batch.done}/{snapshot.batch.total}
            {snapshot.batch.failed > 0 && (
              <span className="text-status-error-bright"> · {snapshot.batch.failed} failed</span>
            )}
          </span>
        </div>
      )}
      <div className="max-h-[280px] overflow-y-auto">
        {snapshot.items.map((i) => (
          <Row key={i.id} item={i} />
        ))}
        {snapshot.items.length === 0 && (
          <div className="px-2 py-3 text-center text-[0.6875rem] text-rex-text-muted">
            No downloads this session
          </div>
        )}
      </div>
    </div>
  );
}
