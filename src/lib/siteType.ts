/**
 * Per-site-type avatar tokens: a letter on a tinted square.
 * WordPress = blue, Laravel = red, Blank-PHP = purple (per the design comps).
 */
export const SITE_TYPE_META: Record<
  string,
  { letter: string; bg: string; color: string; border: string }
> = {
  wordpress: { letter: "W", bg: "var(--rex-accent-blue-bg)", color: "var(--rex-accent-blue)", border: "var(--rex-accent-blue-border)" },
  laravel: { letter: "L", bg: "var(--rex-accent-red-bg)", color: "var(--rex-accent-red)", border: "var(--rex-accent-red-border)" },
  php: { letter: "P", bg: "var(--rex-accent-periwinkle-bg)", color: "var(--rex-accent-periwinkle)", border: "var(--rex-accent-periwinkle-border)" },
};

export function siteTypeMeta(type: string) {
  return SITE_TYPE_META[type] ?? SITE_TYPE_META.php;
}
