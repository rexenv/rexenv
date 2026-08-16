import { useQuery } from "@tanstack/react-query";
import { agentActivity } from "@/lib/ipc";
import { AgentActivityFeed } from "./AgentActivityFeed";

/**
 * The per-site slice of the agent activity feed, for SiteDetail's Overview. It
 * renders ONLY when an agent has actually touched this site (`null` otherwise) —
 * the section appears when there's something to show, never as an empty panel on
 * a site no agent has looked at.
 */
export function SiteAgentActivity({ siteId }: { siteId: string }) {
  const { data: rows = [] } = useQuery({
    queryKey: ["agent-activity", siteId],
    queryFn: () => agentActivity(siteId, 20),
    refetchInterval: 5000,
  });
  if (rows.length === 0) return null;
  return (
    <div className="rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
      <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
        Agent activity
      </div>
      <AgentActivityFeed rows={rows} empty="" hideTarget />
    </div>
  );
}
