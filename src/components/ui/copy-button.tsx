import { useState } from "react";
import { Check, Copy } from "lucide-react";

/** Copy-to-clipboard icon button with a 1.2s "copied" tick.
 *  Lives in ui/ because the destructive confirms need it INSIDE the dialog:
 *  a confirm that makes you retype a domain from memory (or from behind the
 *  modal) is a confirm people misread, and misreading is exactly what the
 *  gate exists to stop. Clipboard failures are silent — a copy that didn't
 *  land still leaves the value on screen to type. */
export function CopyButton({
  value,
  title = "Copy",
  className = "",
  probe,
}: {
  value: string;
  title?: string;
  className?: string;
  /** `data-probe` hook for the WebKit checks (scripts/wk-checks). */
  probe?: string;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      data-probe={probe}
      title={copied ? "Copied" : title}
      aria-label={copied ? "Copied" : title}
      className={`inline-flex h-5 w-5 shrink-0 items-center justify-center rounded text-rex-text-muted transition-colors hover:bg-rex-hover hover:text-rex-text ${className}`}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        } catch {
          /* clipboard unavailable */
        }
      }}
    >
      {copied ? <Check className="h-3.5 w-3.5 text-brand" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  );
}
