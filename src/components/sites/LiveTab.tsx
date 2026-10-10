/**
 * The Live tab (`docs/PLAN-wp-live-sync.md` §2.9): connect this WordPress site
 * to a live one with the key from the rexenv Sync plugin, then Pull — or Push,
 * through a picker (live-owned tables start unticked) and the live host typed
 * out. Rust holds both push gates (ledger #834): the typed host is compared
 * there, and a never-pulled site is refused there; this component only keeps
 * the button disabled until the typing matches, so the refusal is rarely seen.
 *
 * The secret never reaches this component: pairing answers with the key id and
 * the site URL only, and every later call names the site.
 */
import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CloudDownload, CloudUpload, Download, Link2, Loader2, Undo2, Unlink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { confirm } from "@/components/ui/dialog";
import {
  liveSyncActive,
  liveSyncPair,
  liveSyncPairing,
  liveSyncPull,
  liveSyncPush,
  liveSyncPluginZip,
  liveSyncPushPlan,
  liveSyncRollback,
  liveSyncUnpair,
  onLiveSyncState,
  revealPath,
} from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import { cn, TECH_INPUT } from "@/lib/utils";
import type { LiveSyncJobState, LiveSyncPushPlan, Site } from "@/types";

const INPUT =
  "h-8 w-full rounded-md border border-rex-border bg-rex-well px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-brand";

/** What a push is asked to do — a conflict retry repeats the job's own with overrides. */
interface PushAsk {
  tables: string[] | null;
  files: boolean;
}

