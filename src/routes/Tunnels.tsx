import { useMemo, useState } from "react";
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
    <div className="mb-[10px] px-0.5 font-mono text-[0.625rem] uppercase tracking-[0.12em] text-rex-text-muted">
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

  // Search over name, domain AND the public URL. The URL is here because this
  // page is where someone arrives holding a link — "which of my sites is
  // https://odd-cat-42.trycloudflare.com?" is the question the tunnel list is
  // uniquely able to answer, and pasting it is how a person asks it.
  const [query, setQuery] = useState("");
  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (s: Site) => {
      if (!q) return true;
      const url = byDomain.get(s.domain)?.url ?? "";
      return (
        s.name.toLowerCase().includes(q) ||
        s.domain.toLowerCase().includes(q) ||
        url.toLowerCase().includes(q)
      );
    };
    // `byDomain` is rebuilt from `tunnels` on every render, so the dependency
    // that matters is the data behind it.
  }, [query, tunnels]);

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
        searchPlaceholder="Filter sites or paste a link…"
        searchValue={query}
        onSearchChange={setQuery}
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
                <p>
                  Sharing creates a <span className="font-medium text-rex-text">free, temporary public link</span>{" "}
                  through Cloudflare — anyone with the URL reaches your local site, no deploy required. Links
                  last while sharing is on and disappear when you stop or quit rexenv. If rexenv crashes, a
                  link can stay live until rexenv next opens — it's shut down automatically then.
                </p>
                <p className="mt-1.5 text-rex-text-muted">
                  Everything the site's folder serves becomes public while shared: stray dev scripts (a{" "}
                  <span className="font-mono">phpinfo.php</span>, a test file) are runnable through the
                  link, and symlinks inside the project are followed — including ones pointing outside
                  it. Dotfiles (<span className="font-mono">.git</span>,{" "}
                  <span className="font-mono">.env</span>) stay blocked.
                </p>
              </div>
              <span className="flex-none whitespace-nowrap rounded-md border border-rex-border-subtle bg-rex-well px-[9px] py-1 font-mono text-[0.625rem] text-rex-text-muted">
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
              const sharedShown = shared.filter(matches);
              const shareableShown = shareable.filter(matches);
              // A live public URL is not a view preference. The filter may hide
              // a card, and then it has to SAY so — a page where typing three
              // letters makes an exposed site disappear silently is the one
              // place in this app a search box could actually cost something.
              const hiddenShared = shared.length - sharedShown.length;
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
              if (sharedShown.length === 0 && shareableShown.length === 0) {
                return (
                  <div
                    data-probe="tunnels-no-match"
                    className="flex flex-col items-center justify-center gap-1.5 px-5 py-[54px] text-center"
                  >
                    <div className="text-[0.875rem] font-medium text-rex-text-bright">
                      No sites match “{query.trim()}”
                    </div>
                    <div className="text-[0.78125rem] text-rex-text-muted">
                      Search by site name, domain, or a public link.
                    </div>
                    {hiddenShared > 0 && <HiddenSharedNote count={hiddenShared} />}
                  </div>
                );
              }
              return (
                <>
                  {sharedShown.length > 0 && (
                    <>
                      <SectionLabel>Shared now</SectionLabel>
                      <div className="mb-[22px] flex flex-col gap-3">{sharedShown.map(card)}</div>
                    </>
                  )}
                  {hiddenShared > 0 && (
                    <div className="mb-[22px]">
                      <HiddenSharedNote count={hiddenShared} />
                    </div>
                  )}
                  {shareableShown.length > 0 && (
                    <>
                      <SectionLabel>
                        {shared.length ? "Shareable sites" : "All sites"}
                      </SectionLabel>
                      <div className="flex flex-col gap-3">{shareableShown.map(card)}</div>
                    </>
                  )}
                </>
              );
            })()}
          </div>
        )}
      </div>
    </>
  );
}

/** What the filter is hiding, when what it hides is a LIVE PUBLIC URL.
 *
 *  Every other list in rexenv may quietly shrink under a search box: the sites
 *  are still there, and nothing about them changed. Here a hidden row is a site
 *  the whole internet can reach right now, and "I filtered and the shared
 *  section went empty" must never be readable as "nothing is shared". Stop all
 *  sharing stays global for the same reason — it is the machine's state, not
 *  the view's. */
function HiddenSharedNote({ count }: { count: number }) {
  return (
    <div
      data-probe="tunnels-hidden-shared"
      data-count={count}
      className="flex items-center gap-[9px] rounded-lg border border-status-warning-border bg-status-warning-bg px-[13px] py-[9px] text-[0.78125rem] text-rex-text-bright"
    >
      <AlertTriangle className="h-[15px] w-[15px] flex-none text-status-warning-bright" strokeWidth={1.8} />
      <span>
        {count === 1 ? "1 shared site is" : `${count} shared sites are`} hidden by this filter —
        still public until you stop sharing.
      </span>
    </div>
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
          <div className="mt-2 text-[0.71875rem] text-rex-text-muted">
            To change one: Site → WordPress → Users → key icon on the admin user.
          </div>
        </div>
      )}
    </div>
  );
}

