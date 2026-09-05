import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, FolderInput, Loader2, RefreshCw } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Button } from "@/components/ui/button";
import { confirm } from "@/components/ui/dialog";
import { toast, toastBackendError } from "@/lib/toast";
import { CHECK_INPUT, cn } from "@/lib/utils";
import { Track } from "@/components/shell/DownloadPanel";
import {
  onValetImportProgress,
  onValetImportRow,
  resolverHandBack,
  resolverTakeOver,
  resolverTldStatus,
  scanValetImport,
  startServices,
  valetImportCancel,
  valetImportRun,
} from "@/lib/ipc";
import type {
  ImportCandidate,
  ImportOutcome,
  ImportProgress,
  ResolverTldStatus,
} from "@/types";

/** How old the list on screen is. The rescan itself is too fast to see, so
 *  this — a real timestamp of the data being rendered — is what proves it ran. */
function ago(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  return `${Math.floor(s / 3600)}h ago`;
}

/** A row can be ticked only when importing it needs no further decision. */
function selectable(c: ImportCandidate): boolean {
  return c.status.status === "importable" && !!c.servePath;
}

function statusPill(c: ImportCandidate, outcome?: ImportOutcome) {
  if (outcome) {
    const tone =
      outcome.status === "imported"
        ? "border-status-running-border bg-status-running-bg text-status-running-bright"
        : outcome.status === "failed"
          ? "border-status-error-border bg-status-error-bg text-status-error-bright"
          : "border-rex-border-strong bg-rex-surface-2 text-rex-text-muted";
    // The database's own outcome, when databases were requested — the honest
    // reason travels in the string after the colon.
    const db = outcome.db
      ? outcome.db === "imported"
        ? { label: "DB copied", title: "The database was copied — the site still reads the old one until you switch it (Database tab)." }
        : outcome.db.startsWith("failed")
          ? { label: "DB failed", title: outcome.db }
          : { label: "DB skipped", title: outcome.db }
      : null;
    return { label: outcome.status, tone, title: outcome.reason ?? undefined, db };
  }
  switch (c.status.status) {
    case "importable":
      return {
        label: "ready",
        tone: "border-status-running-border bg-status-running-bg text-status-running-bright",
        title: undefined,
      };
    case "needsAttention":
      return {
        label: "needs attention",
        tone: "border-status-warning-border bg-status-warning-bg text-status-warning-bright",
        title: c.status.reason,
      };
    case "alreadyImported":
      return {
        label: "already here",
        tone: "border-rex-border-strong bg-rex-surface-2 text-rex-text-muted",
        title: undefined,
      };
    default:
      return {
        label: "can't import",
        tone: "border-rex-border-strong bg-rex-surface-2 text-rex-text-muted",
        title: c.status.reason,
      };
  }
}

/**
 * Import sites from Valet or Herd.
 *
 * The scan is strictly read-only — their config, symlinks and per-site confs
 * are read, nothing of theirs is written, started or stopped, and no file
 * inside a project is opened. Everything the list can't import is still SHOWN
 * with its reason, because a site the user can see in Herd but not here would
 * make them doubt the whole list.
 */
