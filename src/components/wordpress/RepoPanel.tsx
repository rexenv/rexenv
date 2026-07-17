/** Per-asset repo panel (phase A): opened by clicking a row's git badge.
 *  Shows the checkout's live state — branch (or detached / no commits),
 *  working-tree summary, ahead/behind vs upstream, remote, provenance
 *  source — plus the last add-job log inline. Read-only in phase A; the
 *  git-ops buttons (fetch/pull/checkout/push) land in phase B on the row
 *  this layout reserves. */
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import { repoAssetStatus, tailLog } from "@/lib/ipc";
import type { GitAsset } from "@/types";

function Chip({ children, tone }: { children: React.ReactNode; tone?: "warn" | "ok" }) {
  const color =
    tone === "warn"
      ? "text-status-warning-bright"
      : tone === "ok"
        ? "text-status-running-bright"
        : "text-rex-text-muted";
  return (
    <span className={`rounded-full bg-rex-surface-2 px-2 py-0.5 font-mono text-[0.6875rem] ${color}`}>
      {children}
    </span>
  );
}

export function RepoPanel({
  siteId,
  kind,
  asset,
}: {
  siteId: string;
  kind: "plugin" | "theme";
  asset: GitAsset;
}) {
  const qc = useQueryClient();
  const [logOpen, setLogOpen] = useState(false);
  const statusKey = ["repo-status", siteId, kind, asset.dirName] as const;
  const status = useQuery({
    queryKey: statusKey,
    queryFn: () => repoAssetStatus(siteId, kind, asset.dirName),
    staleTime: 10_000,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const log = useQuery({
    queryKey: ["repo-status-log", siteId, kind, asset.dirName],
    queryFn: () => tailLog(status.data?.logKey ?? "", 200),
    enabled: logOpen && !!status.data?.logKey,
    staleTime: 0,
  });

  const s = status.data;
  const headRef = s
    ? s.unborn
      ? "no commits yet"
      : s.detached
        ? "detached HEAD"
        : (s.branch ?? "?")
    : null;
  const clean = s ? s.changed === 0 && s.untracked === 0 : false;

  return (
    <div className="mx-3 mb-2.5 rounded-lg border border-rex-border bg-rex-surface-2/50 px-3 py-2.5">
      {status.isLoading ? (
        <div className="flex items-center gap-2 text-[0.75rem] text-rex-text-muted">
          <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> Reading checkout state…
        </div>
      ) : status.isError ? (
        <div className="whitespace-pre-line font-mono text-[0.6875rem] text-status-error-bright">
          {String(status.error)}
        </div>
      ) : s ? (
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <Chip tone={s.detached ? "warn" : undefined}>⎇ {headRef}</Chip>
            <Chip tone={clean ? "ok" : "warn"}>
              {clean
                ? "clean"
                : [
                    s.changed > 0 ? `${s.changed} changed` : null,
                    s.untracked > 0 ? `${s.untracked} untracked` : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
            </Chip>
            {s.upstream ? (
              <Chip>
                ↑{s.ahead ?? 0} ↓{s.behind ?? 0} vs {s.upstream}
              </Chip>
            ) : (
              !s.unborn && <Chip tone="warn">no upstream</Chip>
            )}
            <Chip>{asset.source}</Chip>
            <button
              className="ml-auto flex items-center gap-1 text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
              onClick={() => qc.invalidateQueries({ queryKey: statusKey })}
              title="Re-read git status"
            >
              <RefreshCw className="h-3 w-3" /> Refresh
            </button>
          </div>
          <div className="truncate font-mono text-[0.6875rem] text-rex-text-dim">
            {s.remote ?? (asset.url || "(no remote)")}
            {asset.gitRef ? ` · added @ ${asset.gitRef}` : ""}
          </div>
          {s.logKey && (
            <button
              className="text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
              onClick={() => setLogOpen((v) => !v)}
            >
              {logOpen ? "Hide last job log" : "Show last job log"}
            </button>
          )}
          {logOpen && (
            <div className="max-h-[180px] overflow-y-auto rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1.5">
              {(log.data ?? []).length === 0 ? (
                <div className="font-mono text-[0.6875rem] text-rex-text-muted">(empty)</div>
              ) : (
                (log.data ?? []).map((l, i) => (
                  <div
                    key={i}
                    className="whitespace-pre-wrap break-all font-mono text-[0.6875rem] leading-[1.5] text-rex-text-muted"
                  >
                    {l}
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
}
