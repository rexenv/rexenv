import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Play, Square } from "lucide-react";
import { cn } from "@/lib/utils";
import { startServices, stopServices } from "@/lib/ipc";
import type { GlobalStatus } from "@/types";

const SUMMARY_META = {
  all: { label: "All running", dot: "#3FB950", accent: "#3FB950" },
  partial: { label: "Partial", dot: "#D29922", accent: "#D29922" },
  stopped: { label: "Stopped", dot: "#6E7681", accent: "#6E7681" },
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
  const allRunning = status.summary === "all";

  const qc = useQueryClient();
  const toggleAll = useMutation({
    mutationFn: () => (allRunning ? stopServices() : startServices()),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["services"] });
      qc.invalidateQueries({ queryKey: ["global-status"] });
    },
    // Surface a busy-port / cancelled-prompt / download failure instead of
    // silently doing nothing (§2 robustness).
    onError: (e) => window.alert(String(e)),
  });

  return (
    <div className="flex-none p-2.5 pb-3">
      <div className="overflow-hidden rounded-lg border border-rex-border bg-rex-surface-2">
        <div className="h-0.5" style={{ background: meta.accent }} />
        <div className="p-3">
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <span className="relative inline-flex h-[9px] w-[9px]">
                <span
                  className="absolute inset-0 rounded-full opacity-50 animate-rex-ping"
                  style={{ background: meta.dot }}
                />
                <span
                  className="relative h-[9px] w-[9px] rounded-full"
                  style={{ background: meta.dot }}
                />
              </span>
              <span className="text-[12.5px] font-semibold text-rex-text">
                {meta.label}
              </span>
            </div>
            <span className="font-mono text-[10.5px] text-rex-text-dim">
              {status.running}/{status.total}
            </span>
          </div>

          <div className="mb-3.5 flex flex-col gap-2.5">
            <Meter label="CPU" value={`${status.cpuPercent}%`} pct={status.cpuPercent} />
            <Meter
              label="RAM"
              value={`${(status.ramMb / 1024).toFixed(1)} GB`}
              pct={(status.ramMb / status.ramTotalMb) * 100}
            />
          </div>

          <button
            onClick={() => toggleAll.mutate()}
            disabled={toggleAll.isPending}
            className={cn(
              "flex h-[34px] w-full items-center justify-center gap-2 rounded text-[12.5px] font-medium transition-[filter] hover:brightness-110 focus-visible:outline-none disabled:opacity-60",
              allRunning
                ? "border border-rex-border bg-rex-surface-3 text-rex-text"
                : "bg-primary text-white shadow-glow-primary",
            )}
          >
            {allRunning ? (
              <>
                <Square className="h-3 w-3 fill-current" /> Stop all
              </>
            ) : (
              <>
                <Play className="h-3 w-3 fill-current" /> Start all
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
