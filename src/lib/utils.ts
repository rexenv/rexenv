import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** Merge Tailwind class names, resolving conflicts (shadcn/ui convention). */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Browser text-mangling OFF for technical inputs. macOS WKWebView
 *  auto-capitalizes ("test1.site" → "Test1.site") and auto-corrects, which
 *  breaks values that must be exact — domains, paths, slugs, credentials,
 *  search. Spread onto every text input: `<input {...TECH_INPUT} … />`;
 *  override individual keys after the spread where needed (e.g.
 *  `autoComplete="new-password"`). */
export const TECH_INPUT = {
  autoCapitalize: "none",
  autoCorrect: "off",
  autoComplete: "off",
  spellCheck: false,
} as const;

/** The app's checkbox. A BARE `<input type="checkbox">` renders at WebKit's own
 *  ~12px with no accent, which next to these 16px brand-accented ones read as a
 *  different kind of control (QA, 11 Aug 2026 — the install bar's "Activate").
 *  Lived as two identical copies in Import.tsx and WordPressManager.tsx, which
 *  is how a third place ends up with the browser default instead: one string,
 *  imported. */
export const CHECK_INPUT =
  "h-4 w-4 shrink-0 cursor-pointer accent-brand disabled:cursor-not-allowed disabled:opacity-40";
