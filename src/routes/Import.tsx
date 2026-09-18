import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, FolderInput, Loader2, RefreshCw } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Button } from "@/components/ui/button";
import { confirm } from "@/components/ui/dialog";
import { toast, toastBackendError } from "@/lib/toast";
import { CHECK_INPUT, TECH_INPUT, cn } from "@/lib/utils";
import { Track } from "@/components/shell/DownloadPanel";
import { usePlatformWords } from "@/lib/usePlatformWords";
import {
  dbImportDeleteLeftover,
  dbImportLeftovers,
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
  ImportScan,
  ImportSource,
  ResolverTldStatus,
} from "@/types";

const SOURCE_LABEL: Record<ImportSource, string> = {
  valet: "Valet",
  herd: "Herd",
  local: "Local",
};

/** How old the list on screen is. The rescan itself is too fast to see, so
 *  this — a real timestamp of the data being rendered — is what proves it ran. */
function ago(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  return `${Math.floor(s / 3600)}h ago`;
}

/** A row can be ticked only when importing it needs no further decision — or
 *  when every decision it needs has been made on the row: a PHP version rexenv
 *  ships in place of a pin it doesn't, a free name in place of a taken one.
 *  Never a silent substitute. */
function selectable(
  c: ImportCandidate,
  phpPick: Record<string, string>,
  nameOk: (c: ImportCandidate) => boolean,
): boolean {
  if (!c.servePath) return false;
  if (c.status.status === "importable") return true;
  if (!c.phpChoice && !c.domainChoice) return false;
  return (!c.phpChoice || !!phpPick[c.domain]) && (!c.domainChoice || nameOk(c));
}

const tldOf = (domain: string) => domain.slice(domain.lastIndexOf(".") + 1);

/** The full name typed for a row: the label the user typed, on the row's own
 *  (re-homed, policy-allowed) TLD. */
const typedName = (c: ImportCandidate, label: string) =>
  `${label.trim().toLowerCase()}.${tldOf(c.domain)}`;

/** Why the name typed for a `domainChoice` row can't be used, or null when it
 *  can. Checked against every name rexenv answers on and every other row in the
 *  list; the run checks again with the backend's own hostname validator. */
