import { NavLink } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import { onTitleBarMouseDown } from "@/lib/window-drag";
import { NAV_ITEMS, type NavItem } from "./nav";
import { StatusFooter } from "./StatusFooter";
import { ThemeToggle } from "./ThemeToggle";
import { databasesStatus, getGlobalStatus, listSites, mailpitMessages, tunnelsStatus } from "@/lib/ipc";
import type { GlobalStatus } from "@/types";

/** Neutral placeholder until the first real `global_status` poll — all-zero so the
 *  footer + nav badges never flash fake numbers on first paint. */
const ZERO_STATUS: GlobalStatus = {
  summary: "stopped",
  running: 0,
  total: 0,
  cpuPercent: 0,
  ramMb: 0,
  ramTotalMb: 0,
};

/** A nav badge value: shown only when the count is > 0 (a nav full of "0"s is noise). */
function badgeCount(n: number): string | undefined {
  return n > 0 ? String(n) : undefined;
}

function CrownMark() {
  return (
    <div className="flex h-[26px] w-[26px] flex-none items-center justify-center rounded-[8px] border border-[var(--rex-crown-border)] bg-gradient-to-br from-[var(--rex-crown-chip-from)] to-[var(--rex-crown-chip-to)] shadow-glow-crown">
      <svg width="15" height="15" viewBox="0 0 24 24" className="block">
        <path
          d="M3 8.4 L8 12.6 L12 5 L16 12.6 L21 8.4 L19.1 18.7 L4.9 18.7 Z"
          style={{ fill: "var(--rex-brand)", stroke: "var(--rex-brand)" }}
          strokeWidth="1.1"
          strokeLinejoin="round"
        />
        <circle cx="12" cy="5" r="1.5" style={{ fill: "var(--rex-brand-tint)" }} />
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
            ? "bg-brand-active text-brand-tint"
            : "text-rex-text-muted hover:bg-rex-hover hover:text-rex-text-bright",
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
            <span className="inline-flex items-center gap-[5px]">
              {item.activeDot && (
                <span className="relative inline-flex h-1.5 w-1.5">
                  <span className="absolute inset-0 rounded-full bg-status-running opacity-50 animate-rex-ping motion-reduce:animate-none" />
                  <span className="relative h-1.5 w-1.5 rounded-full bg-status-running" />
                </span>
              )}
              <span className="font-mono text-[10.5px] text-rex-text-faint">
                {item.badge}
              </span>
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
    initialData: ZERO_STATUS,
  });

  // Live nav-badge counts. Each query reuses its screen's queryKey, so the cache is
  // shared (opening a screen doesn't double-poll). Every badge shows the section's
  // active count — running services/dbs/tunnels, unread mail, total sites — replacing
  // the old hardcoded mock numbers, and is hidden when zero.
  const { data: sites = [] } = useQuery({ queryKey: ["sites"], queryFn: listSites, refetchInterval: 2000 });
  const { data: dbs = [] } = useQuery({ queryKey: ["databases"], queryFn: databasesStatus, refetchInterval: 2000 });
  const { data: inbox } = useQuery({ queryKey: ["mailpit-messages", ""], queryFn: () => mailpitMessages(""), refetchInterval: 5000 });
  const { data: tunnels = [] } = useQuery({ queryKey: ["tunnels"], queryFn: tunnelsStatus, refetchInterval: 2000 });

  const liveBadges: Record<string, { badge?: string; activeDot?: boolean }> = {
    "/sites": { badge: badgeCount(sites.length) },
    "/services": { badge: badgeCount(status.running) },
    "/databases": { badge: badgeCount(dbs.filter((d) => d.running).length) },
    "/mail": { badge: badgeCount(inbox?.unread ?? 0) },
    "/tunnels": {
      badge: badgeCount(tunnels.filter((t) => t.running).length),
      activeDot: tunnels.some((t) => t.running),
    },
  };
  const withLiveBadge = (item: NavItem): NavItem => ({ ...item, ...liveBadges[item.to] });

  return (
    <aside className="flex w-[220px] flex-none flex-col border-r border-rex-border-subtle bg-rex-surface-1">
      {/* Header: wordmark (drag region). The macOS traffic lights are the real
          OS controls — the window uses titleBarStyle Overlay, so we reserve the
          top row for them instead of drawing fake dots. */}
      <div
        onMouseDown={onTitleBarMouseDown}
        className="drag-region flex h-[84px] flex-none flex-col justify-center gap-3.5 border-b border-rex-border-subtle px-[18px]"
      >
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
                <NavButton key={item.to} item={withLiveBadge(item)} />
              ))}
          </div>
        ))}
        <div className="flex-1" />
        <div className="flex items-center gap-1">
          <div className="min-w-0 flex-1">
            {footer.map((item) => (
              <NavButton key={item.to} item={withLiveBadge(item)} />
            ))}
          </div>
          <ThemeToggle />
        </div>
      </nav>

      <StatusFooter status={status} />
    </aside>
  );
}
