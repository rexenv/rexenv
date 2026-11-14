import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useLocation, useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, Search } from "lucide-react";
import { allSiteDomains, getSitesServing, listSites, openExternal } from "@/lib/ipc";
import { isScratch, siteTypeMeta } from "@/lib/siteType";
import { useSiteUrl } from "@/lib/siteUrl";
import { useModKey } from "@/lib/useModKey";
import { groupByShortlist, useSiteShortlist } from "@/lib/useSiteShortlist";
import { toast, toastBackendError } from "@/lib/toast";
import { cn, TECH_INPUT } from "@/lib/utils";
import type { Site } from "@/types";

/** The tabs a site page can show. One list, read by the page and by the switcher. */
export type SiteTabKey =
  | "overview"
  | "wordpress"
  | "repository"
  | "database"
  | "live"
  | "logs"
  | "terminal"
  | "settings";

/** **The tab a site page RENDERS for the tab its URL asks for.**
 *
 * The switcher keeps the tab when it changes site (`/sites/ea/database` →
 * `/sites/bl/database`), and three tabs exist only on some sites: WordPress and
 * Live on a WordPress site, Repository on a site whose folder is a checkout. So
 * "keep the tab" can name a tab the target does not have, and the page used to
 * render whatever the URL said — `/sites/<a Laravel site>/wordpress` drew the
 * WordPress manager over a site with no WordPress in it. The rule lives here,
 * once: a tab the site does not offer is Overview. The page calls this for
 * every render, so a typed or stale URL gets the same answer as the switcher.
 *
 * `hasRepo` is `undefined` while the filesystem has not answered; the page holds
 * the Repository tab's body until it has (it never flashes Overview first). */
export function resolveSiteTab(
  asked: string | undefined,
  facts: { isWordpress: boolean; hasRepo: boolean | undefined },
): SiteTabKey {
  switch (asked) {
    case "wordpress":
    case "live":
      return facts.isWordpress ? asked : "overview";
    case "repository":
      return facts.hasRepo === false ? "overview" : "repository";
    case "database":
    case "logs":
    case "terminal":
    case "settings":
      return asked;
    default:
      return "overview";
  }
}

/** What the switcher's box searches: every name the site answers to — the same
 *  three the Sites list's filter matches, so the two boxes cannot disagree about
 *  whether a site exists. */
function matches(site: Site, extra: string[] | undefined, q: string): boolean {
  if (!q) return true;
  return (
    site.name.toLowerCase().includes(q) ||
    site.domain.toLowerCase().includes(q) ||
    (extra ?? []).some((d) => d.toLowerCase().includes(q))
  );
}

const PANEL_WIDTH = 320;

/** The searchable list of sites — the body of the breadcrumb's popover.
 *
 * Never capped: past its height it scrolls. A truncated list that looks complete
 * is how someone concludes a site is gone (DESIGN.md, the tray's rule). */
