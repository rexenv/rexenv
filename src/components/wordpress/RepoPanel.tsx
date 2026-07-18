/** Per-asset repo panel: live checkout state (phase A) + git ops (phase B —
 *  Fetch / Pull --ff-only / Checkout / Push, each a streamed cancellable job
 *  on the shared runner). Ops-glue, deliberately NOT a git client: no
 *  commit/stage/merge UI — a diverged branch is an honest error pointing at
 *  the editor/terminal. A pull/checkout that changes lockfiles OFFERS
 *  install/build steps right here (explicit clicks, disclosure shown). */
import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import {
  onRepoJobOutput,
  onRepoJobState,
  onRepoWatchOutput,
  onRepoWatchState,
  repoAssetStatus,
  repoBranches,
  repoCancel,
  repoGitOp,
  repoRunStep,
  repoScriptJob,
  repoScripts,
  repoSiteJobs,
  repoWatchLog,
  repoWatchStart,
  repoWatchStop,
  repoWatches,
  tailLog,
} from "@/lib/ipc";
import type { GitAsset, RepoJobState } from "@/types";
import { toastBackendError } from "@/lib/toast";
import { LogPane, mergeTailAndStreamed, REPO_SCRIPTS_DISCLOSURE, StepDot } from "./repoJobUi";

const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

const LOG_CAP = 500;

function Chip({ children, tone }: { children: React.ReactNode; tone?: "warn" | "ok" }) {
  const color =
    tone === "warn"
      ? "text-status-warning-bright"
      : tone === "ok"
        ? "text-status-running-bright"
        : "text-rex-text-muted";
  return (
    <span className={`rounded-full bg-rex-surface-2 px-2 py-0.5 font-mono text-[0.6875rem] ${color}`}>
      {children}
    </span>
  );
}

