import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * A detected desktop app's OWN icon (a `data:` PNG the backend read off the
 * installed bundle), with an honest fallback.
 *
 * `icon` is null whenever the bundle kept its artwork somewhere we can't read
 * (a compiled asset catalog). That is a real, expected outcome — so the
 * fallback is the caller's own monochrome glyph, never a guessed brand mark or
 * a broken-image frame. `alt=""` because the label always sits next to it; a
 * screen reader reading "Google Chrome" twice is noise.
 */
export function AppIcon({
  icon,
  fallback,
  className,
}: {
  icon: string | null | undefined;
  fallback: ReactNode;
  className?: string;
}) {
  if (!icon) return <>{fallback}</>;
  return (
    <img
      src={icon}
      alt=""
      draggable={false}
      // Icons are square PNGs; object-contain keeps a non-square one honest
      // rather than stretching it.
      className={cn("object-contain", className ?? "h-4 w-4")}
    />
  );
}
