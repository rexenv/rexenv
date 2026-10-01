/** A plugin added from Git, then worked on through the Repository panel.
 *  `repo_add` starts a job whose clone + detect steps run on their own; the
 *  dependency steps detection finds are OFFERED and run one click each
 *  (`commands/repo.rs`: "explicit clicks — never auto-run"). State goes out on
 *  `repo-job://state/<id>`, log lines on `repo-job://output/<id>`. The panel's
 *  Pull is a `repo_git_op` job of its own. */
import type { SceneCtx } from "../demo-backend";
import type { GitAsset, RepoAssetStatus, RepoJobState, RepoProbeResult, RepoStepState } from "@/types";
import { wpHandlers, wpState } from "../wp-fixtures";

type Args = Record<string, unknown> | undefined;

const URL = "https://github.com/acme/booking-widget";
const DIR = "/Users/demo/Sites/agency-blog/wp-content/plugins/booking-widget";

const PROBE: RepoProbeResult = {
  url: URL,
  host: "github.com",
  dirName: "booking-widget",
  refCandidate: null,
  defaultBranch: "main",
  branches: ["main", "develop", "feature/calendar-sync"],
  tags: ["v1.3.0", "v1.4.0"],
};

const STEP_LINES: Record<string, string[]> = {
  clone: [
    `$ git clone --progress --recurse-submodules -- ${URL} ${DIR}`,
    `Cloning into '${DIR}'...`,
    "remote: Enumerating objects: 214, done.",
    "remote: Counting objects: 100% (214/214), done.",
    "Receiving objects: 100% (214/214), 96.10 KiB | 1.18 MiB/s, done.",
    "Resolving deltas: 100% (88/88), done.",
  ],
  detect: ["✓ plugin header: Booking Widget"],
  composer: [
    "$ composer install --no-interaction",
    "Installing dependencies from lock file (including require-dev)",
    "Package operations: 12 installs, 0 updates, 0 removals",
    "Generating autoload files",
  ],
  install: ["$ pnpm install", "Lockfile is up to date, resolution step is skipped", "Packages: +312", "Done in 4.1s"],
  build: [
    "$ pnpm run build",
    "> booking-widget@1.4.0 build",
    "> wp-scripts build",
    "webpack 5.99.9 compiled successfully in 2718 ms",
  ],
  pull: [
    "$ git pull --ff-only",
    "Updating 3f2c7e1..9b44d2e",
    "Fast-forward",
    " src/calendar.js | 42 ++++++++++++++++++++++++++-----",
    " booking-widget.php |  2 +-",
    " 2 files changed, 36 insertions(+), 8 deletions(-)",
  ],
};

