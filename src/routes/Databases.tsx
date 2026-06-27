import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Database } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { databasesStatus, startDatabase, stopDatabase } from "@/lib/ipc";
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
}: {
  db: DbStatus;
  busy: boolean;
  onToggle: () => void;
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
      <Meter label="CPU" value={`${db.cpuPercent.toFixed(1)}%`} pct={db.cpuPercent} />
      <Meter
        label="RAM"
        value={db.ramMb >= 1024 ? `${(db.ramMb / 1024).toFixed(1)} GB` : `${db.ramMb} MB`}
        pct={(db.ramMb / 1024) * 100}
      />
      <StartStopToggle running={db.running} busy={busy} onToggle={onToggle} />
    </div>
  );
}

export function Databases() {
  const qc = useQueryClient();
  const { data: dbs = [], isLoading } = useQuery({
    queryKey: ["databases"],
    queryFn: databasesStatus,
    refetchInterval: 2000,
  });

  const toggle = useMutation({
    mutationFn: (db: DbStatus) => (db.running ? stopDatabase(db.key) : startDatabase(db.key)),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["databases"] }),
    onError: (e) => window.alert(String(e)),
  });

  const running = dbs.filter((d) => d.running).length;

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
              />
            ))}
          </div>
        )}
      </div>
    </>
  );
}