export function LiveTab({ site }: { site: Site }) {
  const qc = useQueryClient();
  const pairing = useQuery({ queryKey: ["live-sync-pairing", site.id], queryFn: () => liveSyncPairing(site.id) });
  const [job, setJob] = useState<LiveSyncJobState | null>(null);
  const [picker, setPicker] = useState<LiveSyncPushPlan | null>(null);
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
    const off = await onLiveSyncState(j.id, (s) => {
      setJob(s);
      if (s.status !== "running") {
        off();
        void qc.invalidateQueries({ queryKey: ["sites"] });
        if (s.status === "ok" && s.kind === "pull") toast.success(`Pulled ${plural(s.tables, "table")} and ${plural(s.files, "file")} from ${site.domain}'s live site`);
        if (s.status === "ok" && s.kind === "push") toast.success(`Pushed ${plural(s.tables, "table")} and ${plural(s.files, "file")} to the live site`);
      }
    });
  };
  const pull = useMutation({
    mutationFn: (uploadsSince?: number) => liveSyncPull(site.id, uploadsSince),
    onSuccess: follow,
    onError: toastBackendError,
  });
  const push = useMutation({
    mutationFn: (a: PushAsk & { confirmHost: string; overrideItems: string[] }) => liveSyncPush(site.id, a.confirmHost, a.tables, a.files, a.overrideItems),
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
  const busy = running || pull.isPending || push.isPending || plan.isPending || rollback.isPending;

  return (
    <div className="flex flex-col gap-4">
      <div className="rounded-lg border border-rex-border bg-rex-surface-1 p-4">
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
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button
            variant="primary"
            disabled={busy}
            onClick={async () => {
              if (
                await confirm({
                  title: `Pull ${p.siteUrl} into ${site.domain}?`,
                  message: "The live site's database and wp-content files replace this site's. The previous local database is kept one step back. Nothing on the live site changes.",
                  confirmLabel: "Pull",
                })
              )
                pull.mutate(undefined);
            }}
          >
            {running && job?.kind === "pull" ? <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> : <CloudDownload className="h-3.5 w-3.5" />} Pull from live
          </Button>
          <Button
            variant="secondary"
            disabled={busy}
            title="Media older than six months stays on the live site — a big library pulls faster"
            onClick={async () => {
              if (await confirm({ title: `Pull ${p.siteUrl}, recent uploads only?`, message: "Uploads older than six months stay on the live site and will 404 here. Everything else is pulled as usual.", confirmLabel: "Pull" }))
                pull.mutate(Math.floor(Date.now() / 1000) - 183 * 86400);
            }}
          >
            Pull, recent uploads only
          </Button>
          <span className="flex-1" />
          <Button variant="secondary" disabled={busy || !!picker} title="Send this site's tables and changed files to the live site — after a picker and the live host typed out" onClick={() => plan.mutate()}>
            {plan.isPending || (running && job?.kind === "push") ? <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> : <CloudUpload className="h-3.5 w-3.5" />} Push to live…
          </Button>
        </div>
      </div>
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
          liveHost={picker?.liveHost ?? hostOf(p.siteUrl)}
          busy={busy}
          onOverride={async (items) => {
            if (!job) return;
            if (
              await confirm({
                title: `Overwrite ${items.length} item(s) on ${hostOf(p.siteUrl)}?`,
                message: `These changed on the live site after your last sync. Pushing anyway replaces them with this site's version:\n\n${items.join("\n")}\n\nThe live site keeps a backup you can roll back to.`,
                confirmLabel: "Push anyway",
                danger: true,
              })
            )
              push.mutate({ tables: job.askedTables, files: job.askedFiles, confirmHost: hostOf(p.siteUrl), overrideItems: items });
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
    </div>
  );
}

/** The host of an `https://…` URL — a URL, not a path (the path guard watches `split("/")`). */
/** "1 file", "2 files" — the card's counts read as sentences. */
function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

function hostOf(url: string): string {
  const m = /^https?:\/\/([^/]+)/.exec(url);
  return m?.[1] ?? url;
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
  const [picked, setPicked] = useState<Set<string>>(() => new Set(plan.tables.filter((t) => !t.liveOwned).map((t) => t.name)));
  const [files, setFiles] = useState(true);
  const [typed, setTyped] = useState("");
  const matches = typed.trim().toLowerCase() === plan.liveHost.toLowerCase();
  const nothing = picked.size === 0 && !files;
  const toggle = (name: string) =>
    setPicked((s) => {
      const n = new Set(s);
      if (n.has(name)) n.delete(name);
      else n.add(name);
      return n;
    });
  return (
    <div className="rounded-lg border border-rex-border bg-rex-surface-1 p-4" data-testid="push-picker">
      <div className="text-[0.8125rem] font-medium text-rex-text">
        Push {site.domain} to <span className="font-mono">{plan.liveHost}</span>
      </div>
      <p className="mt-1 text-[0.78125rem] leading-[1.55] text-rex-text-muted">
        Each ticked table replaces the live one; files changed here since the last sync are sent. The live site keeps a
        backup of everything replaced. Anything that changed on the live site since{" "}
        {plan.baseAt ? <span className="font-mono">{new Date(plan.baseAt * 1000).toLocaleString()}</span> : "the last sync"} stops
        the push first, so nothing is overwritten unseen.
      </p>
      <div className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1 sm:grid-cols-3">
        {plan.tables.map((t) => (
          <label key={t.name} className="flex items-center gap-2 text-[0.75rem] text-rex-text">
            <input type="checkbox" checked={picked.has(t.name)} onChange={() => toggle(t.name)} disabled={busy} aria-label={t.name} />
            <span className="truncate font-mono">{t.name}</span>
            {t.liveOwned && (
              <span className="rounded-full bg-rex-surface-2 px-1.5 py-0.5 text-[0.625rem] text-rex-text-muted" title="The live site writes here (users, comments, orders, entries) — what it wrote since your last pull would be lost">
                live writes here
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
  onOverride,
  onRollback,
}: {
  job: LiveSyncJobState;
  liveHost: string;
  busy: boolean;
  onOverride: (items: string[]) => void;
  onRollback: (backupId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const bad = job.status === "failed" || job.status === "conflicts";
  return (
    <div className={cn("rounded-lg border p-4", bad ? "border-status-error-border bg-status-error-bg/40" : "border-rex-border bg-rex-surface-1")}>
      <div className="flex items-center gap-2 text-[0.8125rem] text-rex-text">
        {job.status === "running" && <Loader2 className="h-3.5 w-3.5 animate-rex-spin text-rex-text-muted" />}
        {job.status === "running" && <span>{job.kind === "push" ? "Pushing…" : "Pulling…"}</span>}
        {job.status === "ok" && job.kind === "pull" && (
          <span>
            Pulled {plural(job.tables, "table")} ({plural(job.rows, "row")}) and {plural(job.files, "file")}.
            {job.backupDb && (
              <>
                {" "}The previous local tables are in <span className="font-mono">{job.backupDb}</span>.
              </>
            )}
          </span>
        )}
        {job.status === "ok" && job.kind === "push" && (
          <span>
            Pushed {plural(job.tables, "table")} and {plural(job.files, "file")} to <span className="font-mono">{liveHost}</span>.
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
        <div className="mt-2">
          <ul className="font-mono text-[0.71875rem] text-rex-text-muted">
            {job.conflicts.map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
          <div className="mt-2 flex gap-2">
            <Button size="sm" variant="secondary" disabled={busy} onClick={() => onOverride(job.conflicts)}>
              Push anyway, overwriting these
            </Button>
          </div>
          <div className="mt-1 text-[0.71875rem] text-rex-text-muted">Or pull first to bring the live changes here, then push again.</div>
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
    <div className="rounded-lg border border-rex-border bg-rex-surface-1 p-4">
      <div className="text-[0.8125rem] font-medium text-rex-text">Connect this site to a live one</div>
      <p className="mt-1 text-[0.78125rem] leading-[1.55] text-rex-text-muted">
        Install the <span className="font-mono">rexenv Sync</span> plugin on the live site, open Tools → rexenv Sync there,
        press <em>Connect to rexenv</em>, and paste the key it shows. rexenv checks the key against the live site before
        keeping it; the live site is never written to by a pull.
      </p>
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
