import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Download,
  ExternalLink,
  FileText,
  Pause,
  Play,
  RefreshCw,
  Trash2,
} from "lucide-react";
// In-house dialog, NOT window.confirm — tauri-plugin-dialog replaces the native
// confirm with an ASYNC override (a bare `if (!confirm(...))` never blocks).
import { confirm } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { toast, toastBackendError } from "@/lib/toast";
import {
  logClear,
  logDownload,
  logTargets,
  openExternal,
  tailLog,
  wpDebugLogClear,
  wpDebugLogDownload,
  wpDebugLogStatus,
  wpDebugLogTail,
} from "@/lib/ipc";
import type { LogCategory, LogTarget, Site } from "@/types";

const LOG_LINES = 500;

/** Heuristic per-line tint for unstructured log text (shared with the
 *  Overview's Recent-logs peek). */
export function logLineColor(line: string): string {
  if (/\berror\b/i.test(line)) return "text-status-error-bright";
  if (/\bwarn(ing)?\b/i.test(line)) return "text-status-warning-bright";
  return "text-rex-text-value";
}

/** Human-readable byte count for the debug-log size chip. */
function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

type LogsSection = "wordpress" | LogCategory;

const SELECT_CLS =
  "h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50";

const ACTION_CLS =
  "flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:opacity-50";

/** The site Logs tab: ONE card with category tabs (WordPress debug log first
 *  and default on WP sites, then the shared Server / Database logs and this
 *  site's Git job logs), a shared toolbar (Refresh / Pause / Clear / Download /
 *  Open file) and one live-tail pane. Server/DB logs are SHARED files — Clear
 *  on those asks first and says so; the WP debug log and Git job logs are
 *  per-site, so Clear there stays one click (matching the old behavior). */
