import { useState } from "react";
import { toastBackendError } from "@/lib/toast";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Check, ChevronRight, Cloud, Copy, ExternalLink, Lightbulb, Share2, Square } from "lucide-react";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { siteTypeMeta } from "@/lib/siteType";
import { listSites, openExternal, startTunnel, stopTunnel, tunnelsStatus, wpDefaultCreds } from "@/lib/ipc";
import type { Site, TunnelInfo } from "@/types";

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="mb-[10px] px-0.5 font-mono text-[0.625rem] uppercase tracking-[0.12em] text-rex-text-label">
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

  // Default-credentials state per WP site, lifted HERE so the page can warn
  // proportionately: a loud per-card warning ONLY where it matters (a live
  // public URL), one quiet collapsible summary for everyone else. One cached
  // wp-cli check per site; any failure reads as "no warning".
  const wpSites = sites.filter((s) => s.type === "wordpress");
  const credQueries = useQueries({
    queries: wpSites.map((s) => ({
      queryKey: ["wp-default-creds", s.id],
      queryFn: () => wpDefaultCreds(s.id),
      staleTime: 5 * 60_000,
      refetchOnWindowFocus: false,
      retry: 0,
    })),
  });
  const defaultCredIds = new Set(
    wpSites.filter((_, i) => credQueries[i].data === true).map((s) => s.id),
  );

  const share = useMutation({
    mutationFn: async ({ id, on }: { id: string; on: boolean }) => {
      if (on) await startTunnel(id);
      else await stopTunnel(id);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["tunnels"] }),
    onError: (e) => toastBackendError(e),
  });

  const active = sites.filter((s) => byDomain.get(s.domain)?.running).length;

  const stopAll = useMutation({
    mutationFn: async () => {
      const ids = sites.filter((s) => byDomain.get(s.domain)?.running).map((s) => s.id);
      await Promise.all(ids.map((id) => stopTunnel(id)));
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["tunnels"] }),
    onError: (e) => toastBackendError(e),
  });

  const stopAllBtn =
    active > 0 ? (
      <button
        onClick={() => stopAll.mutate()}
        disabled={stopAll.isPending}
        className="flex h-[34px] items-center gap-1.5 rounded-[9px] border border-status-error-border bg-status-error-bg px-3 text-[0.8125rem] font-medium text-status-error-bright transition-[filter] hover:brightness-110 disabled:opacity-60"
      >
        <Square className="h-3 w-3 fill-current" />
        Stop all sharing
      </button>
    ) : undefined;

  return (
    <>
      <TopBar
        title="Tunnels"
        subtitle={
          active > 0
            ? `${active} ${active === 1 ? "site" : "sites"} shared publicly`
            : `${sites.length} ${sites.length === 1 ? "site" : "sites"} ready to share`
        }
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
            <div className="mb-[18px] flex items-start gap-[11px] rounded-xl border border-rex-border-subtle bg-rex-surface-1 px-[15px] py-[13px]">
              <Lightbulb
                className="mt-px h-[17px] w-[17px] flex-none"
                style={{ color: "var(--rex-lock-insecure)" }}
                strokeWidth={1.7}
              />
              <div className="flex-1 text-[0.8125rem] leading-[1.55] text-rex-text-bright">
                Sharing creates a <span className="font-medium text-rex-text">free, temporary public link</span>{" "}
                through Cloudflare — anyone with the URL reaches your local site, no deploy required. Links
                last while sharing is on and disappear when you stop.
              </div>
              <span className="flex-none whitespace-nowrap rounded-md border border-rex-border-subtle bg-rex-well px-[9px] py-1 font-mono text-[0.625rem] text-rex-text-dim">
                via cloudflared
              </span>
            </div>
            <DefaultCredsSummary
              sites={sites.filter(
                (s) => defaultCredIds.has(s.id) && !byDomain.get(s.domain)?.running,
              )}
            />
            {(() => {
              const shared = sites.filter((s) => byDomain.get(s.domain)?.running);
              const shareable = sites.filter((s) => !byDomain.get(s.domain)?.running);
              const card = (s: Site) => (
                <TunnelCard
                  key={s.id}
                  site={s}
                  tunnel={byDomain.get(s.domain)}
                  busy={share.isPending && share.variables?.id === s.id}
                  defaultCreds={defaultCredIds.has(s.id)}
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

/** One quiet, collapsible line for NOT-shared sites that still accept
 *  admin/admin — local-only means no real exposure, so this is informational,
 *  not a wall of per-row warnings. Live-shared sites get the loud per-card
 *  warning instead and are excluded here. */
function DefaultCredsSummary({ sites }: { sites: Site[] }) {
  const [open, setOpen] = useState(false);
  if (sites.length === 0) return null;
  const n = sites.length;
  return (
    <div className="mb-[18px] rounded-xl border border-rex-border-subtle bg-rex-surface-1">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-[9px] px-[15px] py-[11px] text-left"
        aria-expanded={open}
      >
        <ChevronRight
          className={cn("h-3.5 w-3.5 flex-none text-rex-text-muted transition-transform", open && "rotate-90")}
          strokeWidth={2}
        />
        <span className="flex-1 text-[0.78125rem] text-rex-text-muted">
          {n === 1 ? "1 site accepts" : `${n} sites accept`} the default{" "}
          <span className="font-mono text-rex-text-bright">admin/admin</span> login — fine locally;
          it only matters while a tunnel is active (you'll be warned on the card).
        </span>
      </button>
      {open && (
        <div className="border-t border-rex-border-subtle px-[15px] py-[10px]">
          <div className="flex flex-wrap gap-1.5">
            {sites.map((s) => (
              <span
                key={s.id}
                className="rounded-md border border-rex-border-subtle bg-rex-well px-2 py-1 font-mono text-[0.6875rem] text-rex-text-muted"
              >
                {s.domain}
              </span>
            ))}
          </div>
          <div className="mt-2 text-[0.71875rem] text-rex-text-dim">
            To change one: Site → WordPress → Users → key icon on the admin user.
          </div>
        </div>
      )}
    </div>
  );
}

type CardState = "idle" | "starting" | "live" | "stopping";

function TunnelCard({
  site,
  tunnel,
  busy,
  defaultCreds,
  onToggle,
}: {
  site: Site;
  tunnel?: TunnelInfo;
  busy: boolean;
  /** Site still accepts admin/admin (checked by the page, passed down). */
  defaultCreds: boolean;
  onToggle: (on: boolean) => void;
}) {
  const on = !!tunnel?.running;
  const state: CardState = busy ? (on ? "stopping" : "starting") : on ? "live" : "idle";
  const border = {
    live: "border-status-running-border shadow-[0_0_0_1px_var(--rex-running-bg)]",
    stopping: "border-status-running-border",
    starting: "border-status-warning-border",
    idle: "border-rex-border-subtle",
  }[state];

  const t = siteTypeMeta(site.type);
  return (
    <div className={cn("rounded-[13px] border bg-rex-surface-1 px-4 py-[14px] transition-colors", border)}>
      <div className="flex items-center gap-[13px]">
        <div
          className="flex h-[34px] w-[34px] flex-none items-center justify-center rounded-[9px] border text-[0.75rem] font-bold"
          style={{ background: t.bg, color: t.color, borderColor: t.border }}
        >
          {site.name.charAt(0).toUpperCase()}
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[0.875rem] font-semibold text-rex-text">{site.name}</div>
          <div className="mt-0.5 font-mono text-[0.6875rem] text-rex-text-muted">{site.domain}</div>
        </div>

        {state === "live" && <StatusPill status="running" label="Live" />}
        {state === "starting" && (
          <span className="flex items-center gap-[7px] text-[0.75rem] text-status-warning-bright">
            <span className="h-3 w-3 rounded-full border-2 border-status-warning/30 border-t-status-warning animate-rex-spin motion-reduce:animate-none" />
            Starting…
          </span>
        )}
        {state === "stopping" && <span className="text-[0.75rem] text-rex-text-muted">Stopping…</span>}
        {state === "idle" && <span className="text-[0.75rem] text-rex-text-muted">Share publicly</span>}

        <StartStopToggle
          running={on || busy}
          variant={!on && busy ? "setting" : "status"}
          onToggle={() => onToggle(!on)}
          label={`${on ? "Stop" : "Start"} sharing ${site.name}`}
        />
      </div>

      {/* Loud only where it MATTERS: a live public URL + default credentials.
          Idle sites are covered by the page-level summary instead. */}
      {on && defaultCreds && (
        <div className="mt-[11px] flex items-start gap-2 rounded-[9px] border border-status-warning-border bg-status-warning-bg px-3 py-2 text-[0.75rem] leading-[1.5] text-status-warning-bright">
          <AlertTriangle className="mt-px h-3.5 w-3.5 flex-none" strokeWidth={1.8} />
          <span>
            This site is PUBLIC and still accepts the default{" "}
            <span className="font-mono">admin/admin</span> login — anyone with the URL can enter
            wp-admin. Change the password (Site → WordPress → Users) or stop sharing.
          </span>
        </div>
      )}

      {on && tunnel && (
        <>
          <div className="mt-[13px] flex items-center gap-[9px] rounded-[10px] border border-rex-well-border bg-rex-well-deep py-[9px] pl-3 pr-[9px]">
            <Cloud
              className="h-[15px] w-[15px] flex-none"
              style={{ color: "var(--rex-lock-insecure)" }}
              strokeWidth={1.7}
            />
            <a
              className="min-w-0 flex-1 truncate font-mono text-[0.75rem] text-rex-link hover:underline"
              onClick={(e) => {
                e.preventDefault();
                void openExternal(tunnel.url).catch(toastBackendError);
              }}
              href={tunnel.url}
              title={tunnel.url}
            >
              {tunnel.url}
            </a>
            <CopyButton value={tunnel.url} />
            <button
              title="Open public URL"
              onClick={() => void openExternal(tunnel.url).catch(toastBackendError)}
              className="rounded p-1 text-rex-text-muted transition-colors hover:bg-rex-surface-2 hover:text-rex-text"
            >
              <ExternalLink className="h-3.5 w-3.5" />
            </button>
            <button
              onClick={() => onToggle(false)}
              className="flex h-7 items-center rounded-md border border-status-error-border bg-status-error-bg px-[11px] text-[0.71875rem] font-medium text-status-error-bright transition-[filter] hover:brightness-110"
            >
              Stop sharing
            </button>
          </div>
          {/* TODO(backend): request count + uptime aren't tracked on TunnelInfo yet. */}
          <div className="mt-2 flex items-center justify-between px-0.5 text-[0.6875rem] text-rex-text-dim">
            <span>Public link active</span>
            <span>Anyone with this link can reach your local site.</span>
          </div>
        </>
      )}
    </div>
  );
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        } catch {
          /* clipboard unavailable */
        }
      }}
      className={cn(
        "flex h-7 flex-none items-center gap-1.5 rounded-md px-2 text-[0.71875rem] transition-colors hover:bg-rex-hover-strong",
        copied ? "text-status-running-bright" : "text-rex-text-muted",
      )}
    >
      {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
      {copied ? "Copied" : "Copy"}
    </button>
  );
}
