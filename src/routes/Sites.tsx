import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderInput, Plus, Globe, FolderOpen, Database, Lock, LockOpen, Trash2, MoreVertical, ArrowDownUp, Pencil, Copy, Code, Link, RefreshCw } from "lucide-react";
import { WordPressIcon } from "@/components/common/WordPressIcon";
import { RexLogo } from "@/components/common/RexLogo";
import { toast, toastBackendError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { Menu, MenuItem, MenuSeparator } from "@/components/ui/menu";
import { PromptDialog } from "@/components/ui/dialog";
import { DeleteSiteDialog } from "@/components/sites/DeleteSiteDialog";
import { siteTypeMeta } from "@/lib/siteType";
import { StatusPill } from "@/components/common/StatusPill";
import { Placeholder } from "@/components/common/Placeholder";
import { NewSiteDialog } from "@/components/sites/NewSiteDialog";
import { Button } from "@/components/ui/button";
import { defaultTld, getSetting, listEditors, listSites, deleteSite, openInEditor, renameSite, openExternal, getSitesServing, sitesResources, siteProvisionCancel, siteProvisionRetry , scanValetImport, dbImportRecords, rewriteRevert } from "@/lib/ipc";
import { SiteProvisionCard, useSiteProvision } from "@/components/sites/SiteProvisionCard";
import { useDownloads } from "@/lib/useDownloads";
import type { DbImportRecord, Site, SiteResources } from "@/types";

/** Compact bytes for the per-site DB size. */
function fmtBytes(b: number): string {
  if (b >= 1024 * 1024 * 1024) return `${(b / (1024 * 1024 * 1024)).toFixed(1)}G`;
  if (b >= 1024 * 1024) return `${Math.round(b / (1024 * 1024))}M`;
  return `${Math.max(1, Math.round(b / 1024))}K`;
}

/** Honest per-site resources. A site is not a process: only FrankenPHP sites
 *  (dedicated backend) get real CPU/RAM; shared nginx+pool sites show ACTIVITY
 *  (last-60s requests) + DB size behind a "shared" badge — never a fabricated
 *  per-site CPU/RAM. */
function SiteMetrics({ res }: { res?: SiteResources }) {
  if (!res) return <span className="w-[168px] flex-none" />;
  const db = res.dbSizeBytes != null ? `${fmtBytes(res.dbSizeBytes)} DB` : null;
  if (res.dedicated) {
    const own =
      res.ramMb != null
        ? `${res.ramMb}M · ${(res.cpuPercent ?? 0).toFixed(1)}%`
        : "not running";
    return (
      <span
        className="w-[168px] flex-none truncate text-right font-mono text-[0.65625rem] text-rex-text-dim"
        title="Dedicated FrankenPHP process — real CPU/RAM for this site (plus its DB size)"
      >
        {own}
        {db ? ` · ${db}` : ""}
      </span>
    );
  }
  const req = res.requestsPerMin ?? 0;
  return (
    <span
      className="flex w-[168px] flex-none items-center justify-end gap-[6px] font-mono text-[0.65625rem] text-rex-text-dim"
      title="Shared nginx + PHP pool — a per-site CPU/RAM number doesn't exist here; showing real activity (requests in the last 60s) and DB size instead"
    >
      <span className="rounded-[5px] border border-rex-border bg-rex-surface-2 px-[5px] py-px text-[0.5625rem] uppercase tracking-[0.08em] text-rex-text-faint">
        shared
      </span>
      <span className="truncate">
        {req}/min{db ? ` · ${db}` : ""}
      </span>
    </span>
  );
}

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
        "flex-none rounded-[6px] border border-rex-border-strong bg-rex-surface-1 px-[7px] py-[3px] font-mono text-[0.65625rem] text-rex-text-bright",
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
    <div className="inline-flex items-center gap-1 rounded-[10px] border border-rex-well-border bg-rex-well p-[3px]">
      {tabs.map((t) => (
        <button
          key={t.key}
          onClick={() => onChange(t.key)}
          className={cn(
            "flex h-7 items-center gap-1.5 rounded-[7px] px-[11px] text-[0.78125rem] font-medium transition-colors",
            value === t.key
              ? "bg-brand-tint-bg text-brand-tint"
              : "text-rex-text-muted hover:text-rex-text-bright",
          )}
        >
          {t.label}
          <span className="font-mono text-[0.65625rem] opacity-70">{counts[t.key]}</span>
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
      className="flex h-[34px] items-center gap-[7px] rounded-[9px] border border-rex-border bg-rex-surface-1 px-3 text-[0.8125rem] text-rex-text-bright transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2"
    >
      <ArrowDownUp className="h-3.5 w-3.5 text-rex-text-muted" strokeWidth={1.8} />
      {SORT_LABEL[value]}
    </button>
  );
}

/** The editor "Open in editor" targets: the preferred_editor setting when it is
 *  still installed, else the first detected editor, else null (no editor). */
function usePreferredEditor() {
  const { data: editors = [] } = useQuery({
    queryKey: ["editors"],
    queryFn: listEditors,
    staleTime: 60_000,
  });
  const { data: preferred } = useQuery({
    queryKey: ["setting", "preferred_editor"],
    queryFn: () => getSetting("preferred_editor"),
  });
  return editors.find((e) => e.id === preferred) ?? editors[0] ?? null;
}

/** Exported for the dev harness (`?panel=provision` badge check). */
export function SiteRow({
  site,
  status,
  resources,
  dbState,
  onOpen,
  onDelete,
  onOpenDatabase,
  onOpenWordpress,
  onRename,
  onDuplicate,
  onRetry,
}: {
  site: Site;
  status: Site["status"];
  resources?: SiteResources;
  /** This site's database-import state (the ONE serialized DbImportRecord
   *  fact — see the badge comment). Undefined = no import. */
  dbState?: DbImportRecord["state"];
  onOpen: () => void;
  onDelete: () => void;
  onOpenDatabase: () => void;
  onOpenWordpress: () => void;
  onRename: () => void;
  onDuplicate: () => void;
  onRetry?: () => void;
}) {
  const t = siteTypeMeta(site.type);
  const [copied, setCopied] = useState(false);
  const editor = usePreferredEditor();
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
        className="flex h-[26px] w-[26px] flex-none items-center justify-center rounded-[7px] border text-[0.6875rem] font-bold"
        style={{ background: t.bg, color: t.color, borderColor: t.border }}
      >
        {t.letter}
      </div>
      {/* The name column is the row's ONE flexible thing: everything to its
          right is nowrap/fixed, so when badges + pills + a long domain all
          land on one row, the name truncates instead of the badges
          ballooning into neighbouring rows (UI-REVIEW §C1.2). */}
      <div className="flex w-[188px] min-w-[110px] shrink flex-col gap-px">
        <div className="truncate text-[0.84375rem] font-semibold text-rex-text">
          {site.name}
        </div>
        <div className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
          {site.domain}
        </div>
      </div>
      {/* Quick actions reserve ~140px even while invisible (opacity). Below
          lg they disappear from LAYOUT too — every action still exists in
          the row menu — so badge-heavy rows fit narrow windows (§C1.2). */}
      <div
        className="ml-1 hidden items-center gap-px opacity-0 transition-opacity group-hover:opacity-100 lg:flex"
        onClick={(e) => e.stopPropagation()}
      >
        <Button
          variant="ghost"
          size="icon"
          aria-label="Open in browser"
          title={`Open https://${site.domain}`}
          onClick={() => void openExternal(`https://${site.domain}`).catch(toastBackendError)}
        >
          <Globe className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Open folder"
          title="Reveal site folder"
          onClick={() => void openExternal(site.path).catch(toastBackendError)}
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
            <WordPressIcon className="h-4 w-4" />
          </Button>
        )}
      </div>
      <div className="flex-1" />
      {/* Metrics reserve a fixed 168px column for cross-row alignment; below
          xl that reservation is what pushed badge-heavy rows past the window
          edge, so the column yields entirely (supplementary data — SiteDetail
          has the full numbers). */}
      <div className="hidden xl:contents">
        <SiteMetrics res={resources} />
      </div>
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
      {site.docrootManaged === false && (
        /* The folder is the user's own — served in place and never deleted with
           the site. True for a linked folder and for one moved out of the sites
           folder, which is why it reads "external" rather than "linked". */
        <span
          className="flex-none whitespace-nowrap rounded-full border border-rex-border-strong bg-rex-surface-2 px-2 py-1 font-mono text-[0.625rem] text-rex-text-muted"
          title={`Served from your own folder (${site.path}) — deleting the site leaves it in place`}
        >
          external
        </span>
      )}
      {dbState === "imported" && (
        /* Renders the SAME serialized fact as the SiteDetail summary
           (DbImportRecord, whose "connected" value only the rewrite job's
           verification can write) — the badge and the summary cannot
           disagree, because neither computes anything. */
        <span
          className="flex-none whitespace-nowrap rounded-full border border-status-warning-border bg-status-warning-bg px-2 py-1 font-mono text-[0.625rem] text-status-warning-bright"
          title="A copy of this site's database is on rexenv's engine, but the site still reads and writes the old one — they drift apart until you switch it over (see the site's Database tab)."
        >
          DB imported
        </span>
      )}
      {dbState === "connected" && (
        /* Wording matches what was PROVEN: the rewritten settings sign in to
           the rexenv copy — not "the site is now using this database". */
        <span
          className="flex-none whitespace-nowrap rounded-full border border-status-running-border bg-status-running-bg px-2 py-1 font-mono text-[0.625rem] text-status-running-bright"
          title="This site's connection settings were rewritten and verified: they sign in to the rexenv copy (see the site's Database tab)."
        >
          DB connected
        </span>
      )}
      {site.provisioned ? (
        <StatusPill status={status} className="min-w-[92px]" />
      ) : (
        /* Honest half-site marker (v16): provisioning died or was cancelled —
           the site is NOT healthy-stopped. Retry re-runs the remaining
           idempotent steps; Delete (menu) removes it. */
        <span
          className="flex min-w-[92px] flex-none items-center justify-center gap-1 whitespace-nowrap rounded-full border border-status-warning-border bg-status-warning-bg px-2 py-1 font-mono text-[0.625rem] text-status-warning-bright"
          title="Provisioning did not finish — Retry re-runs the remaining steps; Delete removes the site."
        >
          setup incomplete
        </span>
      )}
      {!site.provisioned && (
        <div className="flex-none" onClick={(e) => e.stopPropagation()}>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Retry setup"
            title="Retry setup — re-runs the remaining provisioning steps"
            onClick={onRetry}
          >
            <RefreshCw className="h-4 w-4" />
          </Button>
        </div>
      )}
      <div className="flex-none" onClick={(e) => e.stopPropagation()}>
        <Menu
        trigger={
          <button
            type="button"
            aria-label="More actions"
            className="flex h-7 w-7 items-center justify-center rounded-[7px] text-rex-text-muted transition-colors hover:bg-rex-hover-strong hover:text-rex-text"
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
          onSelect={() => {
            // Open the whole site folder as a PROJECT in the user's editor
            // (preferred_editor setting, else first detected). No editor →
            // say so honestly and reveal the folder instead.
            if (editor) {
              openInEditor(editor.id, site.path).catch(toastBackendError);
            } else {
              toast.info(
                "No code editor found (VS Code, Cursor, PhpStorm, Zed, Sublime…) — opening the folder in Finder instead.",
              );
              void openExternal(site.path).catch(toastBackendError);
            }
          }}
        >
          {editor ? `Open in ${editor.name}` : "Open in editor"}
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
  // Empty-state copy names the USER'S configured TLD, not a hardcoded one.
  const { data: tld = "rex" } = useQuery({ queryKey: ["default-tld"], queryFn: defaultTld });
  // A site's displayed status is its live *serving* state, NOT the sites.status
  // column (task 2.1 / H1): serving only when the edge is up AND the site's own
  // upstream (php-fpm pool or FrankenPHP backend) is up, so a partial stack no
  // longer shows every site as running (H1 follow-up). Keyed by domain.
  const { data: serving } = useQuery({
    queryKey: ["sites-serving"],
    queryFn: getSitesServing,
    refetchInterval: 2000,
  });
  // Honest per-site resources (dedicated CPU/RAM vs activity+DB) — 5s poll:
  // each read parses the nginx access-log tail + one DB-sizes query.
  const { data: resources } = useQuery({
    queryKey: ["sites-resources"],
    queryFn: sitesResources,
    refetchInterval: 5000,
  });
  // Settled DB-import facts (v20) — drives the "DB imported · not connected"
  // badge. One query for the whole page.
  const { data: dbRecords = [] } = useQuery({
    queryKey: ["db-import-records"],
    queryFn: dbImportRecords,
  });
  const dbStates = useMemo(
    () => new Map(dbRecords.map((r) => [r.siteId, r.state])),
    [dbRecords],
  );
  const resourcesMap = useMemo(
    () => new Map((resources ?? []).map((r) => [r.id, r])),
    [resources],
  );
  const servingMap = useMemo(
    () => new Map((serving ?? []).map((s) => [s.domain, s.serving])),
    [serving],
  );
  const statusOf = (site: Site): Site["status"] =>
    servingMap.get(site.domain) ? "running" : "stopped";

  const remove = useMutation({
    mutationFn: (site: Site) => deleteSite(site.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toastBackendError(e),
  });

  // D2's default leg for a CONNECTED site: revert the connection change,
  // then delete. If the revert can't run cleanly (file edited since, backup
  // missing), stop and say so — never delete on the back of a revert that
  // didn't happen.
  const revertThenRemove = useMutation({
    mutationFn: async (site: Site) => {
      const out = await rewriteRevert(site.id, false);
      if (out.status === "reverted" || out.status === "noRewrite") {
        await deleteSite(site.id);
        return null;
      }
      return out.message;
    },
    onSuccess: (blocked, site) => {
      if (blocked) {
        toast.info(
          `${site.domain} was not deleted — ${blocked} Resolve it on the site's Database tab, or delete without reverting.`,
        );
      }
      void qc.invalidateQueries({ queryKey: ["sites"] });
      void qc.invalidateQueries({ queryKey: ["db-import-records"] });
    },
    onError: (e) => toastBackendError(e),
  });

  // Streamed provision job (New Site / Retry) — re-adopted here so a create
  // started in the dialog survives closing it. The banner shows while
  // running and stays FROZEN after a failure/cancel (the row also carries
  // the "setup incomplete" badge); a job that settled ok needs no banner.
  const downloads = useDownloads();
  const prov = useSiteProvision();
  useEffect(() => {
    void prov.adopt().then((j) => {
      if (j && j.status === "ok") prov.clear();
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const retry = useMutation({
    mutationFn: (site: Site) => siteProvisionRetry(site.id),
    onSuccess: (snap) => prov.start(snap),
    onError: (e) => toastBackendError(e),
  });

  const rename = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => renameSite(id, name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toastBackendError(e),
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
          <div className="flex items-center gap-[18px] pr-2 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-[var(--rex-placeholder)]">
            <span className="w-[118px]">Stack</span>
            <span className="w-[88px]">Status</span>
          </div>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto px-[18px] pb-[18px]">
        <ImportBanner />
        {prov.job && (prov.running || prov.job.status !== "ok") && (
          <div className="mb-2 mt-1">
            <SiteProvisionCard
              job={prov.job}
              lines={prov.lines}
              downloads={downloads}
              onCancel={() => void siteProvisionCancel(prov.job!.id).catch(toastBackendError)}
            />
            {!prov.running && (
              <button
                className="mt-1 text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
                onClick={() => prov.clear()}
              >
                Dismiss
              </button>
            )}
          </div>
        )}
        {isLoading ? (
          <Placeholder
            icon={<Globe className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="Loading sites…"
            hint="Reading your local sites"
          />
        ) : sites.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-4 p-10 text-center">
            <div className="flex h-[60px] w-[60px] items-center justify-center rounded-2xl border border-rex-crown-border bg-gradient-to-br from-rex-crown-chip-from to-rex-crown-chip-to shadow-glow-crown">
              <RexLogo className="block h-[28px] w-auto" />
            </div>
            <div>
              <div className="text-[1.1875rem] font-semibold text-rex-text">No sites yet</div>
              <div className="mt-2 max-w-[400px] text-[0.84375rem] leading-[1.55] text-rex-text-muted">
                Point rexenv at a folder and it serves your site instantly — with its
                own <span className="font-mono">.{tld}</span> domain, PHP, and database.
              </div>
            </div>
            <Button variant="primary" size="lg" onClick={() => setShowNew(true)}>
              <Plus className="h-4 w-4" strokeWidth={2.3} />
              Create your first site
            </Button>
          </div>
        ) : noResults ? (
          <div className="flex flex-col items-center justify-center gap-1.5 px-5 py-[54px] text-center">
            <div className="text-[0.875rem] font-medium text-rex-text-bright">
              {query ? `No sites match “${query}”` : "No sites in this view"}
            </div>
            <div className="text-[0.78125rem] text-rex-text-dim">
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
                resources={resourcesMap.get(site.id)}
                dbState={dbStates.get(site.id)}
                onOpen={() => navigate(`/sites/${site.id}`)}
                onDelete={() => setDeleteTarget(site)}
                onOpenDatabase={() => navigate(`/sites/${site.id}/database`)}
                onOpenWordpress={() => navigate(`/sites/${site.id}/wordpress`)}
                onRename={() => setRenameTarget(site)}
                onDuplicate={() => setDupSource(site)}
                onRetry={() => retry.mutate(site)}
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
        <DeleteSiteDialog
          site={deleteTarget}
          dbState={dbStates.get(deleteTarget.id)}
          onPlainDelete={() => {
            remove.mutate(deleteTarget);
            setDeleteTarget(null);
          }}
          onRevertThenDelete={() => {
            revertThenRemove.mutate(deleteTarget);
            setDeleteTarget(null);
          }}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </>
  );
}

/**
 * "Import from Valet or Herd" nudge.
 *
 * Only appears when a scan would ACTUALLY find something, so a user with
 * neither tool never sees it, and it is dismissible so it can't nag someone who
 * has already decided. Most useful on an empty site list — which is exactly
 * when a Valet user is wondering where their sites are.
 */
function ImportBanner() {
  const navigate = useNavigate();
  const DISMISS_KEY = "rexenv.importBannerDismissed";
  const [dismissed, setDismissed] = useState(
    () => localStorage.getItem(DISMISS_KEY) === "1",
  );
  const { data } = useQuery({
    queryKey: ["valet-scan"],
    queryFn: scanValetImport,
    enabled: !dismissed,
    staleTime: 60_000,
  });
  const ready = (data?.candidates ?? []).filter(
    (c) => c.status.status === "importable",
  ).length;
  if (dismissed || ready === 0) return null;
  return (
    <div className="mb-2 mt-1 flex items-center gap-3 rounded-xl border border-rex-border bg-rex-surface-1 px-4 py-3">
      <FolderInput className="h-4 w-4 flex-none text-brand" strokeWidth={1.7} />
      <div className="min-w-0 flex-1">
        <div className="text-[0.8125rem] text-rex-text">
          {ready} site{ready === 1 ? "" : "s"} found in Valet or Herd
        </div>
        <div className="text-[0.71875rem] text-rex-text-muted">
          Import them where they already live — nothing is copied, and your Valet setup is left
          untouched.
        </div>
      </div>
      <Button variant="secondary" onClick={() => navigate("/import")}>
        Review import
      </Button>
      <Button
        variant="ghost"
        onClick={() => {
          localStorage.setItem(DISMISS_KEY, "1");
          setDismissed(true);
        }}
      >
        Dismiss
      </Button>
    </div>
  );
}