export function Import() {
  const qc = useQueryClient();
  const { data, isLoading, refetch, isFetching, dataUpdatedAt } = useQuery({
    queryKey: ["valet-scan"],
    queryFn: scanValetImport,
  });
  // A rescan of a few dozen folders finishes in milliseconds, so the spinner
  // came and went inside one frame and the click read as "nothing happened".
  // The floor is on the SPINNER only — never on the scan, and the "scanned N
  // ago" line beside it is the real timestamp of the data on screen.
  const [spinFloor, setSpinFloor] = useState(false);
  const rescan = async () => {
    setSpinFloor(true);
    const min = new Promise((r) => setTimeout(r, 550));
    await Promise.all([refetch(), min]);
    setSpinFloor(false);
  };
  const scanning = isFetching || spinFloor;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 5_000);
    return () => clearInterval(t);
  }, []);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [outcomes, setOutcomes] = useState<Record<string, ImportOutcome>>({});
  const [running, setRunning] = useState(false);
  // ON by default: rexenv is the whole stack, so someone migrating off Valet
  // or Herd is migrating their databases too — a site imported without one
  // still reads the old engine, which is the surprise, not the copy. Visible
  // and untickable before the run, so it stays a choice.
  const [withDatabases, setWithDatabases] = useState(true);
  const [progress, setProgress] = useState<ImportProgress | null>(null);

  const candidates = useMemo(() => data?.candidates ?? [], [data]);
  const ready = useMemo(() => candidates.filter(selectable).map((c) => c.domain), [candidates]);

  // Drop selections for rows a rescan removed, so the count can't lie.
  useEffect(() => {
    setPicked((prev) => {
      const live = new Set(ready);
      const next = new Set([...prev].filter((d) => live.has(d)));
      return next.size === prev.size ? prev : next;
    });
  }, [ready]);

  // Rows settle one at a time; show each as it lands rather than all at the end.
  // Progress ticks in between, so a long site (or a multi-GB database) is never
  // a silent wait.
  useEffect(() => {
    let dead = false;
    const subs = [
      onValetImportRow((row) => {
        if (!dead) setOutcomes((o) => ({ ...o, [row.domain]: row }));
      }),
      onValetImportProgress((p) => {
        if (!dead) setProgress(p);
      }),
    ];
    return () => {
      dead = true;
      subs.forEach((s) => void s.then((f) => f()).catch(() => {}));
    };
  }, []);

  const allPicked = ready.length > 0 && ready.every((d) => picked.has(d));
  const run = useMutation({
    mutationFn: () =>
      valetImportRun({
        domains: [...picked].sort(),
        php: {},
        importDatabases: withDatabases,
      }),
    onMutate: () => {
      setOutcomes({});
      setProgress(null);
      setRunning(true);
    },
    onSuccess: (r) => {
      setRunning(false);
      setProgress(null);
      qc.invalidateQueries({ queryKey: ["sites"] });
      void refetch();
      const bits = [`${r.imported} imported`];
      if (r.failed) bits.push(`${r.failed} failed`);
      if (r.skipped) bits.push(`${r.skipped} skipped`);
      if (r.dbImported || r.dbFailed) {
        bits.push(`${r.dbImported} database${r.dbImported === 1 ? "" : "s"} copied`);
        if (r.dbFailed) bits.push(`${r.dbFailed} database${r.dbFailed === 1 ? "" : "s"} failed`);
      }
      if (r.failed) toast.error(bits.join(", "));
      else toast.success(bits.join(", "));
      // TWO situations, not one. Until 13 Aug 2026 both read "another app is
      // answering port 443 — quit it", and importing before Start-all — an
      // ordinary order — sent people looking for a program that wasn't there.
      if (r.serving?.kind === "stopped") {
        // Nothing holds the port; rexenv just isn't serving yet. The fix is a
        // button in this app, so it IS the button — not advice to find one.
        toast.error(
          `Imported. Your sites won't load until rexenv's services are running.`,
          undefined,
          { label: "Start all", onClick: () => void startServices().catch(toastBackendError) },
        );
      } else if (r.serving?.kind === "foreign") {
        const holder = r.serving.holder ?? "another app";
        const quit = r.serving.app ?? "it";
        toast.error(
          `Imported, but ${holder} is answering port 443 — quit ${quit} and your sites load automatically.`,
          r.serving.fix ?? undefined,
        );
      }
    },
    onError: (e) => {
      // The bar is LEFT where the work stopped — rolling it back or hiding it
      // would erase which sites did come over before the run died.
      setRunning(false);
      toastBackendError(e);
    },
  });

  const blocked = (data?.tlds ?? []).filter((t) => t.owner === "foreign" || t.owner === "drifted");

  return (
    <>
      <TopBar
        title="Import from Valet or Herd"
        subtitle={
          isLoading
            ? "Scanning…"
            : scanning
              ? "Rescanning…"
              : `${candidates.length} found · ${ready.length} ready to import · scanned ${ago(dataUpdatedAt, now)}`
        }
        showSearch={false}
        action={
          <Button variant="secondary" onClick={() => void rescan()} disabled={scanning || running}>
            <RefreshCw className={cn("mr-1.5 h-3.5 w-3.5", scanning && "animate-rex-spin")} />
            {scanning ? "Rescanning…" : "Rescan"}
          </Button>
        }
      />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        {isLoading ? (
          <div className="flex items-center gap-2 text-[0.8125rem] text-rex-text-muted">
            <Loader2 className="h-4 w-4 animate-rex-spin" /> Reading your Valet and Herd setup…
          </div>
        ) : candidates.length === 0 ? (
          <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-6 text-center">
            <FolderInput className="mx-auto h-6 w-6 text-rex-text-dim" strokeWidth={1.6} />
            <div className="mt-2 text-[0.875rem] text-rex-text">No Valet or Herd sites found</div>
            <div className="mt-1 text-[0.75rem] text-rex-text-muted">
              rexenv looked in <span className="font-mono">~/.config/valet</span> and Herd's
              application-support folder. Nothing of theirs was changed.
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-[14px]">
            {(data?.sources ?? []).map((s) => (
              <div
                key={s.home}
                className="rounded-xl border border-rex-border bg-rex-surface-1 px-4 py-3"
              >
                <div className="flex items-baseline justify-between gap-3">
                  <span className="text-[0.84375rem] font-medium text-rex-text">
                    {s.kind === "herd" ? "Herd" : "Valet"}
                  </span>
                  <span className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
                    {s.home}
                  </span>
                </div>
                <div className="mt-0.5 text-[0.71875rem] text-rex-text-muted">
                  serving <span className="font-mono">.{s.tld}</span>
                  {s.parked.length > 0 && ` · ${s.parked.length} parked folder(s)`}
                </div>
                {s.notes.map((n) => (
                  <div key={n} className="mt-1 text-[0.6875rem] text-rex-text-muted">
                    {n}
                  </div>
                ))}
              </div>
            ))}

            {blocked.map((t) => (
              <ResolverConsent key={t.tld} tld={t} onDone={() => void refetch()} />
            ))}

            {progress && (
              <ImportProgressCard
                progress={progress}
                running={running}
                outcomes={outcomes}
                onCancel={() => void valetImportCancel().catch(toastBackendError)}
              />
            )}

            <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
              <div className="flex items-center gap-3 border-b border-rex-border-subtle px-4 py-2.5">
                <input
                  type="checkbox"
                  className={CHECK_INPUT}
                  checked={allPicked}
                  disabled={ready.length === 0 || running}
                  ref={(el) => {
                    if (el) el.indeterminate = !allPicked && ready.some((d) => picked.has(d));
                  }}
                  onChange={() => setPicked(allPicked ? new Set() : new Set(ready))}
                />
                <span className="text-[0.75rem] text-rex-text-muted">
                  {picked.size > 0 ? `${picked.size} selected` : "Select sites to import"}
                </span>
                <div className="ml-auto flex items-center gap-3">
                  <label className="flex cursor-pointer items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
                    <input
                      type="checkbox"
                      className={CHECK_INPUT}
                      checked={withDatabases}
                      disabled={running}
                      onChange={(e) => setWithDatabases(e.target.checked)}
                    />
                    also copy databases
                    <span
                      className="cursor-help"
                      title="After each site imports, rexenv copies its database too (a read — the old database is never touched). The site keeps using the OLD database until you switch it over; each site's Database tab shows the exact change."
                    >
                      ⓘ
                    </span>
                  </label>
                  <Button
                    variant="primary"
                    disabled={picked.size === 0 || running}
                    onClick={() => run.mutate()}
                  >
                    {running ? "Importing…" : `Import ${picked.size || ""}`.trim()}
                  </Button>
                </div>
              </div>
              {candidates.map((c) => {
                // While the batch runs, a picked row that hasn't settled says
                // where it stands — in flight or still queued — instead of
                // showing the pre-run "ready" it no longer means.
                const live =
                  running && picked.has(c.domain) && !outcomes[c.domain]
                    ? progress?.domain === c.domain
                      ? { label: "importing…", waiting: false }
                      : { label: "waiting", waiting: true }
                    : null;
                const pill = live
                  ? {
                      label: live.label,
                      tone: live.waiting
                        ? "border-rex-border-strong bg-rex-surface-2 text-rex-text-muted"
                        : "border-brand bg-rex-surface-2 text-rex-text-bright",
                      title: live.waiting ? undefined : (progress?.detail ?? undefined),
                      db: null,
                    }
                  : statusPill(c, outcomes[c.domain]);
                const can = selectable(c) && !running;
                return (
                  <div
                    key={c.domain}
                    className="flex items-center gap-3 border-b border-rex-border-subtle px-4 py-2.5 last:border-b-0"
                  >
                    <input
                      type="checkbox"
                      className={CHECK_INPUT}
                      checked={picked.has(c.domain)}
                      disabled={!can}
                      title={selectable(c) ? undefined : pill.title}
                      onChange={() =>
                        setPicked((s) => {
                          const n = new Set(s);
                          if (n.has(c.domain)) n.delete(c.domain);
                          else n.add(c.domain);
                          return n;
                        })
                      }
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline gap-2">
                        <span className="truncate font-mono text-[0.78125rem] text-rex-text-bright">
                          {c.domain}
                        </span>
                        {c.label && (
                          <span className="flex-none text-[0.6875rem] text-rex-text-muted">
                            {c.label}
                          </span>
                        )}
                        {c.alsoIn && (
                          <span className="flex-none text-[0.625rem] text-rex-text-muted">
                            also in {c.alsoIn === "herd" ? "Herd" : "Valet"}
                          </span>
                        )}
                      </div>
                      <div className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
                        {c.servePath ?? c.path ?? "—"}
                        {c.docrootRel ? ` (serving ${c.docrootRel}/)` : ""}
                      </div>
                      {pill.title && (
                        <div className="mt-0.5 text-[0.6875rem] text-rex-text-muted">
                          {pill.title}
                        </div>
                      )}
                    </div>
                    <span className="flex-none font-mono text-[0.6875rem] text-rex-text-muted">
                      {c.phpTarget ? `PHP ${c.phpTarget}` : c.phpMinor ? `PHP ${c.phpMinor}` : ""}
                    </span>
                    {"db" in pill && pill.db && (
                      <span
                        className={cn(
                          "flex-none rounded-full border px-2 py-1 font-mono text-[0.625rem]",
                          pill.db.label === "DB copied"
                            ? "border-status-warning-border bg-status-warning-bg text-status-warning-bright"
                            : pill.db.label === "DB failed"
                              ? "border-status-error-border bg-status-error-bg text-status-error-bright"
                              : "border-rex-border-strong bg-rex-surface-2 text-rex-text-muted",
                        )}
                        title={pill.db.title}
                      >
                        {pill.db.label}
                      </span>
                    )}
                    <span
                      className={cn(
                        "flex-none rounded-full border px-2 py-1 font-mono text-[0.625rem]",
                        pill.tone,
                      )}
                    >
                      {pill.label}
                    </span>
                  </div>
                );
              })}
            </div>

            <div className="text-[0.6875rem] leading-[1.55] text-rex-text-muted">
              Importing links each folder where it already is — nothing is copied or moved, and
              deleting a site in rexenv never deletes your folder. Your Valet and Herd setup is
              left exactly as it is, so you can go back at any time. Databases are COPIED, never
              moved — the old one is only read, and each site keeps using it until you switch it
              over on its Database tab.
            </div>
          </div>
        )}
      </div>
    </>
  );
}

