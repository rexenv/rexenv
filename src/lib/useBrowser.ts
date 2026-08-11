import { useQuery } from "@tanstack/react-query";
import { getSetting, listBrowsers, openInBrowser } from "@/lib/ipc";
import { toastBackendError } from "@/lib/toast";
import type { BrowserApp } from "@/types";

/** Browsers installed on this machine. Cached hard: detection shells out to
 *  `sips` for each app's icon, and an app doesn't install itself while the user
 *  is looking at a site page. */
export function useBrowsers(): BrowserApp[] {
  const { data = [] } = useQuery({
    queryKey: ["browsers"],
    queryFn: listBrowsers,
    staleTime: 5 * 60_000,
  });
  return data;
}

/** The browser an `http(s)` link ACTUALLY opens in, so the button can wear its
 *  icon: the `preferred_browser` setting while it is still installed, else the
 *  OS default, else the first detected one, else null.
 *
 *  This mirrors — it does not decide. The routing itself lives in the backend's
 *  `open_external`; if this hook and that command ever disagree the command
 *  wins, which is why the same three-step fallback is written in both. */
export function usePreferredBrowser(): BrowserApp | null {
  const browsers = useBrowsers();
  const { data: preferred } = useQuery({
    queryKey: ["setting", "preferred_browser"],
    queryFn: () => getSetting("preferred_browser"),
  });
  return (
    browsers.find((b) => b.id === preferred) ??
    browsers.find((b) => b.systemDefault) ??
    browsers[0] ??
    null
  );
}

/** Open one url in a chosen browser — the chevron menu's action. Deliberately
 *  does NOT touch the preference: a menu that rewrote the default would leave
 *  the user wondering why everything opens somewhere new. */
export function openUrlIn(browser: BrowserApp, url: string): void {
  void openInBrowser(browser.id, url).catch(toastBackendError);
}
