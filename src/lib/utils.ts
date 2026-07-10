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
