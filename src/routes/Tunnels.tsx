import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, ExternalLink, Globe, Share2, Square } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { listSites, openExternal, startTunnel, stopTunnel, tunnelsStatus } from "@/lib/ipc";
import type { Site, TunnelInfo } from "@/types";

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
          <div className="mx-auto flex max-w-3xl flex-col gap-3">
            <p className="text-[12.5px] text-rex-text-muted">
              Share a site publicly over a temporary Cloudflare tunnel. Each tunnel is scoped to that one
              site — your other sites and internal tools stay private. Start the services first so the tunnel
              has something to serve.
            </p>
            <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
              {sites.map((s) => (
                <TunnelRow
                  key={s.id}
                  site={s}
                  tunnel={byDomain.get(s.domain)}
                  busy={share.isPending && share.variables?.id === s.id}
                  onToggle={(on) => share.mutate({ id: s.id, on })}
                />
              ))}
            </div>
          </div>
        )}
      </div>
    </>
  );
}

function TunnelRow({
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
  return (
    <div className="flex items-center gap-4 border-b border-rex-border-subtle px-4 py-3 last:border-b-0">
      <div className="flex h-7 w-7 flex-none items-center justify-center rounded-md border border-rex-border bg-rex-surface-2 text-rex-text-muted">
        <Globe className="h-4 w-4" strokeWidth={1.7} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-semibold text-rex-text">{site.name}</div>
        {on && tunnel ? (
          <div className="flex items-center gap-1.5">
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
          <div className="font-mono text-[11px] text-rex-text-dim">{site.domain}</div>
        )}
      </div>

      {on && (
        <span className="flex items-center gap-1.5 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] font-medium text-emerald-400">
          <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
          Public
        </span>
      )}
      {busy && <span className="text-[12px] text-rex-text-muted">{on ? "Stopping…" : "Starting…"}</span>}

      <button
        role="switch"
        aria-checked={on}
        aria-label="Share publicly"
        disabled={busy}
        onClick={() => onToggle(!on)}
        className={`relative h-[22px] w-[40px] flex-none rounded-full transition-colors disabled:opacity-50 ${
          on ? "bg-brand" : "bg-rex-surface-3"
        }`}
      >
        <span
          className={`absolute top-[2px] h-[18px] w-[18px] rounded-full bg-white transition-all ${
            on ? "left-[20px]" : "left-[2px]"
          }`}
        />
      </button>
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