export function SiteLogs({
  site,
  isWordpress,
  wpResolved,
}: {
  site: Site;
  isWordpress: boolean;
  /** Whether the wp-info query has settled — until then `isWordpress` is a
   *  placeholder `false`, and rendering the tabs would default to Server and
   *  then self-switch to WordPress when the answer lands (a tab jump under
   *  the user's cursor). Hold the first paint instead. */
  wpResolved: boolean;
}) {
  const qc = useQueryClient();
  const [selectedTab, setSelectedTab] = useState<LogsSection | null>(null);
  const [sel, setSel] = useState<Partial<Record<LogCategory, string>>>({});
  const [paused, setPaused] = useState(false);

  const { data: targets = [] } = useQuery({
    queryKey: ["log-targets", site.id],
    queryFn: () => logTargets(site.id),
  });
  const grouped: Record<LogCategory, LogTarget[]> = { server: [], database: [], git: [] };
  for (const t of targets) grouped[t.category]?.push(t);

  const sections: { key: LogsSection; label: string; show: boolean }[] = [
    { key: "wordpress", label: "WordPress debug log", show: isWordpress },
    { key: "server", label: "Server (nginx/PHP)", show: true },
    { key: "database", label: "Database", show: true },
    { key: "git", label: "Git jobs", show: grouped.git.length > 0 },
  ];
  let active: LogsSection = selectedTab ?? (isWordpress ? "wordpress" : "server");
  if (!sections.find((s) => s.key === active)?.show) active = isWordpress ? "wordpress" : "server";

  // Selected source per category (defaults: first server log; the site's own
  // DB engine's log; the first Git job).
  const dbDefault = site.dbEngine === "mariadb" ? "mariadb-error.log" : "mysql-error.log";
  function currentTarget(cat: LogCategory): LogTarget | null {
    const list = grouped[cat];
    const want = sel[cat] ?? (cat === "database" ? dbDefault : list[0]?.key);
    return list.find((t) => t.key === want) ?? list[0] ?? null;
  }
  const fileTarget = active === "wordpress" ? null : currentTarget(active);

  // ── File-log tail (Server / Database / Git tabs) — 1s live poll.
  const { data: fileLines = [], refetch: refetchFile } = useQuery({
    queryKey: ["tail-log", fileTarget?.key],
    queryFn: () => tailLog(fileTarget!.key, LOG_LINES),
    enabled: !!fileTarget,
    refetchInterval: paused ? false : 1000,
  });

  // ── WordPress debug log — status-aware (WP_DEBUG / WP_DEBUG_LOG), 2s tail.
  const { data: status } = useQuery({
    queryKey: ["wp-debug-log-status", site.id],
    queryFn: () => wpDebugLogStatus(site.id),
    enabled: isWordpress,
    refetchInterval: active === "wordpress" && !paused ? 5000 : false,
  });
  const loggingOn = !!status && status.debug && status.logEnabled;
  const { data: wpLines = [], refetch: refetchWp } = useQuery({
    queryKey: ["wp-debug-log-tail", site.id],
    queryFn: () => wpDebugLogTail(site.id, LOG_LINES),
    // Only poll while the log is live; a stale file stays readable on demand.
    enabled: isWordpress && !!status?.exists,
    refetchInterval: active !== "wordpress" || paused || !loggingOn ? false : 2000,
  });

  const wpClear = useMutation({
    mutationFn: () => wpDebugLogClear(site.id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["wp-debug-log-tail", site.id] });
      qc.invalidateQueries({ queryKey: ["wp-debug-log-status", site.id] });
      toast.success("Debug log cleared.");
    },
    onError: (e) => toastBackendError(e),
  });
  const wpDownload = useMutation({
    mutationFn: () => wpDebugLogDownload(site.id),
    onSuccess: (dest) => toast.success(`Saved to ${dest}`),
    onError: (e) => toastBackendError(e),
  });
  const fileClear = useMutation({
    mutationFn: (key: string) => logClear(key),
    onSuccess: (_d, key) => {
      qc.invalidateQueries({ queryKey: ["tail-log", key] });
      toast.success("Log cleared.");
    },
    onError: (e) => toastBackendError(e),
  });
  const fileDownload = useMutation({
    mutationFn: (key: string) => logDownload(key),
    onSuccess: (dest) => toast.success(`Saved to ${dest}`),
    onError: (e) => toastBackendError(e),
  });

  async function onClear() {
    if (active === "wordpress") {
      wpClear.mutate(); // per-site file — one click, as before
      return;
    }
    if (!fileTarget) return;
    // Server/DB logs are ONE shared file for all sites — say so before wiping.
    if (active === "server" || active === "database") {
      const ok = await confirm({
        title: `Clear ${fileTarget.label}?`,
        message: `${fileTarget.key} is shared by every site — clearing empties it for all sites, not just ${site.domain}.`,
        danger: true,
        confirmLabel: "Clear",
      });
      if (!ok) return;
    }
    fileClear.mutate(fileTarget.key);
  }

  const onRefresh = () => {
    if (paused) setPaused(false);
    else void (active === "wordpress" ? refetchWp() : refetchFile());
  };
  const clearPending = active === "wordpress" ? wpClear.isPending : fileClear.isPending;
  const downloadPending = active === "wordpress" ? wpDownload.isPending : fileDownload.isPending;
  const pathShown = active === "wordpress" ? status?.path : fileTarget?.path;
  // WP actions stay existence-gated (we know); file logs have no cheap
  // existence signal — leave enabled, the backend errors honestly.
  const wpMissing = active === "wordpress" && !status?.exists;

  // Paint the tabs ONCE, with the final default (after every hook — React's
  // rules). Only the first-ever open of a site waits here; wp-info is cached
  // afterwards, so the card renders instantly with no self-switching tab.
  if (!wpResolved) {
    return (
      <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-6 text-[0.8125rem] text-rex-text-muted">
        Loading logs…
      </div>
    );
  }

  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1">
      <div className="flex gap-0.5 border-b border-rex-border px-2">
        {sections
          .filter((s) => s.show)
          .map((s) => (
            <button
              key={s.key}
              onClick={() => setSelectedTab(s.key)}
              className={cn(
                "-mb-px border-b-2 px-3 py-2 text-[0.8125rem] font-medium transition-colors",
                active === s.key
                  ? "border-brand text-rex-text"
                  : "border-transparent text-rex-text-muted hover:text-rex-text",
              )}
            >
              {s.label}
            </button>
          ))}
      </div>

      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-rex-border p-2.5">
        {active === "wordpress" ? (
          <div className="flex min-w-0 items-center gap-2.5">
            <FileText className="h-4 w-4 flex-none text-rex-text-muted" strokeWidth={1.7} />
            <span className="text-[0.8125rem] font-medium text-rex-text">debug.log</span>
            {status && (
              <span
                className={cn(
                  "rounded-full border px-2 py-0.5 font-mono text-[0.65625rem]",
                  loggingOn
                    ? "border-rex-border text-status-running"
                    : "border-rex-border text-rex-text-dim",
                )}
              >
                {status.indeterminate ? "can't determine" : loggingOn ? "logging on" : "logging off"}
              </span>
            )}
            {status?.exists && (
              <span className="font-mono text-[0.65625rem] text-rex-text-dim">
                {fmtBytes(status.sizeBytes)}
              </span>
            )}
          </div>
        ) : (
          <select
            value={fileTarget?.key ?? ""}
            onChange={(e) =>
              setSel((s) => ({ ...s, [active as LogCategory]: e.target.value }))
            }
            className={SELECT_CLS}
          >
            {(grouped[active as LogCategory] ?? []).map((t) => (
              <option key={t.key} value={t.key}>
                {t.label}
              </option>
            ))}
          </select>
        )}
        <div className="flex flex-none items-center gap-1.5">
          <button
            onClick={onRefresh}
            title={paused ? "Resume live refresh" : "Refresh now"}
            className={ACTION_CLS}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Refresh
          </button>
          <button
            onClick={() => setPaused((p) => !p)}
            title={paused ? "Resume live refresh" : "Pause live refresh"}
            className={ACTION_CLS}
          >
            {paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
            {paused ? "Resume" : "Pause"}
          </button>
          <button
            onClick={() => void onClear()}
            disabled={wpMissing || clearPending}
            title={
              active === "server" || active === "database"
                ? "Empty this log file (shared by all sites — asks first)"
                : "Empty this log file"
            }
            className={ACTION_CLS}
          >
            <Trash2 className="h-3.5 w-3.5" />
            Clear
          </button>
          <button
            onClick={() =>
              active === "wordpress"
                ? wpDownload.mutate()
                : fileTarget && fileDownload.mutate(fileTarget.key)
            }
            disabled={wpMissing || downloadPending}
            title="Save a copy to Downloads"
            className={ACTION_CLS}
          >
            <Download className="h-3.5 w-3.5" />
            Download
          </button>
          <button
            onClick={() => pathShown && void openExternal(pathShown).catch(toastBackendError)}
            disabled={wpMissing || !pathShown}
            title="Open the log file in the default app"
            className={ACTION_CLS}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            Open file
          </button>
        </div>
      </div>

      {pathShown && (
        <div className="border-b border-rex-border-subtle px-3 py-1.5">
          <span
            className="truncate font-mono text-[0.6875rem] text-rex-text-dim"
            title={pathShown}
          >
            {pathShown}
          </span>
        </div>
      )}

      {active === "wordpress" ? (
        <LogPane lines={wpLines} paused={paused}>
          {!status ? null : status.indeterminate && !status.exists ? (
            <div className="flex flex-col gap-2 text-rex-text-muted">
              <div>
                Can&apos;t tell whether debug logging is on: this project keeps its WordPress
                config outside <span className="font-mono">wp-config.php</span> (Bedrock-style),
                which rexenv doesn&apos;t read yet.
              </div>
              <div>
                If logging is on, entries appear here once{" "}
                <span className="font-mono">{status.path}</span> exists.
              </div>
            </div>
          ) : !loggingOn && !status.indeterminate && !status.exists ? (
            <div className="flex flex-col gap-2 text-rex-text-muted">
              <div>
                WordPress debug logging is off — nothing is being written to{" "}
                <span className="font-mono">debug.log</span>.
              </div>
              <div>
                Turn on <span className="font-mono">WP_DEBUG</span> from the{" "}
                <span className="text-rex-text">WordPress → Tools</span> tab, and add this to{" "}
                <span className="font-mono">wp-config.php</span> to log to a file:
              </div>
              <pre className="w-fit rounded-lg border border-rex-border-subtle bg-rex-well px-3 py-2 text-[0.6875rem] text-rex-text-bright">
                {"define( 'WP_DEBUG', true );\ndefine( 'WP_DEBUG_LOG', true );"}
              </pre>
            </div>
          ) : !status.exists ? (
            <div className="text-rex-text-muted">
              No <span className="font-mono">debug.log</span> yet — WordPress creates it when
              the first notice, warning, or error is logged.
            </div>
          ) : wpLines.length === 0 ? (
            <div className="text-rex-text-muted">The debug log is empty.</div>
          ) : !loggingOn && !status.indeterminate ? (
            <div className="mb-2 text-rex-text-dim">
              Note: logging is currently off (
              {!status.debug ? "WP_DEBUG is false" : "WP_DEBUG_LOG is false"}) — these are
              older entries.
            </div>
          ) : null}
        </LogPane>
      ) : (
        <LogPane lines={fileLines} paused={paused}>
          {fileLines.length === 0 ? (
            <div className="text-rex-text-muted">
              No log output yet — start the site&apos;s services and traffic will appear here.
            </div>
          ) : null}
        </LogPane>
      )}
    </div>
  );
}

/** The live-tail pane: colored lines, auto-scroll to the newest line unless
 *  the user scrolled up (or paused). `children` renders ABOVE the lines —
 *  empty-state / stale-entries notes; when there are no lines it is the whole
 *  pane content. */
function LogPane({
  lines,
  paused,
  children,
}: {
  lines: string[];
  paused: boolean;
  children?: React.ReactNode;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && atBottomRef.current && !paused) el.scrollTop = el.scrollHeight;
  }, [lines, paused]);

  function onScroll() {
    const el = scrollRef.current;
    if (!el) return;
    atBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  return (
    <div
      ref={scrollRef}
      onScroll={onScroll}
      className="h-[52vh] overflow-auto bg-rex-surface-2/40 p-3 font-mono text-[0.71875rem] leading-relaxed text-rex-text"
    >
      {children}
      {lines.map((l, i) => (
        <div key={i} className={cn("whitespace-pre-wrap break-all", logLineColor(l))}>
          {l}
        </div>
      ))}
    </div>
  );
}
