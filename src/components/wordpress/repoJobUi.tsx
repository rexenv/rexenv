/** Shared primitives for repo-job UIs (GitAddPanel add-jobs, RepoPanel git
 *  ops). Presentational + pure only — each panel owns its own state. */
import { Loader2 } from "lucide-react";
import type { RepoStepState } from "@/types";

/** The consent line — sits ABOVE every button that runs repo scripts. */
export const REPO_SCRIPTS_DISCLOSURE =
  "These run the repo's own scripts (npm postinstall, composer scripts) as your user — " +
  "same as running them in Terminal. Install only code you trust.";

/** Step-status glyph — plain text + color, no animation surprises in WKWebView
 *  (only the running state spins, via the same Loader2 the app already uses). */
export function StepDot({ status }: { status: RepoStepState["status"] }) {
  if (status === "running") return <Loader2 className="h-3.5 w-3.5 animate-rex-spin text-brand" />;
  const glyph =
    status === "ok" ? "✓" : status === "failed" ? "✕" : status === "cancelled" ? "–" : "○";
  const color =
    status === "ok"
      ? "text-status-running-bright"
      : status === "failed"
        ? "text-status-error-bright"
        : "text-rex-text-muted";
  return <span className={`w-3.5 text-center font-mono text-[0.75rem] ${color}`}>{glyph}</span>;
}

/** Merge a log-file tail (authoritative up to its read moment) with lines that
 *  streamed in while the tail was being fetched: drop the streamed prefix that
 *  already appears at the tail's end (the sink writes the file BEFORE emitting,
 *  so an overlapping line is a duplicate, not new output). */
export function mergeTailAndStreamed(tail: string[], streamed: string[]): string[] {
  const max = Math.min(tail.length, streamed.length, 50);
  for (let k = max; k > 0; k--) {
    if (tail.slice(-k).every((l, i) => l === streamed[i])) {
      return [...tail, ...streamed.slice(k)];
    }
  }
  return [...tail, ...streamed];
}

/** Scrolling mono log pane (plain styled div — not a terminal). */
export function LogPane({
  lines,
  innerRef,
}: {
  lines: string[];
  innerRef?: React.Ref<HTMLDivElement>;
}) {
  return (
    <div
      ref={innerRef}
      className="mt-2 max-h-[220px] overflow-y-auto rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1.5"
    >
      {lines.length === 0 ? (
        <div className="font-mono text-[0.6875rem] text-rex-text-muted">(no output yet)</div>
      ) : (
        lines.map((l, i) => (
          <div
            key={i}
            className="whitespace-pre-wrap break-all font-mono text-[0.6875rem] leading-[1.5] text-rex-text-muted"
          >
            {l}
          </div>
        ))
      )}
    </div>
  );
}