function nameProblem(
  c: ImportCandidate,
  labels: Record<string, string>,
  scan: ImportScan | undefined,
): string | null {
  const label = (labels[c.domain] ?? "").trim().toLowerCase();
  if (!label) return "type a name";
  if (!label.split(".").every((p) => /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/.test(p)))
    return "use a–z, 0–9 and - (not at either end)";
  const full = typedName(c, label);
  if (scan?.takenDomains.includes(full)) return `${full} is already a rexenv site`;
  if (scan?.candidates.some((o) => o.domain === full)) return `another row in this list is ${full}`;
  if (
    Object.entries(labels).some(
      ([d, l]) => d !== c.domain && l.trim() !== "" && `${l.trim().toLowerCase()}.${tldOf(d)}` === full,
    )
  )
    return `another row is already taking ${full}`;
  return null;
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
    // What the site reads is NOT asserted here: a Valet/Herd site keeps its old
    // server until connected, a Local site reads none (ledger #575).
    const db = outcome.db
      ? outcome.db === "imported"
        ? outcome.connect === "connected"
          ? {
              label: "DB connected",
              title:
                "The database was copied and the site now reads the copy — its wp-config was edited (backed up; revert it on the site's Database tab).",
            }
          : {
              label: "DB copied",
              title: outcome.connect
                ? `The database was copied, but the site wasn't connected to it — ${outcome.connect}`
                : "The database was copied — the site isn't connected to it yet (its Database tab shows what it reads now and switches it).",
            }
        : outcome.db.startsWith("failed")
          ? { label: "DB failed", title: outcome.db }
          : { label: "DB skipped", title: outcome.db }
      : null;
    const title = outcome.servedAs
      ? [`imported as ${outcome.servedAs}`, outcome.reason].filter(Boolean).join(" — ")
      : (outcome.reason ?? undefined);
    return { label: outcome.status, tone, title, db };
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
 * Import sites from Valet, Herd or Local.
 *
 * The scan is strictly read-only — their config, symlinks, per-site confs and
 * Local's site registry are read, nothing of theirs is written, started or stopped, and no file
 * inside a project is opened. Everything the list can't import is still SHOWN
 * with its reason, because a site the user can see in Herd but not here would
 * make them doubt the whole list.
 */
export function Import() {
  const qc = useQueryClient();
  const words = usePlatformWords();
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
  // Per-row PHP choice for rows pinned to a version rexenv doesn't ship. Starts
  // EMPTY: the row stays unticked until someone picks, so no version is ever
  // chosen for them.
  const [phpPick, setPhpPick] = useState<Record<string, string>>({});
  // Per-row name LABEL for a re-homed row whose picked name is taken. Starts
  // empty for the same reason: nobody's site gets a name they didn't type.
  const [namePick, setNamePick] = useState<Record<string, string>>({});
  const [outcomes, setOutcomes] = useState<Record<string, ImportOutcome>>({});
  const [running, setRunning] = useState(false);
  // ON by default: rexenv is the whole stack, so someone migrating off Valet
  // or Herd is migrating their databases too — a site imported without one
  // still reads the old engine, which is the surprise, not the copy. Visible
  // and untickable before the run, so it stays a choice.
  const [withDatabases, setWithDatabases] = useState(true);
  // ON by default, for LOCAL rows only (owner, 12 Sep 2026): a Local site loads
  // nothing under rexenv until connected, so an import that stops at the copy
  // leaves a broken site. It edits their wp-config, so it is said on the box and
  // stays untickable; Valet/Herd sites keep their per-site choice.
  const [connectLocal, setConnectLocal] = useState(true);
  const hasLocal = (data?.candidates ?? []).some((c) => c.source === "local");
  const [progress, setProgress] = useState<ImportProgress | null>(null);

  const candidates = useMemo(() => data?.candidates ?? [], [data]);
  const ready = useMemo(
    () =>
      candidates
        .filter((c) => selectable(c, phpPick, (x) => nameProblem(x, namePick, data) === null))
        .map((c) => c.domain),
    [candidates, phpPick, namePick, data],
  );
  const isReady = (c: ImportCandidate) =>
    selectable(c, phpPick, (x) => nameProblem(x, namePick, data) === null);
  const chosenName = (c: ImportCandidate) =>
    c.domainChoice && namePick[c.domain] ? typedName(c, namePick[c.domain]) : c.domain;

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
        php: Object.fromEntries(Object.entries(phpPick).filter(([d]) => picked.has(d))),
        domain: Object.fromEntries(
          candidates
            .filter((c) => c.domainChoice && picked.has(c.domain) && namePick[c.domain])
            .map((c) => [c.domain, typedName(c, namePick[c.domain])]),
        ),
        importDatabases: withDatabases,
        connectLocal: withDatabases && connectLocal,
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
      if (r.connected) bits.push(`${r.connected} connected`);
      if (r.connectFailed) bits.push(`${r.connectFailed} not connected`);
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
  // A blocked TLD no Valet/Herd site uses any more — the leftover file. The
  // scan lists it since 5 Sep 2026 (it used to appear nowhere); the card's
  // copy must not promise "these sites" when there are none.
  const consentCards = blocked.map((t) => {
    const theirs = candidates.some((c) => c.domain.endsWith(`.${t.tld}`));
    return (
      <ResolverConsent
        key={t.tld}
        tld={t}
        onDone={() => void refetch()}
        reason={
          theirs ? undefined : (
            <>
              {t.path} still sends <span className="font-mono">.{t.tld}</span> lookups to Valet or
              Herd, but none of their sites use it now. rexenv sites on{" "}
              <span className="font-mono">.{t.tld}</span> — or a domain you change to it — can't
              resolve until rexenv answers it.
            </>
          )
        }
        alternative={
          theirs ? undefined : (
            <>
              Or leave it alone — nothing needs <span className="font-mono">.{t.tld}</span> yet, and
              a domain change to it will offer this again.
            </>
          )
        }
      />
    );
  });

  return (
    <>
      <TopBar
        title="Import from Valet, Herd or Local"
        subtitle={
          // No count where nobody looked. On Windows the card below says rexenv
          // does not know these tools' layouts yet and did not scan (#641) --
          // while this line read "0 found · 0 ready to import · scanned just
          // now", which is the empty-scan report #641 exists to avoid, one
          // element above the sentence denying it (seen on the Dell, 19 Sep 2026).
          !words.importsOtherTools
            ? "not scanned here yet"
            : isLoading
              ? "Scanning…"
              : scanning
                ? "Rescanning…"
                : `${candidates.length} found · ${ready.length} ready to import · scanned ${ago(dataUpdatedAt, now)}`
        }
        showSearch={false}
        action={
          <Button
            variant="secondary"
            onClick={() => void rescan()}
            disabled={scanning || running || !words.importsOtherTools}
          >
            <RefreshCw className={cn("mr-1.5 h-3.5 w-3.5", scanning && "animate-rex-spin")} />
            {scanning ? "Rescanning…" : "Rescan"}
          </Button>
        }
      />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        {isLoading ? (
          <div className="flex items-center gap-2 text-[0.8125rem] text-rex-text-muted">
            <Loader2 className="h-4 w-4 animate-rex-spin" /> Reading your Valet, Herd and Local setup…
          </div>
        ) : candidates.length === 0 ? (
          <div className="flex flex-col gap-[14px]">
            <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-6 text-center">
              <FolderInput className="mx-auto h-6 w-6 text-rex-text-dim" strokeWidth={1.6} />
              <div className="mt-2 text-[0.875rem] text-rex-text">
                {data?.unsupported
                  ? "Importing from Valet, Herd and Local isn't ready here yet"
                  : "No Valet, Herd or Local sites found"}
              </div>
              <div className="mt-1 text-[0.75rem] text-rex-text-muted">
                {data?.unsupported ?? (
                  <>
                    rexenv looked in <span className="font-mono">{words.importSearch}</span>. Nothing
                    of theirs was changed.
                  </>
                )}
              </div>
            </div>
            {/* No sites, but their resolver file can outlive them — this is the
                one place another tool's leftover route for a TLD shows up before someone
                types a .test domain. The empty state used to swallow it. */}
            {consentCards}
            <LeftoverDumpsCard />
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
                    {SOURCE_LABEL[s.kind]}
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

            {consentCards}

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
              </div>
              {candidates.map((c) => {
                // While the batch runs, a picked row that hasn't settled says
                // where it stands — in flight or still queued — instead of
                // showing the pre-run "ready" it no longer means.
                const live =
                  running && picked.has(c.domain) && !outcomes[c.domain]
                    ? progress?.domain === c.domain || progress?.domain === chosenName(c)
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
                  : c.status.status === "needsAttention" && !outcomes[c.domain] && isReady(c)
                    ? {
                        label: "ready",
                        tone: "border-status-running-border bg-status-running-bg text-status-running-bright",
                        title:
                          "Imports " +
                          [
                            c.domainChoice ? `as ${chosenName(c)}` : null,
                            c.phpChoice
                              ? `on PHP ${phpPick[c.domain]} instead of ${c.phpMinor ?? "its pinned version"} — check the site works on it`
                              : null,
                          ]
                            .filter(Boolean)
                            .join(", ") +
                          ".",
                        db: null,
                      }
                    : statusPill(c, outcomes[c.domain]);
                const can = isReady(c) && !running;
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
                      title={isReady(c) ? undefined : pill.title}
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
                        {c.renamedFrom && (
                          <span
                            className="flex-none font-mono text-[0.625rem] text-rex-text-muted"
                            title={`${SOURCE_LABEL[c.source]} served this as ${c.renamedFrom}; rexenv can't use that name, so it imports as ${c.domain}.`}
                          >
                            was {c.renamedFrom}
                          </span>
                        )}
                        {c.alsoIn && (
                          <span className="flex-none text-[0.625rem] text-rex-text-muted">
                            also in {SOURCE_LABEL[c.alsoIn]}
                          </span>
                        )}
                      </div>
                      {/* A link farm folds into this row (`fold_same_folder`); until
                          12 Sep 2026 its other names were imported but never shown, so
                          the row read as one name and the owner concluded they were lost. */}
                      {c.extraDomains.length > 0 && (
                        <div
                          className="truncate text-[0.6875rem] text-rex-text-muted"
                          title={`${SOURCE_LABEL[c.source]} links the same folder under these names too; they import as extra domains of this one site.`}
                        >
                          also answers on{" "}
                          <span className="font-mono text-rex-text">{c.extraDomains.join(", ")}</span>
                        </div>
                      )}
                      {c.multisite !== "none" && (
                        <div
                          className="truncate text-[0.6875rem] text-rex-text-muted"
                          title={`A WordPress multisite network (${c.multisite === "subdomain" ? "subsites on subdomains" : "subsites in subfolders"}). rexenv imports it as a network — it never converts it.`}
                        >
                          multisite network ·{" "}
                          {c.multisite === "subdomain" ? "subdomains" : "subdirectories"}
                          {c.subsites.length > 0 && (
                            <>
                              {" · "}
                              <span className="font-mono text-rex-text">
                                {c.subsites.map((l) => `${l}.${chosenName(c)}`).join(", ")}
                              </span>
                            </>
                          )}
                        </div>
                      )}
                      <div className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
                        {c.servePath ?? c.path ?? "—"}
                        {c.docrootRel ? ` (serving ${c.docrootRel}/)` : ""}
                      </div>
                      {c.domainChoice && c.servePath && !outcomes[c.domain] && (
                        <NameChoice
                          row={c}
                          label={namePick[c.domain] ?? ""}
                          problem={nameProblem(c, namePick, data)}
                          disabled={running}
                          onChange={(v) => {
                            setNamePick((p) => {
                              const n = { ...p };
                              if (v) n[c.domain] = v;
                              else delete n[c.domain];
                              return n;
                            });
                          }}
                        />
                      )}
                      {pill.title && (
                        <div className="mt-0.5 text-[0.6875rem] text-rex-text-muted">
                          {pill.title}
                        </div>
                      )}
                    </div>
                    {c.phpChoice && c.servePath && !outcomes[c.domain] ? (
                      <select
                        aria-label={`PHP version for ${c.domain}`}
                        value={phpPick[c.domain] ?? ""}
                        disabled={running}
                        onChange={(e) => {
                          const v = e.target.value;
                          setPhpPick((p) => {
                            const n = { ...p };
                            if (v) n[c.domain] = v;
                            else delete n[c.domain];
                            return n;
                          });
                          if (!v)
                            setPicked((s) => {
                              const n = new Set(s);
                              n.delete(c.domain);
                              return n;
                            });
                        }}
                        className="h-[26px] flex-none rounded border border-rex-border bg-rex-surface-2 px-1.5 font-mono text-[0.6875rem] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50"
                      >
                        <option value="">PHP {c.phpMinor ?? "?"} → choose…</option>
                        {(data?.availablePhp ?? []).map((v) => (
                          <option key={v} value={v}>
                            PHP {v}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <span className="flex-none font-mono text-[0.6875rem] text-rex-text-muted">
                        {c.phpTarget ? `PHP ${c.phpTarget}` : c.phpMinor ? `PHP ${c.phpMinor}` : ""}
                      </span>
                    )}
                    {"db" in pill && pill.db && (
                      <span
                        className={cn(
                          "flex-none rounded-full border px-2 py-1 font-mono text-[0.625rem]",
                          pill.db.label === "DB connected"
                            ? "border-status-running-border bg-status-running-bg text-status-running-bright"
                            : pill.db.label === "DB copied"
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
              deleting a site in rexenv never deletes your folder. Your Valet, Herd and Local setup
              is left exactly as it is, so you can go back at any time. Databases are COPIED, never
              moved — the old one is only read, and each site keeps using it until you switch it
              over on its Database tab.
            </div>
            <LeftoverDumpsCard />
          </div>
        )}
      </div>
      {/* The action bar — and the batch's progress — live OUTSIDE the scrolling
          list, so both are on screen wherever the list is scrolled to. The bar
          used to sit in the list's own header (ticking a row near the bottom
          meant scrolling back up to press Import), and the progress card at the
          top of the list (pressing Import at the bottom meant scrolling back up
          to see what it was doing) — owner, 12 Sep 2026, both. */}
      {!isLoading && (candidates.length > 0 || progress) && (
        <div className="flex flex-none flex-col gap-2.5 border-t border-rex-border bg-rex-surface-1 px-[18px] py-2.5">
          {progress && (
            <ImportProgressCard
              progress={progress}
              running={running}
              outcomes={outcomes}
              onCancel={() => void valetImportCancel().catch(toastBackendError)}
            />
          )}
          {candidates.length > 0 && (
        <div className="flex flex-wrap items-center gap-3">
          <span className="text-[0.75rem] text-rex-text-muted">
            {picked.size > 0
              ? `${picked.size} of ${ready.length} ready selected`
              : `${ready.length} ready to import`}
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
            {hasLocal && (
              <label
                className={cn(
                  "flex cursor-pointer items-center gap-1.5 text-[0.75rem] text-rex-text-muted",
                  !withDatabases && "opacity-50",
                )}
              >
                <input
                  type="checkbox"
                  className={CHECK_INPUT}
                  checked={withDatabases && connectLocal}
                  disabled={running || !withDatabases}
                  onChange={(e) => setConnectLocal(e.target.checked)}
                />
                also connect Local sites
                <span
                  className="cursor-help"
                  title="A Local site can't load under rexenv until it reads the copied database. This edits each Local site's wp-config to point at the copy — the file is backed up first, and the site's Database tab reverts it in one click. While connected, Local serves the site from rexenv's copy too. Valet and Herd sites are never connected here."
                >
                  ⓘ
                </span>
              </label>
            )}
            <Button
              variant="primary"
              disabled={picked.size === 0 || running}
              onClick={() => run.mutate()}
            >
              {running ? "Importing…" : `Import ${picked.size || ""}`.trim()}
            </Button>
          </div>
        </div>
          )}
        </div>
      )}
    </>
  );
}

/** Dumps kept by failed database imports. They contain a full copy of a
 *  database, so they are LISTED and deletable — never a file someone finds
 *  later. A successful import deletes its own dump. Lived in Settings → DNS &
 *  SSL until 12 Sep 2026; it belongs with the imports that leave it. */
function LeftoverDumpsCard() {
  const qc = useQueryClient();
  const { data: dumps = [] } = useQuery({
    queryKey: ["db-import-leftovers"],
    queryFn: dbImportLeftovers,
  });
  const remove = useMutation({
    mutationFn: (file: string) => dbImportDeleteLeftover(file),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["db-import-leftovers"] }),
    onError: (e) => toastBackendError(e),
  });
  if (dumps.length === 0) return null;
  const total = dumps.reduce((n, d) => n + d.sizeBytes, 0);
  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1 px-4 py-3">
      <div className="text-[0.78125rem] text-rex-text">
        Leftover database dumps: {dumps.length} file{dumps.length === 1 ? "" : "s"},{" "}
        {(total / (1024 * 1024)).toFixed(1)} MB
      </div>
      <div className="mt-0.5 text-[0.71875rem] text-rex-text-muted">
        Kept by database imports that didn't finish, so the copy stays diagnosable.
        Each contains a full copy of a database. Retrying an import replaces its file;
        delete them here when you're done with them.
      </div>
      <div className="mt-2 flex flex-col gap-1">
        {dumps.map((d) => (
          <div key={d.file} className="flex items-center justify-between gap-3">
            <span className="min-w-0 truncate font-mono text-[0.71875rem] text-rex-text-muted">
              {d.file} · {(d.sizeBytes / (1024 * 1024)).toFixed(1)} MB
            </span>
            <Button size="sm" variant="ghost" onClick={() => remove.mutate(d.file)}>
              Delete
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * The name field of a re-homed row whose picked name another rexenv site holds.
 * Only the LABEL is typed: the TLD is the row's own re-homed one, which the
 * policy already allows, so the field can't invent a TLD rexenv would refuse.
 * The problem shows once something is typed — an empty field is a question, not
 * an error.
 */
function NameChoice({
  row,
  label,
  problem,
  disabled,
  onChange,
}: {
  row: ImportCandidate;
  label: string;
  problem: string | null;
  disabled: boolean;
  onChange: (label: string) => void;
}) {
  const suggestion = `${(row.renamedFrom ?? row.domain).split(".")[0]}-local`;
  return (
    <div className="mt-1 flex flex-wrap items-center gap-1.5">
      <span className="text-[0.6875rem] text-rex-text-muted">import as</span>
      <input
        {...TECH_INPUT}
        aria-label={`Name to import ${row.renamedFrom ?? row.domain} under`}
        value={label}
        placeholder={suggestion}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        className="h-[24px] w-40 rounded border border-rex-border bg-rex-surface-2 px-1.5 font-mono text-[0.6875rem] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50"
      />
      <span className="font-mono text-[0.6875rem] text-rex-text-muted">.{tldOf(row.domain)}</span>
      {label.trim() !== "" && problem && (
        <span className="text-[0.6875rem] text-status-error-bright">{problem}</span>
      )}
    </div>
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
    p.stage === "site" || p.stage === "database" || p.stage === "connecting"
      ? `${p.stage === "database" ? "Copying the database for" : p.stage === "connecting" ? "Connecting" : "Importing"} ${p.domain ?? ""}`
      : p.stage === "scanning"
        ? "Reading your Valet, Herd and Local setup"
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
  reason,
}: {
  tld: ResolverTldStatus;
  onDone: () => void;
  alternative?: ReactNode;
  /** Replaces the "to serve these sites" sentence when there are no sites —
   *  a leftover file from an uninstalled Valet has nothing behind it. */
  reason?: ReactNode;
}) {
  const words = usePlatformWords();
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
              : (reason ??
                `${tld.path} tells ${words.osName} where to send .${tld.tld} lookups. To serve these sites, rexenv needs to answer them instead.`)}
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
