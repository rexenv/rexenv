import { useMemo, useState } from "react";
import { toast } from "@/lib/toast";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, Globe, FolderOpen, Database, LayoutDashboard, Lock, LockOpen, Trash2, MoreVertical, ArrowDownUp, Pencil, Copy, Code, Link } from "lucide-react";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { Menu, MenuItem, MenuSeparator } from "@/components/ui/menu";
import { ConfirmDialog, PromptDialog } from "@/components/ui/dialog";
import { siteTypeMeta } from "@/lib/siteType";
import { StatusPill } from "@/components/common/StatusPill";
import { Placeholder } from "@/components/common/Placeholder";
import { NewSiteDialog } from "@/components/sites/NewSiteDialog";
import { Button } from "@/components/ui/button";
import { listSites, deleteSite, renameSite, openExternal, getSitesServing } from "@/lib/ipc";
import type { Site } from "@/types";

function Badge({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "flex-none rounded-[6px] border border-rex-border-strong bg-rex-surface-1 px-[7px] py-[3px] font-mono text-[10.5px] text-rex-text-bright",
        className,
      )}
    >
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
  status,
  onOpen,
  onDelete,
  onOpenDatabase,
  onOpenWordpress,
  onRename,
  onDuplicate,
}: {
  site: Site;
  status: Site["status"];
  onOpen: () => void;
  onDelete: () => void;
  onOpenDatabase: () => void;
  onOpenWordpress: () => void;
  onRename: () => void;
  onDuplicate: () => void;
}) {
  const t = siteTypeMeta(site.type);
  const [copied, setCopied] = useState(false);
  return (
    <div
      role="button"
      tabIndex={0}
      aria-label={`Open ${site.name}`}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen();
        }
      }}
      className="group flex h-11 cursor-pointer items-center gap-[11px] rounded-[9px] pl-3 pr-2 transition-colors hover:bg-rex-surface-1 focus:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-white/15"
    >
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
      <div
        className="ml-1 flex items-center gap-px opacity-0 transition-opacity group-hover:opacity-100"
        onClick={(e) => e.stopPropagation()}
      >
        <Button
          variant="ghost"
          size="icon"
          aria-label="Open in browser"
          title={`Open https://${site.domain}`}
          onClick={() => openExternal(`https://${site.domain}`)}
        >
          <Globe className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Open folder"
          title="Reveal site folder"
          onClick={() => openExternal(site.path)}
        >
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
        {site.type === "wordpress" && (
          <Button
            variant="ghost"
            size="icon"
            aria-label="Manage WordPress"
            title="Manage WordPress"
            onClick={onOpenWordpress}
          >
            <LayoutDashboard className="h-4 w-4" />
          </Button>
        )}
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
      <Badge>{site.phpVersion}</Badge>
      <Badge className="w-[84px] text-center">{site.webServer}</Badge>
      <StatusPill status={status} className="w-[92px]" />
      <div className="flex-none" onClick={(e) => e.stopPropagation()}>
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
        <MenuItem icon={<Pencil className="h-[15px] w-[15px]" strokeWidth={1.7} />} onSelect={onRename}>
          Rename
        </MenuItem>
        <MenuItem icon={<Copy className="h-[15px] w-[15px]" strokeWidth={1.7} />} onSelect={onDuplicate}>
          Duplicate
        </MenuItem>
        <MenuItem
          icon={<Code className="h-[15px] w-[15px]" strokeWidth={1.7} />}
          onSelect={() => openExternal(site.path)}
        >
          Open in editor
        </MenuItem>
        <MenuItem
          icon={<Link className="h-[15px] w-[15px]" strokeWidth={1.7} />}
          onSelect={() => {
            void navigator.clipboard?.writeText(site.domain);
            setCopied(true);
            setTimeout(() => setCopied(false), 1200);
          }}
        >
          {copied ? "Copied!" : "Copy domain"}
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
  // A site's displayed status is its live *serving* state, NOT the sites.status
  // column (task 2.1 / H1): serving only when the edge is up AND the site's own
  // upstream (php-fpm pool or FrankenPHP backend) is up, so a partial stack no
  // longer shows every site as running (H1 follow-up). Keyed by domain.
  const { data: serving } = useQuery({
    queryKey: ["sites-serving"],
    queryFn: getSitesServing,
    refetchInterval: 2000,
  });
  const servingMap = useMemo(
    () => new Map((serving ?? []).map((s) => [s.domain, s.serving])),
    [serving],
  );
  const statusOf = (site: Site): Site["status"] =>
    servingMap.get(site.domain) ? "running" : "stopped";

  const remove = useMutation({
    mutationFn: (site: Site) => deleteSite(site.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toast.error(String(e)),
  });

  const rename = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => renameSite(id, name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toast.error(String(e)),
  });

  // In-app modals (WKWebView doesn't support window.confirm/prompt reliably).
  const [renameTarget, setRenameTarget] = useState<Site | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Site | null>(null);

  // "Duplicate" opens New Site prefilled with this site's setup (type/PHP/server);
  // the user picks a fresh domain. A byte-for-byte clone would need a backend copy.
  const [dupSource, setDupSource] = useState<Site | null>(null);

  const running = sites.filter((s) => statusOf(s) === "running").length;
  const counts: Record<Filter, number> = {
    all: sites.length,
    running,
    stopped: sites.length - running,
  };

  // Filter (segmented) → search (name/domain) → sort.
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = sites.filter((s) => {
      const st = statusOf(s);
      if (filter === "running" && st !== "running") return false;
      if (filter === "stopped" && st === "running") return false;
      if (q && !s.name.toLowerCase().includes(q) && !s.domain.toLowerCase().includes(q))
        return false;
      return true;
    });
    return [...list].sort((a, b) => {
      if (sort === "recent") return b.createdAt.localeCompare(a.createdAt);
      if (sort === "status") {
        // Serving sites first, then by name (per-site status is meaningful again).
        const rank = (s: Site) => (statusOf(s) === "running" ? 0 : 1);
        if (rank(a) !== rank(b)) return rank(a) - rank(b);
      }
      return a.name.localeCompare(b.name);
    });
  }, [sites, filter, query, sort, servingMap]);

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
          <div className="flex flex-col">
            {visible.map((site) => (
              <SiteRow
                key={site.id}
                site={site}
                status={statusOf(site)}
                onOpen={() => navigate(`/sites/${site.id}`)}
                onDelete={() => setDeleteTarget(site)}
                onOpenDatabase={() => navigate(`/sites/${site.id}/database`)}
                onOpenWordpress={() => navigate(`/sites/${site.id}/wordpress`)}
                onRename={() => setRenameTarget(site)}
                onDuplicate={() => setDupSource(site)}
              />
            ))}
          </div>
        )}
      </div>
      {(showNew || dupSource) && (
        <NewSiteDialog
          initial={
            dupSource
              ? {
                  name: `${dupSource.name} copy`,
                  siteType: dupSource.type,
                  phpVersion: dupSource.phpVersion,
                  webServer: dupSource.webServer,
                }
              : undefined
          }
          onClose={() => {
            setShowNew(false);
            setDupSource(null);
          }}
        />
      )}
      {renameTarget && (
        <PromptDialog
          title="Rename site"
          label="Display name"
          initialValue={renameTarget.name}
          submitLabel="Rename"
          onSubmit={(name) => {
            if (name !== renameTarget.name) rename.mutate({ id: renameTarget.id, name });
            setRenameTarget(null);
          }}
          onCancel={() => setRenameTarget(null)}
        />
      )}
      {deleteTarget && (
        <ConfirmDialog
          title={`Delete "${deleteTarget.name}"?`}
          message={
            <>
              This permanently removes <span className="font-mono text-rex-text">{deleteTarget.domain}</span>,
              its files, and its certificate. This can't be undone.
            </>
          }
          confirmLabel="Delete site"
          danger
          onConfirm={() => {
            remove.mutate(deleteTarget);
            setDeleteTarget(null);
          }}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </>
  );
}
