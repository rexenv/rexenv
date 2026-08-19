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
 *    onboarding — the last onboarding step. `edge=herd` puts a named foreign
 *               proxy on :443; `edge=anon` an unattributable one; NO param is
 *               the ordinary case (nothing listening) and must render NOTHING —
 *               reporting the normal state as a problem is the bug the import
 *               path shipped
 *    wppackages — the WP-CLI packages tell (#301). `names=none` is the variant
 *               nobody will ever see by accident: the one that renders when
 *               composer.json could not be read, so the copy must claim NO
 *               count. It exists on no developer's machine, which is exactly
 *               why it needs a render somebody can look at
 *    openin   — the "which app opens this" surfaces: header split button +
 *               the Browser/editor Quick-links tiles. `browsers=one|none`
 *               (the no-chevron and nothing-detected states), `icons=none`
 *               (the honest degrade to a monochrome glyph)
 *    mail     — the whole Mail screen against a mocked Mailpit: the All/Unread
 *               filter, Mark all read, and the read-state flip. The mock marks
 *               a message read when its DETAIL is fetched, exactly as Mailpit
 *               does, so a UI that waits for the next 5s poll is visibly wrong
 *               here. `stale=1` makes Mark all read a no-op on the server side,
 *               which is the scenario that proves the screen updates from its
 *               own patch rather than from a refetch
 */
import { useEffect, useState } from "react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { mockAdminerStatus, mockPhpVersions } from "@/lib/mock";
import { Tunnels as TunnelsScreen } from "@/routes/Tunnels";
import { ThemesPanel as ThemesScreen } from "@/components/wordpress/WordPressManager";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { DatabaseTab } from "@/components/sites/DatabaseTab";
import { DbImportCard } from "@/components/sites/DbImportCard";
import { DeleteSiteDialog } from "@/components/sites/DeleteSiteDialog";
import { KeepSiteDialog, ResolverDriftBanner, ScratchGroupHeading, SiteRow } from "@/routes/Sites";
import { ResolverHandBackRow } from "@/routes/Import";
import { PhpVersionsSetting } from "@/routes/Settings";
import { AdminerVersionCard } from "@/routes/Databases";
import { WpCliPackagesCard } from "@/routes/Settings";
import { Mail as MailScreen } from "@/routes/Mail";
import { OnboardingDone } from "@/routes/Onboarding";
import { SiteProvisionCard } from "@/components/sites/SiteProvisionCard";
import { AgentsMcpCard } from "@/components/mcp/AgentsMcpCard";
import { QuickTile } from "@/routes/SiteDetail";
import { WordPressIcon } from "@/components/common/WordPressIcon";
import { AppIcon } from "@/components/ui/app-icon";
import { SplitButton } from "@/components/ui/split-button";
import { BROWSER_MENU_WIDTH, useBrowserMenu, useEditorMenu } from "@/components/ui/open-in";
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
/** The Settings → PHP versions rows, at the width that broke them.
 *
 *  Shipped broken 17 Aug 2026: five chips (patch, serving, exists, Default, EOL)
 *  inline in a 9rem column, so a badge wrapped INTERNALLY — "EOL" on one line and
 *  "November 2022" on the next, each carrying half the pill's border. It reads as
 *  a rendering fault rather than a long label, and nothing caught it because the
 *  row had no harness scenario at all: every version of it that reviewers saw was
 *  a fresh install, where `serving` and `exists` are both absent and three chips
 *  fit.
 *
 *  The fixture is the WORST case on purpose — a row carrying every chip at once —
 *  because the common case is exactly what hid this. */
function PhpVersionsView() {
  return (
    <div className="flex flex-col gap-4">
      {/* The real Settings content column, not the viewport. The bug lives in
          the column width; rendering this full-bleed would pass and prove
          nothing. */}
      <div data-probe="phpversions" className="w-[22rem] rounded border border-rex-border p-3">
        <PhpVersionsSetting />
      </div>
    </div>
  );
}

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
 *  degrade to the monochrome glyph.
 *
 *  Safari carries `supportsPrivate: false` because the real detection does: it
 *  has no private-window command line. A fixture where every browser could open
 *  privately would render a menu no Mac produces, and the mixed row — one entry
 *  with no private target next to two that have one — is exactly the layout
 *  that has to hold up. */
function mockBrowsers(): BrowserApp[] {
  const noIcons = params.get("icons") === "none";
  const all: BrowserApp[] = [
    { id: "safari", name: "Safari", icon: noIcons ? null : ICON.safari, systemDefault: false, supportsPrivate: false },
    { id: "chrome", name: "Google Chrome", icon: noIcons ? null : ICON.chrome, systemDefault: true, supportsPrivate: true },
    { id: "firefox", name: "Firefox", icon: noIcons ? null : ICON.firefox, systemDefault: false, supportsPrivate: true },
  ];
  if (params.get("browsers") === "one") return [all[1]];
  if (params.get("browsers") === "none") return [];
  return all;
}

/** The Mail screen's inbox fixture. Shaped like real captured mail, not like a
 *  demo: two sites' worth of WordPress notifications with the addresses that
 *  make the site grouping work, a mix of read and unread, and enough of them
 *  that "find the new ones" is a real question — which is the whole reason the
 *  unread filter exists. `MAIL_READ` is mutable on purpose: previewing a
 *  message marks it read in Mailpit as a side effect of fetching it, and a mock
 *  that never reflected that would let a broken read-state flip look fine. */
const MAIL_READ = new Set<string>(["m-4", "m-6"]);
const MAIL_FIXTURE = [
  { id: "m-1", from: { name: "WordPress", address: "wordpress@shop.rex" }, to: [{ name: "", address: "owner@example.com" }], subject: "New order #1042", created: "2026-08-14T09:41:00Z", snippet: "A new order has been placed" },
  { id: "m-2", from: { name: "WordPress", address: "wordpress@shop.rex" }, to: [{ name: "", address: "owner@example.com" }], subject: "Password reset requested", created: "2026-08-14T09:12:00Z", snippet: "Someone asked to reset" },
  { id: "m-3", from: { name: "Contact form", address: "forms@blog.rex" }, to: [{ name: "", address: "editor@example.com" }], subject: "New enquiry from Rina", created: "2026-08-14T08:55:00Z", snippet: "Hello, I wanted to ask" },
  { id: "m-4", from: { name: "WordPress", address: "wordpress@blog.rex" }, to: [{ name: "", address: "editor@example.com" }], subject: "Plugin updated: Akismet", created: "2026-08-13T22:03:00Z", snippet: "Akismet was updated" },
  { id: "m-5", from: { name: "WordPress", address: "wordpress@shop.rex" }, to: [{ name: "", address: "owner@example.com" }], subject: "Your site has updates", created: "2026-08-13T20:15:00Z", snippet: "Please update" },
  { id: "m-6", from: { name: "Newsletter", address: "news@somewhere.test" }, to: [{ name: "", address: "owner@example.com" }], subject: "Weekly digest", created: "2026-08-13T07:00:00Z", snippet: "This week in" },
];

function mailList(query: string | undefined, unreadOnly: boolean) {
  const q = (query ?? "").trim().toLowerCase();
  const all = MAIL_FIXTURE.map((m) => ({ ...m, read: MAIL_READ.has(m.id) }));
  let messages = q
    ? all.filter((m) => m.subject.toLowerCase().includes(q) || m.from.address.toLowerCase().includes(q))
    : all;
  if (unreadOnly) messages = messages.filter((m) => !m.read);
  // total/unread stay MAILBOX-WIDE while filtering, exactly as Mailpit answers
  // — the filter chip's count depends on it.
  return { total: all.length, unread: all.filter((m) => !m.read).length, messages };
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
          menuWidth={BROWSER_MENU_WIDTH}
          chevronLabel="Open this site in another browser"
        >
          <AppIcon
            icon={browser?.icon}
            fallback={<ExternalLink className="h-[15px] w-[15px]" strokeWidth={1.8} />}
            className="h-[15px] w-[15px]"
          />
          Open in browser
        </SplitButton>
        <SplitButton
          variant="primary"
          onClick={() => {}}
          menu={browserMenu}
          menuWidth={BROWSER_MENU_WIDTH}
          chevronLabel="Sign in through another browser"
        >
          <WordPressIcon className="h-[15px] w-[15px]" />
          Magic Login
        </SplitButton>
      </div>
      <div data-probe="tiles" className="grid grid-cols-2 gap-[9px] rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
        <QuickTile
          icon={<AppIcon icon={browser?.icon} fallback={<Globe className="h-4 w-4" />} />}
          iconColor="text-rex-text-muted"
          label={browser ? `Open in ${browser.name}` : "Browser"}
          onClick={() => {}}
          menu={browserMenu}
          menuWidth={BROWSER_MENU_WIDTH}
          menuLabel="Open this site in another browser"
        />
        <QuickTile
          icon={<WordPressIcon className="h-4 w-4" />}
          iconColor="text-rex-accent-blue"
          label="Magic Login"
          onClick={() => {}}
          menu={browserMenu}
          menuWidth={BROWSER_MENU_WIDTH}
          menuLabel="Sign in through another browser"
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
    mockIPC((cmd, args) => {
      switch (cmd) {
        // Recorded, not just accepted: the "open it privately" target has to be
        // provable, and the only difference between it and the row next to it is
        // one argument. The wk-check reads these back — an icon that fired the
        // ordinary open would otherwise look identical from the outside.
        case "open_in_browser":
        case "open_external": {
          const w = window as unknown as { __rexOpens?: unknown[] };
          (w.__rexOpens ??= []).push({ cmd, args });
          return null;
        }
        // The PHP versions rows. Absent until 17 Aug 2026, so the view fell to
        // the default arm and got `null` — which `versions[0]?.…` tolerated
        // silently and `versions.some(…)` did not. The harness surfaced it as a
        // crash the moment a second reader of that array appeared; a fixture
        // that returns the wrong SHAPE is the same defect family as one that
        // returns friendly values.
        case "list_php_versions":
        // The manifest refresh the PHP section fires on mount. Unmocked, it fell
        // to the default arm and published `1` into the versions cache, and
        // `versions.some(…)` threw — the whole view rendered nothing.
        case "php_update_check":
          return mockPhpVersions;
        // The Adminer version card. Four states via `?adminer=`, because a
        // fixture in one state proves that one state renders.
        case "adminer_status":
        case "adminer_update_check":
          switch (params.get("adminer")) {
            // Already on the newest — no button, nothing amber.
            case "current":
              return { staged: "6.0.1", effective: "6.0.1", updatable: null };
            // Chosen but not restaged yet: the ONE case that is amber.
            case "pending":
              return { staged: "5.4.2", effective: "6.0.1", updatable: null };
            // Before the first start: nothing staged is a different sentence
            // from "staged, and it is 5.4.2".
            case "fresh":
              return { staged: null, effective: "5.4.2", updatable: "6.0.1" };
            default:
              return mockAdminerStatus;
          }
        case "adminer_update_apply":
          return {
            staged: String((args as Record<string, unknown> | undefined)?.version ?? ""),
            effective: String((args as Record<string, unknown> | undefined)?.version ?? ""),
            updatable: null,
          };
        case "php_update_apply":
          return {
            patch: String((args as Record<string, unknown> | undefined)?.patch ?? ""),
            restarted: true,
          };
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
        case "setup_edge_conflict":
          // No param = nothing on :443. The backend returns null there and the
          // notice must not render: at onboarding the stack is not running yet.
          if (params.get("edge") === "herd") {
            return { holder: "Herd (nginx, pid 554)", app: "Herd", fix: "osascript -e 'quit app \"Herd\"'" };
          }
          if (params.get("edge") === "anon") {
            return { holder: null, app: null, fix: null };
          }
          return null;
        case "wp_cli_packages":
          // `names=none` = a packages dir that exists and could not be named.
          return params.get("names") === "none"
            ? { dir: "~/.wp-cli/packages", names: [] }
            : {
                dir: "~/.wp-cli/packages",
                names: ["danielbachhuber/php-compat-command", "wp-cli/dist-archive-command"],
              };
        case "mailpit_status":
          return { running: true, uiUrl: "http://127.0.0.1:18025", smtpPort: 11025, httpPort: 18025 };
        case "mailpit_messages": {
          const a = (args ?? {}) as { query?: string; unreadOnly?: boolean };
          const w = window as unknown as { __mailCalls?: unknown[] };
          (w.__mailCalls ??= []).push({ cmd, query: a.query ?? "", unreadOnly: !!a.unreadOnly });
          return mailList(a.query, !!a.unreadOnly);
        }
        case "mailpit_message": {
          const id = String((args as { id?: string } | undefined)?.id ?? "");
          // Mailpit marks a message read as a SIDE EFFECT of this fetch. The
          // mock does the same, so a UI that only flips on the next poll is
          // visibly wrong here rather than accidentally right.
          MAIL_READ.add(id);
          const m = MAIL_FIXTURE.find((x) => x.id === id) ?? MAIL_FIXTURE[0];
          return {
            id: m.id, from: m.from, to: m.to, cc: [], subject: m.subject,
            date: m.created, text: `${m.snippet}…`, html: "",
            headers: [{ name: "Subject", value: m.subject }],
          };
        }
        case "mailpit_mark_all_read": {
          const w = window as unknown as { __mailCalls?: unknown[] };
          (w.__mailCalls ??= []).push({ cmd });
          // Deliberately NOT applied to MAIL_READ when `?stale=1`: that is the
          // scenario proving the screen updates from its own patch rather than
          // from the next 5s poll, which is the difference the user feels.
          if (params.get("stale") !== "1") MAIL_FIXTURE.forEach((m) => MAIL_READ.add(m.id));
          return null;
        }
        case "list_sites":
          if (params.get("view") === "mail") {
            return [
              fixtureSite({ id: "s-shop", name: "shop", domain: "shop.rex" }),
              fixtureSite({ id: "s-blog", name: "blog", domain: "blog.rex" }),
            ];
          }
          // The tunnels fixture: TWO shared and two not, because the claim
          // under test is what a filter does to a row that is currently
          // PUBLIC. One shared site would let a probe pass on a count of 1
          // where the copy has to say "2 shared sites are".
          if (params.get("view") === "tunnels") {
            return [
              fixtureSite({ id: "s-shop", name: "shop", domain: "shop.rex" }),
              fixtureSite({ id: "s-blog", name: "blog", domain: "blog.rex", type: "laravel" }),
              fixtureSite({ id: "s-docs", name: "docs", domain: "docs.rex" }),
              fixtureSite({ id: "s-api", name: "api", domain: "api.rex", type: "laravel" }),
            ];
          }
          return [];
        // The two live tunnels behind that fixture. Real trycloudflare-shaped
        // URLs, because "paste the link you are holding" is one of the three
        // things the search box is for.
        case "tunnels_status":
          return params.get("view") === "tunnels"
            ? [
                {
                  domain: "shop.rex",
                  url: "https://odd-cat-42.trycloudflare.com",
                  running: true,
                  health: "reachable",
                  diagnosis: null,
                },
                {
                  domain: "blog.rex",
                  url: "https://tall-moon-19.trycloudflare.com",
                  running: true,
                  health: "reachable",
                  diagnosis: null,
                },
              ]
            : [];
        // No default-credentials warnings in this fixture: the claim under test
        // is the filter, and a wall of amber would make the ONE amber row that
        // matters indistinguishable in a screenshot.
        case "wp_default_creds":
          return false;
        // The themes grid. Titles that are NOT the slug, plus one theme whose
        // header carries no name at all — a fixture where every title equalled
        // its slug would render identically whether the card read the title or
        // fell back to it, which is the whole thing under test.
        case "wp_themes":
          return [
            { name: "twentytwentyfive", title: "Twenty Twenty-Five", status: "active", version: "1.5", update: "none", updateVersion: "", screenshot: null },
            { name: "twentytwentyfour", title: "Twenty Twenty-Four", status: "inactive", version: "1.5", update: "available", updateVersion: "1.6", screenshot: null },
            { name: "custom-child", title: "", status: "inactive", version: "1.0", update: "none", updateVersion: "", screenshot: null },
          ];
        case "repo_assets":
        case "repo_unmanaged":
          return [];
        // The THEMES panel's install card (`?install=ok|partial`). It is the
        // same hook and the same component as the plugins panel — which is an
        // argument, not evidence, and the reason this fixture exists: the
        // linger/dismiss rules are asserted against the themes panel too, in a
        // job whose `kind` really is "theme".
        case "wp_install_active":
          return params.get("install") === "ok" || params.get("install") === "partial"
            ? {
                id: "wpi-theme-dev",
                siteId: "s-ea",
                kind: "theme",
                source: "wporg",
                slugs: ["astra"],
                itemsTotal: 1,
                itemCursor: 1,
                pct: params.get("install") === "ok" ? 100 : 40,
                status: params.get("install"),
                summary:
                  params.get("install") === "ok"
                    ? "Success: Installed 1 of 1 themes."
                    : "Error: Only installed 0 of 1 themes.",
                error: null,
                logKey: "wp-install-dev.rex-wpi-theme-dev.log",
              }
            : null;
        case "tail_log":
          return ["Installing the theme...", "Downloading installation package..."];
        case "wp_install_cancel":
          return null;
        case "list_editors":
          return params.get("view") === "openin" ? mockEditors() : [];
        case "list_browsers":
          return mockBrowsers();
        case "get_setting":
          return null;
        case "resolver_drift":
          // `?drift=test,dev` is the fixture; absent → [] (the ordinary state,
          // which the probe requires to render NOTHING).
          return (params.get("drift") ?? "").split(",").map((t) => t.trim()).filter(Boolean);
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
          // Tauri's own plumbing is accepted quietly; an APP command is not.
          // The quiet `return 1` published a number into a query cache and took
          // a whole view down with `versions.some is not a function` — a harness
          // that answers every question with a friendly value cannot fail, and
          // an unmocked command is the harness saying it does not know.
          if (cmd.startsWith("plugin:") || cmd.startsWith("tauri")) return 1;
          throw new Error(
            `DevUiReview has no fixture for "${cmd}" — add a case rather than ` +
              `letting a placeholder value reach the view under test.`
          );
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
        {view === "drift" && (
          // The banner + a landmark that renders REGARDLESS, so the probe's
          // "no banner" legs can tell zero-render from a broken route.
          <div data-probe="drift-view">
            <ResolverDriftBanner />
            <div className="text-[0.71875rem] text-rex-text-muted">
              drift harness mounted (fixture: ?drift=tld1,tld2)
            </div>
          </div>
        )}
        {view === "openin" && <OpenInView />}
        {view === "themes" && (
          <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
            <ThemesScreen siteId="s-ea" />
          </div>
        )}
        {view === "tunnels" && (
          // The REAL route, in a frame the height of the app's own region, so
          // the sections scroll exactly as they do in the app.
          <div className="h-[720px] overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
            <div className="flex h-full flex-col">
              <TunnelsScreen />
            </div>
          </div>
        )}
        {view === "mail" && (
          <div className="h-[620px] overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
            <div className="flex h-full flex-col">
              <MailScreen />
            </div>
          </div>
        )}
        {view === "provision" && <ProvisionCardView />}
        {view === "resolver" && (
          <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
            <div className="text-[0.8125rem] font-medium text-rex-text">Valet / Herd</div>
            <div className="mt-3 flex flex-col gap-2">
              <ResolverHandBackRow tld={BORROWED} />
            </div>
          </div>
        )}
        {view === "wppackages" && <WpCliPackagesCard />}
        {view === "onboarding" && <OnboardingDone />}
        {view === "toast" && <ToastView />}
        {view === "pills" && <PillsView />}
        {view === "phpversions" && <PhpVersionsView />}
        {view === "adminer" && (
          // The real Databases content width, not the viewport: this card sits
          // under the engine table and its chips have to fit there.
          <div className="w-[46rem]">
            <AdminerVersionCard />
          </div>
        )}
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
