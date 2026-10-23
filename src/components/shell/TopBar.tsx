import { Search } from "lucide-react";
import type { ReactNode } from "react";
import { onTitleBarMouseDown } from "@/lib/window-drag";
import { TECH_INPUT } from "@/lib/utils";
import { UpdatePill } from "./UpdatePill";

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
      <div className="min-w-0">
        <div className="text-[1.125rem] font-semibold tracking-[-0.01em] text-rex-text">
          {title}
        </div>
        {subtitle && (
          <div className="mt-[3px] font-mono text-[0.6875rem] text-rex-text-muted">
            {subtitle}
          </div>
        )}
      </div>
      <div className="no-drag flex min-w-0 items-center gap-2.5">
        {/* The search SHRINKS (to 80 px) before anything else does: at the window's
            minimum width (980) the Sites header — three buttons plus the update
            pill — is wider than the bar, and a fixed 190 px input pushed the pill
            past the window's edge, version and all (the 15.8 UTM VM, 16 Oct 2026; the
            review fixture had carried ONE button, so the WebKit check never saw
            the real cluster). 100 px held the app's "Update to 0.9.1"; the longest
            single offer the unified pill can carry, "Update to PHP 8.3.28", overflowed
            by 11 px at that floor (23 Oct 2026), so the floor is 80 — the icon and a
            few typed characters, which is what a filter box at its narrowest is for. */}
        {showSearch && (
          <div className="relative w-[190px] min-w-[80px] shrink">
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
              className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-1 pl-[33px] pr-3 text-[0.8125rem] text-rex-text outline-none transition-colors focus:border-brand"
            />
          </div>
        )}
        {action}
        {/* Rendered by the bar itself, not per route: the update notice is on
            every screen without any route opting in (PLAN-update-notice S1). */}
        <UpdatePill variant="bar" />
      </div>
    </header>
  );
}
