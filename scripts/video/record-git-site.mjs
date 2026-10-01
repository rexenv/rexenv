// Tier 2 · "A site from Git": New site → Laravel → From Git → Fetch → branch →
// Create; the job clones, wires .env, runs composer, artisan and the asset build.
import { openStage } from "./lib.mjs";

const narration = {
  intro: {
    text: "Start a site straight from a Git repository — rexenv clones it and gets it running.",
    say: "Start a site straight from a Git repository. Rex env clones it, and gets it running.",
  },
  type: { text: "Click New site, choose Laravel — WordPress and plain PHP work too — and pick From Git." },
  url: {
    text: "Paste the repository URL — or just owner/repo for GitHub. Fetch checks the URL and your access before anything is created.",
    say: "Paste the repository URL, or just owner slash repo for GitHub. Fetch checks the URL and your access, before anything is created.",
  },
  ref: {
    text: "Pick a branch or tag. The site's name comes from the repository.",
  },
  runs: {
    text: "rexenv tells you what it will run: composer install, the app key, the migrations, and the front-end build.",
    say: "Rex env tells you what it will run: composer install, the app key, the migrations, and the front-end build.",
  },
  job: {
    text: "Create site. It clones the repository, creates the database, writes the .env, and runs each step — the log is live.",
    say: "Create site. It clones the repository, creates the database, writes the dot env file, and runs each step. The log is live.",
  },
  done: { text: "And the Laravel app is live at storefront.rex, on its own database.", say: "And the Laravel app is live at storefront dot rex, on its own database." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "git-site" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("A site from Git", "Clone, install, migrate and build — one dialog");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Type + source ────────────────────────────────────────────────────────
await s.say("type");
await s.caption("New site → <b>Laravel</b> → <b>From Git</b>");
await s.click(app.getByRole("button", { name: "New site" }), { ms: 1000 });
const dialog = app.locator("div.w-\\[516px\\]");
await s.click(dialog.getByRole("button", { name: /^Laravel/ }), { fx: 0.3 });
await s.wait(300);
await s.click(dialog.getByRole("button", { name: "Continue" }));
await s.wait(300);
await s.click(dialog.getByRole("button", { name: "From Git" }), { ms: 700 });
await s.voiceDone(200);

// ── URL + Fetch ──────────────────────────────────────────────────────────
await s.say("url");
await s.caption("Paste the URL → <b>Fetch</b>");
const url = dialog.getByPlaceholder("https://github.com/you/your-app");
await s.camera(await s.containerOf(url, 460, 16), 0.8);
await s.click(url, { ms: 800 });
await s.type("https://github.com/acme/storefront", 45);
await s.click(dialog.getByRole("button", { name: "Fetch" }), { ms: 600 });
await dialog.getByText("Branch or tag").waitFor();
await s.voiceDone(200);

// ── Ref + name ───────────────────────────────────────────────────────────
await s.say("ref");
await s.caption("Branch <code>main</code> · name <code>storefront</code>");
await s.moveTo(dialog.getByLabel("Branch or tag to check out"), { ms: 800 });
await s.wait(600);
await s.camera(null);
await s.moveTo(dialog.getByPlaceholder("my-site"), { ms: 800 });
await s.voiceDone(200);

// ── What runs ────────────────────────────────────────────────────────────
await s.say("runs");
await s.caption("Said before it runs: composer · artisan · npm");
const disclosure = dialog.getByText(/Creating this site runs the repository's own code/);
await s.box(disclosure);
await s.camera(await s.containerOf(disclosure, 440, 10), 0.75);
await s.moveTo(disclosure, { ms: 800 });
await s.voiceDone(200);
await s.camera(null);

// ── The job ──────────────────────────────────────────────────────────────
await s.say("job");
await s.caption("Clone → database → .env → composer → migrate → build");
await s.click(dialog.getByRole("button", { name: "Create site" }), { ms: 800 });
const card = app.locator('[data-probe="provision-card"]');
await card.waitFor();
await s.wait(300);
await s.click(card.getByText("Show log"), { ms: 700 });
await s.wait(400);
await s.camera(await s.containerOf(card.getByText("Hide log"), 470, 6), 0.85);
await dialog.waitFor({ state: "detached", timeout: 60_000 });
await s.camera(null);
await s.voiceDone(100);

// ── Done ─────────────────────────────────────────────────────────────────
await app.getByText("storefront.rex", { exact: true }).waitFor();
await s.say("done");
await s.caption("<code>https://storefront.rex</code> — Laravel, migrated, built");
await s.spotlight(await s.containerOf(app.getByText("storefront.rex", { exact: true }), 700));
await s.moveTo(app.getByText("storefront.rex", { exact: true }), { ms: 900 });
await s.voiceDone(900);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("git-site")}`);
