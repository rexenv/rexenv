import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Play, Power } from "lucide-react";
import { Button } from "@/components/ui/button";
import { databasesStatus, isTauri, servicesStatus, startServices } from "@/lib/ipc";
import { toastBackendError } from "@/lib/toast";
import type { SiteDbEngine } from "@/types";

/**
 * What a site tab needs RUNNING before it can say anything true: the site's own
 * database engine (wp-cli reads it; the browser browses it) and, for a tab served
 * THROUGH the stack (Adminer rides the edge → nginx → a pool), the web tier.
 */
export interface StackNeeds {
  engine?: SiteDbEngine;
  webTier?: boolean;
}

/** The stopped dependency a tab must name, or `null` when everything it needs is up. */
export interface StoppedDependency {
  /** "MySQL", "MariaDB", "PostgreSQL" — or the web tier's own words. */
  label: string;
  kind: "engine" | "web";
}

/**
 * Which of `needs` is stopped right now — `undefined` until the first poll answers,
 * `null` when nothing the tab needs is down. Polled every 2 s (the Databases page's
 * cadence, and the same query keys, so the caches are shared) while the tab is
 * mounted, so the panel gives way to the real tab the moment Start all lands.
 *
 * Why this exists: a stopped site's WordPress tab spun "Loading plugins…" for as long
 * as wp-cli took to give up on a database that was not running, and its Database tab
 * printed the Adminer URL above a BLANK frame (Windows, 19 Sep 2026) — two honest-UI
 * failures the same site's Logs tab never had, because that tab says what is off and
 * how to turn it on. Outside the desktop app (the dev harness) nothing is polled and
 * the tab renders as before.
 */
export function useStoppedDependency(needs: StackNeeds): StoppedDependency | null | undefined {
  const inApp = isTauri();
  const dbs = useQuery({
    queryKey: ["databases"],
    queryFn: databasesStatus,
    enabled: inApp && !!needs.engine,
    refetchInterval: 2000,
  });
  const services = useQuery({
    queryKey: ["services"],
    queryFn: servicesStatus,
    enabled: inApp && !!needs.webTier,
    refetchInterval: 2000,
  });
  if (!inApp) return null;
  if (needs.engine) {
    if (!dbs.data) return dbs.isError ? null : undefined;
    const row = dbs.data.find((d) => d.key === needs.engine);
    // An engine this build does not list (refused on this OS) is not "stopped" — the
    // Databases page carries that sentence; here the tab just proceeds.
    if (row && !row.running) return { label: row.label, kind: "engine" };
  }
  if (needs.webTier) {
    if (!services.data) return services.isError ? null : undefined;
    const up = (name: string) => services.data.some((s) => s.name === name && s.running);
    if (!up("Caddy") || !up("Nginx")) return { label: "rexenv's web services", kind: "web" };
  }
  return null;
}

/**
 * The tab's body while `stopped` is down: names the service, says what the tab
 * cannot do without it, and offers Start all — never a spinner with no end and
 * never a blank frame. `what` is the tab's dependency in one clause, e.g.
 * "WordPress's plugins, themes, users and tools read its database".
 */
export function StackNeededPanel({ stopped, what }: { stopped: StoppedDependency; what: string }) {
  const qc = useQueryClient();
  const start = useMutation({
    mutationFn: startServices,
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ["services"] });
      qc.invalidateQueries({ queryKey: ["global-status"] });
      qc.invalidateQueries({ queryKey: ["databases"] });
    },
    onError: (e) => toastBackendError(e),
  });
  const headline =
    stopped.kind === "engine" ? `${stopped.label} is stopped` : "rexenv's web services are stopped";
  const consequence =
    stopped.kind === "engine"
      ? `${what} — nothing answers while it is down.`
      : `${what} is served through them — nothing loads while they are down.`;
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 rounded-xl border border-rex-well-border bg-rex-well-deep p-6 text-center">
      <div className="flex h-[46px] w-[46px] items-center justify-center rounded-[12px] border border-rex-panel-border bg-rex-surface-1 text-rex-text-muted">
        <Power className="h-[22px] w-[22px]" strokeWidth={1.6} />
      </div>
      <div className="text-[0.875rem] text-rex-text">{headline}</div>
      <div className="max-w-[440px] text-[0.8125rem] text-rex-text-muted">
        {consequence} Start all brings it back with the rest of the stack — the same button as
        the footer&apos;s.
      </div>
      <Button variant="primary" size="sm" disabled={start.isPending} onClick={() => start.mutate()}>
        {start.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Play className="h-3.5 w-3.5" />}
        {start.isPending ? "Starting…" : "Start all"}
      </Button>
    </div>
  );
}