/**
 * What the batch is doing right now.
 *
 * Honesty rules (the provision-card family):
 * - the bar is the BACKEND's `pct`: completed rows plus the running job's own
 *   phase-weighted pct — real completions, never a time estimate. Monotonic,
 *   99-capped until the batch settles,
 * - the step line is the running job's OWN label, verbatim; nothing here is
 *   invented by the screen,
 * - when the run ends early the bar FREEZES where the work stopped, and the
 *   settled counts below it say what actually came over.
 */
/** Exported for the DEV harness only (`#/dev/git-panel?panel=import-bar`), so
 *  `wk-checks/importbar.js` can drive a scripted batch — including a mid-batch
 *  FAILURE — through the real card. Asserting this against `batch_pct` in Rust
 *  would test the arithmetic, which is the half this claim is NOT about (#243). */
export function ImportProgressCard({
  progress: p,
  running,
  outcomes,
  onCancel,
}: {
  progress: ImportProgress;
  running: boolean;
  outcomes: Record<string, ImportOutcome>;
  onCancel: () => void;
}) {
  const settled = Object.values(outcomes);
  const imported = settled.filter((o) => o.status === "imported").length;
  const failed = settled.filter((o) => o.status === "failed").length;
  const skipped = settled.filter((o) => o.status === "skipped").length;
  const headline =
    p.stage === "site" || p.stage === "database"
      ? `${p.stage === "database" ? "Copying the database for" : "Importing"} ${p.domain ?? ""}`
      : p.stage === "scanning"
        ? "Reading your Valet and Herd setup"
        : p.stage === "resolvers"
          ? "Making these domains resolve to rexenv"
          : p.stage === "php"
            ? "Getting PHP ready"
            : p.stage === "checking"
              ? "Checking your sites will load"
              : "Finished";

  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1 px-4 py-3">
      <div className="flex items-baseline justify-between gap-3">
        <div className="flex min-w-0 items-baseline gap-2">
          {running && (
            <Loader2 className="h-3.5 w-3.5 flex-none animate-rex-spin text-rex-text-muted" />
          )}
          <span className="truncate text-[0.84375rem] text-rex-text">{headline}</span>
        </div>
        <span className="flex-none font-mono text-[0.6875rem] text-rex-text-muted">
          {p.index > 0 ? `site ${p.index} · ` : ""}
          {p.done} of {p.total} done · {p.pct}%
        </span>
      </div>
      <div className="mt-2">
        <Track pct={p.pct} state={running ? "run" : p.stage === "done" ? "ok" : "stopped"} />
      </div>
      <div className="mt-1.5 flex items-baseline justify-between gap-3">
        <span className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
          {p.detail ?? (running ? "working…" : "stopped")}
        </span>
        {running && (
          <Button variant="ghost" onClick={onCancel}>
            Cancel after current
          </Button>
        )}
      </div>
      {settled.length > 0 && (
        <div className="mt-1.5 text-[0.6875rem] text-rex-text-muted">
          {imported} imported
          {failed > 0 && ` · ${failed} failed`}
          {skipped > 0 && ` · ${skipped} skipped`}
        </div>
      )}
    </div>
  );
}

