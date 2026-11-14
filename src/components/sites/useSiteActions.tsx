import { useMemo, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { dbImportRecords, deleteSite, renameSite, rewriteRevert, worktreeRelations } from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import { PromptDialog } from "@/components/ui/dialog";
import { DeleteSiteDialog } from "@/components/sites/DeleteSiteDialog";
import { NewSiteDialog } from "@/components/sites/NewSiteDialog";
import type { Site, SiteProvisionState } from "@/types";

/** **Rename, Duplicate and Delete for a site — the dialogs and what they call, once.**
 *
 * These three lived inside the Sites list (`routes/Sites.tsx`), so they could be
 * reached only from a row's menu: a user on a site's own page who wanted to delete
 * it went back to the list first. The site page needed them too (14 Nov 2026,
 * `docs/PLAN-sites-ux.md` T1), and a second copy of the delete flow is how the two
 * would come to disagree about the one thing that must not be skipped — a site whose
 * connection settings rexenv rewrote is REVERTED before it is deleted, and is not
 * deleted on the back of a revert that did not happen. So both screens call this.
 *
 * `dialogs` is rendered once by the caller; the three functions open them. */
export function useSiteActions(opts: {
  /** A Duplicate's provision job has started — the Sites list adopts and shows it. */
  onDuplicateStarted?: (snap: SiteProvisionState) => void;
  /** The site is gone (after either delete leg really deleted it). */
  onDeleted?: (site: Site) => void;
} = {}): {
  rename: (site: Site) => void;
  duplicate: (site: Site) => void;
  remove: (site: Site) => void;
  dialogs: ReactNode;
} {
  const qc = useQueryClient();
  const [renameTarget, setRenameTarget] = useState<Site | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Site | null>(null);
  // "Duplicate" opens New Site prefilled with this site's setup (type/PHP/server);
  // the user picks a fresh domain. A byte-for-byte clone would need a backend copy.
  const [dupSource, setDupSource] = useState<Site | null>(null);

  const { data: worktreeRows = [] } = useQuery({
    queryKey: ["sites", "worktree-relations"],
    queryFn: worktreeRelations,
  });
  // Settled DB-import facts (v20): a CONNECTED site is reverted before it is deleted.
  const { data: dbRecords = [] } = useQuery({
    queryKey: ["db-import-records"],
    queryFn: dbImportRecords,
  });
  const dbStates = useMemo(() => new Map(dbRecords.map((r) => [r.siteId, r.state])), [dbRecords]);

  const renameMut = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => renameSite(id, name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toastBackendError(e),
  });

  const removeMut = useMutation({
    mutationFn: (site: Site) => deleteSite(site.id),
    onSuccess: (_, site) => {
      void qc.invalidateQueries({ queryKey: ["sites"] });
      opts.onDeleted?.(site);
    },
    onError: (e) => toastBackendError(e),
  });

  // D2's default leg for a CONNECTED site: revert the connection change,
  // then delete. If the revert can't run cleanly (file edited since, backup
  // missing), stop and say so — never delete on the back of a revert that
  // didn't happen.
  const revertThenRemove = useMutation({
    mutationFn: async (site: Site) => {
      const out = await rewriteRevert(site.id, false);
      if (out.status === "reverted" || out.status === "noRewrite") {
        await deleteSite(site.id);
        return null;
      }
      return out.message;
    },
    onSuccess: (blocked, site) => {
      if (blocked) {
        toast.info(
          `${site.domain} was not deleted — ${blocked} Resolve it on the site's Database tab, or delete without reverting.`,
        );
      } else {
        // Only a delete that HAPPENED is reported as one: a blocked revert left
        // the site where it was, and the page must stay on it.
        opts.onDeleted?.(site);
      }
      void qc.invalidateQueries({ queryKey: ["sites"] });
      void qc.invalidateQueries({ queryKey: ["db-import-records"] });
    },
    onError: (e) => toastBackendError(e),
  });

  const dialogs = (
    <>
      {dupSource && (
        <NewSiteDialog
          initial={{
            name: `${dupSource.name} copy`,
            siteType: dupSource.type,
            phpVersion: dupSource.phpVersion,
            webServer: dupSource.webServer,
          }}
          onClose={() => setDupSource(null)}
          onStarted={opts.onDuplicateStarted}
        />
      )}
      {renameTarget && (
        <PromptDialog
          title="Rename site"
          label="Display name"
          initialValue={renameTarget.name}
          submitLabel="Rename"
          onSubmit={(name) => {
            if (name !== renameTarget.name) renameMut.mutate({ id: renameTarget.id, name });
            setRenameTarget(null);
          }}
          onCancel={() => setRenameTarget(null)}
        />
      )}
      {deleteTarget && (
        <DeleteSiteDialog
          site={deleteTarget}
          removesFolder={worktreeRows.some((w) => w.siteId === deleteTarget.id && w.removesFolder)}
          dbState={dbStates.get(deleteTarget.id)}
          onPlainDelete={() => {
            removeMut.mutate(deleteTarget);
            setDeleteTarget(null);
          }}
          onRevertThenDelete={() => {
            revertThenRemove.mutate(deleteTarget);
            setDeleteTarget(null);
          }}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </>
  );

  return { rename: setRenameTarget, duplicate: setDupSource, remove: setDeleteTarget, dialogs };
}
