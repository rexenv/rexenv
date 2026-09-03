import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, ChevronRight, Copy } from "lucide-react";
import { mcpStatus, mcpSetEnabled, mcpSetMailEnabled, mcpSetSitesEnabled, agentActivityClear } from "@/lib/ipc";
import type { ActivityStatus } from "@/types";
import { toast, toastBackendError } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { cn } from "@/lib/utils";
import { AgentActivityFeed } from "./AgentActivityFeed";
import { AgentDbGrants } from "./AgentDbGrants";
import { AgentSiteGrants } from "./AgentSiteGrants";

/** The connect stanza for editors that read an MCP JSON config (Cursor, VS Code
 *  Copilot). `rex mcp` is the same dumb pipe `claude mcp add` uses. */
const CLIENT_JSON = `{
  "mcpServers": {
    "rexenv": { "command": "rex", "args": ["mcp"] }
  }
}`;

function statusLine(a: ActivityStatus): { dot: string; tone: string; text: string } | null {
  switch (a.kind) {
    case "off":
      return null;
    case "idle":
      return { dot: "bg-rex-text-dim", tone: "text-rex-text-muted", text: "On — no recent agent activity" };
    case "working":
      return {
        dot: "bg-status-running",
        tone: "text-status-running-bright",
        text: `Working — ${a.lastTool}${a.minutesAgo > 0 ? ` · ${a.minutesAgo}m ago` : " · just now"}`,
      };
    case "erroring":
      return {
        dot: "bg-status-warning",
        tone: "text-status-warning-bright",
        text: `On — the last ${a.errored} agent call${a.errored === 1 ? "" : "s"} errored`,
      };
  }
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      title={copied ? "Copied" : "Copy"}
      className="flex-none rounded-md p-1.5 text-rex-text-muted transition-colors hover:bg-rex-hover hover:text-rex-text"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        } catch {
          /* clipboard unavailable */
        }
      }}
    >
      {copied ? <Check className="h-3.5 w-3.5 text-brand" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  );
}

/**
 * Settings → "AI agents (MCP)" — the developer-facing surface for the opt-in MCP
 * endpoint (docs/PLAN-mcp-server.md §6). The toggle is a REAL control over the
 * socket (enabling binds, disabling drops sessions + unlinks). The residual
 * (§3.1) reads at the moment of enabling: it sits above the toggle, verbatim and
 * un-collapsed, worded to stay true as more capable tools arrive.
 *
 * The status line reads from recent call OUTCOMES, never the handshake alone —
 * so it says "working" only while calls succeed, and self-recovers as an error
 * state ages out of the window (the card polls; the backend does the windowing).
 */
