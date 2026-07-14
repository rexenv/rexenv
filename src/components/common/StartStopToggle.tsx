import { cn } from "@/lib/utils";

/**
 * A pill switch. `status` (default) uses green for the ON state (running);
 * `setting` uses violet for non-status settings (autostart, multisite, …).
 * Pass `label` so the aria-label names what's being toggled.
 */
export function StartStopToggle({
  running,
  busy,
  disabled,
  title,
  onToggle,
  variant = "status",
  label,
}: {
  running: boolean;
  busy?: boolean;
  /** Permanently non-interactive (vs `busy` = in-flight). Pair with `title`
   *  so the tooltip says WHY the control is locked. */
  disabled?: boolean;
  title?: string;
  onToggle: () => void;
  variant?: "status" | "setting";
  label?: string;
}) {
  const onTrack =
    variant === "setting"
      ? "bg-brand border-brand-light shadow-glow-primary"
      : "bg-toggle-on border-toggle-on-border shadow-glow-run";
  return (
    <button
      type="button"
      role="switch"
      aria-checked={running}
      aria-label={label ?? (running ? "Turn off" : "Turn on")}
      aria-disabled={disabled || undefined}
      title={title}
      disabled={busy || disabled}
      onClick={onToggle}
      className={cn(
        "relative h-[27px] w-[46px] flex-none rounded-full border transition-[background-color,border-color] duration-200 focus-visible:outline-none focus-visible:shadow-[0_0_0_3px_var(--rex-focus-ring)]",
        running ? onTrack : "bg-toggle-off border-toggle-off-border",
        busy && "opacity-40",
        disabled && "cursor-not-allowed opacity-45",
      )}
    >
      <span
        className={cn(
          "absolute left-[2px] top-[2px] h-[21px] w-[21px] rounded-full shadow-[0_1px_3px_rgba(0,0,0,0.5)] transition-transform duration-200 ease-[cubic-bezier(.4,0,.2,1)]",
          running ? "translate-x-[19px] bg-white" : "bg-rex-text-bright",
        )}
      />
    </button>
  );
}
