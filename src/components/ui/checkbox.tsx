import { Check } from "lucide-react";

/** Token-styled checkbox (UI-REVIEW §C1.5): the native control renders as a
 *  dim OS widget in WKWebView's dark mode. Keeps a real `<input>` for
 *  semantics/labels; the visual is drawn with the design tokens. */
export function Checkbox({
  checked,
  onCheckedChange,
  id,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  id?: string;
}) {
  return (
    <span className="relative inline-flex h-4 w-4 shrink-0">
      <input
        id={id}
        type="checkbox"
        checked={checked}
        onChange={(e) => onCheckedChange(e.target.checked)}
        className="peer h-4 w-4 cursor-pointer appearance-none rounded-[4px] border border-rex-border-strong bg-rex-surface-1 transition-colors checked:border-brand checked:bg-brand focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-brand"
      />
      <Check
        className="pointer-events-none absolute left-0.5 top-0.5 h-3 w-3 text-white opacity-0 transition-opacity peer-checked:opacity-100"
        strokeWidth={3}
      />
    </span>
  );
}
