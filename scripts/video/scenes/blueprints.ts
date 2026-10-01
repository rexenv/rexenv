/** Blueprints: saved from Settings → General → Blueprints (`save_blueprint`),
 *  picked in New site → "Start from blueprint", applied by the provision job's
 *  `blueprint` phase ("applying blueprint", after core_install) — wp-cli's own
 *  install output per item (`core/blueprints.rs`, `site_provision.rs`). */
import type { SceneCtx } from "../demo-backend";
import type { Blueprint, Site, SiteProvisionState } from "@/types";

type Args = Record<string, unknown> | undefined;

const PHASES: Array<[string, string, string[], string, number]> = [
  ["fetch", "downloading binaries", ["binaries already cached"], "skipped", 400],
  ["db", "starting database", [], "ok", 500],
  ["core_download", "downloading WordPress core", ["Downloading from https://wordpress.org/latest.zip ...", "Success: WordPress downloaded."], "ok", 500],
  ["configure", "writing wp-config + creating database", ["Success: Generated 'wp-config.php' file."], "ok", 500],
  ["core_install", "installing WordPress", ["Success: WordPress installed successfully."], "ok", 600],
  [
    "blueprint",
    "applying blueprint",
    [
      "Installing WooCommerce (10.2.1)",
      "Downloading installation package from https://downloads.wordpress.org/plugin/woocommerce.10.2.1.zip...",
      "Unpacking the package...",
      "Installing the plugin...",
      "Plugin installed successfully.",
      "Activating 'woocommerce'...",
      "Plugin 'woocommerce' activated.",
      "Success: Installed 1 of 1 plugins.",
      "Installing Storefront (4.6.1)",
      "Downloading installation package from https://downloads.wordpress.org/theme/storefront.4.6.1.zip...",
      "Theme installed successfully.",
      "Success: Installed 1 of 1 themes.",
    ],
    "ok",
    260,
  ],
  ["serve", "starting to serve", [], "ok", 700],
];

export default function blueprints(ctx: SceneCtx) {
  const list: Blueprint[] = [];
  let job: SiteProvisionState | null = null;
  return {
    list_blueprints: () => list,
    save_blueprint: async (a: Args) => {
      await ctx.sleep(400);
      list.unshift(a?.blueprint as Blueprint);
      return null;
    },
    delete_blueprint: () => true,
    site_provision_active: () => (job && job.status === "running" ? job : null),
    site_provision_job: (a: Args) => {
      const input = a?.site as { name: string; domain: string; type: Site["type"]; phpVersion: string; webServer: Site["webServer"]; dbEngine: Site["dbEngine"] };
      const id = String(ctx.sites.length + 1);
      const site = ctx.site({ id, name: input.name, domain: input.domain, type: "wordpress", phpVersion: input.phpVersion, status: "stopped", provisioned: false, createdAt: "2026-09-30 12:10:00" });
      ctx.sites.push(site);
      const j: SiteProvisionState = {
        id: `prov-${id}`,
        domain: input.domain,
        siteId: id,
        phases: [{ key: "prepare", label: "preparing site (domain, certificate)", status: "ok" }, ...PHASES.map(([key, label]) => ({ key, label, status: "pending" as const }))],
        phaseCursor: 0,
        pct: 4,
        status: "running",
        summary: null,
        error: null,
        logKey: `provision-${id}`,
        downloadIds: [],
      };
      job = j;
      const state = (p: Partial<SiteProvisionState>) => {
        Object.assign(j, p);
        void ctx.emit(`site-provision://state/${j.id}`, structuredClone(j));
      };
      const line = (l: string) => void ctx.emit(`site-provision://output/${j.id}`, l);
      setTimeout(async () => {
        line(`── preparing site (${j.domain}) — done`);
        const pcts = [12, 20, 40, 55, 72, 94, 99];
        for (const [i, [, label, lines, end, ms]] of PHASES.entries()) {
          j.phases[i + 1].status = "running";
          state({ phaseCursor: i + 1 });
          line(`── ${label}`);
          for (const l of lines) {
            await ctx.sleep(ms);
            line(l);
          }
          await ctx.sleep(ms);
          j.phases[i + 1].status = end as "ok";
          state({ pct: pcts[i] });
        }
        site.provisioned = true;
        site.status = "running";
        await ctx.sleep(700);
        state({ status: "ok", pct: 100, summary: `created — serving at https://${j.domain}` });
      }, 350);
      return structuredClone(j);
    },
  };
}
