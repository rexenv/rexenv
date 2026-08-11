/** DEV-ONLY WebKit render harness for the UI review (`#/dev/ui-review`).
 *  Mounted only when `import.meta.env.DEV` (see App.tsx) — never part of a
 *  production bundle. Mocks the Tauri IPC layer (`mockIPC`) with canned
 *  responses so the Stage 2/3 surfaces — including the ones that have NEVER
 *  rendered anywhere (the connected-delete dialog variants, verifyFailed,
 *  the borrowed-resolver row) — can be screenshotted in Playwright WebKit
 *  with zero backend and zero contact with the real app.
 *
 *  Views (`?view=…`):
 *    card     — DbImportCard.   `rec=imported|connected|connectedHttp`,
 *               `preview=ready|refused|noop`, `root=1`, `cache=1`,
 *               `backup=1`, `engine=mariadb`,
 *               `apply=fileChanged|engineStopped|verifyFailed|applied`,
 *               `revert=refusedEdited|backupMissing|reverted`
 *    delete   — DeleteSiteDialog. `kind=connected|preexisting|wp|imported|linked`
 *    badges   — a column of SiteRow variants (badge crowding at narrow widths)
 *    resolver — the borrowed-resolver hand-back row (Settings card context)
 *    toast    — the long revert-then-delete refusal toast + friends
 *    agents   — the AI-agents (MCP) card. `astate=off|idle|working|erroring`
 *               drives the status line, `feed=empty` empties the activity list,
 *               `site=1` also shows the per-site SiteDetail section
 *    scratch  — the Agent-scratch rows in every state, INCLUDING the two that
 *               must not look agent-flavoured (a Kept site, and a user's own
 *               site hand-named `*.scratch.*`)
 *    keep     — the Keep confirm dialog
 *    openin   — the "which app opens this" surfaces: header split button +
 *               the Browser/editor Quick-links tiles. `browsers=one|none`
 *               (the no-chevron and nothing-detected states), `icons=none`
 *               (the honest degrade to a monochrome glyph)
 */
import { useEffect, useState } from "react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { DatabaseTab } from "@/components/sites/DatabaseTab";
import { DbImportCard } from "@/components/sites/DbImportCard";
import { DeleteSiteDialog } from "@/components/sites/DeleteSiteDialog";
import { KeepSiteDialog, ScratchGroupHeading, SiteRow } from "@/routes/Sites";
import { ResolverHandBackRow } from "@/routes/Import";
import { SiteProvisionCard } from "@/components/sites/SiteProvisionCard";
import { AgentsMcpCard } from "@/components/mcp/AgentsMcpCard";
import { QuickTile } from "@/routes/SiteDetail";
import { AppIcon } from "@/components/ui/app-icon";
import { SplitButton } from "@/components/ui/split-button";
import { useBrowserMenu, useEditorMenu } from "@/components/ui/open-in";
import { usePreferredBrowser } from "@/lib/useBrowser";
import { usePreferredEditor } from "@/lib/useEditor";
import { Code, ExternalLink, Globe } from "lucide-react";
import { SiteAgentActivity } from "@/components/mcp/SiteAgentActivity";
import { toast } from "@/lib/toast";
import type { ActivityStatus, AgentAction, BrowserApp, EditorApp, DbImportRecord, McpStatus, ResolverTldStatus, RewriteApplied, RewritePreview, RewriteRevertOutcome, ScratchPackage, Site, SiteProvisionState } from "@/types";

const params = new URLSearchParams(window.location.search);

function fixtureSite(over: Partial<Site> = {}): Site {
  return {
    id: "s-ea",
    name: "ea",
    domain: "myblog.test",
    type: "wordpress",
    status: "running",
    phpVersion: "8.3",
    webServer: "nginx",
    ssl: true,
    path: "/Users/dev/code/myblog",
    createdAt: "2026-07-26 00:00:00",
    multisite: "none",
    dbName: "ea",
    dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
    docrootManaged: false,
    dbCreated: true,
    ...over,
  };
}

const RECORD_BASE = {
  siteId: "s-ea",
  dbName: "ea",
  tableCount: 48,
  sizeBytes: 24 * 1024 * 1024,
  sourceLabel: "MySQL 8.0.27 at 127.0.0.1:3306",
  mirroredUser: null as string | null,
  skippedTables: [] as string[],
  importedAt: "2026-07-27 10:00:00",
};

