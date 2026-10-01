/** Multisite: Agency Blog converted in place from WordPress → Network
 *  (`wp_multisite_convert` → `wp core multisite-convert --subdomains`), then a
 *  sub-site created (`wp site create --slug=shop`). Sub-site URLs follow the
 *  mode: subdomain → https://<slug>.<domain>/. DNS answers every .rex name
 *  with 127.0.0.1 and each site's certificate carries `*.domain`, so a new
 *  subdomain needs no setup (`core/dns.rs`, `core/ssl.rs`). */
import type { SceneCtx } from "../demo-backend";
import type { WpNetworkSite } from "@/types";
import { wpHandlers } from "../wp-fixtures";

type Args = Record<string, unknown> | undefined;

export default function multisite(ctx: SceneCtx) {
  const sites: WpNetworkSite[] = [{ id: "1", url: "https://agency-blog.rex/", registered: "2026-08-21 09:30:00", deleted: false }];
  return {
    ...wpHandlers(ctx),
    wp_multisite_convert: async (a: Args) => {
      await ctx.sleep(1800);
      const s = ctx.sites.find((x) => x.id === a?.id)!;
      s.multisite = a?.mode as typeof s.multisite;
      return s;
    },
    wp_network_sites: () => sites,
    wp_network_site_create: async (a: Args) => {
      await ctx.sleep(1400);
      const slug = String(a?.slug);
      sites.push({ id: String(sites.length + 1), url: `https://${slug}.agency-blog.rex/`, registered: "2026-09-30 11:40:00", deleted: false });
      return null;
    },
    wp_themes_network_enabled: () => ["twentytwentyfive"],
    wp_super_admins: () => ["admin"],
  };
}
