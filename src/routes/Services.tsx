import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { Code, Database, ExternalLink, Inbox, Layers, Mail, Play, Server, Square, type LucideIcon } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { cn } from "@/lib/utils";
import { servicesStatus, setDefaultPhpVersion, startServices, stopServices } from "@/lib/ipc";
import type { ServiceInfo, ServiceKind } from "@/types";

/** Tinted accent per kind (matches the group icon colors). */
const KIND_ACCENT: Record<ServiceKind, { bg: string; border: string; color: string }> = {
  php: { bg: "rgba(125,128,185,0.17)", border: "rgba(125,128,185,0.32)", color: "#A7AADD" },
  database: { bg: "rgba(74,134,170,0.15)", border: "rgba(74,134,170,0.30)", color: "#7DB8D8" },
  mail: { bg: "rgba(210,153,34,0.13)", border: "rgba(210,153,34,0.28)", color: "#D7A93A" },
  web: { bg: "rgba(45,156,143,0.13)", border: "rgba(45,156,143,0.28)", color: "#5FBFA8" },
};

/** A short monogram for the row's accent badge. */
function serviceBadge(svc: ServiceInfo, kind: ServiceKind): string {
  if (kind === "php") return phpMinor(svc);
  const n = svc.name;
  if (/mysql/i.test(n)) return "My";
  if (/postgres/i.test(n)) return "Pg";
  if (/maria/i.test(n)) return "Ma";
  if (/mail/i.test(n)) return "Mp";
  if (/nginx/i.test(n)) return "Nx";
  if (/caddy/i.test(n)) return "Cd";
  if (/franken/i.test(n)) return "Fp";
  return n.slice(0, 2);
}

/** Extract the PHP minor (e.g. "8.3") from version or name. */
function phpMinor(svc: ServiceInfo): string {
  const m = (svc.version ?? svc.name).match(/(\d+\.\d+)/);
  return m ? m[1] : svc.name;
}

function ActionBtn({
  children,
  onClick,
  icon,
  accent,
}: {
  children: React.ReactNode;
  onClick: () => void;
  icon?: React.ReactNode;
  accent?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex h-[29px] items-center gap-1.5 rounded-lg border border-rex-border-strong bg-rex-surface-2 px-3 text-[12px] font-medium transition-colors hover:bg-rex-surface-2-hover",
        accent ? "text-brand-tint" : "text-rex-text-bright",
      )}
    >
      {icon}
      {children}
    </button>
  );
}

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

