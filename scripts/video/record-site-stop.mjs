// Tier 3 · "Stop one site": Stop site on a site → "Stopped by you" → the
// Stopped filter → start it again. Nothing shared stops.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Don't need a site right now? Stop just that one — everything else keeps running." },
  stop: { text: "Open the site and click Stop site. It stops serving, but rexenv's shared services — and your other sites — are untouched." },
  page: { text: "Visitors get a clear “this site is stopped” page instead of an error — and it stays stopped after a restart, until you start it.", say: "Visitors get a clear, this site is stopped page, instead of an error. And it stays stopped after a restart, until you start it." },
  list: { text: "In the Sites list it says “Stopped by you”, and the Stopped filter finds it." },
  start: { text: "Start site brings it straight back." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "site-stop" });
const { app } = s;

await s.title("Stop one site", "Pause a site without touching the rest");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

await s.say("stop");
await s.caption("Site → <b>Stop site</b>");
await s.click(app.getByText("Shop Staging", { exact: true }), { ms: 1000 });
const head = await s.containerOf(app.getByRole("button", { name: "Stop site" }), 800, 10);
await s.camera({ ...head, h: Math.min(head.h, 90) }, 0.7);
await s.click(app.getByRole("button", { name: "Stop site" }), { ms: 800 });
await app.getByText(/stopped — it answers "site stopped" now/).waitFor({ timeout: 8000 });
await s.spotlight(await s.box(app.getByText("Stopped by you").first()));
await s.voiceDone(200);
await s.spotlight(null);

await s.say("page");
await s.caption("A “site stopped” page — not an error");
await s.moveTo(app.getByRole("button", { name: "Start site" }), { ms: 900 });
await s.voiceDone(200);

await s.camera(null);
await s.say("list");
await s.caption("Sites → <b>Stopped</b>");
await s.click(app.getByRole("link", { name: /^Sites/ }), { ms: 800 });
await s.click(app.getByRole("button", { name: /^Stopped/ }).first(), { ms: 800 });
const row = app.getByText("shop-staging.rex", { exact: true });
await row.waitFor();
await s.spotlight(await s.containerOf(row, 700));
await s.voiceDone(200);
await s.spotlight(null);

await s.say("start");
await s.caption("<b>Start site</b> — serving again");
await s.click(app.getByText("Shop Staging", { exact: true }), { ms: 800 });
await s.click(app.getByRole("button", { name: "Start site" }), { ms: 800 });
await app.getByText("shop-staging.rex is serving again.").waitFor({ timeout: 8000 });
await s.spotlight(await s.box(app.getByText("Running", { exact: true }).first()));
await s.voiceDone(300);
await s.wait(1800);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("site-stop")}`);
