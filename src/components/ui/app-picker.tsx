import type { ReactNode } from "react";
import { Check, ChevronDown } from "lucide-react";
import { AppIcon } from "@/components/ui/app-icon";
import { Menu, MenuItem } from "@/components/ui/menu";

export interface AppChoice {
  id: string;
  name: string;
  icon?: string | null;
  /** Secondary line, e.g. which browser "System default" currently means. */
  hint?: string;
}

/**
 * A settings picker for "which installed app do we hand things to" — the same
 * job as a `<select>`, except a native option list can't render an app's icon,
 * and the icon is the fastest way to see you picked the right Chrome.
 *
 * The trigger always shows what is IN EFFECT, including the resolved fallback
 * (an unset preference reads "System default", with the app it resolves to as
 * the hint) — a picker that showed a blank for "unset" would leave the user
 * guessing what a click will actually do.
 */
export function AppPicker({
  value,
  choices,
  onChange,
  fallbackIcon,
  ariaLabel,
  width = 240,
}: {
  /** Selected id. An id not in `choices` falls back to the first entry. */
  value: string;
  choices: AppChoice[];
  onChange: (id: string) => void;
  /** Glyph for apps whose icon couldn't be read. */
  fallbackIcon: ReactNode;
  ariaLabel: string;
  width?: number;
}) {
  const current = choices.find((c) => c.id === value) ?? choices[0];
  if (!current) return null;
  return (
    <Menu
      align="right"
      width={width}
      trigger={
        <button
          type="button"
          aria-label={ariaLabel}
          className="flex h-[34px] min-w-[180px] max-w-[240px] items-center gap-2 rounded-[9px] border border-rex-border-strong bg-rex-well px-3 text-[0.78125rem] text-rex-text outline-none transition-colors hover:border-rex-border-strong-hover focus-visible:border-brand"
        >
          <AppIcon icon={current.icon} fallback={fallbackIcon} />
          <span className="flex-1 truncate text-left">{current.name}</span>
          <ChevronDown className="h-[13px] w-[13px] flex-none text-rex-text-dim" strokeWidth={2} />
        </button>
      }
    >
      {choices.map((c) => (
        <MenuItem
          key={c.id}
          icon={<AppIcon icon={c.icon} fallback={fallbackIcon} />}
          onSelect={() => onChange(c.id)}
        >
          <span className="flex-1 truncate">
            {c.name}
            {c.hint && <span className="ml-1.5 text-rex-text-muted">{c.hint}</span>}
          </span>
          {c.id === current.id && (
            <Check className="h-[13px] w-[13px] flex-none text-brand-tint" strokeWidth={2.2} />
          )}
        </MenuItem>
      ))}
    </Menu>
  );
}
