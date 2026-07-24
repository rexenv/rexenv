/** DEV-ONLY WebKit render harness for GitAddPanel (`#/dev/git-panel`).
 *  Mounted only when `import.meta.env.DEV` (see App.tsx) — never part of a
 *  production bundle. Mocks the Tauri IPC layer (`mockIPC`) with canned
 *  `repo_*` responses so the panel's visual states render in a plain browser
 *  (Playwright WebKit ≈ the packaged WKWebView engine) without an app
 *  backend and WITHOUT touching the real app — the no-synthetic-clicks rule.
 */
import { useEffect, useState } from "react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { GitAddPanel } from "@/components/wordpress/GitAddPanel";
import { RepoPanel } from "@/components/wordpress/RepoPanel";
import { LinkFolderPanel } from "@/components/wordpress/LinkFolderPanel";
import { PluginsPanel } from "@/components/wordpress/WordPressManager";
import { SiteProvisionCard, useSiteProvision } from "@/components/sites/SiteProvisionCard";
import { SiteRow } from "@/routes/Sites";
import { useDownloads } from "@/lib/useDownloads";
import { siteProvisionRetry } from "@/lib/ipc";

const PROBE = {
  url: "https://github.com/acme/my-plugin",
  host: "github.com",
  dirName: "my-plugin",
  refCandidate: null,
  defaultBranch: "main",
  branches: ["main", "develop", "feat/fast-build"],
  tags: ["v1.2.0", "v1.1.0"],
};

/** A snapshot exercising every surface at once: done steps, a FAILED step
 *  with a mapped multi-line error (incl. the `$ `-fix line), pending steps,
 *  the node-version warning, and detection results. */
const JOB = {
  id: "dev-job",
  siteId: "dev",
  kind: "plugin",
  dirName: "my-plugin",
  url: "https://github.com/acme/my-plugin",
  gitRef: "develop",
  op: "add",
  steps: [
    { key: "clone", label: "Clone repository", status: "ok", error: null },
    { key: "detect", label: "Detect dependencies", status: "ok", error: null },
    {
      key: "composer",
      label: "composer install",
      status: "failed",
      error:
        "A native module failed to compile (node-gyp). That needs the Xcode \
Command Line Tools — install them, then retry:\n$ xcode-select --install",
    },
    { key: "install", label: "pnpm install", status: "pending", error: null },
    { key: "build", label: "pnpm run build", status: "pending", error: null },
  ],
  inspection: {
    composer: true,
    node: { manager: "pnpm", pinnedBy: "packageManager", hasBuild: true },
    wp: { kind: "plugin", name: "My Plugin" },
    nodeWant: "18",
  },
  nodeWarning:
    "This repo wants Node 18 — you have v22.23.1. Installs and builds may fail; " +
    "switch with your version manager first (e.g. `nvm install 18`), then hit Re-detect.",
  finishedOk: false,
};

/** `?rehydrate=1`: the panel must ADOPT this running job on mount with zero
 *  clicks (the tab-return reconnect fix) and seed its log from tail_log. */
const RUNNING_JOB = {
  ...JOB,
  id: "dev-running",
  gitRef: "main",
  logKey: "repo-dev.rex-my-plugin.log",
  steps: [
    { key: "clone", label: "Clone repository", status: "ok", error: null },
    { key: "detect", label: "Detect dependencies", status: "ok", error: null },
    { key: "composer", label: "composer install", status: "ok", error: null },
    { key: "install", label: "pnpm install", status: "running", error: null },
    { key: "build", label: "pnpm run build", status: "pending", error: null },
  ],
  nodeWarning: null,
};

const TAIL_LINES = [
  "$ git clone --progress --recurse-submodules -- https://github.com/acme/my-plugin …",
  "Receiving objects: 100% (1432/1432), done.",
  "✓ plugin header: My Plugin",
  "$ composer install --no-interaction",
  "Generating autoload files",
  "$ pnpm install",
  "Progress: resolved 212, reused 212, downloaded 0",
];

/** `?panel=repo`: render the phase-A RepoPanel with a rich mocked status
 *  (dirty + ahead/behind + loss warning) for the WebKit layout check. */
const ASSET = {
  kind: "plugin" as const,
  dirName: "my-plugin",
  url: "https://github.com/acme/my-plugin",
  gitRef: "develop",
  source: "cloned",
};

