import { Plus, Globe, FolderOpen, Database, Lock, MoreHorizontal } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { StatusPill } from "@/components/common/StatusPill";
import { Button } from "@/components/ui/button";
import { mockSites } from "@/lib/mock";
import type { Site } from "@/types";

function Badge({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded border border-rex-border bg-rex-surface-2 px-1.5 py-0.5 font-mono text-[10.5px] text-rex-text-bright">
      {children}
    </span>
  );
}

function SiteRow({ site }: { site: Site }) {
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
      <div className="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
        <Button variant="ghost" size="icon" aria-label="Open in browser">
          <Globe className="h-4 w-4" />
        </Button>
        <Button variant="ghost" size="icon" aria-label="Open folder">
          <FolderOpen className="h-4 w-4" />
        </Button>
        <Button variant="ghost" size="icon" aria-label="Open database">
          <Database className="h-4 w-4" />
        </Button>
        <Button variant="ghost" size="icon" aria-label="More actions">
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}

export function Sites() {
  const running = mockSites.filter((s) => s.status === "running").length;
  return (
    <>
      <TopBar
        title="Sites"
        subtitle={`${mockSites.length} sites · ${running} running`}
        action={
          <Button variant="primary">
            <Plus className="h-[15px] w-[15px]" strokeWidth={2.2} />
            New site
          </Button>
        }
      />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
          {mockSites.map((site) => (
            <SiteRow key={site.id} site={site} />
          ))}
        </div>
      </div>
    </>
  );
}