function record(): DbImportRecord | null {
  switch (params.get("rec")) {
    case "imported":
      return { ...RECORD_BASE, state: "imported" };
    // A copy that is deliberately INCOMPLETE — the source could not read these
    // tables, so they are not here. Names and counts are the real ones from the
    // 8 Aug 2026 database that produced this state, not friendly placeholders:
    // long plugin table names are what the list has to lay out.
    case "importedPartial":
      return {
        ...RECORD_BASE,
        tableCount: 173,
        state: "imported",
        skippedTables: [
          "wp_betterdocs_analytics",
          "wp_betterdocs_search_keyword",
          "wp_betterlinks_clicks",
          "wp_woocommerce_sessions",
          "wp_wsal_occurrences",
        ],
      };
    case "connected":
      return { ...RECORD_BASE, mirroredUser: "rex_myblog_test", state: "connected", verified: "signin" };
    case "connectedHttp":
      return { ...RECORD_BASE, mirroredUser: "rex_myblog_test", state: "connected", verified: "signin+http" };
    default:
      return null;
  }
}

const WP_DIFF = [
  { sign: "-", line: 4, text: "define( 'DB_HOST', '127.0.0.1' );" },
  { sign: "+", line: 4, text: "define( 'DB_HOST', '127.0.0.1:13306' );" },
  { sign: "-", line: 3, text: "define( 'DB_USER', 'root' );" },
  { sign: "+", line: 3, text: "define( 'DB_USER', 'rex_myblog_test' );" },
];

function preview(): RewritePreview {
  if (params.get("preview") === "refused") {
    return {
      status: "refused",
      reason:
        "DB_HOST is set more than once (lines 40 and 61), so rexenv can't tell which one this site actually uses.",
      file: "/Users/dev/code/myblog/wp-config.php",
    };
  }
  return {
    status: "ready",
    file: "/Users/dev/code/myblog/wp-config.php",
    diff: params.get("preview") === "noop" ? [] : WP_DIFF,
    fingerprint: "f".repeat(64),
    createsUser: params.get("root") === "1" ? "rex_myblog_test" : null,
    backupExists: params.get("backup") === "1",
    laravelCacheWarning: params.get("cache") === "1",
    target: "127.0.0.1:13306",
  };
}

function applied(): RewriteApplied {
  switch (params.get("apply")) {
    case "fileChanged":
      return {
        status: "fileChanged",
        message:
          "The file changed since the diff was shown — nothing was written. The refreshed preview shows the change against the file as it is now.",
      };
    case "engineStopped":
      return {
        status: "engineStopped",
        message:
          "this change points myblog.test at rexenv's own MySQL (127.0.0.1:13306), which isn't running — start it from the Databases page, then apply again.",
      };
    case "verifyFailed":
      return {
        status: "verifyFailed",
        reason:
          "the server refused the file's own credentials (Access denied for user 'rex_myblog_test'@'localhost') — the site isn't marked connected until a sign-in succeeds.",
        message:
          "the change was applied and backed up, but the sign-in check didn't pass — myblog.test is not marked connected. You can revert the change from this card.",
      };
    default:
      return {
        status: "applied",
        record: { ...RECORD_BASE, mirroredUser: "rex_myblog_test", state: "connected", verified: "signin+http" },
        message: "verified: the rewritten settings sign in to the rexenv copy of `ea`.",
      };
  }
}

function reverted(): RewriteRevertOutcome {
  switch (params.get("revert")) {
    case "refusedEdited":
      return {
        status: "refusedEdited",
        file: "/Users/dev/code/myblog/wp-config.php",
        reason: "editedSinceRewrite",
        message:
          "This file was edited after the rewrite — restoring the backup would replace those edits. Choose \"restore anyway\" to proceed.",
      };
    case "backupMissing":
      return {
        status: "backupMissing",
        file: "/Users/dev/code/myblog/wp-config.php",
        message:
          "rexenv's copy of the original is gone; your file was left exactly as it is. To go back to the old database, edit the file yourself.",
      };
    default:
      return {
        status: "reverted",
        file: "/Users/dev/code/myblog/wp-config.php",
        message:
          "/Users/dev/code/myblog/wp-config.php was restored to the original, byte for byte; the site is back on its previous connection settings.",
      };
  }
}

