/** The scripted backend the tutorial videos run against.
 *
 *  Answers the IPC commands the recorded screens call, with demo data shaped
 *  like a real developer's machine (a few sites, the stack running). The New
 *  Site provision job is replayed with the SAME phase list, labels and log
 *  lines the Rust job emits for a WordPress site
 *  (`commands/site_provision.rs` `phase_defs` + `enter_phase`, wp-cli's own
 *  output) — a tutorial that shows steps the app never takes teaches the wrong
 *  thing. When the backend's phases change, change them here too.
 *
 *  Unknown commands are logged to `window.__demoUnknown` and answered with
 *  null, so a new screen in a script shows up as a list to fill in rather
 *  than a silent blank. */
import { emit } from "@tauri-apps/api/event";
import {
  mockAppInfo,
  mockAppUpdateState,
  mockDatabases,
  mockPhpVersions,
  mockPlatformWords,
  mockServices,
} from "@/lib/mock";
import type { ServiceInfo, Site, SiteProvisionState } from "@/types";

type Args = Record<string, unknown> | undefined;

const site = (s: Partial<Site> & Pick<Site, "id" | "name" | "domain" | "type">): Site => ({
  status: "running",
  phpVersion: "8.3",
  webServer: "nginx",
  ssl: true,
  path: `~/Sites/${s.domain.replace(/\.rex$/, "")}`,
  createdAt: "2026-09-12 10:00:00",
  multisite: "none",
  dbName: `wp_${s.domain.replace(/\W/g, "_")}`,
  dbEngine: "mysql",
  xdebug: false,
  provisioned: true,
  docrootManaged: true,
  ...s,
});

const sites: Site[] = [
  site({ id: "1", name: "Agency Blog", domain: "agency-blog.rex", type: "wordpress", createdAt: "2026-08-21 09:30:00" }),
  site({ id: "2", name: "Shop Staging", domain: "shop-staging.rex", type: "wordpress", phpVersion: "8.2", createdAt: "2026-09-02 14:10:00" }),
  site({ id: "3", name: "Booking API", domain: "booking-api.rex", type: "laravel", phpVersion: "8.4", dbName: "lv_booking_api_rex", createdAt: "2026-09-10 11:05:00" }),
  site({ id: "4", name: "Landing Page", domain: "landing.rex", type: "php", dbName: "", createdAt: "2026-09-18 16:40:00" }),
];

// The whole stack up, the state a tutorial starts from.
const services: ServiceInfo[] = mockServices.map((s) =>
  s.running ? s : { ...s, running: true, pid: 1300 + s.port % 97, ramMb: s.kind === "database" ? 96 : 18 },
);

const globalStatus = () => ({
  summary: "all",
  running: services.length,
  total: services.length,
  cpuPercent: 3,
  cpuCores: 10,
  ramMb: 742,
  ramTotalMb: 16384,
});

let provision: SiteProvisionState | null = null;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Replays one WordPress provision job: phase boundaries, wp-cli's lines, the
 *  bar's pct, then settle-ok — the order `run_provision_job` emits them in. */
async function runProvision(job: SiteProvisionState, newSite: Site) {
  const state = (patch: Partial<SiteProvisionState>) => {
    Object.assign(job, patch);
    void emit(`site-provision://state/${job.id}`, structuredClone(job));
  };
  const line = (l: string) => void emit(`site-provision://output/${job.id}`, l);

  // `lines` are what the log shows inside the phase: rexenv's own notes
  // (finish_phase's `note`) and wp-cli's output, verbatim. The core zip is
  // `wordpress::core_zip_url` for en_US, fetched as `wp core download <url>`.
  const steps: Array<{ lines: string[]; end: string; pct: number; ms: number }> = [
    // prepare runs inline before the job starts (it is already ok on arrival).
    { lines: [], end: "ok", pct: 4, ms: 0 },
    // fetch on a machine that already serves sites: nothing to download.
    { lines: ["binaries already cached"], end: "skipped", pct: 12, ms: 500 },
    // db: MySQL is already up; the phase logs nothing of its own.
    { lines: [], end: "ok", pct: 22, ms: 700 },
    {
      lines: ["Downloading from https://wordpress.org/latest.zip ...", "Success: WordPress downloaded."],
      end: "ok",
      pct: 48,
      ms: 1700,
    },
    // `wp config create`, then rexenv creates the database itself (no line).
    { lines: ["Success: Generated 'wp-config.php' file."], end: "ok", pct: 66, ms: 1100 },
    { lines: ["Success: WordPress installed successfully."], end: "ok", pct: 88, ms: 1600 },
    { lines: [], end: "ok", pct: 99, ms: 800 },
  ];

  for (let i = 1; i < job.phases.length; i++) {
    const step = steps[i];
    job.phases[i].status = "running";
    state({ phaseCursor: i });
    line(`── ${job.phases[i].label}`);
    const gap = step.lines.length ? step.ms / (step.lines.length + 1) : step.ms;
    for (const l of step.lines) {
      await sleep(gap);
      line(l);
    }
    await sleep(gap);
    job.phases[i].status = step.end;
    state({ pct: step.pct });
  }
  newSite.provisioned = true;
  newSite.status = "running";
  state({ status: "ok", pct: 100, summary: `created — serving at https://${job.domain}` });
}

