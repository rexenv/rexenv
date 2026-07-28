/**
 * Database import for a linked/imported site (Stage 2, steps 8–9).
 *
 * THE INTERIM STATE IS THE POINT. After a successful import and before the
 * Stage 3 rewrite exists, the site still reads and writes its OLD database —
 * the state most users will actually sit in. Everything rendered here (and the
 * Sites badge) comes from ONE serialized fact (`DbImportRecord`), so the badge
 * and this summary can never disagree; and the copy says three things plainly:
 * the copy exists in rexenv, the site still uses the old database, and the two
 * drift apart from now on.
 */
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, Database, Loader2, Undo2, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/dialog";
import { toast, toastBackendError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import {
  dbImportCancel,
  dbImportRecord,
  dbImportStart,
  dbImportState,
  onDbImportState,
  rewriteApply,
  rewritePreview,
  rewriteRevert,
} from "@/lib/ipc";
import type {
  DbImportJobState,
  RewriteApplied,
  RewriteRevertOutcome,
  Site,
} from "@/types";

function bytes(n: number): string {
  if (n >= 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
}

/** The copy-paste block for this site's config shape. A mirrored user (their
 *  own from Stage 2, or the dedicated `rex_…` a rewrite created) holds the
 *  password already in the config, so the user line names IT — omitting the
 *  line while the file might still say `root` gave incomplete instructions. */
function connectionSnippet(site: Site, dbName: string, mirrored: string | null): string[] {
  if (site.type === "laravel") {
    const lines = [`DB_HOST=127.0.0.1`, `DB_PORT=${site.dbEngine === "mariadb" ? 13307 : 13306}`];
    if (mirrored) lines.push(`DB_USERNAME=${mirrored}`);
    else lines.push(`DB_USERNAME=root`, `DB_PASSWORD=`);
    if (dbName) lines.push(`DB_DATABASE=${dbName}`);
    return lines;
  }
  const port = site.dbEngine === "mariadb" ? 13307 : 13306;
  const lines = [`define( 'DB_HOST', '127.0.0.1:${port}' );`];
  if (mirrored) lines.push(`define( 'DB_USER', '${mirrored}' );`);
  else lines.push(`define( 'DB_USER', 'root' );`, `define( 'DB_PASSWORD', '' );`);
  return lines;
}

export function DbImportCard({ site }: { site: Site }) {
  const qc = useQueryClient();
  const { data: record } = useQuery({
    queryKey: ["db-import-record", site.id],
    queryFn: () => dbImportRecord(site.id),
  });
  const [job, setJob] = useState<DbImportJobState | null>(null);
  const [confirmName, setConfirmName] = useState("");

  // ── The Stage 3 rewrite surface ──────────────────────────────────────────
  // Preview runs whenever a settled record exists: for `imported` it drives
  // the consent card; for `connected` it still supplies the Laravel cache
  // flag (§5) beside the verified note.
  const { data: preview } = useQuery({
    queryKey: ["rewrite-preview", site.id],
    queryFn: () => rewritePreview(site.id),
    enabled: !!record && job?.status !== "running",
  });
  const [consent, setConsent] = useState(false);
  const [applyOutcome, setApplyOutcome] = useState<RewriteApplied | null>(null);
  const [revertOutcome, setRevertOutcome] = useState<RewriteRevertOutcome | null>(null);
  const [revertConfirm, setRevertConfirm] = useState<null | "normal" | "force">(null);
  // A new fingerprint means a different file: any prior consent is void.
  const fingerprint = preview?.status === "ready" ? preview.fingerprint : null;
  useEffect(() => setConsent(false), [fingerprint]);

  const apply = useMutation({
    mutationFn: () => {
      if (!fingerprint) return Promise.reject(new Error("no previewed change"));
      return rewriteApply(site.id, fingerprint);
    },
    onSuccess: (out) => {
      setApplyOutcome(out);
      setConsent(false);
      void qc.invalidateQueries({ queryKey: ["rewrite-preview", site.id] });
      if (out.status === "applied") {
        toast.success(out.message);
        void qc.invalidateQueries({ queryKey: ["db-import-record", site.id] });
        void qc.invalidateQueries({ queryKey: ["db-import-records"] });
        void qc.invalidateQueries({ queryKey: ["sites"] });
      }
    },
    onError: toastBackendError,
  });

  const revert = useMutation({
    mutationFn: (force: boolean) => rewriteRevert(site.id, force),
    onSuccess: (out) => {
      setRevertOutcome(out);
      setApplyOutcome(null);
      void qc.invalidateQueries({ queryKey: ["rewrite-preview", site.id] });
      void qc.invalidateQueries({ queryKey: ["db-import-record", site.id] });
      void qc.invalidateQueries({ queryKey: ["db-import-records"] });
      void qc.invalidateQueries({ queryKey: ["sites"] });
      if (out.status === "reverted") toast.success(out.message);
    },
    onError: toastBackendError,
  });

  // Re-attach to a job already running (navigation away and back).
  useEffect(() => {
    let un: (() => void) | undefined;
    let live = true;
    void dbImportState(site.id).then(async (s) => {
      if (!live || !s) return;
      setJob(s);
      if (s.status === "running") {
        un = await onDbImportState(s.id, (next) => {
          setJob(next);
          if (next.status === "ok") {
            void qc.invalidateQueries({ queryKey: ["db-import-record", site.id] });
            void qc.invalidateQueries({ queryKey: ["sites"] });
          }
        });
      }
    });
    return () => {
      live = false;
      un?.();
    };
  }, [site.id, qc]);

  const start = useMutation({
    mutationFn: async (confirmOverwrite?: string) => {
      const snap = await dbImportStart(site.id, confirmOverwrite);
      setJob(snap);
      const un = await onDbImportState(snap.id, (next) => {
        setJob(next);
        if (next.status !== "running") un();
        if (next.status === "ok") {
          void qc.invalidateQueries({ queryKey: ["db-import-record", site.id] });
          void qc.invalidateQueries({ queryKey: ["sites"] });
        }
      });
    },
    onError: toastBackendError,
  });

  const running = job?.status === "running";
  const failed = job?.status === "failed";
  const needsTypedConfirm = failed && (job?.error ?? "").includes("type the database name");

  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <Database className="h-4 w-4 text-rex-text-secondary" />
          <span className="text-sm font-medium">Database import</span>
        </div>
        {!running && (
          <Button size="sm" variant="secondary" onClick={() => start.mutate(undefined)}>
            {record ? "Import again" : "Import database"}
          </Button>
        )}
        {running && job && (
          <Button size="sm" variant="ghost" onClick={() => void dbImportCancel(job.id)}>
            Cancel
          </Button>
        )}
      </div>

      {running && job && (
        <div className="mt-3 space-y-2">
          <div className="h-1.5 overflow-hidden rounded-full bg-rex-surface-2">
            <div
              className="h-full rounded-full bg-brand transition-[width] duration-300"
              style={{ width: `${job.pct}%` }}
            />
          </div>
          <div className="flex items-center gap-2 text-xs text-rex-text-secondary">
            <Loader2 className="h-3 w-3 animate-spin" />
            {job.phases[job.phaseCursor]?.label ?? "working"} · {job.pct}%
          </div>
        </div>
      )}

      {failed && job && (
        <div className="mt-3 space-y-2 rounded-lg border border-status-error-border bg-status-error-bg/30 p-3">
          <div className="flex items-start gap-2 text-sm">
            <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-status-error" />
            <span className="min-w-0 whitespace-pre-wrap break-words">{job.error}</span>
          </div>
          {job.keptArtifact && (
            <p className="text-xs text-rex-text-secondary">
              The copy taken so far was kept at{" "}
              <span className="font-mono">{job.keptArtifact}</span> — it contains this
              database's data. Retry replaces it; Settings can delete it.
            </p>
          )}
          {needsTypedConfirm ? (
            <div className="flex items-center gap-2">
              <input
                value={confirmName}
                onChange={(e) => setConfirmName(e.target.value)}
                placeholder="type the database name"
                className="h-8 flex-1 rounded-md border border-rex-border bg-rex-surface-2 px-2 font-mono text-xs"
              />
              <Button
                size="sm"
                variant="secondary"
                disabled={!confirmName.trim()}
                onClick={() => start.mutate(confirmName.trim())}
              >
                Overwrite and import
              </Button>
            </div>
          ) : (
            <Button size="sm" variant="secondary" onClick={() => start.mutate(undefined)}>
              Retry
            </Button>
          )}
        </div>
      )}

      {record?.state === "connected" && !running && (
        <div className="mt-3 space-y-3">
          <div className="rounded-lg border border-status-running-border bg-status-running-bg/30 p-3 text-sm">
            <div className="flex items-start justify-between gap-2">
              {/* Exactly what was proven (§6): the rewritten settings sign
                  in — not "the site is now using this database". */}
              <div className="min-w-0 space-y-1.5">
                <p className="font-medium">Connected.</p>
                <p>
                  This site's connection settings were rewritten and verified: they sign in
                  to <span className="font-mono text-[0.78125rem]">{record.dbName}</span> on
                  rexenv's engine
                  {record.verified === "signin+http" &&
                    ", and the site answered over HTTP without a database error"}
                  .
                </p>
                {preview?.status === "ready" && preview.laravelCacheWarning && (
                  <p className="text-xs text-rex-text-secondary">
                    This site has a cached config (
                    <span className="font-mono">bootstrap/cache/config.php</span>) — Laravel
                    keeps reading the cache until you run{" "}
                    <span className="font-mono">php artisan config:clear</span>. rexenv never
                    runs your artisan.
                  </p>
                )}
                {preview?.status === "ready" && preview.diff.length > 0 && (
                  /* Same class as §C1.3, one state over: `connected` is a
                     proven PAST fact, but if the file was edited afterwards
                     it no longer points at the copy — the panel must say so
                     rather than let the green badge imply the present. */
                  <p className="text-xs">
                    <strong>The file has changed since verification</strong> — it no
                    longer points at the rexenv copy. Apply the change again to
                    reconnect, or revert.
                  </p>
                )}
              </div>
              <Button size="sm" variant="ghost" onClick={() => setRevertConfirm("normal")}>
                <Undo2 className="mr-1 h-3.5 w-3.5" />
                Revert
              </Button>
            </div>
          </div>
          {revertOutcome?.status === "refusedEdited" && (
            <div className="rounded-lg border border-status-warning-border bg-status-warning-bg/30 p-3 text-sm">
              <p className="whitespace-pre-wrap break-words">{revertOutcome.message}</p>
              <Button
                size="sm"
                variant="danger"
                className="mt-2"
                onClick={() => setRevertConfirm("force")}
              >
                Restore anyway
              </Button>
            </div>
          )}
          {revertOutcome?.status === "backupMissing" && (
            <div className="rounded-lg border border-rex-border bg-rex-surface-2 p-3 text-sm">
              <p className="whitespace-pre-wrap break-words">{revertOutcome.message}</p>
            </div>
          )}
        </div>
      )}

      {revertConfirm === "normal" && (
        <ConfirmDialog
          title="Revert the connection change?"
          message={
            <>
              Restores the file rexenv rewrote to its original, byte for byte — this site
              goes back to reading its old database. rexenv's copy of the database stays
              where it is, and the two drift apart again from that point.
            </>
          }
          confirmLabel="Revert"
          onConfirm={() => {
            setRevertConfirm(null);
            revert.mutate(false);
          }}
          onCancel={() => setRevertConfirm(null)}
        />
      )}
      {revertConfirm === "force" && (
        <ConfirmDialog
          danger
          title="Restore anyway?"
          message={
            <>
              The file changed after rexenv rewrote it. Restoring the backup replaces the
              file's <strong>current</strong> content — edits made since the rewrite are
              lost. This can't be undone.
            </>
          }
          confirmLabel="Restore anyway"
          onConfirm={() => {
            setRevertConfirm(null);
            revert.mutate(true);
          }}
          onCancel={() => setRevertConfirm(null)}
        />
      )}

      {record?.state === "imported" && !running && (
        <div className="mt-3 space-y-3">
          {/* THE ONE-FACT RULE, applied to copy (UI-REVIEW §C1.3): the claim
              about which database the site reads derives from the SAME
              preview the consent card renders — never asserted on its own.
              The old unconditional "still reads and writes the old database"
              contradicted a consent card saying the file already points at
              rexenv (the rewritten-but-unverified state). */}
          <div
            className={cn(
              "rounded-lg border border-status-warning-border bg-status-warning-bg/30 p-3",
            )}
          >
            <div className="flex items-start gap-2">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-status-warning" />
              <div className="min-w-0 space-y-1.5 text-sm">
                <p className="font-medium">
                  {preview?.status === "ready" && preview.diff.length === 0
                    ? "Imported — rewritten, not yet verified."
                    : "Imported — not yet connected."}
                </p>
                <p>
                  <span className="font-mono text-[0.78125rem]">{record.dbName}</span>{" "}
                  ({record.tableCount} tables, {bytes(record.sizeBytes)}) was copied from{" "}
                  {record.sourceLabel} into rexenv's{" "}
                  {site.dbEngine === "mariadb" ? "MariaDB" : "MySQL"}.
                </p>
                {preview === undefined && (
                  <p>Checking which database the site's config points at…</p>
                )}
                {preview?.status === "ready" && preview.diff.length > 0 && (
                  <p>
                    <strong>This site still reads and writes the old database</strong> —
                    and from now on the copy and the original drift apart: changes made
                    on the site go to the old one, and nothing updates the copy.
                  </p>
                )}
                {preview?.status === "ready" && preview.diff.length === 0 && (
                  /* The honest sentence for this state: the FILE is proven
                     (it points at the copy); the CONNECTION is not. Which
                     database the running site actually uses is unknown until
                     the sign-in check passes — so say exactly that. */
                  <p>
                    <strong>
                      The config file now points at the rexenv copy, but the connection
                      hasn't been verified
                    </strong>{" "}
                    — until the check below passes, rexenv can't say which database the
                    site is actually using.
                  </p>
                )}
                {preview?.status === "refused" && (
                  <p>
                    <strong>
                      rexenv couldn't read this site's connection config confidently
                    </strong>{" "}
                    (the reason is below), so it can't say which database the site is
                    using.
                  </p>
                )}
              </div>
            </div>
          </div>
          {applyOutcome?.status === "fileChanged" && (
            /* A normal thing, not an error: they edited the file while the
               diff was open. The preview below is already the refreshed one. */
            <div className="rounded-lg border border-rex-border bg-rex-surface-2 p-3 text-sm">
              <p className="whitespace-pre-wrap break-words">{applyOutcome.message}</p>
              <p className="mt-1 text-xs text-rex-text-secondary">
                Nothing was written. The change shown below is against the file as it is
                now.
              </p>
            </div>
          )}
          {applyOutcome?.status === "engineStopped" && (
            <div className="rounded-lg border border-status-warning-border bg-status-warning-bg/30 p-3 text-sm">
              <p className="whitespace-pre-wrap break-words">{applyOutcome.message}</p>
            </div>
          )}
          {applyOutcome?.status === "verifyFailed" && (
            /* Written and backed up, but NOT verified — a different state
               from "couldn't write", and the copy carries the difference. */
            <div className="rounded-lg border border-status-warning-border bg-status-warning-bg/30 p-3 text-sm">
              <p className="font-medium">Change applied — not verified.</p>
              <p className="mt-1 whitespace-pre-wrap break-words">{applyOutcome.message}</p>
              <p className="mt-1 text-xs text-rex-text-secondary">{applyOutcome.reason}</p>
              <Button
                size="sm"
                variant="secondary"
                className="mt-2"
                onClick={() => setRevertConfirm("normal")}
              >
                <Undo2 className="mr-1 h-3.5 w-3.5" />
                Revert the change
              </Button>
            </div>
          )}

          {preview?.status === "ready" && (
            <div className="space-y-2 rounded-lg border border-rex-border bg-rex-surface-2 p-3">
              {preview.laravelCacheWarning && (
                /* §5: leads the panel, because an applied edit + a cached
                   config looks exactly like "the rewrite didn't work". */
                <div className="rounded-md border border-status-warning-border bg-status-warning-bg/30 p-2 text-xs">
                  <strong>This site has a cached config</strong> (
                  <span className="font-mono">bootstrap/cache/config.php</span>): even after
                  the change below, Laravel keeps reading the cache until you run{" "}
                  <span className="font-mono">php artisan config:clear</span>. rexenv never
                  runs your artisan.
                </div>
              )}
              <p className="text-sm font-medium">Connect this site to the rexenv copy</p>
              {preview.diff.length > 0 ? (
                <>
                  <p className="text-xs text-rex-text-secondary">
                    One change to <span className="font-mono">{preview.file}</span>, shown
                    exactly as it will be written — nothing else in the file is touched:
                  </p>
                  <pre className="overflow-x-auto rounded-md bg-rex-surface-1 p-2 font-mono text-[0.75rem] leading-relaxed">
                    {preview.diff.map((d, i) => (
                      <span
                        key={i}
                        className={cn(
                          "block",
                          d.sign === "-"
                            ? "text-status-error-bright"
                            : "text-status-running-bright",
                        )}
                      >
                        {d.sign} {d.text}
                      </span>
                    ))}
                  </pre>
                </>
              ) : (
                <p className="text-xs text-rex-text-secondary">
                  <span className="font-mono">{preview.file}</span> already points at{" "}
                  <span className="font-mono">{preview.target}</span> — nothing needs to be
                  written. Verifying signs in with the file's own settings and, if that
                  works, marks the site connected.
                </p>
              )}
              {preview.createsUser && (
                <p className="text-xs text-rex-text-secondary">
                  Because this site connects as{" "}
                  <span className="font-mono">root</span>, rexenv will create the dedicated
                  account <span className="font-mono">{preview.createsUser}</span> on its
                  engine, holding the password already in your config — the password line
                  itself is never changed, so it can't appear in the diff.
                </p>
              )}
              {/* Only with its referent (§C1.4): "the diff above" and
                  "before writing" describe a write — a verify-only state
                  writes nothing, so the note reduces to the backup fact
                  when one exists and disappears when there is none. */}
              {preview.diff.length > 0 ? (
                <p className="text-xs text-rex-text-secondary">
                  {preview.backupExists ? (
                    <>
                      An earlier backup of this file already exists on rexenv's side and
                      is kept — the FIRST backup is the one revert restores.
                    </>
                  ) : (
                    <>
                      Before writing, rexenv keeps a byte-exact backup of{" "}
                      <span className="font-mono">{preview.file}</span> on its side
                      (private, mode 600) for one-click revert.
                    </>
                  )}{" "}
                  The diff above can't contain your password — but the backup is the
                  whole file, so it does include it.
                </p>
              ) : (
                preview.backupExists && (
                  <p className="text-xs text-rex-text-secondary">
                    rexenv still holds the backup taken before the rewrite (private,
                    mode 600) — revert restores it. It is the whole original file, so
                    it includes your old password.
                  </p>
                )
              )}
              {site.dbEngine === "mariadb" && (
                /* D5's tell-only surface, in the site's own panel: the socket
                   shortcut serves MySQL only. */
                <p className="text-xs text-rex-text-secondary">
                  MariaDB note: always use{" "}
                  <span className="font-mono">127.0.0.1:13307</span> in this site's config —
                  the <span className="font-mono">localhost</span> socket shortcut doesn't
                  reach rexenv's MariaDB.
                </p>
              )}
              {preview.diff.length > 0 && (
                <label className="flex cursor-pointer items-start gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={consent}
                    onChange={(e) => setConsent(e.target.checked)}
                    className="mt-0.5"
                  />
                  <span>
                    Apply exactly the change shown above to my file (backed up first).
                  </span>
                </label>
              )}
              <Button
                size="sm"
                disabled={(preview.diff.length > 0 && !consent) || apply.isPending}
                onClick={() => apply.mutate()}
              >
                {apply.isPending ? (
                  <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
                ) : null}
                {preview.diff.length > 0 ? "Apply and verify" : "Verify connection"}
              </Button>
            </div>
          )}

          {preview?.status === "refused" && (
            <div className="rounded-lg border border-rex-border bg-rex-surface-2 p-3">
              {/* The tell-only floor: a refusal downgrades here with its
                  reason, never to a guess. The credentials sentence lives
                  HERE (not in the panel above) because it is an instruction
                  about the lines below — beside the consent card it
                  contradicted the dedicated-user note (§C1.3's class). */}
              <p className="mb-2 text-xs text-rex-text-secondary">
                The one-click change isn't available for this site: {preview.reason}
              </p>
              <p className="mb-2 text-xs text-rex-text-secondary">
                {record.mirroredUser ? (
                  <>
                    The database user{" "}
                    <span className="font-mono">{record.mirroredUser}</span> works on
                    rexenv's engine with the password already in your config, so the
                    lines below are the whole change.
                  </>
                ) : (
                  <>
                    This site connects as <span className="font-mono">root</span>, which
                    rexenv never mirrors — so the lines below also set the user to{" "}
                    <span className="font-mono">root</span> with an empty password
                    (rexenv's local-dev default).
                  </>
                )}{" "}
                To switch it over yourself, change{" "}
                {site.type === "laravel" ? ".env" : "wp-config.php"} to:
              </p>
              <pre className="overflow-x-auto font-mono text-[0.75rem] leading-relaxed">
                {connectionSnippet(site, record.dbName, record.mirroredUser).join("\n")}
              </pre>
              <p className="mt-2 text-xs text-rex-text-secondary">
                rexenv never edits your project files without the diff-and-consent step
                above being possible.
                {site.type === "laravel" &&
                  " If you use `php artisan config:cache`, run `config:clear` afterwards — cached config ignores .env edits."}
                {site.dbEngine === "mariadb" &&
                  " MariaDB note: always use 127.0.0.1:13307 — the localhost socket shortcut doesn't reach rexenv's MariaDB."}
              </p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
