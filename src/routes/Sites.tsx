import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, Globe, FolderOpen, Database, Lock, LockOpen, Trash2, MoreVertical, ArrowDownUp, Pencil, Copy, Code, Link } from "lucide-react";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { Menu, MenuItem, MenuSeparator } from "@/components/ui/menu";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { Placeholder } from "@/components/common/Placeholder";
import { NewSiteDialog } from "@/components/sites/NewSiteDialog";
import { Button } from "@/components/ui/button";
import { listSites, startSite, stopSite, deleteSite } from "@/lib/ipc";
import type { Site } from "@/types";

function Badge({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded border border-rex-border bg-rex-surface-2 px-1.5 py-0.5 font-mono text-[10.5px] text-rex-text-bright">
      {children}
    </span>
  );
}

// Per-type avatar: letter + accent (WordPress blue / Laravel red / Blank-PHP purple).
const TYPE_META: Record<string, { letter: string; bg: string; color: string; border: string }> = {
  wordpress: { letter: "W", bg: "rgba(74,134,170,0.15)", color: "#7DB8D8", border: "rgba(74,134,170,0.30)" },
  laravel: { letter: "L", bg: "rgba(224,82,77,0.13)", color: "#EE837C", border: "rgba(224,82,77,0.27)" },
  php: { letter: "P", bg: "rgba(125,128,185,0.17)", color: "#A7AADD", border: "rgba(125,128,185,0.32)" },
};

type Filter = "all" | "running" | "stopped";
type Sort = "name" | "status" | "recent";

const SORT_LABEL: Record<Sort, string> = {
  name: "Name",
  status: "Status",
  recent: "Recent",
};
const SORT_CYCLE: Record<Sort, Sort> = {
  name: "status",
  status: "recent",
  recent: "name",
};

/** All / Running / Stopped segmented control with live counts. */
function FilterTabs({
  value,
  onChange,
  counts,
}: {
  value: Filter;
  onChange: (f: Filter) => void;
  counts: Record<Filter, number>;
}) {
  const tabs: { key: Filter; label: string }[] = [
    { key: "all", label: "All" },
    { key: "running", label: "Running" },
    { key: "stopped", label: "Stopped" },
  ];
  return (
    <div className="inline-flex items-center gap-1 rounded-[10px] border border-[#1E222A] bg-rex-well p-[3px]">
      {tabs.map((t) => (
        <button
          key={t.key}
          onClick={() => onChange(t.key)}
          className={cn(
            "flex h-7 items-center gap-1.5 rounded-[7px] px-[11px] text-[12.5px] font-medium transition-colors",
            value === t.key
              ? "bg-brand-tint-bg text-brand-tint"
              : "text-rex-text-muted hover:text-rex-text-bright",
          )}
        >
          {t.label}
          <span className="font-mono text-[10.5px] opacity-70">{counts[t.key]}</span>
        </button>
      ))}
    </div>
  );
}

function SortButton({ value, onCycle }: { value: Sort; onCycle: () => void }) {
  return (
    <button
      onClick={onCycle}
      title="Change sort order"
      className="flex h-[34px] items-center gap-[7px] rounded-[9px] border border-rex-border bg-rex-surface-1 px-3 text-[13px] text-rex-text-bright transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2"
    >
      <ArrowDownUp className="h-3.5 w-3.5 text-rex-text-muted" strokeWidth={1.8} />
      {SORT_LABEL[value]}
    </button>
  );
}

