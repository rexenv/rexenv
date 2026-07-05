import { useState } from "react";
import { AlertCircle, Check, CheckCircle2, Copy, Info, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useToastStore, type ToastKind } from "@/lib/toast";

const KIND: Record<ToastKind, { Icon: typeof AlertCircle; border: string; color: string }> = {
  error: { Icon: AlertCircle, border: "border-status-error-border", color: "var(--rex-error-bright)" },
  success: { Icon: CheckCircle2, border: "border-status-running-border", color: "var(--rex-running-bright)" },
  info: { Icon: Info, border: "border-rex-border-strong", color: "var(--rex-brand-tint)" },
};

/** Toast overlay — mounted once (App). Renders the toast store bottom-right. */
export function Toaster() {
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[70] flex w-[360px] flex-col gap-2">
      {toasts.map((t) => {
        const { Icon, border, color } = KIND[t.kind];
        return (
          <div
            key={t.id}
            className={cn(
              "pointer-events-auto flex items-start gap-2.5 rounded-[10px] border bg-rex-surface-1 px-3.5 py-3 shadow-menu",
              border,
            )}
          >
            <Icon className="mt-px h-4 w-4 flex-none" style={{ color }} strokeWidth={1.9} />
            <div className="min-w-0 flex-1">
              <span className="break-words text-[12.5px] leading-[1.5] text-rex-text-bright">
                {t.message}
              </span>
              {t.command && <CommandBlock command={t.command} />}
            </div>
            <button
              onClick={() => dismiss(t.id)}
              aria-label="Dismiss"
              className="flex h-5 w-5 flex-none items-center justify-center rounded text-rex-text-muted transition-colors hover:bg-white/[0.07] hover:text-rex-text"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}

/** A suggested fix-it shell command: monospace, horizontally scrollable, copyable. */
function CommandBlock({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="mt-2 flex items-center gap-1.5 rounded-[7px] border border-rex-border bg-rex-surface-2 py-1.5 pl-2.5 pr-1.5">
      <code className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap font-mono text-[11px] leading-[1.5] text-rex-text-bright">
        {command}
      </code>
      <button
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(command);
            setCopied(true);
            setTimeout(() => setCopied(false), 1200);
          } catch {
            /* clipboard unavailable */
          }
        }}
        aria-label="Copy command"
        className={cn(
          "flex h-6 w-6 flex-none items-center justify-center rounded transition-colors hover:bg-white/[0.07]",
          copied ? "text-status-running-bright" : "text-rex-text-muted",
        )}
      >
        {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
      </button>
    </div>
  );
}
