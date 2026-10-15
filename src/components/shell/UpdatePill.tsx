import { Link } from "react-router-dom";
import { useIsMutating, useQuery } from "@tanstack/react-query";
import { ArrowDownToLine, ArrowUp, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { appUpdateReadiness, appUpdateState } from "@/lib/ipc";
import { APP_UPDATE_CHECK_MUTATION, ABOUT_PATH, useCheckForUpdates } from "@/lib/useCheckForUpdates";
import type { AppUpdateReadiness, AppUpdateState } from "@/types";

/** What the pill shows. Derived from the SAME two caches the About card reads,
 *  never from a query of its own — so the pill and the card cannot disagree. */
export type UpdatePillPhase =
  | "dark" //      no key pinned (dev builds): nothing to offer, nothing rendered
  | "idle" //      no offer: the quiet "Check for Updates…" door
  | "checking" //  the card's own check is in flight
  | "offered" //   a verified offer, installable
  | "refused" //   a verified offer this install cannot take (the card says why)
  | "skipped" //   the offer is the version the user set aside — renders as idle
  | "pending"; //  swapped onto disk, waiting for the restart

export function updatePillPhase(
  st: AppUpdateState | undefined,
  ready: AppUpdateReadiness | null | undefined,
  checking: boolean,
): UpdatePillPhase {
  if (!st?.enabled) return "dark";
  if (st.installedPending) return "pending";
  if (st.offered && st.skipped === st.offered.version) return "skipped";
  if (st.offered) return ready?.refusal ? "refused" : "offered";
  return checking ? "checking" : "idle";
}

/**
 * The app-update notice in the app's own chrome (`docs/PLAN-update-notice.md`
 * S1/S2): `bar` sits at the right of every TopBar, `rail` beside Settings in the
 * sidebar.
 *
 * **A door to About, never an installer and never a checker** (ledger #885). It
 * reads `["app-update"]` / `["app-update-readiness"]` from the cache — kept fresh
 * by the backend's `app-update` push (#884), the card and the menu item — and its
 * clicks only NAVIGATE (plus, for the idle door, the same invalidate the app
 * menu's item does). The card owns the check mutation and its failure copy, and
 * the consent sentence in front of the one Install button; a pill that did either
 * would be a second place deciding what they look like.
 *
 * Why it exists: the only always-visible notice used to be a bare mono `0.6.0`
 * beside Settings, and the people it was meant to alert read it as the RUNNING
 * version (owner, 15 Oct 2026). So the pill carries a verb ("Update to",
 * "Restart to finish"), and the rail one carries no version at all.
 */
export function UpdatePill({ variant }: { variant: "bar" | "rail" }) {
  const { data: st } = useQuery({
    queryKey: ["app-update"],
    queryFn: appUpdateState,
    // Told, not polling: the backend's `app-update` event and the card's own
    // mutations are what move this cache.
    staleTime: Infinity,
  });
  const { data: ready } = useQuery({
    queryKey: ["app-update-readiness"],
    queryFn: appUpdateReadiness,
    staleTime: Infinity,
  });
  const checking = useIsMutating({ mutationKey: APP_UPDATE_CHECK_MUTATION }) > 0;
  const checkForUpdates = useCheckForUpdates();
  const phase = updatePillPhase(st, ready, checking);

  if (phase === "dark") return null;
  const notice = phase === "offered" || phase === "refused" || phase === "pending";

  if (variant === "rail") {
    // The rail says only that there is something; the TopBar names it. A
    // version here is the very thing that was misread.
    if (!notice) return null;
    return (
      <Link
        to={ABOUT_PATH}
        data-probe="update-pill"
        data-variant="rail"
        data-phase={phase}
        title={phase === "pending" ? "Restart to finish the update" : "An update is ready — open About"}
        className="no-drag mx-1 mb-1 flex h-[26px] items-center gap-[7px] self-start rounded-full border border-brand/40 bg-brand-active px-2.5 text-[0.75rem] font-medium text-brand-tint transition-colors hover:border-brand"
      >
        <span className="h-1.5 w-1.5 flex-none rounded-full bg-brand" aria-hidden />
        {phase === "pending" ? "Restart to finish update" : "Update ready"}
      </Link>
    );
  }

  if (!notice) {
    return (
      <button
        type="button"
        onClick={checkForUpdates}
        disabled={phase === "checking"}
        data-probe="update-pill"
        data-variant="bar"
        data-phase={phase}
        title="Check for Updates…"
        aria-label="Check for Updates…"
        className="flex h-[34px] w-[34px] flex-none items-center justify-center rounded border border-transparent text-rex-text-muted transition-colors hover:border-rex-border hover:bg-rex-hover hover:text-rex-text-bright disabled:opacity-60"
      >
        {phase === "checking" ? (
          <span className="h-3.5 w-3.5 rounded-full border-2 border-brand/30 border-t-brand animate-rex-spin motion-reduce:animate-none" />
        ) : (
          <RefreshCw className="h-[15px] w-[15px]" strokeWidth={1.9} />
        )}
      </button>
    );
  }

  const version = phase === "pending" ? st?.installedPending : st?.offered?.version;
  const pending = phase === "pending";
  return (
    <Link
      to={ABOUT_PATH}
      data-probe="update-pill"
      data-variant="bar"
      data-phase={phase}
      data-version={version ?? ""}
      className={cn(
        "flex h-[30px] min-w-0 max-w-[15rem] flex-none items-center gap-1.5 rounded-full border px-3 text-[0.78125rem] font-medium transition-colors",
        pending
          ? "border-rex-border-strong bg-rex-surface-2 text-rex-text-bright hover:bg-rex-surface-2-hover"
          : "border-brand/40 bg-brand-active text-brand-tint hover:border-brand",
      )}
    >
      {pending ? (
        <ArrowDownToLine className="h-[14px] w-[14px] flex-none" strokeWidth={2} />
      ) : (
        <ArrowUp className="h-[14px] w-[14px] flex-none" strokeWidth={2} />
      )}
      {/* The LABEL truncates at a narrow window, never the version. */}
      <span className="min-w-0 truncate">{pending ? "Restart to finish" : "Update to"}</span>
      <span className="flex-none font-mono">{version}</span>
    </Link>
  );
}