export function RepoPanel({
  siteId,
  kind,
  asset,
}: {
  siteId: string;
  kind: "plugin" | "theme";
  asset: GitAsset;
}) {
  const qc = useQueryClient();
  const [logOpen, setLogOpen] = useState(false);
  const [opJob, setOpJob] = useState<RepoJobState | null>(null);
  const [opLines, setOpLines] = useState<string[]>([]);
  const [opLogOpen, setOpLogOpen] = useState(false);
  const [checkoutRef, setCheckoutRef] = useState("");
  const opLogRef = useRef<HTMLDivElement | null>(null);
  const adoptedRef = useRef(false);
  const jobsKey = ["repo-jobs", siteId, kind] as const;
  const statusKey = ["repo-status", siteId, kind, asset.dirName] as const;
  const branchesKey = ["repo-branches", siteId, kind, asset.dirName] as const;

  const status = useQuery({
    queryKey: statusKey,
    queryFn: () => repoAssetStatus(siteId, kind, asset.dirName),
    staleTime: 10_000,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const branches = useQuery({
    queryKey: branchesKey,
    queryFn: () => repoBranches(siteId, kind, asset.dirName),
    staleTime: 30_000,
    refetchOnWindowFocus: false,
    retry: false,
  });
  // Scripts + the asset's watcher (phase C).
  const scriptsQ = useQuery({
    queryKey: ["repo-scripts", siteId, kind, asset.dirName],
    queryFn: () => repoScripts(siteId, kind, asset.dirName),
    staleTime: 30_000,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const watchesQ = useQuery({
    queryKey: ["repo-watches", siteId, kind],
    queryFn: () => repoWatches(siteId, kind),
    staleTime: 5_000,
    refetchOnWindowFocus: false,
  });
  const myWatch = (watchesQ.data ?? []).find((w) => w.dirName === asset.dirName) ?? null;
  const [watchLines, setWatchLines] = useState<string[]>([]);
  const [watchLogOpen, setWatchLogOpen] = useState(false);
  const watchLogRef = useRef<HTMLDivElement | null>(null);
  const log = useQuery({
    queryKey: ["repo-status-log", siteId, kind, asset.dirName],
    queryFn: () => tailLog(status.data?.logKey ?? "", 200),
    enabled: logOpen && !!status.data?.logKey,
    staleTime: 0,
  });

  // Reconnect to a live/unfinished OP job for THIS dir after a remount (the
  // add panel owns op === "add"; we own the rest). Same shared query cache.
  const siteJobs = useQuery({
    queryKey: jobsKey,
    queryFn: () => repoSiteJobs(siteId, kind),
    refetchOnWindowFocus: false,
    staleTime: 5_000,
  });
  useEffect(() => {
    if (adoptedRef.current || opJob !== null) return;
    const candidate = [...(siteJobs.data ?? [])]
      .reverse()
      .find((j) => j.op !== "add" && j.dirName === asset.dirName && !j.finishedOk);
    if (!candidate) return;
    adoptedRef.current = true;
    setOpJob(candidate);
    setOpLogOpen(true);
    void tailLog(candidate.logKey, 300)
      .then((tail) => setOpLines((streamed) => mergeTailAndStreamed(tail, streamed)))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [siteJobs.data, opJob, asset.dirName]);

  // Live subscriptions for the op job.
  useEffect(() => {
    if (!opJob?.id) return;
    let dead = false;
    const un: Array<() => void> = [];
    void onRepoJobState(opJob.id, (s) => {
      if (dead) return;
      setOpJob(s);
      qc.setQueryData(jobsKey, (old: RepoJobState[] | undefined) =>
        old ? old.map((j) => (j.id === s.id ? s : j)) : old,
      );
      // Op settled → the panel header + provenance row may have changed.
      if (s.steps.every((st) => st.status !== "running")) {
        qc.invalidateQueries({ queryKey: statusKey });
        qc.invalidateQueries({ queryKey: branchesKey });
        qc.invalidateQueries({ queryKey: ["repo-assets", siteId] });
      }
    }).then((u) => un.push(u));
    void onRepoJobOutput(opJob.id, (line) => {
      if (!dead) setOpLines((l) => [...l.slice(-(LOG_CAP - 1)), line]);
    }).then((u) => un.push(u));
    return () => {
      dead = true;
      un.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opJob?.id]);

  useEffect(() => {
    const el = opLogRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [opLines, opLogOpen]);

  // Watcher subscriptions: live lines + ring-buffer seed on (re)mount.
  useEffect(() => {
    if (!myWatch?.id) return;
    let dead = false;
    const un: Array<() => void> = [];
    void onRepoWatchState(myWatch.id, () => {
      if (!dead) qc.invalidateQueries({ queryKey: ["repo-watches", siteId, kind] });
    }).then((u) => un.push(u));
    void onRepoWatchOutput(myWatch.id, (line) => {
      if (!dead) setWatchLines((l) => [...l.slice(-(LOG_CAP - 1)), line]);
    }).then((u) => un.push(u));
    void repoWatchLog(myWatch.id)
      .then((ring) => {
        if (!dead) setWatchLines((streamed) => mergeTailAndStreamed(ring, streamed));
      })
      .catch(() => {});
    return () => {
      dead = true;
      un.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [myWatch?.id]);
  useEffect(() => {
    const el = watchLogRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [watchLines, watchLogOpen]);

  const runOp = useMutation({
    mutationFn: (args: { op: "fetch" | "pull" | "checkout" | "push"; ref?: string }) =>
      repoGitOp(siteId, kind, asset.dirName, args.op, args.ref ?? null),
    onSuccess: (snap) => {
      adoptedRef.current = true;
      setOpJob(snap);
      setOpLines([]);
      setOpLogOpen(true);
      qc.setQueryData(jobsKey, (old: RepoJobState[] | undefined) =>
        old ? [...old.filter((j) => j.id !== snap.id), snap] : [snap],
      );
    },
    onError: (e) => toastBackendError(e),
  });
  const runStep = useMutation({
    mutationFn: (stepKey: string) => repoRunStep(opJob?.id ?? "", stepKey),
    onError: (e) => toastBackendError(e),
  });
  const runScript = useMutation({
    mutationFn: (script: string) => repoScriptJob(siteId, kind, asset.dirName, script),
    onSuccess: (snap) => {
      adoptedRef.current = true;
      setOpJob(snap);
      setOpLines([]);
      setOpLogOpen(true);
      qc.setQueryData(jobsKey, (old: RepoJobState[] | undefined) =>
        old ? [...old.filter((j) => j.id !== snap.id), snap] : [snap],
      );
    },
    onError: (e) => toastBackendError(e),
  });
  const startWatch = useMutation({
    mutationFn: (script: string) => repoWatchStart(siteId, kind, asset.dirName, script),
    onSuccess: () => {
      setWatchLines([]);
      setWatchLogOpen(true);
      qc.invalidateQueries({ queryKey: ["repo-watches", siteId, kind] });
    },
    onError: (e) => toastBackendError(e),
  });
  const stopWatch = useMutation({
    mutationFn: (id: string) => repoWatchStop(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["repo-watches", siteId, kind] }),
    onError: (e) => toastBackendError(e),
  });

  const s = status.data;
  const headRef = s
    ? s.unborn
      ? "no commits yet"
      : s.detached
        ? "detached HEAD"
        : (s.branch ?? "?")
    : null;
  const clean = s ? s.changed === 0 && s.untracked === 0 : false;
  const opRunning = opJob?.steps.some((st) => st.status === "running") ?? false;
  const opsDisabled = opRunning || runOp.isPending;
  const offeredSteps = useMemo(
    () =>
      (opJob?.steps ?? []).filter(
        (st) => !["fetch", "pull", "checkout", "push"].includes(st.key),
      ),
    [opJob],
  );
  const branchOptions = useMemo(() => {
    const local = branches.data?.local ?? [];
    const remoteShort = (branches.data?.remote ?? [])
      .map((r) => r.replace(/^origin\//, ""))
      .filter((r) => !local.includes(r));
    return { local, remoteShort };
  }, [branches.data]);

  useEffect(() => {
    if (checkoutRef === "" && branches.data?.current) setCheckoutRef(branches.data.current);
  }, [branches.data, checkoutRef]);

  return (
    <div className="mx-3 mb-2.5 rounded-lg border border-rex-border bg-rex-surface-2/50 px-3 py-2.5">
      {status.isLoading ? (
        <div className="flex items-center gap-2 text-[0.75rem] text-rex-text-muted">
          <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> Reading checkout state…
        </div>
      ) : status.isError ? (
        <div className="whitespace-pre-line font-mono text-[0.6875rem] text-status-error-bright">
          {String(status.error)}
        </div>
      ) : s ? (
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <Chip tone={s.detached ? "warn" : undefined}>⎇ {headRef}</Chip>
            <Chip tone={clean ? "ok" : "warn"}>
              {clean
                ? "clean"
                : [
                    s.changed > 0 ? `${s.changed} changed` : null,
                    s.untracked > 0 ? `${s.untracked} untracked` : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
            </Chip>
            {s.upstream ? (
              <Chip>
                ↑{s.ahead ?? 0} ↓{s.behind ?? 0} vs {s.upstream}
              </Chip>
            ) : (
              !s.unborn && <Chip tone="warn">no upstream</Chip>
            )}
            <Chip>{asset.source}</Chip>
            <button
              className="ml-auto flex items-center gap-1 text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
              onClick={() => {
                qc.invalidateQueries({ queryKey: statusKey });
                qc.invalidateQueries({ queryKey: branchesKey });
              }}
              title="Re-read git status"
            >
              <RefreshCw className="h-3 w-3" /> Refresh
            </button>
          </div>
          <div className="truncate font-mono text-[0.6875rem] text-rex-text-dim">
            {s.remote ?? (asset.url || "(no remote)")}
            {asset.gitRef ? ` · added @ ${asset.gitRef}` : ""}
          </div>

          {/* Git ops — jobs on the shared runner, one at a time per dir. */}
          <div className="flex flex-wrap items-center gap-2">
            <button
              className={BTN}
              disabled={opsDisabled}
              onClick={() => runOp.mutate({ op: "fetch" })}
            >
              Fetch
            </button>
            <button
              className={BTN}
              disabled={opsDisabled}
              onClick={() => runOp.mutate({ op: "pull" })}
              title="git pull --ff-only — never merges for you"
            >
              Pull
            </button>
            <button
              className={BTN}
              disabled={opsDisabled}
              onClick={() => runOp.mutate({ op: "push" })}
              title="git push (sets upstream automatically when missing; never force)"
            >
              Push
            </button>
            <select
              value={checkoutRef}
              onChange={(e) => setCheckoutRef(e.target.value)}
              disabled={opsDisabled}
              className="h-[28px] max-w-[200px] rounded border border-rex-border bg-rex-surface-2 px-1.5 font-mono text-[0.71875rem] text-rex-text outline-none focus:border-brand"
              aria-label="Checkout target"
            >
              {branchOptions.local.map((b) => (
                <option key={`l-${b}`} value={b}>
                  {b}
                  {b === branches.data?.current ? " (current)" : ""}
                </option>
              ))}
              {branchOptions.remoteShort.length > 0 && (
                <optgroup label="Remote">
                  {branchOptions.remoteShort.map((b) => (
                    <option key={`r-${b}`} value={b}>
                      {b}
                    </option>
                  ))}
                </optgroup>
              )}
            </select>
            <button
              className={BTN}
              disabled={
                opsDisabled || checkoutRef === "" || checkoutRef === branches.data?.current
              }
              onClick={() => runOp.mutate({ op: "checkout", ref: checkoutRef })}
            >
              Checkout
            </button>
          </div>

          {/* The op job: step(s) + offered install steps + streamed log. */}
          {opJob && (
            <div className="rounded-md border border-rex-border bg-rex-surface-1 px-2.5 py-2">
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0 space-y-1">
                  {opJob.steps.map((st) => (
                    <div key={st.key} className="flex items-center gap-2">
                      <StepDot status={st.status} />
                      <span className="font-mono text-[0.75rem] text-rex-text">{st.label}</span>
                    </div>
                  ))}
                </div>
                <div className="flex flex-none items-center gap-2">
                  {opRunning && (
                    <button
                      className={BTN}
                      onClick={() => opJob && repoCancel(opJob.id).catch(toastBackendError)}
                    >
                      Cancel
                    </button>
                  )}
                  <button
                    className="text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
                    onClick={() => setOpLogOpen((v) => !v)}
                  >
                    {opLogOpen ? "Hide log" : "Show log"}
                  </button>
                </div>
              </div>
              {opJob.steps
                .filter((st) => st.status === "failed" && st.error)
                .map((st) => (
                  <div
                    key={`err-${st.key}`}
                    className="mt-2 whitespace-pre-line rounded-md border border-status-error-border bg-status-error-bg px-2.5 py-1.5 font-mono text-[0.6875rem] text-status-error-bright"
                  >
                    {st.error}
                  </div>
                ))}
              {offeredSteps.length > 0 && (
                <div className="mt-2 space-y-1.5">
                  <div className="text-[0.6875rem] text-status-warning-bright">
                    Dependencies changed with this {opJob.op} — re-install below.
                  </div>
                  <div className="text-[0.6875rem] text-rex-text-muted">
                    {REPO_SCRIPTS_DISCLOSURE}
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    {offeredSteps.map((st) => (
                      <button
                        key={`run-${st.key}`}
                        className={BTN}
                        disabled={opRunning || st.status === "ok"}
                        onClick={() => runStep.mutate(st.key)}
                      >
                        {st.status === "ok" ? `✓ ${st.label}` : st.label}
                      </button>
                    ))}
                  </div>
                </div>
              )}
              {opLogOpen && <LogPane lines={opLines} innerRef={opLogRef} />}
            </div>
          )}

          {(scriptsQ.data?.scripts.length ?? 0) > 0 && (
            <div className="space-y-1.5">
              <div className="text-[0.6875rem] text-rex-text-muted">
                {REPO_SCRIPTS_DISCLOSURE}
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {(scriptsQ.data?.scripts ?? []).map((sc) =>
                  sc.watchy ? (
                    <button
                      key={sc.name}
                      className={BTN}
                      title={sc.command}
                      disabled={startWatch.isPending || myWatch?.status === "running"}
                      onClick={() => startWatch.mutate(sc.name)}
                    >
                      Watch: {sc.name}
                    </button>
                  ) : (
                    <button
                      key={sc.name}
                      className={BTN}
                      title={sc.command}
                      disabled={opsDisabled}
                      onClick={() => runScript.mutate(sc.name)}
                    >
                      Run: {sc.name}
                    </button>
                  ),
                )}
              </div>
              {myWatch && (
                <div className="rounded-md border border-rex-border bg-rex-surface-1 px-2.5 py-2">
                  <div className="flex items-center gap-2">
                    {myWatch.status === "running" ? (
                      <>
                        <span className="h-1.5 w-1.5 flex-none rounded-full bg-status-running" />
                        <span className="font-mono text-[0.75rem] text-rex-text">
                          watching — {myWatch.script}
                        </span>
                      </>
                    ) : (
                      <span className="font-mono text-[0.75rem] text-status-error-bright">
                        watcher exited{myWatch.exit != null ? ` (code ${myWatch.exit})` : ""}
                      </span>
                    )}
                    <div className="ml-auto flex items-center gap-2">
                      {myWatch.status === "running" ? (
                        <button
                          className={BTN}
                          disabled={stopWatch.isPending}
                          onClick={() => stopWatch.mutate(myWatch.id)}
                        >
                          Stop
                        </button>
                      ) : (
                        <button
                          className={BTN}
                          disabled={startWatch.isPending}
                          onClick={() => startWatch.mutate(myWatch.script)}
                        >
                          Restart
                        </button>
                      )}
                      <button
                        className="text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
                        onClick={() => setWatchLogOpen((v) => !v)}
                      >
                        {watchLogOpen ? "Hide output" : "Show output"}
                      </button>
                    </div>
                  </div>
                  {watchLogOpen && <LogPane lines={watchLines} innerRef={watchLogRef} />}
                </div>
              )}
            </div>
          )}

          {s.logKey && (
            <button
              className="text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
              onClick={() => setLogOpen((v) => !v)}
            >
              {logOpen ? "Hide last job log" : "Show last job log"}
            </button>
          )}
          {logOpen && (
            <div className="max-h-[180px] overflow-y-auto rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1.5">
              {(log.data ?? []).length === 0 ? (
                <div className="font-mono text-[0.6875rem] text-rex-text-muted">(empty)</div>
              ) : (
                (log.data ?? []).map((l, i) => (
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
        </div>
      ) : null}
    </div>
  );
}