const OP_JOB = {
  id: "dev-op",
  siteId: "dev",
  kind: "plugin",
  dirName: "my-plugin",
  url: "",
  gitRef: null,
  op: "pull",
  logKey: "repo-dev.rex-my-plugin.log",
  steps: [
    { key: "pull", label: "git pull --ff-only", status: "ok", error: null },
    { key: "composer", label: "composer install", status: "pending", error: null },
    { key: "install", label: "pnpm install", status: "pending", error: null },
  ],
  inspection: {
    composer: true,
    node: { manager: "pnpm", pinnedBy: "lockfile", hasBuild: false },
    wp: { kind: "plugin", name: "My Plugin" },
    nodeWant: null,
  },
  nodeWarning: null,
  finishedOk: false,
};

const ASSET_STATUS = {
  branch: "feat/x",
  detached: false,
  detachedAt: null,
  unborn: false,
  upstream: "origin/feat/x",
  ahead: 2,
  behind: 1,
  changed: 3,
  untracked: 2,
  remote: "git@github.com:acme/my-plugin.git",
  lossWarning: "3 changed files, 2 untracked files, and 2 unpushed commits will be lost.",
  logKey: "repo-dev.rex-my-plugin.log",
  linkTarget: null,
};

/** `?detached=1`: honest detached-HEAD rendering — "detached @ tag" chip,
 *  Pull/Push disabled with a reason, no false ↑0 ↓0, no "no upstream" noise. */
const DETACHED_STATUS = {
  ...ASSET_STATUS,
  branch: null,
  detached: true,
  detachedAt: "v1.2.0",
  upstream: null,
  ahead: null,
  behind: null,
  lossWarning:
    "This checkout can't be verified as pushed (detached HEAD — commits made here may not be on any branch).",
};

/** Settled zero-exec check jobs (`repo_check` returns AFTER the worker ran).
 *  NEEDED: composer stale + node missing → steps offered. CLEAN: report only. */
const CHECK_JOB_BASE = {
  id: "dev-check",
  siteId: "dev",
  kind: "plugin",
  dirName: "my-plugin",
  url: "",
  gitRef: null,
  op: "check",
  logKey: "repo-dev.rex-my-plugin-check.log",
  inspection: {
    composer: true,
    node: { manager: "pnpm", pinnedBy: "lockfile", hasBuild: true },
    wp: { kind: "plugin", name: "My Plugin" },
    nodeWant: null,
  },
  nodeWarning: null,
  finishedOk: false,
};
const CHECK_JOB_NEEDED = {
  ...CHECK_JOB_BASE,
  steps: [
    { key: "check", label: "Check dependencies", status: "ok", error: null },
    { key: "composer", label: "composer install", status: "pending", error: null },
    { key: "install", label: "pnpm install", status: "pending", error: null },
    { key: "build", label: "pnpm run build", status: "pending", error: null },
  ],
};
const CHECK_JOB_CLEAN = {
  ...CHECK_JOB_BASE,
  steps: [{ key: "check", label: "Check dependencies", status: "ok", error: null }],
};
/** "Run all" outcome snapshot: stop-on-failure honesty — composer ran and
 *  passed, install FAILED (mapped error), build never ran → "skipped". */
const RUNALL_JOB_FAILED = {
  ...CHECK_JOB_BASE,
  steps: [
    { key: "check", label: "Check dependencies", status: "ok", error: null },
    { key: "composer", label: "composer install", status: "ok", error: null },
    {
      key: "install",
      label: "pnpm install",
      status: "failed",
      error: "pnpm install failed (exit 1) — see the log below.",
    },
    { key: "build", label: "pnpm run build", status: "skipped", error: null },
  ],
};

const CHECK_LINES = [
  "composer: lockfile changed since last install — install recommended",
  "node (pnpm): node_modules/ missing — install needed",
  "! install steps offered below — nothing runs without a click.",
];

/** Streamed wp.org install-card states (`?panel=wp-add&install=…`). Lines
 *  mirror the real wp-cli output shape (phase lines verbatim). */
