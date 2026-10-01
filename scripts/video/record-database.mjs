// Tier 1 · "Databases": the four engines and their ports → switch PostgreSQL
// to 17 → browse MySQL → a site's Database tab → export and import a dump.
import { openStage } from "./lib.mjs";

const narration = {
  intro: {
    text: "MySQL, MariaDB, PostgreSQL and Redis — built in, with a database browser included.",
  },
  engines: {
    text: "Open Databases. Each engine runs on its own port, so it never clashes with a database you already have installed.",
  },
  versions: {
    text: "Need an older version? Pick it here. Each version keeps its own data folder, so nothing is converted or lost.",
  },
  browse: {
    text: "Browse opens a database browser right inside rexenv — already signed in.",
    say: "Browse opens a database browser right inside rex env. Already signed in.",
  },
  site: {
    text: "Every site has a Database tab too, opening straight into its own database.",
  },
  export: {
    text: "For backups, the WordPress tab has Tools → Export database: a .sql file, straight to Downloads.",
    say: "For backups, the WordPress tab has Tools, then Export database. A dot S Q L file, straight to your Downloads folder.",
  },
  import: {
    text: "Import restores a dump. You type the site's domain to confirm, so it's never an accident.",
  },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "database" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("Databases", "MySQL, MariaDB, PostgreSQL & Redis — with a browser built in");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Engines ──────────────────────────────────────────────────────────────
await s.say("engines");
await s.caption("Own ports — <code>13306</code> · <code>13307</code> · <code>15432</code> · <code>16379</code>");
await s.click(app.getByRole("link", { name: /^Databases/ }), { ms: 1000 });
await app.getByText("127.0.0.1:15432", { exact: false }).waitFor();
await s.wait(300);
const top = await s.containerOf(app.getByText(/127\.0\.0\.1:13306/), 600);
const bottom = await s.containerOf(app.getByText(/127\.0\.0\.1:16379/), 600);
await s.camera({ x: top.x, y: top.y, w: top.w, h: bottom.y + bottom.h - top.y }, 0.8);
await s.moveTo(app.getByText(/127\.0\.0\.1:13306/), { ms: 900 });
await s.voiceDone(200);

// ── Versions ─────────────────────────────────────────────────────────────
await s.say("versions");
await s.caption("PostgreSQL 18 → 17 — each version keeps its own data");
const pgPick = app.getByTitle(/Switch the engine version/).nth(2);
await s.moveTo(pgPick, { ms: 800 });
await s.stage(() => window.stage.press());
await pgPick.selectOption("17.11.0");
await s.camera(null);
const sw = app.getByRole("button", { name: "Switch", exact: true });
await sw.waitFor();
await s.click(sw, { ms: 900 });
await s.voiceDone(200);

// ── Browse ───────────────────────────────────────────────────────────────
await s.say("browse");
await s.caption("<b>Browse</b> — Adminer, signed in");
await s.click(app.getByRole("button", { name: "Browse" }).first(), { ms: 900 });
await app.locator('iframe[title="Adminer"]').waitFor();
await s.wait(800);
await s.moveTo(app.locator('iframe[title="Adminer"]'), { ms: 900, fx: 0.55, fy: 0.55 });
await s.voiceDone(300);
await s.click(app.getByRole("button", { name: "Back" }), { ms: 700 });

// ── A site's Database tab ────────────────────────────────────────────────
await s.say("site");
await s.caption("Site → <b>Database</b> tab");
await s.click(app.getByRole("link", { name: /^Sites/ }), { ms: 800 });
await s.click(app.getByText("Agency Blog", { exact: true }), { ms: 800 });
await s.click(app.getByRole("button", { name: "Database", exact: true }).first(), { ms: 800 });
await app.locator('iframe[title="Adminer"]').waitFor();
await s.wait(700);
await s.moveTo(app.locator('iframe[title="Adminer"]'), { ms: 900, fx: 0.5, fy: 0.45 });
await s.voiceDone(300);

// ── Export ───────────────────────────────────────────────────────────────
await s.say("export");
await s.caption("WordPress → Tools → <b>Export database</b>");
await s.click(app.getByRole("button", { name: "WordPress", exact: true }), { ms: 800 });
await s.click(app.getByRole("button", { name: "Tools", exact: true }), { ms: 700 });
const exp = app.getByRole("button", { name: "Export database" });
await s.box(exp);
await s.camera(await s.containerOf(exp, 380, 12), 0.7);
await s.click(exp, { ms: 800 });
await app.getByText(/Database exported to/).waitFor({ timeout: 10_000 });
await s.voiceDone(300);

// ── Import ───────────────────────────────────────────────────────────────
await s.say("import");
await s.caption("<b>Import database…</b> — confirm by typing the domain");
await s.click(app.getByRole("button", { name: /Import database/ }), { ms: 800 });
await s.camera(null);
await app.getByText("Import a database dump?").waitFor();
await s.click(app.getByRole("button", { name: /Choose \.sql file/ }), { ms: 800 });
await s.wait(500);
const confirmBox = app.getByPlaceholder("agency-blog.rex", { exact: true });
await s.click(confirmBox, { ms: 700 });
await s.type("agency-blog.rex", 60);
await s.click(app.getByRole("button", { name: "Import & overwrite" }), { ms: 800 });
await app.getByText("Database imported.").waitFor({ timeout: 10_000 });
await s.voiceDone(900);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("database")}`);
