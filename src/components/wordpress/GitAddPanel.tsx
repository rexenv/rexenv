/** "From Git" source for the Plugins/Themes add flow: paste a repo URL →
 *  Fetch (`git ls-remote`: URL + access validated BEFORE any clone) → pick a
 *  branch/tag + folder name → Add (clone + dependency detection) → explicit
 *  install/build steps with a live streamed log. The repo's own scripts
 *  (npm postinstall, composer scripts) NEVER run without a click — the
 *  disclosure line sits right above the buttons that run them. */
import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { GitBranch, Loader2, X } from "lucide-react";
import { TECH_INPUT } from "@/lib/utils";
import {
  onRepoJobOutput,
  onRepoJobState,
  repoAdd,
  repoCancel,
  repoJobState,
  repoProbe,
  repoRunStep,
  repoSiteJobs,
  repoTools,
  tailLog,
  wpPluginActivate,
  wpThemeActivate,
} from "@/lib/ipc";
import type { RepoJobState } from "@/types";
import { toast, toastBackendError } from "@/lib/toast";
import { mergeTailAndStreamed, StepDot } from "./repoJobUi";
import { RefPicker } from "./RefPicker";

/** Mirror of WordPressManager's BTN (kept local — importing it would create a
 *  module cycle with the panel embed). */
const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

const LOG_CAP = 500;

