import { cn } from "@/lib/utils";

/** A start/stop switch — green when running. Drives the backend on toggle. */
export function StartStopToggle({
  running,
  busy,
  onToggle,
}: {
  running: boolean;
  busy?: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={running}
      aria-label={running ? "Stop site" : "Start site"}
      disabled={busy}
      onClick={onToggle}
      className={cn(
        "relative h-5 w-9 flex-none rounded-full transition-colors focus-visible:outline-none",
        running ? "bg-status-running" : "bg-rex-surface-3 border border-rex-border",
        busy && "opacity-50",
      )}
    >
      <span
        className={cn(
          "absolute top-1/2 h-3.5 w-3.5 -translate-y-1/2 rounded-full bg-white shadow transition-all",
          running ? "left-[18px]" : "left-[3px]",
        )}
      />
    </button>
  );
}
