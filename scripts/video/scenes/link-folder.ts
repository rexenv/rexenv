/** Serving an existing folder: the picker answers /Users/demo/code/crm, the
 *  inspection (`core/sites.rs` detect_project) finds Laravel serving public/,
 *  and the linked provision runs its three phases — prepare, fetch (skipped:
 *  binaries already cached), serve (`site_provision.rs` phase_defs, linked). */
import type { SceneCtx } from "../demo-backend";
import type { LinkedFolderInfo, Site, SiteProvisionState } from "@/types";

type Args = Record<string, unknown> | undefined;

const INFO: LinkedFolderInfo = {
  root: "/Users/demo/code/crm",
  servePath: "/Users/demo/code/crm/public",
  docrootRel: "public",
  siteType: "laravel",
  label: "Laravel",
  existingInstall: true,
  hasCustomValetDriver: false,
};

export default function linkFolder(ctx: SceneCtx) {
  let job: SiteProvisionState | null = null;
  return {
    "plugin:dialog|open": () => INFO.root,
    inspect_linked_folder: async () => {
      await ctx.sleep(500);
      return INFO;
    },
    site_provision_active: () => (job && job.status === "running" ? job : null),
    site_provision_job: (a: Args) => {
      const input = a?.site as { name: string; domain: string; type: Site["type"]; phpVersion: string; webServer: Site["webServer"]; dbEngine: Site["dbEngine"]; path: string };
      const id = String(ctx.sites.length + 1);
      const site = ctx.site({
        id,
        name: input.name,
        domain: input.domain,
        type: input.type,
        phpVersion: input.phpVersion,
        webServer: input.webServer,
        path: input.path,
        docrootManaged: false,
        dbName: "",
        status: "stopped",
        provisioned: false,
        createdAt: "2026-09-30 11:30:00",
      });
      ctx.sites.push(site);
      const j: SiteProvisionState = {
        id: `prov-${id}`,
        domain: input.domain,
        siteId: id,
        phases: [
          { key: "prepare", label: "preparing site (domain, certificate)", status: "ok" },
          { key: "fetch", label: "downloading binaries", status: "pending" },
          { key: "serve", label: "starting to serve", status: "pending" },
        ],
        phaseCursor: 0,
        pct: 7,
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
        j.phases[1].status = "running";
        state({ phaseCursor: 1 });
        line("── downloading binaries");
        await ctx.sleep(500);
        line("binaries already cached");
        j.phases[1].status = "skipped";
        state({ pct: 75 });
        j.phases[2].status = "running";
        state({ phaseCursor: 2 });
        line("── starting to serve");
        await ctx.sleep(1200);
        j.phases[2].status = "ok";
        state({ pct: 99 });
        site.provisioned = true;
        site.status = "running";
        await ctx.sleep(600);
        state({ status: "ok", pct: 100, summary: `created — serving at https://${j.domain}` });
      }, 350);
      return structuredClone(j);
    },
  };
}