const BORROWED: ResolverTldStatus = {
  tld: "test",
  owner: "borrowed",
  path: "/etc/resolver/test",
  theirContent: "nameserver 127.0.0.1\n",
  ourContent: "nameserver 127.0.0.1\nport 15353\n",
  rexenvSites: 7,
};

function DeleteView() {
  const kind = params.get("kind") ?? "connected";
  // `long=1`: the worst realistic case — the longest domain on the real
  // machine's Valet tree plus a collision-suffixed db name and a deep path.
  const long = params.get("long") === "1";
  const longOver: Partial<Site> = long
    ? {
        name: "acme-reviews-staging",
        domain: "acme-reviews-staging.test",
        dbName: "wp_acme_reviews_staging_9a1b2c3d",
        path: "/Users/dev/Projects/clients/acme/acme-reviews-staging",
      }
    : {};
  const site =
    kind === "preexisting"
      ? fixtureSite({ dbCreated: false })
      : kind === "wp"
        ? fixtureSite({ docrootManaged: true, dbCreated: null })
        : kind === "imported"
          ? fixtureSite({ type: "laravel", dbCreated: true, dbName: "lms" })
          : kind === "linked"
            ? fixtureSite({ dbCreated: null })
            : fixtureSite(longOver);
  const dbState = kind === "connected" || kind === "preexisting" ? ("connected" as const) : undefined;
  return (
    <DeleteSiteDialog
      site={site}
      dbState={dbState}
      onPlainDelete={() => {}}
      onRevertThenDelete={() => {}}
      onCancel={() => {}}
    />
  );
}

/** Replica of SiteDetail's Database-tab REGION CHAIN (the two wrappers around
 *  the tab content — keep the classes in sync with SiteDetail.tsx). The
 *  content inside is the REAL DatabaseTab; the border marks the region bounds
 *  so clipping is visible in screenshots. `shape=plain|imported` +
 *  the usual rec/preview params drive the card's states. */
function DbTabView() {
  const shape = params.get("shape") ?? "plain";
  const site =
    shape === "plain"
      ? fixtureSite({ docrootManaged: true, dbCreated: null })
      : fixtureSite();
  return (
    <div className="flex h-[80vh] flex-col border border-rex-border">
      <div className="min-h-0 flex-1 overflow-auto px-[22px] pb-[22px] pt-[18px]">
        <div className="flex min-h-full flex-col gap-[14px]">
          <DatabaseTab site={site} />
        </div>
      </div>
    </div>
  );
}

/** `rows=N`: a realistic-scale Sites list (his machine holds ~30 rows) with a
 *  clickable row menu — the clipped-menu bug needs the LAST row of a long
 *  list. Mix of badge shapes so the list looks like the real one. */
function SitesScaleView() {
  const n = Math.max(1, Math.min(60, Number(params.get("rows") ?? 28)));
  const noop = () => {};
  const rows = Array.from({ length: n }, (_, i) => {
    const dbState =
      i % 5 === 3 ? ("imported" as const) : i % 7 === 4 ? ("connected" as const) : undefined;
    return {
      site: fixtureSite({
        id: `s${i}`,
        name: i % 6 === 2 ? `acme-reviews-staging-${i}` : `site-${i}`,
        domain: i % 6 === 2 ? `acme-reviews-staging-${i}.test` : `site-${i}.test`,
        docrootManaged: i % 2 === 0 ? true : false,
      }),
      dbState,
    };
  });
  return (
    <div className="space-y-0.5">
      {rows.map((r) => (
        <SiteRow
          key={r.site.id}
          site={r.site}
          status={r.site.status}
          dbState={r.dbState}
          onOpen={noop}
          onDelete={noop}
          onOpenDatabase={noop}
          onOpenWordpress={noop}
          onRename={noop}
          onDuplicate={noop}
          onRetry={noop}
        />
      ))}
    </div>
  );
}

