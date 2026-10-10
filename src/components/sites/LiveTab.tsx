/**
 * The Live tab (`docs/PLAN-wp-live-sync.md` §2.9): connect this WordPress site
 * to a live one with the key from the rexenv Sync plugin, then Pull. Push is a
 * later stage and is not here.
 *
 * The secret never reaches this component: pairing answers with the key id and
 * the site URL only, and every later call names the site.
 */
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CloudDownload, Link2, Loader2, Unlink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { confirm } from "@/components/ui/dialog";
import { liveSyncActive, liveSyncPair, liveSyncPairing, liveSyncPull, liveSyncUnpair, onLiveSyncState } from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import { cn, TECH_INPUT } from "@/lib/utils";
import type { LiveSyncJobState, Site } from "@/types";

const INPUT =
  "h-8 w-full rounded-md border border-rex-border bg-rex-well px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-brand";

export function LiveTab({ site }: { site: Site }) {
  const qc = useQueryClient();
  const pairing = useQuery({ queryKey: ["live-sync-pairing", site.id], queryFn: () => liveSyncPairing(site.id) });
  const [job, setJob] = useState<LiveSyncJobState | null>(null);
  useEffect(() => {
    let off = () => {};
    void liveSyncActive(site.id).then((j) => {
      setJob(j);
      if (j?.status === "running") void onLiveSyncState(j.id, setJob).then((f) => (off = f));
    });
    return () => off();
  }, [site.id]);

  const pull = useMutation({
    mutationFn: (uploadsSince?: number) => liveSyncPull(site.id, uploadsSince),
    onSuccess: async (j) => {
      setJob(j);
      const off = await onLiveSyncState(j.id, (s) => {
        setJob(s);
        if (s.status !== "running") {
          off();
          void qc.invalidateQueries({ queryKey: ["sites"] });
          if (s.status === "ok") toast.success(`Pulled ${s.tables} tables and ${s.files} files from ${site.domain}'s live site`);
        }
      });
    },
    onError: toastBackendError,
  });
  const unpair = useMutation({
    mutationFn: () => liveSyncUnpair(site.id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["live-sync-pairing", site.id] }),
    onError: toastBackendError,
  });

  if (pairing.isLoading) return null;
  if (!pairing.data) return <ConnectForm site={site} onPaired={() => void qc.invalidateQueries({ queryKey: ["live-sync-pairing", site.id] })} />;
  const p = pairing.data;
  const running = job?.status === "running";

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
            disabled={running || unpair.isPending}
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
            disabled={running || pull.isPending}
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
            {running ? <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> : <CloudDownload className="h-3.5 w-3.5" />} Pull from live
          </Button>
          <Button
            variant="secondary"
            disabled={running || pull.isPending}
            title="Media older than six months stays on the live site — a big library pulls faster"
            onClick={async () => {
              if (await confirm({ title: `Pull ${p.siteUrl}, recent uploads only?`, message: "Uploads older than six months stay on the live site and will 404 here. Everything else is pulled as usual.", confirmLabel: "Pull" }))
                pull.mutate(Math.floor(Date.now() / 1000) - 183 * 86400);
            }}
          >
            Pull, recent uploads only
          </Button>
        </div>
      </div>
      {job && <JobCard job={job} />}
    </div>
  );
}

function JobCard({ job }: { job: LiveSyncJobState }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={cn("rounded-lg border p-4", job.status === "failed" ? "border-status-error-border bg-status-error-bg/40" : "border-rex-border bg-rex-surface-1")}>
      <div className="flex items-center gap-2 text-[0.8125rem] text-rex-text">
        {job.status === "running" && <Loader2 className="h-3.5 w-3.5 animate-rex-spin text-rex-text-muted" />}
        {job.status === "running" && <span>Pulling…</span>}
        {job.status === "ok" && (
          <span>
            Pulled {job.tables} tables ({job.rows} rows) and {job.files} files.
            {job.backupDb && (
              <>
                {" "}The previous local tables are in <span className="font-mono">{job.backupDb}</span>.
              </>
            )}
          </span>
        )}
        {job.status === "failed" && <span className="whitespace-pre-line text-status-error-bright">{job.error}</span>}
      </div>
      {job.refusedFiles.length > 0 && (
        <div className="mt-1 text-[0.71875rem] text-rex-text-muted">{job.refusedFiles.length} file(s) the live site would not serve were left out.</div>
      )}
      {job.lines.length > 0 && (
        <button type="button" className="mt-2 text-[0.71875rem] text-rex-accent-blue hover:underline" onClick={() => setOpen((o) => !o)}>
          {open ? "Hide log" : "Show log"}
        </button>
      )}
      {open && (
        <pre className="mt-2 max-h-64 overflow-auto rounded-md bg-rex-well p-2 font-mono text-[0.6875rem] text-rex-text-muted">
          {job.lines.join("\n")}
        </pre>
      )}
    </div>
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
