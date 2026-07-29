import type { AgentAction, AgentOutcome } from "@/types";
import { cn } from "@/lib/utils";

const OUTCOME_LABEL: Record<AgentOutcome, string> = {
  ok: "ok",
  error: "error",
  denied: "denied",
  "unknown-tool": "unknown tool",
  "bad-request": "bad request",
};

/** Relative time from a SQLite UTC stamp ("YYYY-MM-DD HH:MM:SS" — no zone, so
 *  read it as UTC explicitly; a naive `new Date` would treat it as local). */
export function timeAgo(at: string): string {
  const t = Date.parse(at.replace(" ", "T") + "Z");
  if (Number.isNaN(t)) return at;
  const secs = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (secs < 45) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.round(hrs / 24)}d ago`;
}

/**
 * The agent activity list — shared by the Settings card and the per-site
 * SiteDetail section. A concerning row (any non-ok outcome) gets a muted-amber
 * treatment: a thin left accent and an amber outcome label — visible without a
 * loud alarm fill, since a read-only agent erroring is a signal, not an incident.
 */
export function AgentActivityFeed({ rows, empty }: { rows: AgentAction[]; empty: string }) {
  if (rows.length === 0) {
    return <div className="px-1 py-5 text-center text-[0.75rem] text-rex-text-dim">{empty}</div>;
  }
  return (
    <div className="flex flex-col">
      {rows.map((r) => (
        <div
          key={r.id}
          className={cn(
            "flex items-center gap-2.5 border-b border-rex-border-subtle py-2 last:border-b-0",
            r.concerning && "border-l-2 border-l-status-warning pl-2.5",
          )}
        >
          <span className="font-mono text-[0.71875rem] text-rex-text">{r.tool}</span>
          {r.targetSite && (
            <span className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
              → {r.targetSite}
            </span>
          )}
          <span
            className={cn(
              "font-mono text-[0.6875rem]",
              r.concerning ? "text-status-warning-bright" : "text-rex-text-dim",
            )}
            title={r.detail ?? undefined}
          >
            {OUTCOME_LABEL[r.outcome]}
          </span>
          <span className="ml-auto flex flex-none items-center gap-1.5 text-[0.6875rem] text-rex-text-dim">
            <span className="max-w-[130px] truncate" title={r.client}>
              {r.client}
            </span>
            <span aria-hidden>·</span>
            <span title={r.at}>{timeAgo(r.at)}</span>
          </span>
        </div>
      ))}
    </div>
  );
}
