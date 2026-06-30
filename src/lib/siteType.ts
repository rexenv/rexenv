/**
 * Per-site-type avatar tokens: a letter on a tinted square.
 * WordPress = blue, Laravel = red, Blank-PHP = purple (per the design comps).
 */
export const SITE_TYPE_META: Record<
  string,
  { letter: string; bg: string; color: string; border: string }
> = {
  wordpress: { letter: "W", bg: "rgba(74,134,170,0.15)", color: "#7DB8D8", border: "rgba(74,134,170,0.30)" },
  laravel: { letter: "L", bg: "rgba(224,82,77,0.13)", color: "#EE837C", border: "rgba(224,82,77,0.27)" },
  php: { letter: "P", bg: "rgba(125,128,185,0.17)", color: "#A7AADD", border: "rgba(125,128,185,0.32)" },
};

export function siteTypeMeta(type: string) {
  return SITE_TYPE_META[type] ?? SITE_TYPE_META.php;
}