/** Diagnosis-driven line under an Unverified badge. Wording stays SCOPED —
 *  never anything shaped like "reachable everywhere" — and every state points
 *  at the second device, the only true end-to-end test. */
const DIAGNOSIS_LINE: Record<import("@/types").TunnelDiagnosis, string> = {
  "local-dns-behind":
    "Working — reachable at Cloudflare's edge from here; this machine's DNS hasn't caught up yet. Try it from another device.",
  "dns-propagating":
    "Just created — DNS is still propagating (usually seconds). Give it a moment before opening: asking too early can make this network remember the miss for a while.",
  "edge-gone":
    "Cloudflare's edge reports this link is no longer registered — if that persists it will be marked Broken. Stop and share again for a fresh link.",
  offline: "Can't check right now — this machine looks offline.",
};

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
  const health = tunnel?.health ?? "unverified";
  // Override sites are shareable since 15 Aug 2026: the tunnel originates
  // from the site's OWN recorded backend port (core::tunnels::origin_port),
  // so the old "can't be shared yet" wall — and its courtesy mirror here —
  // are gone. A stopped backend is refused by the backend with a message
  // naming the fix; the UI does not pre-guess liveness.
  const state: CardState = busy ? (on ? "stopping" : "starting") : on ? "live" : "idle";
  // The live border reads the SAME fact as the badge — the tunnel's health.
  const liveBorder = {
    reachable: "border-status-running-border shadow-[0_0_0_1px_var(--rex-running-bg)]",
    unverified: "border-status-warning-border",
    broken: "border-status-error-border",
  }[health];
  const border = {
    live: liveBorder,
    stopping: "border-status-running-border",
    starting: "border-status-warning-border",
    idle: "border-rex-border-subtle",
  }[state];

  const t = siteTypeMeta(site.type);
  return (
    <div
      data-probe="tunnel-card"
      data-domain={site.domain}
      data-live={on ? "1" : "0"}
      className={cn("rounded-[13px] border bg-rex-surface-1 px-4 py-[14px] transition-colors", border)}
    >
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

        {state === "live" && health === "reachable" && <StatusPill status="running" label="Live" />}
        {state === "live" && health === "unverified" && (
          <StatusPill status="starting" label="Unverified" />
        )}
        {state === "live" && health === "broken" && <StatusPill status="error" label="Broken" />}
        {state === "starting" && (
          <span className="flex items-center gap-[7px] text-[0.75rem] text-status-warning-bright">
            <span className="h-3 w-3 rounded-full border-2 border-status-warning/30 border-t-status-warning animate-rex-spin motion-reduce:animate-none" />
            Starting…
          </span>
        )}
        {state === "stopping" && <span className="text-[0.75rem] text-rex-text-muted">Stopping…</span>}
        {state === "idle" && (
          <span className="text-[0.75rem] text-rex-text-muted">Share publicly</span>
        )}

        {/* busy DISABLES the control: while "Starting…" the toggle looks
            like a stop, but cancel-during-start isn't supported — a click
            here used to read as cancel while actually requesting a second
            start (the backend claim now refuses it; the UI must not offer
            it). A control that does the opposite of what it looks like is
            worse than a disabled one. */}
        <StartStopToggle
          running={on || busy}
          busy={busy}
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
          {health === "broken" && (
            <div className="mt-[11px] flex items-start gap-2 rounded-[9px] border border-status-error-border bg-status-error-bg px-3 py-2 text-[0.75rem] leading-[1.5] text-status-error-bright">
              <AlertTriangle className="mt-px h-3.5 w-3.5 flex-none" strokeWidth={1.8} />
              <span>
                Cloudflare reports this tunnel is no longer registered — the link won't work.
                Stop sharing, then share again for a fresh link.
              </span>
            </div>
          )}
          {/* TODO(backend): request count + uptime aren't tracked on TunnelInfo yet. */}
          <div className="mt-2 flex items-center justify-between px-0.5 text-[0.6875rem] text-rex-text-muted">
            <span>
              {health === "reachable" && "Public link confirmed reachable — checked every 30 s."}
              {health === "unverified" &&
                ((tunnel?.diagnosis && DIAGNOSIS_LINE[tunnel.diagnosis]) ||
                  "Public link can't be reached from this machine right now — fresh links can take a while to resolve here (local DNS caching) while already working elsewhere. Try it from another device; if it stays unreachable everywhere, stop and share again.")}
              {health === "broken" && "Public link is down."}
            </span>
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
