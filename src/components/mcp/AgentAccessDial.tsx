import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { SlidersHorizontal } from "lucide-react";
import { agentAccess, agentAccessSet } from "@/lib/ipc";
import type { AgentAccessLevel, AgentAccessMode } from "@/types";
import { toastBackendError } from "@/lib/toast";
import { cn } from "@/lib/utils";

/** The level names as the dial shows them. The SENTENCE under each comes from
 *  Rust (`AgentAccess.levels`), so there is one source for what a level hands
 *  over; only the one-word labels live here. */
const LEVEL_LABEL: Record<AgentAccessLevel, string> = { read: "Read", changes: "Changes", full: "Full" };

/** The three durations a level above Read can have (D15). */
const MODES: { mode: AgentAccessMode; label: string; what: string }[] = [
  { mode: "session", label: "This session", what: "switches itself off when you quit rexenv" },
  { mode: "days", label: "7 days", what: "expires on its own after 7 days" },
  { mode: "always", label: "Always", what: "stays until you change it here" },
];

/** The ONE dial for what an agent may do to the sites you made (D15 in
 *  `PLAN-mcp-parity.md`): a level and a duration, global, replacing the
 *  per-site prompts that needed six clicks for one site's ordinary work.
 *
 *  Held by the copy guard in `mcp_server.rs` to the sentences a trim would cut
 *  first: what Read is (free, whenever the endpoint is on), that publishing a
 *  site ALWAYS asks, that the administrator password still asks, and the
 *  residual — code an agent runs in your site runs as you. */
export function AgentAccessDial() {
  const qc = useQueryClient();
  const access = useQuery({ queryKey: ["agentAccess"], queryFn: agentAccess });
  const set = useMutation({
    mutationFn: (v: { level: AgentAccessLevel; mode: AgentAccessMode | null }) => agentAccessSet(v.level, v.mode),
    onSuccess: (a) => {
      qc.setQueryData(["agentAccess"], a);
      void qc.invalidateQueries({ queryKey: ["mcp-status"] });
    },
    onError: (e) => toastBackendError(e),
  });

  const a = access.data;
  const level: AgentAccessLevel = a?.level ?? "read";
  const mode: AgentAccessMode | null = a?.mode ?? null;
  const levels = a?.levels ?? [];
  const label = a?.label ?? "Agent access";

  const choose = (next: AgentAccessLevel) => {
    if (next === "read") set.mutate({ level: "read", mode: null });
    // A level above Read keeps the duration already chosen, else the shortest.
    else set.mutate({ level: next, mode: mode ?? "session" });
  };

  return (
    <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
      <div className="flex items-center gap-1.5 text-[0.84375rem] font-medium text-rex-text">
        <SlidersHorizontal className="h-3.5 w-3.5 text-rex-text-muted" />
        {label}
      </div>
      <p className="mt-1 text-[0.75rem] leading-[1.55] text-rex-text-muted">
        How far an agent may go with the sites you made yourself, and with rexenv itself. One
        setting for every site and every agent. <strong className="font-medium text-rex-text">Read is on whenever the endpoint is</strong>;
        the two above it are your choice, for as long as you say. Whatever the level,{" "}
        <strong className="font-medium text-rex-text">publishing a site to the internet always asks you</strong>{" "}
        (Site access below), and anything that needs an administrator password{" "}
        <strong className="font-medium text-rex-text">still asks you</strong>. Anything an agent runs inside one of your sites runs as
        you, with your files and your permissions.
      </p>

      <div className="mt-2.5 space-y-1.5" role="radiogroup" aria-label={label}>
        {levels.map((l) => {
          const on = l.level === level;
          return (
            <button
              key={l.level}
              type="button"
              role="radio"
              aria-checked={on}
              disabled={set.isPending}
              onClick={() => choose(l.level)}
              className={cn(
                "flex w-full items-start gap-3 rounded-md border px-3 py-2 text-left transition-colors",
                on ? "border-rex-brand-tint-border bg-rex-brand-active" : "border-rex-border-subtle hover:border-rex-border",
              )}
            >
              {/* A ring, not a fill: no background token, so the WCAG scan has no
                  pairing to compute on a dot that carries no text. */}
              <span
                className={cn(
                  "mt-[3px] h-3 w-3 flex-none rounded-full border",
                  on ? "border-[4px] border-rex-brand" : "border-rex-border",
                )}
              />
              <span className="min-w-0 flex-1">
                <span className="block text-[0.78125rem] font-medium text-rex-text">{LEVEL_LABEL[l.level]}</span>
                <span className="mt-0.5 block text-[0.71875rem] leading-[1.55] text-rex-text-muted">An agent can {l.allows}.</span>
              </span>
            </button>
          );
        })}
      </div>

      {level !== "read" && (
        <div className="mt-2.5 flex flex-wrap items-center gap-2">
          <span className="text-[0.71875rem] text-rex-text-muted">For:</span>
          {MODES.map((m) => {
            const on = m.mode === mode;
            return (
              <button
                key={m.mode}
                type="button"
                role="radio"
                aria-checked={on}
                disabled={set.isPending}
                title={`This ${m.what}.`}
                onClick={() => set.mutate({ level, mode: m.mode })}
                className={cn(
                  "rounded-md border px-2.5 py-1 text-[0.71875rem] transition-colors",
                  on ? "border-rex-brand-tint-border bg-rex-brand-active text-rex-text" : "border-rex-border-subtle text-rex-text-muted hover:text-rex-text",
                )}
              >
                {m.label}
              </button>
            );
          })}
          <span className="basis-full text-[0.6875rem] leading-[1.55] text-rex-text-muted">
            {mode === "session" && "This switches itself off when you quit rexenv."}
            {mode === "days" && a?.expiresAt && `Expires on its own at ${a.expiresAt} UTC.`}
            {mode === "always" && "Stays until you change it here."}
          </span>
        </div>
      )}

      {a?.expired && (
        <p className="mt-2 text-[0.6875rem] leading-[1.55] text-rex-text-muted">
          <strong className="font-medium text-rex-text">Your 7-day setting expired</strong>{a.expiresAt ? ` at ${a.expiresAt} UTC` : ""} — agents are back at Read.
        </p>
      )}
    </div>
  );
}
