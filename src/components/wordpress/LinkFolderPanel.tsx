/** "Link folder" source (phase D): symlink an EXISTING local checkout into
 *  this site's wp-content. The folder stays where it is — deleting the asset
 *  later removes ONLY the link (backend-guaranteed on filesystem truth). */
import { baseName } from "@/lib/path";
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FolderSymlink } from "lucide-react";
import { TECH_INPUT } from "@/lib/utils";
import { pickFolder, repoLink } from "@/lib/ipc";
import type { RepoLinkResult } from "@/types";
import { toast, toastBackendError } from "@/lib/toast";

const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

export function LinkFolderPanel({
  siteId,
  kind,
  onInstalled,
}: {
  siteId: string;
  kind: "plugin" | "theme";
  onInstalled: () => void;
}) {
  const qc = useQueryClient();
  const [target, setTarget] = useState("");
  const [dirName, setDirName] = useState("");
  const [result, setResult] = useState<RepoLinkResult | null>(null);

  const link = useMutation({
    mutationFn: () => repoLink(siteId, kind, dirName === "" ? null : dirName, target),
    onSuccess: (r) => {
      setResult(r);
      toast.success(`Linked ${r.dirName}`);
      qc.invalidateQueries({ queryKey: ["repo-unmanaged", siteId, kind] });
      onInstalled();
    },
    onError: (e) => toastBackendError(e),
  });

  const pick = async () => {
    const picked = await pickFolder(`Choose the ${kind} folder to link`);
    if (picked) {
      setTarget(picked);
      setDirName(baseName(picked));
      setResult(null);
    }
  };

  const mismatch = result && result.wp.kind !== "none" && result.wp.kind !== kind;

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <FolderSymlink className="h-3.5 w-3.5 flex-none text-rex-text-muted" />
        <button className={BTN} onClick={pick}>
          Choose folder…
        </button>
        {target !== "" && (
          <>
            <span className="min-w-0 flex-1 truncate font-mono text-[0.71875rem] text-rex-text-muted">
              {target}
            </span>
            <input
              {...TECH_INPUT}
              value={dirName}
              onChange={(e) => setDirName(e.target.value)}
              aria-label="Folder name"
              className="h-[30px] w-[160px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
            />
            <button
              className={BTN}
              disabled={link.isPending}
              onClick={() => link.mutate()}
            >
              Link {kind}
            </button>
          </>
        )}
      </div>
      <div className="text-[0.6875rem] text-rex-text-muted">
        Symlinks the folder into this site — it stays where it is, you keep your own git
        workflow there. Deleting the {kind} in rexenv removes only the link, never your
        folder.
      </div>
      {result && (
        <div className="space-y-1">
          <div className="font-mono text-[0.71875rem] text-status-running-bright">
            ✓ linked as {result.dirName}
            {result.isGit ? " (git checkout — repo panel available)" : " (not a git repo)"}
          </div>
          {result.wp.kind === "none" && (
            <div className="rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] text-status-warning-bright">
              No {kind} header found at the folder root — WordPress won't list it until one
              exists (monorepo? link the {kind} subfolder instead).
            </div>
          )}
          {mismatch && (
            <div className="rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] text-status-warning-bright">
              This folder looks like a {result.wp.kind}, but you linked it as a {kind}.
            </div>
          )}
        </div>
      )}
    </div>
  );
}
