import type { ReactNode } from "react";
import { Code, Globe, VenetianMask } from "lucide-react";
import { AppIcon } from "@/components/ui/app-icon";
import { MenuItem } from "@/components/ui/menu";
import { openUrlIn, useBrowsers, usePreferredBrowser } from "@/lib/useBrowser";
import { openSiteInEditor, useEditors, usePreferredEditor } from "@/lib/useEditor";
import type { BrowserApp } from "@/types";

/** Width for a chevron menu built by {@link useBrowserMenu}. Wider than the
 *  `SplitButton` default because a row here carries an icon, a name, the
 *  `default` tag AND the private target — at the default width the names of the
 *  browsers people actually use ("Firefox Developer Edition") truncate. */
export const BROWSER_MENU_WIDTH = 246;

/**
 * The chevron menus behind "Open in browser" / "Open in editor": every app of
 * that kind installed on this machine, the one a plain click uses marked
 * `default`.
 *
 * Both return `undefined` when there is nothing to choose BETWEEN (zero or one
 * app). A chevron that opens a one-item menu is a control that can't change
 * anything — `SplitButton` drops it when the menu is undefined, so the button
 * simply looks like a button on a machine with one browser.
 *
 * Picking here is a ONE-TIME detour: it opens this url/folder in that app and
 * leaves the preference alone (that lives in Settings). A menu that silently
 * rewrote the default is how "why does everything open in Firefox now" starts.
 *
 * Each browser row carries a SECOND target behind a divider: the same url in
 * that browser's private/incognito window — a logged-out look at the site
 * without signing out of the session you are working in. It appears only for
 * browsers rexenv can really open one in (`supportsPrivate`); Safari has no
 * private-window command line, so its row has no icon at all rather than a
 * control that would open an ordinary, recorded window. (A one-browser Mac
 * still gets no menu — but on macOS that machine only has Safari, which has no
 * private target to offer in the first place.)
 */
export function useBrowserMenu(
  /** The link, or a resolver for one that has to be MINTED on click — a magic
   *  login url is a one-time token, so it can't be computed up front and held
   *  in a menu that may never be opened. */
  url: string | (() => Promise<string>),
): ReactNode | undefined {
  const browsers = useBrowsers();
  const current = usePreferredBrowser();
  if (browsers.length < 2) return undefined;
  // One resolver for both targets: a string url opens straight away, a minted
  // one is fetched on click. Written once so the private target can never drift
  // into opening a different (or a stale, already-spent) url than the row.
  const open = (b: BrowserApp, isPrivate: boolean) => {
    if (typeof url === "string") return openUrlIn(b, url, isPrivate);
    void url().then((resolved) => openUrlIn(b, resolved, isPrivate));
  };
  return browsers.map((b) => (
    <MenuItem
      key={b.id}
      icon={<AppIcon icon={b.icon} fallback={<Globe className="h-4 w-4" />} />}
      onSelect={() => open(b, false)}
      action={
        b.supportsPrivate
          ? {
              icon: <VenetianMask className="h-[15px] w-[15px]" strokeWidth={1.8} />,
              label: `Open in a private ${b.name} window`,
              onSelect: () => open(b, true),
            }
          : undefined
      }
    >
      <span className="flex-1 truncate">{b.name}</span>
      {b.id === current?.id && (
        <span className="flex-none text-[0.6875rem] text-rex-text-dim">default</span>
      )}
    </MenuItem>
  ));
}

export function useEditorMenu(path: string): ReactNode | undefined {
  const editors = useEditors();
  const current = usePreferredEditor();
  if (editors.length < 2) return undefined;
  return editors.map((e) => (
    <MenuItem
      key={e.id}
      icon={<AppIcon icon={e.icon} fallback={<Code className="h-4 w-4" />} />}
      onSelect={() => openSiteInEditor(e, path)}
    >
      <span className="flex-1 truncate">{e.name}</span>
      {e.id === current?.id && (
        <span className="flex-none text-[0.6875rem] text-rex-text-dim">default</span>
      )}
    </MenuItem>
  ));
}