/** SQLite-shaped UTC stamp `h` hours from now (negative = in the past). */
function hoursFromNow(h: number): string {
  return new Date(Date.now() + h * 3600_000).toISOString().slice(0, 19).replace("T", " ");
}

/**
 * The Agent-scratch group, every state it has — and, deliberately, the two
 * states that must NOT look agent-flavoured at all.
 *
 * Rows 5 and 6 are the point of this fixture as much as rows 1–4: a KEPT site
 * and a site the user hand-named `*.scratch.*` are both `origin: "user"`, so
 * they must render as ordinary sites with no badge, no TTL and no Keep item. If
 * either ever picks up an agent badge, the shot shows it.
 */
function ScratchView() {
  const noop = () => {};
  const rows: Array<{
    site: Site;
    packages?: ScratchPackage[];
    reapFailure?: string;
    scratch: boolean;
  }> = [
    {
      site: fixtureSite({
        id: "s-scratch-1",
        name: "plugin-test",
        domain: "plugin-test.scratch.rex",
        path: "/Users/dev/Library/Application Support/rexenv/Sites/plugin-test.scratch.rex",
        origin: "agent",
        agentClient: "Claude Code",
        expiresAt: hoursFromNow(22),
      }),
      packages: [
        {
          siteId: "s-scratch-1",
          slug: "acme-blocks",
          kind: "plugin",
          sourcePath: "/Users/dev/code/acme-blocks",
          syncedAt: hoursFromNow(-0.15),
          sourceMissing: false,
        },
        {
          siteId: "s-scratch-1",
          slug: "acme-theme",
          kind: "theme",
          sourcePath: "/Users/dev/code/acme-theme",
          syncedAt: hoursFromNow(-3),
          sourceMissing: false,
        },
      ],
      scratch: true,
    },
    {
      site: fixtureSite({
        id: "s-scratch-2",
        name: "compat-81",
        domain: "compat-81.scratch.rex",
        origin: "agent",
        agentClient: "Cursor 0.42",
        expiresAt: hoursFromNow(0.6),
        status: "stopped",
      }),
      packages: [
        {
          siteId: "s-scratch-2",
          slug: "acme-blocks",
          kind: "plugin",
          sourcePath: "/Users/dev/code/acme-blocks-old",
          syncedAt: hoursFromNow(-30),
          sourceMissing: true,
        },
      ],
      scratch: true,
    },
    {
      site: fixtureSite({
        id: "s-scratch-3",
        name: "old",
        domain: "old.scratch.rex",
        origin: "agent",
        agentClient: "Claude Code",
        expiresAt: hoursFromNow(-5),
        status: "stopped",
      }),
      scratch: true,
    },
    {
      site: fixtureSite({
        id: "s-scratch-4",
        name: "stuck",
        domain: "stuck.scratch.rex",
        origin: "agent",
        agentClient: "Claude Code",
        expiresAt: hoursFromNow(-40),
        status: "stopped",
      }),
      reapFailure: "its database could not be dropped",
      scratch: true,
    },
    // KEPT: origin flipped to the user's, expiry cleared. Must be an ORDINARY
    // row — no badge, no TTL, no Keep — because that is what the dialog just
    // promised ("it becomes one of your own sites").
    {
      site: fixtureSite({
        id: "s-kept",
        name: "kept",
        domain: "kept.scratch.rex",
        origin: "user",
        agentClient: "Claude Code",
        expiresAt: null,
      }),
      scratch: false,
    },
    // The user's OWN site, hand-named to look like a scratch one (#204).
    {
      site: fixtureSite({
        id: "s-handnamed",
        name: "mine",
        domain: "mine.scratch.rex",
        origin: "user",
        expiresAt: null,
      }),
      scratch: false,
    },
  ];
  const own = rows.filter((r) => !r.scratch);
  const scratch = rows.filter((r) => r.scratch);
  const row = (r: (typeof rows)[number]) => (
        <SiteRow
          key={r.site.id}
          site={r.site}
          status={r.site.status}
          packages={r.packages}
          reapFailure={r.reapFailure}
          onOpen={noop}
          onDelete={noop}
          onOpenDatabase={noop}
          onOpenWordpress={noop}
          onRename={noop}
          onDuplicate={noop}
          onRetry={noop}
          onKeep={r.scratch ? noop : undefined}
        />
  );
  // Same order as the page: the user's own sites, then the group.
  return (
    <div className="space-y-0.5">
      {own.map(row)}
      <ScratchGroupHeading count={scratch.length} />
      {scratch.map(row)}
    </div>
  );
}