function SiteRow({
  site,
  busy,
  onToggle,
  onDelete,
  onOpenDatabase,
}: {
  site: Site;
  busy: boolean;
  onToggle: () => void;
  onDelete: () => void;
  onOpenDatabase: () => void;
}) {
  const running = site.status === "running";
  const t = TYPE_META[site.type] ?? TYPE_META.php;
  return (
    <div className="group flex items-center gap-3 border-b border-rex-border-subtle px-4 py-2.5 transition-colors hover:bg-white/[0.02]">
      <div
        className="flex h-[26px] w-[26px] flex-none items-center justify-center rounded-[7px] border text-[11px] font-bold"
        style={{ background: t.bg, color: t.color, borderColor: t.border }}
      >
        {t.letter}
      </div>
      <div className="flex w-[188px] flex-none flex-col gap-px">
        <div className="truncate text-[13.5px] font-semibold text-rex-text">
          {site.name}
        </div>
        <div className="truncate font-mono text-[11px] text-rex-text-muted">
          {site.domain}
        </div>
      </div>
      <div className="ml-1 flex items-center gap-px opacity-0 transition-opacity group-hover:opacity-100">
        <Button variant="ghost" size="icon" aria-label="Open in browser">
          <Globe className="h-4 w-4" />
        </Button>
        <Button variant="ghost" size="icon" aria-label="Open folder">
          <FolderOpen className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Open database"
          title={site.type === "php" ? "Blank PHP sites have no database" : "Open database"}
          disabled={site.type === "php"}
          onClick={onOpenDatabase}
        >
          <Database className="h-4 w-4" />
        </Button>
      </div>
      <div className="flex-1" />
      <span
        title={site.ssl ? "SSL · trusted" : "No SSL"}
        className="flex flex-none items-center"
        style={{ color: site.ssl ? "var(--rex-lock-secure)" : "var(--rex-lock-insecure)" }}
      >
        {site.ssl ? (
          <Lock className="h-3.5 w-3.5" strokeWidth={1.8} />
        ) : (
          <LockOpen className="h-3.5 w-3.5" strokeWidth={1.8} />
        )}
      </span>
      <Badge>PHP {site.phpVersion}</Badge>
      <Badge>{site.webServer}</Badge>
      <StatusPill status={site.status} />
      <StartStopToggle
        running={running}
        busy={busy}
        onToggle={onToggle}
        label={`${running ? "Stop" : "Start"} ${site.name}`}
      />
      <Menu
        trigger={
          <button
            type="button"
            aria-label="More actions"
            className="flex h-7 w-7 items-center justify-center rounded-[7px] text-rex-text-muted transition-colors hover:bg-white/[0.07] hover:text-rex-text"
          >
            <MoreVertical className="h-4 w-4" />
          </button>
        }
      >
        {/* Rename / Duplicate / Open in editor have no backend yet (shell). */}
        <MenuItem icon={<Pencil className="h-[15px] w-[15px]" strokeWidth={1.7} />}>
          Rename
        </MenuItem>
        <MenuItem icon={<Copy className="h-[15px] w-[15px]" strokeWidth={1.7} />}>
          Duplicate
        </MenuItem>
        <MenuItem icon={<Code className="h-[15px] w-[15px]" strokeWidth={1.7} />}>
          Open in editor
        </MenuItem>
        <MenuItem
          icon={<Link className="h-[15px] w-[15px]" strokeWidth={1.7} />}
          onSelect={() => navigator.clipboard?.writeText(site.domain)}
        >
          Copy domain
        </MenuItem>
        <MenuSeparator />
        <MenuItem
          icon={<Trash2 className="h-[15px] w-[15px]" strokeWidth={1.7} />}
          danger
          onSelect={onDelete}
        >
          Delete
        </MenuItem>
      </Menu>
    </div>
  );
}

