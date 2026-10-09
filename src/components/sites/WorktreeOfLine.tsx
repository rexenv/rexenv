/**
 * "Worktree of shop.rex · feature/x" under a worktree child's domain
 * (`docs/PLAN-git-worktrees.md`, W8). The branch is git's answer now; when the
 * worktree is not there, it says what was asked for and that it is missing.
 * Renders nothing for every other site.
 */
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { GitBranch } from "lucide-react";
import { worktreeOf } from "@/lib/ipc";

export function WorktreeOfLine({ siteId }: { siteId: string }) {
  const navigate = useNavigate();
  const wt = useQuery({ queryKey: ["sites", "worktree-of", siteId], queryFn: () => worktreeOf(siteId) });
  const w = wt.data;
  if (!w) return null;
  return (
    <div className="mt-1 flex items-center gap-1.5 text-[0.71875rem] text-rex-text-muted">
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
      <span className="font-mono text-rex-text">
        {w.present ? (w.branch ?? "detached") : `${w.askedBranch} — not checked out`}
      </span>
      {w.uncommitted != null && w.uncommitted > 0 && (
        <span className="text-status-warning-bright">· {w.uncommitted} uncommitted</span>
      )}
    </div>
  );
}
