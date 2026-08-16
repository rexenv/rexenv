/** Live install-progress card — shared by BOTH add sources (wp.org slugs and
 *  "Upload zip"; one backend job, one card). Honesty rules (B25 —
 *  wp-cli is opaque mid-download, no byte signal exists):
 *  - the bar is PHASE-based (`job.pct`, computed backend-side): each tick is
 *    a line wp-cli actually printed — observed discrete progress, NOT the
 *    byte-level estimate the B25 rule bans (don't "fix" back to
 *    indeterminate). Monotonic; 99-capped until the terminal summary; on
 *    failure/cancel/timeout it FREEZES where it stopped — never snaps to
 *    100, never resets;
 *  - phase label = wp-cli's last output line VERBATIM;
 *  - "installing item k of N" is an ATTEMPT cursor, never "k done" — bar and
 *    cursor advance on the same header lines, one story. A ZIP job prints no
 *    such headers at all, so its cursor can never move: the card omits it
 *    rather than parking it at "1 of N" (nothing beats a stale something);
 *  - silence is shown honestly ("no output for Ns" — the download phase can
 *    legitimately sit for minutes printing nothing, and a bar PARKED at a
 *    percentage reads as frozen without it) with Cancel as the escape,
 *    visible from the moment the job starts. */
import { useEffect, useRef, useState } from "react";
import { Loader2 } from "lucide-react";
import type { WpInstallState } from "@/types";
import { Track } from "@/components/shell/DownloadPanel";
import { LogPane } from "./repoJobUi";

const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

/** What to CALL each item on screen. A zip job's items are absolute paths —
 *  the file name is the only part a person recognises, and a full
 *  `/Users/…/Downloads/…` in a toast pushes the outcome off the end of the
 *  line. The full path stays in the log (and in `job.slugs`), never invented
 *  or shortened there. */
export function installLabels(job: WpInstallState): string[] {
  if (job.source !== "zip") return job.slugs;
  return job.slugs.map((p) => p.split("/").filter(Boolean).pop() ?? p);
}

const END_COPY: Partial<Record<WpInstallState["status"], string>> = {
  cancelled:
    "Cancelled — the current item may remain installed (inactive); leftover temp files may sit in wp-content/upgrade.",
  timed_out:
    "Timed out (outer guard) — wp-cli did not finish; a wedged process was killed. The list below shows what actually installed.",
};

export function WpInstallCard({
  job,
  lines,
  onCancel,
}: {
  job: WpInstallState;
  lines: string[];
  onCancel: () => void;
}) {
  const [logOpen, setLogOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const lastLineAt = useRef(Date.now());
  const logRef = useRef<HTMLDivElement | null>(null);
  const running = job.status === "running";

  useEffect(() => {
    lastLineAt.current = Date.now();
  }, [lines.length]);
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(t);
  }, [running]);
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines, logOpen]);

  const silentFor = Math.floor((now - lastLineAt.current) / 1000);
  const lastLine = lines.length > 0 ? lines[lines.length - 1] : null;
  const label = running
    ? (lastLine ?? "starting wp-cli…")
    : (job.summary ?? job.error ?? END_COPY[job.status] ?? job.status);
  const failedish = job.status === "failed" || job.status === "timed_out";
  // Every non-ok settle FREEZES the bar where the work stopped ("stopped"
  // renders a pct-width tint) — never full ("error"), never empty ("idle").
  const trackState = running ? "run" : job.status === "ok" ? "ok" : "stopped";

  return (
    <div className="mt-2 rounded-md border border-rex-border bg-rex-surface-1 px-2.5 py-2">
      <div className="flex items-center gap-2">
        {running ? (
          <Loader2 className="h-3.5 w-3.5 flex-none animate-rex-spin text-brand" />
        ) : (
          <span
            className={`w-3.5 flex-none text-center font-mono text-[0.75rem] ${
              job.status === "ok"
                ? "text-status-running-bright"
                : job.status === "partial" || failedish
                  ? "text-status-error-bright"
                  : "text-rex-text-muted"
            }`}
          >
            {job.status === "ok" ? "✓" : job.status === "cancelled" ? "–" : "✕"}
          </span>
        )}
        <span
          className="min-w-0 flex-1 truncate font-mono text-[0.71875rem] text-rex-text"
          title={job.source === "zip" ? job.slugs.join("\n") : undefined}
        >
          {job.kind} install{job.source === "zip" ? " (zip)" : ""} ·{" "}
          {installLabels(job).join(" ")}
        </span>
        {/* Zip jobs print no per-item header, so the cursor would sit at "1 of
            N" for the whole batch — a number that stopped being true. Nothing
            beats a stale something. */}
        {running && job.source !== "zip" && job.itemsTotal > 1 && (
          <span className="flex-none font-mono text-[0.6875rem] text-rex-text-muted">
            installing item {Math.min(Math.max(job.itemCursor, 1), job.itemsTotal)} of{" "}
            {job.itemsTotal}
          </span>
        )}
        {/* Cancel is the escape for wp-cli's silent stretches — visible from
            the START, never hidden behind the log toggle. */}
        {running && (
          <button className={BTN} onClick={onCancel}>
            Cancel
          </button>
        )}
        <button
          className="flex-none text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
          onClick={() => setLogOpen((v) => !v)}
        >
          {logOpen ? "Hide log" : "Show log"}
        </button>
      </div>
      <div className="mt-1.5">
        {/* ok → 100 (exit-0 belt); every other settle shows pct FROZEN where
            the job stopped — ended-early is the status', not the bar's, job. */}
        <Track pct={job.status === "ok" ? 100 : job.pct} state={trackState} />
      </div>
      <div
        className={`mt-1.5 truncate font-mono text-[0.6875rem] ${
          !running && (failedish || job.status === "partial")
            ? "text-status-error-bright"
            : "text-rex-text-muted"
        }`}
        title={label ?? undefined}
      >
        {label}
      </div>
      {running && silentFor >= 10 && (
        <div className="mt-0.5 font-mono text-[0.625rem] text-rex-text-muted">
          waiting on wp-cli · no output for {silentFor}s (downloads print nothing until they
          finish — Cancel is safe)
        </div>
      )}
      {!running && job.status === "partial" && (
        <div className="mt-1 text-[0.6875rem] text-status-warning-bright">
          Some items DID install — the list below shows the real state.
        </div>
      )}
      {!running && END_COPY[job.status] && job.summary == null && job.error == null && (
        <div className="mt-1 whitespace-pre-line text-[0.6875rem] text-rex-text-muted">
          {END_COPY[job.status]}
        </div>
      )}
      {logOpen && <LogPane lines={lines} innerRef={logRef} />}
    </div>
  );
}