export function AgentsMcpCard() {
  const qc = useQueryClient();
  const { data } = useQuery({
    queryKey: ["mcp-status"],
    queryFn: mcpStatus,
    refetchInterval: 4000, // live feed + self-recovering status line
  });

  const setMailEnabled = useMutation({
    mutationFn: (on: boolean) => mcpSetMailEnabled(on),
    onSuccess: (s) => {
      qc.setQueryData(["mcp-status"], s);
      toast.success(
        s.mailEnabled
          ? "Agents can read scratch-site mail"
          : "Agents can no longer read any mail",
      );
    },
    onError: (e) => toastBackendError(e),
  });

  const setSitesEnabled = useMutation({
    mutationFn: (on: boolean) => mcpSetSitesEnabled(on),
    onSuccess: (s) => {
      qc.setQueryData(["mcp-status"], s);
      toast.success(
        s.sitesEnabled
          ? "Agents can now ask for access to your own sites"
          : "Agents can no longer touch your own sites",
      );
    },
    onError: (e) => toastBackendError(e),
  });

  const setEnabled = useMutation({
    mutationFn: (on: boolean) => mcpSetEnabled(on),
    onSuccess: (s) => {
      qc.setQueryData(["mcp-status"], s);
      toast.success(s.enabled ? "MCP endpoint enabled" : "MCP endpoint turned off");
    },
    onError: (e) => toastBackendError(e),
  });

  const clear = useMutation({
    mutationFn: agentActivityClear,
    onSuccess: () => qc.invalidateQueries({ queryKey: ["mcp-status"] }),
    onError: (e) => toastBackendError(e),
  });

  const enabled = data?.enabled ?? false;
  const mailEnabled = data?.mailEnabled ?? false;
  const sitesEnabled = data?.sitesEnabled ?? false;
  // The label comes from the backend's ONE constant — the refusal an agent
  // reads names the same string, so the two cannot drift apart (#404).
  const sitesToggleLabel = data?.sitesToggleLabel ?? "Let agents manage my own sites";
  const status = data ? statusLine(data.activity) : null;
  const rows = data?.recent ?? [];
  const connectCommand = data?.connectCommand ?? "claude mcp add rexenv -- rex mcp";

  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 p-5">
      <div className="flex items-start justify-between gap-4">
        <div className="text-[0.875rem] font-semibold text-rex-text">AI agents (MCP)</div>
        {status && (
          <div className={cn("flex items-center gap-1.5 text-[0.71875rem]", status.tone)}>
            <span className={cn("h-1.5 w-1.5 flex-none rounded-full", status.dot)} />
            {status.text}
          </div>
        )}
      </div>

      {/* The residual — verbatim, above the toggle, not behind an expander. */}
      <p className="mt-3 text-[0.75rem] leading-[1.55] text-rex-text-muted">
        Before you turn this on: this lets an AI agent connect to rexenv and use the tools you've
        enabled. It can look at your sites — their status and their logs — and it can create
        disposable &ldquo;scratch&rdquo; sites of its own, put code into them and run it. It cannot
        change or delete the sites you made yourself unless you allow that below,
        one site and one kind of change at a time, for a limited time: that refusal lives in
        rexenv, not in the agent's good behaviour. It can ask to read one of your sites' databases,
        and only you can say yes — each site separately, expiring on its own, revocable here. But code running in a
        scratch site — or in a site you granted — runs as you, with your files and
        your permissions — the same power over this machine as code you run yourself. rexenv never
        asks for your administrator password on an agent's behalf, and every call an agent makes is
        listed below. Turn this off when you're not using it.
      </p>

      <div className="mt-3.5 flex items-center gap-[14px] border-t border-rex-border-subtle pt-3.5">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Enable the MCP endpoint</div>
          <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">
            Off by default. Enabling opens rexenv's private socket for `rex mcp`; turning it off
            closes it and disconnects any agent.
          </div>
        </div>
        <StartStopToggle
          running={enabled}
          busy={setEnabled.isPending}
          variant="setting"
          onToggle={() => setEnabled.mutate(!enabled)}
          label="Enable the MCP endpoint"
        />
      </div>

      {/* Mail sub-toggle (M2b, D4) — the SECOND place a user consents to
          something, and off by default independently of the endpoint. The
          middle paragraph is the honest core: it explains the mechanism and
          states the failure direction in the same breath, which is what makes
          "fail-closed" mean something to someone who has never met the term.
          Held to that by the copy guard's must-say list. */}
      <div className="mt-3.5 flex items-start gap-[14px] border-t border-rex-border-subtle pt-3.5">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">
            Let agents read scratch-site mail
          </div>
          <div className="mt-1 space-y-1.5 text-[0.75rem] leading-[1.55] text-rex-text-muted">
            <p>
              rexenv catches mail from every site in one inbox — yours and the agent's together.
              With this on, an agent can read only the messages that came{" "}
              <strong className="font-medium text-rex-text">from a scratch site it created</strong>;
              your own sites' mail is never returned by those tools, and that includes password-reset
              links. Reading your whole inbox is a separate permission an agent has to ask for in
              Site access below — <em>read</em> on rexenv itself — which needs this switch on too and
              which you can refuse.
            </p>
            <p>
              The way rexenv tells them apart is a small plugin it installs into each scratch site,
              which stamps that site's own address on outgoing mail. If a site's code overrides that
              stamp, its mail simply stops being visible to the agent — so the failure is that the
              agent misses its own mail, never that it sees yours.
            </p>
            <p>
              Off by default. Switch it back off at any time and the agent stops reading mail
              entirely.
            </p>
          </div>
        </div>
        <StartStopToggle
          running={mailEnabled}
          busy={setMailEnabled.isPending}
          variant="setting"
          onToggle={() => setMailEnabled.mutate(!mailEnabled)}
          label="Let agents read scratch-site mail"
        />
      </div>

      {/* Sites sub-toggle (MCP parity, PLAN-mcp-parity §3.3) — the THIRD place a
          user consents to something. Off by default, independent of the other
          two, and on its own it grants NOTHING: it makes per-site scope grants
          possible, each a separate consent below. The residual is restated
          here because this is the widest surface yet. Held by the copy guard. */}
      <div className="mt-3.5 flex items-start gap-[14px] border-t border-rex-border-subtle pt-3.5">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">{sitesToggleLabel}</div>
          <div className="mt-1 space-y-1.5 text-[0.75rem] leading-[1.55] text-rex-text-muted">
            <p>
              With this on, an agent can ask for specific permissions on the sites you made
              yourself — and on rexenv itself — and you decide each one, per site and per kind of
              access, in <strong className="font-medium text-rex-text">Site access</strong> below.
              On its own this switch grants nothing.
            </p>
            <p>
              A permission bounds which site and which kind of change, not what code runs:
              anything an agent runs inside a site you granted runs as you. Deleting a site can
              only ever be allowed for one session at a time, and anything that needs an
              administrator password still asks you.
            </p>
            <p>
              Off by default. Switch it back off at any time and every agent stops being able to
              touch your own sites at once; the permissions you gave stay listed and resume if you
              switch it on again.
            </p>
          </div>
        </div>
        <StartStopToggle
          running={sitesEnabled}
          busy={setSitesEnabled.isPending}
          variant="setting"
          onToggle={() => setSitesEnabled.mutate(!sitesEnabled)}
          label={sitesToggleLabel}
        />
      </div>

      {/* Connect — copy-paste for the agent's client config (zero new install). */}
      <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
        <div className="text-[0.78125rem] font-medium text-rex-text">Connect an agent</div>
        <div className="mt-2 flex items-center gap-2 rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-1.5">
          <code className="min-w-0 flex-1 truncate font-mono text-[0.6875rem] text-rex-text">
            {connectCommand}
          </code>
          <CopyButton value={connectCommand} />
        </div>
        <details className="group mt-2">
          <summary className="flex cursor-pointer list-none items-center gap-1 text-[0.71875rem] text-rex-text-muted hover:text-rex-text">
            <ChevronRight className="h-3.5 w-3.5 transition-transform group-open:rotate-90" />
            Other clients (Cursor, VS Code)
          </summary>
          <div className="mt-2 flex items-start gap-2 rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-2">
            <pre className="min-w-0 flex-1 overflow-x-auto font-mono text-[0.6875rem] leading-relaxed text-rex-text">
              {CLIENT_JSON}
            </pre>
            <CopyButton value={CLIENT_JSON} />
          </div>
          <p className="mt-1.5 text-[0.6875rem] text-rex-text-muted">
            If <span className="font-mono">rex</span> isn't found, install it from Settings →
            General → Command-line tool, then reconnect.
          </p>
        </details>
      </div>

      <AgentDbGrants />
      <AgentSiteGrants />

      {/* Activity — every agent action, none silent. */}
      <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
        <div className="flex items-center justify-between">
          <div className="text-[0.78125rem] font-medium text-rex-text">Recent activity</div>
          {rows.length > 0 && (
            <Button size="sm" variant="ghost" disabled={clear.isPending} onClick={() => clear.mutate()}>
              Clear
            </Button>
          )}
        </div>
        <div className="mt-1">
          <AgentActivityFeed rows={rows} empty="No agent activity yet." />
        </div>
      </div>
    </div>
  );
}