export function SiteSwitcherPanel({
  currentId,
  onPick,
  onClose,
}: {
  /** The site the user is on — checked in the list, and where the cursor starts. */
  currentId?: string;
  onPick: (site: Site) => void;
  onClose: () => void;
}) {
  const { data: sites = [] } = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const { data: serving } = useQuery({
    queryKey: ["sites-serving"],
    queryFn: getSitesServing,
    refetchInterval: 2000,
  });
  const { data: extraDomains = {} } = useQuery({
    queryKey: ["site-domains", "all"],
    queryFn: allSiteDomains,
  });
  const siteUrlOf = useSiteUrl();
  const mod = useModKey();
  const [query, setQuery] = useState("");
  const listRef = useRef<HTMLDivElement>(null);

  const servingOf = useMemo(
    () => new Map((serving ?? []).map((s) => [s.domain, s.serving])),
    [serving],
  );

  // Pinned, then recently opened, then everything else by name, then the agent's
  // scratch sites under the heading the Sites list gives them. The first two are
  // core's order (`useSiteShortlist`) — the same one the sidebar and the tray
  // show — and a site is in exactly ONE group: a pinned site is not also recent,
  // and a scratch site that was opened is under Recent, not twice.
  const shortlist = useSiteShortlist();
  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const hit = sites
      .filter((s) => matches(s, extraDomains[s.id], q))
      .sort((a, b) => a.name.localeCompare(b.name));
    const g = groupByShortlist(hit, shortlist);
    const own = g.rest.filter((s) => !isScratch(s));
    const scratch = g.rest.filter(isScratch);
    return [
      { key: "pinned", label: "Pinned", sites: g.pinned },
      { key: "recent", label: "Recent", sites: g.recent },
      // The label only when something sits above it: with nothing pinned or
      // opened the list is just the sites, as it was.
      { key: "all", label: g.pinned.length + g.recent.length > 0 ? "All sites" : "", sites: own },
      { key: "scratch", label: "Scratch", sites: scratch },
    ].filter((x) => x.sites.length > 0);
  }, [sites, extraDomains, query, shortlist]);
  const flat = useMemo(() => groups.flatMap((g) => g.sites), [groups]);

  // The cursor is a site ID, not an index: the list is re-sorted and re-filtered
  // under it (a poll, a keystroke), and an index would then point at a different
  // site than the one highlighted a moment ago.
  const [cursorId, setCursorId] = useState<string | undefined>(currentId);
  const cursor = flat.find((s) => s.id === cursorId) ?? flat[0];

  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>(`[data-switch-site="${cursor?.id ?? ""}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [cursor?.id]);

  const move = (by: number) => {
    if (flat.length === 0) return;
    const at = Math.max(0, flat.findIndex((s) => s.id === cursor?.id));
    setCursorId(flat[(at + by + flat.length) % flat.length].id);
  };

  const openSiteUrl = (site: Site) => {
    // The Sites row's rule, not a second one: nothing answers a URL the stack is
    // not serving, unless the user stopped THIS site (its URL shows rexenv's own
    // stopped page). The key says why instead of opening "can't connect".
    const url = siteUrlOf(site.domain);
    if (!servingOf.get(site.domain) && site.enabled !== false) {
      toast.info(`Nothing is serving ${url} — Start all first`);
      return;
    }
    void openExternal(url).catch(toastBackendError);
  };

  const row = (site: Site) => {
    const t = siteTypeMeta(site.type);
    const isCursor = site.id === cursor?.id;
    const isServing = !!servingOf.get(site.domain);
    return (
      <button
        key={site.id}
        type="button"
        role="option"
        aria-selected={isCursor}
        data-switch-site={site.id}
        data-cursor={isCursor ? "" : undefined}
        onMouseMove={() => setCursorId(site.id)}
        onClick={() => onPick(site)}
        className={cn(
          "flex w-full items-center gap-2.5 rounded-[7px] px-[9px] py-[6px] text-left transition-colors",
          isCursor ? "bg-rex-hover" : "",
        )}
      >
        <span
          className="flex h-[22px] w-[22px] flex-none items-center justify-center rounded-[6px] border text-[0.625rem] font-bold"
          style={{ background: t.bg, color: t.color, borderColor: t.border }}
        >
          {t.letter}
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[0.8125rem] font-medium text-rex-text">{site.name}</span>
          <span className="truncate font-mono text-[0.65625rem] text-rex-text-muted">
            {site.domain}
          </span>
        </span>
        {site.id === currentId && (
          <Check
            data-switch-current
            className="h-[14px] w-[14px] flex-none text-brand-tint"
            strokeWidth={2.2}
          />
        )}
        <span
          title={
            isServing ? "Serving" : site.enabled === false ? "Stopped by you" : "Not serving"
          }
          className={cn(
            "h-[7px] w-[7px] flex-none rounded-full",
            isServing ? "bg-status-running" : "bg-status-stopped",
          )}
        />
      </button>
    );
  };

  return (
    <div
      data-site-switcher
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          move(1);
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          move(-1);
        } else if (e.key === "Enter") {
          e.preventDefault();
          if (!cursor) return;
          // Mod+Enter: the browser, without leaving the page the user is on.
          if (mod.pressed(e)) openSiteUrl(cursor);
          else onPick(cursor);
        } else if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="relative p-[5px] pb-0">
        <Search
          className="pointer-events-none absolute left-[15px] top-1/2 h-[14px] w-[14px] -translate-y-[calc(50%-2px)] text-rex-text-dim"
          strokeWidth={1.9}
        />
        <input
          {...TECH_INPUT}
          autoFocus
          type="text"
          role="combobox"
          aria-expanded="true"
          aria-controls="site-switcher-list"
          aria-label="Switch site"
          placeholder="Switch site…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="h-[32px] w-full rounded-[7px] border border-rex-border bg-rex-surface-1 pl-[30px] pr-2.5 text-[0.8125rem] text-rex-text outline-none transition-colors focus:border-brand"
        />
      </div>
      <div
        ref={listRef}
        id="site-switcher-list"
        role="listbox"
        className="max-h-[min(60vh,420px)] overflow-y-auto p-[5px]"
      >
        {flat.length === 0 ? (
          <div className="px-[9px] py-[14px] text-center text-[0.78125rem] text-rex-text-muted">
            {query ? `No sites match “${query}”` : "No sites yet"}
          </div>
        ) : (
          groups.map((g) => (
            <div key={g.key} data-switch-group={g.key}>
              {g.label && (
                <div className="px-[9px] pb-1 pt-2.5 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted first:pt-1">
                  {g.label}
                </div>
              )}
              {g.sites.map(row)}
            </div>
          ))
        )}
      </div>
      {/* The keys, said where they work. The modifier is the platform's word
          (`useModKey`), never a "⌘" typed here — that label on Windows names a
          key the keyboard does not have. */}
      <div
        data-switch-keys
        className="flex items-center gap-3 border-t border-rex-border-strong px-[14px] py-[7px] font-mono text-[0.625rem] text-rex-text-muted"
      >
        <span>↑↓ move</span>
        <span>↵ open</span>
        <span>{mod.label}↵ in browser</span>
        <span className="ml-auto">esc</span>
      </div>
    </div>
  );
}

/** `Sites / [W] ea ▾` — the site page's way to the list AND to any other site.
 *
 * It replaced a lone "‹ All sites" link (14 Nov 2026): going from one site to
 * another was back, find the row among 25, click — and the tab was lost on the
 * way. The chevron opens every site, searchable; choosing one keeps the tab. */
export function SiteBreadcrumb({ site, tab }: { site: Site; tab: SiteTabKey }) {
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState({ top: 0, left: 0 });
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const mod = useModKey();
  const t = siteTypeMeta(site.type);

  useLayoutEffect(() => {
    if (!open) return;
    const r = triggerRef.current?.getBoundingClientRect();
    if (!r) return;
    setPos({
      top: r.bottom + 6,
      left: Math.min(Math.max(8, r.left), window.innerWidth - PANEL_WIDTH - 8),
    });
  }, [open]);

  // Closing hands the focus back to the trigger: the popover took it (the search
  // box), and a closed popover that keeps it leaves the keyboard nowhere.
  const close = () => {
    setOpen(false);
    triggerRef.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (
        !panelRef.current?.contains(e.target as Node) &&
        !triggerRef.current?.contains(e.target as Node)
      )
        setOpen(false);
    };
    const onResize = () => setOpen(false);
    document.addEventListener("mousedown", onDoc);
    window.addEventListener("resize", onResize);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("resize", onResize);
    };
  }, [open]);

  return (
    <div className="no-drag mb-[13px] flex items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
      <button
        type="button"
        onClick={() => navigate("/sites")}
        className="rounded-[5px] px-1 py-0.5 transition-colors hover:text-rex-text-bright"
      >
        Sites
      </button>
      <span aria-hidden="true">/</span>
      <button
        ref={triggerRef}
        type="button"
        data-site-switch-trigger
        aria-haspopup="listbox"
        aria-expanded={open}
        title={`Switch to another site (${mod.label}K)`}
        onClick={() => setOpen((o) => !o)}
        className={cn(
          "flex min-w-0 items-center gap-1.5 rounded-[6px] border px-1.5 py-[3px] transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2 hover:text-rex-text-bright",
          open
            ? "border-rex-border-strong bg-rex-surface-2 text-rex-text-bright"
            : "border-rex-border bg-rex-surface-1 text-rex-text-bright",
        )}
      >
        <span
          className="flex h-[15px] w-[15px] flex-none items-center justify-center rounded-[4px] border text-[0.5625rem] font-bold"
          style={{ background: t.bg, color: t.color, borderColor: t.border }}
        >
          {t.letter}
        </span>
        <span className="max-w-[220px] truncate">{site.name}</span>
        <ChevronDown className="h-3 w-3 flex-none text-rex-text-muted" strokeWidth={2} />
      </button>
      {open &&
        createPortal(
          <div
            ref={panelRef}
            style={{ position: "fixed", top: pos.top, left: pos.left, width: PANEL_WIDTH }}
            className="z-[60] rounded-[11px] border border-rex-border-strong bg-rex-surface-2 shadow-menu"
          >
            <SiteSwitcherPanel
              currentId={site.id}
              onClose={close}
              onPick={(target) => {
                setOpen(false);
                // The tab comes along. Whether the target HAS that tab is the
                // page's own rule (`resolveSiteTab`), asked on arrival — the
                // switcher does not guess at another site's WordPress-ness.
                navigate(tab === "overview" ? `/sites/${target.id}` : `/sites/${target.id}/${tab}`);
              }}
            />
          </div>,
          document.body,
        )}
    </div>
  );
}

/** A site route's two parameters, read off the path — the host sits ABOVE the
 *  routes (in the shell), where `useParams` has nothing to say. */
function siteRouteOf(pathname: string): { id?: string; tab?: string } {
  const m = /^\/sites\/([^/]+)(?:\/([^/]+))?\/?$/.exec(pathname);
  return m ? { id: decodeURIComponent(m[1]), tab: m[2] } : {};
}

/** **Mod+K, from every route: the same list the breadcrumb opens.**
 *
 * One component, two doors (owner ruling D1, 14 Nov 2026) — the list, its search
 * and its keys are `SiteSwitcherPanel` either way, so the two cannot drift. On a
 * site page the tab comes along, as it does from the breadcrumb; from anywhere
 * else the site opens on Overview.
 *
 * Three places the shortcut is deliberately NOT taken:
 * - **inside a terminal.** Ctrl+K is the shell's (kill-line in readline, clear in
 *   others) and ⌘K clears the screen in every macOS terminal. A site shell that
 *   lost a key to the app around it is a broken shell; the breadcrumb is one
 *   click away on that same page.
 * - **while a dialog is open.** A modal asks a question; opening a second layer
 *   over it that navigates away would abandon the answer half-given.
 * - **with Shift or Alt held** — those are other chords, and not ours. */
export function SiteSwitcherHost() {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const mod = useModKey();
  const [open, setOpen] = useState(false);
  const here = siteRouteOf(pathname);

  // The listener is registered once and reads the latest `mod` through a ref:
  // re-registering on every words/transport answer would leave a gap in which a
  // keystroke reaches nobody.
  const pressed = useRef(mod.pressed);
  pressed.current = mod.pressed;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() !== "k" || e.shiftKey || e.altKey) return;
      if (!pressed.current(e)) return;
      const target = e.target as Element | null;
      if (target?.closest?.(".xterm")) return;
      if (document.querySelector('[role="dialog"]:not([data-site-switcher-dialog])')) return;
      e.preventDefault();
      // The breadcrumb's popover is already this list: a second copy over it
      // would be two cursors in two search boxes.
      if (document.querySelector("[data-site-switcher]:not([data-site-switcher-dialog] *)")) return;
      setOpen((o) => !o);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // A navigation by any other means (the tray, a toast's link) closes it.
  useEffect(() => setOpen(false), [pathname]);

  if (!open) return null;
  return createPortal(
    <div
      className="fixed inset-0 z-[60] flex items-start justify-center bg-black/50 pt-[12vh]"
      onMouseDown={() => setOpen(false)}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Switch site"
        data-site-switcher-dialog
        onMouseDown={(e) => e.stopPropagation()}
        style={{ width: PANEL_WIDTH + 80 }}
        className="max-w-[calc(100vw-32px)] rounded-[12px] border border-rex-border-strong bg-rex-surface-2 shadow-menu"
      >
        <SiteSwitcherPanel
          currentId={here.id}
          onClose={() => setOpen(false)}
          onPick={(target) => {
            setOpen(false);
            const tab = here.id && here.tab && here.tab !== "overview" ? `/${here.tab}` : "";
            navigate(`/sites/${target.id}${tab}`);
          }}
        />
      </div>
    </div>,
    document.body,
  );
}
