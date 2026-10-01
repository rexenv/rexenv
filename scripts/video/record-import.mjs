// Tier 1 · "Import from Local, Valet & Herd": the Sites banner → the scan →
// taking over Valet's .test → importing three sites with their databases →
// the sites in rexenv's list.
import { openStage } from "./lib.mjs";

const narration = {
  intro: {
    text: "Already using Local, Valet or Herd? Bring your sites over to rexenv — without moving a single file.",
    say: "Already using Local, Valet or Herd? Bring your sites over to rex env, without moving a single file.",
  },
  banner: {
    text: "rexenv finds them on its own. Click Review import.",
    say: "Rex env finds them on its own. Click Review import.",
  },
  scan: {
    text: "The scan only reads their setup — nothing of theirs is written, started or stopped.",
  },
  consent: {
    text: "Valet answers .test on this Mac. Let rexenv answer it instead — Valet's file is backed up, and you can hand it back in one click.",
    say: "Valet answers dot test on this Mac. Let rex env answer it instead. Valet's file is backed up first, and you can hand it back in one click.",
  },
  password: { text: "macOS asks for your password once, for the resolver file.", say: "Mac OS asks for your password once, for the resolver file." },
  rows: {
    text: "Local's sites move from .local to .rex — macOS keeps .local for Bonjour, printers and AirDrop.",
    say: "Local's sites move from dot local to dot rex. Mac OS keeps dot local for Bonjour, printers and AirDrop.",
  },
  select: {
    text: "Select them all, and keep 'also copy databases' — the old database is only read, never changed.",
    say: "Select them all, and keep also copy databases. The old database is only read, never changed.",
  },
  progress: {
    text: "rexenv links each folder where it already lives, copies its database, and connects the Local sites to their copies.",
    say: "Rex env links each folder where it already lives, copies its database, and connects the Local sites to their copies.",
  },
  done: { text: "Three sites imported, three databases copied." },
  sites: {
    text: "They're in rexenv now — and your old setup is untouched, so you can go back any time.",
    say: "They're in rex env now. And your old setup is untouched, so you can go back any time.",
  },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "import" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("Import from Local, Valet & Herd", "Your sites, where they already live — nothing moved, nothing copied");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Banner ───────────────────────────────────────────────────────────────
const review = app.getByRole("button", { name: "Review import" });
await review.waitFor();
await s.say("banner");
await s.caption("3 sites found in Valet, Herd or Local");
const banner = await s.union([app.getByText(/sites? found in Valet, Herd or Local/), review], 14);
await s.spotlight(banner);
await s.moveTo(review, { ms: 1000 });
await s.voiceDone(200);
await s.spotlight(null);
await s.click(review);

// ── Scan ─────────────────────────────────────────────────────────────────
await app.getByText("Import from Valet, Herd or Local").waitFor();
await s.say("scan");
await s.caption("The scan is read-only");
const sources = await s.union([app.getByText("/Users/demo/.config/valet"), app.getByText(/Local's database is never written/)], 20);
await s.camera(sources, 0.8);
await s.voiceDone(200);

// ── Take over .test ──────────────────────────────────────────────────────
const agree = app.getByRole("checkbox", { name: /Let rexenv answer/ });
const consent = await s.union([app.getByText(".test is managed by Valet or Herd"), app.getByRole("button", { name: "Take over .test" })], 18);
await s.say("consent");
await s.caption("Hand <code>.test</code> to rexenv — backed up, and reversible");
await s.camera(consent, 0.7);
await s.click(agree, { ms: 900 });
await s.wait(500);
await s.click(app.getByRole("button", { name: "Take over .test" }), { ms: 800 });
await s.say("password", 100);
await s.caption("macOS asks for your password once");
await app.getByText(".test is managed by Valet or Herd").waitFor({ state: "detached", timeout: 15_000 });
await s.voiceDone(200);
await s.camera(null);

// ── Rows ─────────────────────────────────────────────────────────────────
const header = app.locator('div:has(> span:has-text("Select sites to import")) > input[type=checkbox]');
const list = await s.union([app.getByText("Select sites to import"), app.getByText("portfolio.rex", { exact: true })], 18);
await s.say("rows");
await s.caption("<code>bakery.local</code> → <code>bakery.rex</code>");
await s.camera(list, 0.8);
await s.moveTo(app.getByText("was bakery.local"), { ms: 900 });
await s.voiceDone(200);

await s.say("select");
await s.caption("Select all · keep <b>also copy databases</b>");
await s.click(header, { ms: 800 });
await s.wait(400);
await s.camera(null);
await s.moveTo(app.getByText("also copy databases"), { ms: 900 });
await s.voiceDone(200);
await s.click(app.getByRole("button", { name: /^Import 3$/ }), { ms: 800 });

// ── Progress ─────────────────────────────────────────────────────────────
await s.say("progress", 100);
await s.caption("Linking folders, copying databases, connecting Local sites");
await app.getByText("Cancel after current").waitFor();
await s.wait(300);
await s.camera(await s.containerOf(app.getByText("Cancel after current"), 500, 16), 0.7);
await app.getByText("Cancel after current").waitFor({ state: "detached", timeout: 60_000 });
await s.camera(null);
await s.voiceDone(100);

// ── Done ─────────────────────────────────────────────────────────────────
await s.say("done");
await s.caption("3 imported · 3 databases copied · 2 connected");
const rowsArea = await s.union([app.getByText("bakery.rex", { exact: true }).first(), app.getByText("portfolio.rex", { exact: true }).first()], 18);
await s.spotlight(rowsArea);
await s.voiceDone(500);
await s.spotlight(null);

// ── Sites ────────────────────────────────────────────────────────────────
await s.click(app.getByRole("link", { name: /^Sites/ }), { ms: 1000 });
await app.getByText("crm.test", { exact: true }).waitFor();
await s.say("sites");
await s.caption("Your imported sites, served by rexenv");
await s.wait(400);
const rowsOf = [];
for (const d of ["bakery.rex", "crm.test", "portfolio.rex"]) rowsOf.push(await s.containerOf(app.getByText(d, { exact: true }), 700));
await s.moveTo(app.getByText("crm.test", { exact: true }), { ms: 900 });
await s.spotlight(rowsOf);
await s.voiceDone(900);
await s.caption(null);
await s.spotlight(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("import")}`);
