import { useEffect, useState, useSyncExternalStore } from "react";
import { Check, Copy, Database, ExternalLink } from "lucide-react";
import { Placeholder } from "@/components/common/Placeholder";
import { adminerSetTheme, isTauri, openExternal } from "@/lib/ipc";
import { currentTheme, subscribeTheme } from "@/lib/theme";
import { toastBackendError } from "@/lib/toast";

/** Embeds the stack-served Adminer (framed; its palette follows the APP's theme,
 *  written to the console's docroot before the frame loads — Adminer's own
 *  prefers-color-scheme default is what left it in the opposite theme).
 *  Off-Tauri the `rexdb://` proxy scheme doesn't exist, so we show a placeholder.
 *
 *  `externalUrl` (the `adminerUrl(...)` deep-link) adds a slim bar above the
 *  frame: the first-party https URL with Copy + "Open in browser", so the same
 *  DB session can be driven from a full browser (auto-login works there too —
 *  first-party context, no iframe cookie games needed). */
export function AdminerFrame({ src, externalUrl }: { src: string; externalUrl?: string }) {
  const [copied, setCopied] = useState(false);
  // The console renders in its own process off a file rexenv writes, so the
  // write has to LAND before the frame loads — hence `applied` gating the
  // iframe rather than a plain effect beside it. A frame that loaded first
  // would paint the old scheme and only correct itself on the next navigation.
  const theme = useSyncExternalStore(subscribeTheme, currentTheme, currentTheme);
  const [applied, setApplied] = useState<"dark" | "light" | null>(null);
  useEffect(() => {
    let live = true;
    // A failure still loads the console: it would render in the OS scheme,
    // which is exactly what it did before this existed — worse than matching,
    // better than a blank panel.
    void adminerSetTheme(theme)
      .catch(() => {})
      .then(() => {
        if (live) setApplied(theme);
      });
    return () => {
      live = false;
    };
  }, [theme]);
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
    /* h-full serves a definite-height parent (Databases); flex-1 serves a
       flex-column parent (the site Database tab) — WebKit won't resolve a
       percentage against a min-height-only flex item, which collapsed the
       iframe to its ~150px intrinsic default (UI-REVIEW §C2.2). */
    <div className="flex h-full min-h-0 flex-1 flex-col gap-2">
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
      {/* The iframe fills ABSOLUTELY: percentage heights through flex chains
          are exactly what broke here, and inset-0 sizes against the
          containing block with no percentage resolution at all. */}
      <div className="relative min-h-0 flex-1 overflow-hidden rounded-xl border border-rex-border bg-rex-bg">
        {/* `key` on the palette: a theme change has to RELOAD the console —
            it is a separate document that read the file at request time, and
            nothing inside it re-reads anything. */}
        {applied && (
          <iframe
            key={applied}
            title="Adminer"
            src={src}
            className="absolute inset-0 h-full w-full border-0 bg-rex-bg"
          />
        )}
      </div>
    </div>
  );
}
