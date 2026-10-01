/** Import: a Mac with Laravel Valet (one Laravel app on .test, Valet owning
 *  /etc/resolver/test) and Local (two WordPress sites on .local, re-homed to
 *  .rex). The batch replays `valet_import_run`'s own tick sequence
 *  (`commands/valet_import.rs`): scanning → resolvers (one per target TLD,
 *  sorted) → php (one per minor) → per site: the linked provision's phases,
 *  then the database copy's phases (`db_import.rs`), then Local's connect →
 *  checking → done. Details are the backend's own labels. */
import type { SceneCtx } from "../demo-backend";
import type { ImportCandidate, ImportOutcome, ImportProgress, ImportRequest, ImportScan } from "@/types";

const HOME = "/Users/demo";
const LOCAL_NOTE =
  "rexenv can't serve .local — macOS uses it for Bonjour, so taking it over breaks printers and AirDrop. " +
  "2 sites will be served on .rex instead, and each copied database has its URLs updated to match " +
  "(rexenv's copy only — Local's database is never written).";

const base = {
  phpChoice: false,
  domainChoice: false,
  extraDomains: [],
  proxyTo: null,
  alsoIn: null,
  multisite: "none" as const,
  subsites: [],
  hasCustomValetDriver: false,
};

const CANDIDATES: ImportCandidate[] = [
  {
    ...base,
    source: "local",
    name: "bakery",
    domain: "bakery.rex",
    path: `${HOME}/Local Sites/bakery/app/public`,
    servePath: `${HOME}/Local Sites/bakery/app/public`,
    docrootRel: null,
    siteType: "wordpress",
    label: "WordPress",
    phpMinor: "8.2",
    phpTarget: "8.2",
    secured: false,
    renamedFrom: "bakery.local",
    status: { status: "importable" },
  },
  {
    ...base,
    source: "valet",
    name: "crm",
    domain: "crm.test",
    path: `${HOME}/Sites/crm`,
    servePath: `${HOME}/Sites/crm/public`,
    docrootRel: "public",
    siteType: "laravel",
    label: "Laravel",
    phpMinor: "8.3",
    phpTarget: "8.3",
    secured: true,
    renamedFrom: null,
    status: { status: "importable" },
  },
  {
    ...base,
    source: "local",
    name: "portfolio",
    domain: "portfolio.rex",
    path: `${HOME}/Local Sites/portfolio/app/public`,
    servePath: `${HOME}/Local Sites/portfolio/app/public`,
    docrootRel: null,
    siteType: "wordpress",
    label: "WordPress",
    phpMinor: "8.1",
    phpTarget: "8.1",
    secured: false,
    renamedFrom: "portfolio.local",
    status: { status: "importable" },
  },
];

