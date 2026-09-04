import { useMutation, useQueryClient } from "@tanstack/react-query";
import { setSiteEnabled } from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import type { Site } from "@/types";

/**
 * Start or stop ONE site (v44) — the shared mutation, so the Sites list and the
 * site page cannot drift into saying different things about the same click.
 *
 * The copy is the load-bearing part. Two facts a user has to be told, both of
 * which the backend actually establishes rather than the UI hoping:
 *
 * - **Stopping a site is not "Stop all".** The shared web server and the PHP
 *   pool it sits on serve every other site and keep running, so the toast says
 *   so — otherwise the natural reading of a Stop next to a footer that also says
 *   "Stop all" is that this one is a smaller version of that one.
 * - **A started site is not necessarily serving.** With rexenv's services
 *   stopped, the switch is recorded and nothing answers; the report carries the
 *   reason (`note`) and it is shown as-is, rather than a success toast the
 *   browser will contradict a second later.
 */
export function useSiteEnabled() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ site, enabled }: { site: Site; enabled: boolean }) =>
      setSiteEnabled(site.id, enabled),
    onSuccess: (report, { site, enabled }) => {
      void qc.invalidateQueries({ queryKey: ["sites"] });
      void qc.invalidateQueries({ queryKey: ["sites-serving"] });
      void qc.invalidateQueries({ queryKey: ["site", site.id] });
      if (!report) return;
      if (report.note) {
        toast.info(report.note);
        return;
      }
      toast.success(
        enabled
          ? `${site.domain} is serving again.`
          : `${site.domain} stopped — it answers "site stopped" now. Your other sites keep running.`,
      );
    },
    onError: (e) => toastBackendError(e),
  });
}
