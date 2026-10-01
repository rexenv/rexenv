/** Sharing: every site idle; `start_tunnel` takes a few seconds (cloudflared
 *  reporting its URL), comes back `unverified` with the dns-propagating
 *  diagnosis the first probe gives a fresh link, and turns `reachable` a
 *  moment later (`commands/tunnels.rs`: first probe at once, then every 30 s).
 *  Agency Blog still accepts admin/admin, so its live card carries the warning
 *  (`wp user check-password admin admin`). */
import type { SceneCtx } from "../demo-backend";
import type { TunnelInfo } from "@/types";

export default function share(ctx: SceneCtx) {
  const live = new Map<string, TunnelInfo>();
  const URLS: Record<string, string> = {
    "agency-blog.rex": "https://harbor-lemon-quiet-meadow.trycloudflare.com",
  };
  return {
    tunnels_status: () => [...live.values()].sort((a, b) => a.domain.localeCompare(b.domain)),
    wp_default_creds: (args: Record<string, unknown> | undefined) => {
      const s = ctx.sites.find((x) => x.id === args?.id);
      return s?.domain === "agency-blog.rex";
    },
    start_tunnel: async (args: Record<string, unknown> | undefined) => {
      const s = ctx.sites.find((x) => x.id === args?.id)!;
      await ctx.sleep(3200);
      const info: TunnelInfo = {
        domain: s.domain,
        url: URLS[s.domain] ?? "https://amber-river-soft-lantern.trycloudflare.com",
        running: true,
        health: "unverified",
        diagnosis: "dns-propagating",
        warning: null,
      };
      live.set(s.domain, info);
      setTimeout(() => {
        const t = live.get(s.domain);
        if (t) Object.assign(t, { health: "reachable", diagnosis: null });
      }, 5000);
      return info;
    },
    stop_tunnel: async (args: Record<string, unknown> | undefined) => {
      const s = ctx.sites.find((x) => x.id === args?.id)!;
      await ctx.sleep(900);
      live.delete(s.domain);
      return null;
    },
    open_external: () => null,
  };
}