/** The provision card at the width it actually renders in (the New Site
 *  dialog body), with the LONGEST real phase label and a long domain at once.
 *  This row broke in the real app: the phase label was `flex-none`, so a long
 *  backend label pushed the domain out of the row entirely. The card must
 *  truncate, never overflow — nothing here may reach past the dialog edge. */
function ProvisionCardView() {
  const job = (over: Partial<SiteProvisionState>): SiteProvisionState => ({
    id: "9f1c7e40-2b6a-4d18-9d3c-58a71f0e4b22",
    domain: "acme-reviews-staging.rex",
    siteId: "26ed7ab8-d2c4-450b-bf4a-64531f64fe7e",
    phases: [
      { key: "prepare", label: "preparing site (domain, certificate)", status: "ok" },
      { key: "fetch", label: "downloading binaries", status: "ok" },
      { key: "db", label: "starting database", status: "ok" },
      { key: "app_install", label: "installing Laravel", status: "running" },
      { key: "configure", label: "creating database + .env", status: "pending" },
      { key: "serve", label: "starting to serve", status: "pending" },
    ],
    phaseCursor: 3,
    pct: 62,
    status: "running",
    summary: null,
    error: null,
    logKey: "site-provision-9f1c7e40.log",
    downloadIds: [],
    ...over,
  });
  const lines = ["$ composer create-project laravel/laravel .", "Generating optimized autoload files"];
  return (
    // The dialog body's own width — a card that fits at 860px but not here is
    // exactly the bug this view exists to catch.
    <div className="w-[420px] rounded-xl border border-rex-border bg-rex-surface-1 p-4">
      <div className="space-y-3">
        <SiteProvisionCard job={job({})} lines={lines} onCancel={() => {}} />
        <SiteProvisionCard
          job={job({ status: "failed", error: "composer create-project failed: exit 1", phaseCursor: 3 })}
          lines={lines}
          onCancel={() => {}}
        />
      </div>
    </div>
  );
}

function BadgesView() {
  const noop = () => {};
  const rows: Array<{ site: Site; dbState?: DbImportRecord["state"] }> = [
    { site: fixtureSite({ name: "plain", domain: "plain.rex", docrootManaged: true }) },
    { site: fixtureSite({ name: "external", domain: "linked.test" }) },
    { site: fixtureSite({ name: "imported-db", domain: "lms.test" }), dbState: "imported" },
    { site: fixtureSite({ name: "connected-db", domain: "myblog.test" }), dbState: "connected" },
    {
      site: fixtureSite({ name: "half", domain: "half.test", provisioned: false }),
      dbState: "imported",
    },
    {
      // The worst realistic row: longest real domain + external + a DB badge
      // + running, all at once — the name must truncate, nothing may wrap.
      site: fixtureSite({
        name: "acme-reviews-staging",
        domain: "acme-reviews-staging.test",
      }),
      dbState: "imported",
    },
  ];
  return (
    <div className="space-y-2">
      {rows.map((r) => (
        <SiteRow
          key={r.site.domain}
          site={r.site}
          status={r.site.status}
          dbState={r.dbState}
          onOpen={noop}
          onDelete={noop}
          onOpenDatabase={noop}
          onOpenWordpress={noop}
          onRename={noop}
          onDuplicate={noop}
          onRetry={noop}
        />
      ))}
    </div>
  );
}

/** Every StatusPill state + every StartStopToggle state, in one place.
 *  These render NOWHERE else outside the live app (fixtures elsewhere are all
 *  `running`), and the pill width is the WKWebView metrics fix (`min-w-[92px]`
 *  after "Running" wrapped into two overlapping words) — measured by
 *  uireview.js instead of the one-off console session that verified it. */
