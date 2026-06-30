import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, ExternalLink, Globe, Share2, Square } from "lucide-react";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { listSites, openExternal, startTunnel, stopTunnel, tunnelsStatus } from "@/lib/ipc";
import type { Site, TunnelInfo } from "@/types";

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="mb-[10px] px-0.5 font-mono text-[10px] uppercase tracking-[0.12em] text-rex-text-label">
      {children}
    </div>
  );
}

export function Tunnels() {
  const qc = useQueryClient();
  const { data: sites = [] } = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const { data: tunnels = [] } = useQuery({
    queryKey: ["tunnels"],
    queryFn: tunnelsStatus,
    refetchInterval: 5000,
  });
  const byDomain = new Map(tunnels.map((t) => [t.domain, t]));

  const share = useMutation({
    mutationFn: async ({ id, on }: { id: string; on: boolean }) => {
      if (on) await startTunnel(id);
      else await stopTunnel(id);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["tunnels"] }),
    onError: (e) => window.alert(String(e)),
  });

  const active = sites.filter((s) => byDomain.get(s.domain)?.running).length;

  const stopAll = useMutation({
    mutationFn: async () => {
      const ids = sites.filter((s) => byDomain.get(s.domain)?.running).map((s) => s.id);
      await Promise.all(ids.map((id) => stopTunnel(id)));
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["tunnels"] }),
    onError: (e) => window.alert(String(e)),
  });

  const stopAllBtn =
    active > 0 ? (
      <button
        onClick={() => stopAll.mutate()}
        disabled={stopAll.isPending}
        className="flex h-[34px] items-center gap-1.5 rounded-[9px] border border-status-error-border bg-status-error-bg px-3 text-[13px] font-medium text-status-error-bright transition-[filter] hover:brightness-110 disabled:opacity-60"
      >
        <Square className="h-3 w-3 fill-current" />
        Stop all sharing
      </button>
    ) : undefined;

  return (
    <>
      <TopBar
        title="Tunnels"
        subtitle={`${active} active`}
        showSearch={false}
        action={stopAllBtn}
      />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        {sites.length === 0 ? (
          <Placeholder
            icon={<Share2 className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="No sites to share"
            hint="Create a site, then share it publicly over a Cloudflare quick tunnel."
          />
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col">
            <p className="mb-[18px] text-[12.5px] text-rex-text-muted">
              Share a site publicly over a temporary Cloudflare tunnel. Each tunnel is scoped to that one
              site — your other sites and internal tools stay private. Start the services first so the tunnel
              has something to serve.
            </p>
            {(() => {
              const shared = sites.filter((s) => byDomain.get(s.domain)?.running);
              const shareable = sites.filter((s) => !byDomain.get(s.domain)?.running);
              const card = (s: Site) => (
                <TunnelCard
                  key={s.id}
                  site={s}
                  tunnel={byDomain.get(s.domain)}
                  busy={share.isPending && share.variables?.id === s.id}
                  onToggle={(on) => share.mutate({ id: s.id, on })}
                />
              );
              return (
                <>
                  {shared.length > 0 && (
                    <>
                      <SectionLabel>Shared now</SectionLabel>
                      <div className="mb-[22px] flex flex-col gap-3">{shared.map(card)}</div>
                    </>
                  )}
                  <SectionLabel>{shared.length ? "Shareable sites" : "All sites"}</SectionLabel>
                  <div className="flex flex-col gap-3">{shareable.map(card)}</div>
                </>
              );
            })()}
          </div>
        )}
      </div>
    </>
  );
}

type CardState = "idle" | "starting" | "live" | "stopping";

function TunnelCard({
  site,
  tunnel,
  busy,
  onToggle,
}: {
  site: Site;
  tunnel?: TunnelInfo;
  busy: boolean;
  onToggle: (on: boolean) => void;
}) {
  const on = !!tunnel?.running;
  const state: CardState = busy ? (on ? "stopping" : "starting") : on ? "live" : "idle";
  const border = {
    live: "border-status-running-border shadow-[0_0_0_1px_rgba(63,185,80,0.12)]",
    stopping: "border-status-running-border",
    starting: "border-status-warning-border",
    idle: "border-rex-border-subtle",
  }[state];

  return (
    <div className={cn("rounded-[13px] border bg-rex-surface-1 px-4 py-[14px] transition-colors", border)}>
      <div className="flex items-center gap-[13px]">
        <div className="flex h-[34px] w-[34px] flex-none items-center justify-center rounded-[9px] border border-rex-border bg-rex-surface-2 text-rex-text-muted">
          <Globe className="h-4 w-4" strokeWidth={1.7} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[14px] font-semibold text-rex-text">{site.name}</div>
          {on && tunnel ? (
            <div className="mt-0.5 flex items-center gap-1.5">
              <a
                className="truncate font-mono text-[11.5px] text-brand hover:underline"
                onClick={(e) => {
                  e.preventDefault();
                  openExternal(tunnel.url);
                }}
                href={tunnel.url}
                title={tunnel.url}
              >
                {tunnel.url.replace(/^https:\/\//, "")}
              </a>
              <CopyButton value={tunnel.url} />
              <button
                title="Open public URL"
                onClick={() => openExternal(tunnel.url)}
                className="rounded p-1 text-rex-text-muted transition-colors hover:bg-rex-surface-2 hover:text-rex-text"
              >
                <ExternalLink className="h-3.5 w-3.5" />
              </button>
            </div>
          ) : (
            <div className="mt-0.5 font-mono text-[11px] text-rex-text-muted">{site.domain}</div>
          )}
        </div>

        {state === "live" && <StatusPill status="running" label="Live" />}
        {state === "starting" && (
          <span className="flex items-center gap-[7px] text-[12px] text-status-warning-bright">
            <span className="h-3 w-3 rounded-full border-2 border-status-warning/30 border-t-status-warning animate-rex-spin motion-reduce:animate-none" />
            Starting…
          </span>
        )}
        {state === "stopping" && <span className="text-[12px] text-rex-text-muted">Stopping…</span>}

        <StartStopToggle
          running={on || busy}
          variant={!on && busy ? "setting" : "status"}
          onToggle={() => onToggle(!on)}
          label={`${on ? "Stop" : "Start"} sharing ${site.name}`}
        />
      </div>
    </div>
  );
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      title={copied ? "Copied" : "Copy URL"}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        } catch {
          /* clipboard unavailable */
        }
      }}
      className="rounded p-1 text-rex-text-muted transition-colors hover:bg-rex-surface-2 hover:text-rex-text"
    >
      {copied ? <Check className="h-3.5 w-3.5 text-brand" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  );
}
