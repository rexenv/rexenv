/** "Upload zip" source — the wp-admin move (Plugins → Add New → Upload
 *  Plugin), for a premium or private plugin/theme that isn't on wp.org.
 *
 *  It is a THIN front for the existing streamed install job, on purpose: the
 *  archive is handed to the same `wp <kind> install` wp-cli call as a slug,
 *  so the live card, the cancel button, the per-job log and the settle toast
 *  are the ones already proven by the wp.org flow. Nothing here copies,
 *  unpacks or moves a file — WordPress's own installer does that, which is
 *  why an archive with the wrong shape fails the way it would in wp-admin.
 *
 *  No drag-and-drop: a wry/WKWebView file drop hands over an OS path only
 *  with the Tauri drag-drop plugin wired app-wide, and a half-working drop
 *  target that silently does nothing is worse than a button that always
 *  works. Multi-select in the native dialog covers the batch case. */
import { baseName } from "@/lib/path";
import { usePlatformWords } from "@/lib/usePlatformWords";
import { useState } from "react";
import { FileArchive, Plus, X } from "lucide-react";
import { CHECK_INPUT } from "@/lib/utils";
import { pickZipFiles, wpInstallJob } from "@/lib/ipc";
import type { WpInstallState } from "@/types";
import { toastBackendError } from "@/lib/toast";

const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

export function ZipAddPanel({
  siteId,
  kind,
  busy,
  onStarted,
}: {
  siteId: string;
  kind: "plugin" | "theme";
  /** Another job (install or list action) is already running for this site. */
  busy: boolean;
  /** Hand the fresh snapshot to the panel's shared install card. */
  onStarted: (snap: WpInstallState) => void;
}) {
  const [files, setFiles] = useState<string[]>([]);
  const [activate, setActivate] = useState(kind === "plugin");
  const [starting, setStarting] = useState(false);
  const words = usePlatformWords();

  const pick = async () => {
    try {
      const picked = await pickZipFiles(`Choose the ${kind} .zip to install`);
      // Merge, deduplicated: picking twice adds to the batch instead of
      // silently dropping the first choice.
      if (picked.length > 0) setFiles((cur) => [...cur, ...picked.filter((p) => !cur.includes(p))]);
    } catch (e) {
      toastBackendError(e);
    }
  };

  const install = async () => {
    setStarting(true);
    try {
      const snap = await wpInstallJob(siteId, kind, files, activate, "zip");
      onStarted(snap);
      setFiles([]);
    } catch (e) {
      toastBackendError(e);
    } finally {
      setStarting(false);
    }
  };

  return (
    <div className="space-y-2">
      {files.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          {files.map((f) => (
            <span
              key={f}
              title={f}
              className="flex max-w-full items-center gap-1.5 rounded border border-rex-border bg-rex-surface-2 py-0.5 pl-2 pr-1 font-mono text-[0.71875rem] text-rex-text"
            >
              <FileArchive className="h-3 w-3 flex-none text-rex-text-muted" />
              <span className="truncate">{baseName(f)}</span>
              <button
                type="button"
                aria-label={`Remove ${baseName(f)}`}
                className="flex-none rounded p-0.5 text-rex-text-muted hover:text-rex-text"
                onClick={() => setFiles((cur) => cur.filter((x) => x !== f))}
              >
                <X className="h-3 w-3" />
              </button>
            </span>
          ))}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <button className={BTN + " flex items-center gap-1.5"} onClick={pick}>
          <FileArchive className="h-3.5 w-3.5" />
          Choose .zip{files.length > 0 ? "…" : " file…"}
        </button>
        <div className="flex-1" />
        <label className="flex cursor-pointer items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
          <input
            type="checkbox"
            checked={activate}
            onChange={(e) => setActivate(e.target.checked)}
            className={CHECK_INPUT}
          />
          {kind === "plugin" ? "Activate" : "Activate (switch to it)"}
        </label>
        <button
          className={BTN + " flex items-center gap-1.5"}
          disabled={busy || starting || files.length === 0}
          onClick={() => void install()}
        >
          <Plus className="h-3.5 w-3.5" />
          Install{files.length > 1 ? ` (${files.length})` : ""}
        </button>
      </div>
      <div className="text-[0.6875rem] text-rex-text-muted">
        Installs a {kind} from a .zip on {words.host} — the same thing wp-admin's “Upload{" "}
        {kind}” does, run through WP-CLI so you can watch it and cancel it. The file is
        read where it sits; nothing is uploaded anywhere.
      </div>
    </div>
  );
}
