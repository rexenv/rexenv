/** The WordPress manager on Agency Blog. An install replays
 *  `wp plugin install woocommerce --activate`: wp-cli's own lines on
 *  `wp-install://output/{id}`, a state event on the item header and whenever
 *  pct moves (0/20/40/60/80/99, 100 on "Success: Installed"), then settle
 *  (`commands/wp_install.rs`, `core/wordpress.rs` milestones). An update
 *  replays `wp plugin update`'s lines as `wp-update://plugins/{site}`
 *  snapshots with UPDATE_STEPS' phases and fractions. */
import type { SceneCtx } from "../demo-backend";
import type { WpInstallState, WpOrgPlugin, WpUpdateProgress } from "@/types";
import { wpHandlers, wpState } from "../wp-fixtures";

type Args = Record<string, unknown> | undefined;

const SEARCH: WpOrgPlugin[] = [
  { slug: "woocommerce", name: "WooCommerce", author: "Automattic", rating: 90, numRatings: 4700, activeInstalls: 7000000, icon: null, shortDescription: "Everything you need to launch an online store." },
  { slug: "woocommerce-gateway-stripe", name: "WooCommerce Stripe Payment Gateway", author: "WooCommerce", rating: 76, numRatings: 380, activeInstalls: 900000, icon: null, shortDescription: "Accept cards and more with Stripe." },
  { slug: "woocommerce-payments", name: "WooPayments", author: "WooCommerce", rating: 70, numRatings: 280, activeInstalls: 600000, icon: null, shortDescription: "Payments made for WooCommerce." },
];

export default function wpManager(ctx: SceneCtx) {
  const st = wpState();
  const base = wpHandlers(ctx, st);

  const install = (a: Args): WpInstallState => {
    const slugs = a?.slugs as string[];
    const activate = Boolean(a?.activate);
    const id = "wpi-demo-1";
    const job: WpInstallState = {
      id,
      siteId: String(a?.siteId),
      kind: "plugin",
      source: "wporg",
      slugs,
      itemsTotal: slugs.length,
      itemCursor: 0,
      pct: 0,
      status: "running",
      summary: null,
      error: null,
      blockedBy: null,
      logKey: `wp-install-agency-blog.rex-${id}.log`,
    };
    const state = (p: Partial<WpInstallState>) => {
      Object.assign(job, p);
      void ctx.emit(`wp-install://state/${id}`, structuredClone(job));
    };
    const line = (l: string) => void ctx.emit(`wp-install://output/${id}`, l);
    const script: Array<[string, number | null, number]> = [
      ["Installing WooCommerce (10.2.1)", 0, 500],
      ["Downloading installation package from https://downloads.wordpress.org/plugin/woocommerce.10.2.1.zip...", 20, 1600],
      ["Unpacking the package...", 40, 900],
      ["Installing the plugin...", 60, 700],
      ["Plugin installed successfully.", 80, 500],
      ...(activate
        ? ([
            ["Activating 'woocommerce'...", 99, 900],
            ["Plugin 'woocommerce' activated.", null, 400],
          ] as Array<[string, number | null, number]>)
        : []),
      ["Success: Installed 1 of 1 plugins.", 100, 300],
    ];
    // Subscribed only after this call resolves and the card mounts.
    setTimeout(async () => {
      for (const [l, pct, ms] of script) {
        await ctx.sleep(ms);
        line(l);
        if (l.startsWith("Installing WooCommerce")) state({ itemCursor: 1, pct: 0 });
        else if (pct !== null && pct !== 100 && pct !== job.pct) state({ pct });
      }
      st.plugins.push({ name: "woocommerce", status: activate ? "active" : "inactive", version: "10.2.1", update: "none", updateVersion: "", title: "WooCommerce", file: "woocommerce/woocommerce.php" });
      state({ status: "ok", pct: 100, summary: "Success: Installed 1 of 1 plugins." });
    }, 400);
    return structuredClone(job);
  };

  const update = async (a: Args) => {
    const names = a?.names as string[];
    const siteId = String(a?.id);
    const p = st.plugins.find((x) => x.name === names[0])!;
    const to = p.updateVersion;
    const snap = (phase: string, step: number, line: string, done = 0) =>
      void ctx.emit(`wp-update://plugins/${siteId}`, { total: 1, done, current: p.name, phase, fraction: (done + step) / 1, line } satisfies WpUpdateProgress);
    const steps: Array<[string, number, string, number]> = [
      ["Preparing", 0.05, "Enabling Maintenance mode...", 500],
      ["Downloading", 0.15, `Downloading update from https://downloads.wordpress.org/plugin/${p.name}.${to}.zip...`, 1400],
      ["Unpacking", 0.55, "Unpacking the update...", 700],
      ["Installing", 0.75, "Installing the latest version...", 700],
      ["Cleaning up", 0.9, "Removing the old version of the plugin...", 500],
    ];
    for (const [phase, step, line, ms] of steps) {
      snap(phase, step, line);
      await ctx.sleep(ms);
    }
    snap("Updated", 0, "Plugin updated successfully.", 1);
    await ctx.sleep(400);
    snap("Finishing", 0, "Disabling Maintenance mode...", 1);
    await ctx.sleep(400);
    Object.assign(p, { version: to, update: "none", updateVersion: "" });
    return null;
  };

  return {
    ...base,
    wp_org_search_plugins: (a: Args) => {
      const q = String(a?.query ?? "").toLowerCase();
      return SEARCH.filter((x) => x.slug.includes(q) || x.name.toLowerCase().includes(q));
    },
    wp_install_job: (a: Args) => install(a),
    wp_install_cancel: () => null,
    wp_plugin_update: (a: Args) => update(a),
    wp_plugin_activate: (a: Args) => {
      for (const n of a?.names as string[]) st.plugins.find((x) => x.name === n)!.status = "active";
      return null;
    },
    wp_plugin_deactivate: (a: Args) => {
      for (const n of a?.names as string[]) st.plugins.find((x) => x.name === n)!.status = "inactive";
      return null;
    },
    wp_user_login_url: (a: Args) => `https://agency-blog.rex/?rexenv_login=5b1e0c9a-7d2f-4e8b-a3c6-2f9d8e1b4a70&rexenv_user=${a?.userId}`,
  };
}
