import { cn } from "@/lib/utils";
import type { ServiceStatus } from "@/types";

const STATUS_META: Record<
  ServiceStatus,
  { label: string; dot: string; text: string; fill: string }
> = {
  running: {
    label: "Running",
    dot: "bg-status-running",
    text: "text-status-running-bright",
    fill: "bg-status-running-bg border-status-running-border",
  },
  stopped: {
    label: "Stopped",
    dot: "bg-status-stopped",
    text: "text-rex-text-muted",
    fill: "bg-status-stopped-bg border-status-stopped-border",
  },
  starting: {
    label: "Starting",
    dot: "bg-status-warning",
    text: "text-status-warning-bright",
    fill: "bg-status-warning-bg border-status-warning-border",
  },
  error: {
    label: "Error",
    dot: "bg-status-error",
    text: "text-status-error-bright",
    fill: "bg-status-error-bg border-status-error-border",
  },
};

export function StatusPill({ status }: { status: ServiceStatus }) {
  const meta = STATUS_META[status];
  return (
    <span
      className={cn(
        "inline-flex items-center gap-2 rounded-full border py-[5px] pl-2.5 pr-3",
        meta.fill,
      )}
    >
      <span className="relative inline-flex h-[9px] w-[9px]">
        {status === "running" && (
          <span
            className={cn(
              "absolute inset-0 rounded-full opacity-50 animate-rex-ping",
              meta.dot,
            )}
          />
        )}
        <span className={cn("relative h-[9px] w-[9px] rounded-full", meta.dot)} />
      </span>
      <span className={cn("text-xs font-medium", meta.text)}>{meta.label}</span>
    </span>
  );
}
