import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, SlidersHorizontal } from "lucide-react";
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
 *  first: what Read is (free, whenever the endpoint is on), that Full includes
 *  publishing and that rexenv stops the share, that the administrator password
 *  still asks, and the residual — code an agent runs in your site runs as you. */
export function AgentAccessDial() {
  const qc = useQueryClient();
  // Polled like the asks: a 7-day setting expires while the card is open, and
  // the notice has to appear without a reload.
  const access = useQuery({ queryKey: ["agentAccess"], queryFn: agentAccess, refetchInterval: 4000 });
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
    // A level above Read keeps the duration already chosen, else the shortest —
    // and an EXPIRED 7-day setting is not "already chosen": picking a level
    // after an expiry starts at this session (the live run of 4 Sep 2026 saw
    // "changes (days)" written first, from the stale mode).
    else set.mutate({ level: next, mode: a?.expired ? "session" : (mode ?? "session") });
  };

  return (
    <div className="mt-3.5 border-t border-rex-border-subtle pt-3.5">
      <div className="flex items-center gap-1.5 text-[0.84375rem] font-medium text-rex-text">
        <SlidersHorizontal className="h-3.5 w-3.5 text-rex-text-muted" />
        {label}
      </div>
      <p className="mt-1 text-[0.75rem] leading-[1.55] text-rex-text-muted">
        How far an agent may go with the sites you made, and with rexenv itself — one setting for
        every site and every agent. <strong className="font-medium text-rex-text">Read is on whenever the endpoint is</strong>.
        At <strong className="font-medium text-rex-text">Full</strong> an agent can also publish a
        site to the internet — rexenv stops the share within the hour — and anything that needs an
        administrator password <strong className="font-medium text-rex-text">still asks you</strong>.
        Anything an agent runs inside one of your sites runs as you.
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
                // The chosen-one-of-N card the rest of the app uses (the New site
                // type cards): a brand border + glow and a filled check on the
                // chosen row, a subtle border elsewhere. Brand colours are the
                // `brand.*` Tailwind keys — `bg-rex-brand-*` does not exist, and
                // the first version of this dial shipped with exactly that dead
                // class, which is why nothing looked chosen (4 Sep 2026).
                "flex w-full items-start gap-3 rounded-[10px] border bg-rex-surface-1 px-3 py-2.5 text-left transition-colors",
                on ? "border-brand shadow-glow-primary" : "border-rex-border-subtle hover:border-rex-border-strong",
              )}
            >
              <span className="min-w-0 flex-1">
                <span className={cn("block text-[0.8125rem] font-semibold", on ? "text-brand-tint" : "text-rex-text")}>{LEVEL_LABEL[l.level]}</span>
                <span className="mt-0.5 block text-[0.71875rem] leading-[1.55] text-rex-text-muted">An agent can {l.allows}.</span>
              </span>
              <CheckCircle2
                className="mt-0.5 h-[19px] w-[19px] flex-none"
                style={{ color: on ? "var(--rex-brand)" : "var(--rex-border-strong)" }}
                strokeWidth={2}
                aria-hidden
              />
            </button>
          );
        })}
      </div>

      {level !== "read" && (
        <div className="mt-2.5 flex flex-wrap items-center gap-2.5">
          <span className="text-[0.71875rem] text-rex-text-muted">Duration</span>
          {/* The segmented pill the Sites filter uses: one track, the chosen pill tinted. */}
          <div className="flex rounded-[10px] border border-rex-well-border bg-rex-well p-[3px]" role="radiogroup" aria-label="Duration">
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
                    "rounded-[8px] px-2.5 py-1 text-[0.71875rem] transition-colors",
                    on ? "bg-brand-tint-bg font-medium text-brand-tint" : "text-rex-text-muted hover:text-rex-text-bright",
                  )}
                >
                  {m.label}
                </button>
              );
            })}
          </div>
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
