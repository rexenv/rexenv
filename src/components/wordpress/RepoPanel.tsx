/** Per-asset repo panel: live checkout state (phase A) + git ops (phase B —
 *  Fetch / Pull --ff-only / Checkout / Push, each a streamed cancellable job
 *  on the shared runner). Ops-glue, deliberately NOT a git client: no
 *  commit/stage/merge UI — a diverged branch is an honest error pointing at
 *  the editor/terminal. A pull/checkout that changes lockfiles OFFERS
 *  install/build steps right here (explicit clicks, disclosure shown).
 *
 *  The second row is the working tree: Status / Stash / Restore / Reset. It is
 *  the way OUT of the state every op in the first row refuses on — and the
 *  dirt is usually rexenv's own, left by a composer/npm step started from this
 *  panel. Still not a git client: nothing here stages, commits or merges. */
import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import { confirm } from "@/components/ui/dialog";
import {
  onRepoJobOutput,
  onRepoJobState,
  onRepoWatchOutput,
  onRepoWatchState,
  repoAssetStatus,
  repoBranches,
  repoCancel,
  repoCheck,
  repoDistArchive,
  repoGitOp,
  repoJobState,
  repoPullRefs,
  repoRunOfferedSteps,
  repoRunStep,
  repoScriptJob,
  repoScripts,
  repoSiteJobs,
  repoStashes,
  repoWatchLog,
  repoWatchStart,
  repoWatchStop,
  repoWatches,
  tailLog,
} from "@/lib/ipc";
import type { RepoGitOp, RepoJobState, RepoKind, RepoStepState } from "@/types";
import { revealPath } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { toast, toastBackendError } from "@/lib/toast";
import { LogPane, mergeTailAndStreamed, REPO_SCRIPTS_DISCLOSURE, StepDot } from "./repoJobUi";
import { RefPicker } from "./RefPicker";

const BTN =
  "inline-flex items-center gap-1.5 rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

const LOG_CAP = 500;

/** The ONLY steps the offered row may render: the three a job can APPEND after
 *  detecting changed dependencies. It was a blacklist of op-step keys, so every
 *  op added later leaked into the row — "archive" rendered a bogus
 *  "Dependencies changed with this dist-archive — re-install below" plus a
 *  "Run all" / "wp dist-archive" pair under Build zip. A whitelist cannot leak:
 *  keys live in `commands/repo.rs` (`step("composer"|"install"|"build", …)`). */
const OFFERABLE_STEPS = ["composer", "install", "build"];

/** Zip toasts already announced, keyed by job id — module scope ON PURPOSE.
 *  The panel unmounts whenever the asset row is collapsed, so a per-mount ref
 *  re-announced the same zip on every re-expand: a one-step job never sets
 *  `finishedOk` (backend requires ≥2 steps), so the finished archive job is
 *  re-adopted on each mount and its `archive` field arrives again. */
const toastedArchives = new Set<string>();

/** Step outcomes already announced, keyed `<jobId>:<stepKey>` — module scope for
 *  the same reason as `toastedArchives`: collapsing and re-expanding the asset
 *  row unmounts this panel, and a per-mount ref would re-announce a job the
 *  user already saw settle. */
const toastedSteps = new Set<string>();

/** What to CALL the thing that just finished, in a toast the user reads with the
 *  panel possibly scrolled out of view. The op's own step (its key equals the
 *  job's op, by backend contract) gets the button's own word; a dependency step
 *  gets its real command ("composer install"), because that is what the row it
 *  came from says. */
function actionLabel(job: RepoJobState, step: RepoStepState): string {
  if (step.key !== job.op) return step.label;
  switch (job.op) {
    case "fetch":
      return "Fetch";
    case "pull":
      return "Pull";
    case "push":
      return "Push";
    case "checkout":
      return job.gitRef ? `Checkout ${job.gitRef}` : "Checkout";
    case "stash":
      return "Stash";
    case "stash-pop":
      return job.gitRef ? `Restore ${job.gitRef}` : "Restore";
    case "reset":
      return "Reset";
    case "status":
      return "Status";
    case "check":
      return "Dependency check";
    default:
      return step.label; // scripts: "pnpm run build"; anything new: its own label
  }
}

