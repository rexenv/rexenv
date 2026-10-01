/** Domain endings and extra names. Settings → DNS & SSL → "Default domain
 *  ending" stores the TLD new sites get (`set_default_tld`); `.local` is
 *  refused by policy (`core/tld.rs`). A site's Settings → Domains card adds an
 *  alias and answers with the full new list (`add_site_domain`). */
import type { SceneCtx } from "../demo-backend";

type Args = Record<string, unknown> | undefined;

export default function domains(ctx: SceneCtx) {
  let tld = "rex";
  const extras: Record<string, string[]> = {};
  const list = (id: string) => [ctx.sites.find((s) => s.id === id)!.domain, ...(extras[id] ?? [])];
  return {
    default_tld: () => tld,
    set_default_tld: async (a: Args) => {
      await ctx.sleep(400);
      tld = String(a?.tld);
      return tld;
    },
    tld_policy: (a: Args) =>
      a?.tld === "local"
        ? { allowed: false, warn: false, reason: ".local is used by Bonjour/mDNS — shadowing it breaks printers, AirDrop and other local-network discovery" }
        : { allowed: true, warn: false, reason: "" },
    site_domains: (a: Args) => list(String(a?.id)),
    all_site_domains: () => Object.fromEntries(Object.entries(extras).filter(([, v]) => v.length)),
    add_site_domain: async (a: Args) => {
      await ctx.sleep(1300);
      const id = String(a?.id);
      (extras[id] ??= []).push(String(a?.domain));
      return list(id);
    },
    remove_site_domain: (a: Args) => {
      const id = String(a?.id);
      extras[id] = (extras[id] ?? []).filter((d) => d !== a?.domain);
      return list(id);
    },
  };
}
