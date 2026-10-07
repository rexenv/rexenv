// The "core files missing" banner on a WordPress site's Overview (ledger #604).
//
// rexenv 0.4.0–0.7.1 downloaded WordPress as a tarball that PHP's PharData read
// with every long file name cut at 100 characters, so those sites were created
// missing core classes and default-theme fonts — silently, on macOS. Tools →
// Maintenance could reinstall core, but only for somebody who already suspected
// it, and never the theme files; this tells them and repairs both. The sentence
// comes from Rust (`CutNameReport.message`), beside the rule.
//
// Silent unless something is missing: an offline machine or a version
// wordpress.org does not list renders nothing here (`rex doctor` reports those
// as unchecked), so the banner never cries wolf.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { confirm } from "@/components/ui/dialog";
import { toast, toastBackendError } from "@/lib/toast";
import { wpCoreCutNames, wpCoreRepairCutNames } from "@/lib/ipc";

export const coreCutNamesKey = (siteId: string) => ["wp-core-cut-names", siteId];

export function CoreFilesBanner({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const { data } = useQuery({
    queryKey: coreCutNamesKey(siteId),
    queryFn: () => wpCoreCutNames(siteId),
    staleTime: 5 * 60_000,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const repair = useMutation({
    mutationFn: () => wpCoreRepairCutNames(siteId),
    onSuccess: () => toast.success("Core files repaired"),
    onError: (e) => toastBackendError(e),
    // Asked again either way: a partial repair (one theme file refused) leaves the
    // banner counting only what is still missing.
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: coreCutNamesKey(siteId) });
      void qc.invalidateQueries({ queryKey: ["wp-info", siteId] });
    },
  });
  if (!data?.message) return null;
  return (
    <div className="mb-[14px] flex items-start justify-between gap-3 rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] leading-[1.5] text-status-warning-bright">
      <span>{data.message}</span>
      <button
        className="inline-flex shrink-0 items-center gap-1 rounded border border-status-warning-border px-2 py-0.5 font-medium hover:bg-status-warning-bg disabled:opacity-60"
        disabled={repair.isPending}
        onClick={async () => {
          if (
            await confirm({
              title: "Repair core files?",
              message: `Re-download WordPress ${data.version} core and put back the missing theme files. The database, wp-config.php and your files in wp-content are not touched.`,
              confirmLabel: "Repair",
            })
          ) {
            repair.mutate();
          }
        }}
      >
        {repair.isPending && <Loader2 className="h-3 w-3 animate-spin" />}
        {repair.isPending ? "Repairing…" : "Repair core files"}
      </button>
    </div>
  );
}