export default function repoPanel(ctx: SceneCtx) {
  const st = wpState();
  const assets: GitAsset[] = [];
  let job: RepoJobState | null = null;
  // Answered BY ID: the panel's catch-up read (`repo_job_state`) must get the op
  // job it started, not the add job.
  const jobs = new Map<string, RepoJobState>();
  let status: RepoAssetStatus = {
    branch: "main",
    detached: false,
    detachedAt: null,
    unborn: false,
    upstream: "origin/main",
    updatable: null,
    ahead: 0,
    behind: 2,
    changed: 0,
    untracked: 0,
    remote: URL,
    lossWarning: null,
    logKey: null,
    linkTarget: null,
    hasDistignore: true,
  };

  const emitState = (j: RepoJobState) => void ctx.emit(`repo-job://state/${j.id}`, structuredClone(j));
  const line = (j: RepoJobState, l: string) => void ctx.emit(`repo-job://output/${j.id}`, l);
  const runStep = async (j: RepoJobState, key: string, gap = 260) => {
    const s = j.steps.find((x) => x.key === key)!;
    s.status = "running";
    emitState(j);
    for (const l of STEP_LINES[key] ?? []) {
      await ctx.sleep(gap);
      line(j, l);
    }
    await ctx.sleep(gap);
    s.status = "ok";
    j.finishedOk = j.steps.length >= 2 && j.steps.every((x) => x.status === "ok");
    emitState(j);
  };
  const step = (key: RepoStepState["key"], label: string): RepoStepState => ({ key, label, status: "pending", error: null });

  return {
    ...wpHandlers(ctx, st),
    repo_probe: async () => {
      await ctx.sleep(900);
      return PROBE;
    },
    repo_tools: () => [
      { name: "git", ok: true, version: "2.50.1", path: "/usr/bin/git", error: null },
      { name: "node", ok: true, version: "22.23.1", path: "/Users/demo/.nvm/versions/node/v22.23.1/bin/node", error: null },
    ],
    repo_add: (a: Args) => {
      const j: RepoJobState = {
        id: "repo-job-1",
        siteId: String(a?.siteId),
        kind: "plugin",
        dirName: String(a?.dirName ?? "booking-widget"),
        url: URL,
        gitRef: (a?.gitRef as string | null) ?? "main",
        op: "add",
        logKey: "repo-agency-blog.rex-booking-widget.log",
        steps: [step("clone", "Clone repository"), step("detect", "Detect dependencies")],
        inspection: null,
        nodeWarning: null,
        finishedOk: false,
        archive: null,
      };
      job = j;
      jobs.set(j.id, j);
      setTimeout(async () => {
        await runStep(j, "clone", 220);
        await runStep(j, "detect", 300);
        j.inspection = { composer: true, node: { manager: "pnpm", pinnedBy: "pnpm-lock.yaml", hasBuild: true }, wp: { kind: "plugin", name: "Booking Widget" }, nodeWant: null };
        j.steps.push(step("composer", "composer install"), step("install", "pnpm install"), step("build", "pnpm run build"));
        j.finishedOk = false;
        emitState(j);
        st.plugins.push({ name: "booking-widget", status: "inactive", version: "1.4.0", update: "none", updateVersion: "", title: "Booking Widget", file: "booking-widget/booking-widget.php" });
        assets.push({ kind: "plugin", dirName: "booking-widget", url: URL, gitRef: "main", source: "cloned" });
      }, 350);
      return structuredClone(j);
    },
    wp_plugin_activate: (a: Args) => {
      for (const n of a?.names as string[]) st.plugins.find((x) => x.name === n)!.status = "active";
      return null;
    },
    repo_run_step: (a: Args) => {
      if (job) void runStep(job, String(a?.stepKey));
      return null;
    },
    repo_job_state: (a: Args) => jobs.get(String(a?.jobId)) ?? null,
    repo_site_jobs: () => (job ? [structuredClone(job)] : []),
    repo_assets: () => assets,
    repo_asset_status: () => status,
    repo_branches: () => ({ current: status.branch, local: ["main"], remote: ["origin/main", "origin/develop", "origin/feature/calendar-sync"], tags: ["v1.4.0", "v1.3.0"] }),
    repo_stashes: () => [],
    repo_pull_refs: () => [],
    repo_scripts: () => ({
      manager: "pnpm",
      scripts: [
        { name: "build", command: "wp-scripts build", watchy: false },
        { name: "start", command: "wp-scripts start", watchy: true },
      ],
    }),
    repo_watches: () => [],
    repo_git_op: (a: Args) => {
      const op = String(a?.op);
      const j: RepoJobState = {
        id: `repo-op-${op}`,
        siteId: String(a?.siteId),
        kind: "plugin",
        dirName: "booking-widget",
        url: URL,
        gitRef: "main",
        op,
        logKey: "repo-agency-blog.rex-booking-widget.log",
        steps: [step(op as RepoStepState["key"], op === "pull" ? "git pull --ff-only" : `git ${op}`)],
        inspection: null,
        nodeWarning: null,
        finishedOk: false,
        archive: null,
      };
      jobs.set(j.id, j);
      setTimeout(async () => {
        await runStep(j, op, 300);
        if (op === "pull") status = { ...status, behind: 0 };
        j.finishedOk = true;
        emitState(j);
      }, 300);
      return structuredClone(j);
    },
  };
}
