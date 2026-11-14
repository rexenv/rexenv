import { shortcutsAreTheViewers } from "@/lib/ipc";
import { usePlatformWords } from "@/lib/usePlatformWords";

/** The modifier of rexenv's keyboard shortcuts: its label, and the test for it.
 *
 * In the app's own window the keyboard is the machine's, so both come from the
 * platform's words (`modKey`, `modKeyIsCommand`) — and only ONE modifier is
 * answered. Answering either would be simpler and wrong: in a macOS text field
 * Ctrl+K deletes to the end of the line, and on Windows and Linux there is no
 * Command key to label.
 *
 * Over the web UI the words describe the SERVER's OS, not the keyboard in front
 * of the person: a Mac's browser on a Linux server would be told "Ctrl+K" while
 * its user reaches for ⌘. The page cannot be told which it is by the server, so
 * there it answers both and says both. */
export function useModKey(): { label: string; pressed: (e: KeyboardEvent | React.KeyboardEvent) => boolean } {
  const words = usePlatformWords();
  if (shortcutsAreTheViewers()) {
    return { label: "Ctrl/⌘ ", pressed: (e) => e.metaKey !== e.ctrlKey };
  }
  return words.modKeyIsCommand
    ? { label: words.modKey, pressed: (e) => e.metaKey && !e.ctrlKey }
    : { label: words.modKey, pressed: (e) => e.ctrlKey && !e.metaKey };
}
