import { useQuery } from "@tanstack/react-query";
import { getSetting, listEditors, openExternal, openInEditor } from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import type { EditorApp } from "@/types";

/** Code editors installed on this machine. Cached: detection reads app bundles
 *  and shells out per icon, and nothing installs an IDE mid-session. */
export function useEditors(): EditorApp[] {
  const { data = [] } = useQuery({
    queryKey: ["editors"],
    queryFn: listEditors,
    staleTime: 60_000,
  });
  return data;
}

/** The editor "Open in editor" targets: the preferred_editor setting when it is
 *  still installed, else the first detected editor, else null (no editor). */
export function usePreferredEditor(): EditorApp | null {
  const editors = useEditors();
  const { data: preferred } = useQuery({
    queryKey: ["setting", "preferred_editor"],
    queryFn: () => getSetting("preferred_editor"),
  });
  return editors.find((e) => e.id === preferred) ?? editors[0] ?? null;
}

/** Open the whole site folder as a PROJECT in `editor`. No editor detected →
 *  say so honestly and reveal the folder instead. Shared by every "Open in
 *  editor" entry point so they can't drift into different fallbacks. */
export function openSiteInEditor(editor: EditorApp | null, path: string): void {
  if (editor) {
    openInEditor(editor.id, path).catch(toastBackendError);
    return;
  }
  toast.info(
    "No code editor found (VS Code, Cursor, PhpStorm, Zed, Sublime…) — opening the folder in Finder instead.",
  );
  void openExternal(path).catch(toastBackendError);
}
