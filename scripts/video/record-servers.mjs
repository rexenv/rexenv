// Tier 3 · "Web servers & Redis": switch a site to FrankenPHP (confirm, the
// embedded-PHP note), Apache for .htaccess, and Redis from Services.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Each site can choose its web server — Nginx, FrankenPHP or Apache — and Redis is one switch away." },
  switch: { text: "On a site's Overview, pick another web server and confirm. The site restarts briefly — nothing is rebuilt." },
  franken: { text: "FrankenPHP brings its own PHP, so the version is fixed by it. It runs behind rexenv's HTTPS edge, never in front.", say: "FrankenPHP brings its own PHP, so the version is fixed by it. It runs behind rex env's HTTPS edge, never in front." },
  apache: { text: "Choose Apache when a plugin or theme depends on .htaccess rules.", say: "Choose Apache when a plugin or theme depends on dot H T access rules." },
  redis: { text: "Need Redis? Turn it on in Services. It runs on port 16379, so it never clashes with one you already have." },
  cli: { text: "Then connect with redis-cli on that port." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "servers" });
const { app } = s;

await s.title("Web servers & Redis", "Nginx, FrankenPHP or Apache per site — and Redis on tap");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

await s.say("switch");
await s.caption("Site → Overview → <b>Web server</b>");
await s.click(app.getByText("Landing Page", { exact: true }), { ms: 1000 });
const ws = app.locator("select").nth(1);
await ws.waitFor();
await s.camera(await s.containerOf(app.getByText("Web server", { exact: true }), 700, 12), 0.72);
await s.moveTo(ws, { ms: 800 });
await s.stage(() => window.stage.press());
await ws.selectOption("frankenphp");
await s.camera(null);
await s.click(app.getByRole("button", { name: "Switch", exact: true }), { ms: 900 });
await app.getByText(/Fixed by FrankenPHP/).waitFor({ timeout: 10_000 });
await s.voiceDone(200);

await s.say("franken");
await s.caption("FrankenPHP — its own PHP 8.5, behind the HTTPS edge");
const fixed = app.getByText(/Fixed by FrankenPHP/);
await s.camera(await s.containerOf(fixed, 700, 12), 0.72);
await s.moveTo(fixed, { ms: 800 });
await s.voiceDone(200);

await s.say("apache");
await s.caption("<b>Apache (.htaccess)</b> — for .htaccess rules");
await s.moveTo(ws, { ms: 700 });
await s.voiceDone(200);
await s.camera(null);

await s.say("redis");
await s.caption("Services → <b>Redis</b> — 127.0.0.1:16379");
await s.click(app.getByRole("link", { name: /^Services/ }), { ms: 900 });
const tog = app.getByRole("switch", { name: "Start Redis" }).or(app.getByLabel("Start Redis"));
await s.box(tog.first());
await s.camera(await s.containerOf(tog.first(), 700, 10), 0.7);
await s.click(tog.first(), { ms: 800 });
await s.voiceDone(200);
await s.camera(null);

await s.say("cli");
await s.caption("<code>redis-cli -p 16379</code>");
await s.click(app.getByRole("link", { name: /^Databases/ }), { ms: 900 });
const cli = app.getByText("redis-cli -p 16379");
await cli.waitFor({ timeout: 10_000 });
await s.spotlight(await s.containerOf(cli, 700));
await s.moveTo(cli, { ms: 800 });
await s.voiceDone(900);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("servers")}`);
