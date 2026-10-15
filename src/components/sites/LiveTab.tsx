/**
 * The Live tab (`docs/PLAN-wp-live-sync.md` §2.9): connect this WordPress site
 * to a live one with the key from the rexenv Sync plugin, then **Check status**
 * — up to date / live has changes (pull) / changes here (push) / both — Pull
 * (only what changed on live since the last sync; "everything again" on a
 * menu), or Push through a picker (tables changed here start ticked, live-owned
 * ones unticked) and the live host typed out. Rust holds both push gates (ledger
 * #834): the typed host is compared there, and a never-pulled site is refused
 * there; this component only keeps the button disabled until the typing
 * matches, so the refusal is rarely seen.
 *
 * The secret never reaches this component: pairing answers with the key id and
 * the site URL only, and every later call names the site.
 */
import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDownToLine, ArrowUpFromLine, ChevronDown, CloudDownload, CloudUpload, Download, History, Link2, Loader2, RefreshCw, Undo2, Unlink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { confirm } from "@/components/ui/dialog";
import {
  liveSyncActive,
  liveSyncHistory,
  liveSyncOptions,
  liveSyncPair,
  liveSyncPairing,
  liveSyncPull,
  liveSyncPush,
  liveSyncPluginZip,
  liveSyncPushPlan,
  liveSyncRollback,
  liveSyncSetOptions,
  liveSyncStatus,
  liveSyncUnpair,
  onLiveSyncState,
  revealPath,
  ipcInFlight,
  webviewNote,
} from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import { cn, TECH_INPUT } from "@/lib/utils";
import type { LiveSyncChange, LiveSyncHistoryEntry, LiveSyncJobState, LiveSyncOptions, LiveSyncPushPlan, LiveSyncStatus, Site } from "@/types";

const INPUT =
  "h-8 w-full rounded-md border border-rex-border bg-rex-well px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-brand";

const CARD = "rounded-lg border border-rex-border bg-rex-surface-1 p-4";

/** What a push is asked to do — a conflict retry repeats the job's own with overrides. */
interface PushAsk {
  tables: string[] | null;
  files: boolean;
}