/**
 * Consent before rexenv takes a TLD's resolver file from Valet/Herd.
 *
 * Their file is shown beside ours verbatim, the checkbox is unticked, and the
 * alternative (import on .rex instead) is stated rather than buried — taking
 * someone's system file is not something to slip past them.
 *
 * Exported since 5 Sep 2026 because this card used to exist ONLY here, and
 * only for TLDs the scan found in Valet's own sites. The refusal it answers
 * ("… managed by another tool … rexenv can take that TLD over") is raised by
 * Change domain, by site creation on the default TLD, and by Settings' repair
 * — and each of those told the user about a button that was on another page,
 * or on no page at all once Valet had no `.test` site left to list. The card
 * now renders where the refusal would land; `alternative` is the way out that
 * fits the caller (import on .rex / pick another ending).
 */
export function ResolverConsent({
  tld,
  onDone,
  alternative,
}: {
  tld: ResolverTldStatus;
  onDone: () => void;
  alternative?: ReactNode;
}) {
  const qc = useQueryClient();
  const [agreed, setAgreed] = useState(false);
  const take = useMutation({
    mutationFn: () => resolverTakeOver(tld.tld),
    onSuccess: () => {
      toast.success(`rexenv now answers .${tld.tld} — Valet's file is backed up.`);
      // Every reader of who-owns-this-TLD: the scan, the per-TLD status the
      // dialogs ask, the Settings "can't be resolved" card, the drift banner.
      void qc.invalidateQueries({ queryKey: ["valet-scan"] });
      void qc.invalidateQueries({ queryKey: ["resolver-tld-status"] });
      void qc.invalidateQueries({ queryKey: ["unresolvable-tlds"] });
      void qc.invalidateQueries({ queryKey: ["resolver-drift"] });
      onDone();
    },
    onError: (e) => toastBackendError(e),
  });

  const drifted = tld.owner === "drifted";
  return (
    <div className="rounded-xl border border-status-warning-border bg-status-warning-bg/40 p-4">
      <div className="flex items-start gap-2">
        <AlertCircle className="mt-0.5 h-4 w-4 flex-none text-status-warning-bright" />
        <div className="min-w-0">
          <div className="text-[0.84375rem] font-medium text-rex-text">
            {drifted
              ? `Valet or Herd took .${tld.tld} back`
              : `.${tld.tld} is managed by Valet or Herd`}
          </div>
          <div className="mt-1 text-[0.75rem] leading-[1.55] text-rex-text-muted">
            {drifted
              ? `rexenv had taken over ${tld.path}, but it's theirs again — so rexenv .${tld.tld} sites won't resolve until you take it over again or move them to .rex.`
              : `${tld.path} tells macOS where to send .${tld.tld} lookups. To serve these sites, rexenv needs to answer them instead.`}
          </div>
        </div>
      </div>

      <div className="mt-3 grid grid-cols-2 gap-2">
        <div>
          <div className="mb-1 text-[0.625rem] uppercase tracking-wide text-rex-text-muted">
            Theirs now
          </div>
          <pre className="overflow-x-auto rounded-md border border-rex-border-strong bg-rex-well px-2.5 py-2 font-mono text-[0.6875rem] text-rex-text">
            {tld.theirContent ?? "(couldn't read it)"}
          </pre>
        </div>
        <div>
          <div className="mb-1 text-[0.625rem] uppercase tracking-wide text-rex-text-muted">
            rexenv would write
          </div>
          <pre className="overflow-x-auto rounded-md border border-rex-border-strong bg-rex-well px-2.5 py-2 font-mono text-[0.6875rem] text-rex-text">
            {tld.ourContent}
          </pre>
        </div>
      </div>

      <label className="mt-3 flex cursor-pointer items-start gap-2 text-[0.75rem] text-rex-text">
        <input
          type="checkbox"
          className={cn(CHECK_INPUT, "mt-0.5")}
          checked={agreed}
          onChange={(e) => setAgreed(e.target.checked)}
        />
        <span>
          Let rexenv answer <span className="font-mono">.{tld.tld}</span>. Their file is backed up
          first and you can hand it back in one click. Valet and Herd will take it back themselves
          whenever you run <span className="font-mono">valet install</span> or Herd's onboarding.
        </span>
      </label>

      <div className="mt-3 flex items-center gap-2">
        <Button variant="primary" disabled={!agreed || take.isPending} onClick={() => take.mutate()}>
          {take.isPending ? "Taking over…" : `Take over .${tld.tld}`}
        </Button>
        <span className="text-[0.6875rem] text-rex-text-muted">
          {alternative ?? (
            <>
              Or leave it alone and import these sites on <span className="font-mono">.rex</span>{" "}
              instead — their URLs change, but nothing of Valet's is touched.
            </>
          )}
        </span>
      </div>
    </div>
  );
}

