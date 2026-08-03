import type { AgentAction, AgentOutcome } from "@/types";
import { cn } from "@/lib/utils";

const OUTCOME_LABEL: Record<AgentOutcome, string> = {
  ok: "ok",
  error: "error",
  denied: "denied",
  "unknown-tool": "unknown tool",
  "bad-request": "bad request",
};

/** Relative time from a SQLite UTC stamp ("YYYY-MM-DD HH:MM:SS" — no zone, so
 *  read it as UTC explicitly; a naive `new Date` would treat it as local). */
export function timeAgo(at: string): string {
  const t = Date.parse(at.replace(" ", "T") + "Z");
  if (Number.isNaN(t)) return at;
  const secs = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (secs < 45) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.round(hrs / 24)}d ago`;
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** What a row names as its target. The feed stores the STABLE site id and the
 *  domain is resolved at read time — so a row whose site no longer exists has no
 *  label, and that case is not rare: the reaper's own rows are about a site it
 *  just deleted. A bare UUID is not an answer to "which site" for a human, so
 *  say the true thing instead. (`detail` carries the domain for those rows.) */
function targetText(r: AgentAction): string | null {
  if (r.targetLabel) return r.targetLabel;
  if (!r.targetSite) return null;
  return UUID_RE.test(r.targetSite) ? "(deleted site)" : r.targetSite;
}

/**
 * The agent activity list — shared by the Settings card and the per-site
 * SiteDetail section. A concerning row (any non-ok outcome) gets a muted-amber
 * treatment: a thin left accent and an amber outcome label — visible without a
 * loud alarm fill, since a read-only agent erroring is a signal, not an incident.
 *
 * **A rexenv row says so, in the row** (v28 `actor`). The list can now contain
 * rexenv's own housekeeping — the scratch reaper deleting an expired site —
 * because that event must not be invisible. But an unlabelled row sitting under
 * a heading about agents would be untrue by juxtaposition, so those rows are
 * marked "rexenv · automatic" rather than wearing the client-name slot, where
 * "rexenv" would just read as an agent that calls itself rexenv. Nothing is
 * FILTERED here: hiding a row would trade one false impression for a missing
 * fact. (The status line above the list is the opposite call — it is a claim
 * about the agent's session, so `recent_head` excludes rexenv rows entirely.)
 */
export function AgentActivityFeed({
  rows,
  empty,
  hideTarget = false,
}: {
  rows: AgentAction[];
  empty: string;
  /** Drop the "→ site" reference — used in the per-site section, where naming
   *  the site again is redundant with the page you're already on. */
  hideTarget?: boolean;
}) {
  if (rows.length === 0) {
    return <div className="px-1 py-5 text-center text-[0.75rem] text-rex-text-dim">{empty}</div>;
  }
  return (
    <div className="flex flex-col">
      {rows.map((r) => (
        <div
          key={r.id}
          className={cn(
            "flex items-center gap-2.5 border-b border-rex-border-subtle py-2 last:border-b-0",
            r.concerning && "border-l-2 border-l-status-warning pl-2.5",
          )}
        >
          <span className="font-mono text-[0.71875rem] text-rex-text">{r.tool}</span>
          {r.argsSummary && (
            /* A VERB, not the command — deliberately unlabelled and dimmed.
               `plugin activate` does not say which plugin and `eval` says
               nothing about the code, so a caption like "command" would invite
               "I can see what the agent did" from something that shows only
               what KIND of thing it did. The title says the limit out loud.
               Two `[a-z0-9-]` tokens, clamped in Rust at the write — it cannot
               carry a separator or impersonate the client slot below. */
            <span
              className="flex-none font-mono text-[0.6875rem] text-rex-text-dim"
              title={`WP-CLI command and subcommand. Not the full command — arguments (which plugin, which option, what code) are not recorded.`}
            >
              {r.argsSummary}
            </span>
          )}
          {!hideTarget && targetText(r) && (
            <span className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
              → {targetText(r)}
            </span>
          )}
          <span
            className={cn(
              "font-mono text-[0.6875rem]",
              r.concerning ? "text-status-warning-bright" : "text-rex-text-dim",
            )}
            title={r.detail ?? undefined}
          >
            {OUTCOME_LABEL[r.outcome]}
          </span>
          <span className="ml-auto flex flex-none items-center gap-1.5 text-[0.6875rem] text-rex-text-dim">
            {r.actor === "rexenv" ? (
              <span
                className="max-w-[130px] truncate italic"
                title="rexenv did this itself — not an AI agent"
              >
                rexenv · automatic
              </span>
            ) : (
              <span className="max-w-[130px] truncate" title={r.client}>
                {r.client}
              </span>
            )}
            <span aria-hidden>·</span>
            <span title={r.at}>{timeAgo(r.at)}</span>
          </span>
        </div>
      ))}
    </div>
  );
}
