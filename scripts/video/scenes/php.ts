/** Multiple PHP versions: the shipped minors (7.4–8.5) from the dev mock,
 *  with 8.3 the default and one patch behind (8.3.32 → 8.3.33 offered).
 *  A site switch rewrites the site row and nothing else (`set_site_php_version`:
 *  DB + reload, no rebuild). ini settings are per MINOR, shared by every site
 *  on it; defaults are core's (`core/php.rs`: 1G / 5G / 8G / 60 / -1 / 5000). */
import type { SceneCtx } from "../demo-backend";
import type { PhpSetting, PhpVersion } from "@/types";
import { mockPhpVersions } from "@/lib/mock";

const DEFAULTS: Array<[string, string]> = [
  ["memory_limit", "1G"],
  ["upload_max_filesize", "5G"],
  ["post_max_size", "8G"],
  ["max_execution_time", "60"],
  ["max_input_time", "-1"],
  ["max_input_vars", "5000"],
];

export default function php(ctx: SceneCtx) {
  const versions: PhpVersion[] = structuredClone(mockPhpVersions);
  // Nothing on this machine serves an old patch: the 8.3 pool already runs 8.3.32.
  const v83 = versions.find((v) => v.minor === "8.3")!;
  Object.assign(v83, { serving: null, upstream: "8.3.33", updatable: "8.3.33", updateCost: null });
  // 8.2 without the upstream-only chip and its PostgreSQL cost note: one
  // offered update on screen is the one the video explains.
  Object.assign(versions.find((v) => v.minor === "8.2")!, { upstream: "8.2.33", updatable: "8.2.33", updateCost: null });
  const values: Record<string, Record<string, string | null>> = {};

  return {
    list_php_versions: () => versions,
    php_update_check: () => versions,
    set_site_php_version: async (args: Record<string, unknown> | undefined) => {
      await ctx.sleep(700);
      const s = ctx.sites.find((x) => x.id === args?.id)!;
      s.phpVersion = String(args?.version);
      return s;
    },
    get_php_settings: (args: Record<string, unknown> | undefined) => {
      const m = String(args?.minor);
      return DEFAULTS.map(([key, def]): PhpSetting => ({ key, value: values[m]?.[key] ?? null, default: def }));
    },
    apply_php_settings: async (args: Record<string, unknown> | undefined) => {
      await ctx.sleep(1400);
      const m = String(args?.minor);
      values[m] = {};
      for (const { key, value } of args?.settings as Array<{ key: string; value: string }>) values[m][key] = value;
      return null;
    },
    php_update_apply: async (args: Record<string, unknown> | undefined) => {
      await ctx.sleep(2600);
      const v = versions.find((x) => x.minor === args?.minor)!;
      Object.assign(v, { patch: String(args?.patch), updatable: null, upstream: null, serving: null });
      return { patch: String(args?.patch), restarted: true };
    },
  };
}
