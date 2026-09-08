import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, Eye, Pencil, SlidersHorizontal, Zap } from "lucide-react";
import { agentAccess, agentAccessSet } from "@/lib/ipc";
import type { AgentAccessLevel, AgentAccessMode } from "@/types";
import { toastBackendError } from "@/lib/toast";
import { cn } from "@/lib/utils";

/** The level names as the dial shows them. The SENTENCE under each comes from
 *  Rust (`AgentAccess.levels`), so there is one source for what a level hands
 *  over; only the one-word labels live here. */
const LEVEL_LABEL: Record<AgentAccessLevel, string> = { read: "Read", changes: "Changes", full: "Full" };

/** One glyph per level, so the LADDER is visible before any sentence is read:
 *  an eye that only looks, a pencil that changes what is there, a bolt that
 *  also reaches the internet. Icons order the three; they never replace the
 *  sentence, which is the thing a person consents to. */
const LEVEL_ICON: Record<AgentAccessLevel, typeof Eye> = { read: Eye, changes: Pencil, full: Zap };

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
      {/* The heading carries the ANSWER, not just the question. "Agent access"
          alone made a reader walk three cards to find which one was lit; the
          chip states the standing setting — level and how long it lasts — in
          the place the eye lands first. It is derived from the same status the
          cards render, never typed twice. */}
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <div className="flex items-center gap-1.5 text-[0.84375rem] font-medium text-rex-text">
          <SlidersHorizontal className="h-3.5 w-3.5 text-rex-text-muted" />
          {label}
        </div>
        <span
          className={cn(
            "ml-auto flex items-center gap-1.5 whitespace-nowrap rounded-full border px-2 py-0.5 text-[0.6875rem] font-medium",
            level === "read"
              ? "border-rex-border-strong bg-rex-well text-rex-text-muted"
              : "border-brand/40 bg-brand/10 text-brand",
          )}
        >
          {(() => {
            const Icon = LEVEL_ICON[level];
            return <Icon className="h-3 w-3" strokeWidth={2} />;
          })()}
          {LEVEL_LABEL[level]}
          {level !== "read" && mode && ` · ${MODES.find((m) => m.mode === mode)?.label ?? ""}`}
        </span>
      </div>
      <p className="mt-1 text-[0.75rem] leading-[1.55] text-rex-text-muted">
        How far an agent may go with the sites you made, and with rexenv itself — one setting for
        every site and every agent. <strong className="font-medium text-rex-text">Read is on whenever the endpoint is</strong>.
        At <strong className="font-medium text-rex-text">Full</strong> an agent can also publish a
        site to the internet — rexenv stops the share within the hour — and anything that needs an
        administrator password <strong className="font-medium text-rex-text">still asks you</strong>.
        Anything an agent runs inside one of your sites runs as you.
      </p>

      <div className="mt-2.5 space-y-1" role="radiogroup" aria-label={label}>
        {levels.map((l) => {
          const on = l.level === level;
          const Icon = LEVEL_ICON[l.level];
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
                "flex w-full items-center gap-2.5 rounded-[10px] border bg-rex-surface-1 px-2.5 py-2 text-left transition-colors",
                on ? "border-brand shadow-glow-primary" : "border-rex-border-subtle hover:border-rex-border-strong",
              )}
            >
              <Icon
                className={cn("h-[15px] w-[15px] flex-none", on ? "text-brand" : "text-rex-text-muted")}
                strokeWidth={1.8}
                aria-hidden
              />
              {/* Name and sentence on ONE line at the width this card has: the
                  three sentences are what the user consents to, so they are not
                  truncated — they wrap under the name, without the second block
                  of padding the two-line version spent on nothing. */}
              <span className="min-w-0 flex-1">
                <span className={cn("text-[0.8125rem] font-semibold", on ? "text-brand-tint" : "text-rex-text")}>
                  {LEVEL_LABEL[l.level]}
                </span>
                <span className="ml-2 text-[0.71875rem] leading-[1.5] text-rex-text-muted">
                  An agent can {l.allows}.
                </span>
              </span>
              <CheckCircle2
                className="h-[17px] w-[17px] flex-none"
                style={{ color: on ? "var(--rex-brand)" : "var(--rex-border-strong)" }}
                strokeWidth={2}
                aria-hidden
              />
            </button>
          );
        })}
      </div>

      {level !== "read" && (
        // Indented under the cards, with a rule down its left: the duration
        // belongs to the level just chosen, and as a free-standing row it read
        // as a fourth setting of its own.
        <div className="ml-[7px] mt-1.5 flex flex-wrap items-center gap-x-2.5 gap-y-1 border-l border-rex-border-subtle py-0.5 pl-3">
          <span className="text-[0.71875rem] text-rex-text-muted">
            {LEVEL_LABEL[level]} lasts
          </span>
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
          {/* The way DOWN, next to the ways up. Read is reachable by clicking
              its card, but a person looking for "stop this now" looks for a
              button that says so, not for the top of a list of three. */}
          <button
            type="button"
            disabled={set.isPending}
            onClick={() => choose("read")}
            className="basis-full text-left text-[0.6875rem] font-medium text-rex-text-muted underline-offset-2 transition-colors hover:text-rex-text hover:underline disabled:opacity-50"
          >
            Back to Read now
          </button>
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
