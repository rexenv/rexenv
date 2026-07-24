import type { CSSProperties } from "react";
import logoUrl from "@/assets/rexenv-logo.svg";

/**
 * The rexenv brand mark (crowned R). Bundled as a same-origin asset — the
 * production CSP is `img-src 'self' data: https://*.w.org`, so a Vite-hashed
 * local URL loads; a remote logo URL never would.
 *
 * Sizing is left to the caller's class (the mark is square, so a single axis
 * plus `w-auto`/`h-auto` is enough). Decorative everywhere it is used today —
 * the word "rexenv" always sits next to it — hence `alt=""` + `aria-hidden`.
 */
export function RexLogo({
  className,
  style,
}: {
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <img
      src={logoUrl}
      alt=""
      aria-hidden
      draggable={false}
      className={className}
      style={style}
    />
  );
}