/** Inline job spinner for a button whose start call is still in flight. */
function BtnSpinner() {
  return <Loader2 className="h-3 w-3 animate-rex-spin" />;
}

/** Archive-button copy, kept together because it is guarded as a unit
 *  (`core/dist_archive.rs`, the copy guard). The two load-bearing clauses are
 *  "Nothing is written into the checkout" and "would report that as a success":
 *  the first is what makes the button safe to click on a folder the user cares
 *  about, the second is why it refuses rather than warns. They are the two a
 *  later trim removes first. */
const ARCHIVE_TITLE =
  "wp dist-archive — builds a distributable zip from .distignore and saves it to Downloads. Nothing is written into the checkout.";
const ARCHIVE_BLOCKED_TITLE =
  "No .distignore in this checkout — without one the zip would include .git and node_modules, and dist-archive would report that as a success. Add a .distignore file at the top of this checkout (.gitignore syntax) listing what must not ship.";
const ARCHIVE_BUSY_TITLE = "another job is running for this checkout";

/** Working-tree copy, kept together because the two halves are guarded as a
 *  unit (CLAIM-LEDGER #310/#311). The load-bearing clauses are what these
 *  actions DON'T take: a stash never touches ignored paths (`vendor/`,
 *  `node_modules/` — the installs someone waited minutes for), and Reset
 *  deletes no untracked file, because for those there is no stash, no reflog
 *  and no remote to get them back from. Both are the first lines a later trim
 *  would cut, and both are why these buttons are safe to click. */
const STASH_TITLE =
  "git stash push -u — parks your uncommitted work, INCLUDING new files, so a checkout can proceed. Restore brings it back. Ignored paths (vendor/, node_modules/) are never stashed.";
const RESET_TITLE =
  "git reset --hard HEAD — throws your tracked changes away for good. Untracked files are kept, and ignored paths are untouched.";
const NO_VERSION_NOTE =
  "no version found in the plugin header, style.css or composer.json, so the name carries none";

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

/** What the panel points at. `GitAsset` satisfies this structurally; a SITE's
 *  own checkout supplies the same four display fields with `dirName` set to the
 *  domain — display text the backend never turns into a path. */
export type RepoPanelTarget = {
  dirName: string;
  url: string;
  gitRef: string | null;
  /** Provenance chip: "cloned" | "adopted" | "linked". */
  source: string;
};

