/**
 * Worktrees of one plugin/theme checkout (`docs/PLAN-git-worktrees.md`, W8).
 *
 * Rendered under the asset's RepoPanel: the list of worktree SITES made from
 * this checkout (branch and change count read from git now, never the recorded
 * request), and the dialog that makes a new one. Creating hands off to the
 * Sites page, which adopts the running provision job and shows its card — the
 * one place every create is watched.
 */
import { Fragment, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { GitBranch, Loader2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { confirm, Overlay } from "@/components/ui/dialog";
import { repoBranches, worktreeAdoptable, worktreeChildren, worktreeCreate, worktreePreview, worktreePrune, worktreeRemove, worktreeServe } from "@/lib/ipc";
import { toastBackendError } from "@/lib/toast";
import { CHECK_INPUT, cn, TECH_INPUT } from "@/lib/utils";
import type { WorktreeRequest } from "@/types";
import { RefPicker } from "./RefPicker";

type Kind = "plugin" | "theme" | "site";

const INPUT =
  "h-8 w-full rounded-md border border-rex-border bg-rex-well px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-brand";

export function AssetWorktrees({ siteId, kind, dirName }: { siteId: string; kind: Kind; dirName: string }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const children = useQuery({ queryKey: ["sites", "worktrees", siteId], queryFn: () => worktreeChildren(siteId) });
  // Worktrees of the site's own repository made elsewhere (Claude Code, a plain
  // `git worktree add`) — Shape B only (§9 Q6): a Serve button each.
  const adoptable = useQuery({ queryKey: ["sites", "worktrees-adoptable", siteId], queryFn: () => worktreeAdoptable(siteId), enabled: kind === "site" });
  // Folders git still records but that are gone (#849): one click asks git to drop
  // those records — no file is touched, so no confirm.
  const prune = useMutation({
    mutationFn: () => worktreePrune(siteId),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["sites", "worktrees-adoptable", siteId] }),
    onError: toastBackendError,
  });
  const missingCount = (adoptable.data ?? []).filter((w) => w.missing).length;
  const serve = useMutation({
    mutationFn: (path: string) => worktreeServe(siteId, path),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["sites"] });
      navigate("/sites");
    },
    onError: toastBackendError,
  });
  // A site worktree (Shape B) has no asset; a plugin/theme one names its folder.
  const mine = (children.data ?? []).filter((w) =>
    kind === "site" ? w.assetKind == null : w.assetKind === kind && w.assetDir === dirName,
  );

  const remove = useMutation({
    mutationFn: async ({ id, domain }: { id: string; domain: string }) => {
      try {
        return await worktreeRemove(id, false);
      } catch (e) {
        // Uncommitted work: the backend names the files. Ask before forcing —
        // the branch survives either way, the uncommitted files do not.
        const why = String(e);
        if (!why.includes("uncommitted")) throw e;
        const ok = await confirm({
          title: `Remove ${domain} anyway?`,
          message: why,
          confirmLabel: "Remove anyway",
          danger: true,
        });
        return ok ? worktreeRemove(id, true) : false;
      }
    },
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: ["sites", "worktrees", siteId] });
      void qc.invalidateQueries({ queryKey: ["sites"] });
    },
    onError: toastBackendError,
  });

  return (
    <div className="mx-3 mb-2.5 rounded-lg border border-rex-border bg-rex-surface-2/50 px-3 py-2.5">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 text-[0.75rem] font-medium text-rex-text">
          <GitBranch className="h-3.5 w-3.5 text-rex-text-muted" /> Worktree sites
        </div>
        <Button size="sm" variant="secondary" onClick={() => setOpen(true)}>
          New worktree…
        </Button>
      </div>
      {children.isLoading ? (
        <div className="mt-2 flex items-center gap-2 text-[0.71875rem] text-rex-text-muted">
          <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> Reading worktrees…
        </div>
      ) : mine.length === 0 ? (
        <div className="mt-1.5 text-[0.71875rem] text-rex-text-muted">
          {kind === "site"
            ? "None yet. A worktree site runs another branch of this site's repository beside it — its own folder, domain and copy of the database."
            : `None yet. A worktree site runs another branch of this ${kind} beside this one — its own domain, a copy of this site and its database.`}
        </div>
      ) : (
        <div className="mt-2 flex flex-col gap-1">
          {mine.map((w) => (
            <div key={w.siteId} className="flex items-center gap-2 text-[0.75rem]">
              <button
                type="button"
                className="truncate font-mono text-rex-accent-blue hover:underline"
                onClick={() => navigate(`/sites/${w.siteId}`)}
              >
                {w.domain}
              </button>
              <span className="rounded-full bg-rex-surface-2 px-1.5 py-0.5 font-mono text-[0.625rem] text-rex-text-muted">
                {w.present ? (w.branch ?? "detached") : `${w.askedBranch} (not checked out)`}
              </span>
              {w.uncommitted != null && w.uncommitted > 0 && (
                <span className="text-[0.6875rem] text-status-warning-bright">{w.uncommitted} uncommitted</span>
              )}
              {!w.provisioned && <span className="text-[0.6875rem] text-status-warning-bright">setup incomplete</span>}
              <span className="flex-1" />
              <button
                type="button"
                title={`Remove ${w.domain} — the branch is kept`}
                disabled={remove.isPending}
                onClick={async () => {
                  if (
                    await confirm({
                      title: `Remove ${w.domain}?`,
                      message:
                        "Its worktree, its copy of the site and its database are deleted. The branch is kept in the repository.",
                      confirmLabel: "Remove",
                      danger: true,
                    })
                  )
                    remove.mutate({ id: w.siteId, domain: w.domain });
                }}
                className="rounded p-1 text-rex-text-muted hover:text-status-error-bright disabled:opacity-40"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}
      {kind === "site" && (adoptable.data ?? []).length > 0 && (
        <div className="mt-3 border-t border-rex-border-subtle pt-2">
          <div className="flex items-center gap-2">
            <div className="text-[0.71875rem] text-rex-text-muted">Worktrees of this repository made elsewhere — serve one as a site:</div>
            <span className="flex-1" />
            {missingCount > 0 && (
              <Button size="sm" variant="ghost" disabled={prune.isPending} title="git worktree prune — drops git's record of each folder that is gone; deletes no file" onClick={() => prune.mutate()}>
                Clean up missing ({missingCount})
              </Button>
            )}
          </div>
          <div className="mt-1 flex flex-col gap-1">
            {(adoptable.data ?? []).map((w) => (
              <Fragment key={w.path}>
              <div className="flex items-center gap-2 text-[0.75rem]">
                <span className="truncate font-mono text-rex-text" title={w.path}>{w.path}</span>
                <span className="rounded-full bg-rex-surface-2 px-1.5 py-0.5 font-mono text-[0.625rem] text-rex-text-muted">{w.branch ?? "detached"}</span>
                {w.missing && <span className="text-[0.6875rem] text-status-warning-bright">folder missing</span>}
                <span className="flex-1" />
                {/* Never offer what Serve would refuse (#848): the reason, in its place. */}
                {!w.refusal && (
                  <Button size="sm" variant="secondary" disabled={w.missing || serve.isPending} onClick={() => serve.mutate(w.path)}>
                    Serve
                  </Button>
                )}
              </div>
              {w.refusal && <div className="-mt-0.5 mb-1 text-[0.6875rem] leading-[1.45] text-status-warning-bright">{w.refusal}</div>}
              </Fragment>
            ))}
          </div>
        </div>
      )}
      {open && (
        <NewWorktreeDialog
          siteId={siteId}
          kind={kind}
          dirName={dirName}
          onClose={() => setOpen(false)}
          onStarted={() => {
            setOpen(false);
            void qc.invalidateQueries({ queryKey: ["sites"] });
            navigate("/sites");
          }}
        />
      )}
    </div>
  );
}

function NewWorktreeDialog({
  siteId,
  kind,
  dirName,
  onClose,
  onStarted,
}: {
  siteId: string;
  kind: Kind;
  dirName: string;
  onClose: () => void;
  onStarted: () => void;
}) {
  const [mode, setMode] = useState<"existing" | "new">("existing");
  const [existing, setExisting] = useState("");
  const [newName, setNewName] = useState("");
  const [base, setBase] = useState("");
  const [skipUploads, setSkipUploads] = useState(false);
  const branches = useQuery({
    queryKey: ["repo-branches", siteId, kind, dirName],
    queryFn: () => repoBranches(siteId, kind, dirName),
    staleTime: 0,
  });
  const options = useMemo(() => {
    const local = branches.data?.local ?? [];
    const remote = (branches.data?.remote ?? [])
      .map((r) => r.replace(/^origin\//, ""))
      .filter((r) => !local.includes(r));
    return { local, remote };
  }, [branches.data]);
  const groups = [
    {
      label: null,
      items: options.local.map((b) => ({ value: b, hint: b === branches.data?.current ? "current" : undefined })),
    },
    { label: "Remote", items: options.remote.map((b) => ({ value: b })) },
  ];
  const baseValue = base || branches.data?.current || "";
  const asset = kind === "site" ? {} : { assetKind: kind, assetDir: dirName };
  const request: WorktreeRequest | null =
    mode === "existing"
      ? existing
        ? { parentId: siteId, ...asset, branch: existing, skipUploads }
        : null
      : newName.trim() && baseValue
        ? { parentId: siteId, ...asset, branch: newName.trim(), base: baseValue, skipUploads }
        : null;
  const preview = useQuery({
    queryKey: ["worktree-preview", request],
    queryFn: () => worktreePreview(request!),
    enabled: request != null,
    retry: false,
  });
  const create = useMutation({
    mutationFn: () => worktreeCreate(request!),
    onSuccess: onStarted,
    onError: toastBackendError,
  });
  const busy = create.isPending;

  return (
    <Overlay onClose={busy ? () => {} : onClose} cardClassName="w-[480px]">
      <div className="text-[0.9375rem] font-semibold text-rex-text">New worktree site</div>
      <div className="mt-1.5 text-[0.78125rem] leading-[1.55] text-rex-text-muted">
        {kind === "site" ? (
          <>
            Another branch of this site's repository, checked out in its own folder and served at its own domain,
            with its own copy of the database. This site is not changed.
          </>
        ) : (
          <>
            A copy of this site with <span className="font-mono text-rex-text">{dirName}</span> checked out on
            another branch, at its own domain, with its own copy of the database. This site is not changed.
          </>
        )}
      </div>

      <div className="mt-4 flex gap-4 text-[0.78125rem] text-rex-text">
        {(["existing", "new"] as const).map((m) => (
          <label key={m} className="flex cursor-pointer items-center gap-1.5">
            <input type="radio" checked={mode === m} onChange={() => setMode(m)} disabled={busy} className="accent-brand" />
            {m === "existing" ? "An existing branch" : "A new branch"}
          </label>
        ))}
      </div>

      <div className="mt-3 flex flex-col gap-2.5">
        {mode === "existing" ? (
          <RefPicker
            value={existing}
            onChange={setExisting}
            groups={groups}
            disabled={busy || branches.isLoading}
            ariaLabel="Branch to check out"
            placeholder="Pick a branch"
          />
        ) : (
          <>
            <input
              {...TECH_INPUT}
              value={newName}
              placeholder="feature/my-change"
              disabled={busy}
              onChange={(e) => setNewName(e.target.value)}
              className={INPUT}
              aria-label="New branch name"
            />
            <div className="flex items-center gap-2 text-[0.75rem] text-rex-text-muted">
              from
              <div className="flex-1">
                <RefPicker
                  value={baseValue}
                  onChange={setBase}
                  groups={groups}
                  disabled={busy || branches.isLoading}
                  ariaLabel="Base branch"
                />
              </div>
            </div>
          </>
        )}
        {kind !== "site" && (
        <label className="flex cursor-pointer items-center gap-2 text-[0.75rem] text-rex-text">
          <input
            type="checkbox"
            className={CHECK_INPUT}
            checked={skipUploads}
            disabled={busy}
            onChange={(e) => setSkipUploads(e.target.checked)}
          />
          Leave uploads out of the copy (faster; media will not load on the worktree site)
        </label>
        )}
      </div>

      <div className="mt-3 min-h-[2.5rem] rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-2 text-[0.75rem]">
        {request == null ? (
          <span className="text-rex-text-muted">Choose a branch to see the site's domain.</span>
        ) : preview.isLoading ? (
          <span className="text-rex-text-muted">Checking…</span>
        ) : preview.isError ? (
          <span className="whitespace-pre-line text-status-error-bright">{String(preview.error)}</span>
        ) : (
          <>
            <span className="font-mono text-rex-text">https://{preview.data?.domain}</span>
            {preview.data?.fallback && (
              <div className="mt-1 text-[0.6875rem] text-rex-text-muted">{preview.data.fallback}</div>
            )}
          </>
        )}
      </div>

      <div className="mt-4 flex justify-end gap-2">
        <Button variant="secondary" onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button
          variant="primary"
          disabled={busy || request == null || !preview.isSuccess}
          onClick={() => create.mutate()}
          className={cn(busy && "opacity-80")}
        >
          {busy && <Loader2 className="h-3.5 w-3.5 animate-rex-spin" />} Create worktree site
        </Button>
      </div>
    </Overlay>
  );
}