export function Sites() {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [showNew, setShowNew] = useState(false);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("name");
  const { data: sites = [], isLoading } = useQuery({
    queryKey: ["sites"],
    queryFn: listSites,
  });

  const toggle = useMutation({
    mutationFn: (site: Site) =>
      site.status === "running" ? stopSite(site.id) : startSite(site.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => window.alert(String(e)),
  });

  const remove = useMutation({
    mutationFn: (site: Site) => deleteSite(site.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => window.alert(String(e)),
  });

  const confirmDelete = (site: Site) => {
    if (window.confirm(`Delete "${site.name}" (${site.domain})? This removes its files and certificate.`)) {
      remove.mutate(site);
    }
  };

  const running = sites.filter((s) => s.status === "running").length;
  const counts: Record<Filter, number> = {
    all: sites.length,
    running,
    stopped: sites.length - running,
  };

  // Filter (segmented) → search (name/domain) → sort.
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = sites.filter((s) => {
      if (filter === "running" && s.status !== "running") return false;
      if (filter === "stopped" && s.status === "running") return false;
      if (q && !s.name.toLowerCase().includes(q) && !s.domain.toLowerCase().includes(q))
        return false;
      return true;
    });
    return [...list].sort((a, b) => {
      if (sort === "name") return a.name.localeCompare(b.name);
      if (sort === "recent") return b.createdAt.localeCompare(a.createdAt);
      // status: running first, then by name
      const ar = a.status === "running" ? 0 : 1;
      const br = b.status === "running" ? 0 : 1;
      return ar - br || a.name.localeCompare(b.name);
    });
  }, [sites, filter, query, sort]);

  const noResults = sites.length > 0 && visible.length === 0;

  const newSiteButton = (
    <Button variant="primary" onClick={() => setShowNew(true)}>
      <Plus className="h-[15px] w-[15px]" strokeWidth={2.2} />
      New site
    </Button>
  );

  const headerActions = (
    <>
      <SortButton value={sort} onCycle={() => setSort((s) => SORT_CYCLE[s])} />
      {newSiteButton}
    </>
  );

  return (
    <>
      <TopBar
        title="Sites"
        subtitle={
          isLoading ? "Loading…" : `${sites.length} sites · ${running} running`
        }
        searchPlaceholder="Filter sites…"
        searchValue={query}
        onSearchChange={setQuery}
        action={headerActions}
      />
      {!isLoading && sites.length > 0 && (
        <div className="flex flex-none items-center justify-between px-[22px] pb-[9px] pt-[14px]">
          <FilterTabs value={filter} onChange={setFilter} counts={counts} />
          <div className="flex items-center gap-[18px] pr-2 font-mono text-[10px] uppercase tracking-[0.1em] text-[var(--rex-placeholder)]">
            <span className="w-[118px]">Stack</span>
            <span className="w-[88px]">Status</span>
            <span>Power</span>
          </div>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto px-[18px] pb-[18px]">
        {isLoading ? (
          <Placeholder
            icon={<Globe className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="Loading sites…"
            hint="Reading your local sites"
          />
        ) : sites.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-4 p-10 text-center">
            <div className="flex h-[60px] w-[60px] items-center justify-center rounded-2xl border border-[#2A2E39] bg-gradient-to-br from-[#1C2029] to-[#14161C] shadow-glow-crown">
              <svg width="28" height="28" viewBox="0 0 24 24" className="block">
                <path
                  d="M3 8.4 L8 12.6 L12 5 L16 12.6 L21 8.4 L19.1 18.7 L4.9 18.7 Z"
                  fill="#7C5CFF"
                  stroke="#7C5CFF"
                  strokeWidth="1.1"
                  strokeLinejoin="round"
                />
                <circle cx="3" cy="8.4" r="1.4" fill="#B9A6FF" />
                <circle cx="12" cy="5" r="1.6" fill="#C9BCFF" />
                <circle cx="21" cy="8.4" r="1.4" fill="#B9A6FF" />
              </svg>
            </div>
            <div>
              <div className="text-[19px] font-semibold text-rex-text">No sites yet</div>
              <div className="mt-2 max-w-[400px] text-[13.5px] leading-[1.55] text-rex-text-muted">
                Point rexenv at a folder and it serves your site instantly — with its
                own .test domain, PHP, and database.
              </div>
            </div>
            <Button variant="primary" size="lg" onClick={() => setShowNew(true)}>
              <Plus className="h-4 w-4" strokeWidth={2.3} />
              Create your first site
            </Button>
          </div>
        ) : noResults ? (
          <div className="flex flex-col items-center justify-center gap-1.5 px-5 py-[54px] text-center">
            <div className="text-[14px] font-medium text-rex-text-bright">
              {query ? `No sites match “${query}”` : "No sites in this view"}
            </div>
            <div className="text-[12.5px] text-rex-text-dim">
              Try a different name, domain, or clear the filter.
            </div>
          </div>
        ) : (
          <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
            {visible.map((site) => (
              <SiteRow
                key={site.id}
                site={site}
                busy={toggle.isPending && toggle.variables?.id === site.id}
                onToggle={() => toggle.mutate(site)}
                onDelete={() => confirmDelete(site)}
                onOpenDatabase={() => navigate(`/sites/${site.id}/database`)}
              />
            ))}
          </div>
        )}
      </div>
      {showNew && <NewSiteDialog onClose={() => setShowNew(false)} />}
    </>
  );
}
