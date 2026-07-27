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
import { AlertCircle, Database, Loader2, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { toastBackendError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import {
  dbImportCancel,
  dbImportRecord,
  dbImportStart,
  dbImportState,
  onDbImportState,
} from "@/lib/ipc";
import type { DbImportJobState, Site } from "@/types";

function bytes(n: number): string {
  if (n >= 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
}

/** The copy-paste block for this site's config shape. */
function connectionSnippet(site: Site, dbName: string, mirrored: string | null): string[] {
  if (site.type === "laravel") {
    const lines = [`DB_HOST=127.0.0.1`, `DB_PORT=${site.dbEngine === "mariadb" ? 13307 : 13306}`];
    if (!mirrored) lines.push(`DB_USERNAME=root`, `DB_PASSWORD=`);
    if (dbName) lines.push(`DB_DATABASE=${dbName}`);
    return lines;
  }
  const port = site.dbEngine === "mariadb" ? 13307 : 13306;
  const lines = [`define( 'DB_HOST', '127.0.0.1:${port}' );`];
  if (!mirrored) {
    lines.push(`define( 'DB_USER', 'root' );`, `define( 'DB_PASSWORD', '' );`);
  }
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

      {record && !running && (
        <div className="mt-3 space-y-3">
          <div
            className={cn(
              "rounded-lg border border-status-warning-border bg-status-warning-bg/30 p-3",
            )}
          >
            <div className="flex items-start gap-2">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-status-warning" />
              <div className="min-w-0 space-y-1.5 text-sm">
                <p className="font-medium">Imported — not yet connected.</p>
                <p>
                  <span className="font-mono text-[0.78125rem]">{record.dbName}</span>{" "}
                  ({record.tableCount} tables, {bytes(record.sizeBytes)}) was copied from{" "}
                  {record.sourceLabel} into rexenv's{" "}
                  {site.dbEngine === "mariadb" ? "MariaDB" : "MySQL"}.{" "}
                  <strong>This site still reads and writes the old database</strong> — and
                  from now on the copy and the original drift apart: changes made on the
                  site go to the old one, and nothing updates the copy.
                </p>
                <p>
                  {record.mirroredUser ? (
                    <>
                      Its database user{" "}
                      <span className="font-mono text-[0.78125rem]">{record.mirroredUser}</span>{" "}
                      already works on rexenv's engine with the same password, so switching
                      over is the connection line{record.dbName !== site.dbName ? "s" : ""} below.
                    </>
                  ) : (
                    <>
                      This site connects as <span className="font-mono text-[0.78125rem]">root</span>,
                      which rexenv never mirrors — so switching over also means setting the
                      user to <span className="font-mono text-[0.78125rem]">root</span> with an
                      empty password (rexenv's local-dev default).
                    </>
                  )}
                </p>
              </div>
            </div>
          </div>
          <div className="rounded-lg border border-rex-border bg-rex-surface-2 p-3">
            <p className="mb-2 text-xs text-rex-text-secondary">
              To switch this site to the rexenv copy, change{" "}
              {site.type === "laravel" ? ".env" : "wp-config.php"} to:
            </p>
            <pre className="overflow-x-auto font-mono text-[0.75rem] leading-relaxed">
              {connectionSnippet(site, record.dbName, record.mirroredUser).join("\n")}
            </pre>
            <p className="mt-2 text-xs text-rex-text-secondary">
              rexenv doesn't edit your project files. A one-click, backed-up, diff-first
              version of this change is coming.
              {site.type === "laravel" &&
                " If you use `php artisan config:cache`, run `config:clear` afterwards — cached config ignores .env edits."}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