function PillsView() {
  const noop = () => {};
  return (
    <div className="flex flex-col gap-4">
      <div data-probe="pills" className="flex flex-wrap items-center gap-3">
        {(["running", "stopped", "starting", "error"] as const).map((s) => (
          <StatusPill key={s} status={s} className="min-w-[92px]" />
        ))}
        <StatusPill status="stopped" label="Idle" className="min-w-[92px]" />
      </div>
      <div data-probe="toggles" className="flex flex-wrap items-center gap-3">
        <StartStopToggle running onToggle={noop} label="demo on" />
        <StartStopToggle running={false} onToggle={noop} label="demo off" />
        <StartStopToggle running busy onToggle={noop} label="demo busy" />
        <StartStopToggle
          running={false}
          disabled
          title="disabled with a why-tooltip, like the share toggle on an override site"
          onToggle={noop}
          label="demo locked"
        />
      </div>
    </div>
  );
}

/** SQLite UTC stamp `mins` minutes ago (the shape the card's `timeAgo` parses).
 *  Computed from the real clock so the rendered "Nm ago" is representative. */
function agoStamp(mins: number): string {
  return new Date(Date.now() - mins * 60_000).toISOString().slice(0, 19).replace("T", " ");
}

// Production-shaped site handles: the feed stores `uuid::new_v4()` ids, resolved
// to the current domain (`targetLabel`) at read time by commands::mcp — so the
// fixtures must carry BOTH (friendly fake ids once masked the raw-UUID display).
const EA = "7f3a1c2e-9b40-4d1a-8c22-1f0e5a6b7c8d";
const SHOP = "2b91d0f4-1a33-4e77-9a0c-8d2e4f5a6b1c";
/** A site the reaper DELETED — its label can no longer resolve, which is the
 *  normal case for a reap row and the one that must not render a bare UUID. */
const REAPED = "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24";

/** A realistic activity feed: two concerning rows (a WP-less tail_log error and
 *  an unknown-tool) newest, then successes. `targetSite` is the stored UUID; the
 *  card shows `targetLabel` (the resolved domain). */
const AGENT_ROWS: AgentAction[] = [
  { id: 9, at: agoStamp(1), actor: "agent", client: "Claude Code", tool: "wp_run", targetSite: REAPED, targetLabel: "plugin-test.scratch.rex", outcome: "ok", detail: null, argsSummary: "eval", concerning: false },
  { id: 8, at: agoStamp(1), actor: "agent", client: "Claude Code", tool: "wp_run", targetSite: REAPED, targetLabel: "plugin-test.scratch.rex", outcome: "ok", detail: null, argsSummary: "plugin activate", concerning: false },
  { id: 7, at: agoStamp(2), actor: "rexenv", client: "rexenv", tool: "scratch_reap", targetSite: REAPED, targetLabel: null, outcome: "ok", detail: "probe.scratch.rex — expired, removed", argsSummary: null, concerning: false },
  { id: 6, at: agoStamp(1), actor: "agent", client: "Claude Code", tool: "tail_log", targetSite: EA, targetLabel: "myblog.test", outcome: "error", detail: "no debug.log for this site", argsSummary: null, concerning: true },
  { id: 5, at: agoStamp(3), actor: "agent", client: "Claude Code", tool: "site_status", targetSite: SHOP, targetLabel: "shop.test", outcome: "unknown-tool", detail: "no such tool", argsSummary: null, concerning: true },
  { id: 4, at: agoStamp(4), actor: "agent", client: "Claude Code", tool: "site_status", targetSite: EA, targetLabel: "myblog.test", outcome: "ok", detail: null, argsSummary: null, concerning: false },
  { id: 3, at: agoStamp(9), actor: "agent", client: "Cursor 0.42", tool: "list_sites", targetSite: null, targetLabel: null, outcome: "ok", detail: null, argsSummary: null, concerning: false },
  { id: 2, at: agoStamp(24), actor: "agent", client: "Claude Code", tool: "tail_log", targetSite: EA, targetLabel: "myblog.test", outcome: "ok", detail: null, argsSummary: null, concerning: false },
];

function activityStatusMock(): ActivityStatus {
  switch (params.get("astate")) {
    case "off":
      return { kind: "off" };
    case "idle":
      return { kind: "idle" };
    case "erroring":
      return { kind: "erroring", errored: 2, minutesAgo: 1 };
    default:
      return { kind: "working", lastTool: "site_status", minutesAgo: 4 };
  }
}

