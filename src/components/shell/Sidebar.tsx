import { NavLink } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import { NAV_ITEMS, type NavItem } from "./nav";
import { StatusFooter } from "./StatusFooter";
import { getGlobalStatus } from "@/lib/ipc";
import { mockGlobalStatus } from "@/lib/mock";

function CrownMark() {
  return (
    <div className="flex h-[26px] w-[26px] flex-none items-center justify-center rounded-lg border border-rex-border-strong bg-gradient-to-br from-[#20232C] to-[#13151B]">
      <svg width="15" height="15" viewBox="0 0 24 24" className="block">
        <path
          d="M3 8.4 L8 12.6 L12 5 L16 12.6 L21 8.4 L19.1 18.7 L4.9 18.7 Z"
          fill="#7C5CFF"
          stroke="#7C5CFF"
          strokeWidth="1.1"
          strokeLinejoin="round"
        />
        <circle cx="12" cy="5" r="1.5" fill="#C9BCFF" />
      </svg>
    </div>
  );
}

function NavButton({ item }: { item: NavItem }) {
  const Icon = item.icon;
  return (
    <NavLink
      to={item.to}
      className={({ isActive }) =>
        cn(
          "relative flex w-full items-center gap-2.5 rounded-[9px] py-2 pl-3 pr-2.5 text-left text-[13.5px] transition-colors",
          isActive
            ? "bg-white/[0.055] text-rex-text"
            : "text-rex-text-muted hover:bg-white/[0.045]",
        )
      }
    >
      {({ isActive }) => (
        <>
          <span
            className={cn(
              "absolute left-0 top-1/2 h-[17px] w-[3px] -translate-y-1/2 rounded-r-[3px]",
              isActive ? "bg-brand" : "bg-transparent",
            )}
          />
          <Icon className="h-[17px] w-[17px] flex-none" strokeWidth={1.7} />
          <span className="flex-1">{item.label}</span>
          {item.badge && (
            <span className="font-mono text-[10.5px] text-rex-text-faint">
              {item.badge}
            </span>
          )}
        </>
      )}
    </NavLink>
  );
}

export function Sidebar() {
  const main = NAV_ITEMS.filter((i) => !i.footer);
  const footer = NAV_ITEMS.filter((i) => i.footer);
  const groups: NavItem["group"][] = ["Environment", "Network"];

  // Live system CPU/RAM + running/total, polled every 2s (mock fallback in dev).
  const { data: status } = useQuery({
    queryKey: ["global-status"],
    queryFn: getGlobalStatus,
    refetchInterval: 2000,
    initialData: mockGlobalStatus,
  });

  return (
    <aside className="flex w-[220px] flex-none flex-col border-r border-rex-border-subtle bg-rex-surface-1">
      {/* Header: wordmark (drag region). The macOS traffic lights are the real
          OS controls — the window uses titleBarStyle Overlay, so we reserve the
          top row for them instead of drawing fake dots. */}
      <div className="drag-region flex h-[84px] flex-none flex-col justify-center gap-3.5 border-b border-rex-border-subtle px-[18px]">
        <div className="h-3" aria-hidden />
        <div className="flex items-center gap-2.5">
          <CrownMark />
          <span className="font-display text-[16.5px] font-semibold tracking-[-0.02em] text-rex-text">
            rexenv
          </span>
        </div>
      </div>

      {/* Nav */}
      <nav className="flex flex-1 flex-col gap-0.5 overflow-auto p-2.5">
        {groups.map((group) => (
          <div key={group} className="contents">
            <div className="mb-1.5 mt-1.5 px-3.5 font-mono text-[10px] uppercase tracking-[0.13em] text-rex-text-label first:mt-1.5">
              {group}
            </div>
            {main
              .filter((i) => i.group === group)
              .map((item) => (
                <NavButton key={item.to} item={item} />
              ))}
          </div>
        ))}
        <div className="flex-1" />
        {footer.map((item) => (
          <NavButton key={item.to} item={item} />
        ))}
      </nav>

      <StatusFooter status={status} />
    </aside>
  );
}
