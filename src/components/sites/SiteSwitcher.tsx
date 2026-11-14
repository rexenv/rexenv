import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, Search } from "lucide-react";
import { allSiteDomains, getSitesServing, listSites, openExternal } from "@/lib/ipc";
import { isScratch, siteTypeMeta } from "@/lib/siteType";
import { useSiteUrl } from "@/lib/siteUrl";
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
  const [query, setQuery] = useState("");
  const listRef = useRef<HTMLDivElement>(null);

  const servingOf = useMemo(
    () => new Map((serving ?? []).map((s) => [s.domain, s.serving])),
    [serving],
  );

  // The user's sites by name, then the agent's scratch sites under the same
  // heading the Sites list gives them — on the recorded fact only (`isScratch`).
  const { own, scratch, flat } = useMemo(() => {
    const q = query.trim().toLowerCase();
    const hit = sites
      .filter((s) => matches(s, extraDomains[s.id], q))
      .sort((a, b) => a.name.localeCompare(b.name));
    const own = hit.filter((s) => !isScratch(s));
    const scratch = hit.filter(isScratch);
    return { own, scratch, flat: [...own, ...scratch] };
  }, [sites, extraDomains, query]);

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
          if (e.metaKey || e.ctrlKey) openSiteUrl(cursor);
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
          <>
            {own.map(row)}
            {scratch.length > 0 && (
              <div className="px-[9px] pb-1 pt-2.5 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted">
                Scratch
              </div>
            )}
            {scratch.map(row)}
          </>
        )}
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
        title="Switch to another site"
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
