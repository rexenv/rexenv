import { useQuery } from "@tanstack/react-query";
import { listTerminals, openInTerminalApp, type TerminalAsset } from "@/lib/ipc";
import { toastBackendError } from "@/lib/toast";
import type { TerminalApp } from "@/types";

/** Terminal emulators installed on this machine. Cached hard for the same
 *  reason as {@link useBrowsers}: detection shells out to `sips` per app icon,
 *  and nobody installs a terminal while looking at a site page. */
export function useTerminalApps(): TerminalApp[] {
  const { data = [] } = useQuery({
    queryKey: ["terminals"],
    queryFn: listTerminals,
    staleTime: 5 * 60_000,
  });
  return data;
}

/** Open a site's folder (or one plugin/theme's folder) in the user's own
 *  terminal — the chevron beside every built-in Terminal control. Shared by all
 *  of them so they cannot drift into different behaviour. */
export function openInTerminal(
  terminal: TerminalApp,
  siteId: string,
  asset?: TerminalAsset,
): void {
  void openInTerminalApp(terminal.id, siteId, asset).catch(toastBackendError);
}
