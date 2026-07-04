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

/** An interactive xterm.js terminal bound to a site's PTY session (§4.2). */
export function SiteTerminal({ siteId }: { siteId: string }) {
  const mountRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XTerm | null>(null);
  const [restartN, setRestartN] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) {
      setError("The terminal requires the rexenv desktop app.");
      return;
    }
    const mount = mountRef.current;
    if (!mount) return;
    setError(null);

    const term = new XTerm({
      fontFamily: token("--rex-font-mono", "ui-monospace, monospace"),
      fontSize: 12,
      cursorBlink: true,
      theme: {
        background: token("--rex-bg", "#0d0e12"),
        foreground: token("--rex-text", "#e7e9ee"),
        cursor: token("--rex-brand", "#7c5cff"),
        selectionBackground: "rgba(124, 92, 255, 0.3)",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(mount);
    fit.fit();
    termRef.current = term;

    let sessionId: string | null = null;
    let unlisten: (() => void) | null = null;
    let disposed = false;
    const onData = term.onData((d) => {
      if (sessionId) void writeTerminal(sessionId, d);
    });

    (async () => {
      try {
        const id = await openTerminal(siteId, term.rows, term.cols);
        if (disposed) {
          void closeTerminal(id);
          return;
        }
        sessionId = id;
        unlisten = await onTerminalOutput(id, (bytes) => term.write(bytes));
      } catch (e) {
        setError(String(e));
      }
    })();

    // Keep the PTY size in sync with the rendered viewport.
    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
        if (sessionId) void resizeTerminal(sessionId, term.rows, term.cols);
      } catch {
        /* mount detaching */
      }
    });
    ro.observe(mount);

    return () => {
      disposed = true;
      ro.disconnect();
      onData.dispose();
      if (unlisten) unlisten();
      if (sessionId) void closeTerminal(sessionId);
      term.dispose();
      termRef.current = null;
    };
  }, [siteId, restartN]);

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
        <span className="font-mono text-[11.5px] text-rex-text-muted">bundled php + wp on PATH</span>
        <div className="flex items-center gap-2">
          <button
            onClick={() => termRef.current?.clear()}
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[12px] text-rex-text transition-colors hover:border-brand"
          >
            <Eraser className="h-3.5 w-3.5" />
            Clear
          </button>
          <button
            onClick={() => setRestartN((n) => n + 1)}
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[12px] text-rex-text transition-colors hover:border-brand"
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