export function LiveTab({ site }: { site: Site }) {
  const qc = useQueryClient();
  const pairing = useQuery({ queryKey: ["live-sync-pairing", site.id], queryFn: () => liveSyncPairing(site.id) });
  const history = useQuery({ queryKey: ["live-sync-history", site.id], queryFn: () => liveSyncHistory(site.id), enabled: !!pairing.data });
  const options = useQuery({ queryKey: ["live-sync-options", site.id], queryFn: () => liveSyncOptions(site.id), enabled: !!pairing.data });
  const setOptions = useMutation({
    mutationFn: (o: LiveSyncOptions) => liveSyncSetOptions(site.id, o.anonymise, o.uploadsOnDemand),
    onSuccess: (o) => qc.setQueryData(["live-sync-options", site.id], o),
    onError: toastBackendError,
  });
  const [job, setJob] = useState<LiveSyncJobState | null>(null);
  const [picker, setPicker] = useState<LiveSyncPushPlan | null>(null);
  const [sync, setSync] = useState<LiveSyncStatus | null>(null);
  useEffect(() => {
    let off = () => {};
    void liveSyncActive(site.id).then((j) => {
      setJob(j);
      if (j?.status === "running") void onLiveSyncState(j.id, setJob).then((f) => (off = f));
    });
    return () => off();
  }, [site.id]);

  const follow = async (j: LiveSyncJobState) => {
    setJob(j);
    // A sync changes what the status said: the old verdict must not stay on screen.
    setSync(null);
    const off = await onLiveSyncState(j.id, (s) => {
      setJob(s);
      if (s.status !== "running") {
        off();
        void qc.invalidateQueries({ queryKey: ["sites"] });
        void qc.invalidateQueries({ queryKey: ["live-sync-history", site.id] });
        if (s.status === "ok" && s.kind === "pull") toast.success(`Pulled ${plural(s.tables, "table")} and ${plural(s.files, "file")} from ${site.domain}'s live site`);
        if (s.status === "ok" && s.kind === "push") toast.success(`Pushed ${plural(s.tables, "table")} and ${plural(s.files, "file")} to the live site`);
      }
    });
  };
  // The old verdict goes the moment a new check starts: a card that still said "up to
  // date" while the spinner ran read as the answer (the macOS VM run, 14 Oct 2026).
  // Noted into the app log at both ends: the check's body is seconds, but the click
  // and its first line were up to a minute apart on the test machines (14 Oct 2026).
  const status = useMutation({
    mutationFn: async () => {
      const t = performance.now();
      webviewNote(`check status: clicked with in flight: ${ipcInFlight().join(", ") || "nothing"}`);
      try {
        return await liveSyncStatus(site.id);
      } finally {
        webviewNote(`check status: answered after ${((performance.now() - t) / 1000).toFixed(1)}s`);
      }
    },
    onMutate: () => setSync(null),
    onSuccess: setSync,
    onError: toastBackendError,
  });
  const pull = useMutation({
    mutationFn: (a: { uploadsSince?: number; full?: boolean }) => liveSyncPull(site.id, a.uploadsSince, a.full),
    onSuccess: follow,
    onError: toastBackendError,
  });
  const push = useMutation({
    mutationFn: (a: PushAsk & { confirmHost: string; overrideItems: string[]; skipItems?: string[] }) => liveSyncPush(site.id, a.confirmHost, a.tables, a.files, a.overrideItems, a.skipItems ?? []),
    onSuccess: (j) => {
      setPicker(null);
      void follow(j);
    },
    onError: toastBackendError,
  });
  const plan = useMutation({ mutationFn: () => liveSyncPushPlan(site.id), onSuccess: setPicker, onError: toastBackendError });
  const rollback = useMutation({
    mutationFn: (backupId: string) => liveSyncRollback(site.id, backupId),
    onSuccess: () => {
      toast.success("The live site is back to what it held before the push");
      setJob((j) => (j ? { ...j, backupId: null, lines: [...j.lines, "rolled back"] } : j));
      setSync(null);
      void qc.invalidateQueries({ queryKey: ["live-sync-history", site.id] });
    },
    onError: (e) => {
      // Gone on live already (rolled back elsewhere, or a newer push): the button
      // must not stay (#846). The backend forgets it too.
      if (String(e).includes("no longer on the live site")) setJob((j) => (j ? { ...j, backupId: null } : j));
      toastBackendError(e);
    },
  });
  const unpair = useMutation({
    mutationFn: () => liveSyncUnpair(site.id),
    onSuccess: () => {
      // The last job's card spoke about the pairing that just went (#854).
      setJob(null);
      setSync(null);
      void qc.invalidateQueries({ queryKey: ["live-sync-pairing", site.id] });
    },
    onError: toastBackendError,
  });

  if (pairing.isLoading) return null;
  if (!pairing.data) return <ConnectForm
        site={site}
        onPaired={() => {
          setJob(null);
          void qc.invalidateQueries({ queryKey: ["live-sync-pairing", site.id] });
        }}
      />;
  const p = pairing.data;
  const running = job?.status === "running";
  const busy = running || pull.isPending || push.isPending || plan.isPending || rollback.isPending || status.isPending;
  const last = history.data?.entries[0] ?? null;
  const liveHost = hostOf(p.siteUrl);
  const baseAt = history.data?.baseAt ?? null;

  const askPull = async (a: { uploadsSince?: number; full?: boolean }) => {
    const what = a.full
      ? "Every table and every wp-content file on the live site replaces this site's, whatever changed. The previous local database is kept one step back."
      : "Only the tables and files that changed on the live site since the last sync replace this site's. A table live did not touch keeps what it holds here. The previous local database is kept one step back.";
    const title = a.full ? `Pull everything from ${p.siteUrl} again?` : a.uploadsSince ? `Pull ${p.siteUrl}, recent uploads only?` : `Pull ${p.siteUrl} into ${site.domain}?`;
    const anon = options.data?.anonymise ? " Personal data (e-mails, names, addresses, IPs, form entries) is anonymised in the copy before it is swapped in." : "";
    const uod = a.uploadsSince && options.data?.uploadsOnDemand !== false ? " A missing upload is fetched from the live site the first time a page asks for it." : "";
    const message = a.uploadsSince ? `Uploads older than six months stay on the live site.${uod || " They will 404 here."} Everything else is pulled as usual.${anon} Nothing on the live site changes.` : `${what}${anon} Nothing on the live site changes.`;
    if (await confirm({ title, message, confirmLabel: "Pull" })) pull.mutate(a);
  };

  return (
    <div className="flex flex-col gap-4">
      <div className={CARD}>
        <div className="flex items-center gap-2 text-[0.8125rem] font-medium text-rex-text">
          <Link2 className="h-4 w-4 text-rex-text-muted" /> Connected to{" "}
          <span className="font-mono">{p.siteUrl}</span>
          <span className="rounded-full bg-rex-surface-2 px-1.5 py-0.5 font-mono text-[0.625rem] text-rex-text-muted">{p.keyId}</span>
          <span className="flex-1" />
          <Button
            size="sm"
            variant="ghost"
            disabled={busy || unpair.isPending}
            onClick={async () => {
              if (await confirm({ title: "Disconnect from the live site?", message: "This site keeps everything it has pulled. The live site's own key stays valid until you disconnect there too.", confirmLabel: "Disconnect" }))
                unpair.mutate();
            }}
          >
            <Unlink className="h-3.5 w-3.5" /> Disconnect
          </Button>
        </div>
        <div className="mt-1 text-[0.75rem] text-rex-text-muted" data-testid="live-last-sync">
          {last ? (
            <>
              Last sync: {last.kind === "pull" ? "pulled" : last.kind === "push" ? "pushed" : "rolled back"} {ago(last.at)}
              {last.status !== "ok" && <span className="text-status-warning-bright"> ({last.status})</span>}
            </>
          ) : baseAt ? (
            <>Last sync: {history.data?.baseKind === "push" ? "pushed" : "pulled"} {ago(baseAt)}</>
          ) : (
            "Never synced — pull once first so rexenv knows what the live site looks like."
          )}
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button variant="secondary" disabled={busy} onClick={() => status.mutate()} title="Ask the live site what changed there, and read what changed here — nothing is copied">
            {status.isPending ? <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> : <RefreshCw className="h-3.5 w-3.5" />} {status.isPending ? "Checking…" : "Check status"}
          </Button>
          <span className="flex-1" />
          <SplitButton
            label={
              <>
                {running && job?.kind === "pull" ? <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> : <CloudDownload className="h-3.5 w-3.5" />} Pull from live
              </>
            }
            disabled={busy}
            onClick={() => void askPull({})}
            title="Bring what changed on the live site since the last sync"
            items={[
              { label: "Pull everything again", hint: "every table and file, whatever changed", onClick: () => void askPull({ full: true }) },
              { label: "Pull, recent uploads only", hint: "media older than six months stays on live", onClick: () => void askPull({ uploadsSince: Math.floor(Date.now() / 1000) - 183 * 86400 }) },
            ]}
          />
          <Button variant="secondary" disabled={busy || !!picker} title="Send the tables and files changed here to the live site — after a picker and the live host typed out" onClick={() => plan.mutate()}>
            {plan.isPending || (running && job?.kind === "push") ? <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> : <CloudUpload className="h-3.5 w-3.5" />} Push to live…
          </Button>
        </div>
        {options.data && (
          <div className="mt-3 flex flex-col gap-1.5 border-t border-rex-border-subtle pt-3 text-[0.75rem] text-rex-text" data-testid="live-options">
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                className="mt-0.5"
                checked={options.data.anonymise}
                disabled={busy || setOptions.isPending}
                onChange={(e) => setOptions.mutate({ ...options.data!, anonymise: e.target.checked })}
                aria-label="Anonymise personal data on pull"
              />
              <span>
                Anonymise personal data on pull
                <span className="block text-[0.6875rem] text-rex-text-muted">
                  E-mails, names, addresses, IPs and form entries are scrubbed in the copy before it is swapped in — the live values never land on this machine. Tables it touches are never pushed back.
                </span>
              </span>
            </label>
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                className="mt-0.5"
                checked={options.data.uploadsOnDemand}
                disabled={busy || setOptions.isPending}
                onChange={(e) => setOptions.mutate({ ...options.data!, uploadsOnDemand: e.target.checked })}
                aria-label="Fetch missing uploads from live"
              />
              <span>
                Fetch missing uploads from live
                <span className="block text-[0.6875rem] text-rex-text-muted">
                  A media file this copy does not have is fetched from the live site the first time a page asks for it, saved here and served — so "recent uploads only" still renders every page.
                </span>
              </span>
            </label>
          </div>
        )}
      </div>
      {sync && !running && (
        <StatusCard
          sync={sync}
          busy={busy}
          onPull={() => void askPull({})}
          onPush={() => plan.mutate()}
        />
      )}
      {picker && (
        <PushPicker
          site={site}
          plan={picker}
          busy={push.isPending}
          onCancel={() => setPicker(null)}
          onPush={(tables, files, confirmHost) => push.mutate({ tables, files, confirmHost, overrideItems: [] })}
        />
      )}
      {job && (
        <JobCard
          job={job}
          liveHost={picker?.liveHost ?? liveHost}
          busy={busy}
          onChoose={async (overwrite, keep) => {
            if (!job) return;
            if (
              await confirm({
                title: overwrite.length > 0 ? `Overwrite ${overwrite.length} item(s) on ${liveHost}?` : `Push without the ${keep.length} item(s) live changed?`,
                // A list, not "\n"-joined text: the dialog does not keep newlines, and the
                // names ran into one line with the next sentence ("wpcs_terms The live site").
                message: (
                  <>
                    {overwrite.length > 0 && (
                      <>
                        These changed on the live site after your last sync. Pushing replaces them with this site's version:
                        <ul className="my-2 font-mono text-[0.75rem]">
                          {overwrite.map((i) => (
                            <li key={i}>{i}</li>
                          ))}
                        </ul>
                      </>
                    )}
                    {keep.length > 0 && (
                      <>
                        Left as the live site has them (not sent):
                        <ul className="my-2 font-mono text-[0.75rem]">
                          {keep.map((i) => (
                            <li key={i}>{i}</li>
                          ))}
                        </ul>
                      </>
                    )}
                    The live site keeps a backup you can roll back to.
                  </>
                ),
                confirmLabel: overwrite.length > 0 ? "Push anyway" : "Push",
                danger: overwrite.length > 0,
              })
            )
              push.mutate({ tables: job.askedTables, files: job.askedFiles, confirmHost: liveHost, overrideItems: overwrite, skipItems: keep });
          }}
          onRollback={async (backupId) => {
            if (
              await confirm({
                title: `Roll the live site back to ${backupId}?`,
                message: "The tables and files this push replaced come back on the live site exactly as they were. What the push sent is removed from the live site.",
                confirmLabel: "Roll back",
                danger: true,
              })
            )
              rollback.mutate(backupId);
          }}
        />
      )}
      {history.data && history.data.entries.length > 0 && <HistoryCard entries={history.data.entries} />}
    </div>
  );
}

