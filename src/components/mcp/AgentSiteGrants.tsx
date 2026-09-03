import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRound } from "lucide-react";
import {
  agentSiteRequests,
  agentSiteGrants,
  agentSiteGrant,
  agentSiteDeny,
  agentSiteRevoke,
  agentSiteAutoAllow,
  agentSiteSetAutoAllow,
} from "@/lib/ipc";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import type { AgentScope, AgentSiteAsk, AgentSiteGrantRow, AutoAllowableScope } from "@/types";
import { toastBackendError } from "@/lib/toast";
import { Button } from "@/components/ui/button";

/** How a grant stands right now, from the RECORDED columns — the same reading
 *  `AgentDbGrants` does, for the same reason: the backend stores an expiry date
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

/** The verb the prompt leads with, per scope — the question a person actually
 *  answers. The sentence UNDER it (what the scope allows) comes from Rust with
 *  the ask, so it has one source. */
const VERB: Record<AgentScope, string> = {
  read: "read",
  manage: "manage",
  destroy: "delete or reset",
  run: "run its own code in",
  system: "change",
};

/** The three auto-allowable scopes, in blast-radius order, with the switch copy.
 *  `destroy` and `system` are absent by TYPE on both sides of the bridge — there
 *  is no row to add for them, which is the point. */
const AUTO_ROWS: { scope: AutoAllowableScope; title: string; what: string }[] = [
  { scope: "read", title: "Allow reads without asking", what: "read any of your sites — content, users, logs, mail" },
  { scope: "manage", title: "Allow changes without asking", what: "change how any of your sites is served and what is installed in it" },
  { scope: "run", title: "Allow running code without asking", what: "run commands and code of its choosing in any of your sites, as you" },
];

/** The consent surface for the parity tools — the ones that act on the sites
 *  the USER made, and on rexenv itself: the asks waiting for an answer, the
 *  session's auto-allow switches, and every grant ever given.
 *
 *  The same shape as `AgentDbGrants` and for the same reasons (a prompt with no
 *  history is a decision a user cannot revisit), generalised to a scope. The
 *  wording is deliberately concrete about what each scope hands over, and it
 *  says the one thing no grant changes: code the agent runs in a granted site
 *  runs as you. Held to that by the copy guard in `mcp_server.rs`. */
