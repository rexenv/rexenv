import { cn } from "@/lib/utils";
import type { ServiceStatus } from "@/types";

const STATUS_META: Record<
  ServiceStatus,
  { label: string; dot: string; text: string }
> = {
  running: { label: "Running", dot: "bg-status-running", text: "text-status-running" },
  stopped: { label: "Stopped", dot: "bg-status-stopped", text: "text-rex-text-dim" },
  starting: { label: "Starting", dot: "bg-status-warning", text: "text-status-warning" },
  error: { label: "Error", dot: "bg-status-error", text: "text-status-error" },
};

export function StatusPill({ status }: { status: ServiceStatus }) {
  const meta = STATUS_META[status];
  return (
    <span className="inline-flex items-center gap-2 rounded-full border border-rex-border bg-rex-surface-2 px-2.5 py-1">
      <span className="relative inline-flex h-[7px] w-[7px]">
        {status === "running" && (
          <span
            className={cn(
              "absolute inset-0 rounded-full opacity-50 animate-rex-ping",
              meta.dot,
            )}
          />
        )}
        <span className={cn("relative h-[7px] w-[7px] rounded-full", meta.dot)} />
      </span>
      <span className={cn("text-xs font-medium", meta.text)}>{meta.label}</span>
    </span>
  );
}
