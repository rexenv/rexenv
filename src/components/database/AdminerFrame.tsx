import { useEffect, useState, useSyncExternalStore } from "react";
import { Check, Copy, Database } from "lucide-react";
import { OpenInBrowserSplit } from "@/components/ui/open-in";
import { Placeholder } from "@/components/common/Placeholder";
import { adminerSetTheme, isTauri, transport } from "@/lib/ipc";
import { currentTheme, subscribeTheme } from "@/lib/theme";

/** Embeds the stack-served console — Adminer or phpMyAdmin (framed; its palette
 *  follows the APP's theme, written to the console's docroot before the frame
 *  loads — Adminer's own prefers-color-scheme default is what left it in the
 *  opposite theme, and phpMyAdmin 5.2 has no dark theme of its own at all).
 *  Off-Tauri the `rexdb://` proxy scheme doesn't exist, so we show a placeholder.
 *
 *  `externalUrl` (the `adminerUrl(...)` deep-link) adds a slim bar above the
 *  frame: the first-party https URL with Copy + "Open in browser", so the same
 *  DB session can be driven from a full browser (auto-login works there too —
 *  first-party context, no iframe cookie games needed). */
export function AdminerFrame({
  src,
  externalUrl,
  console = "adminer",
}: {
  src: string;
  externalUrl?: string;
  /** Which console `src` is: the frame's title names what it shows, and the `key` below
   *  reloads the right document on a palette change. */
  console?: "adminer" | "phpmyadmin";
}) {
  const [copied, setCopied] = useState(false);
  // The console renders in its own process off a file rexenv writes, so the
  // write has to LAND before the frame loads — hence `applied` gating the
  // iframe rather than a plain effect beside it. A frame that loaded first
  // would paint the old scheme and only correct itself on the next navigation.
  const theme = useSyncExternalStore(subscribeTheme, currentTheme, currentTheme);
  const [applied, setApplied] = useState<"dark" | "light" | null>(null);
  useEffect(() => {
    let live = true;
    // A failure still loads the console: it would render in its own default
    // scheme, which is exactly what it did before this existed — worse than
    // matching, better than a blank panel. One write covers BOTH consoles
    // (`adminer_set_theme` writes each docroot's palette file; phpMyAdmin's
    // generated config reads it per request since 15 Oct 2026).
    void adminerSetTheme(theme)
      .catch(() => {})
      .then(() => {
        if (live) setApplied(theme);
      });
    return () => {
      live = false;
    };
  }, [theme, console]);
  if (transport() !== "tauri") {
    // The frame loads through the window's own `rexdb:`/`rexpma:` schemes, which a browser does
    // not have: over the web UI it would be an empty panel, so it says so instead
    // (docs/PLAN-headless-web.md T8 — serving the consoles over HTTP is still open).
    return (
      <Placeholder
        icon={<Database className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Database browser"
        hint={
          isTauri()
            ? "The embedded database console is not in the web UI yet — use the desktop app, or rex db export / import."
            : "The embedded database console requires the rexenv desktop app."
        }
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
          {/* The same control as the Databases row: icon + chevron + private
              targets, never a plain button beside a split one (consistency,
              13 Sep / 14 Oct 2026). */}
          <OpenInBrowserSplit
            url={externalUrl}
            variant="ghost"
            title="Open this database in your browser"
            chevronLabel="Open this database in another browser"
            probe="db-frame-open-in-browser"
          />
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
            key={`${console}-${applied}`}
            title={console === "phpmyadmin" ? "phpMyAdmin" : "Adminer"}
            data-probe={`db-frame-${console}`}
            src={src}
            className="absolute inset-0 h-full w-full border-0 bg-rex-bg"
          />
        )}
      </div>
    </div>
  );
}
