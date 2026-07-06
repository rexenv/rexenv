import { useSyncExternalStore } from "react";
import { Monitor, Moon, Sun } from "lucide-react";
import { getStoredTheme, setTheme, subscribeTheme, type Theme } from "@/lib/theme";

const ORDER: Theme[] = ["light", "dark", "system"];
const META: Record<Theme, { icon: typeof Sun; label: string }> = {
  light: { icon: Sun, label: "Light" },
  dark: { icon: Moon, label: "Dark" },
  system: { icon: Monitor, label: "System" },
};

/** Compact sidebar theme switch — cycles Light → Dark → System. Shares the
 *  Settings screen's source of truth (lib/theme), so both stay in sync. */
export function ThemeToggle() {
  const theme = useSyncExternalStore(subscribeTheme, getStoredTheme);
  const next = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length];
  const Icon = META[theme].icon;
  return (
    <button
      onClick={() => setTheme(next)}
      aria-label={`Theme: ${META[theme].label}. Switch to ${META[next].label}`}
      title={`Theme: ${META[theme].label} · click for ${META[next].label}`}
      className="flex h-[30px] w-[30px] flex-none items-center justify-center rounded-[9px] text-rex-text-muted transition-colors hover:bg-rex-hover hover:text-rex-text-bright"
    >
      <Icon className="h-4 w-4" strokeWidth={1.7} />
    </button>
  );
}
