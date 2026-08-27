import { useEffect, useRef, useState } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { Eraser, RotateCw, TerminalSquare } from "lucide-react";
import { Placeholder } from "@/components/common/Placeholder";
import {
  closeTerminal,
  isTauri,
  onTerminalOutput,
  openTerminal,
  resizeTerminal,
  writeTerminal,
} from "@/lib/ipc";

/** Read a design token (CSS var) at runtime so the terminal theme tracks tokens.css. */
function token(name: string, fallback: string): string {
  if (typeof window === "undefined") return fallback;
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

function xtermTheme() {
  return {
    background: token("--rex-bg", "#0d0e12"),
    foreground: token("--rex-text", "#e7e9ee"),
    cursor: token("--rex-brand", "#7c5cff"),
    selectionBackground: token("--rex-selection", "rgba(124, 92, 255, 0.3)"),
  };
}

/**
 * A terminal that OUTLIVES its React component — the same rule the services
 * follow. Switching to another tab unmounts `SiteTerminal`, and the first
 * version killed the PTY and disposed the xterm in its cleanup: a `composer
 * install` died on an alt-tab and its output was gone on the way back. So the
 * shell, its scrollback and its xterm live in this module keyed by site, and the
 * component only borrows them.
 *
 * The DOM node is MOVED, never re-created: xterm renders into the element it was
 * opened with and cannot be `open()`ed twice, so the container is parked in an
 * offscreen holder while nothing shows it. Parked, not detached — a node removed
 * from the document measures as 0×0 and xterm's renderer sizes itself from that.
 */
type Live = {
  term: XTerm;
  fit: FitAddon;
  container: HTMLDivElement;
  /** PTY id, null until `openTerminal` answers (or if it failed). */
  sessionId: string | null;
  unlisten: (() => void) | null;
  /** Set by `dispose`: an `openTerminal` still in flight closes its id instead
   *  of handing a live shell to a session nobody can reach. */
  disposedFlag: boolean;
  dispose: () => void;
  error: string | null;
  lastUsed: number;
};

const live = new Map<string, Live>();

/**
 * How many shells may be kept alive at once. Each one is a real login shell with
 * the user's whole rc behind it, so this is not free — and a session nobody has
 * looked at in five sites' time is worth less than the memory it holds. Evicted
 * sessions come back on the next visit, which is exactly what every session did
 * before this store existed.
 */
const MAX_LIVE = 4;

let holder: HTMLDivElement | null = null;
/** Offscreen parking bay: in the document (so measurement works), never visible. */
function parkingBay(): HTMLDivElement {
  if (!holder) {
    holder = document.createElement("div");
    holder.setAttribute("data-rex-terminal-parking", "");
    holder.style.cssText = "position:absolute;left:-99999px;top:0;width:800px;height:600px;";
    document.body.appendChild(holder);
  }
  return holder;
}

function evictIdle(keep: string) {
  while (live.size > MAX_LIVE) {
    let oldest: string | null = null;
    for (const [key, s] of live) {
      if (key === keep) continue;
      if (!oldest || s.lastUsed < (live.get(oldest)?.lastUsed ?? 0)) oldest = key;
    }
    if (!oldest) return;
    live.get(oldest)?.dispose();
    live.delete(oldest);
  }
}

function createLive(siteId: string, onError: (e: string) => void): Live {
  const container = document.createElement("div");
  container.style.cssText = "width:100%;height:100%;";
  parkingBay().appendChild(container);

  const term = new XTerm({
    fontFamily: token("--rex-font-mono", "ui-monospace, monospace"),
    // xterm is canvas-rendered (px only) — follow the html font-size scale
    // (globals.css) manually: 12px × the root scale, rounded.
    fontSize: Math.round(12 * (parseFloat(getComputedStyle(document.documentElement).fontSize) / 16)),
    cursorBlink: true,
    // Deep enough that a long `composer install` is still readable after a
    // detour through another tab — the whole point of keeping the session.
    scrollback: 5000,
    theme: xtermTheme(),
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(container);
  fit.fit();

  const entry: Live = {
    term,
    fit,
    container,
    sessionId: null,
    unlisten: null,
    disposedFlag: false,
    error: null,
    lastUsed: Date.now(),
    dispose: () => {
      entry.disposedFlag = true;
      entry.unlisten?.();
      onData.dispose();
      if (entry.sessionId) void closeTerminal(entry.sessionId);
      term.dispose();
      entry.container.remove();
    },
  };

  const onData = term.onData((d) => {
    if (entry.sessionId) void writeTerminal(entry.sessionId, d);
  });

  void (async () => {
    try {
      const id = await openTerminal(siteId, term.rows, term.cols);
      // The component may be long gone; the session is not tied to it. Only a
      // dispose (Restart / eviction) makes this id unwanted, and dispose sets
      // `disposed` so the fresh id is closed instead of leaked.
      if (entry.disposedFlag) {
        void closeTerminal(id);
        return;
      }
      entry.sessionId = id;
      const un = await onTerminalOutput(id, (bytes) => term.write(bytes));
      if (entry.disposedFlag) {
        un();
        return;
      }
      entry.unlisten = un;
    } catch (e) {
      entry.error = String(e);
      onError(String(e));
    }
  })();

  return entry;
}

/** An interactive xterm.js terminal bound to a site's PTY session (§4.2). */
export function SiteTerminal({ siteId }: { siteId: string }) {
  const mountRef = useRef<HTMLDivElement>(null);
  const [restartN, setRestartN] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) {
      setError("The terminal requires the rexenv desktop app.");
      return;
    }
    const mount = mountRef.current;
    if (!mount) return;

    let entry = live.get(siteId);
    if (!entry) {
      entry = createLive(siteId, setError);
      live.set(siteId, entry);
      evictIdle(siteId);
    }
    const session = entry;
    session.lastUsed = Date.now();
    setError(session.error);
    mount.appendChild(session.container);
    try {
      session.fit.fit();
      if (session.sessionId) void resizeTerminal(session.sessionId, session.term.rows, session.term.cols);
    } catch {
      /* mount still sizing */
    }

    // Keep the PTY size in sync with the rendered viewport.
    const ro = new ResizeObserver(() => {
      try {
        session.fit.fit();
        if (session.sessionId) void resizeTerminal(session.sessionId, session.term.rows, session.term.cols);
      } catch {
        /* mount detaching */
      }
    });
    ro.observe(mount);

    // Re-read token values when the app theme flips (data-theme on <html>).
    const mo = new MutationObserver(() => {
      session.term.options.theme = xtermTheme();
    });
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

    return () => {
      mo.disconnect();
      ro.disconnect();
      session.lastUsed = Date.now();
      // Park it — the shell keeps running and the scrollback comes back with it.
      parkingBay().appendChild(session.container);
    };
  }, [siteId, restartN]);

  const restart = () => {
    const entry = live.get(siteId);
    if (entry) {
      entry.dispose();
      live.delete(siteId);
    }
    setError(null);
    setRestartN((n) => n + 1);
  };

  if (error) {
    return (
      <Placeholder
        icon={<TerminalSquare className="h-[22px] w-[22px]" strokeWidth={1.6} />}
        label="Terminal unavailable"
        hint={error}
      />
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-rex-border bg-rex-bg">
      <div className="flex items-center justify-between border-b border-rex-border bg-rex-surface-1 px-3 py-2">
        <span className="font-mono text-[0.71875rem] text-rex-text-muted">bundled php + wp on PATH</span>
        <div className="flex items-center gap-2">
          <button
            onClick={() => live.get(siteId)?.term.clear()}
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand"
          >
            <Eraser className="h-3.5 w-3.5" />
            Clear
          </button>
          <button
            onClick={restart}
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand"
          >
            <RotateCw className="h-3.5 w-3.5" />
            Restart
          </button>
        </div>
      </div>
      <div ref={mountRef} className="min-h-0 flex-1 p-2" />
    </div>
  );
}
