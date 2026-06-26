import { Search } from "lucide-react";
import type { ReactNode } from "react";

interface TopBarProps {
  title: string;
  subtitle?: string;
  /** contextual primary action on the right (e.g. "+ New site") */
  action?: ReactNode;
  showSearch?: boolean;
}

export function TopBar({ title, subtitle, action, showSearch = true }: TopBarProps) {
  return (
    <header className="drag-region flex h-[84px] flex-none items-center justify-between gap-4 border-b border-rex-border-subtle px-[22px]">
      <div className="no-drag">
        <div className="text-[18px] font-semibold tracking-[-0.01em] text-rex-text">
          {title}
        </div>
        {subtitle && (
          <div className="mt-[3px] font-mono text-[11px] text-rex-text-dim">
            {subtitle}
          </div>
        )}
      </div>
      <div className="no-drag flex items-center gap-2.5">
        {showSearch && (
          <div className="relative">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 h-[15px] w-[15px] -translate-y-1/2 text-rex-text-dim"
              strokeWidth={1.9}
            />
            <input
              type="text"
              placeholder="Search…"
              className="h-[34px] w-[210px] rounded border border-rex-border bg-rex-surface-1 pl-[33px] pr-3 text-[13px] text-rex-text outline-none transition-colors focus:border-brand"
            />
          </div>
        )}
        {action}
      </div>
    </header>
  );
}
