// Tier 1 · "Multiple PHP versions": switch a site to PHP 7.4 (confirm, the
// EOL note), then Settings → PHP versions: raise 8.3's limits, apply a patch.
import { openStage } from "./lib.mjs";

const narration = {
  intro: {
    text: "Every site can run the PHP version it needs — from 7.4 to 8.5, side by side.",
    say: "Every site can run the PHP version it needs. From seven point four to eight point five, side by side.",
  },
  open: { text: "Say an older client project needs PHP 7.4. Open the site — its PHP version is right here." },
  confirm: {
    text: "Pick 7.4 and confirm. rexenv switches it in place — no rebuild, just a quick reload.",
    say: "Pick seven point four, and confirm. Rex env switches it in place. No rebuild, just a quick reload.",
  },
  eol: {
    text: "PHP 7.4 no longer gets security fixes, and rexenv says so — fine for a legacy project, not for a new one.",
    say: "PHP seven point four no longer gets security fixes, and rex env says so. Fine for a legacy project, not for a new one.",
  },
  pools: {
    text: "Each version runs one pool, shared by every site on it — so switching back is just as quick.",
  },
  settings: { text: "PHP settings live in Settings → Services, one set per version." },
  edit: {
    text: "Give PHP 8.3 more memory and a longer time limit — then save, and its pool restarts.",
    say: "Give PHP eight point three more memory and a longer time limit. Then save, and its pool restarts.",
  },
  update: {
    text: "A new patch is out? One click updates it — and if the pool doesn't come back, rexenv puts the old one back.",
    say: "A new patch is out? One click updates it. And if the pool doesn't come back, rex env puts the old one back.",
  },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "php" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("Multiple PHP versions", "Every site on the PHP it needs — 7.4 to 8.5, side by side");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── A site's PHP ─────────────────────────────────────────────────────────
await s.say("open");
await s.caption("Open the site");
await s.click(app.getByText("Agency Blog", { exact: true }), { ms: 1000 });
const picker = app.locator("select").first();
await picker.waitFor();
await s.wait(400);
await s.camera(await s.containerOf(app.getByText("PHP version", { exact: true }), 700, 12), 0.75);
await s.moveTo(picker, { ms: 800 });
await s.voiceDone(200);

await s.say("confirm");
await s.caption("Pick <b>7.4</b> → <b>Switch</b>");
await s.stage(() => window.stage.press());
await picker.selectOption("7.4");
await s.wait(700);
await s.camera(null);
const sw = app.getByRole("button", { name: "Switch", exact: true });
await sw.waitFor();
await s.click(sw, { ms: 900 });
await app.getByText(/stopped receiving upstream security fixes/).waitFor({ timeout: 10_000 });
await s.voiceDone(200);

await s.say("eol");
await s.caption("End-of-life PHP, said plainly");
const eol = app.getByText(/stopped receiving upstream security fixes/);
await s.camera(await s.containerOf(eol, 700, 12), 0.8);
await s.moveTo(eol, { ms: 800 });
await s.voiceDone(200);

await s.say("pools");
await s.caption("One pool per version, shared by its sites");
await s.moveTo(picker, { ms: 800 });
await s.voiceDone(200);
await s.camera(null);

// ── Settings → PHP versions ──────────────────────────────────────────────
await s.say("settings");
await s.caption("<b>Settings → Services</b> → PHP versions");
await s.click(app.getByRole("link", { name: /^Settings/ }), { ms: 1000 });
await s.click(app.getByRole("button", { name: "Services", exact: true }), { ms: 800 });
const gear = app.getByRole("button", { name: /PHP 8\.3 settings/ });
await gear.waitFor();
await s.moveTo(app.getByText("PHP versions", { exact: true }), { ms: 700 });
await s.camera(await s.containerOf(app.getByText("PHP 8.3", { exact: true }), 380, 8), 0.7);
await s.voiceDone(200);

await s.say("edit");
await s.caption("<code>memory_limit</code> 2G · <code>max_execution_time</code> 300");
await s.click(gear, { ms: 800 });
await s.camera(null);
const mem = app.getByPlaceholder("1G", { exact: true });
await mem.waitFor();
await s.box(app.getByRole("button", { name: "Save & restart pool" }));
await s.camera(await s.containerOf(mem, 380, 14), 0.75);
await s.click(mem, { ms: 800 });
await s.type("2G", 120);
await s.click(app.getByPlaceholder("60", { exact: true }), { ms: 700 });
await s.type("300", 120);
await s.click(app.getByRole("button", { name: "Save & restart pool" }), { ms: 800 });
await app.getByText("PHP 8.3 settings applied — pool restarted").waitFor({ timeout: 10_000 });
await s.voiceDone(200);
await s.camera(null);

// ── Patch update ─────────────────────────────────────────────────────────
await s.say("update");
await s.caption("One-click patch updates — reverted if they don't come up");
const upd = app.getByRole("button", { name: "8.3.33" });
await s.box(upd);
await s.camera(await s.containerOf(upd, 380, 10), 0.6);
await s.click(upd, { ms: 900 });
await app.getByText("PHP 8.3 is now on 8.3.33").waitFor({ timeout: 15_000 });
await s.voiceDone(900);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("php")}`);
