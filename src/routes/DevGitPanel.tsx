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

export function DevGitPanel() {
  const [ready, setReady] = useState(false);
  const params = new URLSearchParams(window.location.search);
  const rehydrate = params.get("rehydrate") === "1";
  const showRepoPanel = params.get("panel") === "repo";
  const showLinkPanel = params.get("panel") === "link";
  const watchMode = params.get("watch"); // "1" running | "exited"
  const detached = params.get("detached") === "1";
  useEffect(() => {
    mockIPC(async (cmd) => {
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
          };
        case "repo_git_op":
          return OP_JOB;
        case "tail_log":
          return TAIL_LINES;
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
        {showLinkPanel ? (
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
