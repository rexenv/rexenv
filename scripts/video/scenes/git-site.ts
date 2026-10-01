/** A Laravel site created from a Git repository. `repo_probe` answers what
 *  `git ls-remote --symref` would; the provision job replays phase_defs() for
 *  Laravel from git with migrate + assets on (`commands/site_provision.rs`):
 *  prepare → fetch → clone → db → configure → deps → finalize → assets → serve,
 *  each opened by "── {label}", with the tools' output streamed verbatim
 *  (ANSI stripped, blank lines dropped — `core/repo.rs` pump_lines). */
import type { SceneCtx } from "../demo-backend";
import type { RepoProbeResult, Site, SiteProvisionState } from "@/types";

type Args = Record<string, unknown> | undefined;

const PROBE: RepoProbeResult = {
  url: "https://github.com/acme/storefront",
  host: "github.com",
  dirName: "storefront",
  refCandidate: null,
  defaultBranch: "main",
  branches: ["main", "develop", "feature/checkout-v2"],
  tags: ["v1.0.0", "v1.1.0", "v1.2.0"],
};

const CLONE_DIR = "/Users/demo/Sites/.rexenv-clone-storefront-1f3a9c2e";

// [phase key, label, lines, ms per line]
const PLAN: Array<[string, string, string[], number]> = [
  ["fetch", "downloading binaries", ["binaries already cached"], 300],
  [
    "clone",
    "cloning the repository",
    [
      `$ git -c protocol.ext.allow=never -c protocol.file.allow=user clone --progress --no-recurse-submodules --branch main -- https://github.com/acme/storefront ${CLONE_DIR}`,
      `Cloning into '${CLONE_DIR}'...`,
      "remote: Enumerating objects: 1286, done.",
      "remote: Counting objects: 100% (1286/1286), done.",
      "remote: Compressing objects: 100% (694/694), done.",
      "Receiving objects:  42% (541/1286)",
      "Receiving objects: 100% (1286/1286), 612.40 KiB | 3.18 MiB/s, done.",
      "Resolving deltas: 100% (581/581), done.",
      "✓ Laravel — serving public/",
    ],
    170,
  ],
  ["db", "starting database", [], 500],
  [
    "configure",
    "creating database + .env",
    [
      "created database `lv_storefront_rex`",
      ".env created from the repository's .env.example",
      "kept the original .env as .env.rexenv-backup",
      ".env wired to this site's database, URL and the Mailpit catch-all",
    ],
    260,
  ],
  [
    "deps",
    "installing dependencies",
    [
      "$ composer install --no-interaction",
      "Installing dependencies from lock file (including require-dev)",
      "Verifying lock file contents can be installed on current platform.",
      "Package operations: 111 installs, 0 updates, 0 removals",
      "  - Installing doctrine/inflector (2.0.10): Extracting archive",
      "  - Installing symfony/console (v7.3.4): Extracting archive",
      "  - Installing laravel/framework (v12.31.1): Extracting archive",
      "Generating optimized autoload files",
      "> Illuminate\\Foundation\\ComposerScripts::postAutoloadDump",
      "> @php artisan package:discover --ansi",
      "INFO  Discovering packages.",
      "laravel/tinker ................................................ DONE",
      "nesbot/carbon ................................................. DONE",
      "81 packages you are using are looking for funding.",
    ],
    190,
  ],
  [
    "finalize",
    "app key + migrations",
    [
      "$ php artisan key:generate --force",
      "INFO  Application key set successfully.",
      "application key generated",
      "$ php artisan migrate --force",
      "INFO  Preparing database.",
      "Creating migration table .................................... 11.62ms DONE",
      "INFO  Running migrations.",
      "0001_01_01_000000_create_users_table ........................ 27.40ms DONE",
      "0001_01_01_000001_create_cache_table ......................... 8.05ms DONE",
      "0001_01_01_000002_create_jobs_table ......................... 20.91ms DONE",
      "2026_08_12_103512_create_products_table ..................... 14.28ms DONE",
      "migrations applied to the site's database",
    ],
    210,
  ],
  [
    "assets",
    "building front-end assets",
    [
      "$ npm install",
      "added 142 packages, and audited 143 packages in 6s",
      "found 0 vulnerabilities",
      "$ npm run build",
      "> build",
      "> vite build",
      "vite v7.1.5 building for production...",
      "✓ 54 modules transformed.",
      "public/build/manifest.json             0.27 kB │ gzip:  0.15 kB",
      "public/build/assets/app-CkG4pS1v.css  38.62 kB │ gzip:  7.41 kB",
      "public/build/assets/app-D9q2Lm0e.js   35.19 kB │ gzip: 14.08 kB",
      "✓ built in 1.12s",
    ],
    200,
  ],
  ["serve", "starting to serve", [], 700],
];

export default function gitSite(ctx: SceneCtx) {
  let job: SiteProvisionState | null = null;

  const run = async (j: SiteProvisionState, site: Site) => {
    const state = (p: Partial<SiteProvisionState>) => {
      Object.assign(j, p);
      void ctx.emit(`site-provision://state/${j.id}`, structuredClone(j));
    };
    const line = (l: string) => void ctx.emit(`site-provision://output/${j.id}`, l);
    line(`── preparing site (${j.domain}) — done`);
    const weights = [3, 7, 10, 5, 8, 25, 15, 20, 7];
    let pct = weights[0];
    for (const [i, [, label, lines, ms]] of PLAN.entries()) {
      const idx = i + 1;
      j.phases[idx].status = "running";
      state({ phaseCursor: idx });
      line(`── ${label}`);
      for (const l of lines) {
        await ctx.sleep(ms);
        line(l);
      }
      await ctx.sleep(ms || 300);
      j.phases[idx].status = PLAN[i][0] === "fetch" ? "skipped" : "ok";
      pct = Math.min(99, pct + weights[idx]);
      state({ pct });
    }
    site.provisioned = true;
    site.status = "running";
    await ctx.sleep(900);
    state({ status: "ok", pct: 100, summary: `created — serving at https://${j.domain}` });
  };

  return {
    repo_probe: async () => {
      await ctx.sleep(1100);
      return PROBE;
    },
    site_provision_active: () => (job && job.status === "running" ? job : null),
    site_provision_job: (a: Args) => {
      const input = a?.site as { name: string; domain: string; type: Site["type"]; phpVersion: string; webServer: Site["webServer"]; dbEngine: Site["dbEngine"] };
      const id = String(ctx.sites.length + 1);
      const site = ctx.site({
        id,
        name: input.name,
        domain: input.domain,
        type: input.type,
        phpVersion: input.phpVersion,
        webServer: input.webServer,
        dbEngine: input.dbEngine,
        dbName: "lv_storefront_rex",
        path: "/Users/demo/Sites/storefront",
        status: "stopped",
        provisioned: false,
        gitUrl: PROBE.url,
        gitRef: "main",
        createdAt: "2026-09-30 11:02:00",
      });
      ctx.sites.push(site);
      const phases: Array<[string, string]> = [["prepare", "preparing site (domain, certificate)"], ...PLAN.map(([k, l]) => [k, l] as [string, string])];
      job = {
        id: `prov-${id}`,
        domain: input.domain,
        siteId: id,
        phases: phases.map(([key, label], i) => ({ key, label, status: i === 0 ? "ok" : "pending" })),
        phaseCursor: 0,
        pct: 3,
        status: "running",
        summary: null,
        error: null,
        logKey: `provision-${id}`,
        downloadIds: [],
      };
      const j = job;
      setTimeout(() => void run(j, site), 350);
      return structuredClone(j);
    },
  };
}
