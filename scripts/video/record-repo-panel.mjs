// Tier 2 · "Plugins & themes from Git": WordPress → Plugins → From Git → Fetch
// → Add plugin → clone + detect → the offered dependency steps, one click each
// → Activate → the git chip → the Repository panel → Pull.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Building a plugin or theme in Git? Add it straight from the repository — and keep using Git from inside rexenv.", say: "Building a plugin or theme in Git? Add it straight from the repository, and keep using Git from inside rex env." },
  add: { text: "In the WordPress tab, pick From Git and paste the repository — owner/repo is enough for GitHub. Fetch, choose a branch, and Add plugin.", say: "In the WordPress tab, pick From Git and paste the repository. Owner slash repo is enough for GitHub. Fetch, choose a branch, and Add plugin." },
  clone: { text: "rexenv clones it into the site and looks for what it needs: Composer, and an npm, pnpm or Yarn build.", say: "Rex env clones it into the site, and looks for what it needs: Composer, and an npm, pnpm or Yarn build." },
  consent: { text: "Each of those steps runs only when you click it — they run the repository's own scripts, as you." },
  activate: { text: "Dependencies installed, assets built. Activate the plugin." },
  panel: { text: "It now carries a git badge. Click it for the repository panel: the branch, local changes, and how far behind you are." },
  pull: { text: "Two commits behind — Pull brings it up to date. It only fast-forwards; it never merges for you." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "repo-panel", path: "/sites/1/wordpress", ready: "text=Plugins" });
const { app } = s;

await s.title("Plugins & themes from Git", "Clone, build, activate — then pull, branch and stash in place");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Add from Git ─────────────────────────────────────────────────────────
await s.say("add");
await s.caption("Plugins → <b>From Git</b> → Fetch → <b>Add plugin</b>");
await s.click(app.getByRole("button", { name: "From Git" }).first(), { ms: 1000 });
const url = app.getByPlaceholder(/owner\/repo/).first();
const bar = await s.containerOf(url, 560, 10);
await s.camera({ ...bar, h: bar.h + 40 }, 0.8);
await s.click(url, { ms: 700 });
await s.type("acme/booking-widget", 60);
await s.click(app.getByRole("button", { name: "Fetch" }).first(), { ms: 600 });
const addBtn = app.getByRole("button", { name: /^Add plugin/ });
await addBtn.waitFor();
await s.wait(400);
await s.wait(500);
await s.click(addBtn, { ms: 700 });
await s.voiceDone(200);
await s.camera(null);

// ── Clone + detect ───────────────────────────────────────────────────────
await s.say("clone");
await s.caption("Clone → detect: composer · pnpm install · pnpm run build");
const steps = app.getByText("Detect dependencies", { exact: true });
await steps.waitFor();
await app.getByRole("button", { name: "composer install", exact: true }).waitFor({ timeout: 15_000 });
await s.camera(await s.containerOf(steps, 560, 12), 0.8);
await s.voiceDone(200);

await s.say("consent");
await s.caption("One click per step — the repo's own scripts, run as you");
for (const name of ["composer install", "pnpm install", "pnpm run build"]) {
  const b = app.getByRole("button", { name, exact: true });
  await s.click(b, { ms: 700 });
  await app.getByRole("button", { name: `✓ ${name}`, exact: true }).waitFor({ timeout: 15_000 });
}
await s.voiceDone(200);

await s.say("activate");
await s.caption("<b>Activate plugin</b>");
await s.click(app.getByRole("button", { name: "Activate plugin" }), { ms: 800 });
await app.getByText("Plugin activated").waitFor({ timeout: 10_000 });
await s.voiceDone(200);
await s.camera(null);

// ── The repository panel ─────────────────────────────────────────────────
await s.say("panel");
await s.caption("The <code>git</code> badge → the repository panel");
const chip = app.getByTitle(/Git checkout — click for repo state/).first();
await s.box(chip);
await s.click(chip, { ms: 900 });
const pull = app.getByRole("button", { name: "Pull", exact: true });
await pull.waitFor({ timeout: 10_000 });
await s.box(pull);
await s.camera(await s.containerOf(pull, 600, 12), 0.8);
await s.moveTo(app.getByText(/↓2|vs origin\/main/).first(), { ms: 800 });
await s.voiceDone(200);

await s.say("pull");
await s.caption("<b>Pull</b> — fast-forward only, never a merge");
await s.click(pull, { ms: 800 });
await app.getByText("Pull — booking-widget finished").waitFor({ timeout: 15_000 });
await s.voiceDone(900);
await s.camera(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("repo-panel")}`);
