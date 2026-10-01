/** Stopping one site: `set_site_enabled` records the choice on the site
 *  (enabled=false) and takes it off the serving surface; the shared services
 *  keep running (`docs/archive/PLAN-per-site-lifecycle.md`). The serving
 *  report then says `{ serving:false, disabled:true }`. */
import type { SceneCtx } from "../demo-backend";

type Args = Record<string, unknown> | undefined;

export default function siteStop(ctx: SceneCtx) {
  const serving = () => ctx.sites.map((s) => ({ domain: s.domain, serving: s.enabled !== false && s.provisioned, disabled: s.enabled === false }));
  return {
    sites_serving: () => serving(),
    set_site_enabled: async (a: Args) => {
      await ctx.sleep(700);
      const s = ctx.sites.find((x) => x.id === a?.id)!;
      s.enabled = Boolean(a?.enabled);
      return { enabled: s.enabled, serving: s.enabled, note: null, ownBackend: false };
    },
  };
}