export function handle(cmd: string, args: Args): unknown {
  switch (cmd) {
    case "app_info":
      return mockAppInfo;
    case "platform_words":
      return mockPlatformWords;
    case "global_status":
      return globalStatus();
    case "list_services":
      return services;
    case "list_sites":
      return sites;
    case "sites_serving":
      return sites.map((s) => ({ domain: s.domain, serving: s.status === "running" && s.provisioned }));
    case "all_site_domains":
      return {};
    case "resolver_drift":
      return [];
    case "site_provision_active":
      return provision && provision.status === "running" ? provision : null;
    case "list_php_versions":
    case "php_update_check":
      return mockPhpVersions;
    case "app_update_state":
      return mockAppUpdateState;
    case "db_engine_refusals":
      return {};
    case "offered_web_servers":
      return ["nginx", "frankenphp", "apache"];
    case "offered_db_engines":
      return ["mysql", "mariadb", "postgres"];
    case "default_tld":
      return "rex";
    case "list_blueprints":
      return [];
    // The quiet rest of the shell: nothing to report, nothing running beside the sites.
    case "init_error":
      return null;
    case "get_setting":
      return null;
    // No app icons in the fixture: the tiles fall back to their own glyphs.
    case "list_browsers":
    case "list_editors":
    case "startup_notices":
    case "repo_watches":
    case "db_import_records":
    case "tunnels_status":
      return [];
    case "downloads_state":
      return { batch: null, items: [], seq: 0 };
    case "databases_status":
      return mockDatabases.map((d) => ({ ...d, running: true, pid: d.pid ?? 1402, ramMb: d.ramMb || 96 }));
    case "mailpit_messages":
      return { total: 0, unread: 0, messages: [] };
    case "scan_valet_import":
      return { sources: [], candidates: [], tlds: [], availablePhp: [], takenDomains: [], unsupported: null };
    case "sites_resources":
      return sites.map((s, i) => ({
        id: s.id,
        domain: s.domain,
        dedicated: false,
        cpuPercent: s.provisioned ? [0.4, 0.2, 0.6, 0.3][i % 4] : null,
        ramMb: s.provisioned ? [42, 38, 55, 40][i % 4] : null,
        requestsPerMin: s.provisioned ? [12, 3, 27, 1][i % 4] : null,
        bytesPerMin: null,
        dbSizeBytes: s.provisioned ? [18_400_000, 9_700_000, 4_200_000, 2_600_000][i % 4] : null,
      }));
    case "site_provision_job": {
      const input = (args?.site ?? {}) as { name: string; domain: string; type: Site["type"]; phpVersion: string; webServer: Site["webServer"]; dbEngine: Site["dbEngine"] };
      const id = String(sites.length + 1);
      const newSite = site({
        id,
        name: input.name,
        domain: input.domain,
        type: input.type,
        phpVersion: input.phpVersion,
        webServer: input.webServer,
        dbEngine: input.dbEngine,
        status: "stopped",
        provisioned: false,
        createdAt: "2026-09-30 10:42:00",
      });
      sites.push(newSite);
      // Mirrors phase_defs() for a new-folder WordPress site with no blueprint.
      const phases: Array<[string, string]> = [
        ["prepare", "preparing site (domain, certificate)"],
        ["fetch", "downloading binaries"],
        ["db", "starting database"],
        ["core_download", "downloading WordPress core"],
        ["configure", "writing wp-config + creating database"],
        ["core_install", "installing WordPress"],
        ["serve", "starting to serve"],
      ];
      provision = {
        id: `prov-${id}`,
        domain: input.domain,
        siteId: id,
        phases: phases.map(([key, label], i) => ({ key, label, status: i === 0 ? "ok" : "pending" })),
        phaseCursor: 0,
        pct: 4,
        status: "running",
        summary: null,
        error: null,
        logKey: `provision-${id}`,
        downloadIds: [],
      };
      const job = provision;
      // Emitted after the reply lands, so the card is listening first.
      setTimeout(() => {
        void emit(`site-provision://output/${job.id}`, `── preparing site (${job.domain}) — done`);
        void runProvision(job, newSite);
      }, 350);
      return structuredClone(job);
    }
    default: {
      const w = window as unknown as { __demoUnknown?: Record<string, number> };
      w.__demoUnknown ??= {};
      w.__demoUnknown[cmd] = (w.__demoUnknown[cmd] ?? 0) + 1;
      console.warn(`[demo-backend] unanswered: ${cmd}`, args);
      return null;
    }
  }
}
