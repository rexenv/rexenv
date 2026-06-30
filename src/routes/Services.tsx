import { useQuery } from "@tanstack/react-query";
import { Code, Database, Layers, Mail, Server, type LucideIcon } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { servicesStatus } from "@/lib/ipc";
import type { ServiceInfo, ServiceKind } from "@/types";

/** Group a service — prefer the backend hint, else derive from the name. */
function serviceKind(svc: ServiceInfo): ServiceKind {
  if (svc.kind) return svc.kind;
  const n = svc.name.toLowerCase();
  if (n.includes("php")) return "php";
  if (n.includes("mysql") || n.includes("maria") || n.includes("postgres")) return "database";
  if (n.includes("mail")) return "mail";
  return "web";
}

const GROUPS: { kind: ServiceKind; title: string; icon: LucideIcon; color: string }[] = [
  { kind: "php", title: "PHP", icon: Code, color: "#A7AADD" },
  { kind: "database", title: "Databases", icon: Database, color: "#7DB8D8" },
  { kind: "mail", title: "Mail", icon: Mail, color: "#D7A93A" },
  { kind: "web", title: "Web servers & edge router", icon: Server, color: "#5FBFA8" },
];

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

const RAM_BUDGET_MB = 4096;

function UsageMetric({
  label,
  value,
  unit,
  pct,
}: {
  label: string;
  value: number;
  unit: string;
  pct: number;
}) {
  return (
    <div>
      <div className="mb-2 flex items-baseline justify-between">
        <span className="text-[12.5px] text-rex-text-muted">{label}</span>
        <span className="font-mono text-[20px] font-semibold text-rex-text">
          {value}
          <span className="text-[13px] text-rex-text-dim">{unit}</span>
        </span>
      </div>
      <div className="h-2 overflow-hidden rounded-full border border-[#23262F] bg-[#0B0C10]">
        <div
          className="h-full rounded-full bg-gradient-to-r from-brand-strong to-brand-light shadow-glow-primary transition-[width] duration-700"
          style={{ width: `${Math.min(100, pct)}%` }}
        />
      </div>
    </div>
  );
}

/** The headline "Total resource usage" card: aggregate CPU / Memory + running/idle. */
function TotalUsageCard({ services }: { services: ServiceInfo[] }) {
  const running = services.filter((s) => s.running).length;
  const idle = services.length - running;
  const totalCpu = Math.round(services.reduce((a, s) => a + s.cpuPercent, 0));
  const totalRam = services.reduce((a, s) => a + s.ramMb, 0);

  return (
    <div className="mb-[18px] rounded-[15px] border border-[#23262F] bg-gradient-to-br from-[#171A21] to-[#121419] px-5 py-[18px]">
      <div className="mb-4 flex items-center gap-[9px]">
        <span className="relative inline-flex h-2 w-2">
          <span className="absolute inset-0 rounded-full bg-status-running opacity-50 animate-rex-ping motion-reduce:animate-none" />
          <span className="relative h-2 w-2 rounded-full bg-status-running shadow-glow-run" />
        </span>
        <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-rex-text-muted">
          Total resource usage
        </span>
        <span className="flex-1" />
        <span className="font-mono text-[10.5px] text-rex-text-faint">
          live · updates every 2s
        </span>
      </div>
      <div className="grid grid-cols-3 gap-[22px]">
        <UsageMetric label="CPU" value={totalCpu} unit="%" pct={totalCpu} />
        <div>
          <UsageMetric
            label="Memory"
            value={totalRam}
            unit=" MB"
            pct={(totalRam / RAM_BUDGET_MB) * 100}
          />
          <div className="mt-[5px] font-mono text-[10px] text-rex-text-label">
            of 4.0 GB budget · stays light
          </div>
        </div>
        <div className="flex items-center gap-[18px] border-l border-[#23262F] pl-4">
          <div>
            <div className="font-mono text-[22px] font-semibold text-status-running-bright">
              {running}
            </div>
            <div className="mt-0.5 text-[11px] text-rex-text-muted">running</div>
          </div>
          <div>
            <div className="font-mono text-[22px] font-semibold text-rex-text-dim">{idle}</div>
            <div className="mt-0.5 text-[11px] text-rex-text-muted">idle</div>
          </div>
        </div>
      </div>
    </div>
  );
}

function ServiceRow({ svc }: { svc: ServiceInfo }) {
  return (
    <div className="flex items-center gap-4 border-b border-rex-border-subtle px-4 py-3 last:border-b-0">
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-semibold text-rex-text">{svc.name}</div>
        <div className="font-mono text-[11px] text-rex-text-dim">
          127.0.0.1:{svc.port}
          {svc.pid != null && ` · pid ${svc.pid}`}
        </div>
      </div>
      <StatusPill status={svc.running ? "running" : "stopped"} />
      <Meter
        label="CPU"
        value={`${svc.cpuPercent.toFixed(1)}%`}
        pct={svc.cpuPercent}
      />
      <Meter
        label="RAM"
        value={svc.ramMb >= 1024 ? `${(svc.ramMb / 1024).toFixed(1)} GB` : `${svc.ramMb} MB`}
        pct={(svc.ramMb / 1024) * 100}
      />
    </div>
  );
}

export function Services() {
  const { data: services = [], isLoading } = useQuery({
    queryKey: ["services"],
    queryFn: servicesStatus,
    refetchInterval: 2000,
  });

  const running = services.filter((s) => s.running).length;

  return (
    <>
      <TopBar
        title="Services"
        subtitle={
          isLoading ? "Loading…" : `${running}/${services.length} running`
        }
        showSearch={false}
      />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        {isLoading ? (
          <Placeholder
            icon={<Layers className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="Loading services…"
            hint="Reading live service status"
          />
        ) : (
          <>
            <TotalUsageCard services={services} />
            {GROUPS.map((g) => {
              const rows = services.filter((s) => serviceKind(s) === g.kind);
              if (rows.length === 0) return null;
              const run = rows.filter((r) => r.running).length;
              const Icon = g.icon;
              return (
                <div key={g.kind} className="mb-[18px] last:mb-0">
                  <div className="mb-[9px] flex items-center gap-2.5 px-0.5">
                    <span className="flex" style={{ color: g.color }}>
                      <Icon className="h-[15px] w-[15px]" strokeWidth={1.7} />
                    </span>
                    <span className="text-[13.5px] font-semibold text-rex-text">{g.title}</span>
                    <span className="font-mono text-[11px] text-rex-text-dim">
                      {run}/{rows.length} running
                    </span>
                  </div>
                  <div className="overflow-hidden rounded-[13px] border border-rex-border-subtle bg-rex-surface-1">
                    {rows.map((svc) => (
                      <ServiceRow key={svc.name} svc={svc} />
                    ))}
                  </div>
                </div>
              );
            })}
          </>
        )}
      </div>
    </>
  );
}