function mcpStatusMock(): McpStatus {
  const recent = params.get("feed") === "empty" ? [] : AGENT_ROWS;
  return {
    enabled: params.get("astate") !== "off",
    // `mail=1` shows the sub-toggle ON; default OFF, which is the shipped default.
    mailEnabled: params.get("mail") === "1",
    connectCommand: "claude mcp add rexenv -- rex mcp",
    activity: activityStatusMock(),
    recent,
  };
}

/** The AI-agents card, plus (with `site=1`) the per-site SiteDetail section. */
function AgentsView() {
  return (
    <div className="space-y-4">
      <AgentsMcpCard />
      {params.get("site") === "1" && (
        <div className="rounded-xl border border-rex-border bg-rex-bg p-3">
          <div className="mb-2 text-[0.71875rem] text-rex-text-muted">
            SiteDetail → Overview section (renders only when the site has activity):
          </div>
          <SiteAgentActivity siteId={EA} />
        </div>
      )}
    </div>
  );
}

function ToastView() {
  useEffect(() => {
    toast.info(
      "Not deleted — /Users/dev/code/crm/wp-config.php was edited after the rewrite — restoring the backup would replace those edits. Choose \"restore anyway\" to proceed. Resolve it on the site's Database tab, or delete without reverting.",
    );
    toast.success("verified: the rewritten settings sign in to the rexenv copy of `ea`.");
    toast.error(
      "the edge is running, but Herd answers port 443 in front of it — every site is unreachable until you quit Herd",
      "osascript -e 'quit app \"Herd\"'",
    );
  }, []);
  return <p className="text-xs text-rex-text-muted">toasts pushed — see overlay.</p>;
}


// Fixture icons are REAL 32×32 PNGs behind a real `data:image/png;base64,` URI,
// not a placeholder string: `AppIcon` renders an <img>, so a fake-shaped
// fixture would render a broken image the probe could still find in the DOM.
// (Production URIs are ~5–9KB; only the length differs.)
const ICON = {
  chrome: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKklEQVR4nGN4EeZCU8QwasGoBaMWjFowasGoBaMWjFowasGoBaMWDBULAEDbCFvpck8UAAAAAElFTkSuQmCC",
  firefox: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKUlEQVR4nO3NQQkAAAgEsItiTNtrCh/CYP9luk5FIBAIBAKBQCAQfAkWir1cW/l31KMAAAAASUVORK5CYII=",
  safari: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKklEQVR4nO3NQQkAAAgEsItkWiOawxQ+hMH+S/WcikAgEAgEAoFAIPgSLJQ2sFvcz2b3AAAAAElFTkSuQmCC",
  vscode: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKklEQVR4nGNQrjhBU8QwasGoBaMWjFowasGoBaMWjFowasGoBaMWDBULADCAjEzTSQo3AAAAAElFTkSuQmCC",
  phpstorm: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKklEQVR4nO3NQQkAAAgEsAtmWOMYyxQ+hMH+S9ecikAgEAgEAoFAIPgSLMD2kFvPw7tMAAAAAElFTkSuQmCC",
} as const;

/** `browsers=one` collapses the list to a single app — the state where the
 *  chevron must NOT render at all. `icons=none` drops every icon, the honest
 *  degrade to the monochrome glyph. */
function mockBrowsers(): BrowserApp[] {
  const noIcons = params.get("icons") === "none";
  const all: BrowserApp[] = [
    { id: "safari", name: "Safari", icon: noIcons ? null : ICON.safari, systemDefault: false },
    { id: "chrome", name: "Google Chrome", icon: noIcons ? null : ICON.chrome, systemDefault: true },
    { id: "firefox", name: "Firefox", icon: noIcons ? null : ICON.firefox, systemDefault: false },
  ];
  if (params.get("browsers") === "one") return [all[1]];
  if (params.get("browsers") === "none") return [];
  return all;
}

function mockEditors(): EditorApp[] {
  const noIcons = params.get("icons") === "none";
  return [
    { id: "vscode", name: "VS Code", icon: noIcons ? null : ICON.vscode },
    { id: "phpstorm", name: "PhpStorm", icon: noIcons ? null : ICON.phpstorm },
  ];
}

