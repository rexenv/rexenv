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

export function DevGitPanel() {
  const [ready, setReady] = useState(false);
  const rehydrate = new URLSearchParams(window.location.search).get("rehydrate") === "1";
  useEffect(() => {
    mockIPC(async (cmd) => {
      switch (cmd) {
        case "repo_site_jobs":
          return rehydrate ? [RUNNING_JOB] : [];
        case "tail_log":
          return rehydrate ? TAIL_LINES : [];
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
        <div className="relative rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
          <GitAddPanel siteId="dev" kind="plugin" onInstalled={() => {}} />
        </div>
      </div>
    </div>
  );
}
