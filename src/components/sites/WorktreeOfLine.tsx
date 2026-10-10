/**
 * "Worktree of shop.rex · feature/x" under a worktree child's domain
 * (`docs/PLAN-git-worktrees.md`, W8). The branch is git's answer now; when the
 * worktree is not there, it says what was asked for and that it is missing.
 * Renders nothing for every other site.
 */
import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { GitBranch } from "lucide-react";
import { worktreeOf, worktreeRecloneDb } from "@/lib/ipc";
import { confirm } from "@/components/ui/dialog";
import { toast, toastBackendError } from "@/lib/toast";

export function WorktreeOfLine({ siteId }: { siteId: string }) {
  const navigate = useNavigate();
  const wt = useQuery({ queryKey: ["sites", "worktree-of", siteId], queryFn: () => worktreeOf(siteId) });
  const reclone = useMutation({
    mutationFn: () => worktreeRecloneDb(siteId),
    onSuccess: (n) => toast.success(`Database re-cloned from the parent — ${n} URL replacements`),
    onError: toastBackendError,
  });
  const w = wt.data;
  if (!w) return null;
  return (
    // Wraps BETWEEN its pieces, never inside one: the header gives this line only what the
    // site name's column has left, and at 1280–1366 px (the Dell, the Ubuntu VM — 10 Oct 2026)
    // a shrinking flex row broke "feature-x" into "feature-" / "x" and the label into two lines.
    <div className="mt-1 flex flex-wrap items-center gap-x-1.5 gap-y-0.5 whitespace-nowrap text-[0.71875rem] text-rex-text-muted">
      <GitBranch className="h-3 w-3" />
      <span>{w.assetKind ? `${w.assetKind} worktree of` : "worktree of"}</span>
      <button
        type="button"
        className="font-mono text-rex-accent-blue hover:underline"
        onClick={() => navigate(`/sites/${w.parentId}`)}
      >
        {w.parentDomain}
      </button>
      <span>·</span>
      <span className="max-w-full truncate font-mono text-rex-text">
        {w.present ? (w.branch ?? "detached") : `${w.askedBranch} — not checked out`}
      </span>
      {w.uncommitted != null && w.uncommitted > 0 && (
        <span className="text-status-warning-bright">· {w.uncommitted} uncommitted</span>
      )}
      {w.present ? (
        <button
          type="button"
          className="ml-1 text-rex-accent-blue hover:underline disabled:opacity-50"
          disabled={reclone.isPending}
          title="Replace this site's database with a fresh copy of the parent's"
          onClick={async () => {
            if (
              await confirm({
                title: "Re-clone the database from the parent?",
                message: `This site's database is replaced by a fresh copy of ${w.parentDomain}'s, with URLs moved to ${"this site"}. Anything written here since is lost.`,
                confirmLabel: "Re-clone",
                danger: true,
              })
            )
              reclone.mutate();
          }}
        >
          · Re-clone DB from parent
        </button>
      ) : null}
    </div>
  );
}
