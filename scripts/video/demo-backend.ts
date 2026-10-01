/** The scripted backend the tutorial videos run against.
 *
 *  Scenes: a video can load `scenes/<name>.ts` (`?scene=<name>` on demo.html).
 *  Its default export receives the shared state (`ctx`) and returns handlers
 *  that answer before the base switch below — so each video carries its own
 *  fixtures and its own replayed jobs without growing this file.
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

export const site = (s: Partial<Site> & Pick<Site, "id" | "name" | "domain" | "type">): Site => ({
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

export const sites: Site[] = [
  site({ id: "1", name: "Agency Blog", domain: "agency-blog.rex", type: "wordpress", createdAt: "2026-08-21 09:30:00" }),
  site({ id: "2", name: "Shop Staging", domain: "shop-staging.rex", type: "wordpress", phpVersion: "8.2", createdAt: "2026-09-02 14:10:00" }),
  site({ id: "3", name: "Booking API", domain: "booking-api.rex", type: "laravel", phpVersion: "8.4", dbName: "lv_booking_api_rex", createdAt: "2026-09-10 11:05:00" }),
  site({ id: "4", name: "Landing Page", domain: "landing.rex", type: "php", dbName: "", createdAt: "2026-09-18 16:40:00" }),
];

// The whole stack up, the state a tutorial starts from.
export const services: ServiceInfo[] = mockServices.map((s) =>
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

/** `?speed=0.4` runs every replayed job (provision, import, downloads…) at
 *  40% of its recorded length — the fast-paced intro uses it. */
const SPEED = Number(new URLSearchParams(location.search).get("speed") ?? "1") || 1;
export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms * SPEED));

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

function base(cmd: string, args: Args): unknown {
  switch (cmd) {
    // The release this video depicts, not the dev mock's 0.1.0.
    case "app_info":
      return { ...mockAppInfo, version: "0.8.10", commit: "0a12913", builtAt: "2026-09-29" };
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
    case "services_status":
      return services;
    // Set up, healthy: the state every video after onboarding starts from.
    case "dns_status":
      return { running: true, mode: "agent", port: 15353, resolverInstalled: true, resolverPath: "/etc/resolver/rex", caTrusted: true };
    case "mailpit_status":
      return { running: true, smtpPort: 11025, httpPort: 18025, uiUrl: "http://127.0.0.1:18025" };
    // A site's own detail screen (any tab).
    case "wp_info": {
      const s = sites.find((x) => x.id === (args?.id as string));
      return { isWordpress: s?.type === "wordpress", version: s?.type === "wordpress" ? "7.1" : null, multisite: (s?.multisite ?? "none") !== "none" };
    }
    case "repo_site_info": {
      const s = sites.find((x) => x.id === (args?.siteId as string));
      return { present: false, projectRoot: s?.path ?? "", clonedFrom: null };
    }
    case "log_targets": {
      const s = sites.find((x) => x.id === (args?.siteId as string));
      const d = s?.domain ?? "site.rex";
      return [
        { key: `nginx-${d}-access.log`, label: "Access log", category: "server", path: `~/Library/Application Support/dev.rexenv.rexenv/logs/nginx-${d}-access.log` },
        { key: `nginx-${d}-error.log`, label: "Error log", category: "server", path: `~/Library/Application Support/dev.rexenv.rexenv/logs/nginx-${d}-error.log` },
      ];
    }
    case "tail_log":
      return [];
    case "agent_activity":
    case "list_terminals":
      return [];
    case "wp_default_creds":
      return false;
    // A site's Settings tab.
    case "site_cert_info": {
      const s = sites.find((x) => x.id === (args?.id as string));
      return {
        notBefore: "2026-08-21 09:30:00",
        notAfter: "2027-09-23 09:30:00",
        daysLeft: 358,
        sans: s ? [s.domain, `*.${s.domain}`] : [],
        certDir: `/Users/demo/Library/Application Support/dev.rexenv.rexenv/certs/${s?.domain ?? ""}`,
      };
    }
    case "site_domains":
      return [sites.find((x) => x.id === (args?.id as string))?.domain].filter(Boolean);
    case "list_site_env":
      return [];
    // Settings → DNS & SSL: every ending allowed, no resolver owned by another tool.
    case "tld_policy":
      return { allowed: true, warn: false, reason: "" };
    case "resolver_tld_status":
      return { tld: args?.tld, owner: "absent", path: `/etc/resolver/${args?.tld}`, theirContent: null, ourContent: "nameserver 127.0.0.1\nport 15353\n", rexenvSites: 0 };
    case "unresolvable_tlds":
      return [];
    case "sites_folder":
      return "/Users/demo/Sites";
    // `rex` is on PATH, pointing at this app's bundled copy.
    case "cli_status":
      return { available: true, installed: true, current: true, linkPath: "/usr/local/bin/rex", bundledPath: "/Applications/rexenv.app/Contents/MacOS/rex", onPath: null };
    case "wp_cli_packages":
      return null;
    // Settings: mail caught (the default), rexenv starting at login.
    case "mail_catch_all":
    case "autostart_status":
      return true;
    case "legacy_notice":
    case "setup_edge_conflict":
      return null;
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
    // The Databases screen's version pickers and the Adminer card.
    case "db_engine_versions":
      return { mysql: ["8.4.6", "8.0.44"], mariadb: ["12.3.2", "11.4.12"], postgres: ["18.6.0", "17.11.0", "16.15.0"], redis: ["8.8.0"] };
    case "adminer_status":
    case "adminer_update_check":
      return { staged: "6.1.1", effective: "6.1.1", pinned: "6.1.1", updatable: null };
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

export type Handlers = Record<string, (args: Args) => unknown>;
export type SceneCtx = {
  sites: Site[];
  services: ServiceInfo[];
  site: typeof site;
  sleep: typeof sleep;
  emit: typeof emit;
  /** The base answer for a command, for a scene that only adjusts it. */
  base: (cmd: string, args?: Args) => unknown;
};
type SceneModule = { default: (ctx: SceneCtx) => Handlers | Promise<Handlers> };

const sceneModules = import.meta.glob<SceneModule>("./scenes/*.ts");

/** The IPC handler for one video: the scene's handlers first, then the base. */
export async function createBackend(scene: string | null) {
  let handlers: Handlers = {};
  // `a,b`: several scenes in one app, so a montage moves between their screens
  // through the sidebar instead of reloading the app (a reload costs ~1.5 s of
  // empty stage). A later scene's handler wins a shared command.
  for (const name of scene ? scene.split(",") : []) {
    const load = sceneModules[`./scenes/${name}.ts`];
    if (!load) throw new Error(`no scene "${name}" in scripts/video/scenes/`);
    handlers = { ...handlers, ...(await (await load()).default({ sites, services, site, sleep, emit, base })) };
  }
  const calls: string[] = [];
  (window as unknown as { __demoCalls: string[] }).__demoCalls = calls;
  // Every answer is a fresh copy, as a real IPC reply (fresh JSON) is. Handing
  // back the same mutated array let React Query's structural sharing see "no
  // change": the sidebar kept saying 4 sites beside a list of 7.
  const fresh = (v: unknown) => (v === undefined ? null : structuredClone(v));
  return (cmd: string, args: Args) => {
    calls.push(cmd);
    const h = handlers[cmd];
    const out = h ? h(args) : base(cmd, args);
    return out instanceof Promise ? out.then(fresh) : fresh(out);
  };
}
