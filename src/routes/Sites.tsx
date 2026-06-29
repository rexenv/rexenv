import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, Globe, FolderOpen, Database, Lock, Trash2, MoreHorizontal } from "lucide-react";
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
      <StartStopToggle running={running} busy={busy} onToggle={onToggle} />
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
  const { data: sites = [], isLoading } = useQuery({
    queryKey: ["sites"],
    queryFn: listSites,
  });

  const toggle = useMutation({
    mutationFn: (site: Site) =>
      site.status === "running" ? stopSite(site.id) : startSite(site.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
  });

  const remove = useMutation({
    mutationFn: (site: Site) => deleteSite(site.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
  });

  const confirmDelete = (site: Site) => {
    if (window.confirm(`Delete "${site.name}" (${site.domain})? This removes its files and certificate.`)) {
      remove.mutate(site);
    }
  };

  const running = sites.filter((s) => s.status === "running").length;
  const newSiteButton = (
    <Button variant="primary" onClick={() => setShowNew(true)}>
      <Plus className="h-[15px] w-[15px]" strokeWidth={2.2} />
      New site
    </Button>
  );

  return (
    <>
      <TopBar
        title="Sites"
        subtitle={
          isLoading ? "Loading…" : `${sites.length} sites · ${running} running`
        }
        action={newSiteButton}
      />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
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
        ) : (
          <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
            {sites.map((site) => (
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