export function RepoPanel({
  siteId,
  kind,
  asset,
}: {
  siteId: string;
  kind: RepoKind;
  asset: RepoPanelTarget;
}) {
  // `wp dist-archive` packages a PLUGIN or THEME. A site's project root is not
  // a distributable, so the button is absent rather than disabled — the
  // disabled-with-a-reason treatment below teaches someone who could fix it,
  // and here there is nothing to fix.
  const canArchive = kind !== "site";
  const qc = useQueryClient();
  const [logOpen, setLogOpen] = useState(false);
  const [opJob, setOpJob] = useState<RepoJobState | null>(null);
  const [opLines, setOpLines] = useState<string[]>([]);
  const [opLogOpen, setOpLogOpen] = useState(false);
  const [checkoutRef, setCheckoutRef] = useState("");
  const [restoreRef, setRestoreRef] = useState("");
  const opLogRef = useRef<HTMLDivElement | null>(null);
  const adoptedRef = useRef(false);
  const jobsKey = ["repo-jobs", siteId, kind] as const;
  const statusKey = ["repo-status", siteId, kind, asset.dirName] as const;
  const branchesKey = ["repo-branches", siteId, kind, asset.dirName] as const;
  const stashKey = ["repo-stashes", siteId, kind, asset.dirName] as const;

  // Branch, dirtiness and ahead/behind are things a TERMINAL changes while this
  // panel is open — `git checkout` outside rexenv left this showing the old
  // branch until the user left the tab and came back. Both reads are local git
  // (no network), so they re-run whenever the native window regains focus
  // (`lib/window-focus.ts`) — the moment the user returns from that terminal.
  // The network read below (ls-remote for PR refs) deliberately does NOT.
  const status = useQuery({
    queryKey: statusKey,
    queryFn: () => repoAssetStatus(siteId, kind, asset.dirName),
    staleTime: 0,
    refetchOnWindowFocus: true,
    retry: false,
  });
  const branches = useQuery({
    queryKey: branchesKey,
    queryFn: () => repoBranches(siteId, kind, asset.dirName),
    staleTime: 0,
    refetchOnWindowFocus: true,
    retry: false,
  });
  // Stashes move under this panel for the same reason branches do — `git stash
  // pop` in a terminal, and worse, git RENUMBERS the list on every pop, so a
  // held `stash@{1}` would name a different entry than the row showing it.
  // Local git, no network: same focus refetch, never cached.
  const stashes = useQuery({
    queryKey: stashKey,
    queryFn: () => repoStashes(siteId, kind, asset.dirName),
    staleTime: 0,
    refetchOnWindowFocus: true,
    retry: false,
  });
  // PR/MR refs are a NETWORK call (ls-remote) — fetched lazily, only after
  // the picker has been opened at least once; never re-fired on focus.
  const [pickerOpened, setPickerOpened] = useState(false);
  const prs = useQuery({
    queryKey: ["repo-prs", siteId, kind, asset.dirName],
    queryFn: () => repoPullRefs(siteId, kind, asset.dirName),
    enabled: pickerOpened,
    staleTime: 60_000,
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
    const jobId = opJob.id;
    const jobLogKey = opJob.logKey;
    let dead = false;
    // Set by the first live state event, so the catch-up snapshot below can
    // never overwrite a newer fact with an older read.
    let sawEvent = false;
    const un: Array<() => void> = [];
    void onRepoJobState(jobId, (s) => {
      if (dead) return;
      sawEvent = true;
      setOpJob(s);
      qc.setQueryData(jobsKey, (old: RepoJobState[] | undefined) =>
        old ? old.map((j) => (j.id === s.id ? s : j)) : old,
      );
      // Op settled → the panel header + provenance row may have changed.
      if (s.steps.every((st) => st.status !== "running")) {
        qc.invalidateQueries({ queryKey: statusKey });
        qc.invalidateQueries({ queryKey: branchesKey });
        qc.invalidateQueries({ queryKey: stashKey });
        qc.invalidateQueries({ queryKey: ["repo-assets", siteId] });
      }
    }).then((u) => un.push(u));
    void onRepoJobOutput(jobId, (line) => {
      if (!dead) setOpLines((l) => [...l.slice(-(LOG_CAP - 1)), line]);
    }).then((u) => un.push(u));
    // Catch-up: Tauri does not replay events, and listener registration is
    // async — everything the job emitted between the spawn and the attach is
    // gone. A short job (dist-archive, an up-to-date fetch) fits ENTIRELY in
    // that window, which left its step frozen at pending "○" with an empty log:
    // the build looked like it had never started, and the pending step then
    // rendered the offered row too. Re-read the authoritative snapshot + log
    // tail once attached; a live event that beat us here wins.
    void repoJobState(jobId)
      .then((s) => {
        if (dead || sawEvent) return;
        setOpJob(s);
        qc.setQueryData(jobsKey, (old: RepoJobState[] | undefined) =>
          old ? old.map((j) => (j.id === s.id ? s : j)) : old,
        );
        // Same settle-invalidation as the event path — the missed event was
        // often the LAST one, which is exactly the one that refreshes header
        // and provenance.
        if (s.steps.every((st) => st.status !== "running")) {
          qc.invalidateQueries({ queryKey: statusKey });
          qc.invalidateQueries({ queryKey: branchesKey });
          qc.invalidateQueries({ queryKey: stashKey });
          qc.invalidateQueries({ queryKey: ["repo-assets", siteId] });
        }
      })
      .catch(() => {});
    if (jobLogKey) {
      void tailLog(jobLogKey, 300)
        .then((tail) => {
          if (!dead) setOpLines((streamed) => mergeTailAndStreamed(tail, streamed));
        })
        .catch(() => {});
    }
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

  // Every button in this panel starts a job that finishes somewhere else: the
  // card's step glyph flips, and that is all. With the panel scrolled away —
  // or the user reading the site in a browser — a Pull, a Push or a Run: build
  // completed in total silence, which is indistinguishable from nothing having
  // happened. Announce each step's OUTCOME once, as it settles.
  useEffect(() => {
    if (!opJob) return;
    for (const st of opJob.steps) {
      // Not finished yet — and "skipped" (never ran, because an earlier
      // Run-all step failed) is not an outcome worth a toast of its own; the
      // failure that caused it already got one.
      if (st.status === "pending" || st.status === "running" || st.status === "skipped") continue;
      const seen = `${opJob.id}:${st.key}`;
      if (toastedSteps.has(seen)) continue;
      toastedSteps.add(seen);
      // A successful zip is announced by the archive effect below, with the
      // file name that really exists and a Show in Finder action — a second
      // "finished" toast would say less, twice. A FAILED zip has no file, so
      // it still belongs here.
      if (opJob.op === "dist-archive" && st.status === "ok" && opJob.archive) continue;
      const what = `${actionLabel(opJob, st)} — ${asset.dirName}`;
      if (st.status === "failed") {
        // First line only: the full text is in the job card and the log pane,
        // and a toast that spans the window is a toast nobody finishes reading.
        const why = st.error?.split("\n").find((l) => l.trim() !== "");
        toast.error(why ? `${what} failed: ${why.trim()}` : `${what} failed`);
      } else if (st.status === "cancelled") {
        toast.info(`${what} cancelled`);
      } else {
        toast.success(`${what} finished`);
      }
    }
  }, [opJob, asset.dirName]);

  // The archive result arrives on the job-state event, not from the mutation —
  // the mutation returns the moment the job STARTS. Keyed by job id in a
  // module-level set so neither a second state emission NOR a remount (collapse
  // + re-expand of the asset row) can announce the same zip twice.
  useEffect(() => {
    const archive = opJob?.archive;
    if (!opJob || opJob.op !== "dist-archive" || !archive) return;
    if (toastedArchives.has(opJob.id)) return;
    toastedArchives.add(opJob.id);
    // The name that actually exists — never a predicted one. For a linked
    // asset it comes from the user's own folder, and collision numbering may
    // have moved it.
    const message = archive.versionMissing
      ? `${archive.fileName} saved to Downloads — ${NO_VERSION_NOTE}.`
      : `${archive.fileName} saved to Downloads`;
    toast.success(message, {
      label: "Show in Finder",
      onClick: () => void revealPath(archive.path).catch(toastBackendError),
    });
    qc.invalidateQueries({ queryKey: statusKey });
  }, [opJob, qc, statusKey]);

  const runOp = useMutation({
    mutationFn: (args: { op: RepoGitOp; ref?: string }) =>
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
  const runAll = useMutation({
    mutationFn: () => repoRunOfferedSteps(opJob?.id ?? ""),
    onSuccess: (snap) => {
      setOpJob(snap);
      qc.setQueryData(jobsKey, (old: RepoJobState[] | undefined) =>
        old ? [...old.filter((j) => j.id !== snap.id), snap] : [snap],
      );
    },
    onError: (e) => toastBackendError(e),
  });
  const checkDeps = useMutation({
    mutationFn: () => repoCheck(siteId, kind, asset.dirName),
    onSuccess: (snap) => {
      adoptedRef.current = true;
      setOpJob(snap);
      setOpLines([]);
      setOpLogOpen(true);
      qc.setQueryData(jobsKey, (old: RepoJobState[] | undefined) =>
        old ? [...old.filter((j) => j.id !== snap.id), snap] : [snap],
      );
      // The check job settles BEFORE this returns (awaited backend) — no
      // events will arrive, so seed the report from its log tail.
      void tailLog(snap.logKey, 300)
        .then((tail) => setOpLines((streamed) => mergeTailAndStreamed(tail, streamed)))
        .catch(() => {});
    },
    onError: (e) => toastBackendError(e),
  });
  const buildZip = useMutation({
    // Only reachable for an asset — the button is not rendered for a site.
    mutationFn: () =>
      repoDistArchive(siteId, kind === "site" ? "plugin" : kind, asset.dirName),
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
        ? `detached @ ${s.detachedAt ?? "?"}`
        : (s.branch ?? "?")
    : null;
  const detachedReason = "Detached HEAD (tag or PR checkout) — check out a branch first";
  const clean = s ? s.changed === 0 && s.untracked === 0 : false;
  const stashList = stashes.data ?? [];

  // Reset is the one button here that destroys work with nothing to recover it
  // from, so the confirm states BOTH halves with the counts the panel just
  // read: what goes ("cannot be recovered" — not "are discarded", which reads
  // like a tidy-up), and what stays. The second half matters as much: without
  // it a user who wants the untracked files gone clicks Reset, believes it
  // took them, and finds out later that it didn't.
  const onReset = async () => {
    const changed = s?.changed ?? 0;
    const untracked = s?.untracked ?? 0;
    const kept =
      untracked > 0
        ? ` The ${untracked} untracked file${untracked === 1 ? "" : "s"} in this checkout ${
            untracked === 1 ? "is" : "are"
          } KEPT, and ignored paths (vendor/, node_modules/) are untouched.`
        : " Ignored paths (vendor/, node_modules/) are untouched.";
    const ok = await confirm({
      title: `Reset ${asset.dirName} to HEAD?`,
      message:
        `${changed} changed file${changed === 1 ? "" : "s"} will be thrown away and CANNOT be ` +
        `recovered — there is no stash, no reflog and no remote copy of uncommitted work.${kept}` +
        `\n\nStash instead if you might want any of it back.`,
      confirmLabel: "Reset",
      danger: true,
    });
    if (ok) runOp.mutate({ op: "reset" });
  };

  // The newest entry is the one someone almost always means; picking it for
  // them is safe because restore is explicit (a second click) — but it must be
  // re-derived, never remembered: the list renumbers on every pop, so a held
  // `stash@{1}` would point at a different entry than the row it came from.
  useEffect(() => {
    const top = stashList[0]?.reference ?? "";
    if (!stashList.some((e) => e.reference === restoreRef)) setRestoreRef(top);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stashes.data]);
  const opRunning = opJob?.steps.some((st) => st.status === "running") ?? false;
  // Which button is waiting on its OWN start call. The job card only appears
  // once the call returns, and for Build zip that call first resolves PHP +
  // the WP-CLI phar (a download, on a cold machine) — without this the click
  // looked like it did nothing at all.
  const pendingOp = runOp.isPending ? (runOp.variables?.op ?? null) : null;
  const opsDisabled = opRunning || runOp.isPending || checkDeps.isPending;
  const offeredSteps = useMemo(
    () => (opJob?.steps ?? []).filter((st) => OFFERABLE_STEPS.includes(st.key)),
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
                {/* null = UNKNOWN (upstream gone) — never render it as 0. */}
                ↑{s.ahead ?? "?"} ↓{s.behind ?? "?"} vs {s.upstream}
              </Chip>
            ) : (
              !s.unborn && !s.detached && <Chip tone="warn">no upstream</Chip>
            )}
            {/* Stashed work is INVISIBLE in a checkout — the files are gone
                from the tree and the branch looks clean. A count here is how
                someone remembers, days later, that it is parked rather than
                lost. Absent when there is none: an empty "0 stashed" chip
                teaches a feature to people who aren't using it. */}
            {stashList.length > 0 && (
              <Chip>
                {stashList.length} stashed
              </Chip>
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
              <RefreshCw
                className={cn("h-3 w-3", (status.isFetching || branches.isFetching) && "animate-spin")}
              />{" "}
              Refresh
            </button>
          </div>
          <div className="truncate font-mono text-[0.6875rem] text-rex-text-dim">
            {s.remote ?? (asset.url || "(no remote)")}
            {asset.gitRef ? ` · added @ ${asset.gitRef}` : ""}
          </div>
          {s.linkTarget && (
            <div
              className="truncate font-mono text-[0.6875rem] text-rex-text-dim"
              title="Symlink target — deleting this asset removes only the link"
            >
              → {s.linkTarget}
            </div>
          )}

          {/* Git ops — jobs on the shared runner, one at a time per dir. */}
          <div className="flex flex-wrap items-center gap-2">
            <button
              className={BTN}
              disabled={opsDisabled}
              onClick={() => runOp.mutate({ op: "fetch" })}
            >
              {pendingOp === "fetch" && <BtnSpinner />} Fetch
            </button>
            <button
              className={BTN}
              disabled={opsDisabled || s.detached}
              onClick={() => runOp.mutate({ op: "pull" })}
              title={s.detached ? detachedReason : "git pull --ff-only — never merges for you"}
            >
              {pendingOp === "pull" && <BtnSpinner />} Pull
            </button>
            <button
              className={BTN}
              disabled={opsDisabled || s.detached}
              onClick={() => runOp.mutate({ op: "push" })}
              title={
                s.detached
                  ? detachedReason
                  : "git push (sets upstream automatically when missing; never force)"
              }
            >
              {pendingOp === "push" && <BtnSpinner />} Push
            </button>
            {/* Build zip — a verb, like the rest of the row. Deliberately
                DISABLED rather than hidden when there is no .distignore:
                hiding it teaches nothing, and the person who needs this is the
                one who has never heard of the file. */}
            {canArchive && (
              <button
                className={BTN}
                disabled={opsDisabled || buildZip.isPending || !s.hasDistignore}
                onClick={() => buildZip.mutate()}
                title={
                  !s.hasDistignore
                    ? ARCHIVE_BLOCKED_TITLE
                    : opsDisabled || buildZip.isPending
                      ? ARCHIVE_BUSY_TITLE
                      : ARCHIVE_TITLE
                }
              >
                {buildZip.isPending && <BtnSpinner />} Build zip
              </button>
            )}
            <RefPicker
              value={checkoutRef}
              onChange={setCheckoutRef}
              disabled={opsDisabled}
              ariaLabel="Checkout target"
              onOpenChange={(o) => o && setPickerOpened(true)}
              groups={[
                {
                  label: null,
                  items: branchOptions.local.map((b) => ({
                    value: b,
                    hint: b === branches.data?.current ? "current" : undefined,
                  })),
                },
                {
                  label: "Remote",
                  items: branchOptions.remoteShort.map((b) => ({ value: b })),
                },
                {
                  label: "Tags",
                  // Full ref on purpose: unambiguous vs a same-named branch,
                  // resolves exactly → detached (panel shows it honestly).
                  items: (branches.data?.tags ?? []).map((t) => ({
                    value: `refs/tags/${t}`,
                    label: t,
                  })),
                },
                {
                  label: "Pull Requests",
                  // Number + sha is all a ref carries (no titles without the
                  // host API). Checkout lands detached, same as tags.
                  items: (prs.data ?? []).map((p) => ({
                    value: p.ref,
                    label: `PR #${p.number}`,
                    hint: p.sha.slice(0, 7),
                  })),
                  note: !pickerOpened
                    ? undefined
                    : prs.isLoading
                      ? "loading pull requests…"
                      : prs.isError
                        ? "PRs unavailable (network / unsupported host)"
                        : prs.data?.length === 0
                          ? "no PR/MR refs advertised by the remote"
                          : undefined,
                },
              ]}
            />
            <button
              className={BTN}
              disabled={
                opsDisabled || checkoutRef === "" || checkoutRef === branches.data?.current
              }
              onClick={() => runOp.mutate({ op: "checkout", ref: checkoutRef })}
            >
              {pendingOp === "checkout" && <BtnSpinner />} Checkout
            </button>
            <button
              className={BTN}
              disabled={opsDisabled || checkDeps.isPending}
              onClick={() => checkDeps.mutate()}
              title="Zero-exec check: are composer/npm deps missing or stale? Runs no repo code — installs stay behind explicit clicks"
            >
              {checkDeps.isPending && <BtnSpinner />} Check deps
            </button>
          </div>

          {/* Working tree — the way OUT of the state the row above refuses on.
              Its own line on purpose: these act on UNCOMMITTED work, not on the
              remote, and one of them cannot be undone. Mixed into the fetch/pull
              row, Reset sat one tab-stop from Pull. */}
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-label">
              Working tree
            </span>
            <button
              className={BTN}
              disabled={opsDisabled}
              onClick={() => runOp.mutate({ op: "status" })}
              title="git status --short --branch — WHICH files are dirty (the chips can only count them), plus the stash list"
            >
              {pendingOp === "status" && <BtnSpinner />} Status
            </button>
            <button
              className={BTN}
              disabled={opsDisabled || clean}
              onClick={() => runOp.mutate({ op: "stash" })}
              title={
                clean
                  ? "Nothing to stash — this checkout has no uncommitted changes"
                  : STASH_TITLE
              }
            >
              {pendingOp === "stash" && <BtnSpinner />} Stash
            </button>
            <RefPicker
              value={restoreRef}
              onChange={setRestoreRef}
              disabled={opsDisabled || stashList.length === 0}
              ariaLabel="Stash entry to restore"
              placeholder="Filter stashes…"
              emptyText="No matching stash entries."
              groups={[
                {
                  label: null,
                  items: stashList.map((e) => ({
                    value: e.reference,
                    // The message is the point — "stash@{2}" alone says nothing
                    // about which afternoon's work it is.
                    label: `${e.reference} · ${e.message}`,
                    hint: e.age,
                  })),
                  note: stashList.length === 0 ? "nothing stashed in this checkout" : undefined,
                },
              ]}
            />
            <button
              className={BTN}
              disabled={opsDisabled || restoreRef === ""}
              onClick={() => runOp.mutate({ op: "stash-pop", ref: restoreRef })}
              title={
                stashList.length === 0
                  ? "Nothing stashed in this checkout"
                  : "git stash pop — puts that entry's changes back and removes it from the list. A conflict keeps the entry, so nothing is lost."
              }
            >
              {pendingOp === "stash-pop" && <BtnSpinner />} Restore
            </button>
            <button
              className={cn(BTN, "hover:border-status-error-bright hover:text-status-error-bright")}
              disabled={opsDisabled || (s?.changed ?? 0) === 0}
              onClick={() => void onReset()}
              title={
                (s?.changed ?? 0) === 0
                  ? "No tracked changes to reset"
                  : RESET_TITLE
              }
            >
              {pendingOp === "reset" && <BtnSpinner />} Reset
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
                      {st.status === "skipped" && (
                        <span className="text-[0.6875rem] text-rex-text-muted">
                          not run — earlier step failed
                        </span>
                      )}
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
                    {offeredSteps.some((st) => st.status === "pending") && (
                      <button
                        className={BTN}
                        disabled={opRunning || runAll.isPending}
                        onClick={() => runAll.mutate()}
                        title="Run the offered steps in order, one after another — stops at the first failure"
                      >
                        Run all
                      </button>
                    )}
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
