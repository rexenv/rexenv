/** Live site-provision card (New Site dialog + Sites-route re-adoption).
 *  Honesty rules (same family as the install card):
 *  - the bar is PHASE-WEIGHTED observed progress (`job.pct`, backend-computed):
 *    real phase completions plus the download Hub's REAL byte fraction during
 *    fetch — never a time estimate. Monotonic; 99-capped until settle-ok; on
 *    failure/cancel/timeout it FREEZES where the work stopped;
 *  - the phase line is the BACKEND'S own step label (deterministic Rust
 *    boundaries); the sub-detail is wp-cli's last output line VERBATIM, or
 *    the live byte row (`bytes / total · speed`) while binaries download —
 *    those bytes come straight from the Hub snapshot, never re-derived, so a
 *    Range-resume visibly CONTINUES instead of restarting at 0;
 *  - silence is shown honestly: the ticker counts from the last SIGNAL (a
 *    line or byte movement), so a parked bar always has a second voice. */
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import type { DownloadsSnapshot, SiteProvisionState } from "@/types";
import { fmtBytes, pctOf, Track } from "@/components/shell/DownloadPanel";
import { LogPane } from "@/components/wordpress/repoJobUi";
import {
  onSiteProvisionOutput,
  onSiteProvisionState,
  siteProvisionActive,
  tailLog,
} from "@/lib/ipc";

const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

const END_COPY: Partial<Record<SiteProvisionState["status"], string>> = {
  cancelled:
    "Cancelled — the site stays in the list as “setup incomplete”: Retry re-runs the remaining steps, Delete removes it. A binary download in flight finishes into the cache (nothing is wasted).",
  timed_out:
    "Timed out (outer guard) — a wedged step was killed. The site stays as “setup incomplete”: Retry re-runs the remaining steps.",
};

/** Start/adopt a provision job and stream its state + log lines. Shared by
 *  the New Site dialog (start) and the Sites route (adopt after the dialog
 *  closes — the job survives unmount). */
