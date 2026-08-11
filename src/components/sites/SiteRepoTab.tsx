/** The site's OWN checkout — Stage 3 of docs/PLAN-git-site-clone.md.
 *
 *  Deliberately the same `RepoPanel` the Plugins/Themes tabs use, pointed one
 *  level up at the project root: branch, dirtiness, ahead/behind, fetch / pull
 *  --ff-only / push / checkout, the dependency steps a pull offers when a
 *  lockfile moves, and the repo's own package.json scripts (which is also how a
 *  Vite build gets re-run after the create-time one). Writing a second panel
 *  would have meant a second answer to "is this checkout dirty" — the question
 *  the delete/checkout confirmations are built on.
 *
 *  `dirName` carries the DOMAIN. For `kind: "site"` the backend uses it as
 *  display text only and never turns it into a path (`job_target`), so the
 *  site target is the one kind with no user-supplied path segment at all. */
import { useQuery } from "@tanstack/react-query";
import { GitBranch } from "lucide-react";
import { repoSiteInfo } from "@/lib/ipc";
import { RepoPanel } from "@/components/wordpress/RepoPanel";
import { CopyButton } from "@/components/ui/copy-button";
import type { Site } from "@/types";

export function SiteRepoTab({ site }: { site: Site }) {
  // Pure filesystem on the backend, so refetching on window focus is honest and
  // cheap: `rm -rf .git` or a fresh `git init` in a terminal both land here the
  // moment the user comes back.
  const info = useQuery({
    queryKey: ["repo-site-info", site.id],
    queryFn: () => repoSiteInfo(site.id),
    staleTime: 0,
    refetchOnWindowFocus: true,
  });

  if (!info.data) return null;
  if (!info.data.present) {
    return (
      <div className="rounded-lg border border-rex-border bg-rex-surface-1 p-4">
        <div className="flex items-center gap-2 text-[0.84375rem] font-medium text-rex-text">
          <GitBranch className="h-4 w-4 text-rex-text-muted" /> No repository here
        </div>
        <p className="mt-1.5 text-[0.75rem] leading-[1.6] text-rex-text-muted">
          rexenv looks for a checkout in this site's own folder and nowhere else — it
          never searches upwards, because the folder above yours can be a repository
          holding every project you have, and this panel's Checkout button would then be
          pointing at it.
        </p>
        <div className="mt-2 flex items-center gap-1.5">
          <code className={"min-w-0 flex-1 truncate rounded bg-rex-well px-2 py-1 font-mono text-[0.6875rem] text-rex-text-dim"}>
            {info.data.projectRoot}
          </code>
          <CopyButton value={info.data.projectRoot} title="Copy the project folder path" />
        </div>
      </div>
    );
  }

  return (
    <RepoPanel
      siteId={site.id}
      kind="site"
      asset={{
        dirName: site.domain,
        url: info.data.clonedFrom ?? "",
        gitRef: site.gitRef ?? null,
        // A site's checkout is either one rexenv cloned or one that was already
        // there — never a symlink into wp-content, which is what "linked" means
        // for an asset.
        source: info.data.clonedFrom ? "cloned" : "adopted",
      }}
    />
  );
}