const WPI_BASE = {
  id: "wpi-dev",
  siteId: "dev",
  kind: "plugin" as const,
  slugs: ["akismet", "bbpress"],
  itemsTotal: 2,
  logKey: "wp-install-dev.rex-wpi-dev.log",
  error: null,
};
// pct mirrors WPI_LINES: item 1 complete, item 2 at the Downloading slice →
// (1 + 1/4)/2 = 62 (phase-based observed progress, backend-computed).
const WPI_RUNNING = { ...WPI_BASE, itemCursor: 2, pct: 62, status: "running", summary: null };
const WPI_PARTIAL = {
  ...WPI_BASE,
  itemCursor: 2,
  pct: 62, // FROZEN where the batch stopped — never 100, never 0
  status: "partial",
  summary: "Error: Only installed 1 of 2 plugins.",
};
const WPI_OK = {
  ...WPI_BASE,
  itemCursor: 2,
  pct: 100,
  status: "ok",
  summary: "Success: Installed 2 of 2 plugins.",
};
const WPI_CANCELLED = {
  ...WPI_BASE,
  itemCursor: 1,
  pct: 25, // cancelled mid-download of item 1 — bar stops exactly here
  status: "cancelled",
  summary: null,
};
const WPI_LINES = [
  "Installing Akismet Anti-spam (5.3)",
  "Downloading installation package from https://downloads.wordpress.org/plugin/akismet.5.3.zip...",
  "Unpacking the package...",
  "Installing the plugin...",
  "Plugin installed successfully.",
  "Installing bbPress (2.6.11)",
  "Downloading installation package from https://downloads.wordpress.org/plugin/bbpress.2.6.11.zip...",
];

/** Streamed site-provision card states (`?panel=provision&prov=…`). Phase
 *  values mirror the real backend staircase (weights 3/27/5/35/5/10/10). */
const PROV_PHASE = (key: string, label: string, status: string) => ({ key, label, status });
const PROV_PHASES = (cur: string, curStatus: string) => {
  const order: Array<[string, string]> = [
    ["prepare", "preparing site (domain, certificate)"],
    ["fetch", "downloading binaries"],
    ["db", "starting database"],
    ["core_download", "downloading WordPress core"],
    ["configure", "writing wp-config + creating database"],
    ["core_install", "installing WordPress"],
    ["serve", "starting to serve"],
  ];
  const at = order.findIndex(([k]) => k === cur);
  return order.map(([k, l], i) =>
    PROV_PHASE(k, l, i < at ? "ok" : i === at ? curStatus : "pending"),
  );
};
const PROV_BASE = {
  id: "prov-dev",
  domain: "shop.rex",
  siteId: "sdev",
  summary: null as string | null,
  error: null as string | null,
  logKey: "site-provision-shop.rex-provdev1.log",
  downloadIds: [] as string[],
};
const PROV_RUNNING = {
  ...PROV_BASE,
  status: "running",
  phaseCursor: 3,
  pct: 36,
  phases: PROV_PHASES("core_download", "running"),
};
const PROV_FETCH = {
  ...PROV_BASE,
  status: "running",
  phaseCursor: 1,
  pct: 12, // 3 (prepare) + 27 × byte-fraction — real bytes, folded
  phases: PROV_PHASES("fetch", "running"),
  downloadIds: ["php-fpm-8.3.31", "mysql-8.4.6"],
};
const PROV_OK = {
  ...PROV_BASE,
  status: "ok",
  phaseCursor: 6,
  pct: 100,
  phases: PROV_PHASES("serve", "skipped"),
  summary: "created — stack is stopped, shop.rex serves on next stack start",
};
const PROV_FAILED = {
  ...PROV_BASE,
  status: "failed",
  phaseCursor: 3,
  pct: 36, // FROZEN where the work stopped
  phases: PROV_PHASES("core_download", "failed"),
  error: "wp core download failed: Error: The requested locale (xx_XX) was not found.",
};
const PROV_CANCELLED = {
  ...PROV_BASE,
  status: "cancelled",
  phaseCursor: 3,
  pct: 36, // FROZEN — never snapped to 100, never reset
  phases: PROV_PHASES("core_download", "cancelled"),
};
const PROV_LINES = [
  "── preparing site (shop.rex) — done",
  "── downloading binaries",
  "binaries already cached",
  "── starting database",
  "── downloading WordPress core",
  "Downloading WordPress 7.0.2 (en_US)...",
];
/** Fetch-phase Hub items: one mid-stream (real bytes), one queued with no
 *  Content-Length (indeterminate). */
const PROV_DL_ITEMS = [
  { id: "php-fpm-8.3.31", name: "php-fpm", version: "8.3.31", label: "PHP 8.3 (FPM)", phase: "downloading", downloadedBytes: 13002342, totalBytes: 35651584, bytesPerSec: 2202009, error: null },
  { id: "mysql-8.4.6", name: "mysql", version: "8.4.6", label: "MySQL 8.4", phase: "pending", downloadedBytes: 0, totalBytes: null, bytesPerSec: null, error: null },
];
/** A half-provisioned site (v16 provisioned=false) — the badge row. */
const INCOMPLETE_SITE = {
  id: "sdev",
  name: "Shop",
  domain: "shop.rex",
  type: "wordpress" as const,
  status: "stopped" as const,
  phpVersion: "8.3",
  webServer: "nginx" as const,
  ssl: true,
  path: "/tmp/shop.rex",
  createdAt: "2026-07-24 00:00:00",
  multisite: "none" as const,
  dbName: "wp_shop_rex",
  dbEngine: "mysql" as const,
  xdebug: false,
  provisioned: false,
};