export default function importScene(ctx: SceneCtx) {
  // `?tookover=1`: .test already handed to rexenv (a fast montage skips the consent step).
  let testOwner: "foreign" | "borrowed" = new URLSearchParams(location.search).get("tookover") ? "borrowed" : "foreign";
  const imported = new Set<string>();

  const scan = (): ImportScan => ({
    sources: [
      { kind: "valet", home: `${HOME}/.config/valet`, tld: "test", loopback: "127.0.0.1", parked: [`${HOME}/Sites`], notes: [] },
      { kind: "local", home: `${HOME}/Library/Application Support/Local`, tld: "local", loopback: "127.0.0.1", parked: [], notes: [LOCAL_NOTE] },
    ],
    candidates: CANDIDATES.map((c) => (imported.has(c.domain) ? { ...c, status: { status: "alreadyImported" } } : c)),
    tlds: [
      { tld: "rex", owner: "ours", path: "/etc/resolver/rex", theirContent: null, ourContent: "nameserver 127.0.0.1\nport 15353\n", rexenvSites: ctx.sites.length },
      {
        tld: "test",
        owner: testOwner,
        path: "/etc/resolver/test",
        theirContent: "nameserver 127.0.0.1\n",
        ourContent: "nameserver 127.0.0.1\nport 15353\n",
        rexenvSites: 0,
      },
    ],
    availablePhp: ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5"],
    takenDomains: ctx.sites.map((s) => s.domain),
    unsupported: null,
  });

  const run = async (req: ImportRequest) => {
    const domains = [...req.domains].sort();
    const total = domains.length;
    let done = 0;
    let lastPct = 0;
    const tick = (stage: ImportProgress["stage"], index: number, domain: string | null, detail: string | null, sitePct: number) => {
      const pct = stage === "done" ? 100 : Math.max(lastPct, Math.min(99, Math.floor((done * 100 + sitePct) / total)));
      lastPct = pct;
      void ctx.emit("valet-import://progress", { total, done, index, domain, stage, detail, sitePct, pct } satisfies ImportProgress);
    };
    const pick = CANDIDATES.filter((c) => domains.includes(c.domain));

    tick("scanning", 0, null, "re-reading your Valet, Herd and Local setup", 0);
    await ctx.sleep(900);
    for (const tld of [...new Set(pick.map((c) => c.domain.split(".").pop()!))].sort()) {
      tick("resolvers", 0, null, `making .${tld} resolve to rexenv`, 0);
      await ctx.sleep(600);
    }
    for (const m of [...new Set(pick.map((c) => c.phpTarget!))].sort()) {
      tick("php", 0, null, `getting PHP ${m} ready`, 0);
      await ctx.sleep(500);
    }
    const outcomes: ImportOutcome[] = [];
    let dbImported = 0;
    let connected = 0;
    for (const [n, c] of pick.entries()) {
      const i = n + 1;
      const withDb = req.importDatabases !== false;
      const scale = withDb ? 0.6 : 1;
      tick("site", i, c.domain, "starting", 0);
      await ctx.sleep(400);
      tick("site", i, c.domain, "downloading binaries", Math.round(30 * scale));
      await ctx.sleep(500);
      tick("site", i, c.domain, "starting to serve", Math.round(75 * scale));
      await ctx.sleep(600);
      const id = String(ctx.sites.length + 1);
      ctx.sites.push(
        ctx.site({
          id,
          name: c.name,
          domain: c.domain,
          type: c.siteType!,
          phpVersion: c.phpTarget!,
          path: c.servePath!,
          docrootManaged: false,
          dbName: c.siteType === "wordpress" ? `local_${c.name}` : `valet_${c.name}`,
          createdAt: "2026-09-30 10:50:00",
        }),
      );
      const row: ImportOutcome = { domain: c.domain, status: "imported", reason: null, siteId: id, logKey: `import-${c.domain}.log`, db: null, servedAs: null, connect: null };
      if (withDb) {
        tick("database", i, c.domain, "copying the database", 60);
        await ctx.sleep(300);
        const phases: Array<[string, number]> = [
          ["checking the source database", 0],
          ["copying the database out", 10],
          ["starting rexenv's database", 50],
          ["restoring the copy", 60],
          ...(c.source === "local" ? ([["updating the copy's URLs", 90]] as Array<[string, number]>) : []),
          ["finishing up", 95],
        ];
        for (const [label, dbPct] of phases) {
          tick("database", i, c.domain, label, 60 + Math.round((dbPct * 40) / 100));
          await ctx.sleep(label === "copying the database out" || label === "restoring the copy" ? 700 : 350);
        }
        row.db = "imported";
        dbImported += 1;
        if (c.source === "local" && req.connectLocal) {
          tick("connecting", i, c.domain, "connecting the site to its copy", 99);
          await ctx.sleep(600);
          row.connect = "connected";
          connected += 1;
        }
      }
      imported.add(c.domain);
      void ctx.emit("valet-import://row", row);
      outcomes.push(row);
      done += 1;
      await ctx.sleep(200);
    }
    tick("checking", 0, null, "checking your sites will load", 0);
    await ctx.sleep(800);
    tick("done", 0, null, null, 0);
    // The page drops its progress card the moment this resolves.
    await ctx.sleep(700);
    return {
      outcomes,
      imported: outcomes.length,
      failed: 0,
      skipped: 0,
      dbImported,
      dbFailed: 0,
      connected,
      connectFailed: 0,
      serving: null,
    };
  };

  return {
    scan_valet_import: () => scan(),
    db_import_leftovers: () => [],
    resolver_take_over: async () => {
      await ctx.sleep(1500);
      testOwner = "borrowed";
      return null;
    },
    valet_import_run: (args: Record<string, unknown> | undefined) => run(args?.request as ImportRequest),
    valet_import_cancel: () => null,
  };
}
