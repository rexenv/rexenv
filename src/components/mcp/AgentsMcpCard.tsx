import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, ChevronRight, Copy } from "lucide-react";
import { mcpStatus, mcpSetEnabled, agentActivityClear, agentAccessSet } from "@/lib/ipc";
import type { ActivityStatus } from "@/types";
import { toast, toastBackendError } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { cn } from "@/lib/utils";
import { usePlatformWords } from "@/lib/usePlatformWords";
import { AgentActivityFeed } from "./AgentActivityFeed";
import { AgentAccessDial } from "./AgentAccessDial";

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
 * endpoint (docs/archive/PLAN-mcp-server.md §6). The toggle is a REAL control over the
 * socket (enabling binds, disabling drops sessions + unlinks). The residual
 * (§3.1) reads at the moment of enabling: it sits above the toggle, verbatim and
 * un-collapsed, worded to stay true as more capable tools arrive.
 *
 * D16/D17 (4 Sep 2026): with the endpoint OFF nothing renders below the toggle —
 * there is nothing to decide about a socket that is not there. With it ON, the
 * card is three things: connect an agent, how far it may go (the Agent access
 * dial), and what it did. Every per-call prompt is gone — the mail sub-toggle
 * and the database grant because both were reads, and the publish prompt (D17)
 * because Full already says an agent may run code of its choosing as the user;
 * asking twice for a thing the level describes was the complexity to remove.
 * The paragraph above the toggle carries their honest sentences, copy-guarded.
 *
 * The status line reads from recent call OUTCOMES, never the handshake alone —
 * so it says "working" only while calls succeed, and self-recovers as an error
 * state ages out of the window (the card polls; the backend does the windowing).
 */