export function AgentSiteGrants() {
  const qc = useQueryClient();
  const requests = useQuery({ queryKey: ["agentSiteRequests"], queryFn: agentSiteRequests, refetchInterval: 4000 });
  const grants = useQuery({ queryKey: ["agentSiteGrants"], queryFn: agentSiteGrants });

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["agentSiteRequests"] });
    void qc.invalidateQueries({ queryKey: ["agentSiteGrants"] });
  };
  const approve = useMutation({
    mutationFn: (v: { ask: AgentSiteAsk; session: boolean }) =>
      agentSiteGrant(v.ask.siteId, v.ask.client, v.ask.scope, v.session),
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
  const autoAllow = useQuery({ queryKey: ["agentSiteAutoAllow"], queryFn: agentSiteAutoAllow });
  const setAuto = useMutation({
    mutationFn: (v: { scope: AutoAllowableScope; on: boolean }) => agentSiteSetAutoAllow(v.scope, v.on),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["agentSiteAutoAllow"] }),
    onError: (e) => toastBackendError(e),
  });

  const asks = requests.data ?? [];
  const rows = grants.data ?? [];
  const auto = new Set(autoAllow.data ?? []);

  return (
    <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
      <div className="flex items-center gap-1.5 text-[0.78125rem] font-medium text-rex-text">
        <KeyRound className="h-3.5 w-3.5 text-rex-text-muted" />
        Site access
      </div>

      {asks.map((a) => {
        const target = a.domain ? (
          <span className="font-mono text-[0.75rem]">{a.domain}</span>
        ) : (
          <span>rexenv itself</span>
        );
        // D9: `destroy` is never a week-long standing permission — the only
        // "yes" it offers is for this session.
        const weekOffered = a.scope !== "destroy";
        return (
          <div
            key={`${a.siteId ?? "stack"}:${a.client}:${a.scope}`}
            className="mt-2 rounded-md border border-status-warning/40 bg-status-warning/5 px-3 py-2.5"
          >
            {/* Concrete, per scope, and unflattering — the DB prompt's rule. */}
            <div className="text-[0.8125rem] leading-[1.5] text-rex-text">
              Allow <strong className="font-medium">{a.client}</strong> to {VERB[a.scope]} {target}?
            </div>
            <div className="mt-1 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
              It asked to: <span className="text-rex-text">{a.wanted}</span>
            </div>
            <div className="mt-1 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
              With <em>{a.scope}</em> it can {a.allows}. Anything it runs inside the site runs as
              you, with your files and your permissions. This access{" "}
              {weekOffered ? "expires in 7 days, or with this session if you choose that" : "lasts only for this session"}
              , and you can revoke it here at any time.
            </div>
            <div className="mt-2 flex flex-wrap gap-2">
              {weekOffered && (
                <Button size="sm" disabled={approve.isPending} onClick={() => approve.mutate({ ask: a, session: false })}>
                  Allow for 7 days
                </Button>
              )}
              <Button
                size="sm"
                variant={weekOffered ? "secondary" : "primary"}
                disabled={approve.isPending}
                onClick={() => approve.mutate({ ask: a, session: true })}
              >
                Allow for this session
              </Button>
              <Button size="sm" variant="ghost" disabled={deny.isPending} onClick={() => deny.mutate(a)}>
                Don't allow
              </Button>
            </div>
          </div>
        );
      })}

      {rows.length === 0 && asks.length === 0 && (
        <p className="mt-1.5 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
          No agent can change the sites you made, or rexenv itself. Scratch sites an agent creates
          are its own and need no permission; your own sites need one, asked for here when an agent
          tries — one site, one kind of access, one agent at a time.{" "}
          <strong className="font-medium text-rex-text">
            A request only lasts while rexenv is running
          </strong>{" "}
          — if an agent asked before you last quit, nothing is waiting here now and you'll need to
          ask it to try again.
        </p>
      )}

      {/* Auto-allow, per scope. The two scopes that lose work or change the
          machine have NO switch — not hidden, absent: the type has no variant
          for them. Each switch's copy names what stops being asked and that it
          dies with the session, the DB switch's two load-bearing sentences. */}
      <div className="mt-3 space-y-2">
        {AUTO_ROWS.map((r) => {
          const on = auto.has(r.scope);
          return (
            <div key={r.scope} className="flex items-start gap-[14px] rounded-md border border-rex-border-subtle px-3 py-2.5">
              <div className="min-w-0 flex-1">
                <div className="text-[0.78125rem] font-medium text-rex-text">{r.title}</div>
                <div className="mt-1 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
                  With this on, an agent that asks to {r.what} gets a yes immediately — you are{" "}
                  <strong className="font-medium text-rex-text">not asked</strong>, for any site.{" "}
                  <strong className="font-medium text-rex-text">
                    This switches itself off when you quit rexenv
                  </strong>
                  ; every grant it makes is listed below, marked <em>auto</em>, and can be revoked.
                </div>
              </div>
              <StartStopToggle
                running={on}
                busy={setAuto.isPending}
                variant="setting"
                onToggle={() => setAuto.mutate({ scope: r.scope, on: !on })}
                label={r.title}
              />
            </div>
          );
        })}
        <p className="text-[0.6875rem] leading-[1.55] text-rex-text-muted">
          Deleting a site and changing rexenv itself can never be allowed without asking — there is
          no switch for them.
        </p>
      </div>

      {rows.length > 0 && (
        <ul className="mt-2 space-y-1">
          {rows.map((g) => {
            const s = state(g);
            const where = g.siteId === null ? "rexenv itself" : (g.siteLabel ?? "(deleted site)");
            return (
              <li key={g.id} className="flex items-center gap-2 rounded-md border border-rex-border-subtle px-2.5 py-1.5">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[0.75rem] text-rex-text">
                    {g.client} · <em>{g.scope}</em> · <span className="font-mono text-[0.6875rem]">{where}</span>
                    {g.autoGranted && (
                      <span
                        className="ml-1.5 rounded border border-rex-border-subtle px-1 py-px text-[0.625rem] uppercase tracking-wide text-rex-text-muted"
                        title="Granted by auto-allow — you were not asked about this one"
                      >
                        auto
                      </span>
                    )}
                  </div>
                  <div className="text-[0.6875rem] text-rex-text-muted">{s.label}</div>
                </div>
                {s.live && (
                  <Button size="sm" variant="ghost" disabled={revoke.isPending} onClick={() => revoke.mutate(g.id)}>
                    Revoke
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
