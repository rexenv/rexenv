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
import { Loader2, X } from "lucide-react";
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
  onDismiss,
  onHoldChange,
  onReplace,
}: {
  job: WpInstallState;
  lines: string[];
  onCancel: () => void;
  /** Clear the card. Offered on every SETTLED job, not only failed ones: a
   *  successful card clears itself after three seconds, but opening its log
   *  holds that timer, and a card held open with no way to close it is a
   *  panel the user is stuck with. */
  onDismiss?: () => void;
  /** True while the log pane is open — the parent stops the success timer, so
   *  reading the log is never a race against it. */
  onHoldChange?: (held: boolean) => void;
  /** Re-run this job with `--force`, given the directory that blocked it. Only
   *  ever offered when wp-cli said that directory is in the way. */
  onReplace?: (dir: string) => void;
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
  useEffect(() => {
    onHoldChange?.(logOpen);
    // Releasing on unmount matters: the panel unmounts on every sub-tab
    // switch, and a hold left set there would keep a settled card forever.
    return () => onHoldChange?.(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [logOpen]);

  const silentFor = Math.floor((now - lastLineAt.current) / 1000);
  const lastLine = lines.length > 0 ? lines[lines.length - 1] : null;
  const label = running
    ? (lastLine ?? "starting wp-cli…")
    : (job.summary ?? job.error ?? END_COPY[job.status] ?? job.status);
  const failedish = job.status === "failed" || job.status === "timed_out";
  // The backend read it off the stream and put it on the state — one parser,
  // in one place, shared with the toast (which never receives the log at all).
  const blocked = running ? null : job.blockedBy;
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
                : blocked
                  ? "text-status-warning-bright"
                  : job.status === "partial" || failedish
                    ? "text-status-error-bright"
                    : "text-rex-text-muted"
            }`}
          >
            {/* A blocked job is not a break: nothing installed AND nothing was
                touched, and there is a one-click way forward under it. Red ✕
                says something went wrong; this says something is in the way. */}
            {job.status === "ok" ? "✓" : blocked ? "!" : job.status === "cancelled" ? "–" : "✕"}
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
        {/* Only once the job has SETTLED: dismissing a running install would
            hide work that is still happening, which is the one thing this card
            exists to prevent. */}
        {!running && onDismiss && (
          <button
            type="button"
            onClick={onDismiss}
            aria-label="Dismiss install result"
            title="Dismiss"
            className="flex h-4 w-4 flex-none items-center justify-center rounded text-rex-text-muted transition-colors hover:text-rex-text"
          >
            <X className="h-3 w-3" />
          </button>
        )}
      </div>
      <div className="mt-1.5">
        {/* ok → 100 (exit-0 belt); every other settle shows pct FROZEN where
            the job stopped — ended-early is the status', not the bar's, job. */}
        <Track pct={job.status === "ok" ? 100 : job.pct} state={trackState} />
      </div>
      <div
        className={`mt-1.5 truncate font-mono text-[0.6875rem] ${
          !running && (failedish || job.status === "partial") && !blocked
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
      {/* wp-cli refused because the folder is already there. That is not a
          failure a user can act on from the summary line ("No plugins
          installed."), so the card says WHICH folder and offers the same way
          out wp-admin does. */}
      {!running && blocked && onReplace && (
        <div className="mt-1.5 flex items-center gap-2">
          <span className="min-w-0 flex-1 text-[0.6875rem] text-rex-text-muted">
            <span className="font-mono text-rex-text">{blocked}</span> is already installed —
            nothing was unpacked.
          </span>
          <button className={BTN} onClick={() => onReplace(blocked)}>
            Replace with the uploaded zip
          </button>
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
