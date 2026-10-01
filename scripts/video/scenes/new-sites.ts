/** A fresh Laravel site and a Blank PHP site with its starter database, each
 *  replaying phase_defs() for its type (`commands/site_provision.rs`):
 *  Laravel — prepare, fetch, db, app_install, configure, finalize, serve;
 *  Blank PHP + starter DB — prepare, fetch, db, configure, serve. Composer and
 *  artisan output is streamed verbatim in the real job; counts here are
 *  illustrative. */
import type { SceneCtx } from "../demo-backend";
import type { Site, SiteProvisionState } from "@/types";

type Args = Record<string, unknown> | undefined;
type Phase = [key: string, label: string, lines: string[], end: string, ms: number];

const slug = (d: string) => d.replace(/[^a-z0-9]/gi, "_");

function laravel(domain: string): Phase[] {
  const db = `lv_${slug(domain)}`;
  return [
    ["fetch", "downloading binaries", ["binaries already cached"], "skipped", 300],
    ["db", "starting database", [], "ok", 400],
    [
      "app_install",
      "installing Laravel",
      [
        "$ composer create-project laravel/laravel .",
        'Creating a "laravel/laravel" project at "./"',
        "Installing laravel/laravel (v12.4.0)",
        "  - Installing laravel/laravel (v12.4.0): Extracting archive",
        "Lock file operations: 110 installs, 0 updates, 0 removals",
        "Generating optimized autoload files",
        "> @php artisan key:generate --ansi",
        "INFO  Application key set successfully.",
      ],
      "ok",
      230,
    ],
    [
      "configure",
      "creating database + .env",
      [`created database \`${db}\``, "kept the original .env as .env.rexenv-backup", ".env wired to this site's database, URL and the Mailpit catch-all"],
      "ok",
      300,
    ],
    [
      "finalize",
      "running migrations",
      [
        "$ php artisan migrate --force",
        "INFO  Preparing database.",
        "Creating migration table .................................... 10.87ms DONE",
        "INFO  Running migrations.",
        "0001_01_01_000000_create_users_table ........................ 25.13ms DONE",
        "0001_01_01_000001_create_cache_table ......................... 7.44ms DONE",
        "0001_01_01_000002_create_jobs_table ......................... 19.02ms DONE",
        "migrations applied to the site's database",
      ],
      "ok",
      220,
    ],
    ["serve", "starting to serve", [], "ok", 600],
  ];
}

function blankPhp(domain: string): Phase[] {
  return [
    ["fetch", "downloading binaries", ["binaries already cached"], "skipped", 300],
    ["db", "starting database", [], "ok", 400],
    ["configure", "creating database + sample data", [`created database \`php_${slug(domain)}\` and seeded \`starter_items\``], "ok", 600],
    ["serve", "starting to serve", [], "ok", 600],
  ];
}

export default function newSites(ctx: SceneCtx) {
  let job: SiteProvisionState | null = null;
  return {
    site_provision_active: () => (job && job.status === "running" ? job : null),
    site_provision_job: (a: Args) => {
      const input = a?.site as { name: string; domain: string; type: Site["type"]; phpVersion: string; webServer: Site["webServer"]; dbEngine: Site["dbEngine"] };
      const id = String(ctx.sites.length + 1);
      const plan = input.type === "laravel" ? laravel(input.domain) : blankPhp(input.domain);
      const site = ctx.site({
        id,
        name: input.name,
        domain: input.domain,
        type: input.type,
        phpVersion: input.phpVersion,
        dbName: input.type === "laravel" ? `lv_${slug(input.domain)}` : `php_${slug(input.domain)}`,
        starterDb: input.type === "php" ? true : null,
        status: "stopped",
        provisioned: false,
        createdAt: "2026-09-30 12:20:00",
      });
      ctx.sites.push(site);
      const j: SiteProvisionState = {
        id: `prov-${id}`,
        domain: input.domain,
        siteId: id,
        phases: [{ key: "prepare", label: "preparing site (domain, certificate)", status: "ok" }, ...plan.map(([key, label]) => ({ key, label, status: "pending" as const }))],
        phaseCursor: 0,
        pct: 5,
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
        for (const [i, [, label, lines, end, ms]] of plan.entries()) {
          j.phases[i + 1].status = "running";
          state({ phaseCursor: i + 1 });
          line(`── ${label}`);
          for (const l of lines) {
            await ctx.sleep(ms);
            line(l);
          }
          await ctx.sleep(ms);
          j.phases[i + 1].status = end as "ok";
          state({ pct: Math.round(((i + 1) / plan.length) * 94) + 5 });
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