export function useSiteProvision(onSettled?: (job: SiteProvisionState) => void) {
  const qc = useQueryClient();
  const [job, setJob] = useState<SiteProvisionState | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const settledCb = useRef(onSettled);
  settledCb.current = onSettled;
  useEffect(() => {
    if (!job?.id) return;
    let dead = false;
    const un: Array<() => void> = [];
    void onSiteProvisionState(job.id, (s) => {
      if (dead) return;
      setJob(s);
      if (s.status !== "running") {
        // Any settle can have changed the list (row inserted at start; the
        // provisioned flag flips on ok).
        qc.invalidateQueries({ queryKey: ["sites"] });
        settledCb.current?.(s);
      }
    }).then((u) => un.push(u));
    void onSiteProvisionOutput(job.id, (l) => {
      if (!dead) setLines((x) => [...x.slice(-499), l]);
    }).then((u) => un.push(u));
    return () => {
      dead = true;
      un.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [job?.id]);
  return {
    job,
    lines,
    running: job?.status === "running",
    start: (snap: SiteProvisionState) => {
      setJob(snap);
      setLines([]);
    },
    adopt: async (domain?: string) => {
      const j = await siteProvisionActive(domain).catch(() => null);
      if (j) {
        setJob(j);
        setLines(await tailLog(j.logKey, 300).catch(() => []));
      }
      return j;
    },
    clear: () => {
      setJob(null);
      setLines([]);
    },
  };
}

export function SiteProvisionCard({
  job,
  lines,
  downloads,
  onCancel,
}: {
  job: SiteProvisionState;
  lines: string[];
  /** The app-wide downloads snapshot (from `useDownloads()` in the host) —
   *  the card filters it to `job.downloadIds` for the fetch byte row. */
  downloads?: DownloadsSnapshot;
  onCancel: () => void;
}) {
  const [logOpen, setLogOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const lastSignalAt = useRef(Date.now());
  const logRef = useRef<HTMLDivElement | null>(null);
  const running = job.status === "running";
  const phase = job.phases[job.phaseCursor];
  const fetching = running && phase?.key === "fetch";
  const mine = (downloads?.items ?? []).filter((i) => job.downloadIds.includes(i.id));
  // Byte movement is a signal too — the fetch phase prints no LINES for
  // minutes while genuinely streaming; the ticker must not call that silence.
  const byteSum = mine.reduce((n, i) => n + i.downloadedBytes, 0);

  useEffect(() => {
    lastSignalAt.current = Date.now();
  }, [lines.length, byteSum]);
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(t);
  }, [running]);
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines, logOpen]);

  const silentFor = Math.floor((now - lastSignalAt.current) / 1000);
  const lastLine = lines.length > 0 ? lines[lines.length - 1] : null;
  const detail = running
    ? (lastLine ?? "starting…")
    : (job.summary ?? job.error ?? job.status); // END_COPY renders separately below
  const failedish = job.status === "failed" || job.status === "timed_out";
  // Non-ok settles FREEZE the bar where the work stopped ("stopped" = a
  // pct-width tint) — never full, never empty.
  const trackState = running ? "run" : job.status === "ok" ? "ok" : "stopped";

  return (
    <div className="rounded-md border border-rex-border bg-rex-surface-1 px-2.5 py-2">
      <div className="flex items-center gap-2">
        {running ? (
          <Loader2 className="h-3.5 w-3.5 flex-none animate-rex-spin text-brand" />
        ) : (
          <span
            className={`w-3.5 flex-none text-center font-mono text-[0.75rem] ${
              job.status === "ok"
                ? "text-status-running-bright"
                : failedish
                  ? "text-status-error-bright"
                  : "text-rex-text-muted"
            }`}
          >
            {job.status === "ok" ? "✓" : job.status === "cancelled" ? "–" : "✕"}
          </span>
        )}
        <span className="min-w-0 flex-1 truncate font-mono text-[0.71875rem] text-rex-text">
          creating {job.domain}
        </span>
        {running && phase && (
          <span className="flex-none font-mono text-[0.6875rem] text-rex-text-muted">
            {phase.label}
          </span>
        )}
        {!running && failedish && phase && (
          <span className="flex-none font-mono text-[0.6875rem] text-status-error-bright">
            failed at: {phase.label}
          </span>
        )}
        {/* Cancel visible from the START — the escape for the long network
            phases; never hidden behind the log toggle. */}
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
        <Track pct={job.status === "ok" ? 100 : job.pct} state={trackState} />
      </div>
      {/* Fetch sub-rows: REAL bytes per binary, straight from the Hub
          snapshot (a 206 resume visibly continues — never zeroed here). */}
      {fetching &&
        mine
          .filter((i) => i.phase === "downloading" || i.phase === "preparing" || i.phase === "pending")
          .map((i) => (
            <div
              key={i.id}
              className="mt-1 flex items-center gap-2 font-mono text-[0.65625rem] text-rex-text-muted"
            >
              <span className="min-w-0 flex-1 truncate">{i.label}</span>
              <span className="flex-none">
                {i.phase === "preparing"
                  ? "preparing…"
                  : i.totalBytes != null
                    ? `${fmtBytes(i.downloadedBytes)} / ${fmtBytes(i.totalBytes)}${
                        i.bytesPerSec != null ? ` · ${fmtBytes(i.bytesPerSec)}/s` : ""
                      }`
                    : fmtBytes(i.downloadedBytes)}
              </span>
              <div className="w-[90px] flex-none">
                <Track pct={pctOf(i)} state="run" />
              </div>
            </div>
          ))}
      <div
        className={`mt-1.5 truncate font-mono text-[0.6875rem] ${
          !running && failedish ? "text-status-error-bright" : "text-rex-text-muted"
        }`}
        title={detail ?? undefined}
      >
        {detail}
      </div>
      {running && silentFor >= 10 && (
        <div className="mt-0.5 font-mono text-[0.625rem] text-rex-text-dim">
          waiting on {phase?.label ?? "the current step"} · no output for {silentFor}s (long
          steps print nothing until they finish — Cancel is safe)
        </div>
      )}
      {!running && END_COPY[job.status] && (
        <div className="mt-1 whitespace-pre-line text-[0.6875rem] text-rex-text-muted">
          {END_COPY[job.status]}
        </div>
      )}
      {logOpen && <LogPane lines={lines} innerRef={logRef} />}
    </div>
  );
}
