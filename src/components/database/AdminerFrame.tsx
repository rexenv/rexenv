import { useState } from "react";
import { Check, Copy, Database, ExternalLink } from "lucide-react";
import { Placeholder } from "@/components/common/Placeholder";
import { isTauri, openExternal } from "@/lib/ipc";
import { toastBackendError } from "@/lib/toast";

/** Embeds the stack-served Adminer (framed, dark via Adminer's prefers-color-scheme).
 *  Off-Tauri the `rexdb://` proxy scheme doesn't exist, so we show a placeholder.
 *
 *  `externalUrl` (the `adminerUrl(...)` deep-link) adds a slim bar above the
 *  frame: the first-party https URL with Copy + "Open in browser", so the same
 *  DB session can be driven from a full browser (auto-login works there too —
 *  first-party context, no iframe cookie games needed). */
export function AdminerFrame({ src, externalUrl }: { src: string; externalUrl?: string }) {
  const [copied, setCopied] = useState(false);
  if (!isTauri()) {
    return (
      <Placeholder
        icon={<Database className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Database browser"
        hint="The embedded Adminer requires the rexenv desktop app."
      />
    );
  }
  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      {externalUrl && (
        <div className="flex flex-none items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-1 py-1 pl-2.5 pr-1">
          <span className="min-w-0 flex-1 truncate font-mono text-[0.6875rem] text-rex-text-muted" title={externalUrl}>
            {externalUrl}
          </span>
          <button
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(externalUrl);
                setCopied(true);
                setTimeout(() => setCopied(false), 1200);
              } catch {
                /* clipboard unavailable */
              }
            }}
            className={`flex h-7 flex-none items-center gap-1.5 rounded-md px-2 text-[0.71875rem] transition-colors hover:bg-rex-hover-strong ${
              copied ? "text-status-running-bright" : "text-rex-text-muted"
            }`}
          >
            {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
            {copied ? "Copied" : "Copy"}
          </button>
          <button
            onClick={() => void openExternal(externalUrl).catch(toastBackendError)}
            title="Open this database in your default browser"
            className="flex h-7 flex-none items-center gap-1.5 rounded-md px-2 text-[0.71875rem] text-rex-text-muted transition-colors hover:bg-rex-hover-strong hover:text-rex-text"
          >
            <ExternalLink className="h-3.5 w-3.5" />
            Open in browser
          </button>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-hidden rounded-xl border border-rex-border bg-rex-bg">
        <iframe title="Adminer" src={src} className="h-full w-full border-0 bg-rex-bg" />
      </div>
    </div>
  );
}
