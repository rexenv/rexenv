import type { ReactNode } from "react";
import { Code, Globe } from "lucide-react";
import { AppIcon } from "@/components/ui/app-icon";
import { MenuItem } from "@/components/ui/menu";
import { openUrlIn, useBrowsers, usePreferredBrowser } from "@/lib/useBrowser";
import { openSiteInEditor, useEditors, usePreferredEditor } from "@/lib/useEditor";

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
 */
export function useBrowserMenu(url: string): ReactNode | undefined {
  const browsers = useBrowsers();
  const current = usePreferredBrowser();
  if (browsers.length < 2) return undefined;
  return browsers.map((b) => (
    <MenuItem
      key={b.id}
      icon={<AppIcon icon={b.icon} fallback={<Globe className="h-4 w-4" />} />}
      onSelect={() => openUrlIn(b, url)}
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
