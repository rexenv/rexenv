import { cn } from "@/lib/utils";
import type { ServiceStatus } from "@/types";

const STATUS_META: Record<
  ServiceStatus,
  { label: string; text: string; fill: string }
> = {
  running: {
    label: "Running",
    text: "text-status-running-bright",
    fill: "bg-status-running-bg border-status-running-border",
  },
  stopped: {
    label: "Stopped",
    text: "text-rex-text-muted",
    fill: "bg-status-stopped-bg border-status-stopped-border",
  },
  starting: {
    label: "Starting",
    text: "text-status-warning-bright",
    fill: "bg-status-warning-bg border-status-warning-border",
  },
  error: {
    label: "Error",
    text: "text-status-error-bright",
    fill: "bg-status-error-bg border-status-error-border",
  },
};

/** The status marker: running pulses + glows, starting spins, error pulses. */
function StatusMarker({ status }: { status: ServiceStatus }) {
  if (status === "starting") {
    // spinner ring (amber), no dot
    return (
      <span className="h-[11px] w-[11px] rounded-full border-2 border-status-warning/30 border-t-status-warning animate-rex-spin motion-reduce:animate-none" />
    );
  }
  if (status === "running") {
    return (
      <span className="relative inline-flex h-[9px] w-[9px]">
        <span className="absolute inset-0 rounded-full bg-status-running opacity-50 animate-rex-ping motion-reduce:animate-none" />
        <span className="relative h-[9px] w-[9px] rounded-full bg-status-running shadow-glow-run" />
      </span>
    );
  }
  if (status === "error") {
    return (
      <span className="h-[9px] w-[9px] rounded-full bg-status-error shadow-glow-err animate-rex-err motion-reduce:animate-none" />
    );
  }
  return <span className="h-[9px] w-[9px] rounded-full bg-status-stopped" />;
}

export function StatusPill({
  status,
  className,
  label,
}: {
  status: ServiceStatus;
  className?: string;
  /** override the default label (e.g. "Idle" instead of "Stopped"). */
  label?: string;
}) {
  const meta = STATUS_META[status];
  return (
    <span
      className={cn(
        "inline-flex items-center gap-2 rounded-full border py-[5px] pl-2.5 pr-3",
        meta.fill,
        className,
      )}
    >
      <StatusMarker status={status} />
      <span className={cn("text-xs font-medium", meta.text)}>{label ?? meta.label}</span>
    </span>
  );
}