export function GitAddPanel({
  siteId,
  kind,
  onInstalled,
}: {
  siteId: string;
  kind: "plugin" | "theme";
  onInstalled: () => void;
}) {
  const qc = useQueryClient();
  const [url, setUrl] = useState("");
  const [selRef, setSelRef] = useState("");
  const [dirName, setDirName] = useState("");
  const [job, setJob] = useState<RepoJobState | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [logOpen, setLogOpen] = useState(false);
  const [activated, setActivated] = useState(false);
  const logRef = useRef<HTMLDivElement | null>(null);
  const adoptedRef = useRef(false);
  const qcSyncKey = ["repo-jobs", siteId, kind] as const;

  // RECONNECT after a remount: the backend job registry outlives this panel
  // (tab switches unmount it). Adopt the newest unfinished job — a blank
  // panel over a live clone invited a dangerous second run.
  const siteJobs = useQuery({
    queryKey: qcSyncKey,
    queryFn: () => repoSiteJobs(siteId, kind),
    refetchOnWindowFocus: false,
    staleTime: 5_000,
  });
  useEffect(() => {
    if (adoptedRef.current || job !== null) return;
    const candidate = [...(siteJobs.data ?? [])]
      .reverse()
      .find((j) => j.op === "add" && !j.finishedOk);
    if (!candidate) return;
    adoptedRef.current = true;
    setJob(candidate);
    setLogOpen(true);
    // Seed the pane from the job's log file; lines that stream in while the
    // tail loads are merged (overlap-deduped) after it lands.
    void tailLog(candidate.logKey, 300)
      .then((tail) => setLines((streamed) => mergeTailAndStreamed(tail, streamed)))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [siteJobs.data, job]);

  // git/node availability — resolved from the LOGIN-SHELL env (nvm-aware).
  const tools = useQuery({
    queryKey: ["repo-tools"],
    queryFn: () => repoTools(false),
    staleTime: 300_000,
    retry: false,
  });
  const redetect = useMutation({
    mutationFn: () => repoTools(true),
    onSuccess: (data) => qc.setQueryData(["repo-tools"], data),
    onError: (e) => toastBackendError(e),
  });

  const probe = useMutation({
    mutationFn: (raw: string) => repoProbe(raw),
    onSuccess: (p) => {
      const refs = [...p.branches, ...p.tags];
      const candidate = p.refCandidate && refs.includes(p.refCandidate) ? p.refCandidate : null;
      setSelRef(candidate ?? p.defaultBranch ?? "");
      setDirName(p.dirName);
    },
    onError: (e) => toastBackendError(e),
  });

  const add = useMutation({
    mutationFn: () =>
      repoAdd(siteId, kind, url, selRef === "" ? null : selRef, dirName === "" ? null : dirName),
    onSuccess: (snap) => {
      adoptedRef.current = true; // a fresh job is never re-adopted over
      setJob(snap);
      setLines([]);
      setLogOpen(true);
      setActivated(false);
      qc.setQueryData(qcSyncKey, (old: RepoJobState[] | undefined) =>
        old ? [...old.filter((j) => j.id !== snap.id), snap] : [snap],
      );
    },
    onError: (e) => toastBackendError(e),
  });

  const runStep = useMutation({
    mutationFn: (stepKey: string) => repoRunStep(job?.id ?? "", stepKey),
    onError: (e) => toastBackendError(e),
  });

  const activate = useMutation({
    mutationFn: async () => {
      if (!job) return;
      if (kind === "plugin") await wpPluginActivate(siteId, [job.dirName]);
      else await wpThemeActivate(siteId, job.dirName);
    },
    onSuccess: () => {
      setActivated(true);
      toast.success(kind === "plugin" ? "Plugin activated" : "Theme activated");
      onInstalled();
    },
    onError: (e) => toastBackendError(e),
  });

  // Live subscriptions for the running job; re-sync once on (re)mount.
  useEffect(() => {
    if (!job?.id) return;
    let dead = false;
    const un: Array<() => void> = [];
    void onRepoJobState(job.id, (s) => {
      if (dead) return;
      setJob(s);
      qc.setQueryData(qcSyncKey, (old: RepoJobState[] | undefined) =>
        old ? old.map((j) => (j.id === s.id ? s : j)) : old,
      );
    }).then((u) => un.push(u));
    void onRepoJobOutput(job.id, (line) => {
      if (!dead) setLines((l) => [...l.slice(-(LOG_CAP - 1)), line]);
    }).then((u) => un.push(u));
    void repoJobState(job.id)
      .then((s) => {
        if (!dead) setJob(s);
      })
      .catch(() => {});
    return () => {
      dead = true;
      un.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [job?.id]);

  // Pin the log to its newest line.
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines, logOpen]);

  // A finished clone+detect (or any step change) refreshes the plugin/theme
  // list — the cloned dir appears there as soon as WP sees its header.
  const cloneDone = job?.steps.find((s) => s.key === "clone")?.status === "ok";
  useEffect(() => {
    if (cloneDone) onInstalled();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cloneDone]);

  const stepRunning = job?.steps.some((s) => s.status === "running") ?? false;
  const runnable = useMemo(
    () => (job?.steps ?? []).filter((s) => !["clone", "detect"].includes(s.key)),
    [job],
  );
  const gitTool = tools.data?.find((t) => t.name === "git");
  const nodeTool = tools.data?.find((t) => t.name === "node");
  const wpMismatch =
    job?.inspection && job.inspection.wp.kind !== "none" && job.inspection.wp.kind !== kind;

  return (
    <div className="space-y-2">
      {/* URL → Fetch → ref + folder → Add */}
      <div className="flex flex-wrap items-center gap-2">
        <GitBranch className="h-3.5 w-3.5 flex-none text-rex-text-muted" />
        <input
          {...TECH_INPUT}
          value={url}
          onChange={(e) => {
            setUrl(e.target.value);
            probe.reset();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && url.trim() !== "") {
              e.preventDefault();
              probe.mutate(url);
            }
          }}
          placeholder="https://github.com/owner/repo · git@host:owner/repo.git · owner/repo"
          className="h-[30px] min-w-[260px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
        />
        <button
          className={BTN}
          disabled={probe.isPending || url.trim() === ""}
          onClick={() => probe.mutate(url)}
        >
          {probe.isPending ? (
            <span className="flex items-center gap-1.5">
              <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> Fetching…
            </span>
          ) : (
            "Fetch"
          )}
        </button>
        {probe.data && (
          <>
            {/* The SAME searchable picker the Repository panel uses. This was
                a plain <select>, which is fine for the five-branch repo people
                test with and useless for the real one: `ls-remote` on a busy
                project answers with hundreds of branches, and a native select
                gives you a scroll and first-letter jumping to find one. The
                two places you pick a branch should also not behave
                differently — this is the FIRST one a new user meets. */}
            <RefPicker
              value={selRef}
              onChange={setSelRef}
              ariaLabel="Branch or tag"
              placeholder="Filter branches and tags…"
              emptyText="No matching branch or tag."
              groups={[
                {
                  label: null,
                  items: probe.data.branches.map((b) => ({
                    value: b,
                    hint: b === probe.data?.defaultBranch ? "default" : undefined,
                  })),
                },
                {
                  label: "Tags",
                  // Bare name, not refs/tags/<name>: this goes to `git clone
                  // --branch`, which takes either — and the name is what the
                  // user picked out of the list.
                  items: probe.data.tags.map((t) => ({ value: t })),
                },
              ]}
            />
            <input
              {...TECH_INPUT}
              value={dirName}
              onChange={(e) => setDirName(e.target.value)}
              aria-label="Folder name"
              className="h-[30px] w-[160px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
            />
            <button
              className={BTN}
              disabled={add.isPending || (job !== null && !job.finishedOk && stepRunning)}
              onClick={() => add.mutate()}
            >
              Add {kind}
            </button>
          </>
        )}
      </div>

      {/* Tool availability + probe hint. Shorthand disclosure is static text —
          always visible, no surprises. */}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[0.6875rem] text-rex-text-muted">
        <span>
          <span className="font-mono">owner/repo</span> means github.com · private repos use your
          own SSH keys/agent
        </span>
        {gitTool && !gitTool.ok && <span className="text-status-error-bright">git missing</span>}
        {nodeTool && (
          <span className="font-mono">
            node {nodeTool.ok ? (nodeTool.version ?? "?") : "missing"}
          </span>
        )}
        <button
          className="underline decoration-dotted hover:text-rex-text"
          onClick={() => redetect.mutate()}
          disabled={redetect.isPending}
        >
          Re-detect tools
        </button>
      </div>
      {gitTool && !gitTool.ok && gitTool.error && (
        <div className="whitespace-pre-line rounded-md border border-status-error-border bg-status-error-bg px-2.5 py-1.5 font-mono text-[0.6875rem] text-status-error-bright">
          {gitTool.error}
        </div>
      )}
      {probe.isError && (
        <div className="whitespace-pre-line rounded-md border border-status-error-border bg-status-error-bg px-2.5 py-1.5 font-mono text-[0.6875rem] text-status-error-bright">
          {String(probe.error)}
        </div>
      )}

      {/* The job: steps + streamed log + explicit run buttons */}
      {job && (
        <div className="rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
          <div className="mb-1.5 truncate font-mono text-[0.6875rem] text-rex-text-muted">
            {job.dirName} · {job.url}
            {job.gitRef ? ` @ ${job.gitRef}` : ""}
          </div>
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0 space-y-1">
              {job.steps.map((s) => (
                <div key={s.key} className="flex items-center gap-2">
                  <StepDot status={s.status} />
                  <span className="font-mono text-[0.75rem] text-rex-text">{s.label}</span>
                  {s.status === "cancelled" && (
                    <span className="text-[0.6875rem] text-rex-text-muted">cancelled</span>
                  )}
                </div>
              ))}
            </div>
            <div className="flex flex-none items-center gap-2">
              {stepRunning && (
                <button
                  className={BTN}
                  onClick={() => job && repoCancel(job.id).catch(toastBackendError)}
                >
                  Cancel
                </button>
              )}
              <button
                className="text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
                onClick={() => setLogOpen((v) => !v)}
              >
                {logOpen ? "Hide log" : "Show log"}
              </button>
            </div>
          </div>

          {/* Failures surface loud: the step's mapped error, verbatim. */}
          {job.steps
            .filter((s) => s.status === "failed" && s.error)
            .map((s) => (
              <div
                key={`err-${s.key}`}
                className="mt-2 whitespace-pre-line rounded-md border border-status-error-border bg-status-error-bg px-2.5 py-1.5 font-mono text-[0.6875rem] text-status-error-bright"
              >
                {s.error}
              </div>
            ))}

          {job.nodeWarning && (
            <div className="mt-2 rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] text-status-warning-bright">
              {job.nodeWarning}
            </div>
          )}
          {wpMismatch && (
            <div className="mt-2 rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] text-status-warning-bright">
              This repo looks like a {job.inspection?.wp.kind}, but you're adding it as a {kind}.
            </div>
          )}

          {/* Explicit steps — the consent moment. */}
          {runnable.length > 0 && (
            <div className="mt-2 space-y-1.5">
              <div className="text-[0.6875rem] text-rex-text-muted">
                These run the repo's own scripts (npm postinstall, composer scripts) as your user —
                same as running them in Terminal. Install only code you trust.
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {runnable.map((s) => (
                  <button
                    key={`run-${s.key}`}
                    className={BTN}
                    disabled={stepRunning || s.status === "ok"}
                    onClick={() => runStep.mutate(s.key)}
                  >
                    {s.status === "ok" ? `✓ ${s.label}` : s.label}
                  </button>
                ))}
                {job.finishedOk && !activated && (
                  <button
                    className={BTN + " border-brand"}
                    disabled={activate.isPending}
                    onClick={() => activate.mutate()}
                  >
                    {activate.isPending ? "Activating…" : `Activate ${kind}`}
                  </button>
                )}
                {activated && (
                  <span className="text-[0.75rem] text-status-running-bright">✓ activated</span>
                )}
              </div>
            </div>
          )}

          {logOpen && (
            <div
              ref={logRef}
              className="mt-2 max-h-[220px] overflow-y-auto rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1.5"
            >
              {lines.length === 0 ? (
                <div className="font-mono text-[0.6875rem] text-rex-text-muted">(no output yet)</div>
              ) : (
                lines.map((l, i) => (
                  <div
                    key={i}
                    className="whitespace-pre-wrap break-all font-mono text-[0.6875rem] leading-[1.5] text-rex-text-muted"
                  >
                    {l}
                  </div>
                ))
              )}
            </div>
          )}

          {job.finishedOk && (
            <button
              className="mt-2 flex items-center gap-1 text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
              onClick={() => {
                setJob(null);
                setUrl("");
                probe.reset();
              }}
            >
              <X className="h-3 w-3" /> Done — add another
            </button>
          )}
        </div>
      )}
    </div>
  );
}
