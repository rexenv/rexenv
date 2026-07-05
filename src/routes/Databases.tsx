import { useState } from "react";
import { toastBackendError } from "@/lib/toast";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, Database, TableProperties } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { AdminerFrame } from "@/components/database/AdminerFrame";
import { databasesStatus, startDatabase, stopDatabase } from "@/lib/ipc";
import { adminerUrl } from "@/lib/adminer";
import type { DbStatus } from "@/types";

function Meter({ label, value, pct }: { label: string; value: string; pct: number }) {
  return (
    <div className="w-24">
      <div className="mb-1 flex justify-between font-mono text-[10px] text-rex-text-dim">
        <span>{label}</span>
        <span className="text-rex-text-bright">{value}</span>
      </div>
      <div className="h-[5px] overflow-hidden rounded-full bg-rex-well">
        <div
          className="h-full rounded-full bg-gradient-to-r from-brand-strong to-brand-light"
          style={{ width: `${Math.min(100, pct)}%` }}
        />
      </div>
    </div>
  );
}

function DbRow({
  db,
  busy,
  onToggle,
  onBrowse,
}: {
  db: DbStatus;
  busy: boolean;
  onToggle: () => void;
  onBrowse: () => void;
}) {
  return (
    <div className="flex items-center gap-4 border-b border-rex-border-subtle px-4 py-3 last:border-b-0">
      <div className="flex h-7 w-7 flex-none items-center justify-center rounded-md border border-rex-border bg-rex-surface-2 text-rex-text-muted">
        <Database className="h-4 w-4" strokeWidth={1.7} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-semibold text-rex-text">
          {db.label}
          {db.version && (
            <span className="ml-2 font-mono text-[10.5px] text-rex-text-dim">{db.version}</span>
          )}
        </div>
        <div className="font-mono text-[11px] text-rex-text-dim">
          127.0.0.1:{db.port}
          {db.pid != null && ` · pid ${db.pid}`}
        </div>
      </div>
      <StatusPill status={db.running ? "running" : "stopped"} />
      <button
        onClick={onBrowse}
        disabled={!db.running}
        title={db.running ? "Open in database browser" : "Start the engine first"}
        className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[12px] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-rex-border"
      >
        <TableProperties className="h-3.5 w-3.5" />
        Browse
      </button>
      <Meter label="CPU" value={`${db.cpuPercent.toFixed(1)}%`} pct={db.cpuPercent} />
      <Meter
        label="RAM"
        value={db.ramMb >= 1024 ? `${(db.ramMb / 1024).toFixed(1)} GB` : `${db.ramMb} MB`}
        pct={(db.ramMb / 1024) * 100}
      />
      <StartStopToggle
        running={db.running}
        busy={busy}
        onToggle={onToggle}
        label={`${db.running ? "Stop" : "Start"} ${db.label}`}
      />
    </div>
  );
}

export function Databases() {
  const qc = useQueryClient();
  const [browse, setBrowse] = useState<{ engine: "mysql" | "postgres"; label: string } | null>(null);
  const { data: dbs = [], isLoading } = useQuery({
    queryKey: ["databases"],
    queryFn: databasesStatus,
    refetchInterval: browse ? false : 2000,
  });

  const toggle = useMutation({
    mutationFn: (db: DbStatus) => (db.running ? stopDatabase(db.key) : startDatabase(db.key)),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["databases"] }),
    onError: (e) => toastBackendError(e),
  });

  const running = dbs.filter((d) => d.running).length;

  if (browse) {
    const engine = browse.engine;
    return (
      <>
        <TopBar title="Databases" subtitle={`Browsing ${browse.label}`} showSearch={false} />
        <div className="flex items-center gap-2 border-b border-rex-border px-[18px] py-2.5">
          <button
            onClick={() => setBrowse(null)}
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[12px] text-rex-text transition-colors hover:border-brand"
          >
            <ArrowLeft className="h-3.5 w-3.5" />
            Back
          </button>
          <span className="font-mono text-[12px] text-rex-text-muted">{browse.label} · Adminer</span>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden p-[18px]">
          <AdminerFrame src={adminerUrl({ engine })} />
        </div>
      </>
    );
  }

  return (
    <>
      <TopBar
        title="Databases"
        subtitle={isLoading ? "Loading…" : `${running}/${dbs.length} running`}
        showSearch={false}
      />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        {isLoading ? (
          <Placeholder
            icon={<Database className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="Loading databases…"
            hint="Reading live engine status"
          />
        ) : (
          <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
            {dbs.map((db) => (
              <DbRow
                key={db.key}
                db={db}
                busy={toggle.isPending && toggle.variables?.key === db.key}
                onToggle={() => toggle.mutate(db)}
                onBrowse={() =>
                  setBrowse({
                    engine: db.key === "postgres" ? "postgres" : "mysql",
                    label: db.label,
                  })
                }
              />
            ))}
          </div>
        )}
      </div>
    </>
  );
}
