import { Search } from "lucide-react";
import type { ReactNode } from "react";
import { onTitleBarMouseDown } from "@/lib/window-drag";
import { TECH_INPUT } from "@/lib/utils";

interface TopBarProps {
  title: string;
  subtitle?: string;
  /** contextual primary action on the right (e.g. "+ New site") */
  action?: ReactNode;
  showSearch?: boolean;
  /** controlled search: when `onSearchChange` is set, the input filters live */
  searchValue?: string;
  onSearchChange?: (value: string) => void;
  searchPlaceholder?: string;
}

export function TopBar({
  title,
  subtitle,
  action,
  showSearch = true,
  searchValue,
  onSearchChange,
  searchPlaceholder,
}: TopBarProps) {
  return (
    <header
      onMouseDown={onTitleBarMouseDown}
      className="drag-region flex h-[84px] flex-none items-center justify-between gap-4 border-b border-rex-border-subtle px-[22px]"
    >
      <div>
        <div className="text-[1.125rem] font-semibold tracking-[-0.01em] text-rex-text">
          {title}
        </div>
        {subtitle && (
          <div className="mt-[3px] font-mono text-[0.6875rem] text-rex-text-muted">
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
            <input {...TECH_INPUT}
              type="text"
              placeholder={searchPlaceholder ?? "Search…"}
              value={onSearchChange ? searchValue ?? "" : undefined}
              onChange={
                onSearchChange ? (e) => onSearchChange(e.target.value) : undefined
              }
              className="h-[34px] w-[190px] rounded border border-rex-border bg-rex-surface-1 pl-[33px] pr-3 text-[0.8125rem] text-rex-text outline-none transition-colors focus:border-brand"
            />
          </div>
        )}
        {action}
      </div>
    </header>
  );
}