/** The "which app opens this" surfaces: the header split button and the two
 *  Quick-links tiles that grew a chevron. Rendered from the SHIPPING
 *  components (`SplitButton`, `QuickTile`, `useBrowserMenu`) — a stand-in
 *  would prove the harness, not the app. */
function OpenInView() {
  const site = fixtureSite();
  const url = `https://${site.domain}`;
  const browser = usePreferredBrowser();
  const editor = usePreferredEditor();
  const browserMenu = useBrowserMenu(url);
  const editorMenu = useEditorMenu(site.path);
  return (
    <div className="space-y-4">
      <div data-probe="header" className="flex items-center gap-[9px] rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
        <SplitButton
          onClick={() => {}}
          menu={browserMenu}
          chevronLabel="Open this site in another browser"
        >
          <AppIcon
            icon={browser?.icon}
            fallback={<ExternalLink className="h-[15px] w-[15px]" strokeWidth={1.8} />}
            className="h-[15px] w-[15px]"
          />
          Open in browser
        </SplitButton>
      </div>
      <div data-probe="tiles" className="grid grid-cols-2 gap-[9px] rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
        <QuickTile
          icon={<AppIcon icon={browser?.icon} fallback={<Globe className="h-4 w-4" />} />}
          iconColor="text-rex-text-muted"
          label={browser ? `Open in ${browser.name}` : "Browser"}
          onClick={() => {}}
          menu={browserMenu}
          menuLabel="Open this site in another browser"
        />
        <QuickTile
          icon={<AppIcon icon={editor?.icon} fallback={<Code className="h-4 w-4" />} />}
          iconColor="text-rex-text-muted"
          label={editor ? `Open in ${editor.name}` : "Open in editor"}
          onClick={() => {}}
          menu={editorMenu}
          menuLabel="Open this project in another editor"
        />
      </div>
    </div>
  );
}

export function DevUiReview() {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "db_import_record":
          return record();
        case "db_import_state":
          return null;
        case "rewrite_preview":
          return preview();
        case "rewrite_apply":
          return applied();
        case "rewrite_revert":
          return reverted();
        case "list_editors":
          return params.get("view") === "openin" ? mockEditors() : [];
        case "list_browsers":
          return mockBrowsers();
        case "get_setting":
          return null;
        case "mcp_status":
        case "mcp_set_enabled":
          return mcpStatusMock();
        case "agent_activity":
          return params.get("feed") === "empty"
            ? []
            : AGENT_ROWS.filter((r) => r.targetSite === EA);
        case "agent_activity_clear":
          return 0;
        default:
          return 1; // plugin:event|listen etc. — accept quietly.
      }
    });
    setReady(true);
  }, []);
  if (!ready) return null;

  const view = params.get("view") ?? "card";
  return (
    <div className="min-h-screen bg-rex-bg p-6">
      <div className="mx-auto max-w-[860px] space-y-3">
        <h1 className="text-[0.8125rem] font-medium text-rex-text-muted">
          DEV harness — UI review ({view}, mocked IPC)
        </h1>
        {view === "card" && <DbImportCard site={fixtureSite({ dbEngine: params.get("engine") === "mariadb" ? "mariadb" : "mysql" })} />}
        {view === "dbtab" && <DbTabView />}
        {view === "sites" && <SitesScaleView />}
        {view === "delete" && <DeleteView />}
        {view === "badges" && <BadgesView />}
        {view === "openin" && <OpenInView />}
        {view === "provision" && <ProvisionCardView />}
        {view === "resolver" && (
          <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
            <div className="text-[0.8125rem] font-medium text-rex-text">Valet / Herd</div>
            <div className="mt-3 flex flex-col gap-2">
              <ResolverHandBackRow tld={BORROWED} />
            </div>
          </div>
        )}
        {view === "toast" && <ToastView />}
        {view === "pills" && <PillsView />}
        {view === "agents" && <AgentsView />}
        {view === "scratch" && <ScratchView />}
        {view === "keep" && (
          <KeepSiteDialog
            site={fixtureSite({ domain: "plugin-test.scratch.rex", origin: "agent" })}
            onKeep={() => {}}
            onCancel={() => {}}
          />
        )}
      </div>
    </div>
  );
}
