import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Database } from "lucide-react";
import { agentDbRequests, agentDbGrants, agentDbGrant, agentDbDeny, agentDbRevoke } from "@/lib/ipc";
import type { AgentDbGrant as Grant } from "@/types";
import { toastBackendError } from "@/lib/toast";
import { Button } from "@/components/ui/button";

/** How a grant stands right now, from the RECORDED columns — never from a
 *  duration recomputed here. The backend stores an expiry date because the
 *  dialog promised one; deriving "expired" any other way in the UI would put a
 *  second clock beside the one that decides. */
function state(g: Grant): { label: string; tone: string; live: boolean } {
  if (g.revokedAt) return { label: "Revoked", tone: "text-rex-text-muted", live: false };
  // The stored timestamps are SQLite's `datetime('now')` — UTC, space-separated.
  // Parsed as UTC explicitly: read as local time, a grant would appear to expire
  // hours early or late depending on where the user is.
  const expires = new Date(`${g.expiresAt.replace(" ", "T")}Z`);
  if (Number.isNaN(expires.getTime())) return { label: "Unknown", tone: "text-rex-text-muted", live: false };
  if (expires.getTime() <= Date.now()) return { label: "Expired", tone: "text-rex-text-muted", live: false };
  const days = Math.ceil((expires.getTime() - Date.now()) / 86_400_000);
  return {
    label: `Expires in ${days} day${days === 1 ? "" : "s"}`,
    tone: "text-rex-text-muted",
    live: true,
  };
}

/** The consent surface for `db_query` on a REAL site: the asks waiting for an
 *  answer, and every grant ever given.
 *
 *  Both halves are here on purpose. A prompt with no history is a decision a
 *  user cannot revisit, and a history with no prompt is a permission they can
 *  only ever remove. Expired and revoked rows stay listed because the question
 *  this screen answers after the fact is "what could that agent see, and until
 *  when" — which a deleted row cannot answer. */
export function AgentDbGrants() {
  const qc = useQueryClient();
  const requests = useQuery({ queryKey: ["agentDbRequests"], queryFn: agentDbRequests, refetchInterval: 4000 });
  const grants = useQuery({ queryKey: ["agentDbGrants"], queryFn: agentDbGrants });

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["agentDbRequests"] });
    void qc.invalidateQueries({ queryKey: ["agentDbGrants"] });
  };
  const approve = useMutation({
    mutationFn: (v: { siteId: string; client: string }) => agentDbGrant(v.siteId, v.client),
    onSuccess: refresh,
    onError: (e) => toastBackendError(e),
  });
  const deny = useMutation({
    mutationFn: (v: { siteId: string; client: string }) => agentDbDeny(v.siteId, v.client),
    onSuccess: refresh,
    onError: (e) => toastBackendError(e),
  });
  const revoke = useMutation({
    mutationFn: (id: string) => agentDbRevoke(id),
    onSuccess: refresh,
    onError: (e) => toastBackendError(e),
  });

  const asks = requests.data ?? [];
  const rows = grants.data ?? [];

  return (
    <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
      <div className="flex items-center gap-1.5 text-[0.78125rem] font-medium text-rex-text">
        <Database className="h-3.5 w-3.5 text-rex-text-muted" />
        Database access
      </div>

      {asks.map((a) => (
        <div
          key={`${a.siteId}:${a.client}`}
          className="mt-2 rounded-md border border-status-warning/40 bg-status-warning/5 px-3 py-2.5"
        >
          {/* The wording is the plan's, and it is deliberately unflattering
              about what is being handed over. A consent prompt that undersells
              the access is worse than no prompt, because it produces a decision
              the user believes they understood. */}
          <div className="text-[0.8125rem] leading-[1.5] text-rex-text">
            Allow <strong className="font-medium">{a.client}</strong> to read the database of{" "}
            <span className="font-mono text-[0.75rem]">{a.domain}</span>?
          </div>
          <div className="mt-1 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
            The agent will be able to read everything in it — including user password hashes and
            any API keys or tokens stored in <span className="font-mono">wp_options</span>. It
            cannot modify or delete anything. This access expires in 7 days, and you can revoke it
            here at any time.
          </div>
          <div className="mt-2 flex gap-2">
            <Button
              size="sm"
              disabled={approve.isPending}
              onClick={() => approve.mutate({ siteId: a.siteId, client: a.client })}
            >
              Allow for 7 days
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={deny.isPending}
              onClick={() => deny.mutate({ siteId: a.siteId, client: a.client })}
            >
              Don't allow
            </Button>
          </div>
        </div>
      ))}

      {rows.length === 0 && asks.length === 0 && (
        <p className="mt-1.5 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
          No agent can read your sites' databases. Scratch sites an agent creates are its own and
          need no permission; your own sites need one, asked for here when an agent tries.
        </p>
      )}

      {rows.length > 0 && (
        <ul className="mt-2 space-y-1">
          {rows.map((g) => {
            const s = state(g);
            return (
              <li
                key={g.id}
                className="flex items-center gap-2 rounded-md border border-rex-border-subtle px-2.5 py-1.5"
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[0.75rem] text-rex-text">
                    {g.client} · <span className="font-mono text-[0.6875rem]">{g.dbUser}</span>
                  </div>
                  <div className={`text-[0.6875rem] ${s.tone}`}>{s.label}</div>
                </div>
                {s.live && (
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={revoke.isPending}
                    onClick={() => revoke.mutate(g.id)}
                  >
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
