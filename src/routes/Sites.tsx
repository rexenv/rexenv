import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, Globe, FolderOpen, Database, Lock, Trash2, MoreHorizontal, ArrowDownUp } from "lucide-react";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
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
  return (
    <div className="group flex items-center gap-3 border-b border-rex-border-subtle px-4 py-2.5 transition-colors hover:bg-white/[0.02]">
      <div className="flex h-7 w-7 flex-none items-center justify-center rounded-md border border-rex-border bg-rex-surface-2 text-rex-text-muted">
        <Globe className="h-4 w-4" strokeWidth={1.7} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13.5px] font-semibold text-rex-text">
          {site.name}
        </div>
        <div className="font-mono text-[11px] text-rex-text-dim">{site.domain}</div>
      </div>
      {site.ssl && <Lock className="h-3.5 w-3.5 text-status-running" strokeWidth={2} />}
      <Badge>PHP {site.phpVersion}</Badge>
      <Badge>{site.webServer}</Badge>
      <StatusPill status={site.status} />
      <StartStopToggle
        running={running}
        busy={busy}
        onToggle={onToggle}
        label={`${running ? "Stop" : "Start"} ${site.name}`}
      />
      <div className="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
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
        <Button
          variant="ghost"
          size="icon"
          aria-label="Delete site"
          onClick={onDelete}
          className="hover:text-status-error"
        >
          <Trash2 className="h-4 w-4" />
        </Button>
        <Button variant="ghost" size="icon" aria-label="More actions">
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </div>
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
          <div className="flex h-full flex-col items-center justify-center gap-4 rounded-xl border border-rex-border bg-rex-surface-1 text-center">
            <div className="flex h-12 w-12 items-center justify-center rounded-xl border border-rex-border bg-rex-surface-2 text-rex-text-muted">
              <Globe className="h-6 w-6" strokeWidth={1.6} />
            </div>
            <div>
              <div className="text-[15px] font-semibold text-rex-text">No sites yet</div>
              <div className="mt-1 text-[13px] text-rex-text-muted">
                Create your first local site to get started.
              </div>
            </div>
            {newSiteButton}
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