/**
 * The consent card for a TLD the user TYPED, looked up on demand: renders the
 * card only while another tool owns (or has reclaimed) that TLD's resolver
 * file, and nothing at all otherwise — so a caller can drop it under any TLD
 * input and let it speak only when there is something to consent to.
 * `onOwnership` reports whether the TLD is currently blocked, for callers that
 * gate their own submit on it.
 */
export function ResolverConsentFor({
  tld,
  alternative,
  onOwnership,
}: {
  tld: string;
  alternative?: ReactNode;
  onOwnership?: (blocked: boolean) => void;
}) {
  const { data, refetch } = useQuery({
    queryKey: ["resolver-tld-status", tld],
    queryFn: () => resolverTldStatus(tld),
    enabled: tld !== "",
    // A refused TLD (policy) throws; the caller's own policy feedback covers
    // that, so this stays silent rather than retrying it.
    retry: false,
  });
  const blocked = data?.owner === "foreign" || data?.owner === "drifted";
  useEffect(() => {
    onOwnership?.(blocked);
  }, [blocked, onOwnership]);
  if (!data || !blocked) return null;
  return <ResolverConsent tld={data} onDone={() => void refetch()} alternative={alternative} />;
}

/**
 * "Hand it back" for a TLD rexenv borrowed — the return half of the borrow,
 * one click rather than "uninstall rexenv".
 */
