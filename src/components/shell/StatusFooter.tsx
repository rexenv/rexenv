import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toastBackendError } from "@/lib/toast";
import { Play, Square } from "lucide-react";
import { cn } from "@/lib/utils";
import { startServices, stopServices } from "@/lib/ipc";
import type { GlobalStatus } from "@/types";

// Per-summary visuals. `accent` is the translucent top strip; `glow` is the
// dot's box-shadow; `labelClass` dims the label when everything is stopped.
const SUMMARY_META = {
  all: {
    label: "All running",
    dot: "var(--rex-running)",
    accent: "var(--rex-running-soft)",
    glow: "var(--rex-glow-run)",
    labelClass: "text-rex-text",
  },
  partial: {
    label: "Partial",
    dot: "var(--rex-warning)",
    accent: "var(--rex-warning-soft)",
    glow: "var(--rex-glow-warn)",
    labelClass: "text-rex-text",
  },
  stopped: {
    label: "Stopped",
    dot: "var(--rex-stopped)",
    accent: "var(--rex-stopped-soft)",
    glow: "none",
    labelClass: "text-rex-text-muted",
  },
} as const;

function Meter({ label, value, pct }: { label: string; value: string; pct: number }) {
  return (
    <div>
      <div className="mb-1.5 flex justify-between">
        <span className="font-mono text-[10px] tracking-wide text-rex-text-dim">
          {label}
        </span>
        <span className="font-mono text-[10.5px] text-rex-text-bright">{value}</span>
      </div>
      <div className="h-[5px] overflow-hidden rounded-full bg-rex-well">
        <div
          className="h-full rounded-full bg-gradient-to-r from-brand-strong to-brand-light transition-[width] duration-700"
          style={{ width: `${Math.min(100, pct)}%` }}
        />
      </div>
    </div>
  );
}

export function StatusFooter({ status }: { status: GlobalStatus }) {
  const meta = SUMMARY_META[status.summary];
  // Design rule: only "Start all" (primary) when nothing runs; any running
  // service ⇒ "Stop all" (secondary). The dot pulses only when something runs.
  const isStart = status.running === 0;
  const pulsing = status.running > 0;

  const qc = useQueryClient();
  const toggleAll = useMutation({
    mutationFn: () => (isStart ? startServices() : stopServices()),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["services"] });
      qc.invalidateQueries({ queryKey: ["global-status"] });
    },
    // Surface a busy-port / cancelled-prompt / download failure instead of
    // silently doing nothing (§2 robustness).
    onError: (e) => toastBackendError(e),
  });

  return (
    <div className="flex-none p-2.5 pb-3">
      <div className="overflow-hidden rounded-lg border border-rex-border bg-rex-surface-2">
        <div className="h-0.5" style={{ background: meta.accent }} />
        <div className="p-3">
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <span className="relative inline-flex h-[9px] w-[9px]">
                {pulsing && (
                  <span
                    className="absolute inset-0 rounded-full opacity-50 animate-rex-ping motion-reduce:animate-none"
                    style={{ background: meta.dot }}
                  />
                )}
                <span
                  className="relative h-[9px] w-[9px] rounded-full"
                  style={{ background: meta.dot, boxShadow: meta.glow }}
                />
              </span>
              <span className={cn("text-[12.5px] font-semibold", meta.labelClass)}>
                {meta.label}
              </span>
            </div>
            <span className="font-mono text-[10.5px] text-rex-text-dim">
              {status.running}/{status.total}
            </span>
          </div>

          {/* rexenv's OWN usage (sum of every supervised process tree — masters
              + workers + the root edge), NOT the whole machine. The CPU value is
              a per-core sum (Activity-Monitor style, can exceed 100); the meter
              fill is its 0-100 machine share (÷ cores). RAM fill = share of
              machine RAM. */}
          <div
            className="mb-3.5 flex flex-col gap-2.5"
            title="rexenv services only (all their processes, workers included) — not the whole machine"
          >
            <Meter
              label="CPU"
              value={`${status.cpuPercent.toFixed(1)}%`}
              pct={status.cpuCores ? status.cpuPercent / status.cpuCores : 0}
            />
            <Meter
              label="RAM"
              value={
                status.ramMb >= 1024
                  ? `${(status.ramMb / 1024).toFixed(1)} GB`
                  : `${status.ramMb} MB`
              }
              pct={status.ramTotalMb ? (status.ramMb / status.ramTotalMb) * 100 : 0}
            />
          </div>

          <button
            onClick={() => toggleAll.mutate()}
            disabled={toggleAll.isPending}
            className={cn(
              "flex h-[34px] w-full items-center justify-center gap-2 rounded text-[12.5px] font-medium transition-[background-color,border-color,filter] hover:brightness-110 focus-visible:outline-none disabled:opacity-60",
              isStart
                ? "bg-primary text-white shadow-glow-primary"
                : "border border-rex-border-strong bg-rex-surface-2 text-rex-text-bright",
            )}
          >
            {isStart ? (
              <>
                <Play className="h-3 w-3 fill-current" /> Start all
              </>
            ) : (
              <>
                <Square className="h-3 w-3 fill-current" /> Stop all
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
