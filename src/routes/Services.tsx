import { useQuery } from "@tanstack/react-query";
import { Layers } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { servicesStatus } from "@/lib/ipc";
import type { ServiceInfo } from "@/types";

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
          <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
            {services.map((svc) => (
              <ServiceRow key={svc.name} svc={svc} />
            ))}
          </div>
        )}
      </div>
    </>
  );
}