export function ResolverHandBackRow({ tld }: { tld: ResolverTldStatus }) {
  const qc = useQueryClient();
  const give = useMutation({
    mutationFn: () => resolverHandBack(tld.tld),
    onSuccess: (plan) => {
      qc.invalidateQueries({ queryKey: ["valet-scan"] });
      if (plan.backupMissing.length) {
        toast.error(
          `rexenv's copy of ${tld.path} was gone, so its own file was removed instead — run \`valet install\` to restore theirs.`,
        );
      } else {
        toast.success(`.${tld.tld} handed back to Valet/Herd.`);
      }
    },
    onError: (e) => toastBackendError(e),
  });

  return (
    <div className="flex items-center justify-between gap-4 rounded-lg border border-rex-border-subtle px-3 py-2.5">
      <div className="min-w-0">
        <div className="font-mono text-[0.78125rem] text-rex-text-bright">.{tld.tld}</div>
        <div className="mt-0.5 text-[0.71875rem] text-rex-text-muted">
          rexenv answers this, borrowed from Valet/Herd. Their file is backed up.
        </div>
      </div>
      <Button
        variant="secondary"
        disabled={give.isPending}
        onClick={async () => {
          const ok = await confirm({
            title: `Hand .${tld.tld} back?`,
            message: (
              <>
                Valet and Herd answer <span className="font-mono">.{tld.tld}</span> again, and their
                original file is restored.
                {tld.rexenvSites > 0 && (
                  <>
                    {" "}
                    <span className="font-medium">
                      Your {tld.rexenvSites} rexenv site
                      {tld.rexenvSites === 1 ? "" : "s"} on this domain will stop resolving
                    </span>{" "}
                    until you take it over again or move them to{" "}
                    <span className="font-mono">.rex</span>.
                  </>
                )}
              </>
            ),
            confirmLabel: "Hand it back",
          });
          if (ok) give.mutate();
        }}
      >
        {give.isPending ? "Handing back…" : "Hand back"}
      </Button>
    </div>
  );
}
