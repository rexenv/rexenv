import { Database } from "lucide-react";
import { Placeholder } from "@/components/common/Placeholder";
import { isTauri } from "@/lib/ipc";

/** Embeds the stack-served Adminer (framed, dark via Adminer's prefers-color-scheme).
 *  Off-Tauri the `rexdb://` proxy scheme doesn't exist, so we show a placeholder. */
export function AdminerFrame({ src }: { src: string }) {
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
    <div className="h-full min-h-0 overflow-hidden rounded-xl border border-rex-border bg-rex-bg">
      <iframe title="Adminer" src={src} className="h-full w-full border-0 bg-rex-bg" />
    </div>
  );
}