export function AgentsMcpCard() {
  const qc = useQueryClient();
  const words = usePlatformWords();
  const { data } = useQuery({
    queryKey: ["mcp-status"],
    queryFn: mcpStatus,
    refetchInterval: 4000, // live feed + self-recovering status line
  });

  const setEnabled = useMutation({
    mutationFn: (on: boolean) => mcpSetEnabled(on),
    onSuccess: (s) => {
      qc.setQueryData(["mcp-status"], s);
      void qc.invalidateQueries({ queryKey: ["agentAccess"] });
      toast.success(s.enabled ? "MCP endpoint enabled" : "MCP endpoint turned off");
    },
    onError: (e) => toastBackendError(e),
  });

  // Folded by default: the feed is a LOG, read when something is being looked
  // into, and open it pushed the toggle and the dial off a 900px window.
  const [showActivity, setShowActivity] = useState(false);

  // The ONLY reason a level above Read is reachable while the endpoint is off:
  // it is the user's setting, it survives the socket, and it applies the moment
  // the toggle goes on. The DIAL does not render there any more — three level
  // cards and a duration row are a decision about a socket that is not open —
  // but the standing sentence keeps its way down, or the setting would be
  // un-lowerable without first turning the endpoint back on.
  const toRead = useMutation({
    mutationFn: () => agentAccessSet("read", null),
    onSuccess: (a) => {
      qc.setQueryData(["agentAccess"], a);
      void qc.invalidateQueries({ queryKey: ["mcp-status"] });
      toast.success("Agent access is back at Read");
    },
    onError: (e) => toastBackendError(e),
  });

  const clear = useMutation({
    mutationFn: agentActivityClear,
    onSuccess: () => qc.invalidateQueries({ queryKey: ["mcp-status"] }),
    onError: (e) => toastBackendError(e),
  });

  const enabled = data?.enabled ?? false;
  const access = data?.access;
  // A level above Read survives the endpoint being off (it is the user's
  // setting), so it must be SAID while off — the dial that would show it is
  // not rendered, and turning the endpoint on would otherwise re-activate it
  // silently (the review's find).
  const standing =
    !enabled && access && access.level !== "read"
      ? `Agent access is set to ${access.level === "full" ? "Full" : "Changes"}${
          access.mode === "always" ? " · Always" : access.mode === "days" ? " · 7 days" : ""
        } and applies as soon as you turn this on.`
      : null;
  const status = data ? statusLine(data.activity) : null;
  const rows = data?.recent ?? [];
  const connectCommand = data?.connectCommand ?? "claude mcp add rexenv -- rex mcp";
  const connectCommandUser = data?.connectCommandUser ?? "claude mcp add --scope user rexenv -- rex mcp";

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

      {/* The residual — verbatim, above the toggle, not behind an expander.
          Every sentence here is one the copy guard holds, because this is now
          the ONE paragraph a person reads before handing an agent the inbox
          and the databases (D16). */}
      <p className="mt-3 text-[0.75rem] leading-[1.55] text-rex-text-muted">
        Before you turn this on: this lets an AI agent connect to rexenv and use its tools. With
        the endpoint on, an agent can look at your sites — status, logs, users and content,{" "}
        <strong className="font-medium text-rex-text">every site's mail</strong> (password-reset
        links included) and their databases,{" "}
        <strong className="font-medium text-rex-text">read-only</strong> (password hashes and API
        keys are in there) — and it can create disposable &ldquo;scratch&rdquo; sites of its own,
        put code into them and run it. It cannot change or delete the sites you made yourself
        unless you turn Agent access up below, for this session, 7 days or always: that refusal
        lives in rexenv, not in the agent's good behaviour. Code running in a scratch site — or
        in one of your sites once you allow changes — runs as you, with your files and your
        permissions. rexenv never asks for your administrator password on an agent's behalf;
        at Full an agent can also publish a site to the internet, and rexenv stops any share it
        starts within the hour. Every call an agent makes is listed below. Turn this off when
        you're not using it.
      </p>

      <div className="mt-3.5 flex items-center gap-[14px] border-t border-rex-border-subtle pt-3.5">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Enable the MCP endpoint</div>
          <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">
            Off by default. Enabling opens rexenv's private socket for `rex mcp`; turning it off
            closes it and disconnects any agent.
            {standing && (
              <span className="mt-1 block text-rex-text">
                {standing}{" "}
                <button
                  type="button"
                  disabled={toRead.isPending}
                  onClick={() => toRead.mutate()}
                  className="font-medium text-brand-light underline-offset-2 transition-opacity hover:underline disabled:opacity-50"
                >
                  Set it back to Read
                </button>
              </span>
            )}
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

      {enabled && (
        <>
          {/* Connect — the next thing a person does, so it comes first. */}
          <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
            <div className="text-[0.78125rem] font-medium text-rex-text">Connect an agent</div>
            {/* TWO commands, because Claude Code's default is per-project and
                that is the thing people are surprised by: rexenv set up in one
                repo, missing in the next. Neither is chosen for them — a
                shared repo wants the project entry, one machine wants the
                user one — so both are here with the difference stated. */}
            {[
              { label: "In this project", command: connectCommand, what: "Claude Code's default: this folder only." },
              { label: "In every project", command: connectCommandUser, what: "Written once for you, on this machine." },
            ].map((row) => (
              <div key={row.label} className="mt-2">
                <div className="flex items-baseline justify-between gap-2">
                  <span className="text-[0.71875rem] font-medium text-rex-text">{row.label}</span>
                  <span className="text-[0.6875rem] text-rex-text-muted">{row.what}</span>
                </div>
                <div className="mt-1 flex items-center gap-2 rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-1.5">
                  <code className="min-w-0 flex-1 truncate font-mono text-[0.6875rem] text-rex-text">{row.command}</code>
                  <CopyButton value={row.command} />
                </div>
              </div>
            ))}
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
              <p className="mt-1.5 text-[0.6875rem] leading-[1.55] text-rex-text-muted">
                Put it in <span className="font-mono">.cursor/mcp.json</span> for one project, or{" "}
                <span className="font-mono">{words.homePrefix}/.cursor/mcp.json</span> for every project. If{" "}
                <span className="font-mono">rex</span> isn't found, install it from Settings →
                General → Command-line tool, then reconnect.
              </p>
            </details>
          </div>

          <AgentAccessDial />

          {/* Activity — every agent action, none silent, but FOLDED. Fifty
              rows of a live feed pushed the two controls that matter (the
              toggle and the dial) off the screen, and a log nobody scrolled to
              is not read more often for being open. Folded is not hidden: the
              header states the COUNT, so the feed's existence and its size are
              on screen whether or not the rows are — which is what "every call
              an agent makes is listed below" promises. */}
          <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
            <div className="flex items-center gap-2">
              <button
                type="button"
                aria-expanded={showActivity}
                onClick={() => setShowActivity((v) => !v)}
                className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-[0.78125rem] font-medium text-rex-text transition-colors hover:text-rex-text-bright"
              >
                <ChevronRight
                  className={cn("h-3.5 w-3.5 flex-none text-rex-text-muted transition-transform", showActivity && "rotate-90")}
                />
                Recent activity
                <span className="font-normal text-rex-text-muted">
                  {rows.length === 0
                    ? "· nothing yet"
                    : `· ${rows.length} call${rows.length === 1 ? "" : "s"}`}
                </span>
              </button>
              {showActivity && rows.length > 0 && (
                <Button size="sm" variant="ghost" disabled={clear.isPending} onClick={() => clear.mutate()}>
                  Clear
                </Button>
              )}
            </div>
            {showActivity && (
              <div className="mt-1">
                <AgentActivityFeed rows={rows} empty="No agent activity yet." />
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