/** "1 file", "2 files" — the card's counts read as sentences. */
function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** "3 minutes ago" / "2 days ago" from Unix seconds. */
function ago(at: number): string {
  const s = Math.max(0, Math.floor(Date.now() / 1000) - at);
  if (s < 60) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${plural(m, "minute")} ago`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${plural(h, "hour")} ago`;
  return `${plural(Math.floor(h / 24), "day")} ago`;
}

/** The host of an `https://…` URL — a URL, not a path (the path guard watches `split("/")`). */
function hostOf(url: string): string {
  const m = /^https?:\/\/([^/]+)/.exec(url);
  return m?.[1] ?? url;
}

/** A primary action with a small menu of its variants (the pull's "everything again"
 *  and "recent uploads only"). Closes on a pick, on Escape, and on a click elsewhere. */
function SplitButton({
  label,
  title,
  disabled,
  onClick,
  items,
}: {
  label: React.ReactNode;
  title?: string;
  disabled: boolean;
  onClick: () => void;
  items: { label: string; hint: string; onClick: () => void }[];
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <div className="relative flex" ref={ref}>
      <Button variant="primary" disabled={disabled} onClick={onClick} title={title} className="rounded-r-none">
        {label}
      </Button>
      <Button variant="primary" disabled={disabled} onClick={() => setOpen((o) => !o)} aria-label="More ways to pull" aria-expanded={open} className="rounded-l-none border-l border-l-rex-bg/30 px-1.5">
        <ChevronDown className="h-3.5 w-3.5" />
      </Button>
      {open && (
        <div role="menu" className="absolute right-0 top-full z-20 mt-1 min-w-[16rem] rounded-md border border-rex-border bg-rex-surface-1 p-1 shadow-lg">
          {items.map((it) => (
            <button
              key={it.label}
              type="button"
              role="menuitem"
              className="flex w-full flex-col items-start rounded px-2 py-1.5 text-left hover:bg-rex-surface-2"
              onClick={() => {
                setOpen(false);
                it.onClick();
              }}
            >
              <span className="text-[0.78125rem] text-rex-text">{it.label}</span>
              <span className="text-[0.6875rem] text-rex-text-muted">{it.hint}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** The verdict, in a sentence a person acts on, and what moved on each side. */
function StatusCard({ sync, busy, onPull, onPush }: { sync: LiveSyncStatus; busy: boolean; onPull: () => void; onPush: () => void }) {
  const [detail, setDetail] = useState(false);
  const liveFiles = sync.files.liveChanged.length + sync.files.liveGone.length;
  const localFiles = sync.files.localChanged.length;
  // Changed, new or gone on either side — "unknown" (a base from before the local
  // stamps existed) is not news, and listed every table on the macOS VM (14 Oct 2026).
  const isMoved = (c: LiveSyncChange) => c === "changed" || c === "new" || c === "gone";
  const moved = sync.tables.filter((t) => isMoved(t.live) || isMoved(t.local));
  const localUnknown = sync.verdict !== "never_synced" && sync.tables.length > 0 && sync.tables.every((t) => t.local === "unknown");
  const tone = { never_synced: "warning", up_to_date: "running", behind: "warning", ahead: "running", diverged: "error" }[sync.verdict] as "warning" | "running" | "error";
  const frame = {
    running: "border-status-running-border bg-status-running-bg/40",
    warning: "border-status-warning-border bg-status-warning-bg/40",
    error: "border-status-error-border bg-status-error-bg/40",
  }[tone];
  const dot = { running: "bg-status-running-bright", warning: "bg-status-warning-bright", error: "bg-status-error-bright" }[tone];
  const headline = {
    never_synced: "Never synced — pull once first.",
    up_to_date: "Up to date — nothing to pull or push.",
    behind: `Live has changes you do not have here — ${plural(sync.tablesBehind, "table")}, ${plural(liveFiles, "file")}. Pull to bring them.`,
    ahead: `This site is ahead of live — ${plural(sync.tablesAhead, "table")}, ${plural(localFiles, "file")} changed here. Push to send them.`,
    diverged: `Both sides changed — live: ${plural(sync.tablesBehind, "table")}, ${plural(liveFiles, "file")}; here: ${plural(sync.tablesAhead, "table")}, ${plural(localFiles, "file")}.`,
  }[sync.verdict];
  const advice = {
    never_synced: "A pull records what both sides look like; after it, this card can say which side moved.",
    up_to_date: `Checked against ${sync.liveHost} ${ago(sync.checkedAt)}.`,
    behind: "A pull replaces only the tables and files that changed on live; what you changed here in other tables stays.",
    ahead: "A push sends the tables changed here (live-owned ones such as users and orders are left out unless you tick them) and the files changed here.",
    diverged: "Pull first: a table both sides changed is replaced by live's (the previous local tables are kept one step back), and a file both sides changed is overwritten here. Then push what is yours.",
  }[sync.verdict];
  return (
    <div className={cn("rounded-lg border p-4", frame)} data-testid="live-status">
      <div className="flex items-center gap-2 text-[0.8125rem] font-medium text-rex-text">
        <span className={cn("inline-block h-2.5 w-2.5 rounded-full", dot)} />
        <span data-testid="live-verdict">{headline}</span>
        <span className="flex-1" />
        {(sync.verdict === "behind" || sync.verdict === "diverged" || sync.verdict === "never_synced") && (
          <Button size="sm" variant="primary" disabled={busy} onClick={onPull}>
            <ArrowDownToLine className="h-3.5 w-3.5" /> Pull
          </Button>
        )}
        {sync.verdict === "ahead" && (
          <Button size="sm" variant="primary" disabled={busy} onClick={onPush}>
            <ArrowUpFromLine className="h-3.5 w-3.5" /> Push…
          </Button>
        )}
      </div>
      <p className="mt-1 text-[0.75rem] leading-[1.5] text-rex-text-muted">{advice}</p>
      {localUnknown && (
        <p className="mt-1 text-[0.71875rem] text-rex-text-muted">
          What changed here cannot be told yet: this site was last synced before rexenv kept local stamps. The next pull or push records them.
        </p>
      )}
      {sync.tablesAheadLiveOwned > 0 && sync.verdict !== "never_synced" && (
        <p className="mt-1 text-[0.71875rem] text-rex-text-muted">
          {plural(sync.tablesAheadLiveOwned, "live-owned table")} changed here (users, comments, orders…) — not counted: a push leaves them out unless you tick them.
        </p>
      )}
      {(moved.length > 0 || liveFiles > 0 || localFiles > 0) && sync.verdict !== "never_synced" && (
        <>
          <button type="button" className="mt-2 text-[0.71875rem] text-rex-accent-blue hover:underline" onClick={() => setDetail((d) => !d)}>
            {detail ? "Hide what moved" : "Show what moved"}
          </button>
          {detail && (
            <div className="mt-2 grid gap-3 text-[0.71875rem] sm:grid-cols-2" data-testid="live-status-detail">
              <div>
                <div className="mb-1 font-medium text-rex-text">Tables</div>
                {moved.length === 0 ? (
                  <div className="text-rex-text-muted">none</div>
                ) : (
                  <table className="w-full">
                    <thead className="text-rex-text-muted">
                      <tr>
                        <th className="text-left font-normal">table</th>
                        <th className="text-left font-normal">on live</th>
                        <th className="text-left font-normal">here</th>
                      </tr>
                    </thead>
                    <tbody>
                      {moved.map((t) => (
                        <tr key={t.name}>
                          <td className="truncate pr-2 font-mono text-rex-text">
                            {t.name}
                            {t.liveOwned && <span className="ml-1 text-rex-text-muted">(live writes here)</span>}
                          </td>
                          <td className="pr-2">{changeWord(t.live)}</td>
                          <td>{changeWord(t.local)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
              <div>
                <div className="mb-1 font-medium text-rex-text">Files (wp-content)</div>
                <FileList label="changed on live" items={sync.files.liveChanged} />
                <FileList label="gone from live" items={sync.files.liveGone} />
                <FileList label="changed here" items={sync.files.localChanged} />
                <FileList label="changed on both sides" items={sync.files.both} />
                {liveFiles === 0 && localFiles === 0 && <div className="text-rex-text-muted">none</div>}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function changeWord(c: LiveSyncChange): React.ReactNode {
  const cls = { same: "text-rex-text-muted", changed: "text-status-warning-bright", new: "text-status-warning-bright", gone: "text-status-error-bright", unknown: "text-rex-text-muted" }[c];
  const word = { same: "—", changed: "changed", new: "new", gone: "gone", unknown: "unknown" }[c];
  return <span className={cls}>{word}</span>;
}

function FileList({ label, items }: { label: string; items: string[] }) {
  if (items.length === 0) return null;
  return (
    <div className="mb-1">
      <div className="text-rex-text-muted">{label}: {items.length}</div>
      <ul className="max-h-24 overflow-auto font-mono text-rex-text">
        {items.slice(0, 20).map((f) => (
          <li key={f} className="truncate">
            {f}
          </li>
        ))}
        {items.length > 20 && <li className="text-rex-text-muted">… and {items.length - 20} more</li>}
      </ul>
    </div>
  );
}

function HistoryCard({ entries }: { entries: LiveSyncHistoryEntry[] }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={CARD} data-testid="live-history">
      <button type="button" className="flex w-full items-center gap-2 text-left text-[0.8125rem] font-medium text-rex-text" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        <History className="h-4 w-4 text-rex-text-muted" /> History
        <span className="text-[0.75rem] font-normal text-rex-text-muted">({plural(entries.length, "sync")})</span>
        <span className="flex-1" />
        <ChevronDown className={cn("h-3.5 w-3.5 text-rex-text-muted transition-transform", open && "rotate-180")} />
      </button>
      {open && (
        <ul className="mt-2 flex flex-col gap-1 text-[0.75rem]">
          {entries.map((e) => (
            <li key={`${e.at}-${e.kind}`} className="flex flex-wrap items-baseline gap-x-2">
              <span className="w-40 shrink-0 whitespace-nowrap font-mono text-rex-text-muted">{new Date(e.at * 1000).toLocaleString()}</span>
              <span className={cn("font-medium", e.status === "ok" ? "text-rex-text" : "text-status-error-bright")}>
                {e.kind === "pull" ? "Pulled" : e.kind === "push" ? "Pushed" : "Rolled back"}
                {e.status !== "ok" && ` (${e.status})`}
              </span>
              {e.kind !== "rollback" && e.status === "ok" && (
                <span className="text-rex-text-muted">
                  {plural(e.tables, "table")}
                  {e.kind === "pull" && e.rows > 0 && ` (${plural(e.rows, "row")})`}, {plural(e.files, "file")}
                </span>
              )}
              {e.backupId && <span className="font-mono text-rex-text-muted">{e.backupId.slice(0, 8)}…</span>}
              {e.error && <span className="text-rex-text-muted">— {e.error}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PushPicker({
  site,
  plan,
  busy,
  onCancel,
  onPush,
}: {
  site: Site;
  plan: LiveSyncPushPlan;
  busy: boolean;
  onCancel: () => void;
  /** Always the explicit list: a `null` would mean "all but the live-owned" to Rust,
   *  and a person who ticked wp_users meant wp_users. */
  onPush: (tables: string[], files: boolean, confirmHost: string) => void;
}) {
  // Ticked by default: changed here (or unknown) and not live-owned.
  const [picked, setPicked] = useState<Set<string>>(() => new Set(plan.tables.filter((t) => !t.liveOwned && !t.anonymised && t.changedHere !== false).map((t) => t.name)));
  const [files, setFiles] = useState(true);
  const [typed, setTyped] = useState("");
  const matches = typed.trim().toLowerCase() === plan.liveHost.toLowerCase();
  const nothing = picked.size === 0 && !files;
  const unchanged = plan.tables.filter((t) => t.changedHere === false && !t.liveOwned).length;
  const toggle = (name: string) =>
    setPicked((s) => {
      const n = new Set(s);
      if (n.has(name)) n.delete(name);
      else n.add(name);
      return n;
    });
  return (
    <div className={CARD} data-testid="push-picker">
      <div className="text-[0.8125rem] font-medium text-rex-text">
        Push {site.domain} to <span className="font-mono">{plan.liveHost}</span>
      </div>
      <p className="mt-1 text-[0.78125rem] leading-[1.55] text-rex-text-muted">
        Each ticked table replaces the live one; files changed here since the last sync are sent. Tables changed here start
        ticked{unchanged > 0 && <> ({plural(unchanged, "unchanged table")} start unticked)</>}. The live site keeps a backup of
        everything replaced. Anything that changed on the live site since{" "}
        {plan.baseAt ? <span className="font-mono">{new Date(plan.baseAt * 1000).toLocaleString()}</span> : "the last sync"} stops
        the push first, so nothing is overwritten unseen.
      </p>
      <div className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1 sm:grid-cols-3">
        {plan.tables.map((t) => (
          <label key={t.name} className="flex items-center gap-2 text-[0.75rem] text-rex-text">
            <input type="checkbox" checked={picked.has(t.name)} onChange={() => toggle(t.name)} disabled={busy || t.anonymised} aria-label={t.name} />
            <span className={cn("truncate font-mono", (t.changedHere === false || t.anonymised) && "text-rex-text-muted")}>{t.name}</span>
            {t.anonymised && (
              <span className="rounded-full bg-rex-surface-2 px-1.5 py-0.5 text-[0.625rem] text-rex-text-muted" title="The pull anonymised this table's personal data here — the scrubbed rows are never pushed back. Turn anonymising off and pull again to push it.">
                anonymised — never pushed
              </span>
            )}
            {t.liveOwned && (
              <span className="rounded-full bg-rex-surface-2 px-1.5 py-0.5 text-[0.625rem] text-rex-text-muted" title="The live site writes here (users, comments, orders, entries) — what it wrote since your last pull would be lost">
                live writes here
              </span>
            )}
            {!t.liveOwned && t.changedHere === true && (
              <span className="rounded-full bg-status-warning-bg px-1.5 py-0.5 text-[0.625rem] text-status-warning-bright" title="Its contents moved here since the last sync">
                changed here
              </span>
            )}
            {!t.liveOwned && t.changedHere === false && (
              <span className="rounded-full bg-rex-surface-2 px-1.5 py-0.5 text-[0.625rem] text-rex-text-muted" title="Nothing here changed since the last sync — sending it would only repeat what live has">
                unchanged
              </span>
            )}
          </label>
        ))}
      </div>
      <label className="mt-3 flex items-center gap-2 text-[0.78125rem] text-rex-text">
        <input type="checkbox" checked={files} onChange={(e) => setFiles(e.target.checked)} disabled={busy} /> Files changed here since the last sync (themes, plugins, uploads)
      </label>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <span className="text-[0.78125rem] text-rex-text-muted">
          Type <span className="font-mono text-rex-text">{plan.liveHost}</span> to push:
        </span>
        <input {...TECH_INPUT} value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={plan.liveHost} className={cn(INPUT, "max-w-[18rem]")} aria-label="Live host to confirm" disabled={busy} />
        <span className="flex-1" />
        <Button variant="ghost" size="sm" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!matches || nothing || busy} onClick={() => onPush([...picked], files, typed)}>
          {busy ? <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> : <CloudUpload className="h-3.5 w-3.5" />} Push
        </Button>
      </div>
    </div>
  );
}

function JobCard({
  job,
  liveHost,
  busy,
  onChoose,
  onRollback,
}: {
  job: LiveSyncJobState;
  liveHost: string;
  busy: boolean;
  /** The per-item choice on a conflict stop: what to overwrite on live, what to keep as live has it. */
  onChoose: (overwrite: string[], keep: string[]) => void;
  onRollback: (backupId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  // Per item, live's version is kept unless the person picks "overwrite" — the safe default.
  const [overwrite, setOverwrite] = useState<Set<string>>(() => new Set());
  const bad = job.status === "failed" || job.status === "conflicts";
  return (
    <div className={cn("rounded-lg border p-4", bad ? "border-status-error-border bg-status-error-bg/40" : "border-rex-border bg-rex-surface-1")}>
      <div className="flex items-center gap-2 text-[0.8125rem] text-rex-text">
        {job.status === "running" && <Loader2 className="h-3.5 w-3.5 animate-rex-spin text-rex-text-muted" />}
        {job.status === "running" && <span>{job.kind === "push" ? "Pushing…" : "Pulling…"}</span>}
        {job.status === "ok" && job.kind === "pull" && (
          <span>
            Pulled {plural(job.tables, "table")} ({plural(job.rows, "row")}) and {plural(job.files, "file")}
            {job.tablesKept + job.filesKept > 0 && (
              <>
                {" "}— {plural(job.tablesKept, "table")} and {plural(job.filesKept, "file")} unchanged on live were kept as they are here
              </>
            )}
            {job.tablesResumed + job.filesResumed > 0 && (
              <>
                ; {plural(job.tablesResumed, "table")} and {plural(job.filesResumed, "file")} the interrupted pull had already copied were reused
              </>
            )}
            {job.filesDeleted > 0 && <>; {plural(job.filesDeleted, "file")} gone from live removed here</>}
            {job.anonymised.length > 0 && <>; personal data anonymised in {plural(job.anonymised.length, "table")}</>}.
            {job.backupDb && (
              <>
                {" "}The previous local tables are in <span className="font-mono">{job.backupDb}</span>.
              </>
            )}
          </span>
        )}
        {job.status === "ok" && job.kind === "push" && (
          <span>
            Pushed {plural(job.tables, "table")} and {plural(job.files, "file")} to <span className="font-mono">{liveHost}</span>
            {job.tablesKept > 0 && <> ({plural(job.tablesKept, "unchanged table")} left out)</>}
            {job.tablesResumed + job.filesResumed > 0 && <> — {plural(job.tablesResumed, "table")} and {plural(job.filesResumed, "file")} the interrupted push had already sent were not sent again</>}.
            {job.backupId ? (
              <>
                {" "}The live site keeps what they replaced as <span className="font-mono">{job.backupId}</span>.
              </>
            ) : (
              " Rolled back."
            )}
          </span>
        )}
        {job.status === "conflicts" && (
          <span>
            Nothing was sent: {job.conflicts.length} item(s) changed on <span className="font-mono">{liveHost}</span> since your last sync.
          </span>
        )}
        {job.status === "failed" && <span className="whitespace-pre-line text-status-error-bright">{job.error}</span>}
        <span className="flex-1" />
        {job.status === "ok" && job.kind === "push" && job.backupId && (
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => onRollback(job.backupId as string)}>
            <Undo2 className="h-3.5 w-3.5" /> Roll back
          </Button>
        )}
      </div>
      {job.status === "conflicts" && (
        <div className="mt-2" data-testid="live-conflicts">
          <div className="text-[0.71875rem] text-rex-text-muted">For each one, choose: keep what the live site has (it is left out of the push) or overwrite it with this site's version.</div>
          <table className="mt-1 text-[0.71875rem]">
            <tbody>
              {job.conflicts.map((c) => (
                <tr key={c}>
                  <td className="pr-3 font-mono text-rex-text">{c}</td>
                  <td className="pr-3">
                    <label className="flex items-center gap-1">
                      <input type="radio" name={`conflict-${c}`} checked={!overwrite.has(c)} disabled={busy} onChange={() => setOverwrite((s) => { const n = new Set(s); n.delete(c); return n; })} aria-label={`Keep live's ${c}`} /> keep live's
                    </label>
                  </td>
                  <td>
                    <label className="flex items-center gap-1">
                      <input type="radio" name={`conflict-${c}`} checked={overwrite.has(c)} disabled={busy} onChange={() => setOverwrite((s) => new Set(s).add(c))} aria-label={`Overwrite ${c}`} /> overwrite
                    </label>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="mt-2 flex flex-wrap gap-2">
            <Button size="sm" variant="secondary" disabled={busy} onClick={() => onChoose(job.conflicts.filter((c) => overwrite.has(c)), job.conflicts.filter((c) => !overwrite.has(c)))}>
              Push with these choices
            </Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => onChoose(job.conflicts, [])}>
              Push anyway, overwriting all
            </Button>
          </div>
          <div className="mt-1 text-[0.71875rem] text-rex-text-muted">Or pull first to bring the live changes here, then push again.</div>
        </div>
      )}
      {job.filesKeptEdited.length > 0 && (
        <div className="mt-1 text-[0.71875rem] text-rex-text-muted">
          {plural(job.filesKeptEdited.length, "file")} gone from live but edited here since the last sync — kept: <span className="font-mono">{job.filesKeptEdited.join(", ")}</span>
        </div>
      )}
      {job.refusedFiles.length > 0 && (
        <div className="mt-1 text-[0.71875rem] text-rex-text-muted">{job.refusedFiles.length} file(s) the live site would not serve were left out.</div>
      )}
      {job.lines.length > 0 && (
        <button type="button" className="mt-2 text-[0.71875rem] text-rex-accent-blue hover:underline" onClick={() => setOpen((o) => !o)}>
          {open ? "Hide log" : "Show log"}
        </button>
      )}
      {open && <JobLog lines={job.lines} />}
    </div>
  );
}

/** The job's log, kept scrolled to its END: the last lines are the verdict ("files: N in place",
 *  "uploads older than … left on live", "swapped in; … backup …"), and in a box that started at the
 *  top they sat below the fold — the macOS VM run (11 Oct 2026) read the recent-uploads line as
 *  missing. Follows new lines unless the reader has scrolled up. */
function JobLog({ lines }: { lines: string[] }) {
  const ref = useRef<HTMLPreElement>(null);
  const pinned = useRef(true);
  useEffect(() => {
    const el = ref.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [lines.length]);
  return (
    <pre
      ref={ref}
      data-testid="live-job-log"
      onScroll={(e) => {
        const el = e.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
      }}
      className="mt-2 max-h-64 overflow-auto rounded-md bg-rex-well p-2 font-mono text-[0.6875rem] text-rex-text-muted"
    >
      {lines.join("\n")}
    </pre>
  );
}

/** "Download plugin": the rexenv Sync plugin this build speaks to, saved to
 *  Downloads as the zip WordPress's uploader takes (plan §11 Q1: in-app only). */
export function DownloadPluginButton() {
  const save = useMutation({
    mutationFn: liveSyncPluginZip,
    onSuccess: (path) => toast.success(`Saved ${path} — upload it on the live site: Plugins → Add New → Upload Plugin`, { label: "Show", onClick: () => void revealPath(path) }),
    onError: toastBackendError,
  });
  return (
    <Button size="sm" variant="secondary" disabled={save.isPending} onClick={() => save.mutate()}>
      {save.isPending ? <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> : <Download className="h-3.5 w-3.5" />} Download plugin (.zip)
    </Button>
  );
}

function ConnectForm({ site, onPaired }: { site: Site; onPaired: () => void }) {
  const [key, setKey] = useState("");
  const [user, setUser] = useState("");
  const [password, setPassword] = useState("");
  const [showAuth, setShowAuth] = useState(false);
  const pair = useMutation({
    mutationFn: () => liveSyncPair(site.id, key.trim(), user.trim() || undefined, password || undefined),
    onSuccess: (r) => {
      toast.success(`Connected to ${r.pairing.siteUrl} — WordPress ${r.wp}, ${r.tables} tables`);
      onPaired();
    },
    onError: toastBackendError,
  });
  return (
    <div className={CARD}>
      <div className="text-[0.8125rem] font-medium text-rex-text">Connect this site to a live one</div>
      <ol className="mt-2 list-decimal pl-5 text-[0.78125rem] leading-[1.55] text-rex-text-muted">
        <li>
          Install the <span className="font-mono">rexenv Sync</span> plugin on the live site (the zip below: Plugins → Add New → Upload Plugin) and activate it.
        </li>
        <li>
          There, open Tools → rexenv Sync and press <em>Connect to rexenv</em>. It shows a key once — Copy key.
        </li>
        <li>Paste the key here and press Connect. rexenv checks it against the live site before keeping it; a pull never writes to the live site.</li>
      </ol>
      <div className="mt-2">
        <DownloadPluginButton />
      </div>
      <textarea
        {...TECH_INPUT}
        value={key}
        onChange={(e) => setKey(e.target.value)}
        placeholder="rexsync1:…"
        rows={3}
        disabled={pair.isPending}
        className="mt-3 w-full rounded-md border border-rex-border bg-rex-well px-2.5 py-1.5 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
        aria-label="Connection key"
      />
      <button type="button" className="mt-2 text-[0.71875rem] text-rex-accent-blue hover:underline" onClick={() => setShowAuth((s) => !s)}>
        {showAuth ? "No HTTP password" : "The live site is behind an HTTP password…"}
      </button>
      {showAuth && (
        <div className="mt-2 flex gap-2">
          <input {...TECH_INPUT} value={user} onChange={(e) => setUser(e.target.value)} placeholder="user" className={INPUT} aria-label="HTTP auth user" />
          <input {...TECH_INPUT} type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder="password" className={INPUT} aria-label="HTTP auth password" />
        </div>
      )}
      <div className="mt-3 flex justify-end">
        <Button variant="primary" disabled={!key.trim().startsWith("rexsync1:") || pair.isPending} onClick={() => pair.mutate()}>
          {pair.isPending && <Loader2 className="h-3.5 w-3.5 animate-rex-spin" />} Connect
        </Button>
      </div>
    </div>
  );
}
