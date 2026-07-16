import type { ReactNode } from "react";

/** Hatched content-region placeholder used while screens are unbuilt. */
export function Placeholder({
  icon,
  label,
  hint,
}: {
  icon: ReactNode;
  label: string;
  hint: string;
}) {
  return (
    <div className="min-h-0 flex-1 p-[18px]">
      <div
        className="flex h-full flex-col items-center justify-center gap-3 rounded-xl border border-rex-well-border bg-rex-well-deep"
        style={{
          backgroundImage:
            "repeating-linear-gradient(45deg,var(--rex-texture-stripe) 0 1px,transparent 1px 12px)",
        }}
      >
        <div className="flex h-[46px] w-[46px] items-center justify-center rounded-[12px] border border-rex-panel-border bg-rex-surface-1 text-[var(--rex-placeholder)]">
          {icon}
        </div>
        <div className="font-mono text-[0.6875rem] uppercase tracking-[0.14em] text-[var(--rex-placeholder)]">
          {label}
        </div>
        <div className="text-[0.8125rem] text-[var(--rex-placeholder-hint)]">{hint}</div>
      </div>
    </div>
  );
}
