/**
 * The delete-site confirms — extracted from Sites.tsx verbatim so the dev
 * harness can render every variant (the connected D2 dialog and its
 * db_created=0 form had never been rendered outside real deletes).
 *
 * Two shapes, one decision (D2): a CONNECTED site's config points at the
 * rexenv copy, so its confirm NAMES both outcomes and makes
 * revert-then-delete the default; everything else keeps the ordinary
 * ConfirmDialog with the copy naming exactly what this delete drops.
 */
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { ConfirmDialog, Overlay } from "@/components/ui/dialog";
import { confirmPhraseMatches, TypeToConfirm } from "@/components/ui/type-to-confirm";
import type { DbImportRecord, Site } from "@/types";

export function DeleteSiteDialog({
  site,
  removesFolder = false,
  dbState,
  onPlainDelete,
  onRevertThenDelete,
  onCancel,
}: {
  site: Site;
  /** A whole-site worktree rexenv made: Delete removes its folder although the site
   *  is linked (`WorktreeRelation.removesFolder`, #848). */
  removesFolder?: boolean;
  /** The site's db-import state, if any (the ONE serialized fact). */
  dbState?: DbImportRecord["state"];
  onPlainDelete: () => void;
  /** Only reachable from the connected variant. */
  onRevertThenDelete: () => void;
  onCancel: () => void;
}) {
  // The folder stays only for a linked site that is NOT a worktree rexenv made (#848).
  const keepsFolder = site.docrootManaged === false && !removesFolder;
  /* Hoisted above the variant split: the connected branch is the only one that
     uses it, but a hook can't sit after an early return. */
  const [typed, setTyped] = useState("");
  const match = confirmPhraseMatches(typed, site.domain);

  if (dbState !== "connected") {
    return (
      <ConfirmDialog
        title={`Delete "${site.name}"?`}
        message={
          <>
            This permanently removes <span className="font-mono text-rex-text">{site.domain}</span>
            {keepsFolder ? "" : ", its files"}
            {site.type === "wordpress" ? (
              ", its database,"
            ) : site.dbCreated === true ? (
              /* Non-WordPress: only explicit import provenance drops — and
                 the confirm names the database it will drop. */
              <>
                , its imported database copy{" "}
                <span className="font-mono text-rex-text">{site.dbName}</span>,
              </>
            ) : (
              ""
            )}{" "}
            and its certificate.
            {keepsFolder && (
              <>
                {" "}Your folder at{" "}
                <span className="font-mono text-rex-text">{site.path}</span> is left
                exactly where it is.
              </>
            )}{" "}
            This can't be undone.
          </>
        }
        confirmLabel="Delete site"
        danger
        confirmPhrase={site.domain}
        onConfirm={onPlainDelete}
        onCancel={onCancel}
      />
    );
  }

  /* D2: a connected site's config points at the rexenv copy, so the confirm
     NAMES both outcomes — as STACKED option blocks, each full-width button
     sitting with its own consequence. Three side-by-side sentence-length
     buttons overflowed the fixed-width overlay (UI-REVIEW §C1.1); stacking
     holds at any width and any domain length, and reads better anyway.
     Revert-then-delete stays the default (autoFocus, primary, first). */
  const preexisting = site.dbCreated === false;
  return (
    <Overlay onClose={onCancel}>
      <div className="text-[0.9375rem] font-semibold text-rex-text">
        Delete "{site.name}"?
      </div>
      <div className="mt-2 space-y-2 text-[0.8125rem] leading-[1.55] text-rex-text-muted">
        <p>
          {preexisting ? (
            <>
              This site's config was rewritten to use a database on rexenv's engine
              that existed before the import —{" "}
              <span className="font-mono text-rex-text">{site.dbName}</span> is kept,
              deleted or not.
            </>
          ) : (
            <>
              This site's config was rewritten to use rexenv's database copy — and
              deleting the site drops that copy (
              <span className="font-mono text-rex-text">{site.dbName}</span>).
            </>
          )}
        </p>
        <p>
          {keepsFolder ? (
            <>
              Your folder at{" "}
              <span className="font-mono text-rex-text">{site.path}</span> is left
              exactly where it is.{" "}
            </>
          ) : (
            <>The site's files are removed. </>
          )}
          This can't be undone.
        </p>
      </div>
      {/* Both actions below delete the site, so the gate sits ABOVE them and
          disables both; the input takes the focus the default button used to. */}
      <TypeToConfirm phrase={site.domain} value={typed} onChange={setTyped} autoFocus />
      <div className="mt-4 space-y-2">
        <div className="rounded-lg border border-rex-border-subtle p-3">
          <Button variant="primary" className="w-full" disabled={!match} onClick={onRevertThenDelete}>
            Revert, then delete
          </Button>
          <p className="mt-2 text-[0.78125rem] leading-[1.5] text-rex-text-muted">
            {preexisting ? (
              <>
                The config file is first restored to point back at the old database,
                then the site is removed.
              </>
            ) : (
              <>
                The config file is first restored to point back at the old database,
                then the site and rexenv's copy are removed. The site keeps working
                against its old database.
              </>
            )}
          </p>
        </div>
        <div className="rounded-lg border border-rex-border-subtle p-3">
          <Button variant="danger" className="w-full" disabled={!match} onClick={onPlainDelete}>
            Delete without reverting
          </Button>
          <p className="mt-2 text-[0.78125rem] leading-[1.5] text-rex-text-muted">
            {preexisting ? (
              <>
                The config keeps pointing at{" "}
                <span className="font-mono text-rex-text">{site.dbName}</span> on
                rexenv's engine, which stays.
              </>
            ) : (
              <>
                The config keeps pointing at rexenv's copy, which no longer exists
                after the delete:{" "}
                <strong className="text-rex-text">the site breaks on next load</strong>{" "}
                until you change its connection settings yourself.
              </>
            )}
          </p>
        </div>
      </div>
      <div className="mt-4 flex justify-end">
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </Overlay>
  );
}
