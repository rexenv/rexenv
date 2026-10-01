/** AI agents over MCP: the endpoint off by default, the Agent access dial at
 *  Read, and an agent's calls landing in the activity feed — a scratch site of
 *  its own, a plugin activated inside it, a read of one of your sites, and a
 *  refused change to one of your sites (the dial's sentence, `core/agent_access.rs`).
 *  `window.__scene.agentWorks()` plays the calls; the scratch site joins the
 *  Sites list as origin "agent" with a 24 h inactivity TTL. */
import type { SceneCtx } from "../demo-backend";
import type { AgentAccess, AgentAccessLevel, AgentAccessMode, AgentAction } from "@/types";

type Args = Record<string, unknown> | undefined;

/** The backend's stamps are SQLite UTC ("YYYY-MM-DD HH:MM:SS"), not ISO — the
 *  feed's timeAgo() reads that shape and prints anything else verbatim. */
const stamp = (ms: number) => new Date(ms).toISOString().slice(0, 19).replace("T", " ");

const ALLOWS: Record<AgentAccessLevel, string> = {
  read: "look at any of your sites — status, content, users, logs, every site's mail (password-reset links included) and their databases, read-only (password hashes and API keys are in there) — and create disposable sites of its own; it cannot change anything you made",
  changes: "change how any of your sites is served and what is installed in it — PHP version, web server, Xdebug, plugins and themes on or off, options, restarts, dry runs, blueprints — and start or stop rexenv's stack (macOS still asks for your password)",
  full: "do everything Changes allows, and also delete or reset a site and its database, run a live search-replace or a database import, publish a site to the internet for up to an hour (anyone with the link reaches it until rexenv stops the share), and run commands and code of its choosing in any of your sites, as you",
};
const LABEL: Record<AgentAccessLevel, string> = { read: "Read", changes: "Changes", full: "Full" };
const MODE: Record<AgentAccessMode, string> = { session: "This session", days: "7 days", always: "Always" };

export default function mcpAgent(ctx: SceneCtx) {
  let enabled = false;
  let level: AgentAccessLevel = "read";
  let mode: AgentAccessMode | null = null;
  const recent: AgentAction[] = [];
  let nextId = 1;

  const access = (): AgentAccess => ({
    level,
    mode,
    expiresAt: mode === "days" ? new Date(Date.now() + 7 * 864e5).toISOString() : null,
    expired: false,
    label: level === "read" ? "Read" : `${LABEL[level]} · ${MODE[mode ?? "session"]}`,
    allows: ALLOWS[level],
    levels: (["read", "changes", "full"] as AgentAccessLevel[]).map((l) => ({ level: l, allows: ALLOWS[l] })),
  });
  const status = () => ({
    enabled,
    connectCommand: "claude mcp add rexenv -- rex mcp",
    connectCommandUser: "claude mcp add --scope user rexenv -- rex mcp",
    activity: !enabled ? { kind: "off" } : recent.length ? { kind: "working", lastTool: recent[0].tool, minutesAgo: 0 } : { kind: "idle" },
    recent,
    access: access(),
  });
  const call = (tool: string, target: string | null, outcome: AgentAction["outcome"], argsSummary: string | null = null, detail: string | null = null) => {
    const site = target ? ctx.sites.find((s) => s.domain === target) : null;
    recent.unshift({
      id: nextId++,
      at: stamp(Date.now()),
      actor: "agent",
      client: "Claude Code",
      tool,
      targetSite: site?.id ?? null,
      targetLabel: target,
      outcome,
      detail,
      argsSummary,
      concerning: outcome === "denied",
    });
  };

  (window as unknown as { __scene: object }).__scene = {
    async agentWorks() {
      call("list_sites", null, "ok");
      await ctx.sleep(900);
      ctx.sites.push(
        ctx.site({
          id: "20",
          name: "plugin-test",
          domain: "plugin-test.scratch.rex",
          type: "wordpress",
          origin: "agent",
          agentClient: "Claude Code",
          expiresAt: stamp(Date.now() + 23.5 * 36e5),
          createdAt: "2026-09-30 11:50:00",
        }),
      );
      call("scratch_create_site", "plugin-test.scratch.rex", "ok");
      await ctx.sleep(900);
      call("scratch_add_package", "plugin-test.scratch.rex", "ok", "plugin");
      await ctx.sleep(800);
      call("wp_run", "plugin-test.scratch.rex", "ok", "plugin activate");
      await ctx.sleep(800);
      call("site_logs", "agency-blog.rex", "ok");
      await ctx.sleep(900);
      call(
        "wp_plugin",
        "agency-blog.rex",
        "denied",
        "update",
        "wp_plugin needs `changes` permission — `Agent access` at Changes or above in rexenv — and it is at Read.",
      );
    },
  };

  return {
    mcp_status: () => status(),
    mcp_set_enabled: async (a: Args) => {
      await ctx.sleep(500);
      enabled = Boolean(a?.enable);
      return status();
    },
    agent_access_get: () => access(),
    agent_access_set: async (a: Args) => {
      await ctx.sleep(300);
      level = a?.level as AgentAccessLevel;
      mode = (a?.mode as AgentAccessMode | null) ?? null;
      return access();
    },
    agent_activity: (a: Args) => (a?.siteId ? recent.filter((r) => r.targetSite === a.siteId) : recent),
    agent_activity_clear: () => {
      const n = recent.length;
      recent.length = 0;
      return n;
    },
    keep_site: () => true,
    scratch_packages: () => [],
  };
}
