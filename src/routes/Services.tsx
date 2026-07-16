import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast, toastBackendError } from "@/lib/toast";
import { useNavigate } from "react-router-dom";
import { Code, Database, ExternalLink, Globe, Inbox, Layers, Mail, Server, type LucideIcon } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { cn } from "@/lib/utils";
import { defaultTld, dnsStatus, servicesStatus, setDefaultPhpVersion, startDatabase, startMail, stopDatabase, stopMail } from "@/lib/ipc";
import type { ServiceInfo, ServiceKind } from "@/types";

/** Tinted accent per kind (matches the group icon colors). */
const KIND_ACCENT: Record<ServiceKind, { bg: string; border: string; color: string }> = {
  php: { bg: "var(--rex-accent-periwinkle-bg)", border: "var(--rex-accent-periwinkle-border)", color: "var(--rex-accent-periwinkle)" },
  database: { bg: "var(--rex-accent-blue-bg)", border: "var(--rex-accent-blue-border)", color: "var(--rex-accent-blue)" },
  mail: { bg: "var(--rex-accent-amber-bg)", border: "var(--rex-accent-amber-border)", color: "var(--rex-accent-amber)" },
  web: { bg: "var(--rex-accent-teal-bg)", border: "var(--rex-accent-teal-border)", color: "var(--rex-accent-teal)" },
};

/** A short monogram for the row's accent badge. */
function serviceBadge(svc: ServiceInfo, kind: ServiceKind): string {
  // Per-site FrankenPHP rows get the marker, never the (long) name/domain —
  // the badge is a 30px circle. Shared pools show their minor ("8.3").
  if (svc.domain) return "Fp";
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
        "flex h-[29px] items-center gap-1.5 rounded-lg border border-rex-border-strong bg-rex-surface-2 px-3 text-[0.75rem] font-medium transition-colors hover:bg-rex-surface-2-hover",
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
  { kind: "php", title: "PHP", icon: Code, color: "var(--rex-accent-periwinkle)" },
  { kind: "database", title: "Databases", icon: Database, color: "var(--rex-accent-blue)" },
  { kind: "mail", title: "Mail", icon: Mail, color: "var(--rex-accent-amber)" },
  { kind: "web", title: "Web servers & edge router", icon: Server, color: "var(--rex-accent-teal)" },
];

/** Stacked CPU-over-RAM mini-meters (design scaling: cpu/12, ram/500). */
function StackedMeters({ cpu, ram }: { cpu: number; ram: number }) {
  const bar = (label: string, pct: number, value: string) => (
    <div className="flex items-center gap-[7px]">
      <span className="w-[22px] font-mono text-[0.53125rem] tracking-[0.06em] text-rex-text-dim">
        {label}
      </span>
      <span className="h-1 flex-1 overflow-hidden rounded-full bg-rex-well-deep">
        <span
          className="block h-full rounded-full bg-gradient-to-r from-brand-strong to-brand-light transition-[width] duration-700"
          style={{ width: `${Math.min(100, pct)}%` }}
        />
      </span>
      <span className="w-8 text-right font-mono text-[0.59375rem] text-rex-text-bright">{value}</span>
    </div>
  );
  return (
    <div className="flex w-[112px] flex-none flex-col gap-1">
      {bar("CPU", (cpu / 12) * 100, `${cpu.toFixed(1)}%`)}
      {bar("RAM", (ram / 500) * 100, ram >= 1024 ? `${(ram / 1024).toFixed(1)}G` : `${ram}M`)}
    </div>
  );
}


