import type { ReactNode } from "react";
import { Code, Globe, TerminalSquare } from "lucide-react";
import { AppIcon } from "@/components/ui/app-icon";
import { IncognitoIcon } from "@/components/common/IncognitoIcon";
import { MenuItem } from "@/components/ui/menu";
import { openUrlIn, useBrowsers, usePreferredBrowser } from "@/lib/useBrowser";
import { openSiteInEditor, useEditors, usePreferredEditor } from "@/lib/useEditor";
import { openInTerminal, useTerminalApps } from "@/lib/useTerminalApp";
import type { TerminalAsset } from "@/lib/ipc";
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
              icon: <IncognitoIcon className="h-[15px] w-[15px]" />,
              label: `Open in a private ${b.name} window`,
              onSelect: () => open(b, true),
            }
          : undefined
      }
    >
      <span className="flex-1 truncate">{b.name}</span>
      {b.id === current?.id && (
        <span className="flex-none text-[0.6875rem] text-rex-text-muted">default</span>
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
        <span className="flex-none text-[0.6875rem] text-rex-text-muted">default</span>
      )}
    </MenuItem>
  ));
}

/**
 * The chevron menu beside every built-in Terminal control: the same folder in
 * one of the user's OWN terminal apps.
 *
 * Unlike the browser/editor menus this one appears even with a single app,
 * because it is not a choice BETWEEN equals — the plain click opens rexenv's
 * built-in tab and the menu is a different destination entirely. (On macOS the
 * list is never empty anyway: Terminal.app cannot be uninstalled.)
 *
 * That shell is the user's plain login shell, with no bundled PHP and no `wp`
 * wrapper on PATH — the built-in tab stays the one that answers with the SITE's
 * PHP version.
 */
export function useTerminalMenu(siteId: string, asset?: TerminalAsset): ReactNode | undefined {
  const terminals = useTerminalApps();
  if (terminals.length === 0) return undefined;
  return terminals.map((t) => (
    <MenuItem
      key={t.id}
      icon={<AppIcon icon={t.icon} fallback={<TerminalSquare className="h-4 w-4" />} />}
      onSelect={() => openInTerminal(t, siteId, asset)}
    >
      <span className="flex-1 truncate">{t.name}</span>
    </MenuItem>
  ));
}
