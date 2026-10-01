// Tier 3 · "Laravel & plain PHP": a fresh Laravel app (installer, MySQL .env,
// migrations) and a Blank PHP site with its starter database.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Not just WordPress: rexenv makes fresh Laravel apps and plain PHP sites, database included.", say: "Not just WordPress. Rex env makes fresh Laravel apps and plain PHP sites, database included." },
  laravel: { text: "For Laravel, name it and click Create site. rexenv runs the Laravel installer, gives it a real MySQL database and wires the .env — mail included.", say: "For Laravel, name it and click Create site. Rex env runs the Laravel installer, gives it a real MySQL database, and wires the dot env file. Mail included." },
  migrate: { text: "Then it runs the migrations against that database, and the app is live." },
  php: { text: "A Blank PHP site gets a starter database by default: a seeded sample table, and a db.php with the connection ready.", say: "A Blank PHP site gets a starter database by default: a seeded sample table, and a db dot php with the connection ready." },
  page: { text: "Open it, and index.php greets you with rows from your own database — edit it and keep going.", say: "Open it, and index dot php greets you with rows from your own database. Edit it, and keep going." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "new-sites" });
const { app } = s;
const dialog = app.locator("div.w-\\[516px\\]");

async function create(kind, name, show) {
  await s.click(app.getByRole("button", { name: "New site" }), { ms: 900 });
  await s.click(dialog.getByRole("button", { name: kind }), { fx: 0.3, ms: 700 });
  await s.click(dialog.getByRole("button", { name: "Continue" }), { ms: 500 });
  await s.click(dialog.getByPlaceholder("my-site"), { ms: 600 });
  await s.type(name, 80);
  if (show) await show();
  await s.click(dialog.getByRole("button", { name: "Create site" }), { ms: 700 });
  const card = app.locator('[data-probe="provision-card"]');
  await card.waitFor();
  await s.click(card.getByText("Show log"), { ms: 500 });
  await s.camera(await s.containerOf(card.getByText("Hide log"), 470, 6), 0.85);
}

await s.title("Laravel & plain PHP", "Fresh apps with a real database, ready in one dialog");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Laravel ──────────────────────────────────────────────────────────────
await s.say("laravel");
await s.caption("New site → <b>Laravel</b> → Create site");
await create(/^Laravel/, "Invoices");
await s.voiceDone(100);
await s.say("migrate");
await s.caption("Installer → MySQL <code>.env</code> → migrations");
await dialog.waitFor({ state: "detached", timeout: 60_000 });
await s.camera(null);
await s.voiceDone(200);

// ── Blank PHP ────────────────────────────────────────────────────────────
await s.say("php");
await s.caption("New site → <b>Blank PHP</b> — starter database on by default");
await create(/Blank PHP/, "Scratchpad", async () => {
  const note = dialog.getByText(/seeds a sample table and writes/);
  await s.moveTo(note, { ms: 800 });
  await s.wait(900);
});
await dialog.waitFor({ state: "detached", timeout: 60_000 });
await s.camera(null);
await s.voiceDone(200);

await s.say("page");
await s.caption("<code>index.php</code> opens on your database's rows");
const row = app.getByText("scratchpad.rex", { exact: true });
await row.waitFor();
await s.spotlight(await s.containerOf(row, 700));
await s.moveTo(row, { ms: 900 });
await s.voiceDone(900);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("new-sites")}`);
