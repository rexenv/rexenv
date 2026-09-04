import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, KeyRound } from "lucide-react";
import { agentSiteRequests, agentSiteGrants, agentSiteGrant, agentSiteDeny, agentSiteRevoke } from "@/lib/ipc";
import type { AgentSiteAsk, AgentSiteGrantRow } from "@/types";
import { toastBackendError } from "@/lib/toast";
import { Button } from "@/components/ui/button";

/** How a grant stands right now, from the RECORDED columns — the same reading
 *  the database prompt did before D16, for the same reason: the backend stores an expiry date
 *  because the button promised one, and a second clock here would be the one
 *  that drifts. */
function state(g: AgentSiteGrantRow): { label: string; live: boolean } {
  if (g.revokedAt) return { label: "Revoked", live: false };
  const expires = new Date(`${g.expiresAt.replace(" ", "T")}Z`);
  if (Number.isNaN(expires.getTime())) return { label: "Unknown", live: false };
  if (expires.getTime() <= Date.now()) return { label: "Expired", live: false };
  if (g.session) return { label: "Until you quit rexenv", live: true };
  const days = Math.ceil((expires.getTime() - Date.now()) / 86_400_000);
  return { label: `Expires in ${days} day${days === 1 ? "" : "s"}`, live: true };
}

/** The one consent that stays a click after D15: publishing a site to the
 *  internet. The Agent access dial above answers everything else; a share is
 *  outward-facing, so it keeps a person's yes — asked for here when an agent
 *  tries, for this session only, listed and revocable.
 *
 *  The shape the database prompt had before D16, for the same reasons (a prompt with no
 *  history is a decision a user cannot revisit). The wording says the one thing
 *  no setting changes: anything the agent runs in your site runs as you. Held
 *  to that by the copy guard in `mcp_server.rs`. */
export function AgentSiteGrants() {
  const qc = useQueryClient();
  const requests = useQuery({ queryKey: ["agentSiteRequests"], queryFn: agentSiteRequests, refetchInterval: 4000 });
  const grants = useQuery({ queryKey: ["agentSiteGrants"], queryFn: agentSiteGrants });

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["agentSiteRequests"] });
    void qc.invalidateQueries({ queryKey: ["agentSiteGrants"] });
  };
  const approve = useMutation({
    // Publishing is session-only by design: a standing week-long "yes" to
    // putting a site on the internet is not a thing this card offers.
    mutationFn: (ask: AgentSiteAsk) => agentSiteGrant(ask.siteId, ask.client, ask.scope, true),
    onSuccess: refresh,
    onError: (e) => toastBackendError(e),
  });
  const deny = useMutation({
    mutationFn: (a: AgentSiteAsk) => agentSiteDeny(a.siteId, a.client, a.scope),
    onSuccess: refresh,
    onError: (e) => toastBackendError(e),
  });
  const revoke = useMutation({
    mutationFn: (id: string) => agentSiteRevoke(id),
    onSuccess: refresh,
    onError: (e) => toastBackendError(e),
  });

  const asks = requests.data ?? [];
  const rows = grants.data ?? [];
  const live = rows.filter((g) => state(g).live);
  const past = rows.filter((g) => !state(g).live);

  // The recorded SCOPE, never a verb this component invented: rows predating
  // D15/D16 hold `read`/`manage`/`destroy`/`system`, and labelling every one
  // "publish" told the user something the record does not say.
  const row = (g: AgentSiteGrantRow, canRevoke: boolean) => {
    const s = state(g);
    const where = g.siteId === null ? "rexenv itself" : (g.siteLabel ?? "(deleted site)");
    return (
      <li key={g.id} className="flex items-center gap-2 rounded-md border border-rex-border-subtle px-2.5 py-1.5">
        <div className="min-w-0 flex-1">
          <div className="truncate text-[0.75rem] text-rex-text">
            {g.client} · <em>{g.scope}</em>{g.scope === "run" ? " (publish)" : ""} ·{" "}
            <span className="font-mono text-[0.6875rem]">{where}</span>
          </div>
          <div className="text-[0.6875rem] text-rex-text-muted">{s.label}</div>
        </div>
        {canRevoke && s.live && (
          <Button size="sm" variant="ghost" disabled={revoke.isPending} onClick={() => revoke.mutate(g.id)}>
            Revoke
          </Button>
        )}
      </li>
    );
  };

  return (
    <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
      <div className="flex items-center gap-1.5 text-[0.78125rem] font-medium text-rex-text">
        <KeyRound className="h-3.5 w-3.5 text-rex-text-muted" />
        Site access
      </div>

      {asks.map((a) => (
        <div
          key={`${a.siteId ?? "stack"}:${a.client}:${a.scope}`}
          className="mt-2 rounded-md border border-status-warning/40 bg-status-warning/5 px-3 py-2.5"
        >
          {/* Concrete and unflattering — the DB prompt's rule. */}
          <div className="text-[0.8125rem] leading-[1.5] text-rex-text">
            Allow <strong className="font-medium">{a.client}</strong> to publish{" "}
            <span className="font-mono text-[0.75rem]">{a.domain ?? "a site"}</span> to the internet?
          </div>
          <div className="mt-1 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
            It asked to: <span className="text-rex-text">{a.wanted}</span>
          </div>
          <div className="mt-1 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
            Anyone with the link reaches the site while the share is up; rexenv stops it on its own
            after the minutes asked for, or when you quit. Anything the agent runs inside the site
            runs as you. This access lasts only for this session, and you can revoke it here at any time.
          </div>
          <div className="mt-2 flex flex-wrap gap-2">
            <Button size="sm" disabled={approve.isPending} onClick={() => approve.mutate(a)}>
              Allow for this session
            </Button>
            <Button size="sm" variant="ghost" disabled={deny.isPending} onClick={() => deny.mutate(a)}>
              Don't allow
            </Button>
          </div>
        </div>
      ))}

      {live.length === 0 && asks.length === 0 && (
        <p className="mt-1.5 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
          Publishing a site is the one thing an agent always has to ask you for, whatever Agent
          access is set to. The ask appears here when it tries, and{" "}
          <strong className="font-medium text-rex-text">a request only lasts while rexenv is running</strong>.
        </p>
      )}

      {live.length > 0 && <ul className="mt-2 space-y-1">{live.map((g) => row(g, true))}</ul>}

      {/* Answered, expired and revoked decisions are EVIDENCE, not a to-do
          list: kept, because "what could that agent do, and until when" is the
          question this section exists to answer — folded away, because a
          column of "Revoked" is the whole section otherwise. */}
      {past.length > 0 && (
        <details className="group mt-2">
          <summary className="flex cursor-pointer list-none items-center gap-1 text-[0.6875rem] text-rex-text-muted hover:text-rex-text">
            <ChevronRight className="h-3 w-3 transition-transform group-open:rotate-90" />
            Earlier decisions ({past.length})
          </summary>
          <ul className="mt-1.5 space-y-1">{past.map((g) => row(g, false))}</ul>
        </details>
      )}
    </div>
  );
}
