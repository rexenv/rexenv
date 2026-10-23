import { Link } from "react-router-dom";
import { useIsMutating, useQuery } from "@tanstack/react-query";
import { ArrowDownToLine, ArrowUp, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { appUpdateReadiness, appUpdateState, updatesInventory } from "@/lib/ipc";
import { APP_UPDATE_CHECK_MUTATION, ABOUT_PATH, useCheckForUpdates } from "@/lib/useCheckForUpdates";
import type { AppUpdateReadiness, AppUpdateState, UpdateInventory, UpdateOffer } from "@/types";

/** What the pill shows. Derived from the SAME caches the cards read — the app's
 *  state and the ONE inventory (PLAN-unified-updates T3) — never from a query of
 *  its own, so the pill and the page cannot disagree. */
export type UpdatePillPhase =
  | "dark" //      no key pinned (dev builds): nothing to offer, nothing rendered
  | "idle" //      no offer: the quiet "Check for Updates…" door
  | "checking" //  the card's own check is in flight
  | "offered" //   one or more verified offers, installable
  | "refused" //   the only offer is the app's, and this install cannot take it
  | "skipped" //   every offer is set aside — renders as idle
  | "pending"; //  the app is swapped onto disk, waiting for the restart

/** The offers the pill counts: every kind the user has not set aside. */
export function pendingOffers(inv: UpdateInventory | undefined): UpdateOffer[] {
  return (inv?.offers ?? []).filter((o) => !o.skipped);
}

export function updatePillPhase(
  st: AppUpdateState | undefined,
  inv: UpdateInventory | undefined,
  ready: AppUpdateReadiness | null | undefined,
  checking: boolean,
): UpdatePillPhase {
  if (!st?.enabled) return "dark";
  if (st.installedPending || inv?.restartPending) return "pending";
  const pending = pendingOffers(inv);
  if (pending.length) {
    // The app's refusal (About is not writable, …) blocks the app row only; with
    // another kind pending the pill is still a door to something installable.
    const onlyApp = pending.length === 1 && pending[0].kind.kind === "app";
    return onlyApp && ready?.refusal ? "refused" : "offered";
  }
  // The backend withdraws a skipped version (`offered` is null, `skipped` says
  // which) — never compare the two, the comparison is already made in Rust.
  if (st.skipped || (inv?.offers.length ?? 0) > 0) return "skipped";
  return checking ? "checking" : "idle";
}

/** The bar pill's words for what is pending: a name and a version for one, a count
 *  for several. The app keeps its bare version ("Update to 0.9.1"); every other kind
 *  uses the inventory's short spelling ("Update to PHP 8.3.28"). */
export function pillWords(pending: UpdateOffer[]): { verb: string; value: string } {
  if (pending.length === 1) {
    const o = pending[0];
    return { verb: "Update to", value: o.kind.kind === "app" ? o.offered : o.display };
  }
  return { verb: "updates", value: String(pending.length) };
}

/**
 * The app-update notice in the app's own chrome (`docs/archive/PLAN-update-notice.md`
 * S1/S2): `bar` sits at the right of every TopBar, `rail` beside Settings in the
 * sidebar.
 *
 * **A door to About, never an installer and never a checker itself** (ledger
 * #885). It reads `["app-update"]` / `["app-update-readiness"]` from the cache —
 * kept fresh by the backend's `app-update` push (#884) and the card — and its
 * clicks only NAVIGATE. The idle "Check for Updates…" door goes to About with
 * `check=1`, and it is the CARD that then runs the check (owner ruling, 15 Oct
 * 2026). The card owns the check mutation and its failure copy, and the consent
 * sentence in front of the one Install button; a pill that did either would be
 * a second place deciding what they look like.
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
  const { data: inv } = useQuery({
    queryKey: ["updates"],
    queryFn: updatesInventory,
    staleTime: Infinity,
  });
  const checking = useIsMutating({ mutationKey: APP_UPDATE_CHECK_MUTATION }) > 0;
  const checkForUpdates = useCheckForUpdates();
  const phase = updatePillPhase(st, inv, ready, checking);
  const pending = pendingOffers(inv);

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
        {phase === "pending" ? "Restart to finish update" : pending.length > 1 ? "Updates ready" : "Update ready"}
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

  const restart = phase === "pending";
  const words = restart
    ? { verb: "Restart to finish", value: inv?.restartPending ?? st?.installedPending ?? "" }
    : pillWords(pending);
  const version = words.value;
  const many = !restart && pending.length > 1;
  return (
    <Link
      to={ABOUT_PATH}
      data-probe="update-pill"
      data-variant="bar"
      data-phase={phase}
      data-version={version}
      data-count={pending.length}
      className={cn(
        // The pill never shrinks: at the window's minimum width it is the
        // TopBar's SEARCH that gives (190 → 80 px, TopBar.tsx), and the bar fits
        // with the pill whole. It used to be pushed past the window's edge,
        // version and all, on the 15.8 UTM VM (16 Oct 2026) — the search was fixed
        // at 190 px then. (A flex item cannot shrink below its nowrap text
        // anyway, so "the label truncates" was never a thing this could do.)
        "flex h-[30px] flex-none items-center gap-1.5 whitespace-nowrap rounded-full border px-3 text-[0.78125rem] font-medium transition-colors",
        restart
          ? "border-rex-border-strong bg-rex-surface-2 text-rex-text-bright hover:bg-rex-surface-2-hover"
          : "border-brand/40 bg-brand-active text-brand-tint hover:border-brand",
      )}
    >
      {restart ? (
        <ArrowDownToLine className="h-[14px] w-[14px] flex-none" strokeWidth={2} />
      ) : (
        <ArrowUp className="h-[14px] w-[14px] flex-none" strokeWidth={2} />
      )}
      {many ? (
        // "3 updates": the count in mono, the noun after it.
        <>
          <span className="flex-none font-mono">{version}</span>
          <span>{words.verb}</span>
        </>
      ) : (
        <>
          <span>{words.verb}</span>
          <span className="flex-none font-mono">{version}</span>
        </>
      )}
    </Link>
  );
}