/** Stacked CPU-over-RAM mini-meters (design scaling: cpu/12, ram/500). */
function StackedMeters({ cpu, ram }: { cpu: number; ram: number }) {
  const bar = (label: string, pct: number, value: string) => (
    <div className="flex items-center gap-[7px]">
      <span className="w-[22px] font-mono text-[8.5px] tracking-[0.06em] text-rex-text-dim">
        {label}
      </span>
      <span className="h-1 flex-1 overflow-hidden rounded-full bg-[#0B0C10]">
        <span
          className="block h-full rounded-full bg-gradient-to-r from-brand-strong to-brand-light transition-[width] duration-700"
          style={{ width: `${Math.min(100, pct)}%` }}
        />
      </span>
      <span className="w-8 text-right font-mono text-[9.5px] text-rex-text-bright">{value}</span>
    </div>
  );
  return (
    <div className="flex w-[132px] flex-none flex-col gap-1">
      {bar("CPU", (cpu / 12) * 100, `${cpu.toFixed(1)}%`)}
      {bar("RAM", (ram / 500) * 100, ram >= 1024 ? `${(ram / 1024).toFixed(1)}G` : `${ram}M`)}
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

function ServiceRow({
  svc,
  onToggle,
  onSetDefault,
  onOpenDatabases,
  onOpenMail,
}: {
  svc: ServiceInfo;
  onToggle: () => void;
  onSetDefault: (minor: string) => void;
  onOpenDatabases: () => void;
  onOpenMail: () => void;
}) {
  const kind = serviceKind(svc);
  const accent = KIND_ACCENT[kind];
  const running = svc.running;
  return (
    <div
      className={cn(
        "flex items-center gap-4 border-b border-rex-border-subtle px-4 py-3 transition-opacity last:border-b-0",
        !running && "opacity-[0.74]",
      )}
    >
      <div className="flex min-w-0 flex-1 items-center gap-[11px]">
        <span
          className="flex h-[30px] w-[30px] flex-none items-center justify-center rounded-lg border font-mono text-[10px] font-bold"
          style={{ background: accent.bg, color: accent.color, borderColor: accent.border }}
        >
          {serviceBadge(svc, kind)}
        </span>
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-[13.5px] font-semibold text-rex-text">{svc.name}</span>
            {svc.isDefault && (
              <span className="rounded-md border border-[rgba(124,92,255,0.28)] bg-brand-tint-bg px-[7px] py-0.5 font-mono text-[9.5px] text-brand-tint">
                default
              </span>
            )}
            {svc.isRouter && (
              <span className="rounded-md border border-[rgba(45,156,143,0.28)] bg-[rgba(45,156,143,0.12)] px-[7px] py-0.5 font-mono text-[9.5px] text-[#5FBFA8]">
                edge router
              </span>
            )}
          </div>
          <div className="font-mono text-[10.5px] text-rex-text-dim">{svc.version ?? "—"}</div>
        </div>
      </div>
      <div className="w-[62px] flex-none font-mono text-[11.5px] text-rex-text-dim">
        :{svc.port}
      </div>
      <StackedMeters cpu={svc.cpuPercent} ram={svc.ramMb} />
      <StatusPill
        status={running ? "running" : "stopped"}
        label={running ? undefined : "Idle"}
        className="w-[86px]"
      />
      <StartStopToggle
        running={running}
        onToggle={onToggle}
        label={`${running ? "Stop" : "Start"} ${svc.name}`}
      />
      <div className="flex w-[124px] flex-none justify-end">
        {kind === "php" && !svc.isDefault && (
          <ActionBtn accent onClick={() => onSetDefault(phpMinor(svc))}>
            Set default
          </ActionBtn>
        )}
        {kind === "database" && (
          <ActionBtn icon={<ExternalLink className="h-3.5 w-3.5" />} onClick={onOpenDatabases}>
            Open
          </ActionBtn>
        )}
        {kind === "mail" && (
          <ActionBtn icon={<Inbox className="h-3.5 w-3.5" />} onClick={onOpenMail}>
            Open inbox
          </ActionBtn>
        )}
      </div>
    </div>
  );
}

export function Services() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { data: services = [], isLoading } = useQuery({
    queryKey: ["services"],
    queryFn: servicesStatus,
    refetchInterval: 2000,
  });

  const setDefault = useMutation({
    mutationFn: (minor: string) => setDefaultPhpVersion(minor),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["services"] });
      qc.invalidateQueries({ queryKey: ["php-versions"] });
    },
    onError: (e) => window.alert(String(e)),
  });

  // TODO: per-service start/stop needs a backend `start_service`/`stop_service`
  // command — only whole-stack startServices/stopServices exist today (§5.4).
  const onServiceToggle = () => {};

  const running = services.filter((s) => s.running).length;
  const isStart = running === 0;
  const toggleAll = useMutation({
    mutationFn: () => (isStart ? startServices() : stopServices()),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["services"] }),
    onError: (e) => window.alert(String(e)),
  });

  const startStopAll = (
    <button
      onClick={() => toggleAll.mutate()}
      disabled={toggleAll.isPending || isLoading}
      className={cn(
        "flex h-9 items-center gap-2 rounded-[9px] px-4 text-[13px] font-medium transition-[filter] hover:brightness-110 focus-visible:outline-none disabled:opacity-60",
        isStart
          ? "bg-primary text-white shadow-glow-primary"
          : "border border-rex-border-strong bg-rex-surface-2 text-rex-text-bright",
      )}
    >
      {isStart ? (
        <Play className="h-3 w-3 fill-current" />
      ) : (
        <Square className="h-3 w-3 fill-current" />
      )}
      {isStart ? "Start all" : "Stop all"}
    </button>
  );

  return (
    <>
      <TopBar
        title="Services"
        subtitle={
          isLoading ? "Loading…" : `${running} of ${services.length} running`
        }
        showSearch={false}
        action={startStopAll}
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
                      <ServiceRow
                        key={svc.name}
                        svc={svc}
                        onToggle={onServiceToggle}
                        onSetDefault={(minor) => setDefault.mutate(minor)}
                        onOpenDatabases={() => navigate("/databases")}
                        onOpenMail={() => navigate("/mail")}
                      />
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
