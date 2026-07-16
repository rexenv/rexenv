import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, ArrowUpRight, Database, TableProperties } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { AdminerFrame } from "@/components/database/AdminerFrame";
import { confirm } from "@/components/ui/dialog";
import { databasesStatus, dbEngineVersions, setDbEngineVersion } from "@/lib/ipc";
import { adminerFrameSrc } from "@/lib/adminer";
import type { DbStatus } from "@/types";

function Meter({ label, value, pct }: { label: string; value: string; pct: number }) {
  return (
    <div className="w-24">
      <div className="mb-1 flex justify-between font-mono text-[0.625rem] text-rex-text-dim">
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

/** Engines Adminer can browse (MariaDB via the MySQL-protocol driver). Redis
 * has no Adminer driver — its row gets no Browse button (the port +
 * `redis-cli` are the client story, shown inline). */
const BROWSABLE = new Set(["mysql", "mariadb", "postgres"]);

function DbRow({
  db,
  versions,
  onSwitchVersion,
  onBrowse,
  onGoServices,
}: {
  db: DbStatus;
  /** Offered versions for this engine (default first); a one-entry set hides the picker. */
  versions: string[];
  onSwitchVersion: (v: string) => void;
  onBrowse: () => void;
  /** Engine lifecycle lives on the Services page (P2-7) — link, never a dead button. */
  onGoServices: () => void;
}) {
  return (
    // flex-wrap + a name floor: the stopped-engine row (pill + long "Start …
    // from Services" button + two meters) is wider than the 980px min window
    // can spare — controls reflow to a second line instead of crushing and
    // overlapping the name block.
    <div className="flex flex-wrap items-center gap-4 border-b border-rex-border-subtle px-4 py-3 last:border-b-0">
      <div className="flex h-7 w-7 flex-none items-center justify-center rounded-md border border-rex-border bg-rex-surface-2 text-rex-text-muted">
        <Database className="h-4 w-4" strokeWidth={1.7} />
      </div>
      <div className="min-w-[9rem] flex-1">
        <div className="flex items-center text-[0.84375rem] font-semibold text-rex-text">
          {db.label}
          {versions.length > 1 ? (
            <select
              value={db.version}
              onChange={(e) => onSwitchVersion(e.target.value)}
              title="Switch the engine version (each version keeps its own data directory)"
              className="ml-2 h-[22px] rounded border border-rex-border bg-rex-surface-2 px-1 font-mono text-[0.65625rem] text-rex-text-muted outline-none transition-colors hover:border-brand focus:border-brand"
            >
              {versions.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          ) : (
            db.version && (
              <span className="ml-2 font-mono text-[0.65625rem] text-rex-text-dim">{db.version}</span>
            )
          )}
        </div>
        <div className="font-mono text-[0.6875rem] text-rex-text-dim">
          127.0.0.1:{db.port}
          {db.pid != null && ` · pid ${db.pid}`}
        </div>
      </div>
      <StatusPill status={db.running ? "running" : "stopped"} />
      {db.running ? (
        BROWSABLE.has(db.key) ? (
          <button
            onClick={onBrowse}
            title="Open in database browser"
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand"
          >
            <TableProperties className="h-3.5 w-3.5" />
            Browse
          </button>
        ) : (
          <span
            title={`No browser for ${db.label} — connect with redis-cli -p ${db.port}`}
            className="font-mono text-[0.6875rem] text-rex-text-dim"
          >
            redis-cli -p {db.port}
          </span>
        )
      ) : (
        // Never a dead button: the engine is stopped and its lifecycle lives
        // on the Services page — take the user there.
        <button
          onClick={onGoServices}
          title={`${db.label} is stopped — start it from the Services page`}
          className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text-muted transition-colors hover:border-brand hover:text-rex-text"
        >
          <ArrowUpRight className="h-3.5 w-3.5" />
          Start {db.label} from Services
        </button>
      )}
      <Meter label="CPU" value={`${db.cpuPercent.toFixed(1)}%`} pct={db.cpuPercent} />
      <Meter
        label="RAM"
        value={db.ramMb >= 1024 ? `${(db.ramMb / 1024).toFixed(1)} GB` : `${db.ramMb} MB`}
        pct={(db.ramMb / 1024) * 100}
      />
    </div>
  );
}

export function Databases() {
  const navigate = useNavigate();
  const [browse, setBrowse] = useState<{
    engine: "mysql" | "mariadb" | "postgres";
    label: string;
  } | null>(null);
  const { data: dbs = [], isLoading } = useQuery({
    queryKey: ["databases"],
    queryFn: databasesStatus,
    refetchInterval: browse ? false : 2000,
  });
  const { data: versions = {} } = useQuery({
    queryKey: ["db-engine-versions"],
    queryFn: dbEngineVersions,
    staleTime: Infinity, // pinned sets only change with an app release
  });
  const queryClient = useQueryClient();
  const switchVersion = useMutation({
    mutationFn: ({ key, version }: { key: string; version: string }) =>
      setDbEngineVersion(key, version),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["databases"] }),
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
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand"
          >
            <ArrowLeft className="h-3.5 w-3.5" />
            Back
          </button>
          <span className="font-mono text-[0.75rem] text-rex-text-muted">{browse.label} · Adminer</span>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden p-[18px]">
          <AdminerFrame src={adminerFrameSrc({ engine })} />
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
                versions={versions[db.key] ?? []}
                onSwitchVersion={async (v) => {
                  if (v === db.version || switchVersion.isPending) return;
                  // A select mis-click must not restart a database server —
                  // confirm, and be honest about per-version data dirs.
                  const ok = await confirm({
                    title: `Switch ${db.label} to ${v}?`,
                    message:
                      `Each version keeps its own data directory: databases created on ` +
                      `${db.version} stay with ${db.version} and won't be visible on ${v} ` +
                      `(export first to move data). ` +
                      (db.running
                        ? `${db.label} restarts on ${v} now.`
                        : `${db.label} will use ${v} on its next start.`),
                    confirmLabel: "Switch",
                  });
                  if (ok) switchVersion.mutate({ key: db.key, version: v });
                }}
                onBrowse={() =>
                  setBrowse({
                    engine:
                      db.key === "postgres" || db.key === "mariadb" ? db.key : "mysql",
                    label: db.label,
                  })
                }
                onGoServices={() => navigate("/services")}
              />
            ))}
          </div>
        )}
      </div>
    </>
  );
}