function ServiceRow({
  svc,
  onSetDefault,
  onOpenDatabases,
  onOpenMail,
  toggleBusy,
  onToggle,
}: {
  svc: ServiceInfo;
  onSetDefault: (minor: string) => void;
  onOpenDatabases: () => void;
  onOpenMail: () => void;
  /** In-flight state for THIS row's start/stop (independent services only). */
  toggleBusy: boolean;
  onToggle: (key: string, start: boolean) => void;
}) {
  const kind = serviceKind(svc);
  const accent = KIND_ACCENT[kind];
  const running = svc.running;
  return (
    <div
      className={cn(
        // NO transition-opacity: opacity<1 promotes the row to its own compositing
        // layer, and WKWebView animating that layer WHILE the pill's label swaps
        // composites the stale frame over the new text — the overlapping-words
        // glitch on Running->Idle (idle = the direction that ADDS the layer and
        // stops all animation, so the ghost lingers). Dim instantly instead.
        // gap-3 (not 4): at the 980px min window + 1.1x type scale the fixed
        // columns left the flex-1 name < 30px — tighter gaps keep names readable.
        "flex items-center gap-3 border-b border-rex-border-subtle px-4 py-3 last:border-b-0",
        !running && "opacity-[0.74]",
      )}
    >
      <div className="flex min-w-0 flex-1 items-center gap-[11px]">
        <span
          className="flex h-[30px] w-[30px] flex-none items-center justify-center rounded-lg border font-mono text-[0.625rem] font-bold"
          style={{ background: accent.bg, color: accent.color, borderColor: accent.border }}
        >
          {serviceBadge(svc, kind)}
        </span>
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate text-[0.84375rem] font-semibold text-rex-text">
              {svc.domain ? "FrankenPHP" : svc.name}
            </span>
            {svc.domain && svc.version && (
              <span className="font-mono text-[0.65625rem] text-rex-text-dim">{svc.version}</span>
            )}
          </div>
          {svc.domain && (
            <div className="truncate font-mono text-[0.6875rem] text-rex-text-dim">{svc.domain}</div>
          )}
        </div>
      </div>
      <div className="w-[62px] flex-none font-mono text-[0.71875rem] text-rex-text-dim">
        :{svc.port}
      </div>
      <StackedMeters cpu={svc.cpuPercent} ram={svc.ramMb} />
      <StatusPill
        status={running ? "running" : "stopped"}
        label={running ? undefined : "Idle"}
        // 92px matches Sites.tsx. 86px fit "Running" with ZERO slack in Chrome;
        // WKWebView's slightly wider Inter metrics overflowed the exact-fit pill
        // and wrapped the label inside the fixed-height pill — rendering as two
        // overlapping words during Idle→Running flips (the reported glitch).
        className="min-w-[92px]"
      />
      <div className="flex w-[124px] flex-none justify-end">
        {/* Only shared POOL rows carry isDefault (backend sends it for those
            alone) — FrankenPHP per-site rows can never grow this control. */}
        {svc.isDefault === false && (
          <ActionBtn accent onClick={() => onSetDefault(phpMinor(svc))}>
            Set default
          </ActionBtn>
        )}
        {svc.isDefault === true && (
          <span
            className="flex h-[29px] items-center rounded-lg border border-rex-border-subtle bg-rex-well px-3 font-mono text-[0.65625rem] uppercase tracking-[0.08em] text-rex-text-muted"
            title="New sites use this PHP version (change in Settings or here)"
          >
            Default
          </span>
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
      <div className="flex w-[132px] flex-none items-center justify-end">
        {svc.serviceKey ? (
          // Independent service (DB engine / Mailpit): safe to toggle alone.
          <StartStopToggle
            running={running}
            busy={toggleBusy}
            onToggle={() => onToggle(svc.serviceKey!, !running)}
            label={`${running ? "Stop" : "Start"} ${svc.name}`}
          />
        ) : (
          // Serving core (edge → web server → PHP): ONE organism — stopping a
          // single piece would 502 every site, so no per-row toggle by design.
          <span
            className="cursor-help whitespace-nowrap rounded-md border border-rex-border-subtle bg-rex-well px-2 py-1 font-mono text-[0.59375rem] uppercase tracking-[0.07em] text-rex-text-dim"
            title="Part of the serving stack (edge → web server → PHP). These start and stop together — use Start all / Stop all / Restart in the sidebar. Stopping one alone would break every site."
          >
            via Start/Stop all
          </span>
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

  // Per-row start/stop for the INDEPENDENT services (DB engines, Mailpit).
  const toggleService = useMutation({
    mutationFn: ({ key, start }: { key: string; start: boolean }) => {
      if (key === "mailpit") return start ? startMail() : stopMail();
      return start ? startDatabase(key) : stopDatabase(key);
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["services"] });
      qc.invalidateQueries({ queryKey: ["global-status"] });
      qc.invalidateQueries({ queryKey: ["databases"] });
    },
    onError: (e) => toastBackendError(e),
  });

  const setDefault = useMutation({
    mutationFn: (minor: string) => setDefaultPhpVersion(minor),
    onSuccess: (_res, minor) => {
      // Visible feedback + keep Settings' PHP cards in sync (shared query keys).
      toast.success(`PHP ${minor} is now the default for new sites.`);
      qc.invalidateQueries({ queryKey: ["services"] });
      qc.invalidateQueries({ queryKey: ["php-versions"] });
    },
    onError: (e) => toastBackendError(e),
  });

  const running = services.filter((s) => s.running).length;

  // The embedded DNS resolver — app-lifetime, never part of Start/Stop all, so
  // it lives outside the stoppable groups (and outside running/total counts).
  const { data: dns } = useQuery({
    queryKey: ["dns-status"],
    queryFn: dnsStatus,
    refetchInterval: 5000,
  });
  // The user's configured TLD — the DNS card copy must describe THEIR domains,
  // not a hardcoded one (they can change it in Settings).
  const { data: tld = "rex" } = useQuery({ queryKey: ["default-tld"], queryFn: defaultTld });

  return (
    <>
      <TopBar
        title="Services"
        subtitle={
          isLoading ? "Loading…" : `${running} of ${services.length} running`
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
                    <span className="text-[0.84375rem] font-semibold text-rex-text">{g.title}</span>
                    <span className="font-mono text-[0.6875rem] text-rex-text-dim">
                      {run}/{rows.length} running
                    </span>
                  </div>
                  <div className="overflow-hidden rounded-[13px] border border-rex-border-subtle bg-rex-surface-1">
                    {rows.map((svc) => (
                      <ServiceRow
                        key={svc.name}
                        svc={svc}
                        onSetDefault={(minor) => setDefault.mutate(minor)}
                        onOpenDatabases={() => navigate("/databases")}
                        onOpenMail={() => navigate("/mail")}
                        toggleBusy={
                          toggleService.isPending &&
                          toggleService.variables?.key === svc.serviceKey
                        }
                        onToggle={(key, start) => toggleService.mutate({ key, start })}
                      />
                    ))}
                  </div>
                </div>
              );
            })}

            {/* Always-on DNS resolver — app-lifetime, NOT controlled by
                Start/Stop all (a dead resolver breaks every local site domain,
                so it must stay visible; the watchdog restarts it automatically). */}
            {dns && (
              <div className="mb-[18px] last:mb-0">
                <div className="mb-[9px] flex items-center gap-2.5 px-0.5">
                  <span className="flex text-rex-text-dim">
                    <Globe className="h-[15px] w-[15px]" strokeWidth={1.7} />
                  </span>
                  <span className="text-[0.84375rem] font-semibold text-rex-text">Always on</span>
                  <span className="font-mono text-[0.6875rem] text-rex-text-dim">
                    {dns.mode === "agent"
                      ? "survives app quits — not affected by Stop all"
                      : "runs with the app — not affected by Stop all"}
                  </span>
                </div>
                <div className="overflow-hidden rounded-[13px] border border-rex-border-subtle bg-rex-surface-1">
                  <div className="flex items-center gap-4 px-4 py-3">
                    <div className="flex min-w-0 flex-1 items-center gap-[11px]">
                      <span className="flex h-[30px] w-[30px] flex-none items-center justify-center rounded-lg border border-rex-border-strong bg-rex-surface-2 font-mono text-[0.625rem] font-bold text-rex-text-bright">
                        Dn
                      </span>
                      <div className="min-w-0">
                        <span className="text-[0.84375rem] font-semibold text-rex-text">
                          DNS resolver
                        </span>
                        {/* Mode is user-facing state: agent survives quits;
                            in-process is the DEGRADED fallback (dies with the
                            app) and must be honestly amber, never blended into
                            a generic "running". */}
                        {dns.running && dns.mode === "in-process" ? (
                          <div
                            className="text-[0.6875rem]"
                            style={{ color: "var(--rex-warning-bright)" }}
                          >
                            Running inside the app — DNS stops when you quit rexenv (restart
                            the app to retry the always-on agent)
                          </div>
                        ) : (
                          <div className="text-[0.6875rem] text-rex-text-dim">
                            Resolves <span className="font-mono">*.{tld}</span> —{" "}
                            {dns.mode === "agent"
                              ? "always on, resolves even when rexenv is closed"
                              : "restarted automatically if it dies"}
                          </div>
                        )}
                      </div>
                    </div>
                    <div className="w-[62px] flex-none font-mono text-[0.71875rem] text-rex-text-dim">
                      :{dns.port}
                    </div>
                    {/* Down = red error, not gray "Idle": always-on means a dead
                        resolver is a fault (every local domain breaks), never a
                        normal stopped state. */}
                    <StatusPill
                      status={dns.running ? "running" : "error"}
                      label={dns.running ? undefined : "Down"}
                      className="min-w-[92px]"
                    />
                  </div>
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </>
  );
}