/** `?panel=provision` host: adopts the mocked active job through the REAL
 *  `useSiteProvision` hook (active → tailLog seed — the dialog-close/remount
 *  re-adoption path), and renders the REAL SiteRow badge for a
 *  `provisioned=false` site whose Retry starts a new job on the card. */
function ProvisionHost() {
  const downloads = useDownloads();
  const prov = useSiteProvision();
  useEffect(() => {
    void prov.adopt();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <div className="flex flex-col gap-3">
      {prov.job && (
        <SiteProvisionCard
          job={prov.job}
          lines={prov.lines}
          downloads={downloads}
          onCancel={() => {}}
        />
      )}
      <div className="rounded-lg border border-rex-border bg-rex-surface-1 px-2 py-1">
        <SiteRow
          site={INCOMPLETE_SITE}
          status="stopped"
          onOpen={() => {}}
          onDelete={() => {}}
          onOpenDatabase={() => {}}
          onOpenWordpress={() => {}}
          onRename={() => {}}
          onDuplicate={() => {}}
          onRetry={() => void siteProvisionRetry(INCOMPLETE_SITE.id).then(prov.start)}
        />
      </div>
    </div>
  );
}

export function DevGitPanel() {
  const [ready, setReady] = useState(false);
  const params = new URLSearchParams(window.location.search);
  const rehydrate = params.get("rehydrate") === "1";
  const showRepoPanel = params.get("panel") === "repo";
  const showLinkPanel = params.get("panel") === "link";
  const watchMode = params.get("watch"); // "1" running | "exited"
  const detached = params.get("detached") === "1";
  useEffect(() => {
    mockIPC(async (cmd, args) => {
      switch (cmd) {
        case "repo_site_jobs":
          return rehydrate ? [RUNNING_JOB] : [];
        case "repo_asset_status":
          return detached ? DETACHED_STATUS : ASSET_STATUS;
        case "repo_scripts":
          return {
            manager: "pnpm",
            scripts: [
              { name: "build", command: "wp-scripts build", watchy: false },
              { name: "lint", command: "eslint .", watchy: false },
              { name: "start", command: "wp-scripts start", watchy: true },
            ],
          };
        case "repo_watches":
          return watchMode === "1"
            ? [{ id: "dev-watch", siteId: "dev", kind: "plugin", dirName: "my-plugin", script: "start", status: "running", exit: null }]
            : watchMode === "exited"
              ? [{ id: "dev-watch", siteId: "dev", kind: "plugin", dirName: "my-plugin", script: "start", status: "exited", exit: 1 }]
              : [];
        case "repo_link":
          return {
            dirName: "my-plugin",
            isGit: true,
            wp: { kind: "plugin", name: "My Plugin" },
          };
        case "plugin:dialog|open":
          return "/Users/dev/checkouts/my-plugin";
        case "repo_watch_log":
          return ["$ pnpm run start", "webpack 5.99 compiled successfully in 830 ms"];
        case "repo_branches":
          // Many branches on purpose — the RefPicker search must stay usable
          // at the ~100-branch scale the plain <select> drowned in.
          return {
            // Detached HEAD ⇒ no current branch (matches parse_status_v2).
            current: detached ? null : "feat/x",
            local: ["feat/x", "main"],
            remote: [
              "origin/main",
              "origin/develop",
              ...Array.from({ length: 96 }, (_, i) => `origin/feat/topic-${i + 1}`),
            ],
            tags: ["v1.2.0", "v1.1.0", "v1.0.0"],
          };
        case "repo_pull_refs":
          // Highest first (backend contract). `?prs=none` exercises the
          // empty-note path.
          return params.get("prs") === "none"
            ? []
            : [
                { number: 128, sha: "9b36e16aa02", ref: "refs/pull/128/head" },
                { number: 97, sha: "821807eff31", ref: "refs/pull/97/head" },
                { number: 42, sha: "5c0ffee4d2b", ref: "refs/pull/42/head" },
              ];
        case "repo_git_op":
          return OP_JOB;
        case "tail_log": {
          const key = String((args as { key?: string } | undefined)?.key ?? "");
          return key.includes("site-provision-")
            ? PROV_LINES
            : key.includes("wp-install-")
              ? WPI_LINES
              : key.includes("-check")
                ? CHECK_LINES
                : TAIL_LINES;
        }
        // `?panel=provision` (streamed site-create card + badge row) mocks:
        case "site_provision_active": {
          const p = params.get("prov");
          return p === "running"
            ? PROV_RUNNING
            : p === "fetch"
              ? PROV_FETCH
              : p === "ok"
                ? PROV_OK
                : p === "failed"
                  ? PROV_FAILED
                  : p === "cancelled"
                    ? PROV_CANCELLED
                    : null;
        }
        case "site_provision_retry":
          return { ...PROV_RUNNING, id: "prov-retry" };
        // SiteRow (badge check) pulls the editor prefs:
        case "list_editors":
          return [];
        case "get_setting":
          return null;
        case "site_provision_cancel":
          return null;
        case "downloads_state":
          return params.get("prov") === "fetch"
            ? { batch: { action: "Create site", done: 0, total: 2 }, items: PROV_DL_ITEMS }
            : { batch: null, items: [] };
        // `?panel=wp-add` install-card mocks (`&install=running|partial`):
        case "wp_install_active":
          return params.get("install") === "running"
            ? WPI_RUNNING
            : params.get("install") === "partial"
              ? WPI_PARTIAL
              : params.get("install") === "ok"
                ? WPI_OK
                : params.get("install") === "cancelled"
                  ? WPI_CANCELLED
                  : null;
        case "wp_install_job":
          return WPI_RUNNING;
        case "wp_install_cancel":
          return null;
        // `?panel=wp-add` (chips-above-input layout check) mocks:
        case "wp_plugins":
          return [];
        case "wp_org_plugin_icons":
          return {};
        case "repo_assets":
        case "repo_unmanaged":
          return [];
        case "wp_org_search_plugins":
          return [
            { slug: "akismet", name: "Akismet Anti-spam", author: "Automattic", rating: 92, numRatings: 900, activeInstalls: 5000000, icon: null, shortDescription: "Spam protection" },
            { slug: "wordpress-seo", name: "Yoast SEO", author: "Team Yoast", rating: 96, numRatings: 27000, activeInstalls: 10000000, icon: null, shortDescription: "SEO" },
            { slug: "woocommerce", name: "WooCommerce", author: "Automattic", rating: 88, numRatings: 4000, activeInstalls: 7000000, icon: null, shortDescription: "Shop" },
          ];
        case "repo_check":
          // `?check=clean` exercises the nothing-to-install card.
          return params.get("check") === "clean" ? CHECK_JOB_CLEAN : CHECK_JOB_NEEDED;
        case "repo_run_offered_steps":
          // Mock returns the FINAL state (no events in the harness) — the
          // real backend returns the pre-run snapshot and streams updates.
          return RUNALL_JOB_FAILED;
        case "repo_tools":
          return [
            {
              name: "git",
              ok: true,
              version: "git version 2.50.1 (Apple Git-155)",
              path: "/usr/bin/git",
              error: null,
            },
            {
              name: "node",
              ok: true,
              version: "v22.23.1",
              path: "/Users/dev/.nvm/versions/node/v22.23.1/bin/node",
              error: null,
            },
          ];
        case "repo_probe":
          return PROBE;
        case "repo_add":
          return JOB;
        case "repo_job_state":
          return rehydrate ? RUNNING_JOB : JOB;
        case "repo_run_step":
        case "repo_cancel":
          return null;
        default:
          // plugin:event|listen etc. — accept quietly.
          return 1;
      }
    });
    setReady(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (!ready) return null;
  return (
    <div className="min-h-screen bg-rex-bg p-6">
      <div className="mx-auto max-w-[860px] space-y-3">
        <h1 className="text-[0.8125rem] font-medium text-rex-text-muted">
          DEV harness — GitAddPanel (mocked IPC)
        </h1>
        {params.get("panel") === "provision" ? (
          <ProvisionHost />
        ) : params.get("panel") === "wp-add" ? (
          <div className="rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
            <PluginsPanel siteId="dev" />
          </div>
        ) : showLinkPanel ? (
          <div className="rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
            <LinkFolderPanel siteId="dev" kind="plugin" onInstalled={() => {}} />
          </div>
        ) : showRepoPanel ? (
          <div className="rounded-lg border border-rex-border bg-rex-surface-1 py-2">
            <RepoPanel siteId="dev" kind="plugin" asset={ASSET} />
          </div>
        ) : (
          <div className="relative rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
            <GitAddPanel siteId="dev" kind="plugin" onInstalled={() => {}} />
          </div>
        )}
      </div>
    </div>
  );
}
